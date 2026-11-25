//! The Actuator tab of a running Spring Boot application (IntelliJ's `SpringBootApplicationEndpoints`
//! tab beside the console): its Beans, Health, Mappings and Environment, read as IntelliJ reads
//! them, by attaching to the application's JVM and calling Spring Boot's endpoint MBeans over JMX
//! (`helpers/ActuatorEndpoints.java`, run with the application's JDK), with Refresh and the time of
//! the last update; IntelliJ's texts when Actuator or the mappings endpoint isn't there.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Role, SharedString, StatefulInteractiveElement as _, Styled as _,
    Task, WeakEntity, Window, div, px,
};
use gpui_kit::component::Selectable as _;
use gpui_kit::component::{h_flex, v_flex};
use serde_json::{Value, json};
use spork_workbench::{ActionButton, Tokens};
use util::ResultExt as _;

use crate::java_project::JavaProject;

const HELPER: &str = include_str!("../helpers/ActuatorEndpoints.java");

/// IntelliJ's texts (`SpringBootRunBundle`).
const NOT_ENABLED: &str = "Spring Boot Actuator is not enabled.";
const ADD_ACTUATOR: &str = "Add dependency to the spring-boot-starter-actuator to enable it";
const ACTUATOR_ADDED: &str = "Actuator dependency added.";
const RESTART: &str = "Restart application";
const MAPPINGS_DISABLED: &str = "Mapping endpoint is not enabled.";
const ADD_WEB: &str = "Add dependency to the spring-boot-starter-web to enable it";
const NO_PROCESS: &str = "Could not find application process to attach";
const NOT_READY: &str = "Application readiness check failed:";
const INDICATORS_DISABLED: &str =
    "All health indicators are disabled. Only overall status is available.";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Beans,
    Health,
    Mappings,
    Environment,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Beans, Tab::Health, Tab::Mappings, Tab::Environment];

    fn title(self) -> &'static str {
        match self {
            Tab::Beans => "Beans",
            Tab::Health => "Health",
            Tab::Mappings => "Mappings",
            Tab::Environment => "Environment",
        }
    }
}

/// What the endpoints gave at the last update.
#[derive(Default)]
struct Endpoints {
    beans: Vec<Bean>,
    health: Option<(String, Vec<(String, String)>)>,
    mappings: Option<Vec<[String; 3]>>,
    profiles: Vec<String>,
    /// Each property source with its properties and their values.
    environment: Vec<(String, Vec<(String, String)>)>,
    /// None of the endpoints is there: no Actuator.
    no_actuator: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct Bean {
    name: String,
    kind: String,
    dependencies: Vec<String>,
}

pub(crate) struct ActuatorView {
    java: WeakEntity<JavaProject>,
    configuration: SharedString,
    main: String,
    tab: Tab,
    data: Option<Endpoints>,
    error: Option<String>,
    loading: bool,
    /// The run whose data is shown, by its marker.
    fetched: Option<String>,
    updated: Option<String>,
    library: bool,
    selected_bean: Option<String>,
    /// The actuator starter was added to the build file.
    added: bool,
    task: Option<Task<()>>,
}

impl ActuatorView {
    pub(crate) fn new(
        java: WeakEntity<JavaProject>,
        configuration: SharedString,
        main: String,
    ) -> Self {
        Self {
            java,
            configuration,
            main,
            tab: Tab::Beans,
            data: None,
            error: None,
            loading: false,
            fetched: None,
            updated: None,
            library: false,
            selected_bean: None,
            added: false,
            task: None,
        }
    }

    pub(crate) fn set_main(&mut self, main: String) {
        self.main = main;
    }

