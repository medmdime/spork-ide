//! Spring Boot as IntelliJ IDEA's paid edition has it (its Spring Boot plugin): a Spring Boot
//! project is found when it is opened, each of its applications gets a Spring Boot run
//! configuration on import, the Spring Boot type runs and debugs them as an Application with
//! IntelliJ's Spring options turned into VM options, Services shows them starting and their
//! port's link once started, and the Spring Boot language server ([`crate::spring_boot_ls`])
//! gives `application*.properties` and `application*.yml` IntelliJ's code insight.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use collections::HashMap;
use dap::adapters::{
    DapDelegate, DebugAdapter, DebugAdapterBinary, DebugAdapterName, DebugTaskDefinition,
};
use gpui::{App, AppContext as _, AsyncApp, Context, Entity, SharedString, Task, TaskExt as _};
use language::{Buffer, LanguageName};
use project::lsp_store::OpenLspBufferHandle;
use regex::Regex;
use serde_json::{Map, Value, json};
use spork_run::ProjectConfigurations;
use spork_run::configurations::{
    self, Configuration, Field, ModifyOption, Runner, SPRING_BOOT, ServiceState,
};
use task::{DebugScenario, TaskTemplate, ZedDebugConfig};
use util::ResultExt as _;
use workspace::Workspace;

use crate::application::{ApplicationRunner, JavaDebugAdapter};
use crate::java_project::{JavaProject, execute_command};
use crate::spring_boot_ls;

/// Whether the project at `root` is a Spring Boot project: its build file names Spring Boot (the
/// parent, the BOM, a starter, the plugin), as the import then finds `spring-boot` on its
/// classpath (IntelliJ's `org.springframework.boot:spring-boot` dependency).
// shortcut: the root's build file alone; read the modules' once multi-module builds are imported
pub fn is_project(root: &Path) -> bool {
    ["pom.xml", "build.gradle", "build.gradle.kts"]
        .iter()
        .any(|file| {
            std::fs::read_to_string(root.join(file))
                .is_ok_and(|text| text.contains("org.springframework.boot"))
        })
}

/// Whether a Java source is a Spring Boot application's: a class annotated
/// `@SpringBootApplication` or `@EnableAutoConfiguration` whose `main` starts Spring
/// (`SpringApplication.run`, `SpringApplication.from`, `SpringApplicationBuilder`, Kotlin's
/// `runApplication`), IntelliJ's rule.
pub fn is_application(text: &str) -> bool {
    let annotated = ["@SpringBootApplication", "@EnableAutoConfiguration"]
        .iter()
        .any(|annotation| text.contains(annotation));
    let starts = [
        "SpringApplication.run(",
        "SpringApplication.from(",
        "SpringApplicationBuilder(",
        "runApplication<",
        "runApplication(",
    ]
    .iter()
    .any(|call| text.contains(call));
    annotated && starts
}

/// The configuration's keys of the Spring options (java-debug ignores them).
const DEBUG_OUTPUT: &str = "debugOutput";
const HIDE_BANNER: &str = "hideBanner";
const NO_LAUNCH_OPTIMIZATION: &str = "disableLaunchOptimization";
const NO_JMX: &str = "disableJmx";
const ACTIVE_PROFILES: &str = "activeProfiles";

/// The configuration's Running Application Update Policies: a policy's id, "" for Do nothing.
const UPDATE_ACTION: &str = "updateAction";
const FRAME_DEACTIVATION: &str = "frameDeactivation";

/// IntelliJ's update policies (`SpringBootApplicationUpdatePolicy`), by id, in its order.
const UPDATE_RESOURCES: &str = "UpdateResources";
const UPDATE_CLASSES: &str = "UpdateClassesAndResources";
const UPDATE_TRIGGER_FILE: &str = "UpdateTriggerFile";
const HOT_SWAP_OR_TRIGGER_FILE: &str = "UpdateClassesAndTriggerFile";
const POLICIES: [(&str, &str); 4] = [
    (UPDATE_RESOURCES, "Update resources"),
    (UPDATE_CLASSES, "Update classes and resources"),
    (UPDATE_TRIGGER_FILE, "Update trigger file"),
    (
        HOT_SWAP_OR_TRIGGER_FILE,
        "Hot swap classes and update trigger file if failed",
    ),
];
const DO_NOTHING: &str = "Do nothing";

/// The file Spring Boot's DevTools restarts on, in the classes' folders (IntelliJ's
/// `TriggerFilePolicy`).
const TRIGGER_FILE: &str = ".restartTriggerFile";

/// The policy a configuration sets for `key`, when it sets one.
fn policy(entry: &Map<String, Value>, key: &str) -> Option<&'static str> {
    let id = entry.get(key)?.as_str()?;
    POLICIES
        .iter()
        .map(|(id, _)| *id)
        .find(|known| *known == id)
}

fn is_trigger_file(policy: &str) -> bool {
    matches!(policy, UPDATE_TRIGGER_FILE | HOT_SWAP_OR_TRIGGER_FILE)
}

/// Whether the project at `root` has Spring Boot's DevTools, which the trigger file policies need.
fn has_devtools(root: &Path) -> bool {
    ["pom.xml", "build.gradle", "build.gradle.kts"]
        .iter()
        .any(|file| {
            std::fs::read_to_string(root.join(file))
                .is_ok_and(|text| text.contains("spring-boot-devtools"))
        })
}

/// The property naming a run's JVM so it can be found again, as IntelliJ's
/// `ij.spring.boot.application.process`.
const PROCESS_MARKER: &str = "spork.boot.process";

