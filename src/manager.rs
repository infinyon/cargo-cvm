use anyhow::Error;
use cargo_toml::Manifest;
use clap::ValueEnum;
use git2::{BranchType, Repository, Tree};
use std::cmp::Ordering;
use std::convert::TryInto;
use std::fs::read_to_string;
use std::fs::{remove_file, File};
use std::io::Write;
use std::path::PathBuf;

use crate::Args;

#[derive(Debug, Clone, Eq)]
pub struct Version {
    major: u8,
    minor: u8,
    patch: u8,
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let major_ord = self.major.cmp(&other.major);
        let minor_ord = self.minor.cmp(&other.minor);
        let patch_ord = self.patch.cmp(&other.patch);

        match major_ord {
            Ordering::Equal => match minor_ord {
                Ordering::Equal => patch_ord,
                _ => minor_ord,
            },
            _ => major_ord,
        }
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.major == other.major && self.minor == other.minor && self.patch == other.patch
    }
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

    pub fn default() -> Self {
        Self {
            major: 0,
            minor: 1,
            patch: 0,
        }
    }
}
#[derive(Debug, Default, Clone, ValueEnum)]
pub enum SemVer {
    #[default]
    Minor,
    Major,
    Patch,
}

impl TryInto<Version> for Manifest {
    type Error = Error;
    fn try_into(self) -> Result<Version, Self::Error> {
        if let Some(pkg) = self.package {
            Ok(pkg.version.try_into()?)
        } else {
            Err(Error::msg("Invalid cargo manifest"))
        }
    }
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
    target_remote: String,
    target_branch: String,
    workspaces: Vec<String>,
    check: bool,
    fix: bool,
    warn: bool,
    force: bool,
    commit: bool,
    repo: Repository,
    ssh_key_path: String,
}

impl Manager {
    // Consumes `args`
    // TODO: Merge `Args` and `Manager` into single struct?
    pub fn new(args: Args) -> Result<Self, Error> {
        let dir = std::env::current_dir()?;
        let repo = Repository::discover(dir.clone())?;
        let ssh_key_path = format!("{}/.ssh/id_rsa", std::env::var("HOME")?);

        Ok(Self {
            semver: args.semver,
            check: args.check,
            fix: args.fix,
            warn: args.warn,
            force: args.force,
            commit: args.commit,
            target_branch: args.branch,
            target_remote: args.remote,
            workspaces: Self::get_cargo_workspaces(dir)?,
            ssh_key_path: args.ssh_key_path
                .unwrap_or(ssh_key_path),
            repo,
        })
    }

    pub fn get_cargo_workspaces(dir: PathBuf) -> Result<Vec<String>, Error> {
        let mut cargo_toml = dir;
        cargo_toml.push("Cargo.toml");

        if !cargo_toml.exists() {
            eprintln!("`cargo cvm` must be run in a directory containing a `Cargo.toml` file.\nFile does not exist at: {:?}", cargo_toml.display());
            std::process::exit(1)
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
        let mut cargo_toml = workspace;
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

            // Add changes to the git index;
            self.git_add_version_update(cargo_toml, new_version.to_string())?;

            Ok(())
        } else {
            eprintln!("invalid cargo file");
            std::process::exit(1)
        }
    }

    pub fn git_add_version_update(
        &self,
        cargo_toml: PathBuf,
        version: String,
    ) -> Result<(), Error> {
        let mut index = self.repo.index()?;

        if let Some(strip_path) = index.path() {
            if let Some(path) = strip_path.to_str() {
                if let Some(file_path) = cargo_toml.to_str() {
                    let root_path = &path.replace(".git/index", "");
                    let relative_file = file_path.replace(root_path, "");
                    index.add_path(PathBuf::from(relative_file).as_path())?;

                    // Update the index for the repo;
                    self.repo.checkout_index(Some(&mut index), None)?;

                    println!("version {} update added to git.", version);
                }
            }
        }

        Ok(())
    }

    pub fn fetch_target(&self) -> Result<(), Error> {
        let mut callbacks = git2::RemoteCallbacks::new();
        callbacks.credentials(|_url, username_from_url, _allowed_types| {
            git2::Cred::ssh_key(
                username_from_url.unwrap_or_default(),
                None,
                std::path::Path::new(&self.ssh_key_path),
                None,
            )
        });

        let mut fetch_options = git2::FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);

