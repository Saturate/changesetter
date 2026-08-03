use clap::Parser;

use changesetter::cli::{Cli, Command};
use changesetter::errors::ChangesetterError;

fn main() {
    let cli = Cli::parse();

    let result = match &cli.command {
        Command::Init(args) => changesetter::cli::init::run(args),
        Command::Add(args) => changesetter::cli::add::run(args),
        Command::Check(args) => changesetter::cli::check::run(args),
        Command::Status(args) => changesetter::cli::status::run(args),
        Command::Version(args) => changesetter::cli::version::run(args),
        Command::Release(args) => changesetter::cli::release::run(args),
        Command::Pre(args) => changesetter::cli::pre::run(args),
    };

    if let Err(e) = result {
        let code = if let Some(ce) = e.downcast_ref::<ChangesetterError>() {
            match ce {
                ChangesetterError::NoChangesets
                | ChangesetterError::InvalidFrontmatter { .. }
                | ChangesetterError::UnknownPackage { .. }
                | ChangesetterError::UnknownBumpLevel { .. } => 1,
                ChangesetterError::DirtyWorkingTree
                | ChangesetterError::GitNotFound
                | ChangesetterError::NotAGitRepo
                | ChangesetterError::BaseRefUnavailable { .. } => 2,
                _ => 1,
            }
        } else {
            1
        };

        eprintln!("error: {e}");
        std::process::exit(code);
    }
}
