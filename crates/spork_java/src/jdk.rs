//! The JDKs installed on the machine, found where IntelliJ's JDK detection looks
//! (`JavaHomeFinder`): `JAVA_HOME`, the `java` on the `PATH` and the usual install folders. Spork
//! IDE runs the Java language server and the programs it runs on the newest one; it never installs
//! one.

use collections::HashMap;
use std::path::{Path, PathBuf};

/// A JDK: its home folder and the Java version its `release` file names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Jdk {
    pub home: PathBuf,
    /// As the `release` file writes it: `17.0.20.1`, `1.8.0_402`.
    pub version: String,
    /// The feature release: 17 for `17.0.20.1`, 8 for `1.8.0_402`.
    pub major: u32,
}

impl Jdk {
    pub fn java(&self) -> PathBuf {
        self.home
            .join("bin")
            .join(format!("java{}", std::env::consts::EXE_SUFFIX))
    }

    /// IntelliJ's name for it in a JDK list: "17", "corretto-21"… Spork IDE has the version only.
    pub fn name(&self) -> String {
        self.major.to_string()
    }

    /// Reads the JDK whose home is `home`, if it is one (it has `bin/java` and a `release` file).
    pub fn at(home: &Path) -> Option<Self> {
        let release = std::fs::read_to_string(home.join("release")).ok()?;
        let version = release.lines().find_map(|line| {
            let value = line.strip_prefix("JAVA_VERSION=")?;
            Some(value.trim().trim_matches('"').to_owned())
        })?;
        let jdk = Self {
            home: home.to_owned(),
            major: major(&version)?,
            version,
        };
        jdk.java().is_file().then_some(jdk)
    }
}

