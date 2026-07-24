use clap::Args;

#[derive(Args)]
pub struct InitArgs {
    /// Generate a starter changesetter.toml
    #[arg(long)]
    pub config: bool,
}

pub fn run(args: &InitArgs) -> anyhow::Result<()> {
    let repo_root = crate::git::find_repo_root()?;
    run_in(&repo_root, args)
}

pub fn run_in(repo_root: &std::path::Path, args: &InitArgs) -> anyhow::Result<()> {
    let changeset_dir = repo_root.join(".changeset");

    if changeset_dir.exists() {
        eprintln!(".changeset/ directory already exists");
    } else {
        std::fs::create_dir_all(&changeset_dir)?;
        eprintln!("Created .changeset/ directory");
    }

    if args.config {
        let config_path = repo_root.join("changesetter.toml");
        if config_path.exists() {
            eprintln!("changesetter.toml already exists, skipping");
        } else {
            std::fs::write(&config_path, STARTER_CONFIG)?;
            eprintln!("Created changesetter.toml");
        }
    }

    Ok(())
}

const STARTER_CONFIG: &str = r#"# changesetter.toml
# See https://github.com/saturate/changesetter for documentation

# Override auto-detected packages (optional)
# [[package]]
# name = "mylib"
# path = "crates/mylib"
# type = "cargo"

# Ignore packages from versioning
# ignore = ["examples", "internal-tools"]

# Changelog settings
# [changelog]
# file = "CHANGELOG.md"
# per_package = false
# none_bump = "section"
# none_bump_heading = "Internal"

# Tag format
# [tag]
# format = "v{version}"          # single-package
# format = "{name}@v{version}"   # monorepo

# Release settings
# [release]
# commit_message = "chore: release {versions}"
# tag_annotated = true

# Post-bump hooks
# [hooks]
# post_bump = ["cargo check"]
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_creates_changeset_dir() {
        let dir = tempfile::tempdir().unwrap();
        let args = InitArgs { config: false };
        run_in(dir.path(), &args).unwrap();
        assert!(dir.path().join(".changeset").exists());
    }

    #[test]
    fn init_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let args = InitArgs { config: false };
        run_in(dir.path(), &args).unwrap();
        run_in(dir.path(), &args).unwrap();
        assert!(dir.path().join(".changeset").exists());
    }

    #[test]
    fn init_with_config() {
        let dir = tempfile::tempdir().unwrap();
        let args = InitArgs { config: true };
        run_in(dir.path(), &args).unwrap();
        assert!(dir.path().join(".changeset").exists());
        assert!(dir.path().join("changesetter.toml").exists());
        let content = std::fs::read_to_string(dir.path().join("changesetter.toml")).unwrap();
        assert!(content.contains("changesetter.toml"));
    }
}
