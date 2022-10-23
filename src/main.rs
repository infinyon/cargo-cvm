mod manager;

use anyhow::Error;
use clap::Parser;
use manager::{Manager, SemVer};

/// Rust Crate Version Manage (CVM)
#[derive(Parser, Default)]
#[command(about, author, version)]
pub struct Args {
    /// Type of Semantic Versioning
    #[arg(short, long, value_enum, default_value_t)]
    pub semver: SemVer,

    /// Which branch to compare to the current. Will attempt to find the version in the target branch and check if the version has been bumped or not
    #[arg(short, long, default_value_t = String::from("master"))]
    pub branch: String,

    /// Determine which remote to use for the target branch
    #[arg(short, long, default_value_t = String::from("origin"))]
    pub remote: String,

    /// Provide the path to your ssh private key for authenticating against remote git hosts. Defaults to $HOME/.ssh/id_rsa
    #[arg(short = 'k', long = "ssh-key")]
    pub ssh_key_path: Option<String>,

    /// Automatically fix the version if it is outdated. By default, this will bump the minor version, unless otherwise specified by the --semver option
    #[arg(short, long)]
    pub fix: bool,

    /// Force a version bump. Can use be used with --semver option to determine version type
    #[arg(short = 'F', long)]
    pub force: bool,

    /// Panic if the versions are out-of-date
    #[arg(short = 'x', long)]
    pub check: bool,

    /// Warn if the versions are out-of-date
    #[arg(short, long)]
    pub warn: bool,

    /// git commit updated version(s), otherwise will only add the files to git. Can only be used with --fix or --force flags
    #[arg(short, long)]
    pub commit: bool,
}

fn main() -> Result<(), Error> {
    env_logger::init();

    // Filter out `cvm` subcommand when used from Cargo
    let args = std::env::args()
        .enumerate()
        .filter_map(|(i, arg)| {
            if i == 1 && arg == "cvm" {
                None
            } else {
                Some(arg)
            }
        });
    let args = Args::parse_from(args);
    let manager = Manager::new(args)?;
    manager.check_workspaces()
}