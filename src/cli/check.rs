use std::path::Path;

use clap::Args;

use crate::changeset::reader;
use crate::errors::ChangesetterError;

#[derive(Args)]
pub struct CheckArgs {
    /// Base branch to compare against
    #[arg(long)]
    pub base: Option<String>,
}

pub fn run(args: &CheckArgs) -> anyhow::Result<()> {
    let repo_root = crate::git::find_repo_root()?;
    run_in(&repo_root, args)
}

pub fn run_in(repo_root: &Path, args: &CheckArgs) -> anyhow::Result<()> {
    let changeset_dir = repo_root.join(".changeset");

    if let Some(base) = &args.base {
        check_with_base(repo_root, base)
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

fn check_with_base(repo_root: &Path, base: &str) -> anyhow::Result<()> {
    let files = crate::git::diff_changeset_files(repo_root, base)?;

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
            .args(["init", "-q"])
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
}
