# brew-rs

A rust interface to the Homebrew command line app. 

The main benefit is a type-safe implementation of `brew --json` output. This
shows up in the form of the `Package` struct, as well as derivative structs. 

## Use
There are three entry points, all of which rely on having the brew command line
installed.

``` rust
update()?; // Updates homebrew and all formulea from github, by calling brew update
let jq = Package::new("jq")?; // equivalent to brew info
// not all packages have descriptions
assert_eq!(jq.desc.unwrap(), "Lightweight and flexible command-line JSON processor");
if !jq.is_installed() {
    jq.install(Options::new().head().force().env_std())?; // brew install --HEAD --force --env=std
}
```

The other main ways to access packages are:
``` rust
let installed_package = all_installed()?; // brew info --installed
let all_packages = all_packages()?        // brew info --all
```

