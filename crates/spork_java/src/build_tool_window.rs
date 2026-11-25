//! IntelliJ's Maven and Gradle tool windows on the right stripe (`MavenProjectsNavigator`,
//! `ExternalProjectsView`): the project's tree (Maven's Lifecycle, Plugins and Dependencies,
//! Gradle's Tasks and Dependencies), the toolbar, and a double click running a phase, goal or
//! task with the project's wrapper in the Run tool window.

use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, AnyElement, App, AppContext as _, Context, DismissEvent, Entity, EventEmitter,
    FocusHandle, Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    Render, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    TaskExt as _, WeakEntity, Window, div, px,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{Selectable as _, h_flex, v_flex};
use spork_workbench::tree::{Tree, TreeItem};
use spork_workbench::{ActionButton, Tokens, popup};
use util::ResultExt as _;
use workspace::dock::{DockPosition, GearItem, Panel, PanelEvent, PanelHandle};
use workspace::{ModalView, Workspace};

use crate::gradle_model::DependencyNode;
use crate::java_project::{Download, JavaProject, Tool, Trigger};
use crate::pom::{Plugin, Pom};

pub mod maven_tool_window {
    gpui::actions!(
        maven_tool_window,
        [
            /// Opens the Maven tool window and focuses it, or gives the focus back to the editor.
            ToggleFocus,
            /// Groups the Maven projects by their aggregators.
            GroupModules,
            /// Shows the Maven projects ignored in the settings.
            ShowIgnoredProjects,
            /// Shows Maven's basic lifecycle phases only.
            ShowBasicPhasesOnly,
            /// Names the Maven projects by their artifactId rather than their name.
            AlwaysShowArtifactId,
            /// Shows each Maven project's version after its name.
            ShowVersion,
        ]
    );
}

pub mod gradle_tool_window {
    gpui::actions!(
        gradle_tool_window,
        [
            /// Opens the Gradle tool window and focuses it, or gives the focus back to the
            /// editor.
            ToggleFocus,
            /// Shows the Gradle projects under the builds that include them.
            GroupGradleModules,
            /// Groups the Gradle tasks by their group.
            GroupTasks,
            /// Shows the tasks a Gradle project inherits from its sub-projects.
            ShowInheritedTasks,
            /// Shows the Gradle projects ignored in the settings.
            ShowIgnoredGradleProjects,
        ]
    );
}

/// The view options of a build tool window's ⋮ (IntelliJ's `MavenProjectsNavigatorState` and
/// `ExternalProjectsViewState`), with their defaults.
// shortcut: kept while the window is open, where IntelliJ keeps them in the project's
// workspace file; persist them with the Maven triggers once a user asks
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ViewOptions {
    group_modules: bool,
    show_ignored: bool,
    basic_phases_only: bool,
    always_show_artifact_id: bool,
    show_versions: bool,
    group_tasks: bool,
    show_inherited_tasks: bool,
}

impl ViewOptions {
    fn new(tool: Tool) -> Self {
        Self {
            group_modules: tool == Tool::Gradle,
            show_ignored: true,
            basic_phases_only: true,
            always_show_artifact_id: false,
            show_versions: false,
            group_tasks: true,
            show_inherited_tasks: true,
        }
    }

    /// The ⋮'s items, IntelliJ's additional gear actions in their order.
    fn gear_items(&self, tool: Tool) -> Vec<GearItem> {
        let item = |label: &'static str, checked: bool, action: Box<dyn Action>| GearItem {
            label: label.into(),
            checked: Some(checked),
            action,
        };
        match tool {
            Tool::Maven => vec![
                item(
                    "Group Modules",
                    self.group_modules,
                    Box::new(maven_tool_window::GroupModules),
                ),
                item(
                    "Show Ignored Projects",
                    self.show_ignored,
                    Box::new(maven_tool_window::ShowIgnoredProjects),
                ),
                item(
                    "Show Basic Phases Only",
                    self.basic_phases_only,
                    Box::new(maven_tool_window::ShowBasicPhasesOnly),
                ),
                item(
                    "Always Show ArtifactId",
                    self.always_show_artifact_id,
                    Box::new(maven_tool_window::AlwaysShowArtifactId),
                ),
                item(
                    "Show version",
                    self.show_versions,
                    Box::new(maven_tool_window::ShowVersion),
                ),
            ],
            Tool::Gradle => vec![
                item(
                    "Group Modules",
                    self.group_modules,
                    Box::new(gradle_tool_window::GroupGradleModules),
                ),
                item(
                    "Group Tasks",
                    self.group_tasks,
                    Box::new(gradle_tool_window::GroupTasks),
                ),
                item(
                    "Display Tasks Inherited from Sub-Projects",
                    self.show_inherited_tasks,
                    Box::new(gradle_tool_window::ShowInheritedTasks),
                ),
                item(
                    "Show Ignored",
                    self.show_ignored,
                    Box::new(gradle_tool_window::ShowIgnoredGradleProjects),
                ),
            ],
        }
    }
}

/// Registers a view option's action on the workspace: it flips the option in the tool window.
fn register_option<A: Action, P: BuildToolWindowView + Panel>(
    workspace: &mut Workspace,
    option: fn(&mut ViewOptions) -> &mut bool,
) {
    workspace.register_action(move |workspace, _: &A, _, cx| {
        if let Some(panel) = workspace.panel::<P>(cx) {
            panel.update(cx, |panel, cx| {
                let flag = option(&mut panel.state_mut().options);
                *flag = !*flag;
                cx.notify();
            });
        }
    });
}

