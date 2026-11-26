//! The Services tool window (⌘8, `ServiceView`): the run configurations of the types added to it
//! (Add Service ▸ Run Configuration…), grouped by type or, from View Options, by status (Running,
//! Failed, Finished, Not Started), beside the selected one's console: its run's (the Run tool
//! window's terminal) or its debug session's. Its toolbar is IntelliJ's `ServiceViewTreeToolbar`
//! (Add Service, View Options | Expand All, Collapse All); a configuration's own toolbar (Run,
//! Debug, Stop | Scroll to End, Clear All) is on its console's left, as the run content's, and in
//! its context menu. Also Show Running List (IntelliJ's "Running List" balloon at the frame's top
//! right) and Stop Background Processes… (the language servers' cancellable work). Docker's
//! connections are listed after the configurations (`docker.rs`).

use std::collections::BTreeSet;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, Anchor, AnyElement, AnyView, App, AppContext as _, Context, DismissEvent, Entity,
    EventEmitter, FocusHandle, Focusable, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, Render, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    TaskExt as _, WeakEntity, Window, anchored, div, point, px,
};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use project::Project;
use project::lsp_store::ProgressToken;
use serde_json::{Value, json};
use spork_workbench::tree::{Tree, TreeItem};
use spork_workbench::{ActionButton, Tokens, popup};
use util::ResultExt as _;
use workspace::dock::{DockPosition, Panel, PanelEvent};
use workspace::{ModalView, Workspace};

use crate::docker::{Docker, DockerEvent, NewConnection};
use crate::list_popup::{ListPopup, Row};
use crate::{Configuration, ProjectConfigurations, run, run_panel};

pub mod services_panel {
    gpui::actions!(
        services_panel,
        [
            /// Opens the Services tool window and focuses it, or gives the focus back to the
            /// editor.
            ToggleFocus,
            /// Shows the services under their type's node, or without it (IntelliJ's Group
            /// Services by Type).
            GroupServicesByType
        ]
    );
}

/// A service of another tool listed after the run configurations and Docker (IntelliJ's
/// `ServiceViewContributor`s): the HTTP Client's requests, with the view its node shows.
pub struct ContributedService {
    /// Its key in Services, unique among the contributor's.
    pub id: SharedString,
    pub label: SharedString,
    pub icon: &'static str,
    pub view: AnyView,
}

/// A contributor's node ("HTTP Request") and its services.
pub struct ContributedGroup {
    pub title: SharedString,
    pub icon: &'static str,
    pub services: Vec<ContributedService>,
}

type Contributor = std::rc::Rc<dyn Fn(&Workspace, &App) -> Option<ContributedGroup>>;

#[derive(Default)]
struct Contributors(Vec<Contributor>);

impl gpui::Global for Contributors {}

/// Lists a tool's services in Services: `contributor` gives its node for a workspace, `None`
/// while it has nothing to show.
pub fn register_contributor(
    contributor: impl Fn(&Workspace, &App) -> Option<ContributedGroup> + 'static,
    cx: &mut App,
) {
    cx.default_global::<Contributors>()
        .0
        .push(std::rc::Rc::new(contributor));
}

fn contributed(workspace: &Workspace, cx: &App) -> Vec<ContributedGroup> {
    cx.try_global::<Contributors>()
        .map(|contributors| {
            contributors
                .0
                .iter()
                .filter_map(|contributor| contributor(workspace, cx))
                .collect()
        })
        .unwrap_or_default()
}

/// Shows the contributed service `id` selected in Services, as IntelliJ shows a run's node; or
/// redraws Services when `id` is `None` (a contributor's services changed).
pub fn show_contributed(
    workspace: &mut Workspace,
    id: Option<SharedString>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(panel) = workspace.panel::<ServicesPanel>(cx) else {
        return;
    };
    let show = id.is_some();
    panel.update(cx, |panel, cx| {
        if let Some(id) = id {
            panel.selected = None;
            panel
                .docker
                .update(cx, |docker, cx| docker.clear_selection(cx));
            panel.contributed = Some(id);
        }
        cx.notify();
    });
    if show {
        workspace.open_panel::<ServicesPanel>(window, cx);
    }
}

pub(crate) fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &services_panel::ToggleFocus, window, cx| {
        // a tool window shown over Services in its dock may leave it the focus: ⌘8 shows it then
        let shown = workspace.panel::<ServicesPanel>(cx).is_some_and(|panel| {
            workspace
                .bottom_dock()
                .read(cx)
                .visible_panel()
                .is_some_and(|visible| visible.panel_id() == panel.entity_id())
        });
        match shown {
            true => {
                workspace.toggle_panel_focus::<ServicesPanel>(window, cx);
            }
            false => {
                workspace.focus_panel::<ServicesPanel>(window, cx);
            }
        }
    });
    // greyed while nothing runs, or no background work can stop, as IntelliJ's
    workspace.register_action_renderer(|div, workspace, window, cx| {
        let running = !running_list(workspace, cx).is_empty();
        let cancellable = !cancellable_work(workspace.project(), cx).is_empty();
        let updaters = match running {
            true => updaters(workspace, cx),
            false => Vec::new(),
        };
        // the menu bar is the front window's, or the last drawn one's while the app is behind
        let in_front = cx
            .active_window()
            .is_none_or(|active| active == window.window_handle());
        if in_front {
            let text = match updaters.as_slice() {
                [updater] => Some(updater.text.clone()),
                _ => None,
            };
            let mut menu = crate::run_menu(cx);
            if menu.update != text {
                menu.update = text;
                cx.set_global(menu);
            }
        }
        div.when(!updaters.is_empty(), |div| {
            div.on_action(cx.listener(
                |workspace, _: &run::UpdateRunningApplication, window, cx| {
                    update_running_application(workspace, window, cx)
                },
            ))
        })
        .when(running, |div| {
            div.on_action(
                cx.listener(|workspace, _: &run::ShowRunningList, window, cx| {
                    show_running_list(workspace, window, cx)
                }),
            )
        })
        .when(cancellable, |div| {
            div.on_action(
                cx.listener(|workspace, _: &run::StopBackgroundProcesses, window, cx| {
                    stop_background_processes(workspace, window, cx)
                }),
            )
        })
    });
}

/// Adds the Services tool window to a workspace whose panels are loaded.
pub(crate) fn add(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    if workspace.panel::<ServicesPanel>(cx).is_some() {
        return;
    }
    let Some(configurations) = ProjectConfigurations::of(workspace, cx) else {
        return;
    };
    let handle = workspace.weak_handle();
    let project = workspace.project().clone();
    let panel = cx.new(|cx| ServicesPanel::new(handle, project, configurations, cx));
    workspace.add_panel(panel, window, cx);
}

