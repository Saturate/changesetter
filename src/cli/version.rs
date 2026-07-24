use clap::Args;

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

pub fn run(_args: &VersionArgs) -> anyhow::Result<()> {
    anyhow::bail!("not yet implemented")
}