/// Adds the tool window of the project's build tool to the workspace.
pub(crate) fn add(
    workspace: &mut Workspace,
    tool: Tool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(java) = JavaProject::of(workspace, cx) else {
        return;
    };
    match tool {
        Tool::Maven => {
            workspace.register_action(
                |workspace, _: &maven_tool_window::ToggleFocus, window, cx| {
                    workspace.toggle_panel_focus::<MavenToolWindow>(window, cx);
                },
            );
            register_option::<maven_tool_window::GroupModules, MavenToolWindow>(workspace, |o| {
                &mut o.group_modules
            });
            register_option::<maven_tool_window::ShowIgnoredProjects, MavenToolWindow>(
                workspace,
                |o| &mut o.show_ignored,
            );
            register_option::<maven_tool_window::ShowBasicPhasesOnly, MavenToolWindow>(
                workspace,
                |o| &mut o.basic_phases_only,
            );
            register_option::<maven_tool_window::AlwaysShowArtifactId, MavenToolWindow>(
                workspace,
                |o| &mut o.always_show_artifact_id,
            );
            register_option::<maven_tool_window::ShowVersion, MavenToolWindow>(workspace, |o| {
                &mut o.show_versions
            });
            let panel = cx.new(|cx| MavenToolWindow(BuildToolWindow::new(java, Tool::Maven, cx)));
            workspace.add_panel(panel, window, cx);
        }
        Tool::Gradle => {
            workspace.register_action(
                |workspace, _: &gradle_tool_window::ToggleFocus, window, cx| {
                    workspace.toggle_panel_focus::<GradleToolWindow>(window, cx);
                },
            );
            register_option::<gradle_tool_window::GroupGradleModules, GradleToolWindow>(
                workspace,
                |o| &mut o.group_modules,
            );
            register_option::<gradle_tool_window::GroupTasks, GradleToolWindow>(workspace, |o| {
                &mut o.group_tasks
            });
            register_option::<gradle_tool_window::ShowInheritedTasks, GradleToolWindow>(
                workspace,
                |o| &mut o.show_inherited_tasks,
            );
            register_option::<gradle_tool_window::ShowIgnoredGradleProjects, GradleToolWindow>(
                workspace,
                |o| &mut o.show_ignored,
            );
            let panel = cx.new(|cx| GradleToolWindow(BuildToolWindow::new(java, Tool::Gradle, cx)));
            workspace.add_panel(panel, window, cx);
        }
    }
}

/// A row that runs something: a Maven phase or goal, a Gradle task.
#[derive(Clone, Debug, PartialEq)]
struct Runnable {
    /// The row's key in the tree, which shows it selected.
    key: String,
    goals: Vec<String>,
}

/// What the Maven and Gradle tool windows share: the project, the selected row, Expand and
/// Collapse All.
pub(crate) struct BuildToolWindow {
    java: Entity<JavaProject>,
    tool: Tool,
    selected: Option<Runnable>,
    /// Expand All and Collapse All draw the tree anew, every node opened or closed.
    generation: usize,
    all_open: Option<bool>,
    options: ViewOptions,
    focus: FocusHandle,
    _subscription: Subscription,
}

/// Maven's default lifecycle phases, the basic ones IntelliJ shows (`MavenConstants.BASIC_PHASES`,
/// "Show Basic Phases Only" being on by default).
const BASIC_PHASES: [&str; 9] = [
    "clean", "validate", "compile", "test", "package", "verify", "install", "site", "deploy",
];

/// Every phase the tool window lists with Show Basic Phases Only off (`MavenConstants.PHASES`).
const PHASES: [&str; 24] = [
    "clean",
    "validate",
    "generate-sources",
    "process-sources",
    "generate-resources",
    "process-resources",
    "compile",
    "process-classes",
    "generate-test-sources",
    "process-test-sources",
    "generate-test-resources",
    "process-test-resources",
    "test-compile",
    "process-test-classes",
    "test",
    "prepare-package",
    "package",
    "pre-integration-test",
    "integration-test",
    "post-integration-test",
    "verify",
    "install",
    "site",
    "deploy",
];

/// The plugins Maven's super POM and the `jar` packaging bind (maven-core 3.9.6's
/// `default-bindings.xml`, the Maven m2e runs in JDT LS 1.43), with their goals.
// shortcut: Maven's bindings of one version, and the goals of each plugin's descriptor written
// out; read them from the project's effective POM once JDT LS exposes it
const DEFAULT_PLUGINS: [(&str, &str, &[&str]); 8] = [
    ("maven-clean-plugin", "3.2.0", &["clean", "help"]),
    (
        "maven-compiler-plugin",
        "3.11.0",
        &["compile", "help", "testCompile"],
    ),
    (
        "maven-deploy-plugin",
        "3.1.1",
        &["deploy", "deploy-file", "help"],
    ),
    (
        "maven-install-plugin",
        "3.1.1",
        &["help", "install", "install-file"],
    ),
    ("maven-jar-plugin", "3.3.0", &["help", "jar", "test-jar"]),
    (
        "maven-resources-plugin",
        "3.3.1",
        &["copy-resources", "help", "resources", "testResources"],
    ),
    (
        "maven-site-plugin",
        "3.12.1",
        &[
            "attach-descriptor",
            "deploy",
            "effective-site",
            "help",
            "jar",
            "run",
            "site",
            "stage",
            "stage-deploy",
        ],
    ),
    ("maven-surefire-plugin", "3.2.2", &["help", "test"]),
];

/// A plugin's goal prefix: `maven-compiler-plugin` → `compiler`, `exec-maven-plugin` → `exec`.
fn goal_prefix(artifact_id: &str) -> &str {
    artifact_id
        .strip_prefix("maven-")
        .and_then(|rest| rest.strip_suffix("-plugin"))
        .or_else(|| artifact_id.strip_suffix("-maven-plugin"))
        .unwrap_or(artifact_id)
}

