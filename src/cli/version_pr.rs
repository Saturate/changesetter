use std::path::Path;

use clap::{Args, ValueEnum};

use crate::cli::tag::{self, TagArgs, TagResult};
use crate::config::Config;
use crate::forge::{self, Forge, OpenPr, ReleaseInfo, VERSION_BRANCH};
use crate::release::executor::{self, ExecuteOptions};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Provider {
    Azure,
    Github,
}

#[derive(Args)]
pub struct VersionPrArgs {
    /// Where the repository is hosted. Azure reads SYSTEM_ACCESSTOKEN,
    /// SYSTEM_COLLECTIONURI, SYSTEM_TEAMPROJECT and BUILD_REPOSITORY_ID;
    /// GitHub reads GITHUB_TOKEN (or GH_TOKEN) and GITHUB_REPOSITORY.
    #[arg(long, value_enum)]
    pub provider: Provider,

    /// Title for a new version PR
    #[arg(long, default_value = "Version Packages")]
    pub title: String,

    /// Branch the version PR merges into
    #[arg(long, default_value = "main")]
    pub base: String,

    /// The commit to check for a version PR merge. Defaults to the CI
    /// commit (BUILD_SOURCEVERSION or GITHUB_SHA), then HEAD.
    #[arg(long)]
    pub commit: Option<String>,

    /// Remote to push tags and the version branch to
    #[arg(long, default_value = "origin")]
    pub remote: String,

    /// GitHub only: don't create a GitHub Release for each new tag
    #[arg(long)]
    pub no_github_release: bool,

    /// GitHub only: create GitHub Releases as drafts
    #[arg(long)]
    pub draft: bool,

    /// Output format (json for CI consumption)
    #[arg(long)]
    pub output: Option<String>,
}

#[derive(Debug)]
pub struct Outcome {
    pub merged_pr: Option<u64>,
    pub tags: Vec<TagResult>,
    /// Per tag, the URL of the GitHub Release this run created.
    pub github_releases: Vec<(String, Option<String>)>,
    pub version_pr: Option<(OpenPr, bool)>,
}

impl Outcome {
    /// What this run released: a tag it created or a release page it
    /// created. A re-run, or the recovery check on a later push, releases
    /// nothing again, so publish steps gated on it don't run twice.
    fn released(&self) -> Vec<&TagResult> {
        self.tags
            .iter()
            .filter(|t| {
                t.created
                    || self
                        .github_releases
                        .iter()
                        .any(|(tag, url)| *tag == t.tag && url.is_some())
            })
            .collect()
    }

    pub fn to_json(&self) -> serde_json::Value {
        let released: Vec<_> = self
            .released()
            .into_iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "version": t.version,
                    "tag": t.tag,
                    "changelog": t.notes.clone().unwrap_or_default(),
                })
            })
            .collect();
        let github_releases: Vec<_> = self
            .github_releases
            .iter()
            .map(|(tag, url)| serde_json::json!({"tag": tag, "created": url.is_some(), "url": url}))
            .collect();
        let mut json = serde_json::json!({
            "merged_pr": self.merged_pr,
            "released": released,
            "github_releases": github_releases,
            "version_pr": self.version_pr.as_ref().map(|(pr, created)| serde_json::json!({
                "id": pr.id,
                "url": pr.url,
                "created": created,
            })),
        });
        json["tags"] = tag::to_json(&self.tags)["tags"].clone();
        json
    }
}

pub fn run(args: &VersionPrArgs) -> anyhow::Result<()> {
    let repo_root = crate::git::find_repo_root()?;
    let forge: Box<dyn Forge> = match args.provider {
        Provider::Azure => Box::new(forge::Azure::from_env(&args.base)?),
        Provider::Github => Box::new(forge::GitHub::from_env(&args.base)?),
    };
    let outcome = run_with(&repo_root, args, forge.as_ref())?;
    if args.output.as_deref() == Some("json") {
        println!("{}", serde_json::to_string_pretty(&outcome.to_json())?);
    }
    Ok(())
}

