//! IntelliJ's run widget in the main toolbar (`RedesignedRunWidget`): the configuration selector
//! with its popup (`RunToolbarPopup`: the configurations by type, each with Run and Debug, then
//! Current File and Edit Configurations…), Run, Debug and, while something runs, Stop; and the
//! Run… and Debug… popups (⌃⌥R, ⌃⌥D).

use gpui::TaskExt as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action as _, AnyElement, App, Context, DismissEvent, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render,
    Role, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    px,
};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{h_flex, v_flex};
use project::TaskSourceKind;
use spork_workbench::tokens::metrics;
use spork_workbench::{ActionButton, ActionSize, Tokens, ToolbarDropdown, popup};
use ui::VisibleOnHover as _;
use util::ResultExt as _;
use workspace::{ModalView, Workspace};

use crate::{Configuration, ProjectConfigurations, run};

gpui::actions!(
    run_widget,
    [
        /// Runs the configuration the run widget shows.
        Run,
        /// Debugs the configuration the run widget shows.
        Debug
    ]
);

pub(crate) fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &Run, window, cx| {
        launch_selected(false, workspace, window, cx)
    });
    workspace.register_action(|workspace, _: &Debug, window, cx| {
        launch_selected(true, workspace, window, cx)
    });
    workspace.register_action(|workspace, _: &run::ChooseRunConfiguration, window, cx| {
        choose(false, workspace, window, cx)
    });
    workspace.register_action(|workspace, _: &run::ChooseDebugConfiguration, window, cx| {
        choose(true, workspace, window, cx)
    });
}

/// Run or Debug on the run widget's configuration.
fn launch_selected(
    debug: bool,
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(configurations) = ProjectConfigurations::of(workspace, cx) else {
        return;
    };
    let selected = configurations.read(cx).selected().cloned();
    launch(selected.as_ref(), debug, workspace, window, cx);
}

/// Whether a configuration runs (a task, or a debug scenario with a runner) or debugs (a debug
/// scenario its debugger can debug); the current file does both.
fn can(configuration: Option<&Configuration>, debug: bool, cx: &gpui::App) -> bool {
    let Some(configuration) = configuration else {
        return true;
    };
    match crate::configurations::runner(configuration, cx) {
        Some(runner) if debug => runner.can_debug(cx),
        Some(_) => true,
        None => configuration.is_task() != debug,
    }
}

/// Runs or debugs a configuration in `workspace`, or the current file without one.
pub(crate) fn launch(
    configuration: Option<&Configuration>,
    debug: bool,
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if !can(configuration, debug, cx) {
        return;
    }
    if let Some(configuration) = configuration
        && let Some(configurations) = ProjectConfigurations::of(workspace, cx)
    {
        let name = configuration.name();
        configurations.update(cx, |configurations, cx| configurations.started(&name, cx));
    }
    let Some(configuration) = configuration else {
        // IntelliJ's Current File: the runnable nearest the caret in the active editor
        // shortcut: Debug on the current file asks for a debug scenario; start one from the
        // file's runnable once the debug locators can say which
        match (debug, workspace.active_item_as::<editor::Editor>(cx)) {
            (false, Some(editor)) => editor.update(cx, |editor, cx| {
                editor.spawn_nearest_task(&editor::actions::SpawnNearestTask::default(), window, cx)
            }),
            (false, None) => window.dispatch_action(zed_actions::Spawn::modal().boxed_clone(), cx),
            (true, _) => window.dispatch_action(debugger_ui::Start.boxed_clone(), cx),
        }
        return;
    };
    if !debug && let Some(runner) = crate::configurations::runner(configuration, cx) {
        let prepare = runner.task(configuration, workspace, cx);
        cx.spawn_in(window, async move |workspace, cx| {
            let template = match prepare.await {
                Ok(template) => template,
                Err(error) => {
                    workspace
                        .update(cx, |workspace, cx| {
                            workspace.show_error(error, cx);
                        })
                        .log_err();
                    return;
                }
            };
            workspace
                .update_in(cx, |workspace, window, cx| {
                    workspace.schedule_task(
                        TaskSourceKind::UserInput,
                        &template,
                        &task::TaskContext::default(),
                        false,
                        window,
                        cx,
                    )
                })
                .log_err();
        })
        .detach();
        return;
    }
    let contexts = tasks_ui::task_contexts(workspace, window, cx);
    let (template, scenario) = (
        configuration.task_template(),
        configuration.debug_scenario(),
    );
    let abs_path = ProjectConfigurations::of(workspace, cx)
        .map(|c| c.read(cx).files(cx).user_tasks)
        .unwrap_or_default();
    cx.spawn_in(window, async move |workspace, cx| {
        let contexts = contexts.await;
        workspace
            .update_in(cx, |workspace, window, cx| {
                let context = contexts.active_context().cloned().unwrap_or_default();
                if let Some(template) = template
                    && let Some(resolved) = template.resolve_task("run configuration", &context)
                {
                    let source = TaskSourceKind::AbsPath {
                        id_base: "run configuration".into(),
                        abs_path,
                    };
                    workspace.schedule_resolved_task(source, resolved, false, window, cx);
                } else if let Some(scenario) = scenario {
                    workspace.start_debug_session(
                        scenario,
                        context.into(),
                        None,
                        contexts.worktree(),
                        window,
                        cx,
                    );
                }
            })
            .log_err();
    })
    .detach();
}

