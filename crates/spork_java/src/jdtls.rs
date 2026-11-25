//! Eclipse's JDT language server (EPL-2.0, run as a separate process), which carries m2e for
//! Maven and Buildship for Gradle: downloaded on first use from download.eclipse.org into the
//! data folder's `languages/jdtls`, checked against the SHA-256 Eclipse publishes beside each
//! release, and run on the newest JDK found. Its release follows that JDK: current releases need
//! Java 25, so a machine with Java 17 or 21 gets the newest release that still runs there.

use collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use futures::AsyncReadExt as _;
use github_releases::{AssetKind, download_server_binary, download_server_raw_binary};
use gpui::AsyncApp;
use http_client::{AsyncBody, HttpClient};
use language::{LanguageName, LspAdapter, LspAdapterDelegate, LspInstaller, Toolchain};
use lsp::{LanguageServerBinary, LanguageServerName, Uri};
use project::lsp_store::language_server_settings;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use util::{ResultExt as _, merge_json_value_into};

use crate::jdk::{self, Jdk};

pub const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("jdtls");

const MILESTONES: &str = "https://download.eclipse.org/jdtls/milestones";

/// The newest JDT LS release each Java runtime runs, from the `Bundle-RequiredExecutionEnvironment`
/// of the bundles each release ships: 1.43.0 is the last on JavaSE-17 (1.44.0's core needs 21), and
/// 1.62.0 the last on JavaSE-21 (1.63.0's `org.eclipse.core.resources` and `jdt.core` need 25).
/// `None` is the newest release.
const RELEASES: [(u32, Option<&str>); 3] = [(25, None), (21, Some("1.62.0")), (17, Some("1.43.0"))];

/// The release a JDK of that feature release runs; `None` when it runs none (before Java 17).
fn release_for(major: u32) -> Option<Option<&'static str>> {
    RELEASES
        .iter()
        .find(|(needs, _)| major >= *needs)
        .map(|(_, release)| *release)
}

/// The Java a release needs.
fn java_needed(version: &str) -> u32 {
    let version = parse_version(version);
    if version <= parse_version("1.43.0") {
        17
    } else if version <= parse_version("1.62.0") {
        21
    } else {
        25
    }
}

fn parse_version(version: &str) -> Vec<u32> {
    version
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// Eclipse's java-debug plugin (microsoft/java-debug, EPL-1.0), which JDT LS loads as a bundle to
/// debug Java over the debug adapter protocol, as vscode-java does: from Maven Central, its
/// SHA-256 pinned here (Maven Central publishes a SHA-1 only; this one was checked against it).
/// 0.53.1 is the newest release and runs on Java 11 and later.
const JAVA_DEBUG: &str = "com.microsoft.java.debug.plugin-0.53.1.jar";
const JAVA_DEBUG_URL: &str = "https://repo.maven.apache.org/maven2/com/microsoft/java/com.microsoft.java.debug.plugin/0.53.1/com.microsoft.java.debug.plugin-0.53.1.jar";
const JAVA_DEBUG_SHA256: &str = "4f4778d452a6a0665536f43ce4e32403a24be6593336b80dc85a322912859e24";

/// Where java-debug is kept, beside the server's releases.
pub fn java_debug_jar(container: &Path) -> PathBuf {
    container.join("java-debug").join(JAVA_DEBUG)
}

/// java-debug, downloaded the first time JDT LS starts.
async fn fetch_java_debug(delegate: &Arc<dyn LspAdapterDelegate>) -> Option<PathBuf> {
    let container = delegate.language_server_download_dir(&SERVER_NAME).await?;
    let jar = java_debug_jar(&container);
    if !jar.is_file() {
        download_server_raw_binary(
            delegate.background_executor(),
            delegate.http_client(),
            JAVA_DEBUG_URL,
            Some(JAVA_DEBUG_SHA256),
            &container.join("java-debug"),
            JAVA_DEBUG,
        )
        .await
        .log_err()?;
    }
    Some(jar)
}

/// IntelliJ's notice when no JDK can run Java (`project.sdk.not.defined`).
pub const NO_JDK: &str = "Project JDK is not defined";

pub struct JdtlsAdapter;

/// A release to run, and the JDK to run it on.
pub struct JdtlsVersion {
    version: String,
    jdk: Jdk,
}

async fn find_jdk(delegate: &Arc<dyn LspAdapterDelegate>) -> Option<Jdk> {
    let env = delegate.shell_env().await;
    let jdk = jdk::for_project(delegate.worktree_root_path(), &env)?;
    (jdk.major >= 17).then_some(jdk)
}

fn install_dir(container: &Path, version: &str) -> PathBuf {
    container.join(format!("jdt-language-server-{version}"))
}

/// The Equinox launcher a complete install has.
fn launcher(install: &Path) -> Option<PathBuf> {
    std::fs::read_dir(install.join("plugins"))
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("org.eclipse.equinox.launcher_") && name.ends_with(".jar")
                })
        })
}