/// IntelliJ's checks of the configuration (`SpringBootApplicationRunConfiguration`).
const NO_CLASS: &str = "No Spring Boot application class specified";
const NOT_AN_APPLICATION: &str = "Not a valid Spring Boot application class";

fn flag(entry: &Map<String, Value>, key: &str) -> bool {
    entry.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn toggle_flag(configuration: &mut Configuration, key: &str) {
    if configuration.entry.remove(key).is_none() {
        configuration.entry.insert(key.into(), Value::Bool(true));
    }
}

/// The VM options IntelliJ's Spring Boot configuration adds before the user's: the launch
/// optimization, debug output, the banner, the active profiles, the properties overridden, ANSI
/// colours, the JMX endpoints the Actuator tab reads, and the run's marker.
pub(crate) fn vm_options(
    entry: &Map<String, Value>,
    jdk_major: u32,
    project: &str,
    marker: &str,
) -> Vec<String> {
    let mut options = Vec::new();
    if !flag(entry, NO_LAUNCH_OPTIMIZATION) {
        options.push("-XX:TieredStopAtLevel=1".to_owned());
        if jdk_major < 13 {
            options.push("-noverify".to_owned());
        }
    }
    if flag(entry, DEBUG_OUTPUT) {
        options.push("-Ddebug".to_owned());
    }
    if flag(entry, HIDE_BANNER) {
        options.push("-Dspring.main.banner-mode=OFF".to_owned());
    }
    let profiles: Vec<&str> = entry
        .get(ACTIVE_PROFILES)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|profile| !profile.is_empty())
        .collect();
    if !profiles.is_empty() {
        options.push(format!("-Dspring.profiles.active={}", profiles.join(",")));
    }
    let configuration = Configuration {
        adapter: Some(SPRING_BOOT.into()),
        shared: true,
        entry: entry.clone(),
    };
    for (name, value) in configurations::override_properties(&configuration) {
        if !name.trim().is_empty() {
            options.push(format!("-D{}={value}", name.trim()));
        }
    }
    if policy(entry, UPDATE_ACTION).is_some_and(is_trigger_file) {
        options.push(format!(
            "-Dspring.devtools.restart.trigger-file={TRIGGER_FILE}"
        ));
    }
    options.push("-Dspring.output.ansi.enabled=always".to_owned());
    if !flag(entry, NO_JMX) {
        options.extend(
            [
                "-Dcom.sun.management.jmxremote",
                "-Dspring.jmx.enabled=true",
                "-Dspring.liveBeansView.mbeanDomain",
                "-Dspring.application.admin.enabled=true",
                "-Dmanagement.endpoints.jmx.exposure.include=*",
            ]
            .map(str::to_owned),
        );
        // the Spring Boot language server finds a project's live process by it
        options.push(format!("-Dspring.boot.project.name={project}"));
    }
    options.push(format!("-D{PROCESS_MARKER}={marker}"));
    options
}

/// A run's marker: unique enough to tell this IDE's runs apart.
pub(crate) fn marker() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{:x}", now.as_nanos())
}

/// An option as the run's shell reads it: quoted when the shell would expand it, as IntelliJ's
/// command line shows `"-Dmanagement.endpoints.jmx.exposure.include=*"`.
fn for_shell(option: String) -> String {
    match option.contains(['*', '?', '[', ' ']) {
        true => format!("\"{option}\""),
        false => option,
    }
}