/// What a configuration is doing, by IntelliJ's status groups.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Status {
    Running,
    Failed,
    Finished,
    NotStarted,
}

impl Status {
    fn title(self) -> &'static str {
        match self {
            Status::Running => "Running",
            Status::Failed => "Failed",
            Status::Finished => "Finished",
            Status::NotStarted => "Not Started",
        }
    }
}

/// The console a configuration shows on the right: its run's or its debug session's.
enum Console {
    Run(run_panel::Run),
    Debug(Entity<debugger_ui::RunningState>),
}

pub struct ServicesPanel {
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    configurations: Entity<ProjectConfigurations>,
    /// The configuration types added to Services, by their names ("Shell Script").
    types: BTreeSet<SharedString>,
    group_by_status: bool,
    /// Group Services by Type: the run configurations under a "Run Configurations" node.
    // shortcut: kept while the IDE runs; IntelliJ keeps it in the project's view state
    group_by_contributor: bool,
    /// The selected configuration, by its name.
    selected: Option<SharedString>,
    /// The selected contributed service, by its id.
    contributed: Option<SharedString>,
    /// Bumped by Expand All and Collapse All, so the tree's groups start over open or closed.
    generation: usize,
    expand: bool,
    /// Docker's connections and what they hold.
    docker: Entity<Docker>,
    focus: FocusHandle,
    /// Draws the tree again shortly while a service starts, until its type says it started.
    poll: Option<gpui::Task<()>>,
    /// Each started run's console text when its type said it started, by its console.
    started: std::cell::RefCell<collections::HashMap<gpui::EntityId, String>>,
    /// The configurations showing their type's view (the Actuator) rather than the console.
    showing_content: BTreeSet<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl ServicesPanel {
    fn new(
        workspace: WeakEntity<Workspace>,
        project: Entity<Project>,
        configurations: Entity<ProjectConfigurations>,
        cx: &mut Context<Self>,
    ) -> Self {
        let dap_store = project.read(cx).dap_store();
        let types = read_types(&project, cx);
        let docker = cx.new(|cx| Docker::new(workspace.clone(), project.clone(), cx));
        Self {
            workspace,
            project,
            _subscriptions: vec![
                cx.observe(&configurations, |_, _, cx| cx.notify()),
                cx.observe(&dap_store, |_, _, cx| cx.notify()),
                cx.observe(&docker, |_, _, cx| cx.notify()),
                cx.subscribe(&docker, |this, _, event: &DockerEvent, cx| match event {
                    DockerEvent::Selected => {
                        this.selected = None;
                        this.contributed = None;
                        cx.notify();
                    }
                }),
            ],
            docker,
            configurations,
            types,
            group_by_status: false,
            group_by_contributor: false,
            selected: None,
            contributed: None,
            generation: 0,
            expand: true,
            focus: cx.focus_handle(),
            poll: None,
            showing_content: BTreeSet::new(),
            started: Default::default(),
        }
    }

    /// What a running configuration's type tells of it (starting, its link), from its console.
    fn service_state(
        &self,
        workspace: &Workspace,
        configuration: &Configuration,
        cx: &App,
    ) -> Option<crate::configurations::ServiceState> {
        let runner = crate::configurations::runner(configuration, cx)?;
        let (console, running, id) = console_of(workspace, &configuration.name(), cx)?;
        if !running {
            return None;
        }
        let state = runner.service(configuration, &console)?;
        // a started run stays started once Clear All empties its console: its type reads the
        // console it started with
        let mut started = self.started.borrow_mut();
        match (state.starting, started.get(&id)) {
            (false, _) => {
                started.insert(id, console);
                Some(state)
            }
            (true, Some(console)) => runner.service(configuration, console),
            (true, None) => Some(state),
        }
    }

    /// The configurations Services shows, with what each is doing.
    fn services(&self, workspace: &Workspace, cx: &App) -> Vec<(Configuration, Status)> {
        self.configurations
            .read(cx)
            .list
            .iter()
            .filter(|configuration| self.types.contains(&configuration.type_name()))
            .map(|configuration| {
                let name = configuration.name();
                let session = debug_session_of(workspace, &name, cx);
                let run = run_panel::run_of(workspace, &name, cx);
                let status = match (&run, &session) {
                    (Some(run), _) if run.running => Status::Running,
                    (_, Some(session)) if !session.read(cx).session().read(cx).is_terminated() => {
                        Status::Running
                    }
                    (Some(run), _) => match run.failed {
                        Some(true) => Status::Failed,
                        _ => Status::Finished,
                    },
                    (None, Some(_)) => Status::Finished,
                    (None, None) => Status::NotStarted,
                };
                (configuration.clone(), status)
            })
            .collect()
    }

    fn console(&self, workspace: &Workspace, name: &str, cx: &App) -> Option<Console> {
        let session = debug_session_of(workspace, name, cx);
        match (run_panel::run_of(workspace, name, cx), session) {
            (_, Some(session)) if !session.read(cx).session().read(cx).is_terminated() => {
                Some(Console::Debug(session))
            }
            (Some(run), _) => Some(Console::Run(run)),
            (None, Some(session)) => Some(Console::Debug(session)),
            (None, None) => None,
        }
    }

    fn group_services_by_type(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.group_by_contributor = !self.group_by_contributor;
        cx.notify();
    }

    fn add_types(&mut self, added: impl IntoIterator<Item = SharedString>, cx: &mut Context<Self>) {
        self.types.extend(added);
        write_types(&self.project, &self.types, cx);
        cx.notify();
    }

    fn launch(&self, name: &SharedString, debug: bool, window: &mut Window, cx: &mut App) {
        let configuration = self
            .configurations
            .read(cx)
            .list
            .iter()
            .find(|configuration| &configuration.name() == name)
            .cloned();
        if let Some(configuration) = configuration {
            self.workspace
                .update(cx, |workspace, cx| {
                    crate::run_widget::launch(Some(&configuration), debug, workspace, window, cx)
                })
                .log_err();
        }
    }

    fn stop(&self, name: &SharedString, cx: &mut App) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        run_panel::stop_runs_of(&workspace, name, cx);
        if let Some(running) = debug_session_of(workspace.read(cx), name, cx) {
            let session = running.read(cx).session().clone();
            session
                .update(cx, |session, cx| session.shutdown(cx))
                .detach();
        }
    }

