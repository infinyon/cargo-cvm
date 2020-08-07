use anyhow::Error;
use clap::ArgMatches;
use log::{error, info};
use std::path::PathBuf;
use std::process::{Command};
use std::fs::{read_to_string};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Manager {
    semver: String,
    target_branch: String,
    current_branch: String,
    workspaces: Vec<String>,
    dir: PathBuf
}

impl Manager {
    pub fn new(args: ArgMatches) -> Result<Self, Error> {
        let dir = std::env::current_dir()?;
        Ok(Self {
            semver: args.value_of("semver").unwrap_or("minor").to_string(),
            target_branch: args.value_of("branch").unwrap_or("master").to_string(),
            current_branch: Self::get_current_branch()?,
            workspaces: Self::get_cargo_workspace()?,
            dir
        })
    }

    pub fn get_current_branch() -> Result<String, Error> {
        let output = Command::new("git").args(&["branch", "--show-current"]).output()?;

        if !output.status.success() {
            panic!("Failed to find current branch; ensure you're on git version >2.22");
        }

        let branch = std::str::from_utf8(&output.stdout)?;

        info!("Current Branch: {}", branch);

        Ok(branch.to_string())
    }

    pub fn get_cargo_workspace() -> Result<Vec<String>, Error> {
        let mut cargo_toml = std::env::current_dir()?;
        cargo_toml.push("Cargo.toml");

        info!("Cargo Path: {:?}", cargo_toml.display());

        if !cargo_toml.exists() {
            panic!("`cargo cvm` must be run in a directory containing a `Cargo.toml` file.\nFile does not exist at: {:?}", cargo_toml.display())
        }

        let config: CargoConfig = toml::from_str(&read_to_string(&cargo_toml)?)?;
        let mut paths: Vec<String> = Vec::new();

        if config.package.is_some() {
            let dir = std::env::current_dir()?;
            dir.to_str().map(|path| paths.push(String::from(path)));
        }
        
        if let Some(workspace) = config.workspace {
            paths.extend(workspace.members.into_iter())
        }

        Ok(paths)
    }

    pub fn check_workspaces(&self) -> Result<(), Error> {
        // For each of the workspace directories, check if any files in the src directory have changed;
        for workspace in self.workspaces.iter() {
            if self.is_workspace_updated(PathBuf::from(workspace))? {
                // workspace has changes, check if the version has been incremented!
                if !self.is_workspace_version_updated(PathBuf::from(workspace))? {
                    // Failed to find workspace version updated;
                    panic!("Version is not updated for workspace with changes: {:?}", workspace);
                }
            }
        }


        Ok(())
        
    }

    pub fn is_workspace_updated(&self, workspace: PathBuf) -> Result<bool, Error> {
        let mut src_dir = workspace.clone();
        
        // Only check the src directory;
        src_dir.push("src");

        if !src_dir.exists() || !src_dir.is_dir() {
            panic!("src directory does not exist at {:?}", src_dir.display())
        }

        let compare = format!("{}..{}", self.target_branch, self.current_branch);
        let args = &["diff", &compare.trim(), "--", &src_dir.display().to_string()];
        let output = Command::new("git").args(args).output()?;

        if !output.status.success() {
            panic!("Command failed: `git {:?}`", args);
        }

        let changes = std::str::from_utf8(&output.stdout)?;

        Ok(!changes.is_empty())
    }

    pub fn is_workspace_version_updated(&self, workspace: PathBuf) -> Result<bool, Error> {
        let mut cargo_toml = workspace.clone();
        
        // Only check the src directory;
        cargo_toml.push("Cargo.toml");

        if !cargo_toml.exists() || !cargo_toml.is_file() {
            panic!("Cargo.toml file does not exist at {:?}", cargo_toml.display())
        }

        let compare = format!("{}..{}", self.target_branch, self.current_branch);
        let args = &["diff", &compare.trim(), "--", &cargo_toml.display().to_string()];
        let output = Command::new("git").args(args).output()?;

        if !output.status.success() {
            panic!("Command failed: `git {:?}`", args);
        }

        let changes = String::from(std::str::from_utf8(&output.stdout)?);

        Ok(changes.contains("+version="))
    }
}


#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CargoPackage {
    name: String,
    version: String,
    authors: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CargoWorkspace {
    members: Vec<String>
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CargoConfig {
    package: Option<CargoPackage>,
    workspace: Option<CargoWorkspace>
}