/// After a version PR merge, tags what it released; then, whenever
/// changesets are pending (also right after a release: some can land while
/// the version PR is open), force-pushes the version branch and creates or
/// updates the version PR.
pub fn run_with(
    repo_root: &Path,
    args: &VersionPrArgs,
    forge: &dyn Forge,
) -> anyhow::Result<Outcome> {
    let rev = args
        .commit
        .clone()
        .or_else(|| forge.ci_commit().filter(|c| !c.trim().is_empty()))
        .unwrap_or_else(|| "HEAD".to_string());
    let commit = tag::resolve_commit(repo_root, rev.trim())?;

    let mut outcome = Outcome {
        merged_pr: None,
        tags: Vec::new(),
        github_releases: Vec::new(),
        version_pr: None,
    };

    let merged = match forge.merged_version_pr(&commit)? {
        Some(pr) => {
            eprintln!("{commit} is the merge of version PR #{}, tagging", pr.id);
            Some(pr)
        }
        None => {
            eprintln!("{commit} is not a version PR merge");
            let pr = forge.recover_merged_version_pr(&commit)?;
            if let Some(pr) = &pr {
                eprintln!(
                    "Checking that version PR #{} ({}) was fully released",
                    pr.id, pr.commit
                );
            }
            pr
        }
    };
    if let Some(pr) = merged {
        outcome.merged_pr = Some(pr.id);
        ensure_commit(repo_root, &pr.commit, &args.remote)?;
        let since = merge_target(
            repo_root,
            &pr.commit,
            pr.target_commit.as_deref(),
            &args.remote,
        )?;
        outcome.tags = tag_commit(repo_root, &pr.commit, &since, &args.remote)?;
        if outcome.tags.is_empty() {
            eprintln!("No package with a tag changed version in this merge");
        }
        if matches!(args.provider, Provider::Github) && !args.no_github_release {
            for t in &outcome.tags {
                let url = forge.publish_release(&ReleaseInfo {
                    tag: t.tag.clone(),
                    title: if t.name.is_empty() {
                        format!("v{}", t.version)
                    } else {
                        format!("{} v{}", t.name, t.version)
                    },
                    notes: t
                        .notes
                        .clone()
                        .unwrap_or_else(|| format!("Release {}", t.version)),
                    prerelease: t.version.contains('-'),
                    draft: args.draft,
                })?;
                match &url {
                    Some(url) => eprintln!("Created GitHub Release {} ({url})", t.tag),
                    None => eprintln!("GitHub Release {} already exists", t.tag),
                }
                outcome.github_releases.push((t.tag.clone(), url));
            }
        }
    }

    let config = Config::load(repo_root)?;
    let pending = crate::changeset::reader::read_changesets(&config.changeset_dir(repo_root))?;
    if pending.is_empty() {
        eprintln!("No pending changesets, no version PR to update");
        return Ok(outcome);
    }

    let body = push_version_branch(repo_root, &args.remote, forge.body_limit())?;
    outcome.version_pr = Some(match forge.open_version_pr()? {
        Some(pr) => {
            forge.update_version_pr(&pr, &body)?;
            eprintln!("Updated version PR #{}", pr.id);
            (pr, false)
        }
        None => {
            let pr = forge.create_version_pr(&args.title, &body)?;
            eprintln!("Created version PR #{}", pr.id);
            (pr, true)
        }
    });
    Ok(outcome)
}

/// Fetches a commit a shallow checkout doesn't have, with its parent, which
/// the release is diffed against.
fn ensure_commit(repo_root: &Path, commit: &str, remote: &str) -> anyhow::Result<()> {
    let have = |rev: &str| -> anyhow::Result<bool> {
        let spec = format!("{rev}^{{commit}}");
        Ok(crate::git::run_git(repo_root, &["cat-file", "-e", &spec])?.is_some())
    };
    if !have(commit)? || !have(&format!("{commit}^1"))? {
        fetch_commit(repo_root, remote, commit, 2)?;
    }
    Ok(())
}

/// `--depth` only in a shallow checkout: in a full clone it would make the
/// repository shallow.
fn fetch_commit(repo_root: &Path, remote: &str, commit: &str, depth: u32) -> anyhow::Result<()> {
    let shallow = git(repo_root, &["rev-parse", "--is-shallow-repository"])?;
    let depth = format!("--depth={depth}");
    let mut args = vec!["fetch", "--quiet"];
    if shallow.trim() == "true" {
        args.push(&depth);
    }
    args.extend([remote, commit]);
    git(repo_root, &args)?;
    Ok(())
}

