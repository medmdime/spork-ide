//! New Project ▸ Spring Boot: IntelliJ's Spring Initializr wizard (the platform's web starter
//! wizard, `WebStarterInitialStep` then `WebStarterLibrariesStep`) on start.spring.io's metadata
//! API: the project's name, place, language, build, coordinates, JDK, Java version, packaging and
//! configuration format, then the Spring Boot version and the dependencies. Create downloads the
//! project (`starter.zip`), unpacks it and opens it, which links and loads it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use futures::AsyncReadExt as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, Window, div, px,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use http_client::{AsyncBody, HttpClient, Url};
use serde::Deserialize;
use spork_workbench::controls;
use spork_workbench::tokens::{Tokens, Typography as _};
use util::ResultExt as _;

use crate::jdk::Jdk;

/// IntelliJ's Spring Initializr server, and the variable that replaces it (the end-to-end tests'
/// recorded answers).
const SERVER: &str = "https://start.spring.io";
pub const SERVER_VARIABLE: &str = "SPORK_IDE_INITIALIZR";
/// The metadata IntelliJ asks for.
const ACCEPT: &str =
    "application/vnd.initializr.v2.3+json,application/vnd.initializr.v2.1+json;q=0.9";

/// Adds Spring Boot to New Project's generators.
pub(crate) fn init(cx: &mut App) {
    spork_projects::register_generator(
        spork_projects::Generator {
            name: "Spring Boot".into(),
            icon: "runConfigurations/springBoot",
            page: Rc::new(|window, cx| cx.new(|cx| Wizard::new(window, cx)).into()),
        },
        cx,
    );
}

/// start.spring.io's metadata (`application/vnd.initializr.v2.3+json`), what the wizard shows of it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    dependencies: Options<Group>,
    r#type: Options<Choice>,
    packaging: Options<Choice>,
    java_version: Options<Choice>,
    language: Options<Choice>,
    boot_version: Options<Choice>,
    #[serde(default)]
    configuration_file_format: Option<Options<Choice>>,
}

#[derive(Deserialize)]
struct Options<T> {
    #[serde(default)]
    default: Option<String>,
    values: Vec<T>,
}

#[derive(Clone, Deserialize)]
struct Choice {
    id: String,
    name: String,
    /// A project type's download: `/starter.zip` for a project, a build file's otherwise.
    #[serde(default)]
    action: Option<String>,
}

#[derive(Deserialize)]
struct Group {
    name: String,
    values: Vec<Dependency>,
}

#[derive(Clone, Deserialize)]
struct Dependency {
    id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
}

impl<T> Options<T> {
    fn default_or(&self, id: impl Fn(&T) -> &str) -> String {
        self.default
            .clone()
            .or_else(|| self.values.first().map(|value| id(value).to_owned()))
            .unwrap_or_default()
    }
}

enum Server {
    /// "Connecting to web service and retrieving options…"
    Loading,
    Failed(String),
    Loaded(Rc<Metadata>),
}

