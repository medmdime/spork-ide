//! The Run tool window (⌘4, `RunContentManagerImpl`): a tab per run, named after its
//! configuration, each the process's console (Zed's terminal, so it takes input) under IntelliJ's
//! toolbars: Rerun and Stop on top, Scroll to End and Clear All on the left. The workspace's tasks
//! run here rather than in the Terminal tool window: the panel is the workspace's terminal
//! provider. A console starts with the command line in IntelliJ's system output colour and ends
//! with "Process finished with exit code N"; as IntelliJ's console (`ConsoleViewImpl`), a plain
//! click opens a file:line link, and what the process writes to its stderr shows in the console's
//! error colour.

use std::process::ExitStatus;

use anyhow::Result;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, Task, WeakEntity, Window, div, px,
};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use project::Project;
use serde_json::{Value, json};
use spork_workbench::{ActionButton, Tokens};
use task::{RevealStrategy, SpawnInTerminal};
use terminal::Terminal;
use terminal_view::TerminalView;
use util::ResultExt as _;
use workspace::dock::{DockPosition, Panel, PanelEvent};
use workspace::{TerminalProvider, Workspace};

use crate::configurations::{self, Configuration};
use crate::test_runner::{TabActions, TestRunView};
use crate::{ProjectConfigurations, run};

pub mod run_panel {
    gpui::actions!(
        run_panel,
        [
            /// Opens the Run tool window and focuses it, or gives the focus back to the editor.
            ToggleFocus
        ]
    );
}

pub(crate) fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &run_panel::ToggleFocus, window, cx| {
        workspace.toggle_panel_focus::<RunPanel>(window, cx);
    });
    // Stop is greyed while nothing runs, as IntelliJ's
    workspace.register_action_renderer(|div, workspace, _, cx| match is_running(workspace, cx) {
        true => div.on_action(
            cx.listener(|workspace, _: &run::Stop, window, cx| stop(workspace, window, cx)),
        ),
        false => div,
    });
    workspace.register_action(|workspace, _: &run::Rerun, window, cx| {
        if let Some(panel) = workspace.panel::<RunPanel>(cx) {
            panel.update(cx, |panel, cx| panel.rerun(panel.active, window, cx));
        }
    });
}

/// Adds the Run tool window and makes it the workspace's terminal provider.
pub(crate) fn add(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    if workspace.panel::<RunPanel>(cx).is_some() {
        return;
    }
    let handle = workspace.weak_handle();
    let project = workspace.project().clone();
    let panel = cx.new(|cx| RunPanel {
        workspace: handle,
        project,
        tabs: Vec::new(),
        active: 0,
        focus: cx.focus_handle(),
    });
    workspace.add_panel(panel.clone(), window, cx);
    workspace.set_terminal_provider(Consoles(panel));
}

/// Whether a run or a debug session is going on, for the run widget's Stop.
pub(crate) fn is_running(workspace: &Workspace, cx: &App) -> bool {
    runs_running(workspace, cx) || debug_session_running(workspace, cx)
}

fn runs_running(workspace: &Workspace, cx: &App) -> bool {
    workspace
        .panel::<RunPanel>(cx)
        .is_some_and(|panel| panel.read(cx).tabs.iter().any(|tab| tab.running(cx)))
}

fn debug_session_running(workspace: &Workspace, cx: &App) -> bool {
    workspace
        .project()
        .read(cx)
        .dap_store()
        .read(cx)
        .sessions()
        .any(|session| !session.read(cx).is_terminated())
}

/// Stop (⌘F2): the debug session when only it runs or the Debug tool window has the focus, the
/// Run tool window's selected run otherwise.
fn stop(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let debug_focused = workspace
        .panel::<debugger_ui::debugger_panel::DebugPanel>(cx)
        .is_some_and(|panel| panel.focus_handle(cx).contains_focused(window, cx));
    if debug_session_running(workspace, cx) && (debug_focused || !runs_running(workspace, cx)) {
        window.dispatch_action(debugger_ui::Stop.boxed_clone(), cx);
        return;
    }
    if let Some(panel) = workspace.panel::<RunPanel>(cx) {
        panel.update(cx, |panel, cx| panel.stop(panel.active, cx));
    }
}

/// A run: its console and what it ran.
struct RunTab {
    title: SharedString,
    icon: &'static str,
    /// The task as Zed scheduled it, to run it again.
    task: SpawnInTerminal,
    terminal: Entity<Terminal>,
    view: Entity<TerminalView>,
    pinned: bool,
    /// How the process ended: its exit code, `None` while it runs.
    ended: Option<Option<i32>>,
    /// A test run's tree, toolbar and consoles, in place of the console alone.
    tests: Option<Entity<TestRunView>>,
}

