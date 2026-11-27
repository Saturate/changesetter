use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clap::Args;

use crate::cli::status::repo_relative;
use crate::config::Config;
use crate::package::detector;

#[derive(Args)]
pub struct TagArgs {
    /// Only tag packages whose version differs from this commit, such as the
    /// first parent of a version PR merge. A tag for such a version that
    /// already points at another commit is then a version collision.
    #[arg(long)]
    pub since: Option<String>,

    /// Check tags on the remote instead of locally, and push new tags to it
    /// in one atomic push
    #[arg(long)]
    pub push: bool,

    /// Remote that --push checks and pushes to
    #[arg(long, default_value = "origin")]
    pub remote: String,

    /// Show which tags would be created without creating them
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (json for CI consumption)
    #[arg(long)]
    pub output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagResult {
    pub name: String,
    pub version: String,
    pub tag: String,
    /// False when the tag already pointed at HEAD.
    pub created: bool,
}

pub fn run(args: &TagArgs) -> anyhow::Result<()> {
    let repo_root = crate::git::find_repo_root()?;
    let results = run_in(&repo_root, args)?;
    if args.output.as_deref() == Some("json") {
        println!("{}", serde_json::to_string_pretty(&to_json(&results))?);
    }
    Ok(())
}

pub fn to_json(results: &[TagResult]) -> serde_json::Value {
    let tags: Vec<_> = results
        .iter()
        .map(|r| {
            serde_json::json!({
                "name": r.name,
                "version": r.version,
                "tag": r.tag,
                "created": r.created,
            })
        })
        .collect();
    serde_json::json!({ "tags": tags })
}

struct Candidate {
    name: String,
    version: String,
    tag: String,
}

/// Creates the tags missing for the current package versions at HEAD and
/// returns them along with the ones that already pointed at HEAD.
pub fn run_in(repo_root: &Path, args: &TagArgs) -> anyhow::Result<Vec<TagResult>> {
    let config = Config::load(repo_root)?;
    let packages = detector::detect_packages(repo_root, &config)?;
    let is_monorepo = packages.len() > 1;
    let head = resolve_commit(repo_root, "HEAD")?;

    let previous = match &args.since {
        Some(rev) => Some(versions_at(repo_root, rev)?),
        None => None,
    };

    let candidates: Vec<Candidate> = packages
        .iter()
        .filter_map(|p| {
            let version = p.version.to_string();
            if let Some(prev) = &previous {
                if prev.get(&p.name) == Some(&version) {
                    return None;
                }
            }
            let path = repo_relative(repo_root, &p.path);
            let tag = config.tag.tag_for(&p.name, &path, &version, is_monorepo)?;
            Some(Candidate {
                name: p.name.clone(),
                version,
                tag,
            })
        })
        .collect();

    let tag_names: Vec<&str> = candidates.iter().map(|c| c.tag.as_str()).collect();
    let existing = if args.push {
        remote_tag_commits(repo_root, &args.remote, &tag_names)?
    } else {
        local_tag_commits(repo_root, &tag_names)?
    };
    let location = if args.push {
        args.remote.as_str()
    } else {
        "this repository"
    };

    let mut results = Vec::new();
    let mut collisions = Vec::new();
    for c in candidates {
        match existing.get(&c.tag) {
            Some(commit) if *commit == head => {
                eprintln!("{} already exists at {head}", c.tag);
                results.push(TagResult {
                    name: c.name,
                    version: c.version,
                    tag: c.tag,
                    created: false,
                });
            }
            // Without --since every package is a candidate, and an older
            // release's tag sitting on an older commit is the normal case.
            Some(_) if previous.is_none() => {}
            Some(commit) => collisions.push(format!(
                "tag {} already exists on {location} at {commit}, not at {head}",
                c.tag
            )),
            None => results.push(TagResult {
                name: c.name,
                version: c.version,
                tag: c.tag,
                created: true,
            }),
        }
    }

    // Another commit already shipped as this version. Tagging the rest would
    // release part of the merge, so nothing is tagged.
    if !collisions.is_empty() {
        anyhow::bail!(
            "Version collision: {}. Bump the version in a new version PR, or move the tag if it is wrong.",
            collisions.join("; ")
        );
    }

    let created: Vec<&TagResult> = results.iter().filter(|r| r.created).collect();
    if args.dry_run {
        for r in &created {
            eprintln!("Would tag {}", r.tag);
        }
        return Ok(results);
    }

    for r in &created {
        let message = if config.release.tag_annotated {
            Some(if is_monorepo && !r.name.is_empty() {
                format!("Release {}@{}", r.name, r.version)
            } else {
                format!("Release {}", r.version)
            })
        } else {
            None
        };
        // -f in push mode: a local tag left by an earlier run that never got
        // pushed isn't on the remote, so it's replaced rather than trusted.
        create_tag(repo_root, &r.tag, &head, message.as_deref(), args.push)?;
        eprintln!("Tagged {}", r.tag);
    }

    if args.push && !created.is_empty() {
        let mut push_args = vec![
            "push".to_string(),
            "--atomic".to_string(),
            args.remote.clone(),
        ];
        push_args.extend(created.iter().map(|r| format!("refs/tags/{}", r.tag)));
        let push_args: Vec<&str> = push_args.iter().map(String::as_str).collect();
        run_checked(repo_root, &push_args)?;
        eprintln!("Pushed {} tag(s) to {}", created.len(), args.remote);
    }

    Ok(results)
}

fn run_checked(repo_root: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .map_err(|_| crate::errors::ChangesetterError::GitNotFound)?;
    if !output.status.success() {
        anyhow::bail!(
            "git {} failed: {}",
            args.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn resolve_commit(repo_root: &Path, rev: &str) -> anyhow::Result<String> {
    let spec = format!("{rev}^{{commit}}");
    match crate::git::run_git(repo_root, &["rev-parse", "--verify", "-q", &spec])? {
        Some(sha) => Ok(sha.trim().to_lowercase()),
        None => anyhow::bail!("{rev} isn't a commit in this repository; fetch it first"),
    }
}

fn create_tag(
    repo_root: &Path,
    tag: &str,
    commit: &str,
    message: Option<&str>,
    force: bool,
) -> anyhow::Result<()> {
    let mut args = vec!["tag"];
    if force {
        args.push("-f");
    }
    if let Some(msg) = message {
        args.extend(["-a", "-m", msg, "--cleanup=verbatim"]);
    }
    args.extend([tag, commit]);
    run_checked(repo_root, &args)?;
    Ok(())
}

fn local_tag_commits(repo_root: &Path, tags: &[&str]) -> anyhow::Result<BTreeMap<String, String>> {
    let mut found = BTreeMap::new();
    for tag in tags {
        let spec = format!("refs/tags/{tag}^{{commit}}");
        if let Some(sha) = crate::git::run_git(repo_root, &["rev-parse", "--verify", "-q", &spec])?
        {
            found.insert((*tag).to_string(), sha.trim().to_lowercase());
        }
    }
    Ok(found)
}

/// The commit each tag points at on the remote. Asks the remote rather than
/// trusting local tags, which a self-hosted agent can keep from earlier runs.
fn remote_tag_commits(
    repo_root: &Path,
    remote: &str,
    tags: &[&str],
) -> anyhow::Result<BTreeMap<String, String>> {
    if tags.is_empty() {
        return Ok(BTreeMap::new());
    }
    // The `^{}` pattern is what makes ls-remote list an annotated tag's
    // commit; without it only the tag object's id comes back.
    let refs: Vec<String> = tags
        .iter()
        .flat_map(|t| [format!("refs/tags/{t}"), format!("refs/tags/{t}^{{}}")])
        .collect();
    let mut args = vec!["ls-remote", "--tags", remote];
    args.extend(refs.iter().map(String::as_str));
    let out = run_checked(repo_root, &args)?;
    Ok(parse_ls_remote(&out, tags))
}

/// Maps each wanted tag to its commit: the peeled `^{}` line for an
/// annotated tag, the plain line for a lightweight one. ls-remote matches
/// patterns by suffix, so only exact refs count.
fn parse_ls_remote(out: &str, tags: &[&str]) -> BTreeMap<String, String> {
    let mut plain = BTreeMap::new();
    let mut peeled = BTreeMap::new();
    for line in out.lines() {
        let Some((sha, reference)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let reference = reference.trim();
        let Some(name) = reference.strip_prefix("refs/tags/") else {
            continue;
        };
        match name.strip_suffix("^{}") {
            Some(name) => peeled.insert(name.to_string(), sha.to_lowercase()),
            None => plain.insert(name.to_string(), sha.to_lowercase()),
        };
    }
    tags.iter()
        .filter_map(|t| {
            let sha = peeled.get(*t).or_else(|| plain.get(*t))?;
            Some(((*t).to_string(), sha.clone()))
        })
        .collect()
}

/// Each package's version at `rev`, read from a temporary worktree so every
/// adapter and the config at that commit apply as they did then.
fn versions_at(repo_root: &Path, rev: &str) -> anyhow::Result<BTreeMap<String, String>> {
    Ok(packages_at(repo_root, rev)?
        .packages
        .into_iter()
        .map(|p| (p.name, p.version))
        .collect())
}

/// The packages at a commit and the config that applied to them.
pub(crate) struct Snapshot {
    pub commit: String,
    pub config: Config,
    pub packages: Vec<SnapshotPackage>,
}

pub(crate) struct SnapshotPackage {
    pub name: String,
    /// Relative to the repo root, as `status` prints it.
    pub path: String,
    pub version: String,
}

pub(crate) fn packages_at(repo_root: &Path, rev: &str) -> anyhow::Result<Snapshot> {
    let commit = resolve_commit(repo_root, rev)?;
    let worktree = TempWorktree::add(repo_root, &commit)?;
    let config = Config::load(&worktree.path)?;
    let packages = detector::detect_packages(&worktree.path, &config)?
        .into_iter()
        .map(|p| SnapshotPackage {
            path: repo_relative(&worktree.path, &p.path),
            version: p.version.to_string(),
            name: p.name,
        })
        .collect();
    Ok(Snapshot {
        commit,
        config,
        packages,
    })
}

pub(crate) struct TempWorktree {
    repo_root: PathBuf,
    pub path: PathBuf,
}

impl TempWorktree {
    pub(crate) fn add(repo_root: &Path, commit: &str) -> anyhow::Result<Self> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let path =
            std::env::temp_dir().join(format!("changesetter-since-{}-{nanos}", std::process::id()));
        let path_str = path.to_string_lossy().into_owned();
        run_checked(
            repo_root,
            &["worktree", "add", "--quiet", "--detach", &path_str, commit],
        )?;
        Ok(Self {
            repo_root: repo_root.to_path_buf(),
            path,
        })
    }
}

impl Drop for TempWorktree {
    fn drop(&mut self) {
        let path = self.path.to_string_lossy().into_owned();
        if let Err(e) = run_checked(&self.repo_root, &["worktree", "remove", "--force", &path]) {
            eprintln!("warning: could not remove worktree {path}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) -> String {
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

    fn write_pkg(dir: &Path, rel: &str, name: &str, version: &str) {
        let pkg = dir.join(rel);
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(
            pkg.join("package.json"),
            format!(r#"{{"name":"{name}","version":"{version}"}}"#),
        )
        .unwrap();
    }

    fn commit(dir: &Path, msg: &str) -> String {
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", msg]);
        git(dir, &["rev-parse", "HEAD"])
    }

    /// apps/web and packages/ui at 1.0.0, then a commit bumping both to 1.0.1.
    fn monorepo() -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        for (k, v) in [
            ("user.email", "t@t"),
            ("user.name", "T"),
            ("commit.gpgsign", "false"),
            ("tag.gpgsign", "false"),
        ] {
            git(p, &["config", k, v]);
        }
        std::fs::write(
            p.join("changesetter.toml"),
            "[tag]\nskip = [\"packages/*\"]\n",
        )
        .unwrap();
        write_pkg(p, "apps/web", "@s/web", "1.0.0");
        write_pkg(p, "apps/admin", "@s/admin", "1.0.0");
        write_pkg(p, "packages/ui", "@s/ui", "1.0.0");
        let base = commit(p, "base");
        write_pkg(p, "apps/web", "@s/web", "1.0.1");
        write_pkg(p, "packages/ui", "@s/ui", "1.0.1");
        let head = commit(p, "version packages");
        (dir, base, head)
    }

    fn args(since: Option<&str>) -> TagArgs {
        TagArgs {
            since: since.map(str::to_string),
            push: false,
            remote: "origin".to_string(),
            dry_run: false,
            output: None,
        }
    }

    fn tags(dir: &Path) -> Vec<String> {
        let out = git(dir, &["tag", "-l"]);
        out.lines().map(str::to_string).collect()
    }

    #[test]
    fn since_tags_only_changed_unskipped_packages() {
        let (dir, base, head) = monorepo();
        let results = run_in(dir.path(), &args(Some(&base))).unwrap();
        assert_eq!(
            results,
            vec![TagResult {
                name: "@s/web".into(),
                version: "1.0.1".into(),
                tag: "@s/web@v1.0.1".into(),
                created: true,
            }]
        );
        assert_eq!(tags(dir.path()), vec!["@s/web@v1.0.1"]);
        assert_eq!(
            git(dir.path(), &["rev-parse", "@s/web@v1.0.1^{commit}"]),
            head
        );
        let worktrees = git(dir.path(), &["worktree", "list"]);
        assert_eq!(worktrees.lines().count(), 1, "{worktrees}");
    }

    #[test]
    fn rerun_reports_existing_tag_at_head_without_creating() {
        let (dir, base, _) = monorepo();
        run_in(dir.path(), &args(Some(&base))).unwrap();
        let again = run_in(dir.path(), &args(Some(&base))).unwrap();
        assert_eq!(again.len(), 1);
        assert!(!again[0].created);
    }

    #[test]
    fn without_since_tags_every_untagged_version_and_skips_older_tags() {
        let (dir, base, _) = monorepo();
        git(dir.path(), &["tag", "@s/admin@v1.0.0", &base]);
        let results = run_in(dir.path(), &args(None)).unwrap();
        let names: Vec<&str> = results.iter().map(|r| r.tag.as_str()).collect();
        assert_eq!(names, vec!["@s/web@v1.0.1"]);
    }

    #[test]
    fn since_fails_on_a_version_collision_and_tags_nothing() {
        let (dir, base, _) = monorepo();
        git(dir.path(), &["tag", "@s/web@v1.0.1", &base]);
        let err = run_in(dir.path(), &args(Some(&base))).unwrap_err();
        assert!(
            err.to_string()
                .contains("Version collision: tag @s/web@v1.0.1"),
            "{err}"
        );
        assert_eq!(
            git(dir.path(), &["rev-parse", "@s/web@v1.0.1^{commit}"]),
            base
        );
    }

    #[test]
    fn dry_run_creates_nothing() {
        let (dir, base, _) = monorepo();
        let mut a = args(Some(&base));
        a.dry_run = true;
        let results = run_in(dir.path(), &a).unwrap();
        assert_eq!(results.len(), 1);
        assert!(tags(dir.path()).is_empty());
    }

    #[test]
    fn push_checks_the_remote_and_pushes_atomically() {
        let (dir, base, head) = monorepo();
        let remote = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "-q", "--bare"]);
        let remote_path = remote.path().to_string_lossy().into_owned();
        git(dir.path(), &["remote", "add", "origin", &remote_path]);
        git(dir.path(), &["push", "-q", "origin", "main"]);
        // A stale local tag from an earlier run that never got pushed.
        git(dir.path(), &["tag", "@s/web@v1.0.1", &base]);

        let mut a = args(Some(&base));
        a.push = true;
        let results = run_in(dir.path(), &a).unwrap();
        assert!(results[0].created);
        let remote_tags = git(remote.path(), &["rev-parse", "@s/web@v1.0.1^{commit}"]);
        assert_eq!(remote_tags, head);

        let again = run_in(dir.path(), &a).unwrap();
        assert!(!again[0].created);
    }

    #[test]
    fn single_package_uses_the_plain_version_tag() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        for (k, v) in [
            ("user.email", "t@t"),
            ("user.name", "T"),
            ("commit.gpgsign", "false"),
            ("tag.gpgsign", "false"),
        ] {
            git(p, &["config", k, v]);
        }
        write_pkg(p, ".", "demo", "1.0.0");
        let base = commit(p, "base");
        write_pkg(p, ".", "demo", "1.1.0");
        commit(p, "bump");
        let results = run_in(p, &args(Some(&base))).unwrap();
        assert_eq!(results[0].tag, "v1.1.0");
        let message = git(p, &["tag", "-l", "--format=%(contents:subject)", "v1.1.0"]);
        assert_eq!(message, "Release 1.1.0");
    }

    #[test]
    fn ls_remote_prefers_peeled_and_exact_refs() {
        let out = "aaa\trefs/tags/v1.0.0\nbbb\trefs/tags/v1.0.0^{}\nccc\trefs/tags/x/v2.0.0\nddd\trefs/tags/lw\n";
        let found = parse_ls_remote(out, &["v1.0.0", "v2.0.0", "lw"]);
        assert_eq!(found.get("v1.0.0").map(String::as_str), Some("bbb"));
        assert_eq!(found.get("lw").map(String::as_str), Some("ddd"));
        assert!(!found.contains_key("v2.0.0"));
    }
}