/// IntelliJ's run widget: the configuration selector, Run, Debug and, while running, Stop.
pub struct RunWidget {
    workspace: gpui::WeakEntity<Workspace>,
    configurations: Option<Entity<ProjectConfigurations>>,
    _subscription: Option<Subscription>,
}

impl RunWidget {
    pub fn new(workspace: &Workspace, cx: &mut Context<Self>) -> Self {
        let configurations = ProjectConfigurations::of(workspace, cx);
        let _subscription = configurations
            .as_ref()
            .map(|configurations| cx.observe(configurations, |_, _, cx| cx.notify()));
        Self {
            workspace: workspace.weak_handle(),
            configurations,
            _subscription,
        }
    }

    /// The workspace's configurations, followed once the workspace has them (the title bar may
    /// come first).
    fn configurations(&mut self, cx: &mut Context<Self>) -> Option<Entity<ProjectConfigurations>> {
        if self.configurations.is_none() {
            let workspace = self.workspace.upgrade()?;
            let configurations = ProjectConfigurations::of(workspace.read(cx), cx)?;
            self._subscription = Some(cx.observe(&configurations, |_, _, cx| cx.notify()));
            self.configurations = Some(configurations);
        }
        self.configurations.clone()
    }
}

/// Selects a configuration for the run widget; `None` is the current file.
fn select(
    configurations: &Entity<ProjectConfigurations>,
    name: Option<SharedString>,
    cx: &mut App,
) {
    configurations.update(cx, |configurations, cx| {
        configurations.selected = name;
        cx.notify();
    });
}

fn dispatch(action: Box<dyn gpui::Action>) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) {
    move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx)
}