    /// Reads the endpoints of the configuration's latest run.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(java) = self.java.upgrade() else {
            return;
        };
        let (jdk, marker) = {
            let java = java.read(cx);
            (
                java.jdk.clone(),
                java.spring
                    .markers
                    .get(self.configuration.as_ref())
                    .cloned(),
            )
        };
        let (Some(jdk), Some(marker)) = (jdk, marker) else {
            self.error = Some(NO_PROCESS.into());
            cx.notify();
            return;
        };
        self.loading = true;
        self.fetched = Some(marker.clone());
        let main = self.main.clone();
        cx.notify();
        let read = cx.background_spawn(async move {
            let helper = helper()?;
            let output = util::command::new_command(jdk.java())
                .arg(&helper)
                .arg(&main)
                .arg(&marker)
                .output()
                .await
                .context("running the Actuator helper")?;
            anyhow::ensure!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("Failed to retrieve endpoints data")
            );
            let text = String::from_utf8_lossy(&output.stdout).into_owned();
            let value: Value = serde_json::from_str(text.trim())?;
            if let Some(error) = value.get("error").and_then(Value::as_str) {
                anyhow::bail!("{error}");
            }
            Ok(parse(&value))
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let read = read.await;
            this.update(cx, |this, cx| {
                this.loading = false;
                match read {
                    Ok(data) => {
                        this.data = Some(data);
                        this.error = None;
                        this.updated = Some(now());
                    }
                    Err(error) => {
                        let error = format!("{error:#}");
                        // a restarted application not up yet: read it again once it started
                        if error.contains(NO_PROCESS) || error.contains(NOT_READY) {
                            this.fetched = None;
                        }
                        this.error = Some(error);
                    }
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    /// Add dependency to the spring-boot-starter-actuator: the starter in the build file, then
    /// the project synced.
    fn add_actuator(&mut self, cx: &mut Context<Self>) {
        let Some(java) = self.java.upgrade() else {
            return;
        };
        let added = java.read(cx).root.clone().map(|root| add_starter(&root));
        match added {
            Some(Ok(())) => {
                self.added = true;
                java.update(cx, |java, cx| java.reload(cx));
            }
            Some(Err(error)) => {
                self.error = Some(format!("Failed to add actuator dependency: {error:#}"))
            }
            None => {}
        }
        cx.notify();
    }

    /// What the end-to-end tests read of the tab.
    pub(crate) fn e2e_state(&self) -> Value {
        let data = self.data.as_ref();
        json!({
            "tab": self.tab.title(),
            "loading": self.loading,
            "error": self.error,
            "updated": self.updated.is_some(),
            "no_actuator": data.map(|data| data.no_actuator),
            "beans": data.map(|data| self.shown_beans(data).iter().map(|bean| bean.name.clone()).collect::<Vec<_>>()),
            "health": data.and_then(|data| data.health.as_ref()).map(|(status, components)| json!({"status": status, "components": components})),
            "mappings": data.map(|data| data.mappings.as_ref().map(|rows| self.shown_mappings(rows))),
            "profiles": data.map(|data| data.profiles.clone()),
            "sources": data.map(|data| data.environment.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>()),
        })
    }

    /// The application's beans, or every bean with Show Library Beans.
    fn shown_beans<'a>(&self, data: &'a Endpoints) -> Vec<&'a Bean> {
        let package = package(&self.main);
        data.beans
            .iter()
            .filter(|bean| self.library || bean.kind.starts_with(&package))
            .collect()
    }

    fn shown_mappings(&self, rows: &[[String; 3]]) -> Vec<[String; 3]> {
        let package = package(&self.main);
        rows.iter()
            .filter(|row| self.library || row[1].starts_with(&package))
            .map(|[path, class, method]| [path.clone(), bean_name(class), method.clone()])
            .collect()
    }
}

/// The application's package: its beans are the project's, the others a library's.
fn package(main: &str) -> String {
    match main.rsplit_once('.') {
        Some((package, _)) => format!("{package}."),
        None => String::new(),
    }
}

/// A class's default bean name: its simple name, decapitalized.
fn bean_name(class: &str) -> String {
    let simple = class.rsplit('.').next().unwrap_or(class);
    let mut chars = simple.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The time of an update as IntelliJ's footer writes it.
fn now() -> String {
    match std::env::var("SPORK_E2E_TIME") {
        Ok(time) => time,
        Err(_) => chrono::Local::now().format("%H:%M:%S").to_string(),
    }
}

/// The helper's source in the data folder, written when it changed.
fn helper() -> Result<PathBuf> {
    let path = paths::data_dir()
        .join("java")
        .join("helpers")
        .join("ActuatorEndpoints.java");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(HELPER) {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(&path, HELPER)?;
    }
    Ok(path)
}

/// Adds `spring-boot-starter-actuator` to the project's build file.
fn add_starter(root: &std::path::Path) -> Result<()> {
    let pom = root.join("pom.xml");
    if pom.is_file() {
        let text = std::fs::read_to_string(&pom)?;
        let dependency = "        <dependency>\n            <groupId>org.springframework.boot</groupId>\n            <artifactId>spring-boot-starter-actuator</artifactId>\n        </dependency>\n";
        let at = text
            .find("    </dependencies>")
            .context("the POM has no dependencies")?;
        let mut text = text;
        text.insert_str(at, dependency);
        std::fs::write(&pom, text)?;
        return Ok(());
    }
    let gradle = ["build.gradle", "build.gradle.kts"]
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
        .context("no build file")?;
    let text = std::fs::read_to_string(&gradle)?;
    let at = text
        .find("dependencies {")
        .map(|at| at + "dependencies {".len())
        .context("the build file has no dependencies")?;
    let line = match gradle.extension().and_then(|e| e.to_str()) {
        Some("kts") => {
            "\n    implementation(\"org.springframework.boot:spring-boot-starter-actuator\")"
        }
        _ => "\n    implementation 'org.springframework.boot:spring-boot-starter-actuator'",
    };
    let mut text = text;
    text.insert_str(at, line);
    std::fs::write(&gradle, text)?;
    Ok(())
}

/// The endpoints' answers as the tabs show them.
fn parse(value: &Value) -> Endpoints {
    let missing: Vec<&str> = value["missing"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut data = Endpoints {
        no_actuator: missing.len() == Tab::ALL.len(),
        ..Endpoints::default()
    };
    let text = |value: &Value| match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    for context in value["beans"]["contexts"].as_object().into_iter().flatten() {
        for (name, bean) in context.1["beans"].as_object().into_iter().flatten() {
            data.beans.push(Bean {
                name: name.clone(),
                kind: text(&bean["type"]),
                dependencies: bean["dependencies"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(text)
                    .collect(),
            });
        }
    }
    data.beans.sort_by(|a, b| a.name.cmp(&b.name));
    if let Some(health) = value.get("health").filter(|health| health.is_object()) {
        let components = health["components"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, component)| (name.clone(), text(&component["status"])))
            .collect();
        data.health = Some((text(&health["status"]), components));
    }
    // without a web server the endpoint answers no dispatcher: IntelliJ's "not enabled"
    let web = value["mappings"]["contexts"]
        .as_object()
        .into_iter()
        .flatten()
        .any(|context| {
            let mappings = &context.1["mappings"];
            mappings.get("dispatcherServlets").is_some()
                || mappings.get("dispatcherHandlers").is_some()
        });
    if !missing.contains(&"mappings") && web {
        let mut rows = Vec::new();
        for context in value["mappings"]["contexts"]
            .as_object()
            .into_iter()
            .flatten()
        {
            let servlets = &context.1["mappings"]["dispatcherServlets"];
            for handlers in servlets.as_object().into_iter().flatten() {
                for handler in handlers.1.as_array().into_iter().flatten() {
                    let details = &handler["details"];
                    let conditions = &details["requestMappingConditions"];
                    let join = |key: &str| {
                        conditions[key]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(text)
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    let class = text(&details["handlerMethod"]["className"]);
                    let path = join("patterns");
                    if path.is_empty() || class.is_empty() {
                        continue;
                    }
                    rows.push([path, class, join("methods")]);
                }
            }
        }
        rows.sort();
        data.mappings = Some(rows);
    }
    let env = &value["env"];
    data.profiles = env["activeProfiles"]
        .as_array()
        .into_iter()
        .flatten()
        .map(text)
        .collect();
    for source in env["propertySources"].as_array().into_iter().flatten() {
        let properties = source["properties"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, property)| (name.clone(), text(&property["value"])))
            .collect();
        data.environment.push((text(&source["name"]), properties));
    }
    data
}

impl Render for ActuatorView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        // the latest run's data, read once it started, as IntelliJ waits for the application to
        // be ready
        let (marker, started) = self
            .java
            .upgrade()
            .map(|java| {
                let java = java.read(cx);
                let marker = java
                    .spring
                    .markers
                    .get(self.configuration.as_ref())
                    .cloned();
                let started = java.workspace.upgrade().is_some_and(|workspace| {
                    spork_run::services::console_text(workspace.read(cx), &self.configuration, cx)
                        .is_some_and(|(console, running)| {
                            running && crate::spring_boot::started(&console)
                        })
                });
                (marker, started)
            })
            .unwrap_or_default();
        if !self.loading && started && marker.is_some() && marker != self.fetched {
            self.refresh(cx);
        }
        let tabs = h_flex()
            .id("actuator-tabs")
            .role(Role::TabList)
            .aria_label("Actuator")
            .flex_none()
            .px(px(6.))
            .py(px(2.))
            .gap(px(2.))
            .children(Tab::ALL.map(|tab| {
                let on = self.tab == tab;
                div()
                    .id(tab.title())
                    .role(Role::Tab)
                    .aria_label(tab.title())
                    .aria_selected(on)
                    .h(px(26.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(on, |t| t.bg(tokens.tab_selected))
                    .when(!on, |t| t.hover(move |t| t.bg(tokens.hover)))
                    .child(tab.title())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tab = tab;
                        cx.notify();
                    }))
            }));
        let library = match self.tab {
            Tab::Beans => Some("Show Library Beans"),
            Tab::Mappings => Some("Show Library Mappings"),
            _ => None,
        };
        let toolbar = v_flex()
            .id("actuator-toolbar")
            .role(Role::Toolbar)
            .aria_label("Actuator Toolbar")
            .flex_none()
            .w(px(32.))
            .pt(px(4.))
            .items_center()
            .gap(px(4.))
            .child(
                ActionButton::new("actuator-refresh", "general/refresh")
                    .tooltip("Refresh")
                    .disabled(self.loading)
                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
            )
            .when_some(library, |toolbar, label| {
                toolbar.child(
                    ActionButton::new("actuator-library", "objectBrowser/showLibraryContents")
                        .tooltip(label)
                        .selected(self.library)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.library = !this.library;
                            cx.notify();
                        })),
                )
            });
        let body = self.body(cx);
        let footer = self.updated.as_ref().map(|time| {
            div()
                .id("actuator-updated")
                .role(Role::Label)
                .aria_label(format!("Updated at {time}"))
                .flex_none()
                .px(px(8.))
                .py(px(4.))
                .border_t_1()
                .border_color(tokens.separator)
                .text_color(tokens.text_secondary)
                .child(format!("Updated at {time}"))
        });
        v_flex()
            .id("actuator")
            .size_full()
            .child(tabs)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(toolbar)
                    .child(
                        div()
                            .id("actuator-content")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scroll()
                            .child(body),
                    ),
            )
            .children(footer)
    }
}

