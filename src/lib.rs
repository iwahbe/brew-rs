use command_builder::{Command, Single};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

fn brew_return(command: command_builder::Output, name: &str) -> Result<Package> {
    if command.success() {
        Ok(Package::new(name)?)
    } else {
        test_brew_installed()?;
        Err(Error::UnknownError(command.stderr().to_owned()))
    }
}

/// Represents a string which might be a version number for Homebrew.
/// Homebrew has requirements for version strings, so it is not possible
/// to definitively parse it.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(transparent)]
pub struct Version {
    original: String,
}

impl Version {
    /// Attempts to return a version of the form "N.N.N".
    pub fn parse(&self) -> Option<version_rs::Version> {
        version_rs::Version::from_str(&self.original).ok()
    }

    /// Returns the original version string.
    pub fn original(&self) -> &str {
        &self.original
    }
}

/// Represents a Homebrew package, which may or may not be installed.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Package {
    pub name: String,
    pub full_name: String,
    pub aliases: Vec<String>,
    pub oldname: Option<String>,
    pub desc: Option<String>,
    pub homepage: Option<String>,
    pub versions: Versions,
    pub urls: HashMap<String, Url>,
    pub revision: usize,
    pub version_scheme: usize,
    pub bottle: HashMap<String, Bottle>,
    pub keg_only: bool,
    pub bottle_disabled: bool,
    pub options: Vec<BrewOption>,
    pub build_dependencies: Vec<String>,
    pub dependencies: Vec<String>,
    pub recommended_dependencies: Vec<String>,
    pub optional_dependencies: Vec<String>,
    pub uses_from_macos: Vec<MapOrString>,
    pub requirements: Vec<Requirment>,
    pub conflicts_with: Vec<String>,
    pub caveats: Option<String>,
    pub installed: Vec<Installed>,
    pub linked_keg: Option<String>,
    pub pinned: bool,
    pub outdated: bool,
    pub analytics: Option<Analytics>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(untagged)]
