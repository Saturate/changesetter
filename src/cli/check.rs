use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use clap::Args;

use crate::changeset::{BumpLevel, Changeset, parser, reader};
use crate::errors::ChangesetterError;

#[derive(Args)]
pub struct CheckArgs {
    /// Base branch to compare against. Counts changesets added since the
    /// merge base, including staged and untracked ones. Passes when nothing
    /// at all changed since the base.
    #[arg(long)]
    pub base: Option<String>,

    /// Output format (json for CI consumption)
    #[arg(long)]
    pub output: Option<String>,

    /// Fail, rather than warn, when files under a package changed since
    /// --base and no changeset on the branch names that package
    #[arg(long, requires = "base")]
    pub strict: bool,
}

pub fn run(args: &CheckArgs) -> anyhow::Result<()> {
    let repo_root = crate::git::find_repo_root()?;
    run_in(&repo_root, args)
}

pub fn run_in(repo_root: &Path, args: &CheckArgs) -> anyhow::Result<()> {
    let found = find_changesets(repo_root, args.base.as_deref())?;
    let review = review(repo_root, &found, args.base.as_deref())?;

    // The JSON goes out even when nothing was found, so CI can tell
    // "no changeset" (JSON, exit 1) apart from a crash (no JSON).
    if args.output.as_deref() == Some("json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&json_report(&found, &review))?
        );
    }

    if found.is_empty() {
        // Nothing to describe, e.g. a gate run on main against itself.
        if let Some(base) = args.base.as_deref()
            && !crate::git::has_changes_since(repo_root, base)?
        {
            eprintln!("No changes since {base}; no changeset needed.");
            return Ok(());
        }
        eprintln!("Run `changesetter add` to create one.");
        anyhow::bail!(ChangesetterError::NoChangesets);
    }

    // A name `version` can't resolve passes a PR gate that doesn't check it,
    // then fails the version PR on main for everyone.
    if let Some(first) = review.unknown.first() {
        for u in &review.unknown {
            eprintln!("{}: unknown package \"{}\"", u.file, u.package);
        }
        eprintln!("Known packages: {}", review.known.join(", "));
        anyhow::bail!(ChangesetterError::UnknownPackage {
            name: first.package.clone(),
            path: first.file.clone().into(),
        });
    }

    if args.output.as_deref() != Some("json") {
        print_summary(&found, args.base.as_deref());
    }

    if let Some(base) = args.base.as_deref()
        && !review.uncovered.is_empty()
    {
        let err = ChangesetterError::UncoveredPackages {
            packages: review.uncovered.clone(),
            base: base.to_string(),
        };
        if args.strict {
            anyhow::bail!(err);
        }
        eprintln!("warning: {err}");
    }
    Ok(())
}

/// A package name in a changeset that no detected package has.
struct Unknown {
    file: String,
    package: String,
}

#[derive(Default)]
struct Review {
    /// Detected package names, for the error message.
    known: Vec<String>,
    unknown: Vec<Unknown>,
    /// Packages with files changed since the base that no changeset on the
    /// branch names. A cascade or a deliberate `none` changeset can cover a
    /// change, so this is a warning unless --strict.
    uncovered: Vec<String>,
}