/// The commit the version PR merged into: versions that differ between it
/// and the merge are what the PR released. The forge's answer covers every
/// merge strategy; a shallow checkout may not have it yet. Without one, the
/// merge's first parent.
fn merge_target(
    repo_root: &Path,
    commit: &str,
    target: Option<&str>,
    remote: &str,
) -> anyhow::Result<String> {
    if let Some(target) = target {
        let spec = format!("{target}^{{commit}}");
        if crate::git::run_git(repo_root, &["cat-file", "-e", &spec])?.is_none() {
            fetch_commit(repo_root, remote, target, 1)?;
        }
        return Ok(target.to_string());
    }
    tag::resolve_commit(repo_root, &format!("{commit}^1")).map_err(|_| {
        anyhow::anyhow!(
            "the parent of {commit} isn't in this repository; fetch depth 2 or more is needed"
        )
    })
}

/// Tags read the versions in the working tree, so a HEAD that moved off the
/// merge gets a worktree at the merge.
fn tag_commit(
    repo_root: &Path,
    commit: &str,
    since: &str,
    remote: &str,
) -> anyhow::Result<Vec<TagResult>> {
    let args = TagArgs {
        since: Some(since.to_string()),
        push: true,
        remote: remote.to_string(),
        dry_run: false,
        output: None,
    };
    if tag::resolve_commit(repo_root, "HEAD")? == commit {
        return tag::run_in(repo_root, &args);
    }
    let worktree = tag::TempWorktree::add(repo_root, commit)?;
    tag::run_in(&worktree.path, &args)
}

/// Bumps in a temporary worktree of HEAD, commits and force-pushes that as
/// the version branch, and returns the PR description. The caller's
/// checkout is never touched: uncommitted edits and stray files in it stay
/// out of the version PR, and a failed bump leaves it as it was.
fn push_version_branch(repo_root: &Path, remote: &str, limit: usize) -> anyhow::Result<String> {
    let head = tag::resolve_commit(repo_root, "HEAD")?;
    let worktree = tag::TempWorktree::add(repo_root, &head)?;
    let wt = worktree.path.as_path();
    let result = executor::execute_version(
        wt,
        &ExecuteOptions {
            dry_run: false,
            no_commit: true,
            snapshot: None,
            origins: false,
        },
    )?;
    let body = crate::release::pr_body::render(&result.plan, limit);

    git(wt, &["add", "-A"])?;
    // Hooks off: the throwaway checkout has none of the tooling a husky
    // pre-commit or pre-push hook expects.
    let mut commit = identity_args(wt)?;
    commit.extend(
        [
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "chore: version packages",
        ]
        .map(str::to_string),
    );
    let commit: Vec<&str> = commit.iter().map(String::as_str).collect();
    git(wt, &commit)?;
    git(
        wt,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "push",
            "--quiet",
            "--force",
            remote,
            &format!("HEAD:refs/heads/{VERSION_BRANCH}"),
        ],
    )?;
    Ok(body)
}

/// A CI checkout often has no git identity; the commit then gets a generic
/// one instead of failing. A configured identity is left alone.
fn identity_args(repo_root: &Path) -> anyhow::Result<Vec<String>> {
    let mut args = Vec::new();
    for (key, fallback) in [
        ("user.name", "changesetter"),
        ("user.email", "changesetter@users.noreply.github.com"),
    ] {
        if crate::git::run_git(repo_root, &["config", key])?.is_none_or(|v| v.trim().is_empty()) {
            args.push("-c".to_string());
            args.push(format!("{key}={fallback}"));
        }
    }
    Ok(args)
}

