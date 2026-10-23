use std::collections::BTreeMap;
use std::path::Path;

use clap::Args;

use crate::changeset::{BumpLevel, Changeset, parser, reader};
use crate::errors::ChangesetterError;

#[derive(Args)]
pub struct CheckArgs {
    /// Base branch to compare against
    #[arg(long)]
    pub base: Option<String>,

    /// Output format (json for CI consumption)
    #[arg(long)]
    pub output: Option<String>,
}

pub fn run(args: &CheckArgs) -> anyhow::Result<()> {
    let repo_root = crate::git::find_repo_root()?;
    run_in(&repo_root, args)
}

pub fn run_in(repo_root: &Path, args: &CheckArgs) -> anyhow::Result<()> {
    let found = find_changesets(repo_root, args.base.as_deref())?;

    // The JSON goes out even when nothing was found, so CI can tell
    // "no changeset" (JSON, exit 1) apart from a crash (no JSON).
    if args.output.as_deref() == Some("json") {
        println!("{}", serde_json::to_string_pretty(&json_report(&found))?);
    }

    if found.is_empty() {
        eprintln!("Run `changesetter add` to create one.");
        anyhow::bail!(ChangesetterError::NoChangesets);
    }

    if args.output.as_deref() != Some("json") {
        print_summary(&found, args.base.as_deref());
    }
    Ok(())
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
            let content = crate::git::read_at_head(repo_root, &f.path)?;
            let changeset = parser::parse(&content, Some(f.path.clone()))?;
            Ok(Found {
                file: f.path,
                changeset: Some(changeset),
            })
        })
        .collect()
}

fn json_report(found: &[Found]) -> serde_json::Value {
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

    serde_json::json!({
        "changesets": changesets,
        "bumps": max_bumps(found),
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

    fn setup_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
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
            .args(["commit", "-q", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        dir
    }

    #[test]
    fn check_with_changesets_succeeds() {
        let dir = tempfile::tempdir().unwrap();
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
        json_report(&find_changesets(dir, base).unwrap())
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
        };
        assert!(run_in(dir.path(), &args).is_err());
    }

    #[test]
    fn json_without_base_reads_the_dir() {
        let dir = tempfile::tempdir().unwrap();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();
        std::fs::write(changeset_dir.join("x.md"), "---\nmylib: patch\n---\n\nX\n").unwrap();

        let report = find(dir.path(), None);
        assert_eq!(report["changesets"][0]["file"], ".changeset/x.md");
        assert_eq!(report["bumps"], serde_json::json!({"mylib": "patch"}));
    }

    #[test]
    fn json_reads_quoted_paths_and_renames_from_head() {
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
        std::fs::rename(
            changeset_dir.join("old.md"),
            changeset_dir.join("ø \"q\".md"),
        )
        .unwrap();
        std::fs::write(
            changeset_dir.join("fooREADME.md"),
            "---\nmylib: minor\n---\n\nF\n",
        )
        .unwrap();
        commit_all(dir.path(), "rename and add");
        // Uncommitted edits must not change what the check sees.
        std::fs::write(changeset_dir.join("fooREADME.md"), "garbage").unwrap();

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
                (".changeset/ø \"q\".md", false),
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
            serde_json::json!({"changesets": [], "bumps": {}})
        );
    }

    #[test]
    fn check_with_base_no_changesets_fails() {
        let dir = setup_repo();

        Command::new("git")
            .args(["checkout", "-b", "feature"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let args = CheckArgs {
            base: Some("main".to_string()),
            output: None,
        };
        let result = run_in(dir.path(), &args);
        assert!(result.is_err());
    }
}