/// The plugins of the project as IntelliJ lists them: the ones Maven binds by default, the POM's
/// own over them, sorted by their prefix.
fn plugins(pom: &Pom) -> Vec<Plugin> {
    let mut plugins: Vec<Plugin> = if pom.packaging == "pom" {
        Vec::new()
    } else {
        DEFAULT_PLUGINS
            .iter()
            .map(|(artifact_id, version, goals)| Plugin {
                group_id: "org.apache.maven.plugins".into(),
                artifact_id: (*artifact_id).into(),
                version: Some((*version).into()),
                goals: goals.iter().map(|goal| (*goal).to_owned()).collect(),
            })
            .collect()
    };
    for declared in &pom.plugins {
        match plugins
            .iter_mut()
            .find(|p| p.artifact_id == declared.artifact_id && p.group_id == declared.group_id)
        {
            Some(plugin) => {
                if declared.version.is_some() {
                    plugin.version = declared.version.clone();
                }
                for goal in &declared.goals {
                    if !plugin.goals.contains(goal) {
                        plugin.goals.push(goal.clone());
                    }
                }
            }
            None => plugins.push(declared.clone()),
        }
    }
    plugins.sort_by_key(|plugin| goal_prefix(&plugin.artifact_id).to_lowercase());
    plugins
}

