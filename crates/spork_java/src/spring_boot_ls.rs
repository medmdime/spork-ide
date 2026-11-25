//! Spring Tools' Spring Boot language server (EPL-1.0, run as a separate process), the engine of
//! IntelliJ's Spring Boot code insight in `application*.properties` and `application*.yml`: the
//! JDT-bridged server of the VS Code extension `vscode-spring-boot`, downloaded from Open VSX
//! with its SHA-256 pinned once a Spring Boot project is opened, never bundled. Its five bundles
//! go into JDT LS, and the bridge relays its `sts/*` requests to JDT LS and JDT LS's classpath
//! callbacks to it, as the VS Code extension does. It runs on Java 21 or newer
//! ([`crate::server_jdk`]); its messages are IntelliJ's.

use collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Weak};
use std::time::Duration;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use github_releases::{AssetKind, download_server_binary};
use gpui::{AsyncApp, BackgroundExecutor};
use http_client::HttpClient;
use language::{LanguageName, LspAdapter, LspAdapterDelegate, LspInstaller, Toolchain};
use lsp::{LanguageServer, LanguageServerBinary, LanguageServerName, Uri};
use project::lsp_store::language_server_settings;
use regex::Regex;
use serde_json::{Value, json};
use util::{ResultExt as _, merge_json_value_into};

use crate::jdk::Jdk;

pub const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("spring-boot");

/// The languages of Spring Boot's configuration files (`application*.properties`,
/// `application*.yml`), which only this server serves, as IntelliJ's Spring Boot plugin.
pub const PROPERTIES: &str = "Spring Boot Properties";
pub const YAML: &str = "Spring Boot YAML";

/// `vscode-spring-boot` 2.4.1 (Spring Tools 5.4.1, 2026-10-15), checked against the SHA-256 Open
/// VSX publishes beside it.
const VERSION: &str = "2.4.1";
const VSIX_URL: &str = "https://open-vsx.org/api/VMware/vscode-spring-boot/2.4.1/file/VMware.vscode-spring-boot-2.4.1.vsix";
const VSIX_SHA256: &str = "6971f2bbc513a944e7f907e48ec68f64848d689630d5616bf54ef0803349f6ed";

/// The bundles JDT LS loads (the extension's `contributes.javaExtensions`).
const BUNDLES: [&str; 5] = [
    "io.projectreactor.reactor-core.jar",
    "org.reactivestreams.reactive-streams.jar",
    "jdt-ls-commons.jar",
    "jdt-ls-extension.jar",
    "sts-gradle-tooling.jar",
];

/// The Java both servers need: the bundles are Java 21 class files.
pub const JAVA_NEEDED: u32 = 21;

/// Where the servers' own JDK is downloaded: the data folder, beside `languages/`, which
/// [`crate::jdk::find`] doesn't search, so it is never offered as a project's JDK, as IntelliJ
/// never lists its own runtime as an SDK.
fn runtime_folder() -> PathBuf {
    paths::data_dir().join("jdks")
}

/// The newest JDK of Java 21 or newer in `folder`.
fn runtime_in(folder: &Path) -> Option<Jdk> {
    std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .filter_map(|entry| crate::jdk::at_folder(&entry.path()))
        .filter(|jdk| jdk.major >= JAVA_NEEDED)
        .max_by_key(|jdk| jdk.major)
}

/// A JDK the servers run on, of Java 21 or newer: one of the machine's, else their own.
pub fn installed_runtime_jdk(env: &HashMap<String, String>) -> Option<Jdk> {
    crate::jdk::find(env)
        .into_iter()
        .find(|jdk| jdk.major >= JAVA_NEEDED)
        .or_else(|| runtime_in(&runtime_folder()))
}

/// A JDK the servers run on: one installed, else Temurin's latest Java 21 downloaded into
/// [`runtime_folder`], as IntelliJ runs its engine on a runtime of its own. A JDK, not a JRE:
/// live data needs `jdk.attach`.
pub async fn runtime_jdk(env: &HashMap<String, String>, http: Arc<dyn HttpClient>) -> Result<Jdk> {
    if let Some(jdk) = installed_runtime_jdk(env) {
        return Ok(jdk);
    }
    crate::jdk_download::download_jdk(
        http,
        "temurin",
        JAVA_NEEDED,
        &runtime_folder(),
        Arc::new(|_| {}),
    )
    .await
}

