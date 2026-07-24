use clap::Parser;

use changesetter::cli::{Cli, Command};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Command::Init(args) => changesetter::cli::init::run(args),
        Command::Add(args) => changesetter::cli::add::run(args),
        Command::Check(args) => changesetter::cli::check::run(args),
        Command::Status(args) => changesetter::cli::status::run(args),
        Command::Version(args) => changesetter::cli::version::run(args),
        Command::Release(args) => changesetter::cli::release::run(args),
    }
}
