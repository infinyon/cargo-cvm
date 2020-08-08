use anyhow::Error;
use clap::ArgMatches;
use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use std::fs::read_to_string;
use std::fs::{remove_file, File};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct Version {
    major: u8,
    minor: u8,
    patch: u8,
}

impl Version {
    pub fn bump(&mut self, semver: SemVer) -> () {
        match semver {
            SemVer::Major => {
                self.major += 1;
                self.minor = 0;
                self.patch = 0;
            }
            SemVer::Minor => {
                self.minor += 1;
                self.patch = 0;
            }
            SemVer::Patch => self.patch += 1,
        };
    }
}
#[derive(Debug, Clone)]
pub enum SemVer {
    Minor,
    Major,
    Patch,
}

impl TryInto<SemVer> for &str {
    type Error = Error;
    fn try_into(self) -> Result<SemVer, Error> {
        let semver = match self {
            "minor" => SemVer::Minor,
            "major" => SemVer::Major,
            "patch" => SemVer::Patch,
            _ => return Err(Error::msg(format!("Invalid option: {:?}", self))),
        };

        Ok(semver)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl TryInto<Version> for String {
    type Error = Error;
    fn try_into(self) -> Result<Version, Self::Error> {
        let version = self
            .split(".")
            .map(|v| v.parse())
            .collect::<Result<Vec<u8>, std::num::ParseIntError>>()?;

        if version.len() < 3 {
            return Err(Error::msg(format!("Invalid version number: {:?}", version)));
        }

        Ok(Version {
            major: version[0],
            minor: version[1],
            patch: version[2],
        })
    }
}

#[derive(Debug, Clone)]
pub struct Manager {
    semver: SemVer,
    target_branch: String,
    current_branch: String,
    workspaces: Vec<String>,
    check: bool,
    fix: bool,
    warn: bool,
    dir: PathBuf,
}

impl Manager {
    pub fn new(args: &ArgMatches) -> Result<Self, Error> {
        let dir = std::env::current_dir()?;

        Ok(Self {
            dir,
            semver: args.value_of("semver").unwrap_or("minor").try_into()?,
            check: args.is_present("check"),
            fix: args.is_present("fix"),
            warn: args.is_present("warn"),
            target_branch: args.value_of("branch").unwrap_or("master").to_string(),
            current_branch: Self::get_current_branch()?,
            workspaces: Self::get_cargo_workspace()?,
        })
    }

    pub fn get_current_branch() -> Result<String, Error> {
        let output = Command::new("git")
            .args(&["branch", "--show-current"])
            .output()?;

        if !output.status.success() {
            panic!("Failed to find current branch; ensure you're on git version >2.22");
        }

        let branch = std::str::from_utf8(&output.stdout)?;

        Ok(branch.to_string())
    }

    pub fn get_cargo_workspace() -> Result<Vec<String>, Error> {
        let mut cargo_toml = std::env::current_dir()?;
        cargo_toml.push("Cargo.toml");

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

    pub fn bump_version(&self, cargo_toml: PathBuf) -> Result<(), Error> {
        let config = read_to_string(&cargo_toml)?;
        if let Some(pkg) = toml::from_str::<CargoConfig>(&config)?.package {
            let old_version: Version = pkg.version.try_into()?;
            let mut new_version = old_version.clone();
            new_version.bump(self.semver.clone());

            let updated_config = config.replace(&old_version.to_string(), &new_version.to_string());

            // Remove the old version of the file;
            remove_file(&cargo_toml)?;

            // Update the new version;
            let mut file = File::create(&cargo_toml)?;
            file.write_all(updated_config.as_bytes())?;

            // Commit the changes;
            Self::commit_version_update(cargo_toml, new_version.to_string())?;

            Ok(())
        } else {
            panic!("invalid cargo file");
        }
    }

    pub fn commit_version_update(cargo_toml: PathBuf, version: String) -> Result<(), Error> {
        Command::new("git")
            .args(&["add", &cargo_toml.display().to_string()])
            .output()
            .expect("Failed to add updated config");

        let commit_msg = format!("update version to {}", version);
        Command::new("git")
            .args(&["commit", "-m", &commit_msg])
            .output()
            .expect("Failed to add updated config");

        println!("successfully committed version update to {}", version);
        Ok(())
    }

    pub fn check_workspaces(&self) -> Result<(), Error> {
        // For each of the workspace directories, check if any files in the src directory have changed;
        for workspace in self.workspaces.iter() {
            if self.is_workspace_updated(PathBuf::from(workspace))? {
                if let Some(version) = Self::get_workspace_version(PathBuf::from(workspace))? {
                    // workspace has changes, check if the version has been incremented!
                    if !self.is_workspace_version_updated(PathBuf::from(workspace))? {
                        // Failed to find workspace version updated;
                        let mut cargo_toml = PathBuf::from(workspace);
                        cargo_toml.push("Cargo.toml".to_string());

                        let msg = format!(
                            "version {} is not updated for changes in workspace Cargo.toml file: {:?}",
                            version, cargo_toml);

                        if self.check {
                            panic!(msg.clone());
                        } else if self.fix {
                            self.bump_version(cargo_toml)?;
                        } else {
                            println!("{}", &msg);
                        }
                    } else {
                        println!(
                            "version {} is up-to-date with {}!",
                            version, self.target_branch
                        );
                    }
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
        let args = &[
            "diff",
            &compare.trim(),
            "--",
            &src_dir.display().to_string(),
        ];
        let output = Command::new("git").args(args).output()?;

        if !output.status.success() {
            panic!("Command failed: `git {:?}`", args);
        }

        let changes = std::str::from_utf8(&output.stdout)?;

        Ok(!changes.is_empty())
    }

    pub fn get_workspace_version(workspace: PathBuf) -> Result<Option<String>, Error> {
        let mut cargo_toml = workspace.clone();
        cargo_toml.push("Cargo.toml");
        let config: CargoConfig = toml::from_str(&read_to_string(&cargo_toml)?)?;
        Ok(config.package.map(|pkg| pkg.version))
    }

    pub fn is_workspace_version_updated(&self, workspace: PathBuf) -> Result<bool, Error> {
        let mut cargo_toml = workspace.clone();
        cargo_toml.push("Cargo.toml");

        if !cargo_toml.exists() || !cargo_toml.is_file() {
            panic!(
                "Cargo.toml file does not exist at {:?}",
                cargo_toml.display()
            )
        }

        let compare = format!("{}..{}", self.target_branch, self.current_branch);
        let args = &[
            "diff",
            &compare.trim(),
            "--",
            &cargo_toml.display().to_string(),
        ];
        let output = Command::new("git").args(args).output()?;

        if !output.status.success() {
            panic!("Command failed: `git {:?}`", args);
        }

        let changes = String::from(std::str::from_utf8(&output.stdout)?);

        Ok(changes.contains("+version ="))
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
    members: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CargoConfig {
    package: Option<CargoPackage>,
    workspace: Option<CargoWorkspace>,
}