pub enum MapOrString {
    MapStringString(HashMap<String, String>),
    String(String),
    MapStringVecString(HashMap<String, Vec<String>>),
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(untagged)]
pub enum NumOrString {
    Num(u32),
    String(String),
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Requirment {
    name: String,
    cask: Option<String>,
    download: Option<String>,
    version: Option<VersionResult>,
    contexts: Vec<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct BrewOption {
    option: String,
    description: String,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    NotInstalled,
    PackageNotFound,
    IOError(std::io::Error),
    ParseError(serde_json::Error),
    InstallFailed(String),
    UnknownError(String),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IOError(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::ParseError(e)
    }
}

fn contains<I, J, E>(iter1: I, iter2: J) -> bool
where
    I: IntoIterator<Item = E>,
    J: IntoIterator<Item = E>,
    E: std::cmp::Eq + std::hash::Hash,
{
    let hash: std::collections::HashSet<E> = iter1.into_iter().collect();
    for item in iter2.into_iter() {
        if !hash.contains(&item) {
            return false;
        }
    }
    true
}

impl Package {
    /// Creates package, filling out struct from the command line toole.
    pub fn new(name: &str) -> Result<Package> {
        let output = Single::new("/usr/local/bin/brew")
            .arg("info")
            .arg(name)
            .arg("--json=v1")
            .env("HOMEBREW_NO_AUTO_UPDATE", "1")
            .run()?;
        if output.success() {
            let packages: Vec<Package> = serde_json::from_str(output.stdout())?;
            packages
                .into_iter()
                .next()
                .map(Result::Ok)
                .unwrap_or(Err(Error::PackageNotFound))
        } else {
            test_brew_installed()?;
            Err(Error::PackageNotFound)
        }
    }

    /// Attempts to install a package, reinstalling a package if it is already installed.
    pub fn install(&self, options: &Options) -> Result<Package> {
        let command = Single::new("brew")
            .arg(if self.is_installed() && options.force {
                "reinstall"
            } else if self.is_installed() {
                let opts = self.install_options().unwrap();
                if contains(opts, options.package_options()) {
                    return Self::new(&self.name);
                } else {
                    "reinstall"
                }
            } else {
                "install"
            })
            .args(options.brew_options().as_slice())
            .arg(&self.name)
            .args(
                &options
                    .package_options()
                    .iter()
                    .map(|f| f.as_str())
                    .collect::<Vec<_>>(),
            )
            .env("HOMEBREW_NO_AUTO_UPDATE", "1")
            .run()?;
        if command.success() {
            let new = Self::new(&self.name)?;
            if new.is_installed() {
                Ok(new)
            } else {
                Err(Error::InstallFailed(
                    "Could not detect new install".to_owned(),
                ))
            }
        } else {
            test_brew_installed()?;
            Err(Error::InstallFailed(command.stderr().to_owned()))
        }
    }

    /// Check if a package is installed.
    pub fn is_installed(&self) -> bool {
        !self.installed.is_empty()
    }

    /// The package options that the package was installed with.
    pub fn install_options(&self) -> Option<&[String]> {
        self.installed
            .first()
            .map(|i: &Installed| i.used_options.as_slice())
    }

    /// Uninstalls the package.
    pub fn uninstall(&self, force: bool, ignore_dependencies: bool) -> Result<Package> {
        let mut args = vec!["uninstall", &self.name];
        if force {
            args.push("--force");
        }
        if ignore_dependencies {
            args.push("--ignore-dependencies");
        }
        let command = Single::new("brew")
            .args(args)
            .env("HOMEBREW_NO_AUTO_UPDATE", "1")
            .run()?;
        brew_return(command, &self.name)
    }

    /// Pin forumla to prevent automatic updates/upgrades.
    pub fn pin(&self) -> Result<Package> {
        if !self.pinned {
            let command = Single::new("brew")
                .arg("pin")
                .arg(&self.name)
                .env("HOMEBREW_NO_AUTO_UPDATE", "1")
                .run()?;
            brew_return(command, &self.name)
        } else {
            Ok(self.clone())
        }
    }

    /// Unpin formula to allow automatic updates/upgrades.
    pub fn unpin(&self) -> Result<Package> {
        if self.pinned {
            let command = Single::new("brew")
                .arg("unpin")
                .arg(&self.name)
                .env("HOMEBREW_NO_AUTO_UPDATE", "1")
                .run()?;
            brew_return(command, &self.name)
        } else {
            Ok(self.clone())
        }
    }

    /// Upgrade formula.
    pub fn upgrade(&self) -> Result<Package> {
        if self.is_installed() {
            let command = Single::new("brew")
                .arg("upgrade")
                .arg(&self.name)
                .env("HOMEBREW_NO_AUTO_UPDATE", "1")
                .run()?;
            brew_return(command, &self.name)
        } else {
            Err(Error::NotInstalled)
        }
    }
}

/// Update homebrew, synchronizing the homebrew-core and package list.
pub fn update() -> Result<()> {
    let command = Single::new("brew").arg("update").run()?;
    if command.success() {
        Ok(())
    } else {
        test_brew_installed()?;
        Err(Error::UnknownError(command.stderr().to_owned()))
    }
}

/// Return a map of all installed packages.
pub fn all_installed() -> Result<HashMap<String, Package>> {
    packages("--installed")
}

/// For internal use, wrapper to get package info.
fn packages(arg: &str) -> Result<HashMap<String, Package>> {
    let output = Single::new("brew")
        .arg("info")
        .arg("--json=v1")
        .arg(arg)
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .run()?;
    if output.success() {
        let v: Vec<Package> = serde_json::from_str(output.stdout())?;
        Ok(v.into_iter().map(|p| (p.name.clone(), p)).collect())
    } else {
        test_brew_installed()?;
        Err(Error::UnknownError(output.stdout().to_string()))
    }
}

/// Returns a map of all packages in the downloaded homebrew repository.
pub fn all_packages() -> Result<HashMap<String, Package>> {
    packages("--all")
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Analytics {
    pub install: Analytic,
    pub install_on_request: Analytic,
    pub build_error: Analytic,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Analytic {
    #[serde(rename = "30d")]
    d30: Option<HashMap<String, usize>>,
    #[serde(rename = "90d")]
    d90: Option<HashMap<String, usize>>,
    #[serde(rename = "d365")]
    d365: Option<HashMap<String, usize>>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Versions {
    pub stable: VersionResult,
    pub devel: Option<VersionResult>,
    pub head: Option<String>,
    pub bottle: bool,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Bottle {
    pub rebuild: usize,
    pub cellar: Option<String>,
    pub prefix: Option<String>,
    pub root_url: String,
    pub files: HashMap<String, File>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct File {
    pub url: String,
    pub sha256: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Url {
    pub url: String,
    pub tag: Option<String>,
    pub revision: Option<NumOrString>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Installed {
    pub version: VersionResult,
    pub used_options: Vec<String>,
    pub built_as_bottle: bool,
    pub poured_from_bottle: bool,
    #[serde(deserialize_with = "parse_null_as_empty_vec")]
    pub runtime_dependencies: Vec<Dependency>,
    pub installed_as_dependency: bool,
    pub installed_on_request: bool,
}

fn parse_null_as_empty_vec<'de, D>(d: D) -> std::result::Result<Vec<Dependency>, D::Error>
where
    D: Deserializer<'de>,
{
    Deserialize::deserialize(d).map(|v: Option<_>| v.unwrap_or_default())
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Dependency {
    pub full_name: String,
    pub version: VersionResult,
}

type VersionResult = Version;

/// Tests weither homebrew is installed by seeing if "brew --version" returns
/// successfully.
pub fn test_brew_installed() -> Result<()> {
    if Single::new("brew")
        .arg("--version")
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .run()
        .map(|o| o.success())
        .unwrap_or(false)
    {
        Ok(())
    } else {
        Err(Error::NotInstalled)
    }
}

/// WARNING: untested
/// installs the homebrew cli in "usr/local" which is it's recomended install location.
#[allow(dead_code)]
fn install_homebrew() -> Result<()> {
    install_homebrew_at("/usr/local")
}

/// WARNING: untested
/// TODO: Test this function
/// installs the homebrew cli in `dir`.
#[allow(dead_code)]
fn install_homebrew_at(dir: &str) -> Result<()> {
    Single::new("mkdir")
        .arg("homebrew")
        .and(
            Single::new("curl")
                .arg("-L")
                .arg("https://github.com/Homebrew/brew/tarball/master"),
        )
        .pipe(
            Single::new("tar")
                .arg("xz")
                .arg("--strip")
                .arg("1")
                .arg("-C")
                .arg("homebrew"),
        )
        .with_dir(dir)
        .run()?;
    test_brew_installed()?;
    Ok(())
}

/// Represents command line options with which to install a package.
#[derive(Clone, Debug, Default)]
pub struct Options {
    env: BuildEnv,
    ignore_dependencies: bool,
    only_dependencies: bool,
    build_from_source: bool,
    include_test: bool,
    force_bottle: bool,
    devel: bool,
    head: bool,
    keep_tmp: bool,
    build_bottle: bool,
    bottle_arch: bool,
    force: bool,
    git: bool,
    package_options: Vec<String>,
}

impl Options {
    /// Represents no options added.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the `--env=std` option.
    pub fn env_std(mut self) -> Self {
        self.env = BuildEnv::Std;
        self
    }

    /// Adds the `env=super` option.
    pub fn env_super(mut self) -> Self {
        self.env = BuildEnv::Super;
        self
    }

    /// Adds the `--ignore-dependencies` flag.
    pub fn ignore_dependencies(mut self) -> Self {
        self.ignore_dependencies = true;
        self
    }

    /// Adds the `--build-from-source` flag.
    pub fn build_from_source(mut self) -> Self {
        self.build_from_source = true;
        self
    }

    /// Adds the `--include-test` flag.
    pub fn include_test(mut self) -> Self {
        self.include_test = true;
        self
    }

    /// Adds the `--force-bottle` flag.
    pub fn force_bottle(mut self) -> Self {
        self.force_bottle = true;
        self
    }

    /// Adds the `--devel` flag.
    pub fn devel(mut self) -> Self {
        self.devel = true;
        self
    }

    /// Adds the `--HEAD` flag.
    pub fn head(mut self) -> Self {
        self.head = true;
        self
    }

    /// Adds the `--keep-tmp` flag.
    pub fn keep_tmp(mut self) -> Self {
        self.keep_tmp = true;
        self
    }

    /// Adds the `--build-bottle` flag.
    pub fn build_bottle(mut self) -> Self {
        self.build_bottle = true;
        self
    }

    /// Adds the `--bottle-arch` flag.
    pub fn bottle_arch(mut self) -> Self {
        self.bottle_arch = true;
        self
    }

    /// Adds the `--force` flag.
    pub fn force(mut self) -> Self {
        self.force = true;
        self
    }

    /// Adds the `--git` flag.
    pub fn git(mut self) -> Self {
        self.git = true;
        self
    }

    /// Adds a flag for the package to use directly.
    pub fn option(mut self, opt: &str) -> Self {
        self.package_options.push(opt.to_string());
        self
    }

    /// Adds an multiple flags for the package to use directly.
    pub fn options<I, S>(mut self, opts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.package_options
            .extend(opts.into_iter().map(|s| s.as_ref().to_string()));
        self
    }

    fn package_options(&self) -> &Vec<String> {
        &self.package_options
    }

    fn brew_options(&self) -> Vec<&str> {
        let mut out = Vec::new();
        match self.env {
            BuildEnv::Std => out.push("--env=std"),
            BuildEnv::Super => out.push("--env=super"),
            BuildEnv::None => {}
        }
        if self.ignore_dependencies {
            out.push("--ignore-dependencies")
        }
        if self.build_from_source {
            out.push("--build-from-source")
        }
        if self.include_test {
            out.push("--include-test")
        }
        if self.force_bottle {
            out.push("--force-bottle")
        }
        if self.devel {
            out.push("--devel")
        }
        if self.head {
            out.push("--HEAD")
        }
        if self.keep_tmp {
            out.push("--keep-tmp")
        }
        if self.build_bottle {
            out.push("--build-bottle")
        }
        if self.bottle_arch {
            out.push("--bottle-arch")
        }
        if self.force {
            out.push("--force")
        }
        if self.git {
            out.push("--git")
        }
        out
    }
}

#[derive(Clone, Copy, Debug)]
pub enum BuildEnv {
    Std,
    Super,
    None,
}

impl Default for BuildEnv {
    fn default() -> Self {
        Self::None
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_brew_install_test() {
        assert!(matches!(crate::test_brew_installed(), Ok(())));
    }

    #[test]
    fn get_info() {
        use crate::Package;
        let exa = Package::new("exa").unwrap();
        assert_eq!(exa.name, "exa");
        assert_eq!(exa.desc.unwrap(), "Modern replacement for 'ls'");
        assert!(
            exa.versions.stable.parse().unwrap() >= version_rs::Version::from((0 as u32, 9 as u32))
        );
    }

    #[test]
    fn all_installed() {
        crate::all_installed().unwrap();
    }
    #[test]

    fn all_packages() {
        crate::all_packages().unwrap();
    }
}