impl BuildToolWindow {
    fn new<V: 'static>(java: Entity<JavaProject>, tool: Tool, cx: &mut Context<V>) -> Self {
        let _subscription = cx.observe(&java, |_, _, cx| cx.notify());
        Self {
            java,
            tool,
            selected: None,
            generation: 0,
            all_open: None,
            options: ViewOptions::new(tool),
            focus: cx.focus_handle(),
            _subscription,
        }
    }

    fn select(&mut self, runnable: Runnable) {
        self.selected = Some(runnable);
    }

    fn run_selected(&self, window: &mut Window, cx: &mut App) {
        if let Some(runnable) = &self.selected {
            let goals = runnable.goals.clone();
            JavaProject::run_tool(&self.java, goals, window, cx);
        }
    }

    fn set_all_open(&mut self, open: bool) {
        self.generation += 1;
        self.all_open = Some(open);
    }

    /// A row that runs: selected on a click, run on a double click; its triggers after it
    /// (`(Before Build)`), and its menu (`Maven.BuildMenu`).
    fn runnable_item(
        &self,
        this: WeakEntity<impl BuildToolWindowView>,
        label: String,
        icon: &'static str,
        runnable: Runnable,
        cx: &App,
    ) -> TreeItem {
        let selected = self
            .selected
            .as_ref()
            .is_some_and(|s| s.key == runnable.key);
        let goal = runnable.goals.join(" ");
        let java = self.java.read(cx);
        let tool = java_tool(&runnable);
        let triggers: &'static [Trigger] = match tool {
            Tool::Maven => &Trigger::ALL,
            Tool::Gradle => &Trigger::GRADLE,
        };
        let on: Vec<Trigger> = triggers
            .iter()
            .copied()
            .filter(|trigger| java.goals_on(*trigger).contains(&goal))
            .collect();
        // IntelliJ's `Run 'fixture [run]'`: the configuration a Gradle task's run makes
        let run_label = match tool {
            Tool::Maven => "Run Maven Build".to_owned(),
            Tool::Gradle => format!("Run '{} [{goal}]'", java.name()),
        };
        let hint = (!on.is_empty()).then(|| {
            let labels: Vec<&str> = on.iter().map(|trigger| trigger.label()).collect();
            format!("({})", labels.join(", "))
        });
        let menu = {
            let (this, java, runnable) = (this.clone(), self.java.clone(), runnable.clone());
            move |menu: PopupMenu, _: &mut Window, _: &mut gpui::Context<PopupMenu>| {
                let run = {
                    let (this, runnable) = (this.clone(), runnable.clone());
                    PopupMenuItem::new(run_label.clone()).on_click(move |_, window, cx| {
                        let runnable = runnable.clone();
                        this.update(cx, |this, cx| {
                            let state = this.state_mut();
                            state.select(runnable);
                            state.run_selected(window, cx);
                            cx.notify();
                        })
                        .log_err();
                    })
                };
                let mut menu = menu.item(run).separator();
                for trigger in triggers.iter().copied() {
                    let (java, goal) = (java.clone(), runnable.goals.join(" "));
                    menu = menu.item(
                        PopupMenuItem::new(format!("Execute {}", trigger.label()))
                            .checked(on.contains(&trigger))
                            .on_click(move |_, _, cx| {
                                java.update(cx, |java, cx| java.toggle_trigger(trigger, &goal, cx))
                            }),
                    );
                }
                // shortcut: Assign Shortcut… is greyed, Spork IDE has no action per task for
                // the keymap to bind; add one when the Keymap page can bind a task
                match tool {
                    Tool::Maven => menu,
                    Tool::Gradle => menu
                        .separator()
                        .item(PopupMenuItem::new("Assign Shortcut…").disabled(true)),
                }
            }
        };
        TreeItem::new(label)
            .colored_icon(icon)
            .active(selected)
            .when_some(hint, |item, hint| item.hint(hint))
            .context_menu(menu)
            .on_click(move |event, window, cx| {
                let runnable = runnable.clone();
                this.update(cx, |this, cx| {
                    let window_state = this.state_mut();
                    window_state.select(runnable);
                    if event.click_count() == 2 {
                        window_state.run_selected(window, cx);
                    }
                    cx.notify();
                })
                .ok();
            })
    }

    fn open(&self, default: bool) -> bool {
        self.all_open.unwrap_or(default)
    }

    fn maven_tree(&self, this: WeakEntity<impl BuildToolWindowView>, cx: &App) -> Vec<TreeItem> {
        let java = self.java.read(cx);
        let Some(pom) = &java.pom else {
            return Vec::new();
        };
        let phases: &[&str] = match self.options.basic_phases_only {
            true => &BASIC_PHASES,
            false => &PHASES,
        };
        let lifecycle = phases.iter().map(|phase| {
            self.runnable_item(
                this.clone(),
                (*phase).to_owned(),
                "build/task",
                Runnable {
                    key: format!("phase:{phase}"),
                    goals: vec![(*phase).to_owned()],
                },
                cx,
            )
        });
        let plugins = plugins(pom).into_iter().map(|plugin| {
            let prefix = goal_prefix(&plugin.artifact_id).to_owned();
            let coordinates = match &plugin.version {
                Some(version) => format!("({}:{}:{version})", plugin.group_id, plugin.artifact_id),
                None => format!("({}:{})", plugin.group_id, plugin.artifact_id),
            };
            let goals: Vec<TreeItem> = plugin
                .goals
                .iter()
                .map(|goal| {
                    let goal = format!("{prefix}:{goal}");
                    self.runnable_item(
                        this.clone(),
                        goal.clone(),
                        "build/mavenPluginGoal",
                        Runnable {
                            key: format!("goal:{goal}"),
                            goals: vec![goal],
                        },
                        cx,
                    )
                })
                .collect();
            TreeItem::new(prefix)
                .colored_icon("build/mavenPlugin")
                .hint(coordinates)
                .default_open(self.open(false))
                .children(goals)
        });
        let dependencies: Vec<TreeItem> = pom
            .dependencies
            .iter()
            .map(|dependency| {
                let only = (dependency.group_id.clone(), dependency.artifact_id.clone());
                let downloads =
                    download_items(self.java.clone(), Some(only), java.downloading.is_none());
                TreeItem::new(dependency.coordinates())
                    .colored_icon("nodes/library")
                    .when(dependency.scope != "compile", |item| {
                        item.hint(format!("({})", dependency.scope))
                    })
                    .context_menu(move |menu, _, _| {
                        downloads.iter().fold(menu, |menu, item| menu.item(item()))
                    })
            })
            .collect();
        let mut children = vec![
            TreeItem::new("Lifecycle")
                .colored_icon("build/taskGroup")
                .default_open(self.open(false))
                .children(lifecycle),
            TreeItem::new("Plugins")
                .colored_icon("build/taskGroup")
                .default_open(self.open(false))
                .children(plugins),
        ];
        if !dependencies.is_empty() {
            children.push(
                TreeItem::new("Dependencies")
                    .colored_icon("nodes/libraryFolder")
                    .default_open(self.open(false))
                    .children(dependencies),
            );
        }
        let name = match self.options.always_show_artifact_id {
            true => pom.artifact_id.clone(),
            false => pom.name.clone().unwrap_or_else(|| pom.artifact_id.clone()),
        };
        let mut items = Vec::new();
        if !pom.profiles.is_empty() {
            let profiles = pom.profiles.iter().map(|profile| {
                let toggle = {
                    let (java, profile) = (self.java.clone(), profile.clone());
                    move |cx: &mut App| {
                        java.update(cx, |java, cx| java.toggle_profile(&profile, cx))
                    }
                };
                let click = toggle.clone();
                // IntelliJ's check box before the profile's name
                TreeItem::new(profile.clone())
                    .colored_icon(match java.profile_on(profile) {
                        true => "diff/gutterCheckBoxSelected@14x14",
                        false => "diff/gutterCheckBox@14x14",
                    })
                    .on_click(move |_, _, cx| click(cx))
                    .context_menu(move |menu, _, _| {
                        let toggle = toggle.clone();
                        menu.item(
                            PopupMenuItem::new("Toggle Profile")
                                .on_click(move |_, _, cx| toggle(cx)),
                        )
                    })
            });
            let java = self.java.clone();
            items.push(
                TreeItem::new("Profiles")
                    .colored_icon("build/mavenProfiles")
                    .default_open(self.open(false))
                    .context_menu(move |menu, _, _| {
                        let java = java.clone();
                        menu.item(
                            PopupMenuItem::new("Reset Profiles").on_click(move |_, _, cx| {
                                java.update(cx, |java, cx| {
                                    java.profiles.clear();
                                    cx.notify();
                                })
                            }),
                        )
                    })
                    .children(profiles),
            );
        }
        let project_menu = {
            let (java, this) = (self.java.clone(), this.clone());
            let downloads =
                download_items(self.java.clone(), None, java.read(cx).downloading.is_none());
            let has_selected = self.selected.is_some();
            move |menu: PopupMenu, _: &mut Window, _: &mut gpui::Context<PopupMenu>| {
                let reload = |label: &'static str| {
                    let java = java.clone();
                    PopupMenuItem::new(label)
                        .on_click(move |_, _, cx| java.update(cx, |java, cx| java.reload(cx)))
                };
                let settings = crate::java_project::maven_settings();
                let settings_item = {
                    let (java, exists) = (java.clone(), settings.is_file());
                    PopupMenuItem::new(match exists {
                        true => "Open settings.xml",
                        false => "Create 'settings.xml'",
                    })
                    .on_click(move |_, window, cx| open_maven_settings(&java, window, cx))
                };
                let run = {
                    let this = this.clone();
                    PopupMenuItem::new("Run Maven Build")
                        .disabled(!has_selected)
                        .on_click(move |_, window, cx| {
                            this.update(cx, |this, cx| this.state_mut().run_selected(window, cx))
                                .log_err();
                        })
                };
                let menu = menu
                    .item(reload("Sync Project"))
                    .item(reload("Generate Sources and Update Folders"))
                    .separator()
                    .item(settings_item)
                    .separator();
                downloads
                    .iter()
                    .fold(menu, |menu, item| menu.item(item()))
                    .separator()
                    .item(run)
            }
        };
        let version = pom.version.clone().filter(|_| self.options.show_versions);
        items.push(
            TreeItem::new(name)
                .colored_icon("build/mavenProject")
                .when_some(version, |item, version| item.hint(format!(":{version}")))
                .default_open(self.open(true))
                .context_menu(project_menu)
                .children(children),
        );
        items
    }

    fn gradle_tree(&self, this: WeakEntity<impl BuildToolWindowView>, cx: &App) -> Vec<TreeItem> {
        let java = self.java.read(cx);
        let Some(model) = &java.gradle else {
            return Vec::new();
        };
        let task_item = |task: &crate::gradle_model::Task| {
            self.runnable_item(
                this.clone(),
                task.name.clone(),
                "build/task",
                Runnable {
                    key: format!("task:{}", task.name),
                    goals: vec![task.name.clone()],
                },
                cx,
            )
        };
        let tasks: Vec<TreeItem> = match self.options.group_tasks {
            true => {
                let mut groups = model.tasks.clone();
                // IntelliJ sorts the groups by name, "other" last
                groups.sort_by_key(|group| (group.name == "other", group.name.clone()));
                groups
                    .into_iter()
                    .map(|group| {
                        TreeItem::new(group.name.clone())
                            .colored_icon("build/taskGroup")
                            .default_open(self.open(false))
                            .children(group.tasks.iter().map(task_item))
                    })
                    .collect()
            }
            // Group Tasks off: every task under Tasks, by name
            false => {
                let mut all: Vec<&crate::gradle_model::Task> = model
                    .tasks
                    .iter()
                    .flat_map(|group| group.tasks.iter())
                    .collect();
                all.sort_by(|a, b| a.name.cmp(&b.name));
                all.into_iter().map(task_item).collect()
            }
        };
        // `Gradle.View.DependencyMenu`: the Dependency Analyzer alone
        // shortcut: greyed, Spork IDE has no Dependency Analyzer (Analyze Dependencies… too)
        fn dependency(node: &DependencyNode, open: bool) -> TreeItem {
            TreeItem::new(node.coordinates.clone())
                .colored_icon("nodes/library")
                .default_open(open)
                .context_menu(|menu, _, _| {
                    menu.item(PopupMenuItem::new("Analyze Dependencies…").disabled(true))
                })
                .children(node.children.iter().map(|child| dependency(child, open)))
        }
        let configurations = model.configurations.iter().map(|configuration| {
            TreeItem::new(configuration.name.clone())
                .colored_icon("nodes/libraryFolder")
                .default_open(self.open(false))
                .children(
                    configuration
                        .dependencies
                        .iter()
                        .map(|node| dependency(node, self.open(false))),
                )
        });
        let project_menu = {
            let java = self.java.clone();
            move |menu: PopupMenu, _: &mut Window, _: &mut gpui::Context<PopupMenu>| {
                let (open, sync, refresh) = (java.clone(), java.clone(), java.clone());
                // `ExternalSystemView.ProjectMenu` and `Gradle.View.ProjectMenu`
                // shortcut: Unlink, Ignore, Tasks Activation and Composite Build Configuration
                // are greyed: the project folder is the one Gradle project; add them with
                // multi-project builds
                menu.item(
                    PopupMenuItem::new("Open Gradle Config")
                        .on_click(move |_, window, cx| open_gradle_config(&open, window, cx)),
                )
                .separator()
                .item(
                    PopupMenuItem::new("Sync Gradle Project")
                        .on_click(move |_, _, cx| sync.update(cx, |java, cx| java.reload(cx))),
                )
                .separator()
                .item(PopupMenuItem::new("Unlink Gradle Project").disabled(true))
                .item(PopupMenuItem::new("Ignore Gradle Project").disabled(true))
                .separator()
                .item(PopupMenuItem::new("Tasks Activation").disabled(true))
                .separator()
                .item(PopupMenuItem::new("Composite Build Configuration").disabled(true))
                .item(PopupMenuItem::new("Refresh Gradle Dependencies").on_click(
                    move |_, _, cx| refresh.update(cx, |java, cx| java.refresh_dependencies(cx)),
                ))
                .item(PopupMenuItem::new("Analyze Dependencies…").disabled(true))
            }
        };
        vec![
            TreeItem::new(java.name())
                .colored_icon("toolwindows/gradle")
                .default_open(self.open(true))
                .context_menu(project_menu)
                .children([
                    TreeItem::new("Tasks")
                        .colored_icon("build/taskGroup")
                        .default_open(self.open(false))
                        .children(tasks),
                    TreeItem::new("Dependencies")
                        .colored_icon("nodes/libraryFolder")
                        .default_open(self.open(false))
                        .children(configurations),
                ]),
        ]
    }

    /// The toolbar under the header, IntelliJ's `Maven.NavigatorActionsToolbar` and
    /// `ExternalSystemView.ActionsToolbar`.
    // shortcut: Download Sources, Add/Link and Unlink, Analyze Dependencies… and the settings
    // gear are greyed: JDT LS fetches sources itself, a project folder is the one project, and
    // Spork IDE has no Dependency Analyzer or Build Tools settings page yet
    fn toolbar(&self, this: WeakEntity<impl BuildToolWindowView>, cx: &App) -> AnyElement {
        let java = self.java.read(cx);
        let name = self.tool.name();
        let wrapper = java.wrapper().is_some();
        let button = |id: &'static str, icon: &'static str, tooltip: String| {
            ActionButton::new(id, icon).tooltip(tooltip)
        };
        let separator = |cx: &App| {
            div()
                .w(px(1.))
                .h(px(16.))
                .mx(px(3.))
                .bg(Tokens::get(cx).separator)
        };
        let update = |f: fn(&mut BuildToolWindow, &mut Window, &mut App)| {
            let this = this.clone();
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                this.update(cx, |this, cx| {
                    f(this.state_mut(), window, cx);
                    cx.notify();
                })
                .ok();
            }
        };
        let execute_tooltip = match wrapper {
            true => format!(
                "Execute {name} {}",
                if self.tool == Tool::Maven {
                    "Goal"
                } else {
                    "Task"
                }
            ),
            false => format!(
                "Execute {name} {}: the project has no {name} wrapper ({})",
                if self.tool == Tool::Maven {
                    "Goal"
                } else {
                    "Task"
                },
                self.tool.wrapper()
            ),
        };
        let mut row = h_flex()
            .id("build-tool-toolbar")
            .role(Role::Toolbar)
            .aria_label(format!("{name} Toolbar"))
            .flex_none()
            // shortcut: a narrow tool window wraps the toolbar where IntelliJ hides what doesn't
            // fit behind a » button; add that overflow to the workbench when a toolbar needs it
            .flex_wrap()
            .min_h(px(32.))
            .py(px(4.))
            .px(px(6.))
            .child(match self.tool {
                // `Maven.SyncAllSplitAction`: Sync All and Reload All in its popup
                Tool::Maven => {
                    let java = self.java.clone();
                    button(
                        "sync-all",
                        "general/refresh",
                        "Sync/Reload All Maven Projects".into(),
                    )
                    .popup_marker()
                    .dropdown_menu(move |menu, _, _| {
                        let item = |label: &'static str| {
                            let java = java.clone();
                            PopupMenuItem::new(label).on_click(move |_, _, cx| {
                                java.update(cx, |java, cx| java.reload(cx))
                            })
                        };
                        menu.item(item("Sync All Maven Projects"))
                            .item(item("Reload All Maven Projects"))
                    })
                    .into_any_element()
                }
                Tool::Gradle => button(
                    "sync-all",
                    "general/refresh",
                    format!("Sync All {name} Projects"),
                )
                .on_click(update(|this, _, cx| {
                    this.java.update(cx, |java, cx| java.reload(cx))
                }))
                .into_any_element(),
            });
        row = match self.tool {
            Tool::Maven => row
                .child(
                    button(
                        "update-folders",
                        "build/updateFolders",
                        "Generate Sources and Update Folders For All Projects".into(),
                    )
                    .on_click(update(|this, _, cx| {
                        this.java.update(cx, |java, cx| java.reload(cx))
                    })),
                )
                .child({
                    let downloads = download_items(self.java.clone(), None, true);
                    button(
                        "download",
                        "general/download",
                        "Download Sources and/or Documentation".into(),
                    )
                    .popup_marker()
                    .disabled(!wrapper || java.downloading.is_some())
                    .dropdown_menu(move |menu, _, _| {
                        downloads.iter().fold(menu, |menu, item| menu.item(item()))
                    })
                }),
            Tool::Gradle => row
                .child(
                    button(
                        "select-data",
                        "nodes/moduleGroup",
                        "Select Project Data To Import".into(),
                    )
                    .disabled(true),
                )
                .child(
                    button("download", "general/download", "Download Sources".into())
                        .disabled(true),
                ),
        };
        let (add, remove) = match self.tool {
            Tool::Maven => ("Add Maven Projects", "Unlink Maven Projects"),
            Tool::Gradle => ("Link Gradle Project", "Unlink Gradle Project"),
        };
        row = row
            .child(separator(cx))
            .child(button("add", "general/add", add.into()).disabled(true))
            .child(button("unlink", "general/remove", remove.into()).disabled(true))
            .child(separator(cx));
        if self.tool == Tool::Maven {
            row = row.child(
                button("run-build", "run/run", "Run Maven Build".into())
                    .disabled(!wrapper || self.selected.is_none())
                    .on_click(update(|this, window, cx| this.run_selected(window, cx))),
            );
        }
        row = row
            .child(
                button("execute", "general/runAnything", execute_tooltip)
                    .disabled(!wrapper)
                    .on_click({
                        let java = self.java.clone();
                        move |_, window, cx| execute(java.clone(), window, cx)
                    }),
            )
            .child(
                button(
                    "offline",
                    "build/toggleOfflineMode",
                    "Toggle Offline Mode".into(),
                )
                .selected(java.offline)
                .on_click(update(|this, _, cx| {
                    this.java.update(cx, |java, cx| {
                        java.offline = !java.offline;
                        cx.notify();
                    })
                })),
            );
        if self.tool == Tool::Maven {
            row = row.child(
                button(
                    "skip-tests",
                    "run/showIgnored",
                    "Toggle 'Skip Tests' Mode".into(),
                )
                .selected(java.skip_tests)
                .on_click(update(|this, _, cx| {
                    this.java.update(cx, |java, cx| {
                        java.skip_tests = !java.skip_tests;
                        cx.notify();
                    })
                })),
            );
        }
        row.child(separator(cx))
            .child(
                button(
                    "analyze",
                    "build/dependencyAnalyzer",
                    "Analyze Dependencies…".into(),
                )
                .disabled(true),
            )
            .child(separator(cx))
            .child(
                button("expand-all", "general/expandAll", "Expand All".into())
                    .on_click(update(|this, _, _| this.set_all_open(true))),
            )
            .child(
                button("collapse-all", "general/collapseAll", "Collapse All".into())
                    .on_click(update(|this, _, _| this.set_all_open(false))),
            )
            .child(separator(cx))
            .child(
                button(
                    "settings",
                    "general/settings",
                    match self.tool {
                        Tool::Maven => "Build Tools Settings".into(),
                        Tool::Gradle => "Build Tool Settings".into(),
                    },
                )
                .disabled(true),
            )
            .into_any_element()
    }

    fn render_view(
        &mut self,
        this: WeakEntity<impl BuildToolWindowView>,
        panel: Arc<dyn PanelHandle>,
        cx: &mut App,
    ) -> AnyElement {
        let tokens = Tokens::get(cx);
        let name = self.tool.name();
        let header = workspace::frame::tool_window_header(
            workspace::frame::tool_window_title_label(name),
            Vec::<AnyElement>::new(),
            panel,
            name.into(),
            cx,
        );
        let items = match self.tool {
            Tool::Maven => self.maven_tree(this.clone(), cx),
            Tool::Gradle => self.gradle_tree(this.clone(), cx),
        };
        let java = self.java.read(cx);
        // what shows in place of the tree: IntelliJ's empty text, or why the model is missing
        let empty: Option<SharedString> = match self.tool {
            _ if !items.is_empty() => None,
            Tool::Maven => Some(
                java.pom_error
                    .clone()
                    .unwrap_or_else(|| "There are no Maven projects to display.".into())
                    .into(),
            ),
            Tool::Gradle if java.gradle_loading => {
                Some("Initializing Gradle projects data…".into())
            }
            Tool::Gradle => Some(
                java.gradle_error
                    .clone()
                    .unwrap_or_else(|| "Initializing Gradle projects data…".into())
                    .into(),
            ),
        };
        let toolbar = self.toolbar(this, cx);
        spork_vcs::spork_look(cx)
            .size_full()
            .child(
                v_flex()
                    .size_full()
                    .bg(tokens.island)
                    .track_focus(&self.focus)
                    .key_context(format!("{name}ToolWindow").as_str())
                    .child(header)
                    .child(toolbar)
                    .child(match empty {
                        Some(text) => div()
                            .id("build-tool-empty")
                            .flex_1()
                            .p(px(16.))
                            .text_color(tokens.text_secondary)
                            .child(text)
                            .into_any_element(),
                        None => div()
                            .id("build-tool-tree")
                            .role(Role::Tree)
                            .aria_label(format!("{name} Projects"))
                            .flex_1()
                            .min_h_0()
                            .py(px(4.))
                            .overflow_y_scroll()
                            .child(
                                Tree::new(format!("{name}-tree-{}", self.generation))
                                    .children(items),
                            )
                            .into_any_element(),
                    }),
            )
            .into_any_element()
    }
}

