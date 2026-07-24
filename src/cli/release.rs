use clap::Args;

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

pub fn run(_args: &ReleaseArgs) -> anyhow::Result<()> {
    anyhow::bail!("not yet implemented")
}