    /// Edit Default Path (`SpringBootApplicationUrlPathEditAction`): IntelliJ's "Configure Default
    /// Path" dialog over the path the configuration's link opens.
    fn edit_default_path(&self, name: &SharedString, cx: &mut App) {
        let configurations = self.configurations.clone();
        let name = name.clone();
        let path = configurations
            .read(cx)
            .list
            .iter()
            .find(|configuration| configuration.name() == name)
            .and_then(|configuration| configuration.entry.get(crate::configurations::URL_PATH))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        crate::docker::open_dialog(DEFAULT_PATH_TITLE, 420., 150., cx, move |window, cx| {
            let input = cx.new(|cx| {
                let mut input = gpui_kit::component::input::InputState::new(window, cx);
                input.set_value(path, window, cx);
                input
            });
            input.update(cx, |input, cx| input.focus(window, cx));
            DefaultPathDialog {
                configurations,
                name,
                input,
            }
        });
    }

    /// Add Service ▸ Run Configuration…: IntelliJ's "Add Configuration" chooser of the types.
    fn choose_type(&self, window: &mut Window, cx: &mut Context<Self>) {
        let types: Vec<(SharedString, Option<SharedString>)> =
            crate::configurations_dialog::types(cx)
                .into_iter()
                .map(|adapter| {
                    (
                        crate::configurations::type_name(adapter.as_deref()),
                        adapter,
                    )
                })
                .filter(|(name, _)| !self.types.contains(name))
                .collect();
        let rows = types
            .iter()
            .map(|(name, adapter)| {
                Row::new(name.clone()).image(crate::configurations::type_icon(adapter.as_deref()))
            })
            .collect();
        let services = cx.entity().downgrade();
        self.workspace
            .update(cx, |workspace, cx| {
                ListPopup::show(
                    workspace,
                    "Add Configuration",
                    rows,
                    "Nothing to show",
                    300.,
                    move |ix, _, cx| {
                        if let Some((name, _)) = types.get(ix) {
                            services
                                .update(cx, |services, cx| services.add_types([name.clone()], cx))
                                .log_err();
                        }
                    },
                    window,
                    cx,
                )
            })
            .log_err();
    }