impl Render for RunWidget {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(configurations) = self.configurations(cx) else {
            return div().into_any_element();
        };
        let selected = configurations.read(cx).selected().cloned();
        let name = selected
            .as_ref()
            .map_or(SharedString::from("Current File"), Configuration::name);
        let quoted = match &selected {
            Some(configuration) => format!(" '{}'", configuration.name()),
            None => String::new(),
        };
        let running = self
            .workspace
            .upgrade()
            .is_some_and(|workspace| crate::run_panel::is_running(workspace.read(cx), cx));
        let icon = selected
            .as_ref()
            .map_or("runConfigurations/application", |c| c.icon());
        let popup_configurations = configurations;
        let selector = Popover::new("run-configurations-popup")
            .appearance(false)
            .trigger(
                ToolbarDropdown::new("run-configurations")
                    .child(spork_workbench::image(icon, cx))
                    .child(
                        div()
                            .max_w(px(metrics::MAIN_TOOLBAR_DROPDOWN_MAX_WIDTH))
                            .truncate()
                            .child(name.clone()),
                    )
                    .tooltip("Run / Debug Configurations")
                    .aria_label(format!("{name}, Run / Debug Configurations")),
            )
            .content(move |_, window, cx| {
                let state = cx.entity();
                configurations_popup(&popup_configurations, state, window, cx)
            });
        let run = ActionButton::new("run", "run/run")
            .size(ActionSize::MainToolbar)
            .colored()
            .disabled(!can(selected.as_ref(), false, cx))
            .tooltip(format!("Run{quoted}"))
            .on_click(dispatch(Run.boxed_clone()));
        let debug = ActionButton::new("debug", "run/debug")
            .size(ActionSize::MainToolbar)
            .colored()
            .disabled(!can(selected.as_ref(), true, cx))
            .tooltip(format!("Debug{quoted}"))
            .on_click(dispatch(Debug.boxed_clone()));
        let stop = running.then(|| {
            ActionButton::new("stop", "run/stop")
                .size(ActionSize::MainToolbar)
                .fill(Tokens::get(cx).toolbar_stop_bg)
                .tooltip(format!("Stop{quoted}"))
                .on_click(dispatch(run::Stop.boxed_clone()))
        });
        // IntelliJ's More Actions (`MoreRunToolbarActions`): the configuration's editing
        let configured = selected.is_some();
        let more = ActionButton::new("run-more", "general/moreVertical")
            .size(ActionSize::MainToolbar)
            .tooltip("More Actions")
            .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| {
                if configured {
                    menu.label("Configuration")
                        .menu("Edit…", run::EditConfigurations.boxed_clone())
                } else {
                    menu.menu(
                        "Run with Parameters…",
                        run::EditConfigurations.boxed_clone(),
                    )
                }
            });
        spork_vcs::spork_look(cx)
            .child(
                h_flex()
                    .gap(px(metrics::RUN_WIDGET_GAP))
                    .child(selector)
                    .child(run)
                    .child(debug)
                    .children(stop)
                    .child(more),
            )
            .into_any_element()
    }
}

/// How many configurations Recent Configurations lists (`max.recent.run.configurations`).
const RECENT_LIMIT: usize = 5;