/// The tool a runnable row belongs to, by its key (`phase:`/`goal:` are Maven's).
fn java_tool(runnable: &Runnable) -> Tool {
    match runnable.key.starts_with("task:") {
        true => Tool::Gradle,
        false => Tool::Maven,
    }
}

type MenuItemFn = Arc<dyn Fn() -> PopupMenuItem>;

/// Download Sources, Download Documentation, Download Sources and Documentation: of every
/// dependency (`Maven.DownloadAllGroup`) or of one (`Maven.DependencyMenu`).
fn download_items(
    java: Entity<JavaProject>,
    only: Option<(String, String)>,
    enabled: bool,
) -> Vec<MenuItemFn> {
    [
        ("Download Sources", Download::Sources),
        ("Download Documentation", Download::Documentation),
        ("Download Sources and Documentation", Download::Both),
    ]
    .into_iter()
    .map(|(label, what)| {
        let (java, only) = (java.clone(), only.clone());
        Arc::new(move || {
            let (java, only) = (java.clone(), only.clone());
            PopupMenuItem::new(label)
                .disabled(!enabled)
                .on_click(move |_, _, cx| {
                    let only = only.clone();
                    java.update(cx, |java, cx| java.download(what, only, cx))
                })
        }) as MenuItemFn
    })
    .collect()
}

