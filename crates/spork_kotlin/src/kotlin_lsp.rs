//! JetBrains' Kotlin language server (`Kotlin/kotlin-lsp`), the "Kotlin language server" in the
//! UI: IntelliJ's Kotlin analysis run headless, with its own Java runtime and its own debug
//! adapter. It is under JetBrains' licence and each build expires about a month after its
//! release, so it is never bundled: the newest release is downloaded from JetBrains onto the
//! user's machine into the data folder's `languages/Kotlin language server`, checked against the SHA-256
//! published beside it, and the older builds removed. A build that says it has expired is set
//! aside (`kotlin_project` watches for it) and the newest one fetched.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use collections::HashMap;
use futures::AsyncReadExt as _;
use github_releases::{AssetKind, download_server_binary, latest_github_release};
use gpui::AsyncApp;
use http_client::{AsyncBody, HttpClient};
use language::{LanguageName, LspAdapter, LspAdapterDelegate, LspInstaller, Toolchain};
use lsp::{LanguageServerBinary, LanguageServerName, Uri};
use project::lsp_store::language_server_settings;
use serde_json::{Value, json};
use spork_java::Tool;
use spork_java::jdk::{self, Jdk};
use util::{ResultExt as _, merge_json_value_into};

/// What the UI calls it (the status bar's "Downloading …", the language servers' menu), never
/// a vendor's name; also its folder in the data folder's `languages` and its `lsp` settings' key.
pub const DISPLAY_NAME: &str = "Kotlin language server";

pub const SERVER_NAME: LanguageServerName = LanguageServerName::new_static(DISPLAY_NAME);

const REPOSITORY: &str = "Kotlin/kotlin-lsp";
const DOWNLOADS: &str = "https://download.jetbrains.com/language-server/kotlin-server";

/// What a build writes on stderr when it has expired, before it exits.
pub const EXPIRED: &str = "has expired";

/// The file that marks a downloaded build as expired.
const EXPIRED_MARK: &str = "expired";

/// The variables that would make the server send statistics (`INTELLIJ_DATA_SHARING`, with its
/// region) or write its launcher's log on stdout, the protocol's channel (`IJ_LAUNCHER_DEBUG`).
const STRIPPED_ENV: [&str; 3] = [
    "INTELLIJ_DATA_SHARING",
    "INTELLIJ_REGION",
    "IJ_LAUNCHER_DEBUG",
];

/// The archive JetBrains publishes for this platform.
fn archive(version: &str) -> (String, AssetKind) {
    let arch = if cfg!(target_arch = "aarch64") {
        "-aarch64"
    } else {
        ""
    };
    match std::env::consts::OS {
        // a `.sit` is a zip
        "macos" => (format!("kotlin-server-{version}{arch}.sit"), AssetKind::Zip),
        "windows" => (format!("kotlin-server-{version}.win.zip"), AssetKind::Zip),
        _ => (
            format!("kotlin-server-{version}{arch}.tar.gz"),
            AssetKind::TarGz,
        ),
    }
}

fn parse_version(version: &str) -> Vec<u32> {
    version
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// The server's launcher in a build's folder (the archive holds one `kotlin-server-<version>`).
fn launcher(install: &Path, version: &str) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "intellij-server.exe"
    } else {
        "intellij-server"
    };
    let launcher = install
        .join(format!("kotlin-server-{version}"))
        .join("bin")
        .join(name);
    launcher.is_file().then_some(launcher)
}

fn is_version(name: &str) -> bool {
    name.split('.').all(|part| part.parse::<u32>().is_ok())
}

/// The builds downloaded that still run, oldest first: complete, and not marked expired.
fn installed(container: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(container) else {
        return Vec::new();
    };
    let mut installed: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let version = entry.file_name().to_str()?.to_owned();
            if !is_version(&version) || entry.path().join(EXPIRED_MARK).exists() {
                return None;
            }
            let launcher = launcher(&entry.path(), &version)?;
            Some((version, launcher))
        })
        .collect();
    installed.sort_by_key(|(version, _)| parse_version(version));
    installed
}