impl RunTab {
    fn running(&self, cx: &App) -> bool {
        self.terminal
            .read(cx)
            .task()
            .is_some_and(|task| task.status == terminal::TaskStatus::Running)
    }
}

pub struct RunPanel {
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    tabs: Vec<RunTab>,
    active: usize,
    focus: FocusHandle,
}

/// The panel as the workspace's terminal provider: Zed's tasks run in its consoles.
struct Consoles(Entity<RunPanel>);

impl TerminalProvider for Consoles {
    fn spawn(
        &self,
        task: SpawnInTerminal,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<Option<Result<ExitStatus>>> {
        let panel = self.0.clone();
        window.spawn(cx, async move |cx| {
            let task = panel
                .update_in(cx, |panel, window, cx| {
                    panel.from_configuration(task, window, cx)
                })
                .ok()?
                .await;
            let task = match task {
                Ok(task) => task,
                Err(error) => return Some(Err(error)),
            };
            let started = panel
                .update_in(cx, |panel, window, cx| panel.start(task, window, cx))
                .ok()?
                .await;
            let terminal = match started {
                Ok(terminal) => terminal,
                Err(error) => return Some(Err(error)),
            };
            // the one wait for the process: its channel gives the status to one waiter only
            let status = terminal
                .read_with(cx, |terminal, cx| terminal.wait_for_completed_task(cx))
                .await;
            panel.update(cx, |panel, cx| panel.finished(&terminal, status, cx));
            status.map(Ok)
        })
    }
}

/// Text in IntelliJ's console system output colour (the command line, the exit message).
pub(crate) fn system_text(text: &str, cx: &App) -> Vec<u8> {
    format!(
        "{}{text}\x1b[0m",
        foreground(Tokens::get(cx).console_system)
    )
    .into_bytes()
}

/// The escape sequence that writes in `color`.
pub(crate) fn foreground(color: gpui::Hsla) -> String {
    let color = color.to_rgb();
    let [r, g, b] = [color.r, color.g, color.b].map(|c| (c * 255.).round() as u8);
    format!("\x1b[38;2;{r};{g};{b}m")
}

/// IntelliJ runs a process on pipes and shows its stderr in the console's error colour; the
/// console is a terminal, one stream, so a POSIX shell runs the command with its stderr on a pipe
/// through a loop that writes each line back in that colour, the command's exit status kept.
// shortcut: a line of stderr shows once it ends, and a shell loop is slow on a flood of it; a
// console of its own reading two pipes would show it as it comes
fn stderr_in_color(spawn: &mut SpawnInTerminal, is_windows: bool, color: gpui::Hsla) {
    let shell = task::ShellKind::new(spawn.command.as_deref().unwrap_or_default(), is_windows);
    if shell != task::ShellKind::Posix {
        return;
    }
    let Some(command) = spawn.args.last_mut() else {
        return;
    };
    let color = foreground(color).replace('\x1b', "\\033");
    *command = format!(
        "exec 3>&1\n\
         __spork_status=$( ( ( (\n{command}\n) 2>&1 1>&3 3>&- 4>&-; echo $? >&4 ) | \
         while IFS= read -r __spork_line || [ -n \"$__spork_line\" ]; do \
         printf '{color}%s\\033[0m\\n' \"$__spork_line\"; done >&2 3>&- 4>&- ) 4>&1 )\n\
         exit \"${{__spork_status:-1}}\""
    );
}

/// IntelliJ's message for how the process ended (`ProcessTerminatedListener`).
pub(crate) fn exit_message(status: Option<ExitStatus>) -> String {
    #[cfg(unix)]
    let signal = status.and_then(|status| std::os::unix::process::ExitStatusExt::signal(&status));
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    match (status.and_then(|status| status.code()), signal) {
        (Some(code), _) => format!("Process finished with exit code {code}"),
        (None, Some(signal)) => {
            let name = match signal {
                1 => "SIGHUP",
                2 => "SIGINT",
                9 => "SIGKILL",
                15 => "SIGTERM",
                _ => "",
            };
            format!(
                "Process finished with exit code {} (interrupted by signal {signal}:{name})",
                128 + signal
            )
        }
        (None, None) => "Process finished".to_owned(),
    }
}

impl RunPanel {
    /// A task that runs a configuration (the gutter's, [`configurations::CONFIGURATION_ENV`]):
    /// the configuration made temporary and selected, then its runner's task; another task as
    /// it is.
    fn from_configuration(
        &mut self,
        task: SpawnInTerminal,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<SpawnInTerminal>> {
        let Some(entry) = task.env.get(configurations::CONFIGURATION_ENV) else {
            return Task::ready(Ok(task));
        };
        let configuration = match serde_json::from_str::<serde_json::Map<String, Value>>(entry) {
            Ok(entry) => Configuration {
                adapter: entry
                    .get("adapter")
                    .and_then(Value::as_str)
                    .map(|adapter| adapter.to_owned().into()),
                shared: false,
                entry,
            },
            Err(error) => return Task::ready(Err(error.into())),
        };
        let (Some(workspace), Some(runner)) = (
            self.workspace.upgrade(),
            configurations::runner(&configuration, cx),
        ) else {
            return Task::ready(Err(anyhow::anyhow!(
                "Unknown run configuration type {}",
                configuration.type_name()
            )));
        };
        if let Some(configurations) = ProjectConfigurations::of(workspace.read(cx), cx) {
            let name = configuration.name();
            configurations.update(cx, |configurations, cx| {
                configurations.run_temporary(configuration.clone(), cx);
                configurations.started(&name, cx);
            });
        }
        let template = workspace.update(cx, |workspace, cx| {
            runner.task(&configuration, workspace, cx)
        });
        cx.background_spawn(async move {
            let template = template.await?;
            let resolved = template
                .resolve_task("run configuration", &task::TaskContext::default())
                .ok_or_else(|| anyhow::anyhow!("The run configuration has no command"))?;
            Ok(resolved.resolved)
        })
    }

    /// Runs a task in a console: in the unpinned tab of its last run (stopping that run unless
    /// it may run beside it), in a new tab otherwise.
    fn start(
        &mut self,
        mut task: SpawnInTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Terminal>>> {
        // a test run's tree listens for its runner's events
        let tests = match task.env.contains_key(crate::test_runner::TEST_RUN) {
            true => match self.test_view(&task, cx) {
                Ok((view, port)) => {
                    task.env
                        .insert(crate::test_runner::TEST_PORT.into(), port.to_string());
                    Some(view)
                }
                Err(error) => return Task::ready(Err(error)),
            },
            false => None,
        };
        let is_windows = self.project.read(cx).path_style(cx).is_windows();
        let mut spawn =
            terminal_view::terminal_panel::prepare_task_for_spawn(&task, &task.shell, is_windows);
        // the console shows IntelliJ's command line and exit message instead of Zed's
        spawn.show_summary = false;
        spawn.show_command = false;
        spawn.hide = task::HideStrategy::Never;
        let error = Tokens::get(cx).console_error;
        stderr_in_color(&mut spawn, is_windows, error);
        let reuse = self
            .tabs
            .iter()
            .position(|tab| tab.task.label == task.label && !tab.pinned)
            .filter(|ix| !(task.allow_concurrent_runs && self.tabs[*ix].running(cx)));
        if let Some(ix) = reuse {
            self.stop(ix, cx);
        }
        let command_line = system_text(&format!("{}\r\n", spawn.command_label), cx);
        let create = self
            .project
            .update(cx, |project, cx| project.create_terminal_task(spawn, cx));
        cx.spawn_in(window, async move |this, cx| {
            let terminal = create.await?;
            this.update_in(cx, |this, window, cx| {
                terminal.update(cx, |terminal, cx| {
                    terminal.links_without_modifier = true;
                    terminal.write_output(&command_line, cx)
                });
                let (reveal, label) = (task.reveal, task.label.clone());
                this.add_tab(task, terminal.clone(), reuse, tests, window, cx);
                this.reveal(reveal, label, window, cx);
            })?;
            Ok(terminal)
        })
    }

    /// A test run's view, listening on a port its runner gets.
    fn test_view(
        &mut self,
        task: &SpawnInTerminal,
        cx: &mut Context<Self>,
    ) -> Result<(Entity<TestRunView>, u16)> {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let listener = smol::net::TcpListener::try_from(listener)?;
        let panel = cx.entity().downgrade();
        let name: SharedString = task.label.clone().into();
        let workspace = self.workspace.clone();
        let view = cx.new(|cx: &mut Context<TestRunView>| {
            let view = cx.entity().downgrade();
            // the tab of this view, found again each time: tabs close and move
            let tab = move |panel: &RunPanel| {
                let view = view.upgrade()?;
                panel
                    .tabs
                    .iter()
                    .position(|tab| tab.tests.as_ref() == Some(&view))
            };
            let (rerun_panel, rerun_tab) = (panel.clone(), tab.clone());
            let (failed_panel, failed_tab) = (panel.clone(), tab.clone());
            let (stop_panel, stop_tab) = (panel, tab);
            let actions = TabActions {
                rerun: Box::new(move |window, cx| {
                    rerun_panel
                        .update(cx, |panel, cx| {
                            if let Some(ix) = rerun_tab(panel) {
                                panel.rerun(ix, window, cx);
                            }
                        })
                        .log_err();
                }),
                rerun_failed: Box::new(move |tests, window, cx| {
                    failed_panel
                        .update(cx, |panel, cx| {
                            if let Some(ix) = failed_tab(panel) {
                                panel.rerun_tests(ix, Some(tests), window, cx);
                            }
                        })
                        .log_err();
                }),
                stop: Box::new(move |_, cx| {
                    stop_panel
                        .update(cx, |panel, cx| {
                            if let Some(ix) = stop_tab(panel) {
                                panel.stop(ix, cx);
                            }
                        })
                        .log_err();
                }),
            };
            TestRunView::new(name, workspace, actions, listener, cx)
        });
        Ok((view, port))
    }

    fn add_tab(
        &mut self,
        task: SpawnInTerminal,
        terminal: Entity<Terminal>,
        reuse: Option<usize>,
        tests: Option<Entity<TestRunView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.new(|cx| {
            TerminalView::new(
                terminal.clone(),
                self.workspace.clone(),
                None,
                self.project.downgrade(),
                window,
                cx,
            )
        });
        let icon = self.icon_for(&task.label, cx);
        let title = match reuse {
            Some(ix) => self.tabs.get(ix).map(|tab| tab.title.clone()),
            None => None,
        }
        .unwrap_or_else(|| self.unique_title(&task.label));
        if let Some(tests) = &tests {
            tests.update(cx, |tests, _| tests.set_console(view.clone()));
        }
        let tab = RunTab {
            title,
            icon,
            task,
            terminal,
            view,
            pinned: false,
            ended: None,
            tests,
        };
        match reuse.filter(|ix| *ix < self.tabs.len()) {
            Some(ix) => {
                self.tabs[ix] = tab;
                self.active = ix;
            }
            None => {
                self.tabs.push(tab);
                self.active = self.tabs.len() - 1;
            }
        }
        self.changed(cx);
    }

    /// IntelliJ names a second tab of one configuration "Main (1)".
    fn unique_title(&self, label: &str) -> SharedString {
        let taken = |title: &str| self.tabs.iter().any(|tab| tab.title.as_ref() == title);
        if !taken(label) {
            return label.to_owned().into();
        }
        (1..)
            .map(|n| format!("{label} ({n})"))
            .find(|title| !taken(title))
            .unwrap_or_default()
            .into()
    }

    /// The icon of the configuration a task comes from, a shell script's otherwise.
    fn icon_for(&self, label: &str, cx: &App) -> &'static str {
        self.workspace
            .upgrade()
            .and_then(|workspace| ProjectConfigurations::of(workspace.read(cx), cx))
            .and_then(|configurations| {
                configurations
                    .read(cx)
                    .list
                    .iter()
                    .find(|c| c.name().as_ref() == label)
                    .map(|c| c.icon())
            })
            .unwrap_or(crate::configurations::type_icon(None))
    }

    fn reveal(
        &mut self,
        reveal: RevealStrategy,
        label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        // IntelliJ shows the Run tool window, or Services when it lists the configuration, and
        // leaves the focus in the editor; the workspace opens it once this panel's update is over
        if reveal != RevealStrategy::Never {
            window.defer(cx, move |window, cx| {
                workspace.update(cx, |workspace, cx| {
                    if !crate::services::reveal(workspace, &label, window, cx) {
                        workspace.open_panel::<RunPanel>(window, cx)
                    }
                });
            });
        }
    }

    /// The process ended: the console says how, the run widget stops showing Stop.
    fn finished(
        &mut self,
        terminal: &Entity<Terminal>,
        status: Option<ExitStatus>,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| &tab.terminal == terminal) {
            tab.ended = Some(status.and_then(|status| status.code()));
            if let Some(tests) = &tab.tests {
                tests.update(cx, |tests, cx| tests.finished(cx));
            }
        }
        let mut message = system_text(&format!("\r\n{}\r\n", exit_message(status)), cx);
        // a finished console has no caret, as IntelliJ's
        message.extend_from_slice(b"\x1b[?25l");
        terminal.update(cx, |terminal, cx| terminal.write_output(&message, cx));
        self.changed(cx);
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        // the run widget's Stop and Run ▸ Stop follow the runs
        if let Some(workspace) = self.workspace.upgrade() {
            if let Some(configurations) = ProjectConfigurations::of(workspace.read(cx), cx) {
                configurations.update(cx, |_, cx| cx.notify());
            }
            cx.defer(move |cx| workspace.update(cx, |_, cx| cx.notify()));
        }
        cx.notify();
    }

