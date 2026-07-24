use clap::Args;

use crate::release::executor::{self, ExecuteOptions};

#[derive(Args)]
pub struct VersionArgs {
    /// Show what would change without writing
    #[arg(long)]
    pub dry_run: bool,

    /// Update files but don't git commit
    #[arg(long)]
    pub no_commit: bool,

    /// Create a snapshot version for CI/preview deploys
    #[arg(long)]
    pub snapshot: Option<String>,
}

pub fn run(args: &VersionArgs) -> anyhow::Result<()> {
    let repo_root = crate::git::find_repo_root()?;

    let opts = ExecuteOptions {
        dry_run: args.dry_run,
        no_commit: args.no_commit,
        snapshot: args.snapshot.clone(),
    };

    executor::execute_version(&repo_root, &opts)?;
    Ok(())
}