/// The installed release that the JDK runs, the newest of them.
fn installed_for(container: &Path, major: u32) -> Option<(String, PathBuf)> {
    let mut installed: Vec<(String, PathBuf)> = std::fs::read_dir(container)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            let version = name.strip_prefix("jdt-language-server-")?.to_owned();
            let path = entry.path();
            (java_needed(&version) <= major && launcher(&path).is_some()).then_some((version, path))
        })
        .collect();
    installed.sort_by_key(|(version, _)| parse_version(version));
    installed.pop()
}

/// The configuration folder of the platform it runs on, which the release ships read-only.
fn shared_configuration(install: &Path) -> PathBuf {
    let name = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "config_mac_arm",
        ("macos", _) => "config_mac",
        ("linux", "aarch64") => "config_linux_arm",
        ("windows", _) => "config_win",
        _ => "config_linux",
    };
    install.join(name)
}

/// Where JDT LS keeps a project's `kind` of state (`workspaces`: its index and build state,
/// `configuration`: Equinox's): in the data folder, one per project folder, as IntelliJ keeps its
/// caches in its system folder; the release's own folder stays read-only.
pub fn project_data(root: &Path, kind: &str) -> PathBuf {
    let digest = Sha256::digest(root.to_string_lossy().as_bytes());
    let hash: String = digest[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let name = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    paths::data_dir()
        .join("java")
        .join(kind)
        .join(format!("{name}-{hash}"))
}

/// The command that runs the server of `install` on `jdk` for the project at `root`.
fn command(install: &Path, jdk: &Jdk, root: &Path) -> Result<LanguageServerBinary> {
    let launcher = launcher(install).context("the language server's launcher is missing")?;
    let arg = |text: String| OsString::from(text);
    let mut arguments = vec![
        arg("-Declipse.application=org.eclipse.jdt.ls.core.id1".into()),
        arg("-Dosgi.bundles.defaultStartLevel=4".into()),
        arg("-Declipse.product=org.eclipse.jdt.ls.core.product".into()),
        arg("-Dosgi.checkConfiguration=true".into()),
        arg(format!(
            "-Dosgi.sharedConfiguration.area={}",
            shared_configuration(install).display()
        )),
        arg("-Dosgi.sharedConfiguration.area.readOnly=true".into()),
        arg("-Dosgi.configuration.cascaded=true".into()),
        arg("-Djava.import.generatesMetadataFilesAtProjectRoot=false".into()),
        // Maven's and Gradle's caches follow HOME (a test's is its own), not the account's home
        arg(format!("-Duser.home={}", paths::home_dir().display())),
        arg("-XX:+UseParallelGC".into()),
        arg("-Xms100m".into()),
        arg("-Xmx1G".into()),
        arg("--add-modules=ALL-SYSTEM".into()),
        arg("--add-opens".into()),
        arg("java.base/java.util=ALL-UNNAMED".into()),
        arg("--add-opens".into()),
        arg("java.base/java.lang=ALL-UNNAMED".into()),
        arg("-jar".into()),
        launcher.into_os_string(),
        arg("-configuration".into()),
        project_data(root, "configuration").into_os_string(),
        arg("-data".into()),
        project_data(root, "workspaces").into_os_string(),
    ];
    if jdk.major >= 24 {
        arguments.insert(0, arg("-Djdk.xml.maxGeneralEntitySizeLimit=0".into()));
        arguments.insert(0, arg("-Djdk.xml.totalEntitySizeLimit=0".into()));
    }
    Ok(LanguageServerBinary {
        path: jdk.java(),
        arguments,
        env: Some(HashMap::from_iter([(
            "JAVA_HOME".to_owned(),
            jdk.home.to_string_lossy().into_owned(),
        )])),
    })
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

/// The newest release in Eclipse's list of milestones.
async fn newest_release(http: &Arc<dyn HttpClient>) -> Result<String> {
    let index = get_text(http, &format!("{MILESTONES}/")).await?;
    index
        .split("milestones/")
        .skip(1)
        .filter_map(|rest| {
            let version: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            let version = version.trim_end_matches('.').to_owned();
            (version.split('.').count() == 3).then_some(version)
        })
        .max_by_key(|version| parse_version(version))
        .context("no release in Eclipse's list of JDT LS milestones")
}

impl LspInstaller for JdtlsAdapter {
    type BinaryVersion = JdtlsVersion;

    /// An installed release the JDK runs, with this project's workspace: the server's command
    /// differs by project, so it is found here, which isn't cached across projects.
    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Toolchain>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        let jdk = find_jdk(delegate).await?;
        let container = delegate.language_server_download_dir(&SERVER_NAME).await?;
        let (_, install) = installed_for(&container, jdk.major)?;
        command(&install, &jdk, delegate.worktree_root_path()).ok()
    }

    async fn fetch_latest_server_version(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: bool,
        _: &mut AsyncApp,
    ) -> Result<JdtlsVersion> {
        let jdk = find_jdk(delegate).await.context(NO_JDK)?;
        let version = match release_for(jdk.major).context(NO_JDK)? {
            Some(pinned) => pinned.to_owned(),
            None => newest_release(&delegate.http_client()).await?,
        };
        Ok(JdtlsVersion { version, jdk })
    }

    fn check_if_version_installed(
        &self,
        version: &JdtlsVersion,
        container_dir: &PathBuf,
        delegate: &Arc<dyn LspAdapterDelegate>,
    ) -> impl Send + Future<Output = Option<LanguageServerBinary>> + use<> {
        let install = install_dir(container_dir, &version.version);
        let jdk = version.jdk.clone();
        let root = delegate.worktree_root_path().to_owned();
        async move { command(&install, &jdk, &root).ok() }
    }

    fn fetch_server_binary(
        &self,
        version: JdtlsVersion,
        container_dir: PathBuf,
        delegate: &Arc<dyn LspAdapterDelegate>,
    ) -> impl Send + Future<Output = Result<LanguageServerBinary>> + use<> {
        let delegate = delegate.clone();
        async move {
            let http = delegate.http_client();
            let base = format!("{MILESTONES}/{}", version.version);
            let file = get_text(&http, &format!("{base}/latest.txt")).await?;
            let file = file.trim();
            let digest = get_text(&http, &format!("{base}/{file}.sha256")).await?;
            let digest = digest
                .split_whitespace()
                .next()
                .context("Eclipse's checksum file is empty")?
                .to_owned();
            let install = install_dir(&container_dir, &version.version);
            download_server_binary(
                delegate.background_executor(),
                http,
                &format!("{base}/{file}"),
                Some(&digest),
                &install,
                AssetKind::TarGz,
            )
            .await?;
            command(&install, &version.jdk, delegate.worktree_root_path())
        }
    }

    async fn cached_server_binary(
        &self,
        container_dir: PathBuf,
        delegate: &dyn LspAdapterDelegate,
    ) -> Option<LanguageServerBinary> {
        let env = delegate.shell_env().await;
        let jdk =
            jdk::for_project(delegate.worktree_root_path(), &env).filter(|jdk| jdk.major >= 17)?;
        let (_, install) = installed_for(&container_dir, jdk.major)?;
        command(&install, &jdk, delegate.worktree_root_path()).ok()
    }
}

/// JDT LS's settings (vscode-java's names) as Spork IDE runs it, the user's `lsp.jdtls.settings`
/// over them.
fn java_settings(jdk: Option<&Jdk>) -> Value {
    json!({
        "java": {
            "import": {
                "maven": {"enabled": true},
                "gradle": {
                    "enabled": true,
                    "wrapper": {"enabled": true},
                    "java": {"home": jdk.map(|jdk| jdk.home.to_string_lossy().into_owned())},
                },
            },
            "configuration": {"updateBuildConfiguration": "automatic"},
            // IntelliJ's generated code: equals() and hashCode() on java.util.Objects,
            // toString() as `Person{name=…, age=…}`
            "codeGeneration": {
                "hashCodeEquals": {"useJava7Objects": true},
                "toString": {"template": "${object.className}{${member.name()}=${member.value}, ${otherMembers}}"},
            },
            "autobuild": {"enabled": true},
            "completion": {"enabled": true},
            "signatureHelp": {"enabled": true},
        }
    })
}

#[async_trait(?Send)]
impl LspAdapter for JdtlsAdapter {
    fn name(&self) -> LanguageServerName {
        SERVER_NAME
    }

    async fn initialization_options(
        self: Arc<Self>,
        delegate: &Arc<dyn LspAdapterDelegate>,
        cx: &mut AsyncApp,
    ) -> Result<Option<Value>> {
        let settings = self
            .workspace_configuration(delegate, None, None, cx)
            .await?;
        let bundles: Vec<PathBuf> = fetch_java_debug(delegate).await.into_iter().collect();
        Ok(Some(json!({
            "bundles": bundles,
            "settings": settings,
            "extendedClientCapabilities": {
                "progressReportProvider": false,
                "classFileContentsSupport": false,
                "advancedOrganizeImportsSupport": false,
            },
        })))
    }

    async fn workspace_configuration(
        self: Arc<Self>,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Toolchain>,
        _: Option<Uri>,
        cx: &mut AsyncApp,
    ) -> Result<Value> {
        let jdk = find_jdk(delegate).await;
        let mut settings = java_settings(jdk.as_ref());
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
        HashMap::from_iter([(LanguageName::new_static("Java"), "java".into())])
    }
}

#[cfg(test)]
mod tests {
    use super::{installed_for, java_needed, release_for};

    #[test]
    fn the_release_follows_the_jdk() {
        assert_eq!(release_for(11), None);
        assert_eq!(release_for(17), Some(Some("1.43.0")));
        assert_eq!(release_for(20), Some(Some("1.43.0")));
        assert_eq!(release_for(21), Some(Some("1.62.0")));
        assert_eq!(release_for(25), Some(None));
        assert_eq!(java_needed("1.43.0"), 17);
        assert_eq!(java_needed("1.44.0"), 21);
        assert_eq!(java_needed("1.62.0"), 21);
        assert_eq!(java_needed("1.63.0"), 25);
    }

    #[test]
    fn an_install_runs_when_it_is_complete_and_the_jdk_runs_it() {
        let container = tempfile::tempdir().expect("a temporary folder");
        let install = |version: &str| {
            let plugins = container
                .path()
                .join(format!("jdt-language-server-{version}/plugins"));
            std::fs::create_dir_all(&plugins).expect("plugins");
            std::fs::write(plugins.join("org.eclipse.equinox.launcher_1.6.jar"), "")
                .expect("launcher");
        };
        install("1.43.0");
        install("1.63.0");
        std::fs::create_dir_all(container.path().join("jdt-language-server-1.62.0"))
            .expect("an incomplete install");
        assert_eq!(
            installed_for(container.path(), 17).map(|(version, _)| version),
            Some("1.43.0".into())
        );
        assert_eq!(
            installed_for(container.path(), 21).map(|(version, _)| version),
            Some("1.43.0".into())
        );
        assert_eq!(
            installed_for(container.path(), 25).map(|(version, _)| version),
            Some("1.63.0".into())
        );
    }
}