    fn toolbar(&self, empty: bool, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let this = cx.entity().downgrade();
        let docker = self.docker.downgrade();
        let add = ActionButton::new("services-add", "general/add")
            .tooltip("Add Service (⌘N)")
            .dropdown_menu(move |menu, _, _| {
                let this = this.clone();
                let docker = docker.clone();
                menu.item(PopupMenuItem::new("Run Configuration…").on_click(
                    move |_, window, cx| {
                        this.update(cx, |this, cx| this.choose_type(window, cx))
                            .log_err();
                    },
                ))
                // nothing can be hidden from Services yet
                .item(PopupMenuItem::new("Restore Hidden Configurations").disabled(true))
                .item(
                    PopupMenuItem::new("Docker Connection")
                        .on_click(move |_, _, cx| NewConnection::open(docker.clone(), cx)),
                )
            });
        let this = cx.entity().downgrade();
        let by_status = self.group_by_status;
        let by_contributor = self.group_by_contributor;
        let options = ActionButton::new("services-view-options", "general/show")
            .tooltip("View Options")
            .dropdown_menu(move |menu, window, cx| {
                let this = this.clone();
                // `ServiceView.ViewOptions`: Services shows run configurations only, so Group
                // Services by Type has one type to group
                menu.item(
                    PopupMenuItem::new("Group Services by Type")
                        .checked(by_contributor)
                        .on_click({
                            let this = this.clone();
                            move |_, window, cx| {
                                this.update(cx, |this, cx| this.group_services_by_type(window, cx))
                                    .log_err();
                            }
                        }),
                )
                .separator()
                .submenu("Run Configurations", window, cx, move |menu, _, _| {
                    let group = |label: &'static str, status: bool| {
                        let this = this.clone();
                        PopupMenuItem::new(label)
                            .checked(by_status == status)
                            .on_click(move |_, _, cx| {
                                this.update(cx, |this, cx| {
                                    this.group_by_status = status;
                                    cx.notify();
                                })
                                .log_err();
                            })
                    };
                    menu.item(PopupMenuItem::label("Group"))
                        .item(group("Type", false))
                        .item(group("Status", true))
                })
            });
        h_flex()
            .id("services-toolbar")
            .role(Role::Toolbar)
            .aria_label("ServiceViewTreeToolbar")
            .flex_none()
            .h(px(32.))
            .px(px(5.))
            .gap(px(4.))
            .child(add)
            .child(options)
            .child(div().w(px(1.)).h(px(20.)).mx(px(3.)).bg(tokens.separator))
            .child(
                ActionButton::new("services-expand-all", "general/expandAll")
                    .tooltip("Expand All (⌘=)")
                    .disabled(empty)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.expand = true;
                        this.generation += 1;
                        cx.notify();
                    })),
            )
            .child(
                ActionButton::new("services-collapse-all", "general/collapseAll")
                    .tooltip("Collapse All (⌘-)")
                    .disabled(empty)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.expand = false;
                        this.generation += 1;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    fn tree(
        &self,
        services: &[(Configuration, Status)],
        contributed: &[ContributedGroup],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = Tokens::get(cx);
        if services.is_empty() && self.docker.read(cx).is_empty() && contributed.is_empty() {
            let this = cx.entity().downgrade();
            return v_flex()
                .id("services-empty")
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .text_color(tokens.text_secondary)
                .child(
                    div()
                        .id("services-none")
                        .role(Role::Label)
                        .aria_label("No services configured.")
                        .child("No services configured."),
                )
                .child(
                    h_flex()
                        .gap(px(4.))
                        .child(
                            spork_workbench::controls::link("services-add-link", "Add service", cx)
                                .on_click(move |_, window, cx| {
                                    this.update(cx, |this, cx| this.choose_type(window, cx))
                                        .log_err();
                                }),
                        )
                        .child("⌘N"),
                )
                .into_any_element();
        }
        let mut groups: Vec<(
            SharedString,
            Option<&'static str>,
            Vec<&(Configuration, Status)>,
        )> = Vec::new();
        for service in services {
            let (title, icon) = match self.group_by_status {
                true => (SharedString::from(service.1.title()), None),
                false => (service.0.type_name(), Some(service.0.icon())),
            };
            match groups.iter_mut().find(|(name, _, _)| name == &title) {
                Some((_, _, members)) => members.push(service),
                None => groups.push((title, icon, vec![service])),
            }
        }
        if self.group_by_status {
            groups.sort_by_key(|(_, _, members)| members.first().map(|(_, status)| *status));
        } else {
            groups.sort_by(|a, b| a.0.cmp(&b.0));
        }
        let items = groups.into_iter().map(|(title, icon, members)| {
            let children = members.into_iter().map(|(configuration, status)| {
                let name = configuration.name();
                let selected = self.selected.as_ref() == Some(&name);
                let icon = match status {
                    Status::Running => "run/run",
                    _ => configuration.icon(),
                };
                let (select, menu) = (cx.entity().downgrade(), cx.entity().downgrade());
                let docker = self.docker.downgrade();
                let (picked, menu_name) = (name.clone(), name.clone());
                let running = *status == Status::Running;
                let default_path = crate::configurations::runner(configuration, cx)
                    .is_some_and(|runner| runner.has_default_path());
                let state = self
                    .workspace
                    .upgrade()
                    .and_then(|workspace| self.service_state(workspace.read(cx), configuration, cx))
                    .unwrap_or_default();
                // IntelliJ animates a starting application's node
                let icon = match state.starting {
                    true => "process/step_4",
                    false => icon,
                };
                let item = match state.link {
                    Some((text, url)) => {
                        let id = SharedString::from(format!("services-link-{name}"));
                        TreeItem::new(name).after_label(move |_, cx| {
                            let url = url.clone();
                            spork_workbench::controls::link(id.clone(), text.clone(), cx)
                                .on_click(move |_, _, cx| cx.open_url(&url))
                        })
                    }
                    None => TreeItem::new(name),
                };
                item.colored_icon(icon)
                    .active(selected)
                    .on_click(move |_, _, cx| {
                        docker
                            .update(cx, |docker, cx| docker.clear_selection(cx))
                            .log_err();
                        select
                            .update(cx, |this, cx| {
                                this.selected = Some(picked.clone());
                                this.contributed = None;
                                cx.notify();
                            })
                            .log_err();
                    })
                    .context_menu(move |popup, _, _| {
                        let item = |label: String, enabled: bool, act: u8| {
                            let (this, name) = (menu.clone(), menu_name.clone());
                            PopupMenuItem::new(label).disabled(!enabled).on_click(
                                move |_, window, cx| {
                                    this.update(cx, |this, cx| match act {
                                        0 => this.launch(&name, false, window, cx),
                                        1 => this.launch(&name, true, window, cx),
                                        2 => this.stop(&name, cx),
                                        _ => this.edit_default_path(&name, cx),
                                    })
                                    .log_err();
                                },
                            )
                        };
                        let popup = popup
                            .item(item(format!("Run '{menu_name}'"), true, 0))
                            .item(item(format!("Debug '{menu_name}'"), true, 1))
                            .item(item("Stop".into(), running, 2));
                        match default_path {
                            true => {
                                popup
                                    .separator()
                                    .item(item("Edit Default Path".into(), true, 3))
                            }
                            false => popup,
                        }
                    })
            });
            let group = TreeItem::new(title)
                .default_open(self.expand)
                .children(children.collect::<Vec<_>>());
            match icon {
                Some(icon) => group.colored_icon(icon),
                None => group,
            }
        });
        let mut items: Vec<TreeItem> = match self.group_by_contributor {
            true if !services.is_empty() => vec![
                TreeItem::new("Run Configurations")
                    .default_open(self.expand)
                    .children(items),
            ],
            _ => items.collect(),
        };
        // Docker's nodes start closed, as IntelliJ's, until Expand All
        let expand = self.expand && self.generation > 0;
        let docker = self
            .docker
            .update(cx, |docker, cx| docker.tree_items(expand, cx));
        match self.group_by_contributor {
            true if !docker.is_empty() => items.push(
                TreeItem::new("Docker")
                    .colored_icon("webReferences/server")
                    .default_open(self.expand)
                    .children(docker),
            ),
            _ => items.extend(docker),
        }
        for group in contributed {
            let children = group.services.iter().map(|service| {
                let (this, docker, id) = (
                    cx.entity().downgrade(),
                    self.docker.downgrade(),
                    service.id.clone(),
                );
                TreeItem::new(service.label.clone())
                    .colored_icon(service.icon)
                    .active(self.contributed.as_ref() == Some(&service.id))
                    .on_click(move |_, _, cx| {
                        docker
                            .update(cx, |docker, cx| docker.clear_selection(cx))
                            .log_err();
                        this.update(cx, |this, cx| {
                            this.selected = None;
                            this.contributed = Some(id.clone());
                            cx.notify();
                        })
                        .log_err();
                    })
            });
            items.push(
                TreeItem::new(group.title.clone())
                    .colored_icon(group.icon)
                    .default_open(true)
                    .children(children.collect::<Vec<_>>()),
            );
        }
        div()
            .id("services-tree")
            .role(Role::Tree)
            .aria_label("Services")
            .flex_1()
            .py(px(4.))
            .overflow_y_scroll()
            .child(Tree::new(format!("services-{}", self.generation)).children(items))
            .into_any_element()
    }

    /// The selected configuration's console, with its toolbar on the left.
    fn details(
        &mut self,
        contributed: &[ContributedGroup],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = Tokens::get(cx);
        let view = contributed
            .iter()
            .flat_map(|group| &group.services)
            .find(|service| self.contributed.as_ref() == Some(&service.id))
            .map(|service| service.view.clone());
        if let Some(view) = view {
            return div()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(view)
                .into_any_element();
        }
        if self.docker.read(cx).has_selection() {
            return div()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(self.docker.clone())
                .into_any_element();
        }
        let centered = |text: &'static str| {
            div()
                .id(text)
                .role(Role::Label)
                .aria_label(text)
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(tokens.text_secondary)
                .child(text)
                .into_any_element()
        };
        let Some(name) = self.selected.clone() else {
            return centered("Select service to view details");
        };
        let Some(workspace) = self.workspace.upgrade() else {
            return centered("Select service to view details");
        };
        let console = self.console(workspace.read(cx), &name, cx);
        let running = match &console {
            Some(Console::Run(run)) => run.running,
            Some(Console::Debug(session)) => !session.read(cx).session().read(cx).is_terminated(),
            None => false,
        };
        let terminal = match &console {
            Some(Console::Run(run)) => Some(run.terminal.clone()),
            _ => None,
        };
        let update = match &console {
            Some(Console::Debug(session)) => Some(session.read(cx).session().clone()),
            _ => None,
        };
        let update = running
            .then(|| update_button(&workspace, &name, update, cx))
            .flatten();
        // the type's own view beside the console, as IntelliJ's Actuator tab
        let configuration = self
            .configurations
            .read(cx)
            .list
            .iter()
            .find(|configuration| configuration.name() == name)
            .cloned();
        let extra = configuration.as_ref().and_then(|configuration| {
            crate::configurations::runner(configuration, cx)?.content(
                configuration,
                &workspace,
                window,
                cx,
            )
        });
        let showing_extra = extra.is_some() && self.showing_content.contains(&name);
        let content: AnyElement = match (console, &extra) {
            (_, Some((_, view))) if showing_extra => view.clone().into_any_element(),
            (Some(Console::Run(run)), _) => AnyView::from(run.view).into_any_element(),
            (Some(Console::Debug(session)), _) => {
                session.read(cx).console_view().into_any_element()
            }
            (None, _) => centered("No content available"),
        };
        let tabs = extra.map(|(title, _)| {
            let tab = |id: &'static str, label: SharedString, on: bool, cx: &mut Context<Self>| {
                let show = !id.ends_with("console");
                let name = name.clone();
                div()
                    .id(id)
                    .role(Role::Tab)
                    .aria_label(label.clone())
                    .aria_selected(on)
                    .h(px(28.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(on, |tab| tab.bg(tokens.tab_selected))
                    .when(!on, |tab| tab.hover(move |tab| tab.bg(tokens.hover)))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        match show {
                            true => this.showing_content.insert(name.clone()),
                            false => this.showing_content.remove(&name),
                        };
                        cx.notify();
                    }))
            };
            h_flex()
                .id("services-content-tabs")
                .role(Role::TabList)
                .aria_label("Service Content")
                .flex_none()
                .px(px(6.))
                .py(px(2.))
                .gap(px(2.))
                .border_b_1()
                .border_color(tokens.separator)
                .child(tab(
                    "services-content-console",
                    "Console".into(),
                    !showing_extra,
                    cx,
                ))
                .child(tab("services-content-extra", title, showing_extra, cx))
        });
        let (rerun, debug, stop) = (
            cx.entity().downgrade(),
            cx.entity().downgrade(),
            cx.entity().downgrade(),
        );
        let (rerun_name, debug_name, stop_name) = (name.clone(), name.clone(), name.clone());
        let (scroll, clear) = (terminal.clone(), terminal.clone());
        h_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .items_start()
            .child(
                v_flex()
                    .id("services-console-toolbar")
                    .role(Role::Toolbar)
                    .aria_label("ServiceViewItemToolbar")
                    .flex_none()
                    .w(px(32.))
                    .h_full()
                    .pt(px(4.))
                    .items_center()
                    .gap(px(4.))
                    .border_r_1()
                    .border_color(tokens.separator)
                    .child(
                        ActionButton::new("services-run", "run/run")
                            .colored()
                            .tooltip(format!("Run '{name}'"))
                            .on_click(move |_, window, cx| {
                                rerun
                                    .update(cx, |this, cx| {
                                        this.launch(&rerun_name, false, window, cx)
                                    })
                                    .log_err();
                            }),
                    )
                    .child(
                        ActionButton::new("services-debug", "run/debug")
                            .colored()
                            .tooltip(format!("Debug '{name}'"))
                            .on_click(move |_, window, cx| {
                                debug
                                    .update(cx, |this, cx| {
                                        this.launch(&debug_name, true, window, cx)
                                    })
                                    .log_err();
                            }),
                    )
                    .child(
                        match running {
                            true => ActionButton::new("services-stop", "run/stop").colored(),
                            false => ActionButton::new("services-stop", "run/stop"),
                        }
                        .tooltip(format!("Stop '{name}'"))
                        .disabled(!running)
                        .on_click(move |_, _, cx| {
                            stop.update(cx, |this, cx| this.stop(&stop_name, cx))
                                .log_err();
                        }),
                    )
                    .children(update)
                    .child(div().w(px(20.)).h(px(1.)).my(px(3.)).bg(tokens.separator))
                    .child(
                        ActionButton::new("services-scroll-to-end", "general/scrollDown")
                            .tooltip("Scroll to End")
                            .disabled(terminal.is_none())
                            .on_click(move |_, window, cx| {
                                if let Some(terminal) = &scroll {
                                    terminal.update(cx, |terminal, cx| {
                                        terminal.scroll_to_bottom();
                                        terminal.sync(window, cx);
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        ActionButton::new("services-clear-all", "general/delete")
                            .tooltip("Clear All")
                            .disabled(terminal.is_none())
                            .on_click(move |_, window, cx| {
                                if let Some(terminal) = &clear {
                                    terminal.update(cx, |terminal, cx| {
                                        terminal.clear();
                                        terminal.sync(window, cx);
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child(
                v_flex().flex_1().min_w_0().h_full().children(tabs).child(
                    div()
                        .id("services-console")
                        .role(Role::Group)
                        .aria_label("Console")
                        .flex_1()
                        .min_h_0()
                        .min_w_0()
                        .child(content),
                ),
            )
            .into_any_element()
    }
}

impl Render for ServicesPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let services = match self.workspace.upgrade() {
            Some(workspace) => self.services(workspace.read(cx), cx),
            None => Vec::new(),
        };
        if self
            .selected
            .as_ref()
            .is_some_and(|name| !services.iter().any(|(c, _)| &c.name() == name))
        {
            self.selected = None;
        }
        let header = workspace::frame::tool_window_header(
            workspace::frame::tool_window_title_label("Services").into_any_element(),
            Vec::new(),
            std::sync::Arc::new(cx.entity()),
            "Services".into(),
            cx,
        )
        .into_any_element();
        // shortcut: polls the consoles of starting services; follow their output once a console
        // tells its listeners of new lines
        let starting = self.workspace.upgrade().is_some_and(|workspace| {
            services.iter().any(|(configuration, status)| {
                *status == Status::Running
                    && self
                        .service_state(workspace.read(cx), configuration, cx)
                        .is_some_and(|state| state.starting)
            })
        });
        if starting && self.poll.is_none() {
            self.poll = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                this.update(cx, |this, cx| {
                    this.poll = None;
                    cx.notify();
                })
                .ok();
            }));
        }
        let contributed = self
            .workspace
            .upgrade()
            .map(|workspace| contributed(workspace.read(cx), cx))
            .unwrap_or_default();
        let empty =
            services.is_empty() && self.docker.read(cx).is_empty() && contributed.is_empty();
        let toolbar = self.toolbar(empty, cx);
        let tree = self.tree(&services, &contributed, cx);
        let details = self.details(&contributed, window, cx);
        spork_vcs::spork_look(cx).size_full().child(
            v_flex()
                .size_full()
                .bg(tokens.island)
                .track_focus(&self.focus)
                .key_context("ServicesPanel")
                .on_action(cx.listener(
                    |this, _: &services_panel::GroupServicesByType, window, cx| {
                        this.group_services_by_type(window, cx)
                    },
                ))
                .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                    let keystroke = &event.keystroke;
                    if keystroke.key == "n" && keystroke.modifiers.platform {
                        cx.stop_propagation();
                        this.choose_type(window, cx);
                    }
                }))
                .child(header)
                .child(
                    h_flex()
                        .flex_1()
                        .min_h_0()
                        .items_start()
                        .child(
                            v_flex()
                                .w(relative_tree_width())
                                .flex_none()
                                .h_full()
                                .border_r_1()
                                .border_color(tokens.separator)
                                .child(toolbar)
                                .child(tree),
                        )
                        .child(details),
                ),
        )
    }
}

const DEFAULT_PATH_TITLE: &str = "Configure Default Path";

/// IntelliJ's "Configure Default Path" dialog: "Default Path:" over the path a configuration's
/// Services link opens; OK keeps it in the configuration.
struct DefaultPathDialog {
    configurations: Entity<ProjectConfigurations>,
    name: SharedString,
    input: Entity<gpui_kit::component::input::InputState>,
}

impl DefaultPathDialog {
    fn ok(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.input.read(cx).value().trim().to_owned();
        let name = self.name.clone();
        self.configurations.update(cx, |configurations, cx| {
            let mut list = configurations.list.clone();
            if let Some(configuration) = list.iter_mut().find(|c| c.name() == name) {
                match path.is_empty() {
                    true => configuration.entry.remove(crate::configurations::URL_PATH),
                    false => configuration
                        .entry
                        .insert(crate::configurations::URL_PATH.into(), path.into()),
                };
            }
            configurations.save(list, cx).detach_and_log_err(cx);
        });
        window.remove_window();
    }
}

impl Render for DefaultPathDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label = "Default Path:";
        let body = h_flex()
            .gap(px(8.))
            .child(spork_workbench::controls::form_label(label))
            .child(
                div()
                    .flex_1()
                    .child(spork_workbench::TextField::new(&self.input).aria_label(label)),
            )
            .into_any_element();
        let buttons = h_flex()
            .gap(px(8.))
            .child(
                spork_workbench::Button::new("default-path-cancel", "Cancel")
                    .on_click(|_, window, _| window.remove_window()),
            )
            .child(
                spork_workbench::Button::new("default-path-ok", "OK")
                    .primary(true)
                    .on_click(cx.listener(|this, _, window, cx| this.ok(window, cx))),
            )
            .into_any_element();
        div()
            .size_full()
            .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| this.ok(window, cx)))
            .child(crate::docker::dialog_frame(
                DEFAULT_PATH_TITLE,
                body,
                buttons,
                cx,
            ))
    }
}