/// The feature release of a Java version: `17.0.20.1` → 17, `1.8.0_402` → 8, `21` → 21.
pub fn major(version: &str) -> Option<u32> {
    let mut parts = version.split(['.', '_', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    match first {
        1 => parts.next()?.parse().ok(),
        _ => Some(first),
    }
}

/// The JDK homes to use instead of searching, separated as `PATH` is: what the machine's other
/// JDKs must not change (the end-to-end tests set it empty to have no JDK).
pub const JDKS_VARIABLE: &str = "SPORK_IDE_JDKS";

/// The JDKs found, the newest first. `env` is the project's environment (`JAVA_HOME`, `PATH`,
/// `HOME`).
pub fn find(env: &HashMap<String, String>) -> Vec<Jdk> {
    let mut homes: Vec<PathBuf> = Vec::new();
    let only = env
        .get(JDKS_VARIABLE)
        .cloned()
        .or_else(|| std::env::var(JDKS_VARIABLE).ok());
    if let Some(only) = only {
        homes.extend(std::env::split_paths(&only).filter(|home| !home.as_os_str().is_empty()));
        return valid(homes);
    }
    if let Some(home) = env.get("JAVA_HOME") {
        homes.push(PathBuf::from(home));
    }
    // `java` on the PATH, its home two folders up once its links are followed
    if let Some(path) = env.get("PATH") {
        for folder in std::env::split_paths(path) {
            let java = folder.join(format!("java{}", std::env::consts::EXE_SUFFIX));
            if let Ok(java) = std::fs::canonicalize(java)
                && let Some(home) = java.parent().and_then(Path::parent)
            {
                homes.push(home.to_owned());
            }
        }
    }
    let user_home = env.get("HOME").map(PathBuf::from);
    let mut folders: Vec<PathBuf> = vec![
        "/Library/Java/JavaVirtualMachines".into(),
        "/usr/lib/jvm".into(),
        "/usr/java".into(),
        "/opt/homebrew/opt".into(),
        "/usr/local/opt".into(),
    ];
    if let Some(user_home) = &user_home {
        folders.push(user_home.join("Library/Java/JavaVirtualMachines"));
        folders.push(user_home.join(".jdks"));
        folders.push(user_home.join(".sdkman/candidates/java"));
    }
    for folder in folders {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // a macOS bundle keeps its home in Contents/Home, Homebrew's in libexec
            for home in [
                path.join("Contents/Home"),
                path.join("libexec/openjdk.jdk/Contents/Home"),
                path.clone(),
            ] {
                homes.push(home);
            }
        }
    }
    valid(homes)
}

/// The JDKs among `homes`, each once, the newest first.
fn valid(homes: Vec<PathBuf>) -> Vec<Jdk> {
    let mut jdks: Vec<Jdk> = Vec::new();
    for home in homes {
        let home = std::fs::canonicalize(&home).unwrap_or(home);
        if jdks.iter().any(|jdk| jdk.home == home) {
            continue;
        }
        if let Some(jdk) = Jdk::at(&home) {
            jdks.push(jdk);
        }
    }
    jdks.sort_by_key(|jdk| std::cmp::Reverse(jdk.major));
    jdks
}

/// The JDK Spork IDE runs Java with: the newest found.
pub fn newest(env: &HashMap<String, String>) -> Option<Jdk> {
    find(env).into_iter().next()
}

/// The project's JDK (IntelliJ's Project SDK): the one chosen for it with Setup SDK, else the
/// newest found.
pub fn for_project(root: &Path, env: &HashMap<String, String>) -> Option<Jdk> {
    chosen(root).or_else(|| newest(env))
}

/// The JDK chosen for the project at `root`, while it is still one.
fn chosen(root: &Path) -> Option<Jdk> {
    let home = std::fs::read_to_string(chosen_file(root)).ok()?;
    Jdk::at(Path::new(home.trim()))
}

/// Makes `jdk` the project's JDK, kept in the data folder as IntelliJ keeps the project's SDK in
/// its `misc.xml`.
pub fn choose(root: &Path, jdk: &Jdk) -> anyhow::Result<()> {
    let file = chosen_file(root);
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    std::fs::write(&file, jdk.home.to_string_lossy().as_bytes())?;
    Ok(())
}

fn chosen_file(root: &Path) -> PathBuf {
    crate::jdtls::project_data(root, "jdk").with_extension("txt")
}

/// The JDK whose home is `folder`, or a macOS bundle's (`…/Contents/Home`), as Add JDK from Disk
/// takes either.
pub fn at_folder(folder: &Path) -> Option<Jdk> {
    Jdk::at(folder).or_else(|| Jdk::at(&folder.join("Contents/Home")))
}

/// The process's own environment, for what has no project's at hand.
pub fn process_env() -> HashMap<String, String> {
    std::env::vars().collect::<HashMap<_, _>>()
}

#[cfg(test)]
mod tests {
    use super::{Jdk, major};

    #[test]
    fn reads_the_feature_release_of_old_and_new_versions() {
        assert_eq!(major("17.0.20.1"), Some(17));
        assert_eq!(major("1.8.0_402"), Some(8));
        assert_eq!(major("21"), Some(21));
        assert_eq!(major("25-ea"), Some(25));
        assert_eq!(major("zulu"), None);
    }

    #[test]
    fn the_variable_replaces_the_search() {
        let mut env = collections::HashMap::default();
        env.insert(super::JDKS_VARIABLE.to_owned(), String::new());
        assert_eq!(super::find(&env), Vec::new());
    }

    #[test]
    fn a_home_is_a_jdk_with_its_release_file_and_java() {
        let home = tempfile::tempdir().expect("a temporary folder");
        assert_eq!(Jdk::at(home.path()), None);
        std::fs::create_dir_all(home.path().join("bin")).expect("bin");
        std::fs::write(home.path().join("bin/java"), "").expect("java");
        std::fs::write(
            home.path().join("release"),
            "IMPLEMENTOR=\"Azul Systems, Inc.\"\nJAVA_VERSION=\"17.0.20.1\"\n",
        )
        .expect("release");
        let jdk = Jdk::at(home.path()).expect("a JDK");
        assert_eq!((jdk.major, jdk.version.as_str()), (17, "17.0.20.1"));
    }
}