/// Package detection only runs when there is a changeset to check against
/// it, so a branch without any still fails with "no changeset" first.
fn review(repo_root: &Path, found: &[Found], base: Option<&str>) -> anyhow::Result<Review> {
    let changesets: Vec<(&str, &Changeset)> = found
        .iter()
        .filter_map(|f| f.changeset.as_ref().map(|cs| (f.file.as_str(), cs)))
        .collect();
    if found.is_empty() {
        return Ok(Review::default());
    }
    // A branch that only deletes changesets is a version PR, which changes
    // manifests, lockfiles and changelogs of packages no changeset on it
    // names. Its other changes still need a changeset.
    let only_deletions = changesets.is_empty();

    let config = crate::config::Config::load(repo_root)?;
    let packages = crate::package::detector::detect_packages(repo_root, &config)?;
    let known: Vec<String> = packages.iter().map(|p| p.name.clone()).collect();

    // `default` is the package in a single-package repo; `version` resolves
    // it the same way and leaves it unresolved anywhere else.
    let resolve = |name: &str| -> Option<String> {
        if name == "default" && packages.len() == 1 {
            return Some(packages[0].name.clone());
        }
        known.iter().find(|k| *k == name).cloned()
    };

    let mut unknown = Vec::new();
    let mut named = BTreeSet::new();
    for (file, cs) in &changesets {
        for name in cs.packages.keys() {
            match resolve(name) {
                Some(resolved) => {
                    named.insert(resolved);
                }
                None => unknown.push(Unknown {
                    file: (*file).to_string(),
                    package: name.clone(),
                }),
            }
        }
    }

    let uncovered = match base {
        Some(base) => {
            let mut changed = crate::git::changed_paths_since(repo_root, base)?;
            if only_deletions {
                // A version PR bumps a package by changing its manifest, and
                // post_bump hooks may write more files inside it.
                named.extend(bumped_packages(repo_root, &packages, &changed));
                changed.retain(|path| !is_release_file(path, &config.changelog.file));
            }
            touched_packages(
                repo_root,
                &packages,
                &changed,
                config.changeset_dir_relative(),
            )
            .into_iter()
            .filter(|name| !named.contains(name))
            // A nameless single package is named `default` in a changeset.
            .map(|name| {
                if name.is_empty() {
                    "default".to_string()
                } else {
                    name
                }
            })
            .collect()
        }
        None => Vec::new(),
    };

    Ok(Review {
        known,
        unknown,
        uncovered,
    })
}

/// Packages whose own manifest is among the changed paths.
fn bumped_packages(
    repo_root: &Path,
    packages: &[crate::package::types::Package],
    changed: &[String],
) -> Vec<String> {
    packages
        .iter()
        .filter(|p| {
            let dir = crate::cli::status::repo_relative(repo_root, &p.path);
            changed.iter().any(|path| {
                let (parent, name) = path.rsplit_once('/').unwrap_or((".", path));
                parent == dir && is_manifest(name)
            })
        })
        .map(|p| p.name.clone())
        .collect()
}

fn is_manifest(name: &str) -> bool {
    name.ends_with(".csproj")
        || [
            "Cargo.toml",
            "package.json",
            "pyproject.toml",
            "Chart.yaml",
            "project.godot",
        ]
        .contains(&name)
}

/// Files `version` rewrites: manifests, lockfiles (through `post_bump`
/// hooks) and changelogs.
fn is_release_file(path: &str, changelog_file: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let changelog = changelog_file.rsplit('/').next().unwrap_or(changelog_file);
    name == changelog
        || name.ends_with(".csproj")
        || [
            "Cargo.toml",
            "Cargo.lock",
            "package.json",
            "package-lock.json",
            "pnpm-lock.yaml",
            "yarn.lock",
            "bun.lock",
            "bun.lockb",
            "pyproject.toml",
            "Chart.yaml",
            "project.godot",
        ]
        .contains(&name)
}

/// The packages owning at least one changed path. A path belongs to the
/// package with the deepest directory containing it, so a root package
/// doesn't claim files that sit in a nested one.
fn touched_packages(
    repo_root: &Path,
    packages: &[crate::package::types::Package],
    changed: &[String],
    changeset_dir: &str,
) -> BTreeSet<String> {
    // A workspace root like `{"private": true}` is detected as a nameless
    // package, but a monorepo changeset can't name it, so it owns nothing.
    let monorepo = packages.len() > 1;
    let dirs: Vec<(String, &str)> = packages
        .iter()
        .filter(|p| !(monorepo && p.name.is_empty()))
        .map(|p| {
            (
                crate::cli::status::repo_relative(repo_root, &p.path),
                p.name.as_str(),
            )
        })
        .collect();
    let changeset_prefix = format!("{changeset_dir}/");
    changed
        .iter()
        .filter(|path| !path.starts_with(&changeset_prefix))
        .filter_map(|path| {
            dirs.iter()
                .filter(|(dir, _)| {
                    dir == "." || path == dir || path.starts_with(&format!("{dir}/"))
                })
                .max_by_key(|(dir, _)| if dir == "." { 0 } else { dir.len() })
                .map(|(_, name)| (*name).to_string())
        })
        .collect()
}

