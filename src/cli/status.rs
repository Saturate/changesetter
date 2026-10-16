use std::path::Path;

use clap::Args;

use crate::changeset::reader;
use crate::config::Config;
use crate::package::detector;
use crate::release::plan;

#[derive(Args)]
pub struct StatusArgs {
    /// Output format (json for CI consumption)
    #[arg(long)]
    pub output: Option<String>,
}

pub fn run(args: &StatusArgs) -> anyhow::Result<()> {
    let repo_root = crate::git::find_repo_root()?;
    if args.output.as_deref() == Some("json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&json_status(&repo_root)?)?
        );
        return Ok(());
    }
    run_in(&repo_root)
}

/// Pending changesets, every package's current version and tag, and what a
/// release would do. CI reads this instead of globbing the changeset dir or
/// guessing tag names, so it follows `changeset_dir` and the tag format.
pub fn json_status(repo_root: &Path) -> anyhow::Result<serde_json::Value> {
    let config = Config::load(repo_root)?;
    let changeset_dir = config.changeset_dir(repo_root);
    let changesets = reader::read_changesets(&changeset_dir)?;
    let packages = detector::detect_packages(repo_root, &config)?;
    let is_monorepo = packages.len() > 1;

    let package_entries = packages
        .iter()
        .map(|p| {
            let version = p.version.to_string();
            let tag = config.tag.format_tag(&p.name, &version, is_monorepo);
            let path = p.path.strip_prefix(repo_root).unwrap_or(&p.path);
            Ok(serde_json::json!({
                "name": p.name,
                "version": version,
                "path": path.to_string_lossy(),
                "tagged": crate::git::tag_exists(repo_root, &tag)?,
                "tag": tag,
            }))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let (releases, none_entries) = if changesets.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        let pre_state = crate::release::pre::read_pre_state(&changeset_dir);
        let release_plan = plan::assemble(&changesets, &packages, &config, pre_state.as_ref());
        let releases = release_plan
            .releases
            .iter()
            .map(|r| {
                let version = r.version.to_string();
                serde_json::json!({
                    "name": r.name,
                    "previous_version": r.previous_version.to_string(),
                    "version": version,
                    "bump": r.bump.to_string(),
                    "tag": config.tag.format_tag(&r.name, &version, is_monorepo),
                })
            })
            .collect();
        let none_entries = release_plan
            .none_entries
            .iter()
            .map(|n| serde_json::json!({ "title": n.title }))
            .collect();
        (releases, none_entries)
    };

    let pending: Vec<_> = changesets
        .iter()
        .filter_map(|c| c.filename.clone())
        .collect();

    Ok(serde_json::json!({
        "pending": pending,
        "packages": package_entries,
        "releases": releases,
        "none_entries": none_entries,
    }))
}

pub fn run_in(repo_root: &Path) -> anyhow::Result<()> {
    let config = Config::load(repo_root)?;
    let changeset_dir = config.changeset_dir(repo_root);
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
        Command::new("git")
            .args(["config", "tag.gpgsign", "false"])
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

    #[test]
    fn json_without_changesets() {
        let dir = setup_repo();
        let status = json_status(dir.path()).unwrap();
        assert_eq!(status["pending"], serde_json::json!([]));
        assert_eq!(status["releases"], serde_json::json!([]));
        assert_eq!(
            status["packages"],
            serde_json::json!([{
                "name": "testpkg",
                "version": "1.0.0",
                "path": "",
                "tag": "v1.0.0",
                "tagged": false,
            }])
        );
    }

    #[test]
    fn json_with_changesets_and_tag() {
        let dir = setup_repo();
        Command::new("git")
            .args(["tag", "v1.0.0"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let nested = dir.path().join(".changeset/team");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            nested.join("test.md"),
            "---\ntestpkg: minor\n---\n\n#### New feature\n",
        )
        .unwrap();
        std::fs::write(dir.path().join(".changeset/README.md"), "# Changesets\n").unwrap();

        let status = json_status(dir.path()).unwrap();
        assert_eq!(status["pending"], serde_json::json!(["team/test"]));
        assert_eq!(status["packages"][0]["tagged"], true);
        assert_eq!(
            status["releases"],
            serde_json::json!([{
                "name": "testpkg",
                "previous_version": "1.0.0",
                "version": "1.1.0",
                "bump": "minor",
                "tag": "v1.1.0",
            }])
        );
    }
}