/// Where the extension is unpacked, under the language servers' folder.
pub fn install_dir(container: &Path) -> PathBuf {
    container.join(format!("vscode-spring-boot-{VERSION}"))
}

/// The folder the server's downloads go to.
pub fn container() -> PathBuf {
    paths::languages_dir().join(SERVER_NAME.0.as_ref())
}

/// The extension, downloaded the first time a Spring Boot project needs it.
pub async fn fetch(http: Arc<dyn HttpClient>, executor: &BackgroundExecutor) -> Result<PathBuf> {
    let container = container();
    let install = install_dir(&container);
    if server_jar(&install).is_some() {
        return Ok(install);
    }
    std::fs::create_dir_all(&container)?;
    download_server_binary(
        executor,
        http,
        VSIX_URL,
        Some(VSIX_SHA256),
        &install,
        AssetKind::Zip,
    )
    .await?;
    server_jar(&install)
        .map(|_| install.clone())
        .with_context(|| format!("no language server in {}", install.display()))
}

/// The bundles JDT LS loads for the server, once the extension is there.
pub fn bundles(install: &Path) -> Vec<PathBuf> {
    let jars: Vec<PathBuf> = BUNDLES
        .iter()
        .map(|jar| install.join("extension/jars").join(jar))
        .collect();
    match jars.iter().all(|jar| jar.is_file()) {
        true => jars,
        false => Vec::new(),
    }
}

/// The server's launcher: the one `spring-boot-language-server-*.jar`.
fn server_jar(install: &Path) -> Option<PathBuf> {
    std::fs::read_dir(install.join("extension/language-server"))
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("spring-boot-language-server-") && name.ends_with(".jar")
                })
        })
}

/// The command that runs the server on `jdk`, with the VS Code extension's arguments.
fn command(install: &Path, jdk: &Jdk) -> Result<LanguageServerBinary> {
    let jar = server_jar(install).context("the Spring Boot language server is missing")?;
    let arguments = [
        "-Xmx1024m",
        "-Dspring.config.location=classpath:/application.properties",
        "-Djdk.util.zip.disableZip64ExtraFieldValidation=true",
        "-Dspring.main.web-application-type=NONE",
        "-Dsts.lsp.client=vscode",
        "-Dlogging.level.root=error",
        "-Xlog:jni+resolve=off",
        "-jar",
    ]
    .into_iter()
    .map(OsString::from)
    .chain([jar.into_os_string()])
    .collect();
    Ok(LanguageServerBinary {
        path: jdk.java(),
        arguments,
        env: Some(HashMap::from_iter([(
            "JAVA_HOME".to_owned(),
            jdk.home.to_string_lossy().into_owned(),
        )])),
    })
}

pub struct SpringBootLsAdapter;

impl LspInstaller for SpringBootLsAdapter {
    type BinaryVersion = Jdk;

    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Toolchain>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        self.cached_server_binary(container(), delegate.as_ref())
            .await
    }

    /// The JDK to run on: the server has one release, pinned.
    async fn fetch_latest_server_version(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: bool,
        _: &mut AsyncApp,
    ) -> Result<Jdk> {
        let root = delegate.worktree_root_path();
        anyhow::ensure!(
            crate::spring_boot::is_project(root),
            "{} isn't a Spring Boot project",
            root.display()
        );
        let env = delegate.shell_env().await;
        runtime_jdk(&env, delegate.http_client()).await
    }

    fn check_if_version_installed(
        &self,
        jdk: &Jdk,
        container_dir: &PathBuf,
        _: &Arc<dyn LspAdapterDelegate>,
    ) -> impl Send + Future<Output = Option<LanguageServerBinary>> + use<> {
        let install = install_dir(container_dir);
        let jdk = jdk.clone();
        async move { command(&install, &jdk).ok() }
    }

    fn fetch_server_binary(
        &self,
        jdk: Jdk,
        _: PathBuf,
        delegate: &Arc<dyn LspAdapterDelegate>,
    ) -> impl Send + Future<Output = Result<LanguageServerBinary>> + use<> {
        let delegate = delegate.clone();
        async move {
            let install = fetch(delegate.http_client(), delegate.background_executor()).await?;
            command(&install, &jdk)
        }
    }

    async fn cached_server_binary(
        &self,
        container_dir: PathBuf,
        delegate: &dyn LspAdapterDelegate,
    ) -> Option<LanguageServerBinary> {
        if !crate::spring_boot::is_project(delegate.worktree_root_path()) {
            return None;
        }
        let env = delegate.shell_env().await;
        let jdk = installed_runtime_jdk(&env)?;
        command(&install_dir(&container_dir), &jdk).ok()
    }
}

