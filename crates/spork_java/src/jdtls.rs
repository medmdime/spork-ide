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

/// java-debug as installed: the official jar patched ([`patch_java_debug`]). Its own name, so an
/// unpatched jar installed before is replaced, and JDT LS swaps the bundle at its new location.
const JAVA_DEBUG_PATCHED: &str = "com.microsoft.java.debug.plugin-0.53.1-spork.jar";

/// java-debug's `JavaDebugServer` built from `java-debug-patch/` (its source, EPL-1.0, and
/// `build.sh`): the official one listens on every interface, unauthenticated, for JDT LS's whole
/// life, so anyone on the network could send DAP `launch` with their own `javaExec`; this one
/// listens on the loopback interface only.
const JAVA_DEBUG_PATCH: [(&str, &[u8]); 3] = [
    (
        "com/microsoft/java/debug/plugin/internal/JavaDebugServer.class",
        include_bytes!("../java-debug-patch/JavaDebugServer.class"),
    ),
    (
        "com/microsoft/java/debug/plugin/internal/JavaDebugServer$1.class",
        include_bytes!("../java-debug-patch/JavaDebugServer$1.class"),
    ),
    (
        "com/microsoft/java/debug/plugin/internal/JavaDebugServer$2.class",
        include_bytes!("../java-debug-patch/JavaDebugServer$2.class"),
    ),
];

/// Where java-debug is kept, beside the server's releases.
pub fn java_debug_jar(container: &Path) -> PathBuf {
    container.join("java-debug").join(JAVA_DEBUG_PATCHED)
}

/// java-debug, downloaded the first time JDT LS starts, checked, then patched.
async fn fetch_java_debug(delegate: &Arc<dyn LspAdapterDelegate>) -> Option<PathBuf> {
    let container = delegate.language_server_download_dir(&SERVER_NAME).await?;
    let jar = java_debug_jar(&container);
    if !jar.is_file() {
        let folder = container.join("java-debug");
        download_server_raw_binary(
            delegate.background_executor(),
            delegate.http_client(),
            JAVA_DEBUG_URL,
            Some(JAVA_DEBUG_SHA256),
            &folder,
            JAVA_DEBUG,
        )
        .await
        .log_err()?;
        install_java_debug(&folder.join(JAVA_DEBUG), &jar)
            .await
            .log_err()?;
    }
    Some(jar)
}

/// Patches the downloaded `official` jar into `jar`, then removes the official one.
async fn install_java_debug(official: &Path, jar: &Path) -> Result<()> {
    let patched = patch_java_debug(smol::fs::read(official).await?).await?;
    // written aside then moved in: a half-written jar would pass for an installed one
    let part = jar.with_extension("jar.part");
    smol::fs::write(&part, patched).await?;
    smol::fs::rename(&part, jar).await?;
    smol::fs::remove_file(official).await?;
    Ok(())
}

/// The jar with [`JAVA_DEBUG_PATCH`]'s classes in place of its own and its signature files
/// (`META-INF/*.SF`, `.RSA`, `.DSA`, `.EC`) left out, so the JDK no longer checks its entries
/// against the manifest's digests. The manifest keeps them: without signature files they are
/// inert, and JDT LS's Equinox verifies no bundle (neither `osgi.signedcontent.support` nor
/// `osgi.support.signature.verify` is set).
async fn patch_java_debug(official: Vec<u8>) -> Result<Vec<u8>> {
    use async_zip::ZipEntryBuilder;
    use async_zip::base::{read::mem::ZipFileReader, write::ZipFileWriter};

    let reader = ZipFileReader::new(official)
        .await
        .context("reading java-debug's jar")?;
    let mut writer = ZipFileWriter::new(Vec::new());
    let mut replaced = 0;
    for (ix, entry) in reader.file().entries().iter().enumerate() {
        let name = entry
            .filename()
            .as_str()
            .context("reading a name in java-debug's jar")?;
        let signature = name.starts_with("META-INF/")
            && [".SF", ".RSA", ".DSA", ".EC"]
                .iter()
                .any(|extension| name.ends_with(extension));
        if signature {
            continue;
        }
        let data = match JAVA_DEBUG_PATCH
            .iter()
            .find(|(patched, _)| *patched == name)
        {
            Some((_, class)) => {
                replaced += 1;
                class.to_vec()
            }
            None => {
                let mut data = Vec::new();
                reader
                    .reader_with_entry(ix)
                    .await?
                    .read_to_end_checked(&mut data)
                    .await
                    .with_context(|| format!("reading {name} in java-debug's jar"))?;
                data
            }
        };
        writer
            .write_entry_whole(
                ZipEntryBuilder::new(name.to_owned().into(), entry.compression())
                    .last_modification_date(*entry.last_modification_date()),
                &data,
            )
            .await?;
    }
    anyhow::ensure!(
        replaced == JAVA_DEBUG_PATCH.len(),
        "java-debug's jar lacks the classes Spork IDE patches"
    );
    Ok(writer.close().await?)
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
    server_jdk(delegate.worktree_root_path(), &env)
}