/// A changeset this check counts, with its path relative to the repo root.
/// `None` is one the branch deleted: a version PR consumes changesets
/// without adding any, and still has to pass.
struct Found {
    file: String,
    changeset: Option<Changeset>,
}

fn find_changesets(repo_root: &Path, base: Option<&str>) -> anyhow::Result<Vec<Found>> {
    let config = crate::config::Config::load(repo_root)?;
    let changeset_rel = config.changeset_dir_relative();

    let Some(base) = base else {
        let changesets = reader::read_changesets(&config.changeset_dir(repo_root))?;
        return Ok(changesets
            .into_iter()
            .map(|changeset| Found {
                file: format!(
                    "{changeset_rel}/{}.md",
                    changeset.filename.as_deref().unwrap_or_default()
                ),
                changeset: Some(changeset),
            })
            .collect());
    };

    let files = crate::git::diff_changeset_files(repo_root, base, changeset_rel)?;
    files
        .into_iter()
        .filter(|f| {
            let name = f.path.rsplit('/').next().unwrap_or_default();
            name.ends_with(".md") && name != "README.md"
        })
        .map(|f| {
            if f.deleted {
                return Ok(Found {
                    file: f.path,
                    changeset: None,
                });
            }
            let content = crate::git::read_changeset_file(repo_root, &f.path)?;
            let changeset = parser::parse(&content, Some(f.path.clone()))?;
            Ok(Found {
                file: f.path,
                changeset: Some(changeset),
            })
        })
        .collect()
}

fn json_report(found: &[Found], review: &Review) -> serde_json::Value {
    let changesets: Vec<_> = found
        .iter()
        .map(|f| match &f.changeset {
            Some(cs) => {
                let packages: serde_json::Map<_, _> = cs
                    .packages
                    .iter()
                    .map(|(name, bump)| (name.clone(), bump.to_string().into()))
                    .collect();
                serde_json::json!({
                    "file": f.file,
                    "deleted": false,
                    "packages": packages,
                    "summary": cs.body.trim(),
                })
            }
            None => serde_json::json!({
                "file": f.file,
                "deleted": true,
                "packages": {},
                "summary": "",
            }),
        })
        .collect();

    let unknown: Vec<_> = review
        .unknown
        .iter()
        .map(|u| serde_json::json!({"file": u.file, "package": u.package}))
        .collect();

    serde_json::json!({
        "changesets": changesets,
        "bumps": max_bumps(found),
        "unknown_packages": unknown,
        "uncovered": review.uncovered,
    })
}

fn max_bumps(found: &[Found]) -> BTreeMap<&str, String> {
    let mut by_package: BTreeMap<&str, BumpLevel> = BTreeMap::new();
    for cs in found.iter().filter_map(|f| f.changeset.as_ref()) {
        for (pkg, bump) in &cs.packages {
            let max = by_package.entry(pkg.as_str()).or_insert(*bump);
            *max = (*max).max(*bump);
        }
    }
    by_package
        .into_iter()
        .map(|(pkg, bump)| (pkg, bump.to_string()))
        .collect()
}