fn git(repo_root: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .map_err(|_| crate::errors::ChangesetterError::GitNotFound)?;
    if !output.status.success() {
        let sub = args
            .iter()
            .find(|a| !a.starts_with('-') && !a.contains('='))
            .copied();
        anyhow::bail!(
            "git {} failed: {}",
            sub.unwrap_or_default(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::MergedPr;
    use std::cell::RefCell;
    use std::process::Command;

    #[derive(Default)]
    struct FakeForge {
        merged: Option<MergedPr>,
        open: Option<OpenPr>,
        recover: Option<MergedPr>,
        existing_releases: Vec<String>,
        fail_lookup: bool,
        calls: RefCell<Vec<String>>,
        published: RefCell<Vec<ReleaseInfo>>,
        bodies: RefCell<Vec<String>>,
    }

    impl Forge for FakeForge {
        fn merged_version_pr(&self, commit: &str) -> anyhow::Result<Option<MergedPr>> {
            self.calls.borrow_mut().push(format!("merged {commit}"));
            if self.fail_lookup {
                anyhow::bail!("Pull request query failed: HTTP 401");
            }
            Ok(self.merged.clone())
        }
        fn open_version_pr(&self) -> anyhow::Result<Option<OpenPr>> {
            self.calls.borrow_mut().push("open".into());
            Ok(self.open.clone())
        }
        fn create_version_pr(&self, title: &str, body: &str) -> anyhow::Result<OpenPr> {
            self.calls.borrow_mut().push(format!("create {title}"));
            self.bodies.borrow_mut().push(body.to_string());
            Ok(OpenPr { id: 8, url: None })
        }
        fn update_version_pr(&self, pr: &OpenPr, body: &str) -> anyhow::Result<()> {
            self.calls.borrow_mut().push(format!("update {}", pr.id));
            self.bodies.borrow_mut().push(body.to_string());
            Ok(())
        }
        fn recover_merged_version_pr(&self, _commit: &str) -> anyhow::Result<Option<MergedPr>> {
            self.calls.borrow_mut().push("recover".into());
            Ok(self.recover.clone())
        }
        fn publish_release(&self, release: &ReleaseInfo) -> anyhow::Result<Option<String>> {
            self.calls
                .borrow_mut()
                .push(format!("release {}", release.tag));
            let exists = self.existing_releases.contains(&release.tag)
                || self.published.borrow().iter().any(|r| r.tag == release.tag);
            if exists {
                return Ok(None);
            }
            self.published.borrow_mut().push(release.clone());
            Ok(Some(format!("https://gh/releases/{}", release.tag)))
        }
        fn body_limit(&self) -> usize {
            crate::release::pr_body::AZURE_LIMIT
        }
        fn ci_commit(&self) -> Option<String> {
            None
        }
    }

    fn sh(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn pkg(dir: &Path, rel: &str, name: &str, version: &str, deps: &str) {
        std::fs::create_dir_all(dir.join(rel)).unwrap();
        std::fs::write(
            dir.join(rel).join("package.json"),
            format!(r#"{{"name":"{name}","version":"{version}"{deps}}}"#),
        )
        .unwrap();
    }

    struct Repo {
        _tmp: tempfile::TempDir,
        origin: std::path::PathBuf,
        work: std::path::PathBuf,
        base: String,
        head: String,
    }

    /// A workspace whose last commit is a version PR merge bumping web and ui,
    /// pushed to a bare origin. `late` adds a changeset that landed while the
    /// version PR was open.
    fn repo(late: bool) -> Repo {
        let tmp = tempfile::tempdir().unwrap();
        let origin = tmp.path().join("origin.git");
        let work = tmp.path().join("work");
        sh(
            tmp.path(),
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                origin.to_str().unwrap(),
            ],
        );
        sh(
            tmp.path(),
            &[
                "clone",
                "-q",
                origin.to_str().unwrap(),
                work.to_str().unwrap(),
            ],
        );
        for (k, v) in [
            ("user.email", "t@t"),
            ("user.name", "T"),
            ("commit.gpgsign", "false"),
            ("tag.gpgsign", "false"),
        ] {
            sh(&work, &["config", k, v]);
        }
        std::fs::write(work.join("package.json"), r#"{"private":true}"#).unwrap();
        std::fs::write(
            work.join("changesetter.toml"),
            "ignore = [\"\"]\nupdate_internal_dependencies = \"patch\"\n[tag]\nskip = [\"packages/*\"]\n",
        )
        .unwrap();
        let dep = r#","dependencies":{"@d/ui":"workspace:*"}"#;
        pkg(&work, "apps/web", "@d/web", "1.0.0", dep);
        pkg(&work, "apps/admin", "@d/admin", "1.0.0", dep);
        pkg(&work, "packages/ui", "@d/ui", "0.1.0", "");
        std::fs::create_dir_all(work.join(".changeset")).unwrap();
        sh(&work, &["add", "-A"]);
        sh(&work, &["commit", "-q", "-m", "base"]);
        let base = sh(&work, &["rev-parse", "HEAD"]);
        pkg(&work, "apps/web", "@d/web", "1.0.1", dep);
        pkg(&work, "packages/ui", "@d/ui", "0.1.1", "");
        if late {
            std::fs::write(
                work.join(".changeset/late.md"),
                "---\n\"@d/ui\": minor\n---\n\nAdded a date picker.\n",
            )
            .unwrap();
        }
        sh(&work, &["add", "-A"]);
        sh(
            &work,
            &["commit", "-q", "-m", "Merged PR 7: Version Packages"],
        );
        sh(&work, &["push", "-q", "origin", "main"]);
        let head = sh(&work, &["rev-parse", "HEAD"]);
        Repo {
            _tmp: tmp,
            origin,
            work,
            base,
            head,
        }
    }

    fn args() -> VersionPrArgs {
        VersionPrArgs {
            provider: Provider::Azure,
            title: "Next Release".into(),
            base: "main".into(),
            commit: None,
            remote: "origin".into(),
            no_github_release: false,
            draft: false,
            output: None,
        }
    }

    fn remote_refs(r: &Repo) -> String {
        sh(&r.work, &["ls-remote", r.origin.to_str().unwrap()])
    }

    fn merged(r: &Repo) -> Option<MergedPr> {
        Some(MergedPr {
            id: 7,
            commit: r.head.clone(),
            target_commit: Some(r.base.clone()),
        })
    }

    #[test]
    fn merge_without_pending_changesets_only_tags() {
        let r = repo(false);
        let forge = FakeForge {
            merged: merged(&r),
            ..Default::default()
        };
        let out = run_with(&r.work, &args(), &forge).unwrap();
        assert_eq!(out.merged_pr, Some(7));
        let tags: Vec<_> = out.tags.iter().map(|t| t.tag.as_str()).collect();
        assert_eq!(tags, ["@d/web@v1.0.1"]);
        let refs = remote_refs(&r);
        assert!(refs.contains("refs/tags/@d/web@v1.0.1"), "{refs}");
        assert!(!refs.contains(VERSION_BRANCH), "{refs}");
        assert_eq!(*forge.calls.borrow(), [format!("merged {}", r.head)]);
        assert_eq!(out.to_json()["version_pr"], serde_json::Value::Null);
    }

    #[test]
    fn not_a_merge_with_changesets_creates_the_version_pr() {
        let r = repo(true);
        let forge = FakeForge::default();
        let out = run_with(&r.work, &args(), &forge).unwrap();
        assert_eq!(out.merged_pr, None);
        assert!(out.tags.is_empty());
        assert!(!remote_refs(&r).contains("refs/tags/"));
        assert_eq!(
            *forge.calls.borrow(),
            [
                format!("merged {}", r.head),
                "recover".into(),
                "open".into(),
                "create Next Release".into()
            ]
        );

        // The branch has the bump and the consumed changeset; main is untouched.
        let branch = format!("origin/{VERSION_BRANCH}");
        sh(&r.work, &["fetch", "-q", "origin"]);
        let ui = sh(
            &r.work,
            &["show", &format!("{branch}:packages/ui/package.json")],
        );
        assert!(ui.contains("0.2.0"), "{ui}");
        assert!(
            sh(&r.work, &["ls-tree", "-r", "--name-only", &branch])
                .lines()
                .all(|f| f != ".changeset/late.md")
        );
        assert_eq!(sh(&r.work, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
        assert_eq!(sh(&r.work, &["rev-parse", "origin/main"]), r.head);
        assert_eq!(sh(&r.work, &["status", "--porcelain"]), "");

        let body = &forge.bodies.borrow()[0];
        assert!(body.contains("## @d/ui 0.1.1 → 0.2.0\n\n"), "{body}");
        assert!(body.contains("Added a date picker."), "{body}");
        assert!(
            body.contains(
                "## Dependency updates only\n\n- @d/admin 1.0.0 → 1.0.1\n- @d/web 1.0.1 → 1.0.2"
            ),
            "{body}"
        );
    }

    #[test]
    fn merge_with_late_changesets_tags_then_updates_the_open_pr() {
        let r = repo(true);
        let forge = FakeForge {
            merged: merged(&r),
            open: Some(OpenPr { id: 9, url: None }),
            ..Default::default()
        };
        let out = run_with(&r.work, &args(), &forge).unwrap();
        assert_eq!(out.tags.len(), 1);
        assert!(remote_refs(&r).contains(VERSION_BRANCH));
        assert_eq!(forge.calls.borrow().last().unwrap(), "update 9");
        assert_eq!(
            out.to_json()["version_pr"],
            serde_json::json!({"id": 9, "url": null, "created": false})
        );
    }

    #[test]
    fn untracked_files_stay_out_of_the_version_commit() {
        let r = repo(true);
        std::fs::write(r.work.join("changesetter.tar.gz"), "binary").unwrap();
        run_with(&r.work, &args(), &FakeForge::default()).unwrap();
        sh(&r.work, &["fetch", "-q", "origin"]);
        let branch = format!("origin/{VERSION_BRANCH}");
        let files = sh(&r.work, &["ls-tree", "-r", "--name-only", &branch]);
        assert!(!files.contains("changesetter.tar.gz"), "{files}");
        assert!(r.work.join("changesetter.tar.gz").exists());
    }

    #[test]
    fn a_failed_lookup_fails_before_anything_is_pushed() {
        let r = repo(true);
        let forge = FakeForge {
            fail_lookup: true,
            ..Default::default()
        };
        let err = run_with(&r.work, &args(), &forge).unwrap_err();
        assert!(err.to_string().contains("HTTP 401"), "{err}");
        let refs = remote_refs(&r);
        assert!(
            !refs.contains("refs/tags/") && !refs.contains(VERSION_BRANCH),
            "{refs}"
        );
    }

    #[test]
    fn tags_the_given_commit_when_head_moved_on() {
        let r = repo(false);
        sh(&r.work, &["commit", "-q", "--allow-empty", "-m", "later"]);
        let mut a = args();
        a.commit = Some(r.head.clone());
        let forge = FakeForge {
            merged: merged(&r),
            ..Default::default()
        };
        run_with(&r.work, &a, &forge).unwrap();
        let tagged = sh(
            &r.work,
            &[
                "ls-remote",
                r.origin.to_str().unwrap(),
                "refs/tags/@d/web@v1.0.1^{}",
            ],
        );
        assert!(tagged.starts_with(&r.head), "{tagged}");
    }

    #[test]
    fn shallow_checkout_fetches_the_merge_target() {
        let r = repo(false);
        let shallow = r._tmp.path().join("shallow");
        sh(
            r._tmp.path(),
            &[
                "clone",
                "-q",
                "--depth=1",
                &format!("file://{}", r.origin.display()),
                shallow.to_str().unwrap(),
            ],
        );
        sh(&shallow, &["config", "tag.gpgsign", "false"]);
        let forge = FakeForge {
            merged: merged(&r),
            ..Default::default()
        };
        let out = run_with(&shallow, &args(), &forge).unwrap();
        assert_eq!(out.tags.len(), 1);
    }

    fn github() -> VersionPrArgs {
        VersionPrArgs {
            provider: Provider::Github,
            ..args()
        }
    }

    #[test]
    fn github_publishes_a_release_per_new_tag_once() {
        let r = repo(false);
        std::fs::write(
            r.work.join("CHANGELOG.md"),
            "# Changelog\n\n## 1.0.1, 0.1.1 - 2026-02-01\n\n### @d/ui\n\n- UI fix\n\n### @d/web\n\n- Web fix\n",
        )
        .unwrap();
        sh(&r.work, &["add", "-A"]);
        sh(&r.work, &["commit", "-q", "--amend", "--no-edit"]);
        sh(&r.work, &["push", "-q", "-f", "origin", "main"]);
        let head = sh(&r.work, &["rev-parse", "HEAD"]);
        let merged = Some(MergedPr {
            id: 7,
            commit: head.clone(),
            target_commit: Some(r.base.clone()),
        });

        let forge = FakeForge {
            merged: merged.clone(),
            ..Default::default()
        };
        let out = run_with(&r.work, &github(), &forge).unwrap();
        let published = forge.published.borrow();
        assert_eq!(
            *published,
            [ReleaseInfo {
                tag: "@d/web@v1.0.1".into(),
                title: "@d/web v1.0.1".into(),
                notes: "- Web fix".into(),
                prerelease: false,
                draft: false,
            }]
        );
        let json = out.to_json();
        assert_eq!(json["released"][0]["changelog"], "- Web fix");
        assert_eq!(json["github_releases"][0]["created"], true);

        // A re-run creates nothing and reports nothing released.
        let again = FakeForge {
            merged,
            existing_releases: vec!["@d/web@v1.0.1".into()],
            ..Default::default()
        };
        let out = run_with(&r.work, &github(), &again).unwrap();
        assert_eq!(out.to_json()["released"], serde_json::json!([]));
        assert!(again.published.borrow().is_empty());
    }

    #[test]
    fn github_recovers_a_merge_whose_run_was_cancelled() {
        let r = repo(false);
        sh(
            &r.work,
            &["commit", "-q", "--allow-empty", "-m", "unrelated"],
        );
        sh(&r.work, &["push", "-q", "origin", "main"]);
        let forge = FakeForge {
            recover: merged(&r),
            ..Default::default()
        };
        let out = run_with(&r.work, &github(), &forge).unwrap();
        assert_eq!(out.merged_pr, Some(7));
        let tagged = sh(
            &r.work,
            &[
                "ls-remote",
                r.origin.to_str().unwrap(),
                "refs/tags/@d/web@v1.0.1^{}",
            ],
        );
        assert!(tagged.starts_with(&r.head), "{tagged}");
        assert_eq!(out.released().len(), 1);

        // The next push finds it fully released.
        let out = run_with(
            &r.work,
            &github(),
            &FakeForge {
                recover: merged(&r),
                existing_releases: vec!["@d/web@v1.0.1".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert!(out.released().is_empty());
        assert!(!out.tags[0].created);
    }

    #[test]
    fn github_prerelease_and_draft_flags() {
        let r = repo(false);
        let mut a = github();
        a.draft = true;
        let forge = FakeForge {
            merged: merged(&r),
            ..Default::default()
        };
        run_with(&r.work, &a, &forge).unwrap();
        assert!(forge.published.borrow()[0].draft);

        let mut a = github();
        a.no_github_release = true;
        let r = repo(false);
        let forge = FakeForge {
            merged: merged(&r),
            ..Default::default()
        };
        run_with(&r.work, &a, &forge).unwrap();
        assert!(forge.published.borrow().is_empty());
    }

    #[test]
    fn tracked_edits_in_the_checkout_stay_out_and_stay_put() {
        let r = repo(true);
        std::fs::write(r.work.join("apps/admin/notes.txt"), "local").unwrap();
        sh(&r.work, &["add", "apps/admin/notes.txt"]);
        sh(&r.work, &["commit", "-q", "-m", "notes"]);
        std::fs::write(r.work.join("apps/admin/notes.txt"), "edited locally").unwrap();
        run_with(&r.work, &args(), &FakeForge::default()).unwrap();
        sh(&r.work, &["fetch", "-q", "origin"]);
        let on_branch = sh(
            &r.work,
            &[
                "show",
                &format!("origin/{VERSION_BRANCH}:apps/admin/notes.txt"),
            ],
        );
        assert_eq!(on_branch, "local");
        assert_eq!(
            std::fs::read_to_string(r.work.join("apps/admin/notes.txt")).unwrap(),
            "edited locally"
        );
    }

    #[test]
    fn a_failed_bump_leaves_the_checkout_alone() {
        let r = repo(true);
        let toml = std::fs::read_to_string(r.work.join("changesetter.toml")).unwrap();
        std::fs::write(
            r.work.join("changesetter.toml"),
            format!("{toml}[hooks]\npost_bump = [\"false\"]\n"),
        )
        .unwrap();
        sh(&r.work, &["commit", "-q", "-am", "failing hook"]);
        let err = run_with(&r.work, &args(), &FakeForge::default()).unwrap_err();
        assert!(err.to_string().contains("post-bump hook failed"), "{err}");
        assert_eq!(sh(&r.work, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
        assert_eq!(sh(&r.work, &["status", "--porcelain"]), "");
        assert!(!remote_refs(&r).contains(VERSION_BRANCH));
        assert_eq!(sh(&r.work, &["worktree", "list"]).lines().count(), 1);
    }
}
