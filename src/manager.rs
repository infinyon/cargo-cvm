use anyhow::Error;
use cargo_toml::Manifest;
use clap::ArgMatches;
use git2::{Repository, Tree, BranchType};
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
    pub fn bump(&mut self, semver: SemVer) {
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

impl TryInto<SemVer> for String {
    type Error = Error;
    fn try_into(self) -> Result<SemVer, Error> {
        let semver = match self.as_ref() {
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
            .split('.')
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

pub struct Manager {
    semver: SemVer,
    target_branch: String,
    current_branch: String,
    workspaces: Vec<String>,
    check: bool,
    fix: bool,
    warn: bool,
    force: bool,
    commit: bool,
    repo: Repository,
}

impl Manager {

    pub fn new(args: &ArgMatches) -> Result<Self, Error> {
        let dir = std::env::current_dir()?;

        let repo = Repository::discover(dir.clone())?;

        Ok(Self {
            semver: args.value_of("semver").unwrap_or("minor").try_into()?,
            check: args.is_present("check"),
            fix: args.is_present("fix"),
            warn: args.is_present("warn"),
            force: args.is_present("force"),
            commit: args.is_present("commit"),
            target_branch: args.value_of("branch").unwrap_or("master").to_string(),
            current_branch: Self::get_current_branch(&repo)?,
            workspaces: Self::get_cargo_workspaces(dir)?,
            repo
        })
    }

    pub fn get_current_branch(repo: &Repository) -> Result<String, Error> {
        if let Some(name) = repo.head()?.name() {
            Ok(name.replace("refs/heads/", ""))
        } else {
            panic!("Failed to find current branch")
        }
    }

    pub fn get_cargo_workspaces(dir: PathBuf) -> Result<Vec<String>, Error> {
        let mut cargo_toml = dir;
        cargo_toml.push("Cargo.toml");

        if !cargo_toml.exists() {
            panic!("`cargo cvm` must be run in a directory containing a `Cargo.toml` file.\nFile does not exist at: {:?}", cargo_toml.display())
        }

        let config: Manifest = toml::from_str(&read_to_string(&cargo_toml)?)?;
        let mut paths: Vec<String> = Vec::new();

        if config.package.is_some() {
            let dir = std::env::current_dir()?;
            if let Some(path) = dir.to_str() {
                paths.push(String::from(path));
            }
        }

        if let Some(workspace) = config.workspace {
            paths.extend(workspace.members.into_iter())
        }

        Ok(paths)
    }

    pub fn bump_version(&self, workspace: PathBuf) -> Result<(), Error> {
        let mut cargo_toml = workspace.clone();
        cargo_toml.push("Cargo.toml");

        let config = read_to_string(&cargo_toml)?;
        if let Some(pkg) = toml::from_str::<Manifest>(&config)?.package {
            let old_version: Version = pkg.version.try_into()?;
            let mut new_version = old_version.clone();
            new_version.bump(self.semver.clone());

            // Replace only the first instance of the old_version to the new_version;
            // this will not replace dependency versions;
            let updated_config =
                config.replacen(&old_version.to_string(), &new_version.to_string(), 1);

            // Remove the old version of the file;
            remove_file(&cargo_toml)?;

            // Update the new version;
            let mut file = File::create(&cargo_toml)?;
            file.write_all(updated_config.as_bytes())?;

            // Commit the changes;
            Self::git_add_version_update(cargo_toml, new_version.to_string())?;

            Ok(())
        } else {
            panic!("invalid cargo file");
        }
    }

    pub fn git_add_version_update(cargo_toml: PathBuf, version: String) -> Result<(), Error> {
        Command::new("git")
            .args(&["add", &cargo_toml.display().to_string()])
            .output()
            .expect("Failed to add updated config");

        println!("version {} update added to git.", version);
        Ok(())
    }

    pub fn check_workspaces(&self) -> Result<(), Error> {
        let mut failed = false;

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
                            eprintln!("{}", msg.clone());
                            // set failed to true;
                            failed = true;
                        } else if self.fix {
                            self.bump_version(PathBuf::from(workspace))?;
                        } else if self.warn {
                            eprintln!("{}", &msg);
                        } else {
                            println!("{}", &msg);
                        }
                    } else if self.force {
                        // force an update even if the workspace version is already updated;
                        self.bump_version(PathBuf::from(workspace))?;
                    }
                }
            }
        }

        if failed {
            panic!("One or more workspace versions are out of date");
        }

        if (self.commit && self.fix) || (self.commit && self.force) {
            let commit_msg = format!("updated crate version(s)");
            Command::new("git")
                .args(&["commit", "-m", &commit_msg])
                .output()
                .expect("Failed to add updated crate versions");
        }

        Ok(())
    }

    /// Returns (target, current) trees based on target and current branch;
    pub fn get_comparison_trees(&self) -> Result<(Tree, Tree), Error> {
        let target_branch_tree = self.repo.find_branch(&self.target_branch, BranchType::Local)?.into_reference().peel_to_tree()?;
        let current_branch_tree = self.repo.find_branch(&self.current_branch, BranchType::Local)?.into_reference().peel_to_tree()?;
        Ok((target_branch_tree, current_branch_tree))
    }

    pub fn is_workspace_updated(&self, workspace: PathBuf) -> Result<bool, Error> {
        let mut src_dir = workspace;

        // Only check the src directory;
        src_dir.push("src");

        if !src_dir.exists() || !src_dir.is_dir() {
            panic!("src directory does not exist at {:?}", src_dir.display())
        }

        let ( target_tree, current_tree ) = self.get_comparison_trees()?;

        let diff = self.repo.diff_tree_to_tree(
            Some(&target_tree),
            Some(&current_tree),
            None,
        )?;

        let mut src_files_changed = false;

        diff.foreach(
            &mut |delta, _value| {
                if let Some(path) = delta.new_file().path() {
                    if let Some(uri) = PathBuf::from(path).to_str() {
                        if uri.contains("src") {
                            // set src_files_changed to true;
                            src_files_changed = true;
                        }
                    }
                }

                true
            },
            None,
            None,
            None,
        )?;

        Ok(src_files_changed)
    }

    pub fn get_workspace_version(workspace: PathBuf) -> Result<Option<String>, Error> {
        let mut cargo_toml = workspace;
        cargo_toml.push("Cargo.toml");
        let config: Manifest = toml::from_str(&read_to_string(&cargo_toml)?)?;
        Ok(config.package.map(|pkg| pkg.version))
    }

    pub fn is_workspace_version_updated(&self, workspace: PathBuf) -> Result<bool, Error> {
        let mut cargo_toml = workspace;
        cargo_toml.push("Cargo.toml");

        if !cargo_toml.exists() || !cargo_toml.is_file() {
            panic!(
                "Cargo.toml file does not exist at {:?}",
                cargo_toml.display()
            )
        }

        let ( target_tree, current_tree ) = self.get_comparison_trees()?;

        let diff = self.repo.diff_tree_to_tree(
            Some(&target_tree),
            Some(&current_tree),
            None,
        )?;

        let mut is_version_updated = false;

        diff.foreach(
            &mut |_delta, _value| {
                true
            },
            Some(&mut |delta, binary| {
                if let Some(path) = delta.new_file().path() {
                    if let Some(uri) = PathBuf::from(path).to_str() {
                        if uri.contains("Cargo.toml") {
                            if let Ok(text) = std::str::from_utf8(binary.new_file().data()) {
                                println!("text: {:?}", text);
                                is_version_updated = text.contains("+version = ");
                            }
                        }
                    }
                }


                true
            }),
            None,
            None,
        )?;

        Ok(is_version_updated)
    }
}