/// The wizard's two pages and what they chose.
pub(crate) struct Wizard {
    server: String,
    metadata: Server,
    _load: Task<()>,
    second_page: bool,
    name: Entity<InputState>,
    location: Entity<InputState>,
    group: Entity<InputState>,
    artifact: Entity<InputState>,
    package: Entity<InputState>,
    search: Entity<InputState>,
    /// The artifact follows the name, the package the group and artifact, until edited.
    artifact_edited: bool,
    package_edited: bool,
    git: bool,
    language: String,
    project_type: String,
    java: String,
    packaging: String,
    format: String,
    boot: String,
    jdks: Vec<Jdk>,
    jdk: Option<usize>,
    /// The dependencies added, in the order added.
    added: Vec<String>,
    /// The dependency whose description shows.
    described: Option<String>,
    collapsed: BTreeSet<String>,
    problem: Option<String>,
    creating: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Wizard {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = |text: &str, window: &mut Window, cx: &mut Context<Self>| {
            let text = text.to_owned();
            cx.new(|cx| InputState::new(window, cx).default_value(text))
        };
        let name = input("demo", window, cx);
        let location = input(
            &spork_projects::projects_folder().display().to_string(),
            window,
            cx,
        );
        let group = input("com.example", window, cx);
        let artifact = input("demo", window, cx);
        let package = input("com.example.demo", window, cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let changed = |field: &Entity<InputState>,
                       on_change: fn(&mut Self, &mut Window, &mut Context<Self>),
                       window: &mut Window,
                       cx: &mut Context<Self>| {
            cx.subscribe_in(
                field,
                window,
                move |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        on_change(this, window, cx);
                        cx.notify();
                    }
                },
            )
        };
        let subscriptions = vec![
            changed(
                &name,
                |this, window, cx| {
                    if !this.artifact_edited {
                        let name = this.name.read(cx).value();
                        this.artifact
                            .update(cx, |artifact, cx| artifact.set_value(name, window, cx));
                        this.follow_package(window, cx);
                    }
                },
                window,
                cx,
            ),
            changed(&location, |_, _, _| {}, window, cx),
            changed(
                &group,
                |this, window, cx| this.follow_package(window, cx),
                window,
                cx,
            ),
            changed(
                &artifact,
                |this, window, cx| {
                    this.artifact_edited = true;
                    this.follow_package(window, cx);
                },
                window,
                cx,
            ),
            changed(
                &package,
                |this, _, _| this.package_edited = true,
                window,
                cx,
            ),
            changed(&search, |_, _, _| {}, window, cx),
        ];
        let jdks = crate::jdk::find(&crate::jdk::process_env());
        let jdk = jdks
            .iter()
            .enumerate()
            .max_by_key(|(_, jdk)| jdk.major)
            .map(|(ix, _)| ix);
        let mut this = Self {
            server: std::env::var(SERVER_VARIABLE).unwrap_or_else(|_| SERVER.to_owned()),
            metadata: Server::Loading,
            _load: Task::ready(()),
            second_page: false,
            name,
            location,
            group,
            artifact,
            package,
            search,
            artifact_edited: false,
            package_edited: false,
            git: false,
            language: String::new(),
            project_type: String::new(),
            java: String::new(),
            packaging: String::new(),
            format: String::new(),
            boot: String::new(),
            jdks,
            jdk,
            added: Vec::new(),
            described: None,
            collapsed: BTreeSet::new(),
            problem: None,
            creating: None,
            _subscriptions: subscriptions,
        };
        this.load(cx);
        this
    }

    /// The package name IntelliJ derives from the group and the artifact, until it is edited.
    fn follow_package(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.package_edited {
            return;
        }
        let artifact: String = self
            .artifact
            .read(cx)
            .value()
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
            .collect();
        let package = format!("{}.{artifact}", self.group.read(cx).value().trim());
        self.package
            .update(cx, |field, cx| field.set_value(package, window, cx));
    }

    /// Reads the server's options.
    fn load(&mut self, cx: &mut Context<Self>) {
        let (http, server) = (cx.http_client(), self.server.clone());
        self._load = cx.spawn(async move |this, cx| {
            let metadata = fetch_metadata(&http, &server).await;
            this.update(cx, |this, cx| {
                this.metadata = match metadata {
                    Ok(metadata) => {
                        this.loaded(&metadata);
                        Server::Loaded(Rc::new(metadata))
                    }
                    // `message.no.connection.with.error.content`
                    Err(error) => Server::Failed(format!(
                        "Initialization failed for '{}'\nPlease check URL, network and proxy settings.\n\nError message:\n{error:#}",
                        this.server
                    )),
                };
                cx.notify();
            })
            .log_err();
        });
        self.metadata = Server::Loading;
        cx.notify();
    }

    /// The server's defaults.
    fn loaded(&mut self, metadata: &Metadata) {
        self.language = metadata.language.default_or(|choice| &choice.id);
        self.project_type = metadata.r#type.default_or(|choice| &choice.id);
        self.java = metadata.java_version.default_or(|choice| &choice.id);
        self.packaging = metadata.packaging.default_or(|choice| &choice.id);
        self.boot = metadata.boot_version.default_or(|choice| &choice.id);
        self.format = metadata
            .configuration_file_format
            .as_ref()
            .map(|format| format.default_or(|choice| &choice.id))
            .unwrap_or_default();
    }

    fn folder(&self, cx: &App) -> PathBuf {
        expand_home(&self.location.read(cx).value()).join(self.name.read(cx).value().trim())
    }

    /// Why Next can't go on: IntelliJ's checks of the first page.
    fn first_page_problem(&self, cx: &App) -> Option<String> {
        if self.name.read(cx).value().trim().is_empty() {
            return Some("Field must be set".into());
        }
        let folder = self.folder(cx);
        if !spork_projects::usable(&folder) {
            return Some(format!("Directory '{}' is not empty", folder.display()));
        }
        for field in [&self.group, &self.artifact] {
            if field.read(cx).value().trim().is_empty() {
                return Some("Field must be set".into());
            }
        }
        let package = self.package.read(cx).value();
        if !is_package_name(&package) {
            return Some(format!("'{package}' is not a valid package name"));
        }
        let jdk = self.jdk.and_then(|ix| self.jdks.get(ix))?;
        let java: u32 = self.java.parse().ok()?;
        (java > jdk.major).then(|| {
            format!(
                "Selected version of Java {java} is not supported by the project SDK '{}'. Either choose a lower version of Java or set a higher version of the SDK.\n\nTo download additional SDKs, click the JDK drop-down.",
                jdk.name()
            )
        })
    }

    fn next(&mut self, cx: &mut Context<Self>) {
        self.problem = self.first_page_problem(cx);
        if self.problem.is_none() {
            self.second_page = true;
        }
        cx.notify();
    }

    /// The download of the project, as IntelliJ asks start.spring.io for it.
    fn starter_url(&self, cx: &App) -> Result<Url> {
        let mut url = Url::parse(&format!(
            "{}/starter.zip",
            self.server.trim_end_matches('/')
        ))?;
        let text = |field: &Entity<InputState>| field.read(cx).value().trim().to_owned();
        let name = text(&self.name);
        url.query_pairs_mut()
            .append_pair("type", &self.project_type)
            .append_pair("language", &self.language)
            .append_pair("bootVersion", &self.boot)
            .append_pair("groupId", &text(&self.group))
            .append_pair("artifactId", &text(&self.artifact))
            .append_pair("name", &name)
            .append_pair("packageName", &text(&self.package))
            .append_pair("packaging", &self.packaging)
            .append_pair("javaVersion", &self.java);
        if !self.format.is_empty() {
            url.query_pairs_mut()
                .append_pair("configurationFileFormat", &self.format);
        }
        if !self.added.is_empty() {
            url.query_pairs_mut()
                .append_pair("dependencies", &self.added.join(","));
        }
        Ok(url)
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = match self.starter_url(cx) {
            Ok(url) => url,
            Err(error) => {
                self.problem = Some(format!("{error:#}"));
                cx.notify();
                return;
            }
        };
        let folder = self.folder(cx);
        let git = self.git;
        let jdk = self.jdk.and_then(|ix| self.jdks.get(ix)).cloned();
        let http = cx.http_client();
        self.problem = None;
        self.creating = Some(cx.spawn_in(window, async move |this, cx| {
            let downloaded = download(&http, &url, &folder).await;
            this.update_in(cx, |this, window, cx| match downloaded {
                Ok(()) => {
                    if let Some(jdk) = jdk {
                        crate::jdk::choose(&folder, &jdk).log_err();
                    }
                    window.close_dialog(cx);
                    spork_projects::create_project(folder.clone(), git, window, cx);
                }
                Err(error) => {
                    this.problem = Some(format!("{error:#}"));
                    this.creating = None;
                    cx.notify();
                }
            })
            .log_err();
        }));
        cx.notify();
    }

    fn page_one(&self, metadata: &Metadata, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let tokens = Tokens::get(cx);
        let folder = self.folder(cx);
        let segmented = |id: &'static str,
                         label: &'static str,
                         choices: Vec<Choice>,
                         chosen: &str,
                         set: fn(&mut Self, String),
                         cx: &mut Context<Self>| {
            let names: Vec<&str> = choices.iter().map(|choice| choice.name.as_str()).collect();
            let selected = choices
                .iter()
                .position(|choice| choice.id == chosen)
                .unwrap_or(0);
            let this = cx.entity().downgrade();
            let ids: Vec<String> = choices.iter().map(|choice| choice.id.clone()).collect();
            row(
                label,
                h_flex().child(controls::segmented_button(
                    id,
                    label,
                    &names,
                    selected,
                    move |ix, _, cx| {
                        if let Some(id) = ids.get(ix).cloned() {
                            this.update(cx, |this, cx| {
                                set(this, id);
                                cx.notify();
                            })
                            .log_err();
                        }
                    },
                    cx,
                )),
            )
        };
        let mut rows = vec![
            row(
                "Name:",
                spork_workbench::TextField::new(&self.name).aria_label("Name:"),
            ),
            row(
                "Location:",
                spork_workbench::TextField::new(&self.location).aria_label("Location:"),
            ),
            div()
                .pl(px(LABEL + GAP))
                .text_ui_small()
                .text_color(tokens.text_secondary)
                .child(format!(
                    "Project will be created in: {}",
                    util::paths::PathExt::compact(&folder).display()
                ))
                .into_any_element(),
            div()
                .pl(px(LABEL + GAP))
                .child(
                    controls::labeled_check(
                        "initializr-git",
                        "Create Git repository",
                        self.git,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.git = !this.git;
                        cx.notify();
                    })),
                )
                .into_any_element(),
            segmented(
                "initializr-language",
                "Language:",
                metadata.language.values.clone(),
                &self.language,
                |this, id| this.language = id,
                cx,
            ),
            segmented(
                "initializr-type",
                "Type:",
                metadata
                    .r#type
                    .values
                    .iter()
                    .filter(|choice| choice.action.as_deref() == Some("/starter.zip"))
                    .cloned()
                    .collect(),
                &self.project_type,
                |this, id| this.project_type = id,
                cx,
            ),
            row(
                "Group:",
                spork_workbench::TextField::new(&self.group).aria_label("Group:"),
            ),
            row(
                "Artifact:",
                spork_workbench::TextField::new(&self.artifact).aria_label("Artifact:"),
            ),
            row(
                "Package name:",
                spork_workbench::TextField::new(&self.package).aria_label("Package name:"),
            ),
            row("JDK:", self.jdk_combo(cx)),
            row(
                "Java:",
                choice_combo(
                    "initializr-java",
                    "Java:",
                    &metadata.java_version.values,
                    &self.java,
                    |this, id| this.java = id,
                    cx,
                ),
            ),
            segmented(
                "initializr-packaging",
                "Packaging:",
                metadata.packaging.values.clone(),
                &self.packaging,
                |this, id| this.packaging = id,
                cx,
            ),
        ];
        if let Some(formats) = &metadata.configuration_file_format {
            rows.push(segmented(
                "initializr-configuration",
                "Configuration:",
                formats.values.clone(),
                &self.format,
                |this, id| this.format = id,
                cx,
            ));
        }
        rows
    }

    fn jdk_combo(&self, cx: &mut Context<Self>) -> AnyElement {
        let chosen = self.jdk.and_then(|ix| self.jdks.get(ix));
        let (label, detail) = chosen.map_or(("<No SDK>".to_owned(), String::new()), |jdk| {
            (jdk.name(), jdk.version_string())
        });
        let names: Vec<String> = self
            .jdks
            .iter()
            .map(|jdk| format!("{} {}", jdk.name(), jdk.version_string()))
            .collect();
        let this = cx.entity().downgrade();
        controls::dropdown(
            controls::ComboBox::new("initializr-jdk", label)
                .aria_label("JDK:")
                .detail(detail)
                .w_full(),
            false,
            move |mut menu, _, _| {
                for (ix, name) in names.iter().enumerate() {
                    let this = this.clone();
                    menu = menu.item(PopupMenuItem::new(name.clone()).on_click(move |_, _, cx| {
                        this.update(cx, |this, cx| {
                            this.jdk = Some(ix);
                            cx.notify();
                        })
                        .log_err();
                    }));
                }
                menu
            },
        )
        .into_any_element()
    }

    fn page_two(&self, metadata: &Metadata, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let tokens = Tokens::get(cx);
        let typed = self.search.read(cx).value().to_lowercase();
        let mut tree = Vec::new();
        for group in &metadata.dependencies.values {
            let shown: Vec<&Dependency> = group
                .values
                .iter()
                .filter(|dependency| {
                    typed.is_empty()
                        || dependency.name.to_lowercase().contains(&typed)
                        || dependency.id.contains(&typed)
                        || dependency
                            .description
                            .as_deref()
                            .is_some_and(|text| text.to_lowercase().contains(&typed))
                })
                .collect();
            if shown.is_empty() {
                continue;
            }
            // the search shows every group it finds dependencies in open
            let open = !typed.is_empty() || !self.collapsed.contains(&group.name);
            let name = group.name.clone();
            tree.push(
                controls::settings_tree_row(
                    SharedString::from(format!("initializr-group-{}", group.name)),
                    group.name.clone(),
                    1,
                    Some(open),
                    false,
                    true,
                    false,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.collapsed.remove(&name) {
                        this.collapsed.insert(name.clone());
                    }
                    cx.notify();
                }))
                .into_any_element(),
            );
            if !open {
                continue;
            }
            for dependency in shown {
                let (id, added) = (dependency.id.clone(), self.added.contains(&dependency.id));
                tree.push(
                    div()
                        .pl(px(45.))
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .child(
                            controls::labeled_check(
                                SharedString::from(format!("initializr-dependency-{id}")),
                                dependency.name.clone(),
                                added,
                                cx,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    match this.added.iter().position(|added| *added == id) {
                                        Some(ix) => {
                                            this.added.remove(ix);
                                        }
                                        None => this.added.push(id.clone()),
                                    }
                                    this.described = Some(id.clone());
                                    cx.notify();
                                },
                            )),
                        )
                        .into_any_element(),
                );
            }
        }
        let dependency = |id: &str| {
            metadata.dependencies.values.iter().find_map(|group| {
                group
                    .values
                    .iter()
                    .find(|dependency| dependency.id == id)
                    .map(|dependency| (group, dependency))
            })
        };
        let description = self
            .described
            .as_deref()
            .and_then(dependency)
            .map(|(_, dependency)| {
                v_flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .child(dependency.name.clone()),
                    )
                    .children(dependency.description.clone())
            });
        let mut added = v_flex().gap(px(2.));
        let mut last_group = None;
        for id in &self.added {
            let Some((group, dependency)) = dependency(id) else {
                continue;
            };
            if last_group != Some(&group.name) {
                added = added.child(
                    controls::form_label(group.name.clone())
                        .pt(px(4.))
                        .text_color(tokens.text_secondary),
                );
                last_group = Some(&group.name);
            }
            let id = id.clone();
            added = added.child(
                h_flex()
                    .gap(px(4.))
                    .child(controls::form_label(dependency.name.clone()))
                    .child(
                        spork_workbench::ActionButton::new(
                            SharedString::from(format!("initializr-remove-{id}")),
                            "general/closeSmall",
                        )
                        .tooltip("Remove")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.added.retain(|added| *added != id);
                            cx.notify();
                        })),
                    ),
            );
        }
        vec![
            row(
                "Spring Boot:",
                choice_combo(
                    "initializr-boot",
                    "Spring Boot:",
                    &metadata.boot_version.values,
                    &self.boot,
                    |this, id| this.boot = id,
                    cx,
                ),
            ),
            h_flex()
                .items_start()
                .gap(px(16.))
                .h(px(300.))
                .child(
                    v_flex()
                        .w(px(300.))
                        .h_full()
                        .gap(px(6.))
                        .child(controls::form_label("Dependencies:"))
                        .child(spork_workbench::TextField::new(&self.search).aria_label("Search"))
                        .child(
                            v_flex()
                                .id("initializr-dependencies")
                                .role(gpui::Role::Tree)
                                .aria_label("Dependencies")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .border_1()
                                .border_color(tokens.separator)
                                .children(tree),
                        ),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .gap(px(12.))
                        .child(div().min_h(px(96.)).children(description))
                        .child(controls::form_label("Added dependencies:"))
                        .child(match self.added.is_empty() {
                            true => controls::form_label("No dependencies added")
                                .text_color(tokens.text_secondary)
                                .into_any_element(),
                            false => added.into_any_element(),
                        }),
                )
                .into_any_element(),
        ]
    }
}