        match self.repo.find_remote(&self.target_remote) {
            Ok(mut remote) => {
                remote.fetch(&[&self.target_branch], Some(&mut fetch_options), None)?;
                Ok(())
            }
            Err(e) => {
                eprint!(
                    "Failed to find target remote host: {:?}; Error: {:?}",
                    &self.target_remote, e
                );
                let remotes = self.repo.remotes()?;
                let remotes = &remotes
                    .iter()
                    .map(|remote| remote.unwrap_or(""))
                    .collect::<Vec<&str>>();
                println!("\nAvailable Remotes: {:?}", remotes);
                eprintln!("Remote does not exist; try again with an available remote.");
                std::process::exit(1)
            }
        }
    }

    pub fn check_workspaces(&self) -> Result<(), Error> {
        self.fetch_target()?;

        let mut failed = false;

        // For each of the workspace directories, check if any files in the src directory have changed;
        for workspace in self.workspaces.iter() {
            if let Some((version, cargo_toml)) =
                self.is_version_outdated(PathBuf::from(workspace))?
            {
                let msg = format!(
                    "version {} is not updated for changes in workspace Cargo.toml file: {:?}",
                    version, cargo_toml
                );

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

        if failed {
            eprintln!("Found outdated version, exiting process unsuccessfully");
            std::process::exit(1)
        }

        if (self.force || self.fix) && self.commit {
            self.commit_changes("updated crate version(s)")?;
        }

        Ok(())
    }

    pub fn new_signature(&self) -> Result<git2::Signature, Error> {
        let config = self.repo.config()?;

        let name = config.get_entry("user.name")?;
        let email = config.get_entry("user.email")?;

        let sig = git2::Signature::now(
            name.value().unwrap_or_default(),
            email.value().unwrap_or_default(),
        )?;

        Ok(sig)
    }

    pub fn commit_changes(&self, msg: &str) -> Result<(), Error> {
        let mut index = self.repo.index()?;
        let oid = index.write_tree()?;
        let tree = self.repo.find_tree(oid)?;
        let sig = self.new_signature()?;
        let parent_commit = self.repo.head()?.peel_to_commit()?;
        let new_commit =
            self.repo
                .commit(Some("HEAD"), &sig, &sig, msg, &tree, &[&parent_commit])?;

        println!("commit {:?} includes version updates", new_commit);
        Ok(())
    }

    /// Returns (target, current) trees based on target and current branch;
    pub fn get_comparison_trees(&self) -> Result<(Tree, Tree), Error> {
        let remote = format!("{}/{}", self.target_remote, self.target_branch);

        let target_branch_tree = self
            .repo
            .find_branch(&remote, BranchType::Remote)?
            .into_reference()
            .peel_to_tree()?;
        let current_branch_tree = self.repo.head()?.peel_to_tree()?;
        Ok((target_branch_tree, current_branch_tree))
    }

    pub fn get_version_comparison(
        &self,
        old_oid: git2::Oid,
        new_oid: git2::Oid,
    ) -> Result<(Version, Version), Error> {
        let old_manifest: Manifest = toml::from_slice(self.repo.find_blob(old_oid)?.content())?;
        let new_manifest: Manifest = toml::from_slice(self.repo.find_blob(new_oid)?.content())?;

        let old_version: Version = old_manifest.try_into()?;
        let new_version: Version = new_manifest.try_into()?;

        Ok((old_version, new_version))
    }

    pub fn get_workspace_version(workspace: PathBuf) -> Result<Version, Error> {
        let mut cargo_toml = workspace;
        cargo_toml.push("Cargo.toml");
        let config: Manifest = toml::from_str(&read_to_string(&cargo_toml)?)?;
        config.try_into()
    }

    pub fn is_version_outdated(
        &self,
        workspace: PathBuf,
    ) -> Result<Option<(Version, PathBuf)>, Error> {
        let mut src_dir = workspace.clone();
        let mut cargo_toml = workspace.clone();

        // Only check the src directory;
        src_dir.push("src");
        cargo_toml.push("Cargo.toml");

        if !src_dir.exists() || !src_dir.is_dir() || !cargo_toml.exists() || !cargo_toml.is_file() {
            eprintln!("src directory does not exist at {:?}", src_dir.display());
            std::process::exit(1)
        }

        let (target_tree, current_tree) = self.get_comparison_trees()?;

        let diff = self
            .repo
            .diff_tree_to_tree(Some(&target_tree), Some(&current_tree), None)?;

        let mut no_changes = true;
        let mut src_files_changed = false;
        let mut version_is_updated = false;
        let mut outdated_version: Version = Self::get_workspace_version(workspace)?;

        diff.foreach(
            &mut |delta, _value| {
                let old_file = delta.old_file();
                let new_file = delta.new_file();

                if let Some(path) = new_file.path() {
                    if let Some(uri) = PathBuf::from(path).to_str() {
                        if let Some(repo_path) = self.repo.path().to_str() {
                            let mut path = PathBuf::from(repo_path.replace("/.git", ""));
                            path.push(uri);
                            if let Some(dir) = src_dir.to_str() {
                                if let Some(file) = path.to_str() {
                                    if file.contains(dir) {
                                        src_files_changed = true;
                                        no_changes = false;
                                    }
                                }
                            }

                            if cargo_toml == path {
                                if let Ok((old_version, new_version)) =
                                    self.get_version_comparison(old_file.id(), new_file.id())
                                {
                                    version_is_updated = new_version > old_version;

                                    if !version_is_updated {
                                        outdated_version = new_version;
                                    } else {
                                        outdated_version = old_version;
                                    }
                                }
                            }
                        }
                    }
                }

                true
            },
            None,
            None,
            None,
        )?;

        if src_files_changed && version_is_updated || no_changes {
            Ok(None)
        } else {
            Ok(Some((outdated_version, cargo_toml)))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::convert::TryInto;

    fn dummy_manager() -> Result<super::Manager, Box<dyn std::error::Error>> {
        let dir = std::env::current_dir()?;

        println!("Current directory: {:?}", dir);

        let repo = git2::Repository::discover(dir.clone())?;
        let ssh_key_path = format!("{}/.ssh/id_rsa", std::env::var("HOME")?);

        Ok(super::Manager {
            semver: String::from("minor").try_into()?,
            check: false,
            fix: false,
            warn: true,
            force: false,
            commit: false,
            target_remote: String::from("origin"),
            target_branch: String::from("master"),
            workspaces: super::Manager::get_cargo_workspaces(dir)?,
            ssh_key_path,
            repo,
        })
    }

    #[test]
    fn test_is_workspace_updated() -> Result<(), Box<dyn std::error::Error>> {
        let mgr = dummy_manager()?;

        let dir = std::env::current_dir()?;

        assert!(mgr.is_version_outdated(dir)?.is_some());

        Ok(())
    }

    #[test]
    fn test_signature() -> Result<(), Box<dyn std::error::Error>> {
        let mgr = dummy_manager()?;

        mgr.new_signature()?;

        Ok(())
    }
}