#[cfg(test)]
mod tests {
    use std::convert::TryInto;

    fn dummy_manager() -> Result<super::Manager, Box<dyn std::error::Error>> {
        let dir = std::env::current_dir()?;

        println!("Current directory: {:?}", dir);

        let repo = git2::Repository::discover(dir.clone())?;

        Ok(super::Manager {
            semver: String::from("minor").try_into()?,
            check: false,
            fix: false,
            warn: true,
            force: false,
            commit: false,
            target_branch: String::from("master"),
            current_branch: super::Manager::get_current_branch(&repo)?,
            workspaces: super::Manager::get_cargo_workspaces(dir)?,
            repo
        })
    } 
    
    #[test]
    fn test_current_branch() -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::current_dir()?;

        let repo = git2::Repository::discover(dir.clone())?;
        let branch = super::Manager::get_current_branch(&repo)?;
        println!("branch: {:?}", branch.replace("refs/heads/", ""));
        assert_eq!(branch.is_empty(), false);
        

        Ok(())
    }

    #[test]
    fn test_is_workspace_updated() -> Result<(), Box<dyn std::error::Error>> {
        let mgr = dummy_manager()?;

        let dir = std::env::current_dir()?;
        mgr.is_workspace_updated(dir.clone())?;

        mgr.is_workspace_version_updated(dir)?;
        
        Ok(())
    }

}