/// The server's settings (the VS Code extension's `boot-java` and `spring-boot` sections) as
/// Spork IDE runs it: IntelliJ shows no code lenses and doesn't check versions on spring.io.
fn settings() -> Value {
    json!({
        "boot-java": {
            "live-information": {
                "automatic-connection": {"on": false},
                "fetch-data": {"max-retries": 10, "retry-delay-in-seconds": 3},
            },
            "scan-java-test-sources": {"on": false},
            "support-spring-xml-config": {"on": false},
            "highlight-codelens": {"on": false},
            "validation": {"java": {"version-validation": "OFF"}},
            "ai": {"mcp-server-enabled": false},
        },
        "spring-boot": {"ls": {"checkJVM": true}},
    })
}

#[async_trait(?Send)]
impl LspAdapter for SpringBootLsAdapter {
    fn name(&self) -> LanguageServerName {
        SERVER_NAME
    }

    /// Classpath listening starts once the bridge to JDT LS is in place
    /// ([`enable_classpath_listening`]).
    async fn initialization_options(
        self: Arc<Self>,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: &mut AsyncApp,
    ) -> Result<Option<Value>> {
        let root = Uri::from_file_path(delegate.worktree_root_path())
            .map_err(|()| anyhow::anyhow!("the project's folder isn't a path"))?;
        Ok(Some(json!({
            "workspaceFolders": [root.to_string()],
            "enableJdtClasspath": false,
        })))
    }

    async fn workspace_configuration(
        self: Arc<Self>,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Toolchain>,
        _: Option<Uri>,
        cx: &mut AsyncApp,
    ) -> Result<Value> {
        let mut settings = settings();
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
        HashMap::from_iter([
            (
                LanguageName::new_static(PROPERTIES),
                "spring-boot-properties".into(),
            ),
            (
                LanguageName::new_static(YAML),
                "spring-boot-properties-yaml".into(),
            ),
        ])
    }

    fn process_diagnostics(
        &self,
        params: &mut lsp::PublishDiagnosticsParams,
        _: lsp::LanguageServerId,
    ) {
        // a properties file's type mismatch names the type only: its value is the file's text
        let text = params
            .uri
            .to_file_path()
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok());
        for diagnostic in &mut params.diagnostics {
            let value = text
                .as_deref()
                .and_then(|text| text_in(text, diagnostic.range));
            if let Some(message) = intellij_message(diagnostic.message.as_str(), value.as_deref()) {
                diagnostic.message = lsp::DiagnosticMessage::String(message);
                // IntelliJ's Invalid properties/YAML configuration inspections, all at Warning
                // level, where the Spring LS has duplicate keys and bad values as errors
                diagnostic.severity = Some(lsp::DiagnosticSeverity::WARNING);
            }
        }
    }
}

/// The text of a one-line range of `text`.
fn text_in(text: &str, range: lsp::Range) -> Option<String> {
    if range.start.line != range.end.line {
        return None;
    }
    let line = text.lines().nth(range.start.line as usize)?;
    let units: Vec<u16> = line.encode_utf16().collect();
    let part = units.get(range.start.character as usize..range.end.character as usize)?;
    String::from_utf16(part).ok()
}