    pub(crate) fn stop(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(ix) {
            tab.terminal
                .update(cx, |terminal, _| terminal.kill_active_task());
        }
        cx.notify();
    }

    /// Rerun: the tab's task again, in the tab; a test run's configuration again (built
    /// first, as IntelliJ's), the failed tests alone for Rerun Failed Tests.
    pub(crate) fn rerun_tests(
        &mut self,
        ix: usize,
        tests: Option<Vec<String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab) = self.tabs.get(ix).filter(|tab| tab.tests.is_some()) else {
            return false;
        };
        let Some(workspace) = self.workspace.upgrade() else {
            return false;
        };
        let label = tab.task.label.clone();
        let Some(mut configuration) = ProjectConfigurations::of(workspace.read(cx), cx)
            .and_then(|c| c.read(cx).list.iter().find(|c| c.name() == label).cloned())
        else {
            return false;
        };
        if let Some(tests) = tests {
            configuration.entry.insert(
                configurations::RERUN_TESTS.into(),
                Value::Array(tests.into_iter().map(Value::from).collect()),
            );
        }
        if let Some(tab) = self.tabs.get_mut(ix) {
            tab.pinned = false;
        }
        // the workspace runs it once this panel's update is over
        window.defer(cx, move |window, cx| {
            workspace.update(cx, |workspace, cx| {
                crate::run_widget::launch(Some(&configuration), false, workspace, window, cx)
            })
        });
        true
    }

