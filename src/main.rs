use std::{path::PathBuf};

use structopt::StructOpt;

mod statics;
mod server;


fn main() -> anyhow::Result<()> {
    VaultOpts::from_args().run()
}

#[derive(StructOpt)]
#[structopt(name = "vault", about = "A secure place to store your thoughts")]
struct VaultOpts {
    #[structopt(short,long,parse(from_occurrences))]
    verbose: u8,

    #[structopt(subcommand)]
    command: MainCommands,
}

#[derive(StructOpt)]
enum MainCommands {
    Open(OpenCommand),
    Init(InitCommand),
    Upgrade(UpgradeCommand),
}

#[derive(StructOpt)]
#[structopt(about = "Open the database for writing/reading")]
struct OpenCommand {
    #[structopt(parse(from_os_str))]
    sqlite_file: PathBuf,
}

impl OpenCommand {
    fn run(&self, opts: &VaultOpts) -> anyhow::Result<()> {
        async_std::task::block_on(server::async_run_server(opts))
    }
}

#[derive(StructOpt)]
#[structopt(about = "Initialize a new database file")]

struct InitCommand { 
    #[structopt(parse(from_os_str))]
    sqlite_file: PathBuf,
}

impl InitCommand {
    fn run(&self, opts: &VaultOpts) -> anyhow::Result<()> {
        todo!("Implement InitCommand");
    }
}

#[derive(StructOpt)]
#[structopt(about = "Upgrade database schema to a new version")]

struct UpgradeCommand {
    #[structopt(parse(from_os_str))]
    sqlite_file: PathBuf,
}

impl UpgradeCommand {
    fn run(&self, opts: &VaultOpts) -> anyhow::Result<()> {
        todo!("Implement impl UpgradeCommand");
    }
}

impl VaultOpts {
    fn run(&self) -> anyhow::Result<()> {
        match &self.command {
            MainCommands::Init(cmd) => cmd.run(&self),
            MainCommands::Open(cmd) => cmd.run(&self),
            MainCommands::Upgrade(cmd) => cmd.run(&self),
        }
    }
}