/// Open settings.xml, or Create 'settings.xml' from IntelliJ's template when the user has none.
fn open_maven_settings(java: &Entity<JavaProject>, window: &mut Window, cx: &mut App) {
    let Some(workspace) = java.read(cx).workspace.upgrade() else {
        return;
    };
    let path = crate::java_project::maven_settings();
    if !path.is_file() {
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, crate::java_project::MAVEN_SETTINGS_TEMPLATE));
        if written.log_err().is_none() {
            return;
        }
    }
    workspace.update(cx, |workspace, cx| {
        workspace
            .open_abs_path(path, workspace::OpenOptions::default(), window, cx)
            .detach_and_log_err(cx);
    });
}

/// Open Gradle Config: the project's build script in an editor.
fn open_gradle_config(java: &Entity<JavaProject>, window: &mut Window, cx: &mut App) {
    let java = java.read(cx);
    let (Some(workspace), Some(root)) = (java.workspace.upgrade(), java.root.clone()) else {
        return;
    };
    let Some(script) = [
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "settings.gradle.kts",
    ]
    .iter()
    .map(|name| root.join(name))
    .find(|path| path.is_file()) else {
        return;
    };
    workspace.update(cx, |workspace, cx| {
        workspace
            .open_abs_path(script, workspace::OpenOptions::default(), window, cx)
            .detach_and_log_err(cx);
    });
}