/// The Spring LS's message for a configuration file's problem in IntelliJ's words (its
/// `SpringBootBundle`'s inspections), `None` for one IntelliJ has no message for.
fn intellij_message(message: &str, value: Option<&str>) -> Option<String> {
    static RULES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
        [
            (
                r"^Unknown property '(.+)' for type '(.+)'$",
                "Cannot resolve property '{1}' in {2}",
            ),
            (
                r"^'(.+)' is an unknown property\.",
                "Cannot resolve configuration property '{1}'",
            ),
            (
                r"^Unknown property '(.+)'$",
                "Cannot resolve configuration property '{1}'",
            ),
            (
                r"^Property '([^']+)'.* is Deprecated",
                "Deprecated configuration property '{1}'",
            ),
            (
                r"^Duplicate (?:property|key) '(.+)'$",
                "Duplicate configuration key '{1}'",
            ),
            (
                r"^Expecting a '(.+)' but got '(.*)'$",
                "Cannot convert '{2}' to {1}",
            ),
            (
                r"^'(.*)' is an unknown '.+'\. Valid values are: \[(.*)\]$",
                "Invalid value '{1}', must be one of {2}",
            ),
        ]
        .into_iter()
        .filter_map(|(pattern, text)| Some((Regex::new(pattern).log_err()?, text)))
        .collect()
    });
    let fill = |template: &str, captures: &[&str]| {
        captures
            .iter()
            .enumerate()
            .fold(template.to_owned(), |text, (ix, capture)| {
                text.replace(&format!("{{{}}}", ix + 1), capture)
            })
    };
    for (rule, template) in RULES.iter() {
        if let Some(captures) = rule.captures(message) {
            let captures: Vec<&str> = captures
                .iter()
                .skip(1)
                .map(|capture| capture.map_or("", |capture| capture.as_str()))
                .collect();
            return Some(fill(template, &captures));
        }
    }
    let expected = message.strip_prefix("Expecting '")?.strip_suffix('\'')?;
    Some(format!("Cannot convert '{}' to {expected}", value?.trim()))
}

/// The requests the Spring LS asks the client to answer, each answered by a bundle's JDT LS
/// command.
macro_rules! relayed {
    ($($request:ident = $method:literal => $command:literal,)*) => {
        $(
            enum $request {}

            impl lsp::request::Request for $request {
                type Params = Value;
                type Result = Value;
                const METHOD: &'static str = $method;
            }
        )*

        fn relay_all(jdtls: &Arc<LanguageServer>, spring: &LanguageServer) -> Vec<lsp::Subscription> {
            vec![$(relay::<$request>(jdtls, spring, $command)),*]
        }
    };
}

relayed! {
    AddClasspathListener = "sts/addClasspathListener" => "sts.java.addClasspathListener",
    RemoveClasspathListener = "sts/removeClasspathListener" => "sts.java.removeClasspathListener",
    JavaType = "sts/javaType" => "sts.java.type",
    JavadocHoverLink = "sts/javadocHoverLink" => "sts.java.javadocHoverLink",
    JavaLocation = "sts/javaLocation" => "sts.java.location",
    Javadoc = "sts/javadoc" => "sts.java.javadoc",
    JavaSearchTypes = "sts/javaSearchTypes" => "sts.java.search.types",
    JavaSearchPackages = "sts/javaSearchPackages" => "sts.java.search.packages",
    JavaSubTypes = "sts/javaSubTypes" => "sts.java.hierarchy.subtypes",
    JavaSuperTypes = "sts/javaSuperTypes" => "sts.java.hierarchy.supertypes",
    JavaCodeComplete = "sts/javaCodeComplete" => "sts.java.code.completions",
    ProjectGav = "sts/project/gav" => "sts.project.gav",
}

/// Answers the Spring LS's request `R` with JDT LS's `command`.
fn relay<R>(
    jdtls: &Arc<LanguageServer>,
    spring: &LanguageServer,
    command: &'static str,
) -> lsp::Subscription
where
    R: lsp::request::Request<Params = Value, Result = Value>,
{
    let jdtls = Arc::downgrade(jdtls);
    spring.on_request::<R, _, _>(move |params, _| {
        let jdtls = jdtls.clone();
        async move {
            let jdtls = jdtls
                .upgrade()
                .context("the Java language server has stopped")?;
            execute(&jdtls, command, relayed_arguments(R::METHOD, params)).await
        }
    })
}

/// The arguments of a relayed request's JDT LS command: the classpath listeners take their
/// callback's id, the others their request's parameters.
fn relayed_arguments(method: &str, params: Value) -> Vec<Value> {
    match method {
        "sts/addClasspathListener" | "sts/removeClasspathListener" => {
            vec![params.get("callbackCommandId").cloned().unwrap_or(params)]
        }
        _ => vec![params],
    }
}

