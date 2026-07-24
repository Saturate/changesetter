use clap::Args;

#[derive(Args)]
pub struct InitArgs {
    /// Generate a starter changesetter.toml
    #[arg(long)]
    pub config: bool,
}

pub fn run(_args: &InitArgs) -> anyhow::Result<()> {
    anyhow::bail!("not yet implemented")
}