/// The run widget's popup (`RunToolbarPopup`): Pinned, Recent Configurations with the All
/// Configurations toggle over the configurations by type once there are five or more, each row
/// with its Run and Debug buttons and its submenu, then Current File and Edit Configurations….
fn configurations_popup(
    configurations: &Entity<ProjectConfigurations>,
    state: Entity<gpui_kit::component::popover::PopoverState>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let list = configurations.read(cx).list.clone();
    let selected = configurations.read(cx).selected.clone();
    let history = &configurations.read(cx).history;
    let find = |name: &String| list.iter().find(|c| c.name().as_ref() == name).cloned();
    let pinned: Vec<Configuration> = history.pinned.iter().filter_map(find).collect();
    let recents: Vec<Configuration> = history
        .history
        .iter()
        .filter(|name| !history.pinned.contains(name))
        .filter_map(find)
        .take(RECENT_LIMIT)
        .collect();
    let with_toggle = (!recents.is_empty() || !pinned.is_empty()) && list.len() >= RECENT_LIMIT;
    let all_shown = !with_toggle || history.all_expanded;
    // the row whose submenu shows, the last the pointer was on
    let submenu = window.use_keyed_state("run-configurations-submenu", cx, |_, _| None::<usize>);
    let open = *submenu.read(cx);
    let close = move |window: &mut Window, cx: &mut App| {
        state.update(cx, |state, cx| state.dismiss(window, cx));
    };
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut ix = 0;
    let mut row =
        |configuration: Option<Configuration>, rows: &mut Vec<AnyElement>, cx: &mut App| {
            let on = configuration.as_ref().map(Configuration::name) == selected;
            let pin = configuration
                .as_ref()
                .map(|c| history_pinned(configurations, &c.name(), cx));
            let entry = PopupRow {
                ix,
                configuration,
                selected: on || open == Some(ix),
                pinned: pin,
                submenu: submenu.clone(),
                submenu_open: open == Some(ix),
            };
            rows.push(popup_row(entry, configurations, close.clone(), cx));
            ix += 1;
        };
    if !pinned.is_empty() {
        rows.push(popup::labeled_separator("Pinned", true, cx).into_any_element());
        for configuration in &pinned {
            row(Some(configuration.clone()), &mut rows, cx);
        }
    }
    if with_toggle {
        rows.push(
            popup::labeled_separator("Recent Configurations", pinned.is_empty(), cx)
                .into_any_element(),
        );
        for configuration in &recents {
            row(Some(configuration.clone()), &mut rows, cx);
        }
        rows.push(popup::separator(cx).into_any_element());
        rows.push(all_configurations_toggle(
            configurations,
            list.len(),
            all_shown,
            cx,
        ));
    } else if !pinned.is_empty() {
        rows.push(popup::separator(cx).into_any_element());
    }
    if all_shown {
        let mut last_type: Option<SharedString> = None;
        for configuration in &list {
            let type_name = configuration.type_name();
            if last_type.as_ref().is_some_and(|last| last != &type_name) {
                rows.push(popup::separator(cx).into_any_element());
            }
            last_type = Some(type_name);
            row(Some(configuration.clone()), &mut rows, cx);
        }
    }
    if !list.is_empty() && all_shown {
        rows.push(popup::separator(cx).into_any_element());
    }
    row(None, &mut rows, cx);
    rows.push(popup::separator(cx).into_any_element());
    rows.push(
        popup::row("edit-configurations", false, cx)
            .role(Role::MenuItem)
            .aria_label("Edit Configurations…")
            .child(div().w(px(16.)))
            .child("Edit Configurations…")
            .on_click(move |_, window, cx| {
                close(window, cx);
                window.dispatch_action(run::EditConfigurations.boxed_clone(), cx);
            })
            .into_any_element(),
    );
    popup::surface(cx)
        .id("run-configurations-menu")
        .role(Role::Menu)
        .aria_label("Run / Debug Configurations")
        .min_w(px(240.))
        .max_w(px(500.))
        .py(px(4.))
        .children(rows)
        .into_any_element()
}

fn history_pinned(configurations: &Entity<ProjectConfigurations>, name: &str, cx: &App) -> bool {
    configurations
        .read(cx)
        .history
        .pinned
        .iter()
        .any(|pinned| pinned == name)
}

/// Toggle All Run/Debug Configurations: "All Configurations  N", opening or closing the
/// configurations by type under the recent ones; the popup stays.
fn all_configurations_toggle(
    configurations: &Entity<ProjectConfigurations>,
    count: usize,
    open: bool,
    cx: &mut App,
) -> AnyElement {
    let tokens = Tokens::get(cx);
    let configurations = configurations.clone();
    let chevron = match open {
        true => "general/chevronDown",
        false => "general/chevronRight",
    };
    popup::row("all-configurations", false, cx)
        .role(Role::MenuItem)
        .aria_label(SharedString::from(format!("All Configurations {count}")))
        .child(
            spork_workbench::icon(chevron)
                .size(px(16.))
                .text_color(tokens.icon),
        )
        .child("All Configurations")
        .child(
            div()
                .text_color(tokens.text_secondary)
                .child(count.to_string()),
        )
        .on_click(move |_, _, cx| {
            configurations.update(cx, |configurations, cx| {
                configurations.toggle_all_configurations(cx)
            })
        })
        .into_any_element()
}