    /// Rerun: the tab's task again, in the tab.
    pub(crate) fn rerun(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.rerun_tests(ix, None, window, cx) {
            return;
        }
        let Some(tab) = self.tabs.get(ix) else {
            return;
        };
        let task = tab.task.clone();
        if let Some(tab) = self.tabs.get_mut(ix) {
            // run again in this tab even when pinned
            tab.pinned = false;
        }
        self.workspace
            .update(cx, |workspace, cx| {
                workspace.spawn_in_terminal(task, window, cx).detach();
            })
            .log_err();
    }

    fn close(&mut self, keep: impl Fn(usize, &RunTab) -> bool, cx: &mut Context<Self>) {
        let mut ix = 0;
        self.tabs.retain(|tab| {
            let kept = keep(ix, tab);
            if !kept {
                tab.terminal
                    .update(cx, |terminal, _| terminal.kill_active_task());
            }
            ix += 1;
            kept
        });
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        self.changed(cx);
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let panel = std::sync::Arc::new(cx.entity());
        let this = cx.entity().downgrade();
        let tabs = self.tabs.iter().enumerate().map(|(ix, tab)| {
            let on = ix == self.active;
            let pinned = tab.pinned;
            let this = this.clone();
            h_flex()
                .id(("run-tab", ix))
                .role(Role::Tab)
                .aria_label(tab.title.clone())
                .aria_selected(on)
                .h(px(28.))
                .px(px(8.))
                .gap(px(4.))
                .rounded(px(6.))
                .border_1()
                .cursor_pointer()
                .map(|t| match on {
                    true => t
                        .bg(tokens.tab_selected)
                        .border_color(tokens.tab_selected_border),
                    false => t
                        .border_color(gpui::transparent_black())
                        .hover(move |t| t.bg(tokens.hover)),
                })
                .child(spork_workbench::image(tab.icon, cx).size(px(16.)))
                .child(div().max_w(px(260.)).truncate().child(tab.title.clone()))
                .when(pinned, |t| {
                    t.child(spork_workbench::icon("general/pin").size(px(12.)))
                })
                .child(
                    ActionButton::new(("close-run-tab", ix), "general/closeSmall")
                        .tooltip("Close")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.close(|other, _| other != ix, cx)
                        })),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.active = ix;
                    cx.notify();
                }))
                .context_menu(move |menu, _, _| {
                    let item = |label: &'static str, action: fn(&mut RunPanel, usize, &mut Context<RunPanel>)| {
                        let this = this.clone();
                        PopupMenuItem::new(label).on_click(move |_, _, cx| {
                            this.update(cx, |this, cx| action(this, ix, cx)).log_err();
                        })
                    };
                    menu.item(item("Close Tab", |this, ix, cx| {
                        this.close(|other, _| other != ix, cx)
                    }))
                    .item(item("Close All Tabs", |this, _, cx| this.close(|_, _| false, cx)))
                    .item(item("Close Other Tabs", |this, ix, cx| {
                        this.close(|other, _| other == ix, cx)
                    }))
                    .separator()
                    .item(item(
                        match pinned {
                            true => "Unpin Tab",
                            false => "Pin Tab",
                        },
                        |this, ix, cx| {
                            if let Some(tab) = this.tabs.get_mut(ix) {
                                tab.pinned = !tab.pinned;
                            }
                            cx.notify();
                        },
                    ))
                })
        });
        let title = h_flex()
            .gap(px(12.))
            .child(workspace::frame::tool_window_title_label("Run"))
            .child(h_flex().gap(px(2.)).children(tabs))
            .into_any_element();
        workspace::frame::tool_window_header(title, Vec::new(), panel, "Run".into(), cx)
            .into_any_element()
    }

    fn top_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.tabs.get(self.active);
        let name = tab.map(|tab| tab.title.clone()).unwrap_or_default();
        let running = tab.is_some_and(|tab| tab.running(cx));
        let ix = self.active;
        h_flex()
            .id("run-top-toolbar")
            .role(Role::Toolbar)
            .aria_label("RunTab.TopToolbar")
            .flex_none()
            .h(px(32.))
            .px(px(5.))
            .gap(px(4.))
            .child(
                ActionButton::new("run-rerun", "run/rerun")
                    .tooltip(format!("Rerun '{name}' (⌘R)"))
                    .disabled(tab.is_none())
                    .on_click(cx.listener(move |this, _, window, cx| this.rerun(ix, window, cx))),
            )
            .child(
                ActionButton::new("run-stop", "run/stop")
                    .tooltip(format!("Stop '{name}' (⌘F2)"))
                    .disabled(!running)
                    .on_click(cx.listener(move |this, _, _, cx| this.stop(ix, cx))),
            )
    }

    fn left_toolbar(&self) -> impl IntoElement {
        let terminal = self.tabs.get(self.active).map(|tab| tab.terminal.clone());
        let missing = terminal.is_none();
        let button = |id: &'static str, icon: &'static str, tooltip: &'static str| {
            ActionButton::new(id, icon)
                .tooltip(tooltip)
                .disabled(missing)
        };
        let scroll = terminal.clone();
        let clear = terminal;
        // shortcut: no Up/Down the Stack Trace, Soft-Wrap or Print…: the console is a terminal,
        // which always wraps and has no stack trace navigation; add them with a console of its own
        v_flex()
            .id("run-left-toolbar")
            .role(Role::Toolbar)
            .aria_label("Console Toolbar")
            .flex_none()
            .w(px(32.))
            .pt(px(4.))
            .items_center()
            .gap(px(4.))
            .child(
                button("run-scroll-to-end", "general/scrollDown", "Scroll to End").on_click(
                    move |_, window, cx| {
                        if let Some(terminal) = &scroll {
                            terminal.update(cx, |terminal, cx| {
                                terminal.scroll_to_bottom();
                                terminal.sync(window, cx);
                                cx.notify();
                            });
                        }
                    },
                ),
            )
            .child(
                button("run-clear-all", "general/delete", "Clear All").on_click(
                    move |_, window, cx| {
                        if let Some(terminal) = &clear {
                            // at once, not at the console's next frame
                            terminal.update(cx, |terminal, cx| {
                                terminal.clear();
                                terminal.sync(window, cx);
                                cx.notify();
                            });
                        }
                    },
                ),
            )
    }
}

