

mod crypto;
mod db;
mod statics;
mod server;

use std::{path::PathBuf};

use async_std::task::block_on;
use structopt::StructOpt;

use db::VaultExt as _;

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
    // #[structopt(setting(structopt::clap::AppSettings::Hidden))] // Not yet implemented.
    // Upgrade(UpgradeCommand),
}

#[derive(StructOpt)]
#[structopt(about = "Open the database for writing/reading")]
struct OpenCommand {
    #[structopt(parse(from_os_str))]
    sqlite_file: PathBuf,
}

impl OpenCommand {
    fn run(&self, opts: &VaultOpts) -> anyhow::Result<()> {
        block_on(server::async_run_server(opts, self))
    }
}

#[derive(StructOpt)]
#[structopt(about = "Initialize a new database file")]

struct InitCommand { 
    #[structopt(parse(from_os_str))]
    sqlite_file: PathBuf,
}

impl InitCommand {
    fn run(&self, _opts: &VaultOpts) -> anyhow::Result<()> {
        let db = block_on(db::create_db(&self.sqlite_file))?;

        let secret = crypto::SealedBoxPrivateKey::generate();
        let pub_key = secret.public().to_string();
        block_on(db.write_setting(db::SETTING_PUBLIC_KEY, &pub_key))?;
        block_on(db.close());
        println!("OK. Database initialized.");
        println!("Your PRIVATE KEY (password) is: {}", secret);
        println!("You must save this. There is no way to recover or reset it.");

        Ok(())
    }
}

// #[derive(StructOpt)]
// #[structopt(about = "Upgrade database schema to a new version")]
// struct UpgradeCommand {
//     #[structopt(parse(from_os_str))]
//     sqlite_file: PathBuf,
// }

// impl UpgradeCommand {
//     fn run(&self, opts: &VaultOpts) -> anyhow::Result<()> {
//         todo!("Implement impl UpgradeCommand");
//     }
// }

impl VaultOpts {
    fn run(&self) -> anyhow::Result<()> {
        match &self.command {
            MainCommands::Init(cmd) => cmd.run(&self),
            MainCommands::Open(cmd) => cmd.run(&self),
            // MainCommands::Upgrade(cmd) => cmd.run(&self),
        }
    }
}