/// A row of the popup and where it stands.
struct PopupRow {
    ix: usize,
    /// The configuration, or the current file.
    configuration: Option<Configuration>,
    selected: bool,
    /// Whether the configuration is pinned; the current file can't be.
    pinned: Option<bool>,
    /// The popup's open submenu, which the row opens when the pointer comes onto it.
    submenu: Entity<Option<usize>>,
    submenu_open: bool,
}

/// A row of the popup: a configuration (or the current file), selecting it on a click, its Run
/// and Debug buttons running it at once, its submenu beside it while the pointer is on it.
fn popup_row(
    row: PopupRow,
    configurations: &Entity<ProjectConfigurations>,
    close: impl Fn(&mut Window, &mut App) + Clone + 'static,
    cx: &mut App,
) -> AnyElement {
    let PopupRow {
        ix,
        configuration,
        selected,
        pinned,
        submenu,
        submenu_open,
    } = row;
    let tokens = Tokens::get(cx);
    let name = configuration
        .as_ref()
        .map_or(SharedString::from("Current File"), Configuration::name);
    let icon = configuration.as_ref().map(Configuration::icon);
    let group = SharedString::from(format!("run-configuration-{ix}"));
    let button = |debug: bool| {
        let (verb, icon) = match debug {
            false => ("Run", "run/run"),
            true => ("Debug", "run/debug"),
        };
        let configuration = configuration.clone();
        let close = close.clone();
        let tooltip = match &configuration {
            Some(configuration) => format!("{verb} '{}'", configuration.name()),
            None => format!("{verb} Current File"),
        };
        can(configuration.as_ref(), debug, cx).then(|| {
            ActionButton::new(SharedString::from(format!("{verb}-{ix}")), icon)
                .colored()
                .tooltip(tooltip)
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    close(window, cx);
                    run_in_window(configuration.clone(), debug, window, cx);
                })
        })
    };
    let (run, debug) = (button(false), button(true));
    let submenu_panel = submenu_open.then(|| {
        submenu_items(
            configuration.clone(),
            pinned,
            configurations,
            close.clone(),
            cx,
        )
    });
    let configurations = configurations.clone();
    popup::row(("run-configuration", ix), selected, cx)
        .group(group.clone())
        .relative()
        .role(Role::MenuItem)
        .aria_label(name.clone())
        .child(match icon {
            Some(icon) => spork_workbench::image(icon, cx)
                .size(px(16.))
                .into_any_element(),
            None => div().w(px(16.)).flex_none().into_any_element(),
        })
        .child(div().flex_1().min_w_0().truncate().child(name))
        // the selected row shows its buttons, the others while the pointer is on them
        .child(
            h_flex()
                .flex_none()
                .gap(px(2.))
                .when(!selected, |buttons| buttons.visible_on_hover(group))
                .children(run)
                .children(debug),
        )
        .child(
            spork_workbench::icon("general/chevronRight")
                .size(px(16.))
                .flex_none()
                .text_color(tokens.icon),
        )
        .on_hover(move |hovered, _, cx| {
            if *hovered {
                submenu.update(cx, |open, cx| {
                    if *open != Some(ix) {
                        *open = Some(ix);
                        cx.notify();
                    }
                });
            }
        })
        .on_click({
            let configuration = configuration.clone();
            move |_, window, cx| {
                close(window, cx);
                select(
                    &configurations,
                    configuration.as_ref().map(Configuration::name),
                    cx,
                );
            }
        })
        .children(submenu_panel)
        .into_any_element()
}

/// Runs or debugs a configuration (or the current file) in the window's workspace.
fn run_in_window(
    configuration: Option<Configuration>,
    debug: bool,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(workspace) = Workspace::for_window(window, cx) {
        workspace.update(cx, |workspace, cx| {
            launch(configuration.as_ref(), debug, workspace, window, cx)
        });
    }
}

