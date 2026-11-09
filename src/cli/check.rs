use std::path::Path;

use clap::Args;

use crate::changeset::reader;
use crate::errors::ChangesetterError;

#[derive(Args)]
pub struct CheckArgs {
    /// Base branch to compare against. Counts changesets added since the
    /// merge base, including staged and untracked ones.
    #[arg(long)]
    pub base: Option<String>,
}

pub fn run(args: &CheckArgs) -> anyhow::Result<()> {
    let repo_root = crate::git::find_repo_root()?;
    run_in(&repo_root, args)
}

pub fn run_in(repo_root: &Path, args: &CheckArgs) -> anyhow::Result<()> {
    let config = crate::config::Config::load(repo_root)?;
    let changeset_dir = config.changeset_dir(repo_root);

    if let Some(base) = &args.base {
        check_with_base(repo_root, &config, base)
    } else {
        check_without_base(&changeset_dir)
    }
}

fn check_without_base(changeset_dir: &Path) -> anyhow::Result<()> {
    let changesets = reader::read_changesets(changeset_dir)?;

    if changesets.is_empty() {
        eprintln!("Run `changesetter add` to create one.");
        anyhow::bail!(ChangesetterError::NoChangesets);
    }

    print_summary(&changesets);
    Ok(())
}

fn check_with_base(
    repo_root: &Path,
    config: &crate::config::Config,
    base: &str,
) -> anyhow::Result<()> {
    let changeset_rel = config.changeset_dir_relative();
    let files = crate::git::diff_changeset_files(repo_root, base, changeset_rel)?;

    let added: Vec<&str> = files
        .iter()
        .filter(|f| f.ends_with(".md") && !f.ends_with("README.md"))
        .map(|f| f.as_str())
        .collect();

    if added.is_empty() {
        eprintln!("Run `changesetter add` to create one.");
        anyhow::bail!(ChangesetterError::NoChangesets);
    }

    eprintln!("{} changeset(s) found vs {base}:", added.len());
    for f in &added {
        eprintln!("  {f}");
    }

    Ok(())
}

fn print_summary(changesets: &[crate::changeset::Changeset]) {
    use std::collections::BTreeMap;

    let mut by_package: BTreeMap<&str, Vec<&crate::changeset::BumpLevel>> = BTreeMap::new();
    for cs in changesets {
        for (pkg, bump) in &cs.packages {
            by_package.entry(pkg.as_str()).or_default().push(bump);
        }
    }

    eprintln!("{} changeset(s) found:", changesets.len());
    for (pkg, bumps) in &by_package {
        let max_bump = bumps.iter().copied().max().unwrap();
        eprintln!("  {pkg}: {max_bump}");
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

        let args = CheckArgs { base: None };
        let result = run_in(dir.path(), &args);
        assert!(result.is_ok());
    }

    #[test]
    fn check_empty_fails() {
        let dir = tempfile::tempdir().unwrap();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();

        let args = CheckArgs { base: None };
        let result = run_in(dir.path(), &args);
        assert!(result.is_err());
    }

    #[test]
    fn check_missing_dir_fails() {
        let dir = tempfile::tempdir().unwrap();
        let args = CheckArgs { base: None };
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
        };
        let result = run_in(dir.path(), &args);
        assert!(result.is_ok());
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
        };
        let result = run_in(dir.path(), &args);
        assert!(result.is_err());
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
        crate::git::diff_changeset_files(dir, "main", ".changeset").unwrap()
    }

    #[test]
    fn check_with_base_counts_untracked_changeset() {
        let dir = feature_branch();
        write_changeset(dir.path(), "untracked.md");

        assert_eq!(files(dir.path()), [".changeset/untracked.md"]);
        let args = CheckArgs {
            base: Some("main".to_string()),
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
}
