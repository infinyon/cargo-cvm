mod manager;

use anyhow::Error;
use clap::{crate_authors, crate_description, crate_version, App, Arg, SubCommand};
use log::info;
use manager::Manager;

fn main() -> Result<(), Error> {
    env_logger::init();

    let args = App::new("Cargo Version Manager (cargo vsm)")
        // Need to run this as a sub-command since we are extending cargo
        .subcommand(SubCommand::with_name("vsm")

        .version(crate_version!())
        .author(crate_authors!())
        .about(crate_description!())

        .arg(Arg::with_name("semver")
            .short("s")
            .value_name("semver")
            .help("Type of Semantic Versioning; i.e. `minor`, `major`, or `patch`. Defaults to `minor`")
            .default_value("minor"))

        .arg(Arg::with_name("branch")
            .short("b")
            .value_name("branch")
            .help("Which branch to compare to the current. Will attempt to find the version in the target branch and check if the version has been bumped or not.")
            .takes_value(true)))
        .get_matches();

    // Search for Cargo.toml files, parse the version from the target branch vs. the current version;
    // Run the operations in a temporary directory when cloning; requires there to be a remote git
    // that can be cloned.

    // Local checking can be done to, however, it requires that the current branch is in a clean state with changes committed.
    // Look for Cargo.toml workspace first, this will show where all potential targets exist and their paths;
    // Check if a workspace has changed if one exists. If one has not existed before, we can ignore;

    Manager::new(args)?.check_repo_state()?;

    Ok(())
}
