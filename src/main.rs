mod manager;

use anyhow::Error;
use clap::{crate_authors, crate_description, crate_version, App, Arg, SubCommand};
use manager::Manager;

fn main() -> Result<(), Error> {
    env_logger::init();

    if let Some(args) = App::new("Rust Crate Version Manage (CVM)")
        .version(crate_version!())
        .author(crate_authors!())
        .about(crate_description!())
        .subcommand(
            SubCommand::with_name("cvm")
                .arg(
                    Arg::with_name("semver")
                        .short("s")
                        .long("semver")
                        .help("Type of Semantic Versioning; i.e. `minor`, `major`, or `patch`. Defaults to `minor`")
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("branch")
                        .short("b")
                        .long("branch")
                        .help("Which branch to compare to the current. Will attempt to find the version in the target branch and check if the version has been bumped or not.")
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("remote")
                        .short("r")
                        .long("remote")
                        .help("Determine which remote to use for the target branch. Defaults to `origin`.")
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("ssh-key")
                        .short("k")
                        .long("ssh-key")
                        .help("Provide the path to your ssh private key for authenticating against remote git hosts. Defaults to $HOME/.ssh/id_rsa")
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("fix")
                        .short("f")
                        .long("fix")
                        .takes_value(false)
                        .help("Automatically fix the version if it is outdated. By default, this will bump the minor version, unless otherwise specified by the --semver option"))
                .arg(
                    Arg::with_name("force")
                        .short("F")
                        .long("force")
                        .takes_value(false)
                        .help("Force a version bump. Can use be used with --semver option to determine version type"),
                )
                .arg(
                    Arg::with_name("check")
                        .short("x")
                        .long("check")
                        .takes_value(false)
                        .help("Panic if the versions are out-of-date"),
                )
                .arg(
                    Arg::with_name("warn")
                        .short("w")
                        .long("warn")
                        .takes_value(false)
                        .help("Warn if the versions are out-of-date"),
                )
                .arg(
                    Arg::with_name("commit")
                        .short("c")
                        .long("commit")
                        .takes_value(false)
                        .help("git commit updated version(s), otherwise will only add the files to git. Can only be used with --fix or --force flags"),
                ),
        )
        .get_matches()
        .subcommand_matches("cvm")
    {
        let manager = Manager::new(args)?;
        manager.check_workspaces()?;
    };

    Ok(())
}