/// The JDK JDT LS runs on: the project's when it can (17 or newer), else the newest that can; a
/// project on an older JDK still builds on its own through `java.configuration.runtimes`.
fn server_jdk(root: &Path, env: &HashMap<String, String>) -> Option<Jdk> {
    jdk::for_module(root, env)
        .filter(|jdk| jdk.major >= 17)
        .or_else(|| jdk::find(env).into_iter().find(|jdk| jdk.major >= 17))
}

/// The JDK the server runs on: [`find_jdk`]'s, or for a Spring Boot project, whose bundles need
/// Java 21, one of 21 or newer ([`crate::spring_boot_ls::runtime_jdk`]); the project still builds
/// on its own through `java.configuration.runtimes`.
// Spork IDE: Spring Boot
async fn engine_jdk(delegate: &Arc<dyn LspAdapterDelegate>) -> Option<Jdk> {
    let jdk = find_jdk(delegate).await?;
    if jdk.major >= crate::spring_boot_ls::JAVA_NEEDED
        || !crate::spring_boot::is_project(delegate.worktree_root_path())
    {
        return Some(jdk);
    }
    let env = delegate.shell_env().await;
    crate::spring_boot_ls::runtime_jdk(&env, delegate.http_client())
        .await
        .log_err()
        .or(Some(jdk))
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
        let jdk = engine_jdk(delegate).await?;
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
        let jdk = engine_jdk(delegate).await.context(NO_JDK)?;
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
        let root = delegate.worktree_root_path();
        let jdk = server_jdk(root, &env)?;
        // Spork IDE: Spring Boot's servers' JDK, when one is there
        let jdk = match jdk.major < crate::spring_boot_ls::JAVA_NEEDED
            && crate::spring_boot::is_project(root)
        {
            true => crate::spring_boot_ls::installed_runtime_jdk(&env).unwrap_or(jdk),
            false => jdk,
        };
        let (_, install) = installed_for(&container_dir, jdk.major)?;
        command(&install, &jdk, delegate.worktree_root_path()).ok()
    }
}

/// JDT LS's name of a release's execution environment: `JavaSE-1.8`, `JavaSE-21`.
fn execution_environment(major: u32) -> String {
    format!("JavaSE-{}", jdk::feature_text(major))
}

/// The JDKs JDT LS builds on (`java.configuration.runtimes`), one per execution environment, the
/// project's the default: what its Maven and Gradle imports bind each project's release to.
fn runtimes(project: Option<&Jdk>, found: &[Jdk]) -> Value {
    let mut seen = collections::HashSet::default();
    project
        .into_iter()
        .chain(found)
        .filter(|jdk| seen.insert(jdk.major))
        .map(|jdk| {
            json!({
                "name": execution_environment(jdk.major),
                "path": jdk.home,
                "default": Some(jdk) == project,
            })
        })
        .collect()
}

/// A plain folder's settings: its language level as the compiler options JDT LS reads
/// (`java.settings.url`), and its compiler output.
fn folder_settings(root: &Path) -> Value {
    if crate::java_project::Tool::detect(root).is_some() {
        return json!({});
    }
    let settings = crate::project_settings::ProjectSettings::load(root);
    let output = settings
        .compiler_output_or_default(root)
        .strip_prefix(root)
        .map(|output| output.to_string_lossy().into_owned())
        .ok();
    let url = crate::project_settings::language_level(root, None).and_then(|level| {
        let file = project_data(root, "compiler").with_extension("prefs");
        let folder = file.parent()?;
        std::fs::create_dir_all(folder).log_err()?;
        std::fs::write(&file, crate::project_settings::compiler_preferences(level)).log_err()?;
        Some(file)
    });
    json!({
        "java": {
            "settings": {"url": url},
            "project": {"outputPath": output},
        }
    })
}

