use clap::Args;

#[derive(Args)]
pub struct StatusArgs {}

pub fn run(_args: &StatusArgs) -> anyhow::Result<()> {
    anyhow::bail!("not yet implemented")
}