/// The label column of the wizard's rows and the gap after it.
const LABEL: f32 = 110.;
const GAP: f32 = 8.;

fn row(label: &'static str, field: impl IntoElement) -> AnyElement {
    h_flex()
        .gap(px(GAP))
        .child(controls::form_label(label).w(px(LABEL)).flex_none())
        .child(div().flex_1().min_w_0().child(field))
        .into_any_element()
}

/// A combo box of the server's choices (Java, Spring Boot).
fn choice_combo(
    id: &'static str,
    label: &'static str,
    choices: &[Choice],
    chosen: &str,
    set: fn(&mut Wizard, String),
    cx: &mut Context<Wizard>,
) -> AnyElement {
    let shown = choices
        .iter()
        .find(|choice| choice.id == chosen)
        .map_or_else(|| chosen.to_owned(), |choice| choice.name.clone());
    let choices = choices.to_vec();
    let this = cx.entity().downgrade();
    controls::dropdown(
        controls::ComboBox::new(id, shown)
            .aria_label(label)
            .w_full(),
        false,
        move |mut menu, _, _| {
            for choice in &choices {
                let (this, id) = (this.clone(), choice.id.clone());
                menu = menu.item(PopupMenuItem::new(choice.name.clone()).on_click(
                    move |_, _, cx| {
                        let id = id.clone();
                        this.update(cx, |this, cx| {
                            set(this, id);
                            cx.notify();
                        })
                        .log_err();
                    },
                ));
            }
            menu
        },
    )
    .into_any_element()
}

