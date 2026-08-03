use std::path::Path;

use clap::Args;

use crate::changeset::reader;
use crate::config::Config;
use crate::package::detector;
use crate::release::plan;

#[derive(Args)]
pub struct StatusArgs {}

pub fn run(_args: &StatusArgs) -> anyhow::Result<()> {
    let repo_root = crate::git::find_repo_root()?;
    run_in(&repo_root)
}

pub fn run_in(repo_root: &Path) -> anyhow::Result<()> {
    let config = Config::load(repo_root)?;
    let changeset_dir = repo_root.join(".changeset");
    let changesets = reader::read_changesets(&changeset_dir)?;

    if changesets.is_empty() {
        println!("No pending changesets");
        return Ok(());
    }

    let packages = detector::detect_packages(repo_root, &config)?;
    let pre_state = crate::release::pre::read_pre_state(&changeset_dir);
    let release_plan = plan::assemble(&changesets, &packages, &config, pre_state.as_ref());

    println!("{} changeset(s) pending\n", changesets.len());

    if !release_plan.releases.is_empty() {
        println!("Releases:");
        for r in &release_plan.releases {
            println!(
                "  {} {} -> {} ({})",
                r.name, r.previous_version, r.version, r.bump
            );
        }
    }

    if !release_plan.none_entries.is_empty() {
        println!("\nInternal (no version change):");
        for n in &release_plan.none_entries {
            println!("  {}", n.title);
        }
    }

    Ok(())
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
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"testpkg\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-q", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        dir
    }

    #[test]
    fn status_no_changesets() {
        let dir = setup_repo();
        let result = run_in(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn status_with_changesets() {
        let dir = setup_repo();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();
        std::fs::write(
            changeset_dir.join("test.md"),
            "---\ntestpkg: minor\n---\n\n#### New feature\n",
        )
        .unwrap();

        let result = run_in(dir.path());
        assert!(result.is_ok());
    }
}