/// A row's submenu: Run and Debug, then under Configuration Edit… and Delete…, and Pin or Unpin;
/// the current file's Run and Debug, then Run with Parameters….
fn submenu_items(
    configuration: Option<Configuration>,
    pinned: Option<bool>,
    configurations: &Entity<ProjectConfigurations>,
    close: impl Fn(&mut Window, &mut App) + Clone + 'static,
    cx: &mut App,
) -> AnyElement {
    let item = |id: &'static str, label: SharedString, cx: &App| {
        popup::row(id, false, cx)
            .role(Role::MenuItem)
            .aria_label(label.clone())
            .child(label)
    };
    let mut items: Vec<AnyElement> = Vec::new();
    for debug in [false, true] {
        if !can(configuration.as_ref(), debug, cx) {
            continue;
        }
        let (id, label) = match debug {
            false => ("run-configuration-run", "Run"),
            true => ("run-configuration-debug", "Debug"),
        };
        let (configuration, close) = (configuration.clone(), close.clone());
        items.push(
            item(id, label.into(), cx)
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    close(window, cx);
                    run_in_window(configuration.clone(), debug, window, cx);
                })
                .into_any_element(),
        );
    }
    match configuration {
        Some(configuration) => {
            let name = configuration.name();
            items.push(popup::labeled_separator("Configuration", false, cx).into_any_element());
            let (edited, edit_close) = (name.clone(), close.clone());
            items.push(
                item("run-configuration-edit", "Edit…".into(), cx)
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        edit_close(window, cx);
                        if let Some(workspace) = Workspace::for_window(window, cx) {
                            let edited = edited.clone();
                            workspace.update(cx, |workspace, cx| {
                                crate::configurations_dialog::open(
                                    workspace,
                                    Some(edited),
                                    window,
                                    cx,
                                )
                            });
                        }
                    })
                    .into_any_element(),
            );
            let (deleted, delete_close, delete_from) =
                (name.clone(), close.clone(), configurations.clone());
            items.push(
                item("run-configuration-delete", "Delete…".into(), cx)
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        delete_close(window, cx);
                        confirm_delete(delete_from.clone(), deleted.clone(), window, cx);
                    })
                    .into_any_element(),
            );
            let pin_label = match pinned {
                Some(true) => "Unpin",
                _ => "Pin",
            };
            let pinned_in = configurations.clone();
            items.push(
                item("run-configuration-pin", pin_label.into(), cx)
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        close(window, cx);
                        pinned_in.update(cx, |configurations, cx| {
                            configurations.toggle_pin(&name, cx)
                        });
                    })
                    .into_any_element(),
            );
        }
        None => {
            items.push(popup::separator(cx).into_any_element());
            items.push(
                item(
                    "run-configuration-parameters",
                    "Run with Parameters…".into(),
                    cx,
                )
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    close(window, cx);
                    window.dispatch_action(run::EditConfigurations.boxed_clone(), cx);
                })
                .into_any_element(),
            );
        }
    }
    // beside the row, out of the popup's clipping, on its left: the run widget's popup opens at
    // the window's right edge, where IntelliJ's submenu turns to the left
    div()
        .absolute()
        .top_0()
        .left(px(-popup::SELECTION_INSET))
        .child(
            gpui::deferred(
                gpui::anchored().anchor(gpui::Anchor::TopRight).child(
                    popup::surface(cx)
                        .id("run-configuration-submenu")
                        .role(Role::Menu)
                        .aria_label("Configuration")
                        .occlude()
                        .min_w(px(180.))
                        .py(px(4.))
                        .children(items),
                ),
            )
            // over the popup's layer (gpui-kit's `POPUP_PRIORITY`, 100)
            .with_priority(101),
        )
        .into_any_element()
}