/// JDT LS's `workspace/executeClientCommand`: a command the client runs for JDT LS, here the
/// Spring LS's classpath callback.
enum ExecuteClientCommand {}

#[derive(serde::Deserialize, serde::Serialize)]
struct ClientCommand {
    command: String,
    #[serde(default)]
    arguments: Vec<Value>,
}

impl lsp::request::Request for ExecuteClientCommand {
    type Params = ClientCommand;
    type Result = Value;
    const METHOD: &'static str = "workspace/executeClientCommand";
}

/// The Spring LS's `spring/index/updated`: its index of beans and request mappings changed.
enum IndexUpdated {}

impl lsp::notification::Notification for IndexUpdated {
    type Params = Value;
    const METHOD: &'static str = "spring/index/updated";
}

/// The bridge between a project's JDT LS and its Spring LS, alive while it is held.
pub struct Bridge {
    pub spring: Weak<LanguageServer>,
    jdtls: Weak<LanguageServer>,
    _subscriptions: Vec<lsp::Subscription>,
}

impl Bridge {
    /// Whether it bridges these two servers.
    pub fn connects(&self, jdtls: &Arc<LanguageServer>, spring: &Arc<LanguageServer>) -> bool {
        let same = |weak: &Weak<LanguageServer>, server| {
            weak.upgrade()
                .is_some_and(|ours| Arc::ptr_eq(&ours, server))
        };
        same(&self.jdtls, jdtls) && same(&self.spring, spring)
    }
}

/// Connects the two servers: the Spring LS's `sts/*` requests answered by JDT LS's commands,
/// JDT LS's callbacks run by the Spring LS. `classpath` is told each time JDT LS sends a
/// project's classpath, `indexed` each time the Spring LS's index changes. A server takes one handler per method: a previous bridge of either
/// server must be dropped first.
pub fn bridge(
    jdtls: &Arc<LanguageServer>,
    spring: &Arc<LanguageServer>,
    classpath: impl Fn(&mut AsyncApp) + Send + 'static,
    indexed: impl Fn(&mut AsyncApp) + Send + 'static,
) -> Bridge {
    let mut subscriptions = relay_all(jdtls, spring);
    subscriptions.push(spring.on_notification::<IndexUpdated, _>(move |_, cx| indexed(cx)));
    let to_spring = Arc::downgrade(spring);
    subscriptions.push(
        jdtls.on_request::<ExecuteClientCommand, _, _>(move |params, cx| {
            classpath(cx);
            let spring = to_spring.clone();
            async move {
                let spring = spring
                    .upgrade()
                    .context("the Spring Boot language server has stopped")?;
                execute(&spring, &params.command, params.arguments).await
            }
        }),
    );
    Bridge {
        spring: Arc::downgrade(spring),
        jdtls: Arc::downgrade(jdtls),
        _subscriptions: subscriptions,
    }
}

/// Starts the Spring LS's classpath listening, once the bridge is in place and JDT LS has
/// imported the project.
pub async fn enable_classpath_listening(spring: &LanguageServer) -> Result<Value> {
    execute(
        spring,
        "sts.vscode-spring-boot.enableClasspathListening",
        vec![Value::Bool(true)],
    )
    .await
}