/// The newest build that runs, its version and launcher.
pub fn newest_installed(container: &Path) -> Option<(String, PathBuf)> {
    installed(container).pop()
}

/// Sets the build of `version` aside: it isn't run again, and the next start fetches the newest.
pub fn mark_expired(container: &Path, version: &str) -> Result<()> {
    std::fs::write(container.join(version).join(EXPIRED_MARK), "")
        .with_context(|| format!("marking the Kotlin language server {version} expired"))
}

/// The folder the server's builds are downloaded into.
pub fn container() -> PathBuf {
    paths::languages_dir().join(SERVER_NAME.0.as_ref())
}

/// The command that runs `launcher` for the project at `root`.
fn command(launcher: &Path, root: &Path, jdk: Option<&Jdk>) -> LanguageServerBinary {
    let mut arguments: Vec<OsString> = Vec::new();
    // the language servers inherit the shell's environment: `env -u` takes these out of it
    // shortcut: on Windows the variables stay; start it through a wrapper there once Spork IDE
    // runs on Windows
    let path = if cfg!(unix) {
        for name in STRIPPED_ENV {
            arguments.push("-u".into());
            arguments.push(name.into());
        }
        arguments.push(launcher.as_os_str().to_owned());
        PathBuf::from("/usr/bin/env")
    } else {
        launcher.to_owned()
    };
    arguments.push("--stdio".into());
    arguments.push("--data-sharing=none".into());
    // its configuration and system folder, one per project as one server owns it; the indexes
    // go to the caches folder under the user's home
    arguments.push(
        format!(
            "--system-path={}",
            spork_java::jdtls::project_data(root, "kotlin-lsp").display()
        )
        .into(),
    );
    let mut env = HashMap::default();
    // the JVM takes its home from the account, not HOME: a test's HOME is its own (Maven's
    // repository, Gradle's caches and the indexes follow it)
    // shortcut: a user's own IJ_JAVA_OPTIONS (a bigger heap) is replaced; append to it once
    // Settings has the server's VM options
    env.insert(
        "IJ_JAVA_OPTIONS".to_owned(),
        format!("-Duser.home={}", paths::home_dir().display()),
    );
    if let Some(jdk) = jdk {
        // the import runs Gradle and Maven on it
        env.insert(
            "JAVA_HOME".to_owned(),
            jdk.home.to_string_lossy().into_owned(),
        );
    }
    LanguageServerBinary {
        path,
        arguments,
        env: Some(env),
    }
}

async fn find_jdk(delegate: &Arc<dyn LspAdapterDelegate>) -> Option<Jdk> {
    let env = delegate.shell_env().await;
    jdk::for_project(delegate.worktree_root_path(), &env)
}

async fn get_text(http: &Arc<dyn HttpClient>, url: &str) -> Result<String> {
    let mut response = http
        .get(url, AsyncBody::default(), true)
        .await
        .with_context(|| format!("fetching {url}"))?;
    anyhow::ensure!(
        response.status().is_success(),
        "fetching {url} failed with status {}",
        response.status()
    );
    let mut text = String::new();
    response
        .body_mut()
        .read_to_string(&mut text)
        .await
        .with_context(|| format!("reading {url}"))?;
    Ok(text)
}

pub struct KotlinLspAdapter;

impl LspInstaller for KotlinLspAdapter {
    type BinaryVersion = String;