fn print_summary(found: &[Found], base: Option<&str>) {
    match base {
        Some(base) => eprintln!("{} changeset(s) found vs {base}:", found.len()),
        None => eprintln!("{} changeset(s) found:", found.len()),
    }
    for f in found {
        let note = if f.changeset.is_none() {
            " (deleted)"
        } else {
            ""
        };
        eprintln!("  {}{note}", f.file);
    }
    for (pkg, bump) in max_bumps(found) {
        eprintln!("  {pkg}: {bump}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// The packages the test changesets name, so `check` can resolve them.
    fn write_packages(dir: &Path) {
        for name in ["mylib", "other"] {
            std::fs::create_dir_all(dir.join(name)).unwrap();
            std::fs::write(
                dir.join(name).join("package.json"),
                format!(r#"{{"name":"{name}","version":"1.0.0"}}"#),
            )
            .unwrap();
        }
    }

    fn setup_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        write_packages(dir.path());
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "commit.gpgsign", "false"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-q", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        dir
    }

    #[test]
    fn check_with_changesets_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        write_packages(dir.path());
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();
        std::fs::write(
            changeset_dir.join("cool-dogs-dance.md"),
            "---\nmylib: patch\n---\n\n#### Fix\n",
        )
        .unwrap();

        let args = CheckArgs {
            base: None,
            output: None,
            strict: false,
        };
        let result = run_in(dir.path(), &args);
        assert!(result.is_ok());
    }

    #[test]
    fn check_empty_fails() {
        let dir = tempfile::tempdir().unwrap();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();

        let args = CheckArgs {
            base: None,
            output: None,
            strict: false,
        };
        let result = run_in(dir.path(), &args);
        assert!(result.is_err());
    }

    #[test]
    fn check_missing_dir_fails() {
        let dir = tempfile::tempdir().unwrap();
        let args = CheckArgs {
            base: None,
            output: None,
            strict: false,
        };
        let result = run_in(dir.path(), &args);
        assert!(result.is_err());
    }

    #[test]
    fn check_with_base_finds_added_files() {
        let dir = setup_repo();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();

        Command::new("git")
            .args(["checkout", "-b", "feature"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::write(
            changeset_dir.join("test.md"),
            "---\nmylib: patch\n---\n\n#### Fix\n",
        )
        .unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-q", "-m", "add changeset"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let args = CheckArgs {
            base: Some("main".to_string()),
            output: None,
            strict: false,
        };
        let result = run_in(dir.path(), &args);
        assert!(result.is_ok());
    }

    fn commit_all(dir: &Path, msg: &str) {
        for args in [vec!["add", "-A"], vec!["commit", "-q", "-m", msg]] {
            let out = Command::new("git")
                .args(&args)
                .current_dir(dir)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }

    fn find(dir: &Path, base: Option<&str>) -> serde_json::Value {
        let found = find_changesets(dir, base).unwrap();
        json_report(&found, &review(dir, &found, base).unwrap())
    }

    #[test]
    fn json_lists_only_the_branch_changesets_with_bumps() {
        let dir = setup_repo();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();
        std::fs::write(
            changeset_dir.join("old.md"),
            "---\nmylib: major\n---\n\nOld\n",
        )
        .unwrap();
        commit_all(dir.path(), "old changeset");

        Command::new("git")
            .args(["checkout", "-q", "-b", "feature"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::create_dir(changeset_dir.join("nested")).unwrap();
        std::fs::write(
            changeset_dir.join("nested/a.md"),
            "---\nmylib: patch\nother: minor\n---\n\nFixed the **thing**.\n",
        )
        .unwrap();
        std::fs::write(
            changeset_dir.join("b.md"),
            "---\nmylib: minor\n---\n\nAdded it.\n",
        )
        .unwrap();
        std::fs::write(changeset_dir.join("README.md"), "docs").unwrap();
        commit_all(dir.path(), "add changesets");

        let report = find(dir.path(), Some("main"));
        assert_eq!(
            report,
            serde_json::json!({
                "changesets": [
                    {"file": ".changeset/b.md", "deleted": false, "packages": {"mylib": "minor"}, "summary": "Added it."},
                    {"file": ".changeset/nested/a.md", "deleted": false, "packages": {"mylib": "patch", "other": "minor"}, "summary": "Fixed the **thing**."},
                ],
                "bumps": {"mylib": "minor", "other": "minor"},
                "unknown_packages": [],
                "uncovered": [],
            })
        );
    }

    #[test]
    fn json_marks_deleted_changesets_and_still_counts_them() {
        let dir = setup_repo();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();
        std::fs::write(
            changeset_dir.join("old.md"),
            "---\nmylib: patch\n---\n\nOld\n",
        )
        .unwrap();
        commit_all(dir.path(), "old changeset");

        Command::new("git")
            .args(["checkout", "-q", "-b", "version-pr"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::remove_file(changeset_dir.join("old.md")).unwrap();
        commit_all(dir.path(), "consume changeset");

        let report = find(dir.path(), Some("main"));
        assert_eq!(report["changesets"][0]["file"], ".changeset/old.md");
        assert_eq!(report["changesets"][0]["deleted"], true);
        assert_eq!(report["bumps"], serde_json::json!({}));
        assert!(
            run_in(
                dir.path(),
                &CheckArgs {
                    base: Some("main".to_string()),
                    output: Some("json".to_string()),
                    strict: false,
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn invalid_changeset_on_branch_fails() {
        let dir = setup_repo();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();
        Command::new("git")
            .args(["checkout", "-q", "-b", "feature"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::write(changeset_dir.join("bad.md"), "---\nmylib: huge\n---\n\nX\n").unwrap();
        commit_all(dir.path(), "bad changeset");

        let args = CheckArgs {
            base: Some("main".to_string()),
            output: None,
            strict: false,
        };
        assert!(run_in(dir.path(), &args).is_err());
    }

    #[test]
    fn json_without_base_reads_the_dir() {
        let dir = tempfile::tempdir().unwrap();
        write_packages(dir.path());
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();
        std::fs::write(changeset_dir.join("x.md"), "---\nmylib: patch\n---\n\nX\n").unwrap();

        let report = find(dir.path(), None);
        assert_eq!(report["changesets"][0]["file"], ".changeset/x.md");
        assert_eq!(report["bumps"], serde_json::json!({"mylib": "patch"}));
    }

    #[test]
    fn json_reads_quoted_paths_and_renames() {
        let dir = setup_repo();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();
        std::fs::write(
            changeset_dir.join("old.md"),
            "---\nmylib: patch\n---\n\nOld\n",
        )
        .unwrap();
        commit_all(dir.path(), "old changeset");

        Command::new("git")
            .args(["checkout", "-q", "-b", "feature"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        // git quotes the non-ASCII name; `"` adds an escape where the file
        // system allows it (Windows doesn't).
        let renamed = if cfg!(windows) {
            "ø q.md"
        } else {
            "ø \"q\".md"
        };
        std::fs::rename(changeset_dir.join("old.md"), changeset_dir.join(renamed)).unwrap();
        std::fs::write(
            changeset_dir.join("fooREADME.md"),
            "---\nmylib: minor\n---\n\nF\n",
        )
        .unwrap();
        commit_all(dir.path(), "rename and add");

        let report = find(dir.path(), Some("main"));
        let files: Vec<_> = report["changesets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| (c["file"].as_str().unwrap(), c["deleted"].as_bool().unwrap()))
            .collect();
        assert_eq!(
            files,
            [
                (".changeset/fooREADME.md", false),
                (".changeset/old.md", true),
                (format!(".changeset/{renamed}").as_str(), false),
            ]
        );
        assert_eq!(report["bumps"], serde_json::json!({"mylib": "minor"}));
    }

    #[test]
    fn invalid_changeset_with_quoted_name_fails() {
        let dir = setup_repo();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();
        Command::new("git")
            .args(["checkout", "-q", "-b", "feature"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::write(changeset_dir.join("a.md"), "---\nmylib: patch\n---\n\nA\n").unwrap();
        std::fs::write(changeset_dir.join("ø.md"), "---\nmylib: huge\n---\n\nX\n").unwrap();
        commit_all(dir.path(), "changesets");

        assert!(find_changesets(dir.path(), Some("main")).is_err());
    }

    #[test]
    fn json_report_when_nothing_found_is_empty() {
        let dir = setup_repo();
        Command::new("git")
            .args(["checkout", "-q", "-b", "feature"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert_eq!(
            find(dir.path(), Some("main")),
            serde_json::json!({"changesets": [], "bumps": {}, "unknown_packages": [], "uncovered": []})
        );
    }

    fn check_main(dir: &Path) -> anyhow::Result<()> {
        run_in(
            dir,
            &CheckArgs {
                base: Some("main".to_string()),
                output: None,
                strict: false,
            },
        )
    }

    #[test]
    fn check_with_base_no_changes_passes() {
        let dir = setup_repo();
        Command::new("git")
            .args(["checkout", "-q", "-b", "feature"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(check_main(dir.path()).is_ok());
    }

    #[test]
    fn check_base_against_itself_passes() {
        let dir = setup_repo();
        assert!(check_main(dir.path()).is_ok());
    }

    #[test]
    fn check_with_base_committed_change_without_changeset_fails() {
        let dir = feature_branch();
        std::fs::write(
            dir.path().join("lib.rs"),
            "fn main() {}
",
        )
        .unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-q", "-m", "code"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let err = check_main(dir.path()).unwrap_err();
        assert!(err.to_string().contains("no changeset"), "{err}");
    }

    #[test]
    fn check_with_base_uncommitted_change_without_changeset_fails() {
        let dir = feature_branch();
        std::fs::write(
            dir.path().join("lib.rs"),
            "fn main() {}
",
        )
        .unwrap();
        assert!(check_main(dir.path()).is_err());
    }

    #[test]
    fn check_with_base_gitignored_change_only_passes() {
        let dir = setup_repo();
        std::fs::write(
            dir.path().join(".gitignore"),
            "target/
",
        )
        .unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-q", "-m", "ignore"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::create_dir_all(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("target/out"), "x").unwrap();
        assert!(check_main(dir.path()).is_ok());
    }

    fn feature_branch() -> tempfile::TempDir {
        let dir = setup_repo();
        std::fs::create_dir_all(dir.path().join(".changeset")).unwrap();
        Command::new("git")
            .args(["checkout", "-q", "-b", "feature"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        dir
    }

    fn write_changeset(dir: &Path, name: &str) {
        std::fs::write(
            dir.join(".changeset").join(name),
            "---
mylib: patch
---

#### Fix
",
        )
        .unwrap();
    }

    fn files(dir: &Path) -> Vec<String> {
        crate::git::diff_changeset_files(dir, "main", ".changeset")
            .unwrap()
            .into_iter()
            .map(|f| f.path)
            .collect()
    }

    #[test]
    fn check_with_base_counts_untracked_changeset() {
        let dir = feature_branch();
        write_changeset(dir.path(), "untracked.md");

        assert_eq!(files(dir.path()), [".changeset/untracked.md"]);
        let args = CheckArgs {
            base: Some("main".to_string()),
            output: None,
            strict: false,
        };
        assert!(run_in(dir.path(), &args).is_ok());
    }

    #[test]
    fn check_with_base_counts_staged_and_committed_once() {
        let dir = feature_branch();
        write_changeset(dir.path(), "committed.md");
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-q", "-m", "cs"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        write_changeset(dir.path(), "staged.md");
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();

        assert_eq!(
            files(dir.path()),
            [".changeset/committed.md", ".changeset/staged.md"]
        );
    }

    #[test]
    fn check_with_base_skips_ignored_and_deleted() {
        let dir = feature_branch();
        std::fs::write(
            dir.path().join(".gitignore"),
            ".changeset/ignored.md
",
        )
        .unwrap();
        write_changeset(dir.path(), "ignored.md");
        write_changeset(dir.path(), "gone.md");
        Command::new("git")
            .args(["add", ".changeset/gone.md"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-q", "-m", "cs"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::remove_file(dir.path().join(".changeset/gone.md")).unwrap();

        assert!(files(dir.path()).is_empty());
    }

    #[test]
    fn check_with_unknown_base_reports_unavailable() {
        let dir = feature_branch();
        let err = crate::git::diff_changeset_files(dir.path(), "nope", ".changeset").unwrap_err();
        assert!(err.to_string().contains("\"nope\""), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn check_with_base_refuses_symlinked_changeset() {
        let dir = feature_branch();
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.md");
        std::fs::write(&secret, "---\nmylib: patch\n---\n\nsecret\n").unwrap();
        std::os::unix::fs::symlink(&secret, dir.path().join(".changeset/link.md")).unwrap();
        commit_all(dir.path(), "symlink");

        let err = find_changesets(dir.path(), Some("main"))
            .err()
            .expect("symlink must be refused");
        assert!(err.to_string().contains("not a regular file"), "{err}");
    }

    fn check_json(dir: &Path, strict: bool) -> anyhow::Result<()> {
        run_in(
            dir,
            &CheckArgs {
                base: Some("main".to_string()),
                output: None,
                strict,
            },
        )
    }

    #[test]
    fn unknown_package_fails_and_json_names_file_and_package() {
        let dir = feature_branch();
        std::fs::write(
            dir.path().join(".changeset/typo.md"),
            "---\nmylib: patch\nmylbi: patch\n---\n\nFix\n",
        )
        .unwrap();
        commit_all(dir.path(), "typo");

        let report = find(dir.path(), Some("main"));
        assert_eq!(
            report["unknown_packages"],
            serde_json::json!([{"file": ".changeset/typo.md", "package": "mylbi"}])
        );
        let err = check_json(dir.path(), false).unwrap_err();
        match err.downcast_ref::<ChangesetterError>() {
            Some(ChangesetterError::UnknownPackage { name, path }) => {
                assert_eq!(name, "mylbi");
                assert_eq!(path, Path::new(".changeset/typo.md"));
            }
            other => panic!("expected UnknownPackage, got {other:?}"),
        }
    }

    #[test]
    fn unknown_package_fails_without_base() {
        let dir = tempfile::tempdir().unwrap();
        write_packages(dir.path());
        std::fs::create_dir(dir.path().join(".changeset")).unwrap();
        std::fs::write(
            dir.path().join(".changeset/x.md"),
            "---\nnope: patch\n---\n\nX\n",
        )
        .unwrap();
        let args = CheckArgs {
            base: None,
            output: Some("json".to_string()),
            strict: false,
        };
        assert!(run_in(dir.path(), &args).is_err());
    }

    #[test]
    fn default_and_ignored_names_resolve_like_version_does() {
        // Two packages: `default` names neither.
        let dir = tempfile::tempdir().unwrap();
        write_packages(dir.path());
        std::fs::create_dir(dir.path().join(".changeset")).unwrap();
        std::fs::write(
            dir.path().join(".changeset/d.md"),
            "---\ndefault: patch\n---\n\nX\n",
        )
        .unwrap();
        let report = find(dir.path(), None);
        assert_eq!(report["unknown_packages"][0]["package"], "default");

        // Ignoring one leaves a single package, which `default` then is,
        // and the ignored one is unknown.
        std::fs::write(
            dir.path().join("changesetter.toml"),
            "ignore = [\"other\"]\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".changeset/o.md"),
            "---\nother: patch\n---\n\nX\n",
        )
        .unwrap();
        let report = find(dir.path(), None);
        assert_eq!(
            report["unknown_packages"],
            serde_json::json!([{"file": ".changeset/o.md", "package": "other"}])
        );
    }

    #[test]
    fn deleted_changesets_are_not_validated() {
        let dir = setup_repo();
        std::fs::create_dir_all(dir.path().join(".changeset")).unwrap();
        std::fs::write(
            dir.path().join(".changeset/old.md"),
            "---\nremoved-since: patch\n---\n\nOld\n",
        )
        .unwrap();
        commit_all(dir.path(), "old");
        Command::new("git")
            .args(["checkout", "-q", "-b", "version-pr"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::remove_file(dir.path().join(".changeset/old.md")).unwrap();
        std::fs::write(
            dir.path().join("mylib/package.json"),
            r#"{"name":"mylib","version":"1.0.1"}"#,
        )
        .unwrap();
        commit_all(dir.path(), "version packages");

        let report = find(dir.path(), Some("main"));
        assert_eq!(report["unknown_packages"], serde_json::json!([]));
        assert_eq!(report["uncovered"], serde_json::json!([]));
        assert!(check_json(dir.path(), true).is_ok());
    }

    #[test]
    fn coverage_warns_about_changed_packages_no_changeset_names() {
        let dir = feature_branch();
        std::fs::write(dir.path().join("mylib/index.js"), "x").unwrap();
        std::fs::write(dir.path().join("other/index.js"), "y").unwrap();
        std::fs::write(dir.path().join("README.md"), "root file").unwrap();
        write_changeset(dir.path(), "fix.md");
        commit_all(dir.path(), "change both, name one");

        let report = find(dir.path(), Some("main"));
        assert_eq!(report["uncovered"], serde_json::json!(["other"]));
        assert!(check_json(dir.path(), false).is_ok());

        let err = check_json(dir.path(), true).unwrap_err();
        assert!(
            matches!(
                err.downcast_ref::<ChangesetterError>(),
                Some(ChangesetterError::UncoveredPackages { packages, .. }) if packages == &["other"]
            ),
            "{err}"
        );
        assert!(
            err.to_string().contains(
                "a package changed since main but no changeset on this branch names it: other"
            ),
            "{err}"
        );
    }

    #[test]
    fn coverage_counts_uncommitted_changes_and_none_bumps() {
        let dir = feature_branch();
        std::fs::write(dir.path().join("other/untracked.js"), "y").unwrap();
        std::fs::write(
            dir.path().join(".changeset/n.md"),
            "---\nmylib: patch\nother: none\n---\n\nX\n",
        )
        .unwrap();
        assert_eq!(
            find(dir.path(), Some("main"))["uncovered"],
            serde_json::json!([])
        );
        assert!(check_json(dir.path(), true).is_ok());
    }

    #[test]
    fn touched_packages_picks_the_deepest_package() {
        let root = Path::new("/repo");
        let pkg = |name: &str, path: &str| crate::package::types::Package {
            name: name.to_string(),
            path: root.join(path),
            package_type: crate::package::types::PackageType::Npm,
            version: crate::package::types::Version::new(1, 0, 0),
        };
        let packages = [
            pkg("root", ""),
            pkg("web", "apps/web"),
            pkg("webx", "apps/webx"),
        ];
        let changed = [
            "apps/web/src/a.ts".to_string(),
            "apps/webx-old/b.ts".to_string(),
            ".changeset/x.md".to_string(),
            "tools/x.sh".to_string(),
        ];
        let touched = touched_packages(root, &packages, &changed, ".changeset");
        assert_eq!(
            touched.into_iter().collect::<Vec<_>>(),
            vec!["root".to_string(), "web".to_string()]
        );
    }

    #[test]
    fn deleting_a_changeset_does_not_exempt_code_changes() {
        let dir = setup_repo();
        std::fs::create_dir_all(dir.path().join(".changeset")).unwrap();
        std::fs::write(
            dir.path().join(".changeset/old.md"),
            "---\nmylib: patch\n---\n\nOld\n",
        )
        .unwrap();
        commit_all(dir.path(), "old");
        Command::new("git")
            .args(["checkout", "-q", "-b", "feature"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::remove_file(dir.path().join(".changeset/old.md")).unwrap();
        std::fs::write(dir.path().join("other/index.js"), "code").unwrap();
        std::fs::write(
            dir.path().join("mylib/package.json"),
            r#"{"name":"mylib","version":"1.0.1"}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("mylib/CHANGELOG.md"), "## 1.0.1\n").unwrap();
        commit_all(dir.path(), "delete changeset, change code");

        assert_eq!(
            find(dir.path(), Some("main"))["uncovered"],
            serde_json::json!(["other"])
        );
        assert!(check_json(dir.path(), true).is_err());
    }

    #[test]
    fn nameless_single_package_is_reported_as_default() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "commit.gpgsign", "false"],
            vec!["config", "user.email", "t@t"],
            vec!["config", "user.name", "t"],
        ] {
            Command::new("git")
                .args(&args)
                .current_dir(p)
                .output()
                .unwrap();
        }
        std::fs::write(p.join("package.json"), r#"{"version":"1.0.0"}"#).unwrap();
        std::fs::create_dir(p.join(".changeset")).unwrap();
        commit_all(p, "init");
        Command::new("git")
            .args(["checkout", "-q", "-b", "f"])
            .current_dir(p)
            .output()
            .unwrap();
        std::fs::write(p.join("index.js"), "x").unwrap();
        std::fs::write(p.join(".changeset/e.md"), "---\n---\n\nNothing.\n").unwrap();
        assert_eq!(
            find(p, Some("main"))["uncovered"],
            serde_json::json!(["default"])
        );
    }

    #[test]
    fn version_pr_with_hook_written_files_passes_strict() {
        let dir = setup_repo();
        std::fs::create_dir_all(dir.path().join(".changeset")).unwrap();
        std::fs::write(
            dir.path().join(".changeset/old.md"),
            "---\nmylib: patch\n---\n\nOld\n",
        )
        .unwrap();
        commit_all(dir.path(), "old");
        Command::new("git")
            .args(["checkout", "-q", "-b", "changesetter/version-packages"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::remove_file(dir.path().join(".changeset/old.md")).unwrap();
        std::fs::write(
            dir.path().join("mylib/package.json"),
            r#"{"name":"mylib","version":"1.0.1"}"#,
        )
        .unwrap();
        // Written by a post_bump hook.
        std::fs::write(
            dir.path().join("mylib/version.ts"),
            "export const v = '1.0.1';",
        )
        .unwrap();
        commit_all(dir.path(), "version packages");

        assert_eq!(
            find(dir.path(), Some("main"))["uncovered"],
            serde_json::json!([])
        );
        assert!(check_json(dir.path(), true).is_ok());
    }
}
