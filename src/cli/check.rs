use clap::Args;

#[derive(Args)]
pub struct CheckArgs {
    /// Base branch to compare against
    #[arg(long)]
    pub base: Option<String>,
}

pub fn run(_args: &CheckArgs) -> anyhow::Result<()> {
    anyhow::bail!("not yet implemented")
}