impl Render for RunPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let header = self.header(cx);
        let console = self.tabs.get(self.active).map(|tab| tab.view.clone());
        let tests = self.tabs.get(self.active).and_then(|tab| tab.tests.clone());
        spork_vcs::spork_look(cx).size_full().child(
            v_flex()
                .size_full()
                .bg(tokens.island)
                .track_focus(&self.focus)
                .key_context("RunPanel")
                .on_action(cx.listener(|this, _: &run::Rerun, window, cx| {
                    this.rerun(this.active, window, cx)
                }))
                .on_action(cx.listener(|this, _: &run::SendEof, _, cx| {
                    if let Some(tab) = this.tabs.get(this.active) {
                        tab.terminal
                            .update(cx, |terminal, _| terminal.input(&b"\x04"[..]));
                    }
                }))
                .child(header)
                .when_some(tests.clone(), |this, tests| {
                    this.child(div().flex_1().min_h_0().child(tests))
                })
                .when(console.is_some() && tests.is_none(), |this| {
                    this.child(self.top_toolbar(cx))
                })
                .when(tests.is_none(), |this| {
                    this.child(
                        h_flex()
                            .flex_1()
                            .min_h_0()
                            .items_start()
                            .when(console.is_some(), |this| this.child(self.left_toolbar()))
                            .child(
                                div()
                                    .id("run-console")
                                    .role(Role::Group)
                                    .aria_label("Console")
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .children(console),
                            ),
                    )
                }),
        )
    }
}