impl Render for Wizard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let server = Url::parse(&self.server)
            .ok()
            .and_then(|url| {
                Some(match url.port() {
                    Some(port) => format!("{}:{port}", url.host_str()?),
                    None => url.host_str()?.to_owned(),
                })
            })
            .unwrap_or_else(|| self.server.clone());
        let mut body = v_flex().gap(px(10.));
        if !self.second_page {
            body = body.child(row(
                "Server URL:",
                h_flex()
                    .gap(px(6.))
                    .child(controls::link("initializr-server", server, cx))
                    .when(matches!(self.metadata, Server::Failed(_)), |row| {
                        row.child(
                            spork_workbench::ActionButton::new(
                                "initializr-retry",
                                "general/refresh",
                            )
                            .tooltip("Retry")
                            .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                        )
                    }),
            ));
        }
        body = match &self.metadata {
            Server::Loading => body.child(
                div()
                    .text_color(tokens.text_secondary)
                    .child("Connecting to web service and retrieving options…"),
            ),
            Server::Failed(error) => body.child(
                div()
                    .text_color(tokens.text_error)
                    .whitespace_normal()
                    .child(error.clone()),
            ),
            Server::Loaded(metadata) => {
                let metadata = metadata.clone();
                body.children(match self.second_page {
                    false => self.page_one(&metadata, cx),
                    true => self.page_two(&metadata, cx),
                })
            }
        };
        let loaded = matches!(self.metadata, Server::Loaded(_));
        let creating = self.creating.is_some();
        let buttons = h_flex()
            .pt(px(8.))
            .gap(px(8.))
            .justify_end()
            .child(
                spork_workbench::Button::new("initializr-cancel", "Cancel")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .when(self.second_page, |buttons| {
                buttons
                    .child(
                        spork_workbench::Button::new("initializr-previous", "Previous")
                            .disabled(creating)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.second_page = false;
                                this.problem = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        spork_workbench::Button::new("initializr-create", "Create")
                            .primary(true)
                            .disabled(creating)
                            .on_click(cx.listener(|this, _, window, cx| this.create(window, cx))),
                    )
            })
            .when(!self.second_page, |buttons| {
                buttons.child(
                    spork_workbench::Button::new("initializr-next", "Next")
                        .primary(true)
                        .disabled(!loaded)
                        .on_click(cx.listener(|this, _, _, cx| this.next(cx))),
                )
            });
        v_flex()
            .h(px(480.))
            .child(
                div()
                    .id("initializr-page")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
            .children(self.problem.clone().map(|problem| {
                div()
                    .pt(px(6.))
                    .text_ui_small()
                    .text_color(tokens.text_error)
                    .whitespace_normal()
                    .child(problem)
            }))
            .child(buttons)
    }
}