/// The tree's width: IntelliJ's splitter gives it 0.3 of the tool window.
fn relative_tree_width() -> gpui::DefiniteLength {
    gpui::relative(0.3)
}

impl Focusable for ServicesPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EventEmitter<PanelEvent> for ServicesPanel {}

impl Panel for ServicesPanel {
    fn persistent_name() -> &'static str {
        "Services"
    }

    fn panel_key() -> &'static str {
        "ServicesToolWindow"
    }

    fn position(&self, _: &Window, _: &App) -> DockPosition {
        DockPosition::Bottom
    }

    // shortcut: the tool window stays where IntelliJ puts it, as the Run tool window does, until
    // the stripes let tool windows move
    fn position_is_valid(&self, position: DockPosition) -> bool {
        position == DockPosition::Bottom
    }

    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}

    // a local daemon started since is found once Services shows again, as IntelliJ finds it
    fn set_active(&mut self, active: bool, _: &mut Window, cx: &mut Context<Self>) {
        if active {
            self.docker.update(cx, |docker, cx| docker.detect(cx));
        }
    }

    fn default_size(&self, _: &Window, _: &App) -> Pixels {
        px(300.)
    }

    fn icon(&self, _: &Window, _: &App) -> Option<ui::IconName> {
        Some(ui::IconName::Server)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some("Services")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(services_panel::ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        30
    }
}