impl Focusable for RunPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.tabs.get(self.active) {
            Some(tab) => tab.view.focus_handle(cx),
            None => self.focus.clone(),
        }
    }
}

impl EventEmitter<PanelEvent> for RunPanel {}

impl Panel for RunPanel {
    fn persistent_name() -> &'static str {
        "Run"
    }

    fn panel_key() -> &'static str {
        "RunToolWindow"
    }

    fn position(&self, _: &Window, _: &App) -> DockPosition {
        DockPosition::Bottom
    }

    // shortcut: the tool window stays where IntelliJ puts it, as the Find tool window does, until
    // the stripes let tool windows move
    fn position_is_valid(&self, position: DockPosition) -> bool {
        position == DockPosition::Bottom
    }

    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}

    fn default_size(&self, _: &Window, _: &App) -> Pixels {
        px(300.)
    }

    fn icon(&self, _: &Window, _: &App) -> Option<ui::IconName> {
        Some(ui::IconName::PlayFilled)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some("Run")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(run_panel::ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        10
    }
}

/// A configuration's run, for the Services tool window and the running list.
pub(crate) struct Run {
    pub title: SharedString,
    /// The configuration's name (a second run's title is "Main (1)").
    pub label: SharedString,
    pub view: Entity<TerminalView>,
    pub terminal: Entity<Terminal>,
    pub running: bool,
    /// Whether it ended with an exit code other than 0; `None` while it runs.
    pub failed: Option<bool>,
}