/// The configuration's own VM options, as java-debug takes them: a line or a list.
fn user_vm_options(entry: &Map<String, Value>) -> Vec<String> {
    match entry.get("vmArgs") {
        Some(Value::String(text)) => configurations::split_arguments(text),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// IntelliJ's checks before a run: the main class, the properties' names.
fn check(configuration: &Configuration) -> Result<String> {
    let main = configuration.text(Field::Command).trim().to_owned();
    anyhow::ensure!(!main.is_empty(), NO_CLASS);
    for (ix, (name, _)) in configurations::override_properties(configuration)
        .iter()
        .enumerate()
    {
        anyhow::ensure!(
            !name.trim().is_empty(),
            "Empty property name in row {}",
            ix + 1
        );
    }
    Ok(main)
}

/// Whether `main` is one of the project's Spring Boot applications, as JDT LS finds its mains.
async fn check_application(server: &lsp::LanguageServer, root: &Path, main: &str) -> Result<()> {
    let applications = applications(server, root).await?;
    anyhow::ensure!(
        applications.iter().any(|(found, _)| found == main),
        NOT_AN_APPLICATION
    );
    Ok(())
}

/// The project's Spring Boot applications: the main classes JDT LS finds (java-debug's
/// `resolveMainClass`) outside test sources whose class is a Spring Boot application's, with
/// their files.
async fn applications(server: &lsp::LanguageServer, root: &Path) -> Result<Vec<(String, PathBuf)>> {
    let root_uri = lsp::Uri::from_file_path(root)
        .map_err(|()| anyhow::anyhow!("{root:?} isn't a file path"))?
        .to_string();
    let mains = execute_command(
        server,
        "vscode.java.resolveMainClass",
        vec![root_uri.into()],
    )
    .await?;
    let mut found = Vec::new();
    for main in mains.as_array().into_iter().flatten() {
        let (Some(class), Some(file)) = (main["mainClass"].as_str(), main["filePath"].as_str())
        else {
            continue;
        };
        let file = PathBuf::from(file);
        if file.components().any(|part| part.as_os_str() == "test") {
            continue;
        }
        if std::fs::read_to_string(&file).is_ok_and(|text| is_application(&text)) {
            found.push((class.to_owned(), file));
        }
    }
    Ok(found)
}

pub(crate) struct SpringBootRunner;

impl SpringBootRunner {
    /// The configuration as an Application one runs it: Spring's VM options before its own,
    /// the run marked with `marker`.
    fn as_application(
        configuration: &Configuration,
        jdk_major: u32,
        project: &str,
        marker: &str,
        shell: bool,
    ) -> Configuration {
        let mut options: Vec<String> = vm_options(&configuration.entry, jdk_major, project, marker);
        if shell {
            options = options.into_iter().map(for_shell).collect();
        }
        options.extend(user_vm_options(&configuration.entry));
        let mut application = configuration.clone();
        application.entry.insert(
            "vmArgs".into(),
            Value::Array(options.into_iter().map(Value::from).collect()),
        );
        application
    }
}

impl Runner for SpringBootRunner {
    fn task(
        &self,
        configuration: &Configuration,
        workspace: &Workspace,
        cx: &mut App,
    ) -> Task<Result<TaskTemplate>> {
        let main = match check(configuration) {
            Ok(main) => main,
            Err(error) => return Task::ready(Err(error)),
        };
        let Some(java) = JavaProject::of(workspace, cx) else {
            return Task::ready(Err(anyhow::anyhow!("The project isn't a Java project")));
        };
        let (jdk, root, name) = {
            let java = java.read(cx);
            (java.jdk.clone(), java.root.clone(), java.name())
        };
        let (Some(jdk), Some(root)) = (jdk, root) else {
            return Task::ready(Err(anyhow::anyhow!(crate::jdtls::NO_JDK)));
        };
        let server = java.read(cx).language_server(cx);
        let marker = remember_run(&java, configuration, cx);
        let application = Self::as_application(configuration, jdk.major, &name, &marker, true);
        let run = ApplicationRunner.task(&application, workspace, cx);
        cx.background_spawn(async move {
            if let Some(server) = server {
                check_application(&server, &root, &main).await?;
            }
            run.await
        })
    }

    fn can_debug(&self, cx: &App) -> bool {
        ApplicationRunner.can_debug(cx)
    }

    /// The Application's rows (its JRE), then the update policies Modify options shows.
    fn controls(
        &self,
        configuration: &Configuration,
        edit: configurations::Edit,
        cx: &mut App,
    ) -> Vec<gpui::AnyElement> {
        let mut rows = ApplicationRunner.controls(configuration, edit.clone(), cx);
        for (key, label, on_deactivation) in [
            (UPDATE_ACTION, "On 'Update' action:", false),
            (FRAME_DEACTIVATION, "On frame deactivation:", true),
        ] {
            if configuration.entry.contains_key(key) {
                rows.push(policy_field(
                    configuration,
                    key,
                    label,
                    on_deactivation,
                    edit.clone(),
                ));
            }
        }
        rows
    }

    fn update_text(
        &self,
        configuration: &Configuration,
        workspace: &Workspace,
        cx: &App,
    ) -> Option<SharedString> {
        let policy = policy(&configuration.entry, UPDATE_ACTION)?;
        let root = JavaProject::of(workspace, cx)?.read(cx).root.clone()?;
        // IntelliJ's trigger file policies are there with DevTools only
        (!is_trigger_file(policy) || has_devtools(&root))
            .then(|| SharedString::from(format!("Update '{}' application", configuration.name())))
    }

    fn update(
        &self,
        configuration: &Configuration,
        session: Option<Entity<project::debugger::session::Session>>,
        workspace: &Entity<Workspace>,
        cx: &mut App,
    ) {
        if let Some(policy) = policy(&configuration.entry, UPDATE_ACTION) {
            run_policy(policy, session, workspace, cx);
        }
    }

    // IntelliJ's Spring Boot options: the Application's, then the "Spring" group
    fn modify_options(&self, configuration: &Configuration) -> Vec<ModifyOption> {
        let entry = &configuration.entry;
        let mut options = ApplicationRunner.modify_options(configuration);
        options.extend([
            ModifyOption {
                group: "Spring",
                label: "Active profiles",
                checked: entry.contains_key(ACTIVE_PROFILES),
                toggle: Some(|configuration| {
                    if configuration.entry.remove(ACTIVE_PROFILES).is_none() {
                        configuration
                            .entry
                            .insert(ACTIVE_PROFILES.into(), "".into());
                    }
                }),
            },
            ModifyOption {
                group: "Spring",
                label: "Enable debug output",
                checked: flag(entry, DEBUG_OUTPUT),
                toggle: Some(|configuration| toggle_flag(configuration, DEBUG_OUTPUT)),
            },
            ModifyOption {
                group: "Spring",
                label: "Hide banner",
                checked: flag(entry, HIDE_BANNER),
                toggle: Some(|configuration| toggle_flag(configuration, HIDE_BANNER)),
            },
            ModifyOption {
                group: "Spring",
                label: "Disable launch optimization",
                checked: flag(entry, NO_LAUNCH_OPTIMIZATION),
                toggle: Some(|configuration| toggle_flag(configuration, NO_LAUNCH_OPTIMIZATION)),
            },
            ModifyOption {
                group: "Spring",
                label: "Disable JMX endpoints",
                checked: flag(entry, NO_JMX),
                toggle: Some(|configuration| toggle_flag(configuration, NO_JMX)),
            },
            ModifyOption {
                group: "Spring",
                label: "On 'Update' action",
                checked: entry.contains_key(UPDATE_ACTION),
                toggle: Some(|configuration| toggle_policy(configuration, UPDATE_ACTION)),
            },
            ModifyOption {
                group: "Spring",
                label: "On frame deactivation",
                checked: entry.contains_key(FRAME_DEACTIVATION),
                toggle: Some(|configuration| toggle_policy(configuration, FRAME_DEACTIVATION)),
            },
            ModifyOption {
                group: "Spring",
                label: "Override configuration properties",
                checked: entry.contains_key(configurations::OVERRIDE_PROPERTIES),
                toggle: Some(|configuration| {
                    if configuration
                        .entry
                        .remove(configurations::OVERRIDE_PROPERTIES)
                        .is_none()
                    {
                        configurations::set_override_properties(configuration, &[]);
                    }
                }),
            },
        ]);
        options
    }

    fn shows(&self, configuration: &Configuration, field: Field) -> bool {
        match field {
            Field::ActiveProfiles => configuration.entry.contains_key(ACTIVE_PROFILES),
            field => ApplicationRunner.shows(configuration, field),
        }
    }

    fn service(&self, configuration: &Configuration, console: &str) -> Option<ServiceState> {
        Some(service_state(configuration, console))
    }

    fn has_default_path(&self) -> bool {
        true
    }

    fn content(
        &self,
        configuration: &Configuration,
        workspace: &Entity<Workspace>,
        _: &mut gpui::Window,
        cx: &mut App,
    ) -> Option<(SharedString, gpui::AnyView)> {
        let java = JavaProject::of(workspace.read(cx), cx)?;
        let main = configuration.text(Field::Command).trim().to_owned();
        let view = crate::actuator::view(&java, configuration.name(), main, cx);
        Some(("Actuator".into(), view.into()))
    }
}

/// What a Spring Boot application's console says of it: started once Spring Boot logs
/// `Started DemoApplication in 1.2 seconds` (IntelliJ's `Started [a-zA-Z0-9_.]* in `), and its
/// web server's port, which the link opens with the context path and the default path.
// shortcut: the port from the web server's log line, where IntelliJ reads `local.server.port`
// over JMX; read it from the Spring LS's live data once the Actuator tab connects to it
fn service_state(configuration: &Configuration, console: &str) -> ServiceState {
    static PORT: std::sync::LazyLock<Option<Regex>> = std::sync::LazyLock::new(|| {
        Regex::new(r"(?:Tomcat|Jetty|Undertow|Netty) started on port(?:s|\(s\))?:? (\d+)(?:.*with context path '([^']*)')?")
            .log_err()
    });
    let started = started(console);
    let link = started
        .then(|| PORT.as_ref()?.captures(console))
        .flatten()
        .and_then(|captures| {
            let port = captures.get(1)?.as_str().to_owned();
            let context = captures
                .get(2)
                .map_or("", |path| path.as_str())
                .trim_end_matches('/');
            let path = configuration
                .entry
                .get(configurations::URL_PATH)
                .and_then(Value::as_str)
                .unwrap_or("/");
            let path = match path.starts_with('/') {
                true => path.to_owned(),
                false => format!("/{path}"),
            };
            Some((
                SharedString::from(format!(":{port}{context}{path}")),
                format!("http://localhost:{port}{context}{path}"),
            ))
        });
    ServiceState {
        starting: !started,
        link,
    }
}

/// Whether a Spring Boot application's console says it started (IntelliJ's
/// `Started [a-zA-Z0-9_.]* in `).
pub(crate) fn started(console: &str) -> bool {
    static STARTED: std::sync::LazyLock<Option<Regex>> =
        std::sync::LazyLock::new(|| Regex::new(r"Started [a-zA-Z0-9_.$]* in ").log_err());
    STARTED
        .as_ref()
        .is_some_and(|started| started.is_match(console))
}

/// Modify options' On 'Update' action or On frame deactivation: its row, Do nothing at first.
fn toggle_policy(configuration: &mut Configuration, key: &str) {
    if configuration.entry.remove(key).is_none() {
        configuration.entry.insert(key.into(), "".into());
    }
}

/// An update policy's combo: Do nothing and the policies, those of the trigger file not on frame
/// deactivation (IntelliJ's `isAvailableOnFrameDeactivation`).
fn policy_field(
    configuration: &Configuration,
    key: &'static str,
    label: &'static str,
    on_deactivation: bool,
    edit: configurations::Edit,
) -> gpui::AnyElement {
    use gpui::{IntoElement as _, ParentElement as _, Styled as _};
    let chosen = policy(&configuration.entry, key)
        .and_then(|id| POLICIES.iter().find(|(known, _)| *known == id))
        .map_or(DO_NOTHING, |(_, name)| *name);
    let choices: Vec<(&str, &str)> = std::iter::once(("", DO_NOTHING))
        .chain(
            POLICIES
                .iter()
                .copied()
                .filter(|(id, _)| !(on_deactivation && is_trigger_file(id))),
        )
        .collect();
    let combo = spork_workbench::controls::dropdown(
        spork_workbench::controls::ComboBox::new(key, chosen)
            .aria_label(label)
            .w_full(),
        false,
        move |mut menu, _, _| {
            for (id, name) in &choices {
                let (edit, id) = (edit.clone(), *id);
                menu = menu.item(
                    gpui_kit::component::menu::PopupMenuItem::new(*name).on_click(
                        move |_, _, cx| {
                            edit(
                                &|configuration: &mut Configuration| {
                                    configuration.entry.insert(key.into(), id.into());
                                },
                                cx,
                            )
                        },
                    ),
                );
            }
            menu
        },
    );
    gpui_kit::component::h_flex()
        .gap(gpui::px(8.))
        .child(
            gpui::div()
                .w(gpui::px(160.))
                .flex_none()
                .child(spork_workbench::controls::form_label(label)),
        )
        .child(gpui::div().flex_1().min_w_0().child(combo))
        .into_any_element()
}

/// Runs an update policy on a running application (IntelliJ's `runUpdate`): the build for the
/// resources and classes, then for the trigger file policies the file DevTools restarts on,
/// written once HotSwap fails when the application is debugged.
// shortcut: Update resources builds the classes too (JDT LS builds the whole project), where
// IntelliJ copies the resources only
fn run_policy(
    policy: &'static str,
    session: Option<Entity<project::debugger::session::Session>>,
    workspace: &Entity<Workspace>,
    cx: &mut App,
) {
    let Some(java) = JavaProject::of(workspace.read(cx), cx) else {
        return;
    };
    let Some(root) = java.read(cx).root.clone() else {
        return;
    };
    let build = (policy != UPDATE_TRIGGER_FILE).then(|| java.read(cx).build(cx));
    let classpath = java.read(cx).classpath(&root, cx);
    let proxy = session
        .filter(|_| policy == HOT_SWAP_OR_TRIGGER_FILE)
        .and_then(|session| crate::debugger::proxy_of(session.read(cx), cx));
    if let Some(proxy) = &proxy {
        // the build's classes are reloaded here, not asked about
        proxy.reloading.store(true, Ordering::Relaxed);
    }
    let workspace = workspace.downgrade();
    cx.spawn(async move |cx| {
        if let Some(build) = build {
            build.await?;
        }
        if !is_trigger_file(policy) {
            return Ok(());
        }
        if let Some(proxy) = proxy {
            let reloaded = cx.update(|cx| crate::debugger::reload_classes(workspace, proxy, cx));
            if reloaded.await {
                return Ok(());
            }
        }
        let folders: Vec<PathBuf> = classpath
            .await?
            .into_iter()
            .map(PathBuf::from)
            .filter(|path| path.starts_with(&root) && path.is_dir())
            .collect();
        anyhow::ensure!(
            !folders.is_empty(),
            "Failed to update trigger file: class roots not found"
        );
        cx.background_spawn(async move {
            for folder in folders {
                std::fs::write(folder.join(TRIGGER_FILE), "")?;
            }
            anyhow::Ok(())
        })
        .await
    })
    .detach_and_log_err(cx);
}

/// On frame deactivation: once the IDE is no longer the active app, each running Spring Boot
/// configuration's frame deactivation policy (IntelliJ's `FrameStateListener`).
fn frame_deactivated(workspace: &Entity<Workspace>, cx: &mut App) {
    let Some(configurations) = ProjectConfigurations::of(workspace.read(cx), cx) else {
        return;
    };
    let running: Vec<&'static str> = configurations
        .read(cx)
        .list
        .iter()
        .filter(|configuration| configuration.adapter.as_deref() == Some(SPRING_BOOT))
        .filter(|configuration| {
            spork_run::is_running(workspace.read(cx), &configuration.name(), cx)
        })
        .filter_map(|configuration| policy(&configuration.entry, FRAME_DEACTIVATION))
        .collect();
    // both policies build the project, once is enough
    if let Some(policy) = running.first() {
        run_policy(policy, None, workspace, cx);
    }
}

/// The Spring Boot type's debug adapter: java-debug, given Spring's VM options.
pub(crate) struct SpringBootDebugAdapter;

#[async_trait(?Send)]
impl DebugAdapter for SpringBootDebugAdapter {
    fn name(&self) -> DebugAdapterName {
        DebugAdapterName(SPRING_BOOT.into())
    }

    async fn config_from_zed_format(&self, zed_scenario: ZedDebugConfig) -> Result<DebugScenario> {
        JavaDebugAdapter.config_from_zed_format(zed_scenario).await
    }

    async fn get_binary(
        &self,
        delegate: &Arc<dyn DapDelegate>,
        definition: &DebugTaskDefinition,
        user_installed_path: Option<PathBuf>,
        user_args: Option<Vec<String>>,
        user_env: Option<HashMap<String, String>>,
        cx: &mut AsyncApp,
    ) -> Result<DebugAdapterBinary> {
        let entry = definition
            .config
            .as_object()
            .cloned()
            .context("a Spring Boot configuration is a JSON object")?;
        let configuration = Configuration {
            adapter: Some(SPRING_BOOT.into()),
            shared: true,
            entry,
        };
        let main = check(&configuration)?;
        let root = delegate.worktree_root_path().to_owned();
        let (major, name, server, marker) = cx.update(|cx| {
            let java =
                JavaProject::containing(&root, cx).context("The project isn't a Java project")?;
            let marker = remember_run(&java, &configuration, cx);
            let java = java.read(cx);
            let major = java.jdk.as_ref().map_or(17, |jdk| jdk.major);
            anyhow::Ok((major, java.name(), java.language_server(cx), marker))
        })?;
        if let Some(server) = server {
            check_application(&server, &root, &main).await?;
        }
        let application =
            SpringBootRunner::as_application(&configuration, major, &name, &marker, false);
        let mut definition = definition.clone();
        definition.config = Value::Object(application.entry);
        JavaDebugAdapter
            .get_binary(
                delegate,
                &definition,
                user_installed_path,
                user_args,
                user_env,
                cx,
            )
            .await
    }

    fn adapter_language_name(&self) -> Option<LanguageName> {
        Some(LanguageName::new_static("Java"))
    }

    fn dap_schema(&self) -> Value {
        let mut schema = JavaDebugAdapter.dap_schema();
        if let Some(properties) = schema["properties"].as_object_mut() {
            properties.insert(
                ACTIVE_PROFILES.into(),
                json!({"type": "string", "description": "Comma-separated list of profiles"}),
            );
        }
        schema
    }
}

/// A new run of `configuration`: its marker, which its Actuator tab finds its JVM by.
fn remember_run(java: &Entity<JavaProject>, configuration: &Configuration, cx: &mut App) -> String {
    let marker = marker();
    java.update(cx, |java, cx| {
        java.spring
            .markers
            .insert(configuration.name().to_string(), marker.clone());
        cx.notify();
    });
    marker
}

/// A Spring Boot project's Spring state, kept on its Java project.
#[derive(Default)]
pub(crate) struct SpringBoot {
    /// The configuration file opened with the language servers, which keeps the Spring LS
    /// running for the project.
    config_file: Option<(Entity<Buffer>, OpenLspBufferHandle)>,
    bridge: Option<spring_boot_ls::Bridge>,
    /// JDT LS has sent the Spring LS a project's classpath.
    classpath: Arc<AtomicBool>,
    /// The applications found at the last import.
    applications: Vec<String>,
    /// Each configuration's latest run, by the marker its JVM carries.
    pub(crate) markers: HashMap<String, String>,
    /// Each configuration's Actuator tab.
    pub(crate) actuators: HashMap<String, Entity<crate::actuator::ActuatorView>>,
    /// The Spring LS's index: the endpoints and beans.
    pub(crate) index: crate::endpoints::SpringIndex,
    task: Option<Task<()>>,
}

/// After JDT LS imported (or reimported) a Spring Boot project: a Spring Boot configuration for
/// each application without one, the first new one selected (IntelliJ's
/// `SpringInitializrRunConfigurationService`), and the Spring LS started with its bridge.
pub(crate) fn imported(java: &mut JavaProject, cx: &mut Context<JavaProject>) {
    let Some(root) = java.root.clone() else {
        return;
    };
    if !is_project(&root) {
        return;
    }
    let task = cx.spawn(async move |this, cx| {
        // shortcut: polls until JDT LS is idle, as External Libraries do
        loop {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let Ok(busy) = this.read_with(cx, |this, cx| this.language_server_busy(cx)) else {
                return;
            };
            if busy == Some(false) {
                break;
            }
        }
        let Ok(Some(server)) = this.read_with(cx, |this, cx| this.language_server(cx)) else {
            return;
        };
        let found = applications(&server, &root)
            .await
            .log_err()
            .unwrap_or_default();
        this.update(cx, |this, cx| {
            this.spring.applications = found.iter().map(|(main, _)| main.clone()).collect();
            if spork_settings::spring_boot_creates_run_configurations(cx) {
                add_configurations(this, &found, cx);
            }
            start_language_server(this, &root, cx);
            cx.notify();
        })
        .ok();
        connect(this, cx).await;
    });
    java.spring.task = Some(task);
}

/// A Spring Boot configuration for each application that has none, named after its class (made
/// unique) and stored with the project; the first new one becomes the run widget's.
fn add_configurations(java: &JavaProject, found: &[(String, PathBuf)], cx: &mut App) {
    let Some(workspace) = java.workspace.upgrade() else {
        return;
    };
    let Some(configurations) = ProjectConfigurations::of(workspace.read(cx), cx) else {
        return;
    };
    configurations.update(cx, |configurations, cx| {
        let mut list = configurations.list.clone();
        let mut first = None;
        for (main, _) in found {
            if configuration_of(&list, main).is_some() {
                continue;
            }
            let name = unique_name(&list, simple_name(main));
            let mut configuration = Configuration::new_debug(SPRING_BOOT, &name);
            configuration.set_text(Field::Command, main);
            list.push(configuration);
            first.get_or_insert(name);
        }
        if let Some(name) = first {
            configurations.selected = Some(name.into());
            configurations.save(list, cx).detach_and_log_err(cx);
        }
    });
}

/// The Spring Boot configuration of the main class `main`.
fn configuration_of<'a>(list: &'a [Configuration], main: &str) -> Option<&'a Configuration> {
    list.iter().find(|configuration| {
        configuration.adapter.as_deref() == Some(SPRING_BOOT)
            && configuration.text(Field::Command).trim() == main
    })
}