    /// A build already downloaded, with this project's command: the command differs by project
    /// (its system folder), so it is found here, which isn't cached across projects.
    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Toolchain>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        let container = delegate.language_server_download_dir(&SERVER_NAME).await?;
        let (_, launcher) = newest_installed(&container)?;
        let jdk = find_jdk(delegate).await;
        Some(command(
            &launcher,
            delegate.worktree_root_path(),
            jdk.as_ref(),
        ))
    }

    /// The newest release on GitHub (the tag `kotlin-lsp/v271.2085.0`), which JetBrains' download
    /// site serves.
    async fn fetch_latest_server_version(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: bool,
        _: &mut AsyncApp,
    ) -> Result<String> {
        let release = latest_github_release(REPOSITORY, false, false, delegate.http_client())
            .await
            .context("finding the newest Kotlin language server")?;
        let version = release
            .tag_name
            .rsplit('v')
            .next()
            .filter(|version| !version.is_empty())
            .context("the Kotlin language server's release has no version")?
            .to_owned();
        let container = delegate.language_server_download_dir(&SERVER_NAME).await;
        if container.is_some_and(|container| container.join(&version).join(EXPIRED_MARK).exists()) {
            anyhow::bail!(
                "No build of the {DISPLAY_NAME} newer than {version} is out yet; Kotlin's code \
                 insight comes back once one is"
            );
        }
        Ok(version)
    }

    fn check_if_version_installed(
        &self,
        version: &String,
        container_dir: &PathBuf,
        delegate: &Arc<dyn LspAdapterDelegate>,
    ) -> impl Send + Future<Output = Option<LanguageServerBinary>> + use<> {
        let install = container_dir.join(version);
        let launcher = (!install.join(EXPIRED_MARK).exists())
            .then(|| launcher(&install, version))
            .flatten();
        let root = delegate.worktree_root_path().to_owned();
        let delegate = delegate.clone();
        async move {
            let launcher = launcher?;
            let jdk = find_jdk(&delegate).await;
            Some(command(&launcher, &root, jdk.as_ref()))
        }
    }

    fn fetch_server_binary(
        &self,
        version: String,
        container_dir: PathBuf,
        delegate: &Arc<dyn LspAdapterDelegate>,
    ) -> impl Send + Future<Output = Result<LanguageServerBinary>> + use<> {
        let delegate = delegate.clone();
        async move {
            let http = delegate.http_client();
            let (file, kind) = archive(&version);
            let url = format!("{DOWNLOADS}/{version}/{file}");
            // "<hex> *<file name>"
            let digest = get_text(&http, &format!("{url}.sha256")).await?;
            let digest = digest
                .split_whitespace()
                .next()
                .context("the Kotlin language server's checksum file is empty")?
                .to_owned();
            let install = container_dir.join(&version);
            download_server_binary(
                delegate.background_executor(),
                http,
                &url,
                Some(&digest),
                &install,
                kind,
            )
            .await?;
            let launcher = launcher(&install, &version)
                .context("the Kotlin language server's archive has no launcher")?;
            // the newest build is kept, the others (expired ones too) removed: each is over a
            // gigabyte
            if let Ok(entries) = std::fs::read_dir(&container_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if is_version(&name) && name != version.as_str() {
                        std::fs::remove_dir_all(entry.path()).log_err();
                    }
                }
            }
            let jdk = find_jdk(&delegate).await;
            Ok(command(
                &launcher,
                delegate.worktree_root_path(),
                jdk.as_ref(),
            ))
        }
    }

    async fn cached_server_binary(
        &self,
        container_dir: PathBuf,
        delegate: &dyn LspAdapterDelegate,
    ) -> Option<LanguageServerBinary> {
        let (_, launcher) = newest_installed(&container_dir)?;
        let env = delegate.shell_env().await;
        let root = delegate.worktree_root_path();
        let jdk = jdk::for_project(root, &env);
        Some(command(&launcher, root, jdk.as_ref()))
    }
}

