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

/// A file under the changeset dir that differs between a base and HEAD.
pub struct ChangedFile {
    pub path: String,
    pub deleted: bool,
}

pub fn diff_changeset_files(
    repo_root: &Path,
    base: &str,
    changeset_dir: &str,
) -> anyhow::Result<Vec<ChangedFile>> {
    let dir_arg = format!("{changeset_dir}/");
    // -z, because without it git quotes any path with non-ASCII or `"` in
    // it, and the quoted name no longer ends in `.md`. --no-renames turns a
    // rename into a delete plus an add, so the new name is read like any
    // other added file.
    let output = Command::new("git")
        .args([
            "diff",
            "--name-status",
            "-z",
            "--no-renames",
            &format!("{base}...HEAD"),
            "--",
            &dir_arg,
        ])
        .current_dir(repo_root)
        .output()
        .map_err(|_| ChangesetterError::GitNotFound)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("unknown revision") || stderr.contains("bad revision") {
            anyhow::bail!(ChangesetterError::BaseRefUnavailable {
                base: base.to_string()
            });
        }
        anyhow::bail!("git diff failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut fields = stdout.split('\0').filter(|f| !f.is_empty());
    let mut files = Vec::new();
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        files.push(ChangedFile {
            path: path.to_string(),
            deleted: status == "D",
        });
    }
    Ok(files)
}

/// The content of `path` as committed at HEAD. Reading the commit rather
/// than the working tree keeps it to what the diff compared, and a
/// committed symlink comes back as its target path instead of being
/// followed out of the repo.
pub fn read_at_head(repo_root: &Path, path: &str) -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(["show", &format!("HEAD:{path}")])
        .current_dir(repo_root)
        .output()
        .map_err(|_| ChangesetterError::GitNotFound)?;
    if !output.status.success() {
        anyhow::bail!(
            "git show failed for {path}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8(output.stdout).with_context(|| format!("{path} is not valid UTF-8"))
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