/// Shows the configuration `name` in Services when Services lists it, as IntelliJ shows a run
/// there rather than in the Run tool window; whether it did.
pub(crate) fn reveal(
    workspace: &mut Workspace,
    name: &str,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> bool {
    let Some(panel) = workspace.panel::<ServicesPanel>(cx) else {
        return false;
    };
    let listed = panel
        .read(cx)
        .services(workspace, cx)
        .iter()
        .any(|(configuration, _)| configuration.name().as_ref() == name);
    if listed {
        panel.update(cx, |panel, cx| {
            panel.selected = Some(name.to_owned().into());
            cx.notify();
        });
        workspace.open_panel::<ServicesPanel>(window, cx);
    }
    listed
}

/// The debug session of the configuration `name`, its latest.
fn debug_session_of(
    workspace: &Workspace,
    name: &str,
    cx: &App,
) -> Option<Entity<debugger_ui::RunningState>> {
    let panel = workspace.panel::<debugger_ui::debugger_panel::DebugPanel>(cx)?;
    panel
        .read(cx)
        .session_items()
        .into_iter()
        .rev()
        .map(|session| session.read(cx).running_state().clone())
        .find(|running| {
            running
                .read(cx)
                .session()
                .read(cx)
                .label()
                .is_some_and(|label| label.as_ref() == name)
        })
}

/// Whether the configuration `name` runs, in the Run tool window or a debug session.
pub fn is_running(workspace: &Workspace, name: &str, cx: &App) -> bool {
    run_panel::run_of(workspace, name, cx).is_some_and(|run| run.running)
        || debug_session_of(workspace, name, cx)
            .is_some_and(|running| !running.read(cx).session().read(cx).is_terminated())
}

/// The key the types added to Services are kept under, per project.
fn types_key(project: &Entity<Project>, cx: &App) -> Option<String> {
    let root = project
        .read(cx)
        .visible_worktrees(cx)
        .next()?
        .read(cx)
        .abs_path();
    Some(format!("spork_services_types_{}", root.display()))
}

/// The types Services shows before any is added, as IntelliJ's run dashboard's default types.
fn default_types() -> BTreeSet<SharedString> {
    BTreeSet::from([SharedString::from(crate::configurations::SPRING_BOOT)])
}

fn read_types(project: &Entity<Project>, cx: &App) -> BTreeSet<SharedString> {
    let Some(key) = types_key(project, cx) else {
        return default_types();
    };
    db::kvp::KeyValueStore::global(cx)
        .read_kvp(&key)
        .log_err()
        .flatten()
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).log_err())
        .map(|types| types.into_iter().map(SharedString::from).collect())
        .unwrap_or_else(default_types)
}