/// Delete…: IntelliJ's confirmation (`ChooseRunConfigurationManager.deleteConfiguration`), then
/// the configuration out of its file.
// shortcut: no "Don't ask again" check box; add it with a setting that remembers it
fn confirm_delete(
    configurations: Entity<ProjectConfigurations>,
    name: SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(workspace) = Workspace::for_window(window, cx) else {
        return;
    };
    workspace.update(cx, |workspace, cx| {
        workspace.toggle_modal(window, cx, |window, cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            ConfirmDelete {
                configurations,
                name,
                focus,
            }
        })
    });
}

struct ConfirmDelete {
    configurations: Entity<ProjectConfigurations>,
    name: SharedString,
    focus: FocusHandle,
}

impl ConfirmDelete {
    fn answer(&mut self, yes: bool, window: &mut Window, cx: &mut Context<Self>) {
        if yes {
            let name = self.name.clone();
            let delete = self
                .configurations
                .update(cx, |configurations, cx| configurations.delete(&name, cx));
            window
                .spawn(cx, async move |_| delete.await)
                .detach_and_log_err(cx);
        }
        cx.emit(DismissEvent);
    }
}

impl EventEmitter<DismissEvent> for ConfirmDelete {}

impl ModalView for ConfirmDelete {}

impl Focusable for ConfirmDelete {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ConfirmDelete {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        spork_vcs::spork_look(cx).child(
            popup::surface(cx)
                .id("confirm-delete-configuration")
                .role(Role::Dialog)
                .aria_label("Confirmation")
                .w(px(400.))
                .track_focus(&self.focus)
                .key_context("ConfirmDeleteConfiguration menu")
                .on_action(
                    cx.listener(|this, _: &menu::Confirm, window, cx| {
                        this.answer(true, window, cx)
                    }),
                )
                .on_action(
                    cx.listener(|this, _: &menu::Cancel, window, cx| {
                        this.answer(false, window, cx)
                    }),
                )
                .child(popup::title("Confirmation", cx))
                .child(
                    v_flex()
                        .px(px(16.))
                        .pb(px(14.))
                        .gap(px(12.))
                        .child({
                            let message: SharedString =
                                format!("Are you sure you want to delete {}?", self.name).into();
                            div()
                                .id("confirm-delete-message")
                                .role(Role::Label)
                                .aria_label(message.clone())
                                .child(message)
                        })
                        .child(
                            h_flex()
                                .justify_end()
                                .gap(px(8.))
                                .child(
                                    spork_workbench::Button::new("confirm-delete-no", "No")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.answer(false, window, cx)
                                        })),
                                )
                                .child(
                                    spork_workbench::Button::new("confirm-delete-yes", "Yes")
                                        .primary(true)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.answer(true, window, cx)
                                        })),
                                ),
                        ),
                ),
        )
    }
}

/// Run… and Debug… (`ChooseRunConfigurationPopup`): Edit Configurations… numbered 0, the
/// configurations from 1; a digit picks one, ⇧ switches to the other executor.
pub(crate) struct ChooseConfiguration {
    workspace: gpui::WeakEntity<Workspace>,
    list: Vec<Configuration>,
    debug: bool,
    shift: bool,
    selected: usize,
    focus: FocusHandle,
}

fn choose(
    debug: bool,
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(configurations) = ProjectConfigurations::of(workspace, cx) else {
        return;
    };
    let list = configurations.read(cx).list.clone();
    let handle = workspace.weak_handle();
    workspace.toggle_modal(window, cx, move |window, cx| {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        ChooseConfiguration {
            workspace: handle,
            selected: usize::from(!list.is_empty()),
            list,
            debug,
            shift: false,
            focus,
        }
    });
}

impl ChooseConfiguration {
    fn debugs(&self) -> bool {
        self.debug != self.shift
    }