/// Execute Maven Goal / Execute Gradle Task: IntelliJ's Run Anything popup, the build tool's
/// command typed in, run with the project's wrapper.
fn execute(java: Entity<JavaProject>, window: &mut Window, cx: &mut App) {
    let Some(workspace) = java.read(cx).workspace.upgrade() else {
        return;
    };
    workspace.update(cx, |workspace, cx| {
        workspace.toggle_modal(window, cx, move |window, cx| {
            RunAnything::new(java, window, cx)
        });
    });
}

struct RunAnything {
    java: Entity<JavaProject>,
    field: Entity<InputState>,
    _subscription: Subscription,
}

impl EventEmitter<DismissEvent> for RunAnything {}

impl ModalView for RunAnything {}

impl Focusable for RunAnything {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl RunAnything {
    fn new(java: Entity<JavaProject>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prefix = match java.read(cx).tool {
            Some(Tool::Gradle) => "gradle ",
            _ => "mvn ",
        };
        let field = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(prefix, window, cx);
            state
        });
        let _subscription = cx.subscribe_in(&field, window, |this, _, event, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.confirm(window, cx);
            }
        });
        Self {
            java,
            field,
            _subscription,
        }
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let line = self.field.read(cx).value().to_string();
        let mut words = spork_run::configurations::split_arguments(&line);
        // Run Anything's command names the tool, which the wrapper stands in for
        if words
            .first()
            .is_some_and(|word| ["mvn", "gradle", "./mvnw", "./gradlew"].contains(&word.as_str()))
        {
            words.remove(0);
        }
        if !words.is_empty() {
            JavaProject::run_tool(&self.java, words, window, cx);
        }
        cx.emit(DismissEvent);
    }
}

