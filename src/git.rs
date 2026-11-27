use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;

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

/// A file under the changeset dir that differs between a base and the
/// working tree.
#[derive(Debug)]
pub struct ChangedFile {
    pub path: String,
    pub deleted: bool,
}

/// Changeset files added, changed or deleted since `base`: committed on this
/// branch, staged, unstaged or still untracked. Local gates run before the
/// commit, so committed-only would miss the changeset they are meant to find;
/// CI checkouts are clean, so for them this is the same as `base...HEAD`.
pub fn diff_changeset_files(
    repo_root: &Path,
    base: &str,
    changeset_dir: &str,
) -> anyhow::Result<Vec<ChangedFile>> {
    let dir_arg = format!("{changeset_dir}/");

    let merge_base = merge_base(repo_root, base)?;

    // -z, because without it git quotes any path with non-ASCII or `"` in
    // it, and the quoted name no longer ends in `.md`. --no-renames turns a
    // rename into a delete plus an add, so the new name is read like any
    // other added file.
    let tracked = run_git(
        repo_root,
        &[
            "diff",
            "--name-status",
            "-z",
            "--no-renames",
            &merge_base,
            "--",
            &dir_arg,
        ],
    )?
    .ok_or_else(|| anyhow::anyhow!("git diff against {base} failed"))?;

    let untracked = run_git(
        repo_root,
        &[
            "ls-files",
            "-z",
            "--others",
            "--exclude-standard",
            "--",
            &dir_arg,
        ],
    )?
    .ok_or_else(|| anyhow::anyhow!("git ls-files failed"))?;

    let mut files = std::collections::BTreeMap::new();
    let mut fields = tracked.split('\0').filter(|f| !f.is_empty());
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        files.insert(path.to_string(), status == "D");
    }
    for path in untracked.split('\0').filter(|f| !f.is_empty()) {
        files.insert(path.to_string(), false);
    }
    Ok(files
        .into_iter()
        .map(|(path, deleted)| ChangedFile { path, deleted })
        .collect())
}

/// Reads a changeset file the diff reported, relative to the repo root. The
/// content ends up in a PR comment, so a symlink is refused instead of
/// followed out of the repo.
pub fn read_changeset_file(repo_root: &Path, path: &str) -> anyhow::Result<String> {
    let full = repo_root.join(path);
    let meta = std::fs::symlink_metadata(&full).with_context(|| format!("reading {path}"))?;
    if !meta.is_file() {
        anyhow::bail!("{path} is not a regular file");
    }
    let root = repo_root.canonicalize()?;
    if !full.canonicalize()?.starts_with(&root) {
        anyhow::bail!("{path} resolves outside the repository");
    }
    std::fs::read_to_string(&full).with_context(|| format!("{path} is not valid UTF-8"))
}

/// Whether anything changed since the branch left `base`: committed, staged,
/// unstaged or untracked (and not gitignored).
pub fn has_changes_since(repo_root: &Path, base: &str) -> anyhow::Result<bool> {
    Ok(!changed_paths_since(repo_root, base)?.is_empty())
}

/// Every path changed since the branch left `base`, relative to the repo
/// root: committed, staged, unstaged or untracked (and not gitignored). A
/// rename lists both names, since both sides changed.
pub fn changed_paths_since(repo_root: &Path, base: &str) -> anyhow::Result<Vec<String>> {
    let merge_base = merge_base(repo_root, base)?;
    let changed = run_git(
        repo_root,
        &["diff", "--name-only", "-z", "--no-renames", &merge_base],
    )?
    .ok_or_else(|| anyhow::anyhow!("git diff against {base} failed"))?;
    let untracked = run_git(
        repo_root,
        &["ls-files", "-z", "--others", "--exclude-standard"],
    )?
    .ok_or_else(|| anyhow::anyhow!("git ls-files failed"))?;
    let paths: std::collections::BTreeSet<String> = changed
        .split('\0')
        .chain(untracked.split('\0'))
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect();
    Ok(paths.into_iter().collect())
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
pub(crate) fn run_git(repo_root: &Path, args: &[&str]) -> anyhow::Result<Option<String>> {
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
