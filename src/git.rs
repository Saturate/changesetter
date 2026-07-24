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

pub fn diff_changeset_files(repo_root: &Path, base: &str) -> anyhow::Result<Vec<String>> {
    let output = Command::new("git")
        .args([
            "diff",
            "--name-only",
            &format!("{base}...HEAD"),
            "--",
            ".changeset/",
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
    Ok(stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect())
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

pub fn git_tag(repo_root: &Path, tag: &str, message: Option<&str>) -> anyhow::Result<()> {
    let output = if let Some(msg) = message {
        Command::new("git")
            .args(["tag", "-a", tag, "-m", msg])
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
