use clap::Args;

#[derive(Args)]
pub struct AddArgs {
    /// Package(s) affected by this change
    #[arg(long)]
    pub package: Vec<String>,

    /// Bump level (patch, minor, major)
    #[arg(long)]
    pub bump: Option<String>,

    /// Create a none-bump changeset (no version change)
    #[arg(long)]
    pub no_bump: bool,

    /// Change description (non-interactive mode)
    #[arg(long, short)]
    pub message: Option<String>,
}

pub fn run(_args: &AddArgs) -> anyhow::Result<()> {
    anyhow::bail!("not yet implemented")
}