impl ActuatorView {
    fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let note = |id: &'static str, text: SharedString| {
            div()
                .id(id)
                .role(Role::Label)
                .aria_label(text.clone())
                .child(text)
        };
        let centered = |children: Vec<AnyElement>| {
            v_flex()
                .size_full()
                .p(px(24.))
                .items_center()
                .justify_center()
                .gap(px(4.))
                .text_color(tokens.text_secondary)
                .children(children)
                .into_any_element()
        };
        if let Some(error) = &self.error {
            return centered(vec![
                note("actuator-error", error.clone().into()).into_any_element(),
            ]);
        }
        let Some(data) = &self.data else {
            return centered(vec![spork_workbench::Spinner::new().into_any_element()]);
        };
        if data.no_actuator {
            let children = match self.added {
                true => vec![
                    note("actuator-added", ACTUATOR_ADDED.into()).into_any_element(),
                    spork_workbench::controls::link("actuator-restart", RESTART, cx)
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(workspace) = this
                                .java
                                .upgrade()
                                .and_then(|java| java.read(cx).workspace.upgrade())
                            {
                                let name = this.configuration.clone();
                                workspace.update(cx, |workspace, cx| {
                                    spork_run::services::launch_configuration(
                                        workspace, &name, false, window, cx,
                                    )
                                });
                            }
                        }))
                        .into_any_element(),
                ],
                false => vec![
                    note("actuator-disabled", NOT_ENABLED.into()).into_any_element(),
                    spork_workbench::controls::link("actuator-add", ADD_ACTUATOR, cx)
                        .on_click(cx.listener(|this, _, _, cx| this.add_actuator(cx)))
                        .into_any_element(),
                ],
            };
            return centered(children);
        }
        let row = |id: SharedString, left: String, right: String| {
            h_flex()
                .id(id)
                .role(Role::Row)
                .aria_label(SharedString::from(left.clone()))
                .px(px(8.))
                .h(px(24.))
                .gap(px(8.))
                .child(div().child(left))
                .child(div().text_color(tokens.text_secondary).child(right))
                .into_any_element()
        };
        match self.tab {
            Tab::Beans => {
                let beans = self.shown_beans(data);
                let selected = self
                    .selected_bean
                    .as_ref()
                    .and_then(|name| beans.iter().find(|bean| &bean.name == name).copied());
                let list = v_flex()
                    .id("actuator-beans")
                    .role(Role::List)
                    .aria_label("Beans")
                    .flex_1()
                    .children(beans.iter().map(|bean| {
                        let name = bean.name.clone();
                        let on = self.selected_bean.as_ref() == Some(&name);
                        h_flex()
                            .id(SharedString::from(format!("bean-{name}")))
                            .role(Role::Row)
                            .aria_label(SharedString::from(name.clone()))
                            .aria_selected(on)
                            .px(px(8.))
                            .h(px(24.))
                            .gap(px(8.))
                            .cursor_pointer()
                            .when(on, |row| row.bg(tokens.selection_inactive))
                            .child(name.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected_bean = Some(name.clone());
                                cx.notify();
                            }))
                    }));
                let details = selected.map(|bean| {
                    let injected: Vec<String> = data
                        .beans
                        .iter()
                        .filter(|other| other.dependencies.contains(&bean.name))
                        .map(|other| other.name.clone())
                        .collect();
                    let section = |title: &'static str, lines: Vec<String>| {
                        v_flex()
                            .gap(px(2.))
                            .child(
                                div()
                                    .id(title)
                                    .role(Role::Label)
                                    .aria_label(title)
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child(title),
                            )
                            .children(lines.into_iter().map(|line| div().pl(px(12.)).child(line)))
                    };
                    v_flex()
                        .id("actuator-bean-details")
                        .flex_1()
                        .p(px(8.))
                        .gap(px(8.))
                        .border_l_1()
                        .border_color(tokens.separator)
                        .child(section("Bean definition", vec![bean.kind.clone()]))
                        .child(section("Injected into", injected))
                        .child(section("Depends on", bean.dependencies.clone()))
                });
                h_flex()
                    .size_full()
                    .items_start()
                    .child(list)
                    .children(details)
                    .into_any_element()
            }
            Tab::Health => match &data.health {
                None => centered(vec![]),
                Some((status, components)) => v_flex()
                    .id("actuator-health")
                    .p(px(8.))
                    .gap(px(4.))
                    .child(note("actuator-status", format!("Status: {status}").into()))
                    .when(components.is_empty(), |health| {
                        health.child(
                            div()
                                .text_color(tokens.text_secondary)
                                .child(note("actuator-indicators", INDICATORS_DISABLED.into())),
                        )
                    })
                    .children(components.iter().map(|(name, status)| {
                        row(
                            format!("health-{name}").into(),
                            name.clone(),
                            status.clone(),
                        )
                    }))
                    .into_any_element(),
            },
            Tab::Mappings => match &data.mappings {
                None => centered(vec![
                    note("actuator-mappings-disabled", MAPPINGS_DISABLED.into()).into_any_element(),
                    div()
                        .id("actuator-add-web")
                        .role(Role::Label)
                        .aria_label(ADD_WEB)
                        .child(ADD_WEB)
                        .into_any_element(),
                ]),
                Some(rows) => {
                    let cells = |id: SharedString, cells: [String; 3], header: bool| {
                        h_flex()
                            .id(id)
                            .role(Role::Row)
                            .aria_label(SharedString::from(cells[0].clone()))
                            .px(px(8.))
                            .h(px(24.))
                            .gap(px(8.))
                            .when(header, |row| {
                                row.text_color(tokens.text_secondary)
                                    .border_b_1()
                                    .border_color(tokens.separator)
                            })
                            .children(cells.map(|cell| div().flex_1().truncate().child(cell)))
                    };
                    v_flex()
                        .id("actuator-mappings")
                        .role(Role::Table)
                        .aria_label("Mappings")
                        .child(cells(
                            "mappings-header".into(),
                            ["Path".into(), "Bean".into(), "Method".into()],
                            true,
                        ))
                        .children(self.shown_mappings(rows).into_iter().map(|cells_| {
                            cells(format!("mapping-{}", cells_[0]).into(), cells_, false)
                        }))
                        .into_any_element()
                }
            },
            Tab::Environment => v_flex()
                .id("actuator-environment")
                .p(px(8.))
                .gap(px(6.))
                .child(note(
                    "actuator-profiles",
                    format!("Active profiles: {}", data.profiles.join(", "))
                        .trim_end()
                        .to_owned()
                        .into(),
                ))
                .children(data.environment.iter().map(|(source, properties)| {
                    v_flex()
                        .gap(px(2.))
                        .child(
                            div()
                                .id(SharedString::from(format!("source-{source}")))
                                .role(Role::Label)
                                .aria_label(SharedString::from(source.clone()))
                                .font_weight(gpui::FontWeight::BOLD)
                                .child(source.clone()),
                        )
                        .children(properties.iter().map(|(name, value)| {
                            div().pl(px(12.)).child(format!("{name} = {value}"))
                        }))
                }))
                .into_any_element(),
        }
    }
}