/// Runs or debugs the configuration `name`, as its Run and Debug buttons do.
pub fn launch_configuration(
    workspace: &mut Workspace,
    name: &str,
    debug: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let configuration = ProjectConfigurations::of(workspace, cx).and_then(|configurations| {
        configurations
            .read(cx)
            .list
            .iter()
            .find(|configuration| configuration.name().as_ref() == name)
            .cloned()
    });
    if let Some(configuration) = configuration {
        crate::run_widget::launch(Some(&configuration), debug, workspace, window, cx);
    }
}

/// The console text of the configuration `name`'s latest run or debug session, and whether it
/// still runs.
pub fn console_text(workspace: &Workspace, name: &str, cx: &App) -> Option<(String, bool)> {
    console_of(workspace, name, cx).map(|(text, running, _)| (text, running))
}

/// The console text, whether it runs, and the console's entity.
fn console_of(
    workspace: &Workspace,
    name: &str,
    cx: &App,
) -> Option<(String, bool, gpui::EntityId)> {
    let session = debug_session_of(workspace, name, cx);
    match (run_panel::run_of(workspace, name, cx), session) {
        (_, Some(running)) if !running.read(cx).session().read(cx).is_terminated() => {
            let id = running.entity_id();
            // the program's own terminal when it runs in one (Java's), the debugger's output
            if let Some(terminal) = running.read(cx).program_terminal(cx) {
                return Some((
                    terminal.read(cx).terminal().read(cx).get_content(),
                    true,
                    id,
                ));
            }
            let session = running.read(cx).session().read(cx);
            let (events, _) = session.output(project::debugger::session::OutputToken(0));
            Some((
                events.map(|event| event.output.as_str()).collect(),
                true,
                id,
            ))
        }
        (Some(run), _) => Some((
            run.terminal.read(cx).get_content(),
            run.running,
            run.terminal.entity_id(),
        )),
        _ => None,
    }
}

fn write_types(project: &Entity<Project>, types: &BTreeSet<SharedString>, cx: &mut App) {
    let Some(key) = types_key(project, cx) else {
        return;
    };
    let value = serde_json::to_string(&types.iter().map(|t| t.to_string()).collect::<Vec<_>>());
    let store = db::kvp::KeyValueStore::global(cx);
    cx.background_spawn(async move { store.write_kvp(key, value?).await })
        .detach_and_log_err(cx);
}

/// What runs at the moment: the runs, then the debug sessions, each with its executor's icon.
fn running_list(workspace: &Workspace, cx: &App) -> Vec<(SharedString, &'static str, bool)> {
    let runs = run_panel::runs(workspace, cx)
        .into_iter()
        .filter(|run| run.running)
        .map(|run| (run.title, "run/run", false));
    let sessions = workspace
        .panel::<debugger_ui::debugger_panel::DebugPanel>(cx)
        .map(|panel| panel.read(cx).session_items())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|session| {
            let running = session.read(cx).running_state().read(cx);
            let session = running.session().read(cx);
            (!session.is_terminated())
                .then(|| (session.label().unwrap_or_default(), "run/debug", true))
        });
    runs.chain(sessions).collect()
}

/// A running configuration Update Running Application updates: it, its debug session when it
/// is debugged, its runner and what the update says.
struct Updater {
    configuration: Configuration,
    session: Option<Entity<project::debugger::session::Session>>,
    runner: std::rc::Rc<dyn crate::configurations::Runner>,
    text: SharedString,
}

/// The running configurations whose type updates them (IntelliJ's `RunningApplicationUpdater`s),
/// the runs first.
fn updaters(workspace: &Workspace, cx: &App) -> Vec<Updater> {
    let runs = run_panel::runs(workspace, cx)
        .into_iter()
        .filter(|run| run.running)
        .map(|run| (run.label, None));
    let sessions = workspace
        .panel::<debugger_ui::debugger_panel::DebugPanel>(cx)
        .map(|panel| panel.read(cx).session_items())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            let session = item.read(cx).running_state().read(cx).session().clone();
            let label = session.read(cx).label()?;
            (!session.read(cx).is_terminated()).then_some((label, Some(session)))
        });
    runs.chain(sessions)
        .filter_map(|(name, session)| updater(workspace, &name, session, cx))
        .collect()
}

/// The running configuration `name`'s update, when its type updates it.
fn updater(
    workspace: &Workspace,
    name: &str,
    session: Option<Entity<project::debugger::session::Session>>,
    cx: &App,
) -> Option<Updater> {
    let configurations = ProjectConfigurations::of(workspace, cx)?;
    let configuration = configurations
        .read(cx)
        .list
        .iter()
        .find(|configuration| configuration.name() == name)?
        .clone();
    let runner = crate::configurations::runner(&configuration, cx)?;
    let text = runner.update_text(&configuration, workspace, cx)?;
    Some(Updater {
        configuration,
        session,
        runner,
        text,
    })
}

/// The Run and Debug tool windows' Update button on the running configuration `name` (IntelliJ's
/// `UpdateRunningApplication` in the run content's toolbar after Rerun), when its type updates it.
pub(crate) fn update_button(
    workspace: &Entity<Workspace>,
    name: &str,
    session: Option<Entity<project::debugger::session::Session>>,
    cx: &App,
) -> Option<ActionButton> {
    let updater = updater(workspace.read(cx), name, session, cx)?;
    let workspace = workspace.clone();
    Some(
        ActionButton::new("run-update", "javaee/updateRunningApplication")
            .tooltip(format!("{} (⌘F10)", updater.text))
            .on_click(move |_, _, cx| {
                updater.runner.update(
                    &updater.configuration,
                    updater.session.clone(),
                    &workspace,
                    cx,
                )
            }),
    )
}

/// Update Running Application (⌘F10): the one running application's update, or a choice of
/// them ("Select Process to Update").
fn update_running_application(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let mut updaters = updaters(workspace, cx);
    let handle = cx.entity();
    if updaters.len() == 1
        && let Some(updater) = updaters.pop()
    {
        cx.defer(move |cx| {
            updater
                .runner
                .update(&updater.configuration, updater.session, &handle, cx)
        });
        return;
    }
    let rows = updaters
        .iter()
        .map(|updater| Row::new(updater.configuration.name()).image(updater.configuration.icon()))
        .collect();
    ListPopup::show(
        workspace,
        "Select Process to Update",
        rows,
        "",
        300.,
        move |ix, _, cx| {
            if let Some(updater) = updaters.get(ix) {
                updater
                    .runner
                    .update(&updater.configuration, updater.session.clone(), &handle, cx);
            }
        },
        window,
        cx,
    );
}

