use std::collections::BTreeMap;
use std::path::Path;

use clap::{Args, Subcommand};

use crate::release::pre::{self, PreState};

#[derive(Args)]
pub struct PreArgs {
    #[command(subcommand)]
    pub command: PreCommand,
}

#[derive(Subcommand)]
pub enum PreCommand {
    /// Enter pre-release mode with a tag (e.g. rc, beta, alpha)
    Enter {
        /// Pre-release tag (e.g. rc, beta, alpha)
        tag: String,
    },
    /// Exit pre-release mode (next release will be stable)
    Exit,
    /// Show current pre-release state
    Status,
}

pub fn run(args: &PreArgs) -> anyhow::Result<()> {
    let repo_root = crate::git::find_repo_root()?;
    run_in(&repo_root, args)
}

pub fn run_in(repo_root: &Path, args: &PreArgs) -> anyhow::Result<()> {
    let changeset_dir = repo_root.join(".changeset");
    if !changeset_dir.exists() {
        std::fs::create_dir_all(&changeset_dir)?;
    }

    match &args.command {
        PreCommand::Enter { tag } => enter(&changeset_dir, tag),
        PreCommand::Exit => exit(&changeset_dir),
        PreCommand::Status => status(&changeset_dir),
    }
}

fn enter(changeset_dir: &Path, tag: &str) -> anyhow::Result<()> {
    if let Some(state) = pre::read_pre_state(changeset_dir) {
        if state.mode == "pre" {
            anyhow::bail!(
                "Already in pre-release mode with tag \"{}\". Run `changesetter pre exit` first.",
                state.tag
            );
        }
    }

    let state = PreState {
        mode: "pre".to_string(),
        tag: tag.to_string(),
        packages_released: BTreeMap::new(),
    };
    pre::write_pre_state(changeset_dir, &state)?;
    eprintln!("Entered pre-release mode with tag \"{tag}\"");
    Ok(())
}

fn exit(changeset_dir: &Path) -> anyhow::Result<()> {
    let Some(mut state) = pre::read_pre_state(changeset_dir) else {
        anyhow::bail!("Not in pre-release mode. Run `changesetter pre enter <tag>` first.");
    };

    if state.mode != "pre" {
        anyhow::bail!("Not in pre-release mode. Run `changesetter pre enter <tag>` first.");
    }

    state.mode = "exit".to_string();
    pre::write_pre_state(changeset_dir, &state)?;
    eprintln!("Exiting pre-release mode. Next release will be stable.");
    Ok(())
}

fn status(changeset_dir: &Path) -> anyhow::Result<()> {
    match pre::read_pre_state(changeset_dir) {
        Some(state) if state.mode == "pre" => {
            println!("Pre-release mode: active");
            println!("Tag: {}", state.tag);
            if !state.packages_released.is_empty() {
                println!("Packages released:");
                for (name, count) in &state.packages_released {
                    println!("  {name}: {count} pre-release(s)");
                }
            }
        }
        Some(state) if state.mode == "exit" => {
            println!("Pre-release mode: exiting");
            println!("Tag: {}", state.tag);
            println!("Next release will produce stable versions.");
        }
        _ => {
            println!("Not in pre-release mode");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_creates_pre_json() {
        let dir = tempfile::tempdir().unwrap();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();

        let args = PreArgs {
            command: PreCommand::Enter {
                tag: "rc".to_string(),
            },
        };
        run_in(dir.path(), &args).unwrap();

        let state = pre::read_pre_state(&changeset_dir).unwrap();
        assert_eq!(state.mode, "pre");
        assert_eq!(state.tag, "rc");
        assert!(state.packages_released.is_empty());
    }

    #[test]
    fn enter_twice_errors() {
        let dir = tempfile::tempdir().unwrap();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();

        let args = PreArgs {
            command: PreCommand::Enter {
                tag: "rc".to_string(),
            },
        };
        run_in(dir.path(), &args).unwrap();
        let result = run_in(dir.path(), &args);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Already in pre-release mode"), "got: {err}");
    }

    #[test]
    fn exit_sets_mode() {
        let dir = tempfile::tempdir().unwrap();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();

        run_in(
            dir.path(),
            &PreArgs {
                command: PreCommand::Enter {
                    tag: "beta".to_string(),
                },
            },
        )
        .unwrap();

        run_in(
            dir.path(),
            &PreArgs {
                command: PreCommand::Exit,
            },
        )
        .unwrap();

        let state = pre::read_pre_state(&changeset_dir).unwrap();
        assert_eq!(state.mode, "exit");
        assert_eq!(state.tag, "beta");
    }

    #[test]
    fn exit_without_enter_errors() {
        let dir = tempfile::tempdir().unwrap();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();

        let result = run_in(
            dir.path(),
            &PreArgs {
                command: PreCommand::Exit,
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn status_not_in_pre_mode() {
        let dir = tempfile::tempdir().unwrap();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();

        let result = run_in(
            dir.path(),
            &PreArgs {
                command: PreCommand::Status,
            },
        );
        assert!(result.is_ok());
    }

    #[test]
    fn status_in_pre_mode() {
        let dir = tempfile::tempdir().unwrap();
        let changeset_dir = dir.path().join(".changeset");
        std::fs::create_dir(&changeset_dir).unwrap();

        run_in(
            dir.path(),
            &PreArgs {
                command: PreCommand::Enter {
                    tag: "rc".to_string(),
                },
            },
        )
        .unwrap();

        let result = run_in(
            dir.path(),
            &PreArgs {
                command: PreCommand::Status,
            },
        );
        assert!(result.is_ok());
    }
}