    /// Runs the row `ix` (0 is Edit Configurations…), closing the popup.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
        let Some(configuration) = ix.checked_sub(1).map(|ix| self.list.get(ix).cloned()) else {
            window.dispatch_action(run::EditConfigurations.boxed_clone(), cx);
            return;
        };
        let debug = self.debugs();
        self.workspace
            .update(cx, |workspace, cx| {
                launch(configuration.as_ref(), debug, workspace, window, cx)
            })
            .log_err();
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        match key {
            "up" => self.selected = self.selected.saturating_sub(1),
            "down" => self.selected = (self.selected + 1).min(self.list.len()),
            "enter" => return self.confirm(self.selected, window, cx),
            "escape" => return cx.emit(DismissEvent),
            digit if digit.len() == 1 && digit.chars().all(|c| c.is_ascii_digit()) => {
                if let Ok(ix) = digit.parse::<usize>()
                    && ix <= self.list.len()
                {
                    self.shift = event.keystroke.modifiers.shift;
                    return self.confirm(ix, window, cx);
                }
            }
            _ => {}
        }
        cx.notify();
    }
}

impl EventEmitter<DismissEvent> for ChooseConfiguration {}

impl ModalView for ChooseConfiguration {}

impl Focusable for ChooseConfiguration {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ChooseConfiguration {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let (title, other) = match self.debugs() {
            false => ("Run", "Debug"),
            true => ("Debug", "Run"),
        };
        let number = |n: usize| {
            div()
                .w(px(14.))
                .flex_none()
                .text_color(tokens.text_secondary)
                .child(n.to_string())
        };
        let row = |ix: usize, label: SharedString, icon: &'static str, cx: &mut Context<Self>| {
            popup::row(("choose-configuration", ix), self.selected == ix, cx)
                .role(Role::MenuItem)
                .aria_label(label.clone())
                .child(number(ix))
                .child(
                    spork_workbench::icon(icon)
                        .size(px(16.))
                        .text_color(tokens.icon),
                )
                .child(label)
                .on_click(cx.listener(move |this, _, window, cx| this.confirm(ix, window, cx)))
        };
        let mut rows =
            vec![row(0, "Edit Configurations…".into(), "general/edit", cx).into_any_element()];
        if !self.list.is_empty() {
            rows.push(popup::separator(cx).into_any_element());
        }
        for (ix, configuration) in self.list.clone().iter().enumerate() {
            rows.push(
                popup::row(
                    ("choose-configuration", ix + 1),
                    self.selected == ix + 1,
                    cx,
                )
                .role(Role::MenuItem)
                .aria_label(configuration.name())
                .child(number(ix + 1))
                .child(spork_workbench::image(configuration.icon(), cx).size(px(16.)))
                .child(configuration.name())
                .on_click(cx.listener(move |this, _, window, cx| this.confirm(ix + 1, window, cx)))
                .into_any_element(),
            );
        }
        spork_vcs::spork_look(cx).child(
            popup::surface(cx)
                .id("choose-configuration")
                .role(Role::Dialog)
                .aria_label(title)
                .track_focus(&self.focus)
                .key_context("ChooseConfiguration")
                .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
                .on_key_down(cx.listener(Self::key_down))
                .on_modifiers_changed(cx.listener(
                    |this, event: &gpui::ModifiersChangedEvent, _, cx| {
                        this.shift = event.modifiers.shift;
                        cx.notify();
                    },
                ))
                .w(px(300.))
                .child(
                    div()
                        .flex_none()
                        .py(px(6.))
                        .text_center()
                        .font_weight(gpui::FontWeight::BOLD)
                        .child(title),
                )
                .child(
                    v_flex()
                        .id("choose-configuration-list")
                        .role(Role::Menu)
                        .aria_label(title)
                        .pb(px(4.))
                        .children(rows),
                )
                .child(
                    div()
                        .id("choose-configuration-hint")
                        .role(Role::Label)
                        .aria_label(SharedString::from(format!("Hold ⇧ to {other}")))
                        .flex_none()
                        .px(px(12.))
                        .py(px(6.))
                        .bg(tokens.panel)
                        .text_color(tokens.text_secondary)
                        .child(format!("Hold ⇧ to {other}")),
                ),
        )
    }
}