/// What the end-to-end tests read of Update Running Application: each updatable configuration and
/// what its update says.
pub(crate) fn e2e_updaters(workspace: &Workspace, cx: &App) -> Value {
    updaters(workspace, cx)
        .iter()
        .map(|updater| json!([updater.configuration.name(), updater.text]))
        .collect()
}

/// Show Running List (`ShowLiveRunConfigurations`): IntelliJ's "Running List" balloon under the
/// frame's top right corner, a link per run that shows its tool window.
fn show_running_list(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let list = running_list(workspace, cx);
    let handle = workspace.weak_handle();
    workspace.toggle_modal(window, cx, move |window, cx| {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        RunningList {
            list,
            workspace: handle,
            focus,
        }
    });
}

struct RunningList {
    list: Vec<(SharedString, &'static str, bool)>,
    workspace: WeakEntity<Workspace>,
    focus: FocusHandle,
}

impl EventEmitter<DismissEvent> for RunningList {}

impl ModalView for RunningList {}

impl Focusable for RunningList {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for RunningList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let width = window.viewport_size().width;
        let rows = self
            .list
            .iter()
            .enumerate()
            .map(|(ix, (title, icon, debug))| {
                let (workspace, title, debug) = (self.workspace.clone(), title.clone(), *debug);
                h_flex()
                    .id(("running-list-row", ix))
                    .role(Role::Link)
                    .aria_label(title.clone())
                    .gap(px(4.))
                    .cursor_pointer()
                    .child(spork_workbench::image(icon, cx).size(px(16.)))
                    .child(div().text_color(tokens.text_link).child(title.clone()))
                    .on_click(cx.listener(move |_, _, window, cx| {
                        cx.emit(DismissEvent);
                        workspace
                            .update(cx, |workspace, cx| match debug {
                                true => workspace
                                    .open_panel::<debugger_ui::debugger_panel::DebugPanel>(
                                        window, cx,
                                    ),
                                false => {
                                    if !reveal(workspace, &title, window, cx) {
                                        run_panel::show_run(workspace, &title, window, cx)
                                    }
                                }
                            })
                            .log_err();
                    }))
                    .into_any_element()
            });
        let empty = self.list.is_empty();
        anchored()
            .anchor(Anchor::TopRight)
            .position(point(width - px(12.), px(44.)))
            .snap_to_window_with_margin(px(8.))
            .child(
                spork_vcs::spork_look(cx).child(
                    popup::surface(cx)
                        .id("running-list")
                        .role(Role::Dialog)
                        .aria_label("Running List")
                        .track_focus(&self.focus)
                        .key_context("RunningList")
                        .occlude()
                        .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
                        .min_w(px(220.))
                        .p(px(12.))
                        .gap(px(6.))
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::BOLD)
                                .child("Running List"),
                        )
                        .children(rows)
                        .when(empty, |list| list.child("Nothing to show"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(tokens.text_secondary)
                                .child("Click to navigate"),
                        ),
                ),
            )
    }
}

/// The language servers' work that can be cancelled: IntelliJ's cancellable background tasks.
fn cancellable_work(
    project: &Entity<Project>,
    cx: &App,
) -> Vec<(lsp::LanguageServerId, ProgressToken, SharedString)> {
    project
        .read(cx)
        .language_server_statuses(cx)
        .flat_map(|(id, status)| {
            status
                .pending_work
                .iter()
                .filter(|(_, progress)| progress.is_cancellable)
                .map(move |(token, progress)| {
                    let title = progress
                        .title
                        .clone()
                        .or_else(|| progress.message.clone())
                        .unwrap_or_else(|| status.name.to_string());
                    (id, token.clone(), SharedString::from(title))
                })
        })
        .collect()
}

/// Stop Background Processes… (⇧⌘F2): IntelliJ's popup of the cancellable background tasks.
fn stop_background_processes(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let work = cancellable_work(workspace.project(), cx);
    let title = match work.len() {
        1 => "Confirm Background Process Stop",
        _ => "Stop Background Process",
    };
    let rows = work
        .iter()
        .map(|(_, _, title)| Row::new(title.clone()))
        .collect();
    let project = workspace.project().downgrade();
    ListPopup::show(
        workspace,
        title,
        rows,
        "Nothing to show",
        320.,
        move |ix, _, cx| {
            if let Some((id, token, _)) = work.get(ix).cloned() {
                project
                    .update(cx, |project, cx| {
                        project.cancel_language_server_work(id, Some(token), cx)
                    })
                    .log_err();
            }
        },
        window,
        cx,
    );
}

/// The Services tool window, for the end-to-end tests: its groups with their configurations and
/// their status, the selected one and the console it shows.
pub(crate) fn e2e_services(workspace: &Workspace, cx: &App) -> Value {
    let Some(panel) = workspace.panel::<ServicesPanel>(cx) else {
        return Value::Null;
    };
    let panel = panel.read(cx);
    let services: serde_json::Map<String, Value> = panel
        .services(workspace, cx)
        .into_iter()
        .map(|(configuration, status)| {
            let state = panel.service_state(workspace, &configuration, cx);
            (
                configuration.name().to_string(),
                json!({
                    "type": configuration.type_name().as_ref(),
                    "status": status.title(),
                    "starting": state.as_ref().map(|state| state.starting),
                    "link": state.and_then(|state| state.link).map(|(text, url)| json!([text.as_ref(), url])),
                }),
            )
        })
        .collect();
    let console = panel.selected.as_ref().and_then(|name| {
        Some(match panel.console(workspace, name, cx)? {
            Console::Run(run) => json!({"run": run.title.as_ref()}),
            Console::Debug(_) => json!({"debug": name.as_ref()}),
        })
    });
    json!({
        "types": panel.types.iter().map(|t| t.as_ref()).collect::<Vec<_>>(),
        "services": services,
        "selected": panel.selected.as_ref().map(|s| s.as_ref()),
        "contributed": panel.contributed.as_ref().map(|s| s.as_ref()),
        "group_by_status": panel.group_by_status,
        "group_by_contributor": panel.group_by_contributor,
        "console": console,
        "docker": panel.docker.read(cx).e2e_state(cx),
        "running_list": running_list(workspace, cx)
            .into_iter()
            .map(|(title, _, _)| title.to_string())
            .collect::<Vec<_>>(),
    })
}