/// The runs of the Run tool window, its tabs in order.
pub(crate) fn runs(workspace: &Workspace, cx: &App) -> Vec<Run> {
    let Some(panel) = workspace.panel::<RunPanel>(cx) else {
        return Vec::new();
    };
    panel
        .read(cx)
        .tabs
        .iter()
        .map(|tab| Run {
            title: tab.title.clone(),
            label: tab.task.label.clone().into(),
            view: tab.view.clone(),
            terminal: tab.terminal.clone(),
            running: tab.running(cx),
            failed: tab.ended.map(|code| code != Some(0)),
        })
        .collect()
}

/// The latest run of the configuration `name`.
pub(crate) fn run_of(workspace: &Workspace, name: &str, cx: &App) -> Option<Run> {
    let panel = workspace.panel::<RunPanel>(cx)?;
    let labels: Vec<bool> = panel
        .read(cx)
        .tabs
        .iter()
        .map(|tab| tab.task.label == name)
        .collect();
    let ix = labels.iter().rposition(|of| *of)?;
    runs(workspace, cx).into_iter().nth(ix)
}

/// Stops the runs of the configuration `name`.
pub(crate) fn stop_runs_of(workspace: &Entity<Workspace>, name: &str, cx: &mut App) {
    if let Some(panel) = workspace.read(cx).panel::<RunPanel>(cx) {
        panel.update(cx, |panel, cx| {
            let of: Vec<usize> = (0..panel.tabs.len())
                .filter(|ix| panel.tabs[*ix].task.label == name)
                .collect();
            for ix in of {
                panel.stop(ix, cx);
            }
        });
    }
}

/// Shows the run titled `title` in the Run tool window: the running list's navigation.
pub(crate) fn show_run(
    workspace: &mut Workspace,
    title: &str,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(panel) = workspace.panel::<RunPanel>(cx) else {
        return;
    };
    panel.update(cx, |panel, cx| {
        if let Some(ix) = panel
            .tabs
            .iter()
            .position(|tab| tab.title.as_ref() == title)
        {
            panel.active = ix;
            cx.notify();
        }
    });
    workspace.open_panel::<RunPanel>(window, cx);
}

