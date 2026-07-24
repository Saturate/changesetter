pub mod add;
pub mod check;
pub mod init;
pub mod release;
pub mod status;
pub mod version;

use clap::{Parser, Subcommand};

pub use add::AddArgs;
pub use check::CheckArgs;
pub use init::InitArgs;
pub use release::ReleaseArgs;
pub use status::StatusArgs;
pub use version::VersionArgs;

#[derive(Parser)]
#[command(
    name = "changesetter",
    version,
    about = "Polyglot changeset management CLI"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Initialize a repo for changesetter
    Init(InitArgs),
    /// Create a new changeset
    Add(AddArgs),
    /// Verify that at least one changeset exists
    Check(CheckArgs),
    /// Show pending changesets and what would happen on release
    Status(StatusArgs),
    /// Bump versions and update changelogs
    Version(VersionArgs),
    /// Run the full release pipeline
    Release(ReleaseArgs),
}
