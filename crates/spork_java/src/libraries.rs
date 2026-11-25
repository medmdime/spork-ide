//! IntelliJ's External Libraries in the Project tool window: the project's JDK (`< 17 >`) and the
//! libraries of its classpath as JDT LS resolves it (`Maven: junit:junit:4.13.2`, `Gradle: …`),
//! each with its jar. The tree shows them through an invisible worktree on a folder of links in
//! the data folder, its rows named by the folder roles.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use crate::java_project::Tool;
use crate::jdk::Jdk;

/// The rows' names and icons: the folder's, the JDK's and each library's.
pub(crate) const ROOT_LABEL: &str = "External Libraries";
pub(crate) const ROOT_ICON: &str = "icons/file_icons/external_libraries.svg";
const JDK_ICON: &str = "icons/file_icons/jdk.svg";
const LIBRARY_ICON: &str = "icons/file_icons/library.svg";

/// A library of the classpath: its name in the tree and its jar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Library {
    pub(crate) name: String,
    pub(crate) jar: PathBuf,
}

/// The library a jar of the classpath is, named as IntelliJ names an imported one: the build
/// tool, then the coordinates its repository's layout gives (Maven's
/// `group/as/folders/artifact/version/`, Gradle's cache `group/artifact/version/hash/`).
pub(crate) fn library(jar: &Path, tool: Option<Tool>) -> Library {
    let file_name = jar
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let folders: Vec<String> = jar
        .parent()
        .map(|parent| {
            parent
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let coordinates = match folders.iter().rposition(|folder| folder == "files-2.1") {
        // Gradle's cache: files-2.1/group/artifact/version/hash/file.jar
        Some(cache) if folders.len() == cache + 5 => Some(format!(
            "{}:{}:{}",
            folders[cache + 1],
            folders[cache + 2],
            folders[cache + 3]
        )),
        _ => maven_coordinates(&folders, &file_name),
    };
    let name = match (coordinates, tool) {
        (Some(coordinates), Some(tool)) => format!("{}: {coordinates}", tool.name()),
        (Some(coordinates), None) => coordinates,
        (None, _) => file_name,
    };
    Library {
        name,
        jar: jar.to_owned(),
    }
}

/// A Maven repository's coordinates: `…/repository/group/parts/artifact/version/artifact-version.jar`.
fn maven_coordinates(folders: &[String], file_name: &str) -> Option<String> {
    let repository = folders.iter().rposition(|folder| folder == "repository")?;
    let [group @ .., artifact, version] = folders.get(repository + 1..)? else {
        return None;
    };
    if group.is_empty() || !file_name.starts_with(&format!("{artifact}-{version}")) {
        return None;
    }
    Some(format!("{}:{artifact}:{version}", group.join(".")))
}

/// A row the folder roles name: its path, name and icon.
pub(crate) type Row = (PathBuf, String, &'static str);

/// Writes the folder the tree shows: the JDK's link first, then a folder per library (sorted by
/// name, as IntelliJ's) holding its jar's link. The rows to name.
pub(crate) fn write(folder: &Path, jdk: Option<&Jdk>, libraries: &[Library]) -> Result<Vec<Row>> {
    // the folder holds links alone: removing it leaves what they point to
    if folder.exists() {
        std::fs::remove_dir_all(folder).with_context(|| format!("clearing {folder:?}"))?;
    }
    std::fs::create_dir_all(folder).with_context(|| format!("creating {folder:?}"))?;
    let mut rows: Vec<Row> = vec![(folder.to_owned(), ROOT_LABEL.into(), ROOT_ICON)];
    if let Some(jdk) = jdk {
        let link = folder.join("0-jdk");
        symlink(&jdk.home, &link)?;
        rows.push((link, format!("< {} >", jdk.name()), JDK_ICON));
    }
    let mut libraries = libraries.to_vec();
    libraries.sort_by(|a, b| a.name.cmp(&b.name));
    libraries.dedup_by(|a, b| a.name == b.name);
    for (ix, library) in libraries.iter().enumerate() {
        let library_folder = folder.join(format!("{}-{ix:04}", 1));
        std::fs::create_dir_all(&library_folder)?;
        if let Some(file_name) = library.jar.file_name() {
            symlink(&library.jar, &library_folder.join(file_name))?;
        }
        rows.push((library_folder, library.name.clone(), LIBRARY_ICON));
    }
    Ok(rows)
}

fn symlink(target: &Path, link: &Path) -> Result<()> {
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let linked = match target.is_dir() {
        true => std::os::windows::fs::symlink_dir(target, link),
        false => std::os::windows::fs::symlink_file(target, link),
    };
    linked.with_context(|| format!("linking {link:?} to {target:?}"))
}

#[cfg(test)]
mod tests {
    use super::{Library, library, write};
    use crate::java_project::Tool;
    use std::path::Path;

    #[test]
    fn names_libraries_by_their_repository_coordinates() {
        assert_eq!(
            library(
                Path::new(
                    "/home/a/.m2/repository/org/hamcrest/hamcrest-core/1.3/hamcrest-core-1.3.jar"
                ),
                Some(Tool::Maven)
            )
            .name,
            "Maven: org.hamcrest:hamcrest-core:1.3"
        );
        assert_eq!(
            library(
                Path::new("/home/a/.gradle/caches/modules-2/files-2.1/junit/junit/4.13.2/8ac9e16d933b6fb43bc7f576336b8f4d7eb5ba12/junit-4.13.2.jar"),
                Some(Tool::Gradle)
            )
            .name,
            "Gradle: junit:junit:4.13.2"
        );
        assert_eq!(
            library(Path::new("/opt/lib/tool.jar"), Some(Tool::Maven)).name,
            "tool.jar"
        );
    }

    #[test]
    fn writes_the_jdk_first_and_the_libraries_by_name() {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let jar = folder.path().join("junit-4.13.2.jar");
        std::fs::write(&jar, "").expect("a jar");
        let shown = folder.path().join("shown");
        let libraries = [
            Library {
                name: "Maven: org.hamcrest:hamcrest-core:1.3".into(),
                jar: jar.clone(),
            },
            Library {
                name: "Maven: junit:junit:4.13.2".into(),
                jar,
            },
        ];
        let rows = write(&shown, None, &libraries).expect("written");
        let names: Vec<&str> = rows.iter().map(|(_, name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "External Libraries",
                "Maven: junit:junit:4.13.2",
                "Maven: org.hamcrest:hamcrest-core:1.3"
            ]
        );
        assert!(rows[1].0.join("junit-4.13.2.jar").exists());
        // written again, the old links go
        let rows = write(&shown, None, &libraries[..1]).expect("written again");
        assert_eq!(rows.len(), 2);
        assert!(!shown.join("1-0001").exists());
    }
}