/// The lines a console shows, each with the colour its text starts in, and where they are.
pub(crate) fn shown_lines(terminal: &Terminal) -> Vec<(String, Option<terminal::Rgb>)> {
    let content = terminal.last_content();
    let mut lines: Vec<(String, Option<terminal::Rgb>)> =
        vec![(String::new(), None); content.screen_lines];
    for cell in &content.cells {
        let row = cell.point.line + content.display_offset as i32;
        let Some((text, color)) = usize::try_from(row).ok().and_then(|row| lines.get_mut(row))
        else {
            continue;
        };
        if color.is_none() && !cell.character().is_whitespace() {
            *color = match cell.foreground() {
                terminal::Color::Spec(rgb) => Some(rgb),
                _ => Some(terminal::Rgb { r: 0, g: 0, b: 0 }),
            };
        }
        text.push(cell.character());
    }
    lines
}

/// Where `text` shows last in the active run console (the command line may show it too), in
/// window coordinates, for the end-to-end tests to click it.
pub fn e2e_console_point(workspace: &Workspace, text: &str, cx: &App) -> Option<(f32, f32)> {
    let panel = workspace.panel::<RunPanel>(cx)?;
    let panel = panel.read(cx);
    let terminal = panel.tabs.get(panel.active)?.terminal.read(cx);
    let bounds = terminal.last_content().terminal_bounds;
    shown_lines(terminal)
        .iter()
        .enumerate()
        .rev()
        .find_map(|(row, (line, _))| {
            let column = line.find(text)?;
            let column = line[..column].chars().count() as f32 + text.chars().count() as f32 / 2.;
            Some((
                f32::from(bounds.bounds.origin.x + bounds.cell_width * column),
                f32::from(bounds.bounds.origin.y + bounds.line_height * (row as f32 + 0.5)),
            ))
        })
}

/// The runs, for the end-to-end tests: each tab's title, whether it runs, its console's text, its
/// output after the command line and the lines in the console's error colour.
pub(crate) fn e2e_tabs(workspace: &Workspace, cx: &App) -> Value {
    let Some(panel) = workspace.panel::<RunPanel>(cx) else {
        return json!([]);
    };
    let panel = panel.read(cx);
    let error = Tokens::get(cx).console_error.to_rgb();
    let [r, g, b] = [error.r, error.g, error.b].map(|c| (c * 255.).round() as u8);
    let error = terminal::Rgb { r, g, b };
    panel
        .tabs
        .iter()
        .enumerate()
        .map(|(ix, tab)| {
            let text = tab.terminal.read(cx).get_content();
            let last_line = text.lines().rev().find(|line| !line.trim().is_empty());
            json!({
                "title": tab.title,
                "active": ix == panel.active,
                "running": tab.running(cx),
                "pinned": tab.pinned,
                "text": text.trim_end(),
                "last_line": last_line.map(str::trim_end),
                // what the program wrote: the console after its command line
                "output": text.split_once('\n').map_or("", |(_, output)| output.trim()),
                "errors": shown_lines(tab.terminal.read(cx))
                    .into_iter()
                    .filter(|(_, color)| *color == Some(error))
                    .map(|(line, _)| line.trim_end().to_owned())
                    .collect::<Vec<_>>(),
                "tests": tab.tests.as_ref().map(|tests| tests.read(cx).e2e_state()),
            })
        })
        .collect()
}

#[cfg(all(test, unix))]
mod tests {
    use task::SpawnInTerminal;

    use super::stderr_in_color;

    #[test]
    #[allow(clippy::disallowed_methods, reason = "a test")]
    fn stderr_shows_in_color_and_the_exit_status_stays() {
        let mut spawn = SpawnInTerminal {
            command: Some("/bin/sh".into()),
            args: vec!["-c".into(), "echo out; echo err >&2; exit 3".into()],
            ..SpawnInTerminal::default()
        };
        stderr_in_color(&mut spawn, false, gpui::rgb(0xF75464).into());
        let ran = std::process::Command::new("/bin/sh")
            .args(&spawn.args)
            .output()
            .expect("sh runs");
        assert_eq!(ran.status.code(), Some(3));
        assert_eq!(String::from_utf8_lossy(&ran.stdout), "out\n");
        assert_eq!(
            String::from_utf8_lossy(&ran.stderr),
            "\x1b[38;2;247;84;100merr\x1b[0m\n"
        );
    }
}
