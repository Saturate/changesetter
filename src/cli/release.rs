use std::path::Path;

use clap::Args;

use crate::release::executor::{self, ExecuteOptions, ExecuteResult};

#[derive(Args)]
pub struct ReleaseArgs {
    /// Show what would happen without modifying anything
    #[arg(long)]
    pub dry_run: bool,

    /// Make changes but don't commit or tag
    #[arg(long)]
    pub no_commit: bool,

    /// Output format (json for GitHub Action consumption)
    #[arg(long)]
    pub output: Option<String>,
}

pub fn run(args: &ReleaseArgs) -> anyhow::Result<()> {
    let repo_root = crate::git::find_repo_root()?;
    run_in(&repo_root, args)
}

pub fn run_in(repo_root: &Path, args: &ReleaseArgs) -> anyhow::Result<()> {
    let opts = ExecuteOptions {
        dry_run: args.dry_run,
        no_commit: args.no_commit,
        snapshot: None,
        origins: args.output.as_deref() == Some("json"),
    };

    let result = executor::execute_version(repo_root, &opts)?;

    // Tools reading `--output json` get JSON even when nothing is released, so
    // they never have to tell an empty stdout from a failure.
    if !result.plan.releases.is_empty() && !args.dry_run && !args.no_commit {
        create_tags(repo_root, &result)?;
    }

    if args.output.as_deref() == Some("json") {
        print_json_output(&result)?;
    }

    Ok(())
}

fn create_tags(repo_root: &Path, result: &ExecuteResult) -> anyhow::Result<()> {
    for release in &result.plan.releases {
        let tag = result.config.tag.format_tag(
            &release.name,
            &release.version.to_string(),
            result.is_monorepo,
        );

        let message = if result.config.release.tag_annotated {
            Some(release.changelog.as_str())
        } else {
            None
        };

        crate::git::git_tag(repo_root, &tag, message)?;
        eprintln!("Tagged {tag}");
    }

    Ok(())
}

fn print_json_output(result: &ExecuteResult) -> anyhow::Result<()> {
    let releases: Vec<serde_json::Value> = result
        .plan
        .releases
        .iter()
        .map(|r| {
            let tag =
                result
                    .config
                    .tag
                    .format_tag(&r.name, &r.version.to_string(), result.is_monorepo);
            serde_json::json!({
                "name": r.name,
                "version": r.version.to_string(),
                "previous_version": r.previous_version.to_string(),
                "bump": r.bump.to_string(),
                "tag": tag,
                "changelog": r.changelog,
                "changesets": r.changesets,
                "entries": r.entries.iter().map(|e| e.to_json()).collect::<Vec<_>>(),
            })
        })
        .collect();

    let none_entries: Vec<serde_json::Value> = result
        .plan
        .none_entries
        .iter()
        .map(|n| n.to_json())
        .collect();

    let output = serde_json::json!({
        "releases": releases,
        "none_entries": none_entries,
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
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
            "[package]\nname = \"testpkg\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir_all(&changeset_dir).unwrap();
        std::fs::write(
            changeset_dir.join("test.md"),
            "---\ntestpkg: minor\n---\n\n#### New feature\n",
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
    fn release_creates_tag() {
        let dir = setup_repo();
        let args = ReleaseArgs {
            dry_run: false,
            no_commit: false,
            output: None,
        };
        run_in(dir.path(), &args).unwrap();

        let output = Command::new("git")
            .args(["tag", "-l"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let tags = String::from_utf8_lossy(&output.stdout);
        assert!(tags.contains("v1.1.0"), "tags: {tags}");
    }

    #[test]
    fn release_dry_run_no_tag() {
        let dir = setup_repo();
        let args = ReleaseArgs {
            dry_run: true,
            no_commit: false,
            output: None,
        };
        run_in(dir.path(), &args).unwrap();

        let output = Command::new("git")
            .args(["tag", "-l"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let tags = String::from_utf8_lossy(&output.stdout);
        assert!(tags.trim().is_empty());
    }

    #[test]
    fn release_no_commit_no_tag() {
        let dir = setup_repo();
        let args = ReleaseArgs {
            dry_run: false,
            no_commit: true,
            output: None,
        };
        run_in(dir.path(), &args).unwrap();

        let output = Command::new("git")
            .args(["tag", "-l"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let tags = String::from_utf8_lossy(&output.stdout);
        assert!(tags.trim().is_empty());
    }
}