fn simple_name(main: &str) -> &str {
    main.rsplit('.').next().unwrap_or(main)
}

/// `name`, or `name (1)`, `name (2)`… when a configuration has it.
fn unique_name(list: &[Configuration], name: &str) -> String {
    let taken = |candidate: &str| list.iter().any(|c| c.name().as_ref() == candidate);
    if !taken(name) {
        return name.to_owned();
    }
    (1..)
        .map(|ix| format!("{name} ({ix})"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| name.to_owned())
}

/// Opens the project's configuration file with the language servers, which starts the Spring LS
/// as a Java file starts JDT LS.
// shortcut: a project without `application.properties` or `.yml` in its main resources starts
// the Spring LS once one is opened; start it without a buffer once the language servers can
fn start_language_server(java: &mut JavaProject, root: &Path, cx: &mut Context<JavaProject>) {
    if java.spring.config_file.is_some() {
        return;
    }
    let resources = root.join("src/main/resources");
    let Some(file) = [
        "application.properties",
        "application.yml",
        "application.yaml",
    ]
    .iter()
    .map(|name| resources.join(name))
    .find(|path| path.is_file()) else {
        return;
    };
    let project = java.project.clone();
    let open = project.update(cx, |project, cx| project.open_local_buffer(file, cx));
    cx.spawn(async move |this, cx| {
        let buffer = open.await?;
        let handle = project.update(cx, |project, cx| {
            project.register_buffer_with_language_servers(&buffer, cx)
        });
        this.update(cx, |this, _| {
            this.spring.config_file = Some((buffer, handle))
        })
    })
    .detach_and_log_err(cx);
}

/// Waits for the Spring LS, then bridges it to JDT LS and starts its classpath listening.
async fn connect(this: gpui::WeakEntity<JavaProject>, cx: &mut AsyncApp) {
    // the first start downloads the server: give it its time
    for _ in 0..600 {
        let servers = this.read_with(cx, |this, cx| {
            let spring = spring_language_server(&this.project, cx);
            (this.language_server(cx), spring)
        });
        let Ok((jdtls, spring)) = servers else {
            return;
        };
        if let (Some(jdtls), Some(spring)) = (jdtls, spring) {
            // a reimport finds the servers bridged already; a server started again replaces
            // the bridge, the old one dropped before its handlers come again
            let bridged = this.update(cx, |this, _| {
                let bridged = this
                    .spring
                    .bridge
                    .as_ref()
                    .is_some_and(|bridge| bridge.connects(&jdtls, &spring));
                if !bridged {
                    this.spring.bridge = None;
                }
                bridged
            });
            if bridged.unwrap_or(true) {
                return;
            }
            let classpath = this
                .read_with(cx, |this, _| this.spring.classpath.clone())
                .unwrap_or_default();
            let indexed = this.clone();
            let bridge = spring_boot_ls::bridge(
                &jdtls,
                &spring,
                move |_| {
                    classpath.store(true, Ordering::Relaxed);
                },
                move |cx| crate::endpoints::on_index_updated(indexed.clone(), cx),
            );
            this.update(cx, |this, _| this.spring.bridge = Some(bridge))
                .ok();
            spring_boot_ls::enable_classpath_listening(&spring)
                .await
                .log_err();
            this.update(cx, crate::endpoints::refresh).log_err();
            return;
        }
        cx.background_executor()
            .timer(Duration::from_millis(500))
            .await;
    }
}

/// The Spring LS of `project`, once it runs.
pub(crate) fn spring_language_server(
    project: &Entity<project::Project>,
    cx: &App,
) -> Option<Arc<lsp::LanguageServer>> {
    let lsp_store = project.read(cx).lsp_store();
    let lsp_store = lsp_store.read(cx);
    let id = lsp_store
        .language_server_statuses()
        .find(|(_, status)| status.name == spring_boot_ls::SERVER_NAME)
        .map(|(id, _)| id)?;
    lsp_store.language_server_for_id(id)
}

/// The gutter's run icon on a Spring Boot application's `main`: its Spring Boot configuration,
/// made (and selected) when it has none, as IntelliJ's `SpringBootApplicationRunConfigurationProducer`
/// comes first; the run gets the configuration's VM options. `None` for another class.
pub(crate) fn gutter_options(
    java: &Entity<JavaProject>,
    main: &str,
    text: &str,
    cx: &mut App,
) -> Option<(Vec<String>, Vec<String>)> {
    let (root, workspace, major, name) = {
        let java = java.read(cx);
        (
            java.root.clone()?,
            java.workspace.upgrade()?,
            java.jdk.as_ref().map_or(17, |jdk| jdk.major),
            java.name(),
        )
    };
    if !is_project(&root) || !is_application(text) {
        return None;
    }
    let configurations = ProjectConfigurations::of(workspace.read(cx), cx)?;
    let configuration = configurations.update(cx, |configurations, cx| {
        if let Some(configuration) = configuration_of(&configurations.list, main) {
            return configuration.clone();
        }
        let mut list = configurations.list.clone();
        let name = unique_name(&list, simple_name(main));
        let mut configuration = Configuration::new_debug(SPRING_BOOT, &name);
        configuration.set_text(Field::Command, main);
        list.push(configuration.clone());
        configurations.selected = Some(name.into());
        configurations.save(list, cx).detach_and_log_err(cx);
        configuration
    });
    let marker = remember_run(java, &configuration, cx);
    let application = SpringBootRunner::as_application(&configuration, major, &name, &marker, true);
    let vm_options = user_vm_options(&application.entry);
    let arguments = configurations::split_arguments(&configuration.text(Field::Arguments));
    Some((vm_options, arguments))
}

/// Registers the Spring Boot type: its runner and debug adapter.
pub(crate) fn init(cx: &mut App) {
    configurations::register_runner(SPRING_BOOT, std::rc::Rc::new(SpringBootRunner), cx);
    dap::DapRegistry::global(cx).add_adapter(Arc::new(SpringBootDebugAdapter));
    cx.observe_new(|_: &mut Workspace, window, cx| {
        let Some(window) = window else { return };
        cx.observe_window_activation(window, |_, window, cx| {
            if window.is_window_active() {
                return;
            }
            // a dialog of the IDE's taking the focus isn't the frame's deactivation
            let workspace = cx.entity();
            cx.spawn(async move |_, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                cx.update(|cx| {
                    if cx.active_window().is_none() {
                        frame_deactivated(&workspace, cx);
                    }
                });
            })
            .detach();
        })
        .detach();
    })
    .detach();
}

