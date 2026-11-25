//! The IDE's SDK table (IntelliJ's Platform Settings › SDKs, its `jdk.table.xml`): the JDKs
//! added, each by its name and home, kept in the config folder for every project.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use util::ResultExt as _;

use crate::jdk::Jdk;

/// A JDK of the table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sdk {
    pub name: String,
    pub home: PathBuf,
}

impl Sdk {
    /// The JDK at its home, while it is one.
    pub fn jdk(&self) -> Option<Jdk> {
        Jdk::at(&self.home)
    }
}

fn file() -> PathBuf {
    paths::config_dir().join("jdk.table.json")
}

/// The table, as written last.
pub fn load() -> Vec<Sdk> {
    let Ok(text) = std::fs::read_to_string(file()) else {
        return Vec::new();
    };
    serde_json::from_str(&text)
        .context("reading the SDK table")
        .log_err()
        .unwrap_or_default()
}

pub fn save(sdks: &[Sdk]) -> Result<()> {
    let file = file();
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    std::fs::write(&file, serde_json::to_string_pretty(sdks)?)
        .with_context(|| format!("writing {file:?}"))
}

/// The name of the JDK at `home` in `sdks`.
pub fn named(sdks: &[Sdk], home: &Path) -> Option<String> {
    sdks.iter()
        .find(|sdk| sdk.home == home)
        .map(|sdk| sdk.name.clone())
}

/// `jdk`'s name in the table, or the one IntelliJ suggests for it.
pub fn name_of(jdk: &Jdk) -> String {
    named(&load(), &jdk.home).unwrap_or_else(|| jdk.suggested_name())
}

/// IntelliJ's unique name for a new SDK (`createUniqueSdkName`): the suggested one, then
/// "zulu-17 (2)", "zulu-17 (3)"….
pub fn unique_name(sdks: &[Sdk], suggested: &str) -> String {
    let taken = |name: &str| sdks.iter().any(|sdk| sdk.name == name);
    if !taken(suggested) {
        return suggested.to_owned();
    }
    (2..)
        .map(|ix| format!("{suggested} ({ix})"))
        .find(|name| !taken(name))
        .unwrap_or_else(|| suggested.to_owned())
}

/// Adds `jdk` to the table unless its home is there already; its name in the table.
pub fn register(jdk: &Jdk) -> Result<String> {
    let mut sdks = load();
    if let Some(name) = named(&sdks, &jdk.home) {
        return Ok(name);
    }
    let name = unique_name(&sdks, &jdk.suggested_name());
    sdks.push(Sdk {
        name: name.clone(),
        home: jdk.home.clone(),
    });
    save(&sdks)?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::{Sdk, unique_name};

    #[test]
    fn a_taken_name_gets_intellijs_number() {
        let sdk = |name: &str| Sdk {
            name: name.into(),
            home: name.into(),
        };
        assert_eq!(unique_name(&[], "zulu-17"), "zulu-17");
        assert_eq!(unique_name(&[sdk("zulu-17")], "zulu-17"), "zulu-17 (2)");
        assert_eq!(
            unique_name(&[sdk("zulu-17"), sdk("zulu-17 (2)")], "zulu-17"),
            "zulu-17 (3)"
        );
    }
}