async fn execute(server: &LanguageServer, command: &str, arguments: Vec<Value>) -> Result<Value> {
    let result = server
        .request::<lsp::request::ExecuteCommand>(
            lsp::ExecuteCommandParams {
                command: command.into(),
                arguments,
                ..Default::default()
            },
            Duration::from_secs(120),
        )
        .await
        .into_response()?;
    Ok(result.unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::{
        SpringBootLsAdapter, intellij_message, relayed_arguments, runtime_folder, runtime_in,
        text_in,
    };
    use language::LspAdapter as _;
    use serde_json::json;

    #[test]
    fn the_servers_jdk_is_their_own_not_a_project_one() {
        assert!(runtime_folder().starts_with(paths::data_dir()));
        assert!(!runtime_folder().starts_with(crate::jdk_download::default_folder()));

        let folder = tempfile::tempdir().expect("a temporary folder");
        let jdk = |name: &str, version: &str| {
            let home = folder.path().join(name).join("Contents/Home");
            std::fs::create_dir_all(home.join("bin")).expect("bin");
            std::fs::write(home.join("bin/java"), "").expect("java");
            std::fs::write(
                home.join("release"),
                format!("IMPLEMENTOR=\"Eclipse Adoptium\"\nJAVA_VERSION=\"{version}\"\n"),
            )
            .expect("release");
        };
        assert_eq!(runtime_in(folder.path()), None);
        jdk("temurin-17.0.13", "17.0.13");
        assert_eq!(runtime_in(folder.path()), None);
        jdk("temurin-21.0.5", "21.0.5");
        assert_eq!(
            runtime_in(folder.path()).map(|jdk| jdk.version),
            Some("21.0.5".into())
        );
    }

    #[test]
    fn configuration_problems_are_intellijs_warnings() {
        let diagnostic = |severity, message: &str| lsp::Diagnostic {
            severity: Some(severity),
            message: lsp::DiagnosticMessage::String(message.into()),
            ..Default::default()
        };
        let mut params = lsp::PublishDiagnosticsParams {
            uri: lsp::Uri::from_file_path("/nowhere/application.properties").expect("a uri"),
            diagnostics: vec![
                diagnostic(
                    lsp::DiagnosticSeverity::ERROR,
                    "Duplicate property 'server.port'",
                ),
                diagnostic(
                    lsp::DiagnosticSeverity::ERROR,
                    "Expecting a 'int' but got 'abc'",
                ),
                diagnostic(lsp::DiagnosticSeverity::ERROR, "Something else"),
            ],
            version: None,
        };
        SpringBootLsAdapter.process_diagnostics(&mut params, lsp::LanguageServerId(0));
        let shown: Vec<_> = params
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.severity, diagnostic.message.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                (
                    Some(lsp::DiagnosticSeverity::WARNING),
                    "Duplicate configuration key 'server.port'"
                ),
                (
                    Some(lsp::DiagnosticSeverity::WARNING),
                    "Cannot convert 'abc' to int"
                ),
                (Some(lsp::DiagnosticSeverity::ERROR), "Something else"),
            ]
        );
    }

    #[test]
    fn the_spring_messages_read_as_intellijs() {
        let message = |text: &str| intellij_message(text, None);
        assert_eq!(
            message("'server.prot' is an unknown property. Did you mean 'server.port'?").as_deref(),
            Some("Cannot resolve configuration property 'server.prot'")
        );
        assert_eq!(
            message("Unknown property 'foo'").as_deref(),
            Some("Cannot resolve configuration property 'foo'")
        );
        assert_eq!(
            message("Unknown property 'bar' for type 'Server'").as_deref(),
            Some("Cannot resolve property 'bar' in Server")
        );
        assert_eq!(
            message("Property 'server.max-http-header-size' is Deprecated: Use 'server.max-http-request-header-size' instead.")
                .as_deref(),
            Some("Deprecated configuration property 'server.max-http-header-size'")
        );
        assert_eq!(
            message("Duplicate property 'server.port'").as_deref(),
            Some("Duplicate configuration key 'server.port'")
        );
        assert_eq!(
            message("Expecting a 'int' but got 'abc'").as_deref(),
            Some("Cannot convert 'abc' to int")
        );
        assert_eq!(
            intellij_message("Expecting 'int'", Some("abc")).as_deref(),
            Some("Cannot convert 'abc' to int")
        );
        assert_eq!(message("Expecting 'int'"), None);
        assert_eq!(message("Something else"), None);
    }

    #[test]
    fn reads_a_ranges_text() {
        let range = lsp::Range::new(lsp::Position::new(1, 12), lsp::Position::new(1, 15));
        assert_eq!(
            text_in("a=1\nserver.port=abc\n", range).as_deref(),
            Some("abc")
        );
    }

    #[test]
    fn a_classpath_listener_is_named_by_its_callback() {
        assert_eq!(
            relayed_arguments(
                "sts/addClasspathListener",
                json!({"callbackCommandId": "x"})
            ),
            vec![json!("x")]
        );
        assert_eq!(
            relayed_arguments("sts/javaType", json!({"bindingKey": "k"})),
            vec![json!({"bindingKey": "k"})]
        );
    }
}