/// JDT LS's settings (vscode-java's names) as Spork IDE runs it, the user's `lsp.jdtls.settings`
/// over them.
fn java_settings(jdk: Option<&Jdk>, project: Option<&Jdk>, found: &[Jdk]) -> Value {
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
            "configuration": {
                "updateBuildConfiguration": "automatic",
                "runtimes": runtimes(project, found),
            },
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
        let mut bundles: Vec<PathBuf> = fetch_java_debug(delegate).await.into_iter().collect();
        // Spork IDE: Spring Boot's bundles, the Spring Boot language server's side in JDT LS
        if crate::spring_boot::is_project(delegate.worktree_root_path())
            && engine_jdk(delegate)
                .await
                .is_some_and(|jdk| jdk.major >= crate::spring_boot_ls::JAVA_NEEDED)
            && let Some(install) =
                crate::spring_boot_ls::fetch(delegate.http_client(), delegate.background_executor())
                    .await
                    .log_err()
        {
            bundles.extend(crate::spring_boot_ls::bundles(&install));
        }
        Ok(Some(json!({
            "bundles": bundles,
            "settings": settings,
            "extendedClientCapabilities": {
                "progressReportProvider": false,
                // a library's class comes back as a `jdt://` location, which the app opens as
                // its source (`project::lsp_store::NonFileUriResolver`)
                "classFileContentsSupport": true,
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
        let env = delegate.shell_env().await;
        let root = delegate.worktree_root_path();
        let mut settings = java_settings(
            jdk.as_ref(),
            jdk::for_module(root, &env).as_ref(),
            &jdk::find(&env),
        );
        merge_json_value_into(folder_settings(root), &mut settings);
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
    use super::{
        JAVA_DEBUG_PATCH, installed_for, java_needed, patch_java_debug, release_for, runtimes,
    };

    #[test]
    fn java_debug_is_patched_to_listen_on_the_loopback_interface_unsigned() {
        use async_zip::base::{read::mem::ZipFileReader, write::ZipFileWriter};
        use async_zip::{Compression, ZipEntryBuilder};

        let server = JAVA_DEBUG_PATCH[0].0;
        smol::block_on(async {
            let mut writer = ZipFileWriter::new(Vec::new());
            for (name, data) in [
                ("META-INF/MANIFEST.MF", "Manifest-Version: 1.0\n".as_bytes()),
                ("META-INF/MSFTSIG.SF", b"signature"),
                ("META-INF/MSFTSIG.RSA", b"certificate"),
                ("plugin.xml", b"<plugin/>"),
                (JAVA_DEBUG_PATCH[0].0, b"official"),
                (JAVA_DEBUG_PATCH[1].0, b"official"),
                (JAVA_DEBUG_PATCH[2].0, b"official"),
            ] {
                writer
                    .write_entry_whole(
                        ZipEntryBuilder::new(name.into(), Compression::Deflate),
                        data,
                    )
                    .await
                    .expect("an entry");
            }
            let official = writer.close().await.expect("a jar");

            let patched = patch_java_debug(official).await.expect("patched");
            let reader = ZipFileReader::new(patched).await.expect("a jar");
            let mut entries = Vec::new();
            for (ix, entry) in reader.file().entries().iter().enumerate() {
                let mut data = Vec::new();
                reader
                    .reader_with_entry(ix)
                    .await
                    .expect("an entry")
                    .read_to_end_checked(&mut data)
                    .await
                    .expect("its data");
                entries.push((entry.filename().as_str().expect("a name").to_owned(), data));
            }
            let names: Vec<&str> = entries.iter().map(|(name, _)| name.as_str()).collect();
            assert_eq!(
                names,
                [
                    "META-INF/MANIFEST.MF",
                    "plugin.xml",
                    JAVA_DEBUG_PATCH[0].0,
                    JAVA_DEBUG_PATCH[1].0,
                    JAVA_DEBUG_PATCH[2].0,
                ]
            );
            assert_eq!(entries[1].1, b"<plugin/>");
            for (ix, (_, class)) in JAVA_DEBUG_PATCH.iter().enumerate() {
                assert_eq!(entries[ix + 2].1, *class);
            }

            let mut writer = ZipFileWriter::new(Vec::new());
            writer
                .write_entry_whole(
                    ZipEntryBuilder::new(server.into(), Compression::Stored),
                    b"x",
                )
                .await
                .expect("an entry");
            let incomplete = writer.close().await.expect("a jar");
            assert!(patch_java_debug(incomplete).await.is_err());
        });
        let calls = |name: &str| {
            JAVA_DEBUG_PATCH[0]
                .1
                .windows(name.len())
                .any(|bytes| bytes == name.as_bytes())
        };
        assert!(calls("getLoopbackAddress"));
    }

    #[test]
    fn every_jdk_is_a_runtime_once_the_projects_the_default() {
        let jdk = |major: u32, home: &str| crate::jdk::Jdk {
            home: home.into(),
            version: major.to_string(),
            major,
            variant: None,
            arch: None,
        };
        let project = jdk(21, "/jdks/21");
        let found = [
            jdk(25, "/jdks/25"),
            jdk(21, "/jdks/other-21"),
            jdk(8, "/jdks/8"),
        ];
        assert_eq!(
            runtimes(Some(&project), &found),
            serde_json::json!([
                {"name": "JavaSE-21", "path": "/jdks/21", "default": true},
                {"name": "JavaSE-25", "path": "/jdks/25", "default": false},
                {"name": "JavaSE-1.8", "path": "/jdks/8", "default": false},
            ])
        );
    }

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