impl Render for RunAnything {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        spork_vcs::spork_look(cx).child(
            popup::surface(cx)
                .id("run-anything")
                .role(Role::Dialog)
                .aria_label("Run Anything")
                .w(px(560.))
                .key_context("RunAnything")
                .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
                .child(popup::title("Run Anything", cx))
                .child(v_flex().px(px(12.)).pb(px(12.)).child(
                    spork_workbench::TextField::new(&self.field).aria_label("Run Anything"),
                )),
        )
    }
}

/// The Maven or Gradle tool window's view, for the rows' handlers.
pub(crate) trait BuildToolWindowView: 'static + Sized {
    fn state_mut(&mut self) -> &mut BuildToolWindow;
}

macro_rules! tool_window {
    ($name:ident, $persistent:literal, $key:literal, $toggle:path, $priority:literal, $icon:ident) => {
        pub(crate) struct $name(BuildToolWindow);

        impl BuildToolWindowView for $name {
            fn state_mut(&mut self) -> &mut BuildToolWindow {
                &mut self.0
            }
        }

        impl $name {
            pub(crate) fn state(&self) -> &BuildToolWindow {
                &self.0
            }
        }

        impl Render for $name {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let this = cx.entity().downgrade();
                let panel: Arc<dyn PanelHandle> = Arc::new(cx.entity());
                self.0.render_view(this, panel, cx)
            }
        }

        impl Focusable for $name {
            fn focus_handle(&self, _: &App) -> FocusHandle {
                self.0.focus.clone()
            }
        }

        impl EventEmitter<PanelEvent> for $name {}

        impl Panel for $name {
            fn persistent_name() -> &'static str {
                $persistent
            }

            fn panel_key() -> &'static str {
                $key
            }

            fn position(&self, _: &Window, _: &App) -> DockPosition {
                DockPosition::Right
            }

            // shortcut: the tool window stays where IntelliJ puts it, as Spork's others do
            fn position_is_valid(&self, position: DockPosition) -> bool {
                position == DockPosition::Right
            }

            fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}

            fn default_size(&self, _: &Window, _: &App) -> Pixels {
                px(360.)
            }

            fn icon(&self, _: &Window, _: &App) -> Option<ui::IconName> {
                Some(ui::IconName::$icon)
            }

            fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
                Some($persistent)
            }

            fn toggle_action(&self) -> Box<dyn Action> {
                Box::new($toggle)
            }

            fn activation_priority(&self) -> u32 {
                $priority
            }

            fn gear_items(&self, _: &Window, _: &App) -> Vec<GearItem> {
                self.0.options.gear_items(self.0.tool)
            }
        }
    };
}

tool_window!(
    MavenToolWindow,
    "Maven",
    "MavenToolWindow",
    maven_tool_window::ToggleFocus,
    13,
    ListTree
);
tool_window!(
    GradleToolWindow,
    "Gradle",
    "GradleToolWindow",
    gradle_tool_window::ToggleFocus,
    14,
    ListTree
);

/// The key of the build tool window's selected row (`phase:compile`, `task:build`).
pub(crate) fn e2e_selected(workspace: &Workspace, tool: Tool, cx: &App) -> Option<String> {
    let selected = match tool {
        Tool::Maven => workspace
            .panel::<MavenToolWindow>(cx)?
            .read(cx)
            .state()
            .selected
            .clone(),
        Tool::Gradle => workspace
            .panel::<GradleToolWindow>(cx)?
            .read(cx)
            .state()
            .selected
            .clone(),
    };
    selected.map(|runnable| runnable.key)
}

#[cfg(test)]
mod tests {
    use super::{goal_prefix, plugins};
    use crate::pom::Pom;

    #[test]
    fn lists_the_default_plugins_by_their_prefix_with_the_poms_own() {
        assert_eq!(goal_prefix("maven-compiler-plugin"), "compiler");
        assert_eq!(goal_prefix("exec-maven-plugin"), "exec");
        let pom = Pom::parse(
            "<project><artifactId>a</artifactId><build><plugins>\
             <plugin><groupId>org.codehaus.mojo</groupId><artifactId>exec-maven-plugin</artifactId>\
             <version>3.5.0</version></plugin>\
             <plugin><artifactId>maven-compiler-plugin</artifactId><version>3.13.0</version></plugin>\
             </plugins></build></project>",
        )
        .expect("a POM");
        let listed: Vec<_> = plugins(&pom)
            .iter()
            .map(|p| {
                format!(
                    "{}:{}",
                    goal_prefix(&p.artifact_id),
                    p.version.clone().unwrap_or_default()
                )
            })
            .collect();
        assert_eq!(
            listed,
            [
                "clean:3.2.0",
                "compiler:3.13.0",
                "deploy:3.1.1",
                "exec:3.5.0",
                "install:3.1.1",
                "jar:3.3.0",
                "resources:3.3.1",
                "site:3.12.1",
                "surefire:3.2.2"
            ]
        );
    }
}
