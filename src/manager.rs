use anyhow::Error;
use clap::ArgMatches;
use log::{error, info};
use git2::Repository;

pub struct Manager {
    semver: String,
    branch: String,
    repo: Repository
}

impl Manager {
    pub fn new(args: ArgMatches) -> Result<Self, Error> {
        let dir = std::env::current_dir()?;
        match Repository::open(&dir) {
            Ok(repo) => {
                Ok(Self {
                    semver: args.value_of("semver").unwrap_or("minor").to_string(),
                    branch: args.value_of("branch").unwrap_or("master").to_string(),
                    repo
                })
            },
            Err(_e) => {
                error!("Aborting! Repository does not exist at: {:?}", dir.display());
                panic!("Needs to be run in a repository");
            }
        }
    }

    pub fn check_repo_state(&self) -> Result<(), Error> {
        let status = self.repo.state();
        info!("Repo status: {:?}", status);

        Ok(())
        
    }
}