/// The Actuator tab of the configuration `name`, made the first time it is asked for.
pub(crate) fn view(
    java: &Entity<JavaProject>,
    name: SharedString,
    main: String,
    cx: &mut App,
) -> Entity<ActuatorView> {
    let existing = java.read(cx).spring.actuators.get(name.as_ref()).cloned();
    if let Some(view) = existing {
        view.update(cx, |view, _| view.set_main(main));
        return view;
    }
    let weak = java.downgrade();
    let view = cx.new(|_| ActuatorView::new(weak, name.clone(), main));
    java.update(cx, |java, _| {
        java.spring.actuators.insert(name.to_string(), view.clone())
    });
    view
}

#[cfg(test)]
mod tests {
    use super::{bean_name, parse};
    use serde_json::json;

    #[test]
    fn reads_the_endpoints_answers() {
        let answer = json!({
            "beans": {"contexts": {"demo": {"beans": {
                "helloController": {"type": "com.example.demo.HelloController", "dependencies": []},
                "demoApplication": {"type": "com.example.demo.DemoApplication", "dependencies": ["helloController"]},
                "dataSource": {"type": "com.zaxxer.hikari.HikariDataSource", "dependencies": []}
            }}}},
            "health": {"status": "UP", "components": {"diskSpace": {"status": "UP"}, "ping": {"status": "UP"}}},
            "mappings": {"contexts": {"demo": {"mappings": {"dispatcherServlets": {"dispatcherServlet": [
                {"handler": "x", "details": {"handlerMethod": {"className": "com.example.demo.HelloController", "name": "hello"},
                 "requestMappingConditions": {"methods": ["GET"], "patterns": ["/hello"]}}},
                {"handler": "ResourceHttpRequestHandler [classpath [static/]]", "predicate": "/**"}
            ]}}}}},
            "env": {"activeProfiles": ["dev"], "propertySources": [{"name": "server.ports", "properties": {"local.server.port": {"value": 18081}}}]},
            "missing": []
        });
        let data = parse(&answer);
        assert!(!data.no_actuator);
        assert_eq!(data.beans.len(), 3);
        assert_eq!(data.beans[1].name, "demoApplication");
        assert_eq!(
            data.health,
            Some((
                "UP".into(),
                vec![
                    ("diskSpace".into(), "UP".into()),
                    ("ping".into(), "UP".into())
                ]
            ))
        );
        assert_eq!(
            data.mappings,
            Some(vec![[
                "/hello".into(),
                "com.example.demo.HelloController".into(),
                "GET".into()
            ]])
        );
        assert_eq!(data.profiles, ["dev"]);
        assert_eq!(
            data.environment,
            [(
                "server.ports".into(),
                vec![("local.server.port".into(), "18081".into())]
            )]
        );
        let none = parse(&json!({"missing": ["beans", "health", "mappings", "env"]}));
        assert!(none.no_actuator && none.mappings.is_none());
        let worker =
            parse(&json!({"mappings": {"contexts": {"worker": {"mappings": {}}}}, "missing": []}));
        assert!(!worker.no_actuator && worker.mappings.is_none());
        assert_eq!(
            bean_name("com.example.demo.HelloController"),
            "helloController"
        );
    }
}