/// The project kotlin-lsp imports: its build tool and build file, run on the project's JDK.
fn projects(root: &Path, jdk: Option<&Jdk>) -> Vec<Value> {
    let Some(tool) = Tool::detect(root) else {
        return Vec::new();
    };
    let (kind, files): (&str, &[&str]) = match tool {
        Tool::Maven => ("maven", &["pom.xml"]),
        Tool::Gradle => (
            "gradle",
            &[
                "build.gradle.kts",
                "build.gradle",
                "settings.gradle.kts",
                "settings.gradle",
            ],
        ),
    };
    let Some(file) = files
        .iter()
        .map(|file| root.join(file))
        .find(|file| file.is_file())
    else {
        return Vec::new();
    };
    let Ok(uri) = Uri::from_file_path(&file) else {
        return Vec::new();
    };
    let mut project = json!({"type": kind, "path": uri.to_string()});
    if let Some(jdk) = jdk {
        project["java-home"] = jdk.home.to_string_lossy().into_owned().into();
    }
    vec![project]
}

#[async_trait(?Send)]
impl LspAdapter for KotlinLspAdapter {
    fn name(&self) -> LanguageServerName {
        SERVER_NAME
    }

    async fn initialization_options(
        self: Arc<Self>,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: &mut AsyncApp,
    ) -> Result<Option<Value>> {
        let jdk = find_jdk(delegate).await;
        Ok(Some(json!({
            "projects": projects(delegate.worktree_root_path(), jdk.as_ref()),
        })))
    }

    async fn workspace_configuration(
        self: Arc<Self>,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Toolchain>,
        _: Option<Uri>,
        cx: &mut AsyncApp,
    ) -> Result<Value> {
        let mut settings = json!({});
        let user = cx.update(|cx| {
            language_server_settings(delegate.as_ref(), &SERVER_NAME, cx)
                .and_then(|settings| settings.settings.clone())
        });
        if let Some(user) = user {
            merge_json_value_into(user, &mut settings);
        }
        Ok(settings)
    }

    fn language_ids(&self) -> HashMap<LanguageName, String> {
        HashMap::from_iter([(LanguageName::new_static("Kotlin"), "kotlin".into())])
    }
}

#[cfg(test)]
mod tests {
    use super::{archive, installed, mark_expired, newest_installed, projects};

    #[test]
    fn the_newest_complete_build_runs_until_it_expires() {
        let container = tempfile::tempdir().expect("a temporary folder");
        let install = |version: &str| {
            let bin = container
                .path()
                .join(version)
                .join(format!("kotlin-server-{version}/bin"));
            std::fs::create_dir_all(&bin).expect("bin");
            std::fs::write(bin.join("intellij-server"), "").expect("launcher");
        };
        install("263.7084.0");
        install("271.2085.0");
        std::fs::create_dir_all(container.path().join("272.1.0")).expect("an incomplete build");
        assert_eq!(
            newest_installed(container.path()).map(|(version, _)| version),
            Some("271.2085.0".into())
        );
        mark_expired(container.path(), "271.2085.0").expect("marked");
        assert_eq!(
            newest_installed(container.path()).map(|(version, _)| version),
            Some("263.7084.0".into())
        );
        mark_expired(container.path(), "263.7084.0").expect("marked");
        assert!(installed(container.path()).is_empty());
    }

    #[test]
    fn names_the_platforms_archive() {
        let (file, _) = archive("271.2085.0");
        assert!(file.starts_with("kotlin-server-271.2085.0"));
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            assert_eq!(file, "kotlin-server-271.2085.0-aarch64.sit");
        }
    }

    #[test]
    fn imports_the_build_tools_project() {
        let root = tempfile::tempdir().expect("a temporary folder");
        assert!(projects(root.path(), None).is_empty());
        std::fs::write(root.path().join("build.gradle.kts"), "").expect("a build script");
        let found = projects(root.path(), None);
        assert_eq!(found[0]["type"], "gradle");
        assert!(
            found[0]["path"]
                .as_str()
                .is_some_and(|path| path.ends_with("/build.gradle.kts"))
        );
    }
}