/// What the end-to-end tests read of a Spring Boot project: whether it is one, its applications,
/// whether the Spring LS runs, is bridged and has had a classpath.
pub(crate) fn e2e_state(java: &JavaProject, cx: &App) -> Value {
    let project = java.root.as_deref().is_some_and(is_project);
    if !project {
        return Value::Null;
    }
    let spring = &java.spring;
    json!({
        "applications": spring.applications,
        "server": spring_language_server(&java.project, cx).map(|server| server.process_id()),
        "bridged": spring.bridge.as_ref().is_some_and(|bridge| bridge.spring.upgrade().is_some()),
        "classpath": spring.classpath.load(Ordering::Relaxed),
        "index": crate::endpoints::e2e_state(
            java,
            java.workspace.upgrade().as_ref().map(|workspace| workspace.read(cx)),
            cx,
        ),
        "actuator": spring
            .actuators
            .iter()
            .map(|(name, view)| (name.clone(), view.read(cx).e2e_state()))
            .collect::<serde_json::Map<_, _>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        Configuration, SPRING_BOOT, check, is_application, service_state, unique_name, vm_options,
    };
    use serde_json::json;

    fn configuration(entry: serde_json::Value) -> Configuration {
        Configuration {
            adapter: Some(SPRING_BOOT.into()),
            shared: true,
            entry: entry.as_object().cloned().unwrap_or_default(),
        }
    }

    #[test]
    fn a_spring_boot_application_is_annotated_and_starts_spring() {
        let application = "@SpringBootApplication\npublic class DemoApplication {\n  public static void main(String[] args) {\n    SpringApplication.run(DemoApplication.class, args);\n  }\n}";
        assert!(is_application(application));
        assert!(!is_application(
            "public class Main { public static void main(String[] a) {} }"
        ));
        assert!(!is_application("@SpringBootApplication\nclass Config {}"));
    }

    #[test]
    fn the_spring_options_become_intellijs_vm_options() {
        let entry = json!({
            "mainClass": "com.example.Demo",
            "activeProfiles": "dev, local",
            "hideBanner": true,
            "debugOutput": true,
            "properties": [{"name": "server.port", "value": "9090"}],
        });
        let options = vm_options(entry.as_object().expect("an object"), 17, "demo", "m1");
        assert_eq!(
            options,
            [
                "-XX:TieredStopAtLevel=1",
                "-Ddebug",
                "-Dspring.main.banner-mode=OFF",
                "-Dspring.profiles.active=dev,local",
                "-Dserver.port=9090",
                "-Dspring.output.ansi.enabled=always",
                "-Dcom.sun.management.jmxremote",
                "-Dspring.jmx.enabled=true",
                "-Dspring.liveBeansView.mbeanDomain",
                "-Dspring.application.admin.enabled=true",
                "-Dmanagement.endpoints.jmx.exposure.include=*",
                "-Dspring.boot.project.name=demo",
                "-Dspork.boot.process=m1",
            ]
        );
        let trigger = |policy: &str| {
            let entry = json!({ "updateAction": policy });
            vm_options(entry.as_object().expect("an object"), 17, "demo", "m3")
                .contains(&"-Dspring.devtools.restart.trigger-file=.restartTriggerFile".to_owned())
        };
        assert!(trigger("UpdateTriggerFile") && trigger("UpdateClassesAndTriggerFile"));
        assert!(!trigger("UpdateResources") && !trigger(""));
        let off = json!({"disableLaunchOptimization": true, "disableJmx": true});
        let options = vm_options(off.as_object().expect("an object"), 11, "demo", "m2");
        assert_eq!(options[0], "-Dspring.output.ansi.enabled=always");
        assert_eq!(options.len(), 2);
    }

    #[test]
    fn intellijs_checks() {
        assert_eq!(
            check(&configuration(json!({"mainClass": " "})))
                .err()
                .map(|error| error.to_string()),
            Some("No Spring Boot application class specified".into())
        );
        assert_eq!(
            check(&configuration(json!({"mainClass": "a.B", "properties": [{"name": "x", "value": "1"}, {"name": "", "value": "2"}]})))
                .err()
                .map(|error| error.to_string()),
            Some("Empty property name in row 2".into())
        );
    }

    #[test]
    fn started_once_spring_boot_says_so_with_the_ports_link() {
        let configuration = configuration(json!({"mainClass": "a.Demo"}));
        let starting = service_state(&configuration, "  .   ____\n:: Spring Boot ::\n");
        assert!(starting.starting);
        assert_eq!(starting.link, None);
        let started = service_state(
            &configuration,
            "o.s.b.w.e.tomcat.TomcatWebServer  : Tomcat started on port 8080 (http) with context path '/'\nc.e.d.DemoApplication : Started DemoApplication in 1.234 seconds (process running for 1.5)\n",
        );
        assert!(!started.starting);
        assert_eq!(
            started.link,
            Some((":8080/".into(), "http://localhost:8080/".into()))
        );
        let plain = service_state(&configuration, "Started Worker in 0.9 seconds");
        assert_eq!((plain.starting, plain.link), (false, None));
    }

    #[test]
    fn a_new_configuration_takes_a_free_name() {
        let list = vec![configuration(json!({"label": "DemoApplication"}))];
        assert_eq!(unique_name(&list, "DemoApplication"), "DemoApplication (1)");
        assert_eq!(unique_name(&list, "Other"), "Other");
    }
}
