#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn test_brew_install_test() {
        assert!(matches!(test_brew_installed(), Ok(())));
    }

    #[test]
    fn get_info() {
        let exa = Package::new("exa").unwrap();
        assert_eq!(exa.name, "exa");
        assert_eq!(exa.desc.unwrap(), "Modern replacement for 'ls'");
        assert!(
            exa.versions.stable.parse().unwrap() >= version_rs::Version::from((0 as u32, 9 as u32))
        );
    }

    #[test]
    fn look_at_everything() {
        all_installed().unwrap();
        all_packages().unwrap();
    }
}

use command_builder::{Command, Single};
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::str::FromStr;
use version_rs;

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct Version {
    original: String,
}

impl Version {
    pub fn parse(&self) -> Option<version_rs::Version> {
        version_rs::Version::from_str(&self.original).ok()
    }

    pub fn original(&self) -> &str {
        &self.original
    }
}

#[derive(Deserialize, Serialize)]
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

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub enum MapOrString {
    MapStringString(HashMap<String, String>),
    String(String),
    MapStringVecString(HashMap<String, Vec<String>>),
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub enum NumOrString {
    Num(u32),
    String(String),
}

#[derive(Deserialize, Serialize)]
pub struct Requirment {
    name: String,
    cask: Option<String>,
    download: Option<String>,
    version: Option<VersionResult>,
    contexts: Vec<String>,
}

#[derive(Deserialize, Serialize)]
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
    return true;
}

impl Package {
    pub fn new(name: &str) -> Result<Package> {
        let output = Single::new("/usr/local/bin/brew")
            .a("info")
            .a(name)
            .a("--json=v1")
            .a("--analytics")
            .env("HOMEBREW_NO_AUTO_UPDATE", "1")
            .run()?;
        if output.success() {
            let packages: Vec<Package> = serde_json::from_str(output.stdout())?;
            packages
                .into_iter()
                .next()
                .map(|p| Ok(p))
                .unwrap_or(Err(Error::PackageNotFound))
        } else {
            test_brew_installed()?;
            println!("stderr: {}", output.stderr());
            Err(Error::PackageNotFound)
        }
    }

    pub fn install(&self, options: &[&str], force: bool) -> Result<Package> {
        let command = Single::new("brew")
            .a(if self.is_installed() && force {
                "reinstall"
            } else if self.is_installed() {
                let arr: Option<Vec<_>> = self
                    .install_options()
                    .map(|i| i.iter().map(|s| s.as_str()).collect());
                if arr
                    .map(|v| contains::<_, _, &str>(v, options.iter().map(|s| *s)))
                    .unwrap_or(false)
                {
                    return Self::new(&self.name);
                } else {
                    "reinstall"
                }
            } else {
                "install"
            })
            .a(&self.name)
            .args(options)
            .env("HOMEBREW_NO_AUTO_UPDATE", "1")
            .run()?;
        if command.success() {
            Ok(Self::new(&self.name)?)
        } else {
            test_brew_installed()?;
            Err(Error::InstallFailed(command.stderr().to_owned()))
        }
    }

    pub fn is_installed(&self) -> bool {
        self.installed.len() != 0
    }

    pub fn install_options(&self) -> Option<&[String]> {
        self.installed
            .first()
            .map(|i: &Installed| i.used_options.as_slice())
    }

    pub fn uninstall(&self) -> Result<Package> {
        let command = Single::new("brew")
            .a("uninstall")
            .a(&self.name)
            .env("HOMEBREW_NO_AUTO_UPDATE", "1")
            .run()?;
        if command.success() {
            Ok(Self::new(&self.name)?)
        } else {
            test_brew_installed()?;
            Err(Error::UnknownError(command.stderr().to_owned()))
        }
    }
}

pub fn update() -> Result<()> {
    let command = Single::new("brew").a("update").run()?;
    if command.success() {
        Ok(())
    } else {
        test_brew_installed()?;
        Err(Error::UnknownError(command.stderr().to_owned()))
    }
}

pub fn all_installed() -> Result<HashMap<String, Package>> {
    packages("--installed")
}

fn packages(arg: &str) -> Result<HashMap<String, Package>> {
    let output = Single::new("brew")
        .a("info")
        .a("--json=v1")
        .a(arg)
        .a("--analytics")
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .pipe(Single::new("jq"))
        .run()?;
    if output.success() {
        let v: Vec<Package> = serde_json::from_str(output.stdout())?;
        Ok(v.into_iter().map(|p| (p.name.clone(), p)).collect())
    } else {
        test_brew_installed()?;
        Err(Error::UnknownError(output.stdout().to_string()))
    }
}

pub fn all_packages() -> Result<HashMap<String, Package>> {
    packages("--all")
}

#[derive(Deserialize, Serialize)]
pub struct Analytics {
    pub install: Analytic,
    pub install_on_request: Analytic,
    pub build_error: Analytic,
}

#[derive(Deserialize, Serialize)]
pub struct Analytic {
    #[serde(rename = "30d")]
    d30: Option<HashMap<String, usize>>,
    #[serde(rename = "90d")]
    d90: Option<HashMap<String, usize>>,
    #[serde(rename = "d365")]
    d365: Option<HashMap<String, usize>>,
}

#[derive(Deserialize, Serialize)]
pub struct Versions {
    pub stable: VersionResult,
    pub devel: Option<VersionResult>,
    pub head: Option<String>,
    pub bottle: bool,
}

#[derive(Deserialize, Serialize)]
pub struct Bottle {
    pub rebuild: usize,
    pub cellar: String,
    pub prefix: String,
    pub root_url: String,
    pub files: HashMap<String, File>,
}

#[derive(Deserialize, Serialize)]
pub struct File {
    pub url: String,
    pub sha256: String,
}

#[derive(Deserialize, Serialize)]
pub struct Url {
    pub url: String,
    pub tag: Option<String>,
    pub revision: Option<NumOrString>,
}

#[derive(Deserialize, Serialize)]
pub struct Installed {
    pub version: VersionResult,
    pub used_options: Vec<String>,
    pub built_as_bottle: bool,
    pub poured_from_bottle: bool,
    pub runtime_dependencies: Vec<Dependency>,
    pub installed_as_dependency: bool,
    pub installed_on_request: bool,
}

#[derive(Deserialize, Serialize)]
pub struct Dependency {
    pub full_name: String,
    pub version: VersionResult,
}

type VersionResult = Version;

fn test_brew_installed() -> Result<()> {
    if Single::new("brew")
        .a("--version")
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

pub fn install_homebrew() -> Result<()> {
    install_homebrew_at("/usr/local")
}

pub fn install_homebrew_at(dir: &str) -> Result<()> {
    Single::new("mkdir")
        .a("homebrew")
        .and(
            Single::new("curl")
                .a("-L")
                .a("https://github.com/Homebrew/brew/tarball/master"),
        )
        .pipe(
            Single::new("tar")
                .a("xz")
                .a("--strip")
                .a("1")
                .a("-C")
                .a("homebrew"),
        )
        .with_dir(dir)
        .run()?;
    test_brew_installed()?;
    Ok(())
}