/// IntelliJ's paths start with `~` for the home folder: the fields take them that way too.
fn expand_home(text: &str) -> PathBuf {
    match text.trim().strip_prefix("~/") {
        Some(rest) => util::paths::home_dir().join(rest),
        None => PathBuf::from(text.trim()),
    }
}

/// Whether `text` is a Java package name: identifiers between dots.
fn is_package_name(text: &str) -> bool {
    !text.is_empty()
        && text.split('.').all(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .is_some_and(|first| first.is_alphabetic() || first == '_')
                && chars.all(|c| c.is_alphanumeric() || c == '_')
        })
}

async fn fetch_metadata(http: &Arc<dyn HttpClient>, server: &str) -> Result<Metadata> {
    let request = http_client::Request::builder()
        .uri(server)
        .header("Accept", ACCEPT)
        .body(AsyncBody::default())?;
    let mut response = http.send(request).await?;
    anyhow::ensure!(response.status().is_success(), "{}", response.status());
    let mut text = String::new();
    response.body_mut().read_to_string(&mut text).await?;
    serde_json::from_str(&text).context("reading the server's options")
}

/// Downloads the project and unpacks it into `folder`.
async fn download(http: &Arc<dyn HttpClient>, url: &Url, folder: &Path) -> Result<()> {
    let mut response = http.get(url.as_str(), AsyncBody::default(), true).await?;
    anyhow::ensure!(
        response.status().is_success(),
        "Error while generating sources for module: {}",
        response.status()
    );
    std::fs::create_dir_all(folder)?;
    util::archive::extract_zip(folder, response.body_mut()).await
}

#[cfg(test)]
mod tests {
    use super::{Metadata, is_package_name};

    #[test]
    fn package_names_are_identifiers_between_dots() {
        assert!(is_package_name("com.example.demo"));
        assert!(is_package_name("_a.b1"));
        assert!(!is_package_name("com..example"));
        assert!(!is_package_name("com.1example"));
        assert!(!is_package_name("com.my-app"));
    }

    #[test]
    fn reads_the_servers_metadata() {
        let text = include_str!("../../../tools/e2e/fixtures/initializr/metadata.json");
        let metadata: Metadata = serde_json::from_str(text).expect("the metadata");
        assert_eq!(metadata.boot_version.default.as_deref(), Some("4.2.0"));
        assert!(
            metadata
                .dependencies
                .values
                .iter()
                .any(|group| group.name == "Web"
                    && group.values.iter().any(|dependency| dependency.id == "web"))
        );
    }
}
