use std::path::{Path, PathBuf};
use std::process::Command;

use crate::errors::ChangesetterError;

pub fn find_repo_root() -> anyhow::Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|_| ChangesetterError::GitNotFound)?;

    if !output.status.success() {
        anyhow::bail!(ChangesetterError::NotAGitRepo);
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(path))
}

pub fn is_working_tree_clean(repo_root: &Path) -> anyhow::Result<bool> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo_root)
        .output()
        .map_err(|_| ChangesetterError::GitNotFound)?;

    Ok(output.stdout.is_empty())
}

/// Changeset files added since `base`: committed on this branch, staged, or
/// still untracked. Local gates run before the commit, so committed-only
/// would miss the changeset they are meant to find; CI checkouts are clean,
/// so for them this is the same as comparing `base...HEAD`.
pub fn diff_changeset_files(
    repo_root: &Path,
    base: &str,
    changeset_dir: &str,
) -> anyhow::Result<Vec<String>> {
    let dir_arg = format!("{changeset_dir}/");

    let merge_base = merge_base(repo_root, base)?;
    let merge_base = merge_base.as_str();

    let added = run_git(
        repo_root,
        &[
            "diff",
            "--name-only",
            "--diff-filter=A",
            merge_base,
            "--",
            &dir_arg,
        ],
    )?
    .ok_or_else(|| anyhow::anyhow!("git diff against {base} failed"))?;

    let untracked = run_git(
        repo_root,
        &["ls-files", "--others", "--exclude-standard", "--", &dir_arg],
    )?
    .ok_or_else(|| anyhow::anyhow!("git ls-files failed"))?;

    let files: std::collections::BTreeSet<String> = added
        .lines()
        .chain(untracked.lines())
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();
    Ok(files.into_iter().collect())
}

/// Whether anything changed since the branch left `base`: committed, staged,
/// unstaged or untracked (and not gitignored).
pub fn has_changes_since(repo_root: &Path, base: &str) -> anyhow::Result<bool> {
    let merge_base = merge_base(repo_root, base)?;
    let changed = run_git(repo_root, &["diff", "--name-only", &merge_base])?
        .ok_or_else(|| anyhow::anyhow!("git diff against {base} failed"))?;
    if !changed.trim().is_empty() {
        return Ok(true);
    }
    let untracked = run_git(repo_root, &["ls-files", "--others", "--exclude-standard"])?
        .ok_or_else(|| anyhow::anyhow!("git ls-files failed"))?;
    Ok(!untracked.trim().is_empty())
}

/// The commit where HEAD's branch left `base`. `base...HEAD` diffs against
/// it; diffing it against the working tree also takes in uncommitted changes.
fn merge_base(repo_root: &Path, base: &str) -> anyhow::Result<String> {
    let out = run_git(repo_root, &["merge-base", base, "HEAD"])?.ok_or_else(|| {
        ChangesetterError::BaseRefUnavailable {
            base: base.to_string(),
        }
    })?;
    Ok(out.trim().to_string())
}

/// Runs git and returns stdout, or `None` if git exited with an error.
fn run_git(repo_root: &Path, args: &[&str]) -> anyhow::Result<Option<String>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .map_err(|_| ChangesetterError::GitNotFound)?;
    Ok(output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned()))
}

pub fn git_add(repo_root: &Path, paths: &[&str]) -> anyhow::Result<()> {
    let mut cmd = Command::new("git");
    cmd.arg("add").current_dir(repo_root);
    for path in paths {
        cmd.arg(path);
    }
    let output = cmd.output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git add failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

pub fn git_commit(repo_root: &Path, message: &str) -> anyhow::Result<()> {
    let output = Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(repo_root)
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

pub fn tag_exists(repo_root: &Path, tag: &str) -> anyhow::Result<bool> {
    let output = Command::new("git")
        .args(["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}")])
        .current_dir(repo_root)
        .output()
        .map_err(|_| ChangesetterError::GitNotFound)?;
    // 1 is "no such ref"; anything else (128) is git failing, which must not
    // read as "untagged" to a caller deciding whether to release.
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => anyhow::bail!(
            "git rev-parse failed for tag {tag}: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

pub fn git_tag(repo_root: &Path, tag: &str, message: Option<&str>) -> anyhow::Result<()> {
    let output = if let Some(msg) = message {
        Command::new("git")
            .args(["tag", "-a", tag, "-m", msg, "--cleanup=verbatim"])
            .current_dir(repo_root)
            .output()?
    } else {
        Command::new("git")
            .args(["tag", tag])
            .current_dir(repo_root)
            .output()?
    };
    if !output.status.success() {
        anyhow::bail!(
            "git tag failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}
