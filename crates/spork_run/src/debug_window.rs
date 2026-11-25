//! The Debug tool window (⌘5) in IntelliJ's layout (`XDebugSessionTab3`) over Zed's debugger: a
//! tab per session; the toolbar (Rerun, Stop | Resume Program, Pause Program, Step Over, Step
//! Into, Step Out | View Breakpoints…, Mute Breakpoints, More) beside the Threads & Variables and
//! Console tabs; Threads & Variables is the thread's frames beside the evaluate field and the
//! variables tree with its watches (Zed's) and the watches' toolbar, Console Zed's debug console.
//! Also the debugger's actions beyond Zed's: Smart Step Into with its chooser, Force Step Over and
//! Force Run to Cursor (the breakpoints skipped until the program stops), Reset Frame (the
//! adapter's restartFrame), each greyed where the adapter can't, as IntelliJ greys them.

use std::rc::Rc;

use collections::HashMap;
use debugger_ui::debugger_panel::{DebugPanel, DebugToolWindow};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, AnyElement, App, AppContext as _, Context, Entity, EntityId, Global,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, TaskExt as _, WeakEntity,
    Window, div, px,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Selectable as _, h_flex, v_flex};
use project::debugger::breakpoint_store::{BreakpointState, SourceBreakpoint};
use project::debugger::session::{SessionEvent, ThreadStatus};
use spork_workbench::{ActionButton, Tokens};
use util::ResultExt as _;
use workspace::Workspace;

use crate::run;

pub(crate) fn init(cx: &mut App) {
    cx.set_global(DebugToolWindow(Rc::new(|_, window, cx| {
        let panel = cx.entity();
        DebugWindow::of(panel, window, cx).into_any_element()
    })));
}

pub(crate) fn register(workspace: &mut Workspace) {
    // greyed without a debug session, and without what the adapter can't do, as IntelliJ's
    workspace.register_action_renderer(|div, workspace, _, cx| {
        let Some(running) = running_state(workspace, cx) else {
            return div;
        };
        let session = running.read(cx).session().read(cx);
        if session.is_terminated() {
            return div;
        }
        let stopped = running.read(cx).thread_status(cx) == Some(ThreadStatus::Stopped);
        let capabilities = session.capabilities();
        let step_in_targets = capabilities
            .supports_step_in_targets_request
            .unwrap_or_default();
        let restart_frame = capabilities.supports_restart_frame.unwrap_or_default();
        div.on_action(
            cx.listener(|workspace, _: &run::ShowExecutionPoint, window, cx| {
                if let Some(running) = running_state(workspace, cx) {
                    running.update(cx, |running, cx| running.show_execution_point(window, cx));
                }
            }),
        )
        .when(stopped, |div| {
            div.on_action(
                cx.listener(|workspace, _: &run::EvaluateExpression, window, cx| {
                    crate::evaluate_dialog::open(workspace, window, cx)
                }),
            )
            .on_action(
                cx.listener(|workspace, _: &run::QuickEvaluateExpression, window, cx| {
                    crate::evaluate_dialog::quick_evaluate(workspace, window, cx)
                }),
            )
            .on_action(cx.listener(|workspace, _: &run::ForceStepOver, _, cx| {
                if let Some(running) = running_state(workspace, cx) {
                    skipping_breakpoints(&running, |running, cx| running.step_over(cx), cx);
                }
            }))
            .on_action(cx.listener(|workspace, _: &run::ForceRunToCursor, _, cx| {
                force_run_to_cursor(workspace, cx)
            }))
        })
        .when(stopped && step_in_targets, |div| {
            div.on_action(
                cx.listener(|workspace, _: &run::SmartStepInto, window, cx| {
                    smart_step_into(workspace, window, cx)
                }),
            )
        })
        .when(stopped && restart_frame, |div| {
            div.on_action(cx.listener(|workspace, _: &run::ResetFrame, _, cx| {
                if let Some(running) = running_state(workspace, cx)
                    && let Some(frame) = running.read(cx).opened_stack_frame(cx)
                {
                    let session = running.read(cx).session().clone();
                    session.update(cx, |session, cx| session.restart_stack_frame(frame, cx));
                }
            }))
        })
    });
}

/// Force Step Over and Force Run to Cursor (`XDebugSessionImpl`'s `forceStepOver`,
/// `forceRunToPosition`): the session skips the breakpoints until the program stops again.
fn skipping_breakpoints(
    running: &Entity<debugger_ui::RunningState>,
    go: impl FnOnce(&mut debugger_ui::RunningState, &mut Context<debugger_ui::RunningState>) + 'static,
    cx: &mut App,
) {
    let session = running.read(cx).session().clone();
    let skipping = session.read(cx).ignore_breakpoints();
    let skip = match skipping {
        true => Task::ready(Default::default()),
        false => session.update(cx, |session, cx| session.toggle_ignore_breakpoints(cx)),
    };
    let running = running.downgrade();
    cx.spawn(async move |cx| {
        skip.await;
        running.update(cx, go)?;
        if skipping {
            return anyhow::Ok(());
        }
        let (stopped, stop) = futures::channel::oneshot::channel();
        let mut stopped = Some(stopped);
        // the sender goes with the subscription when the session does
        let _subscription = cx.update(|cx| {
            cx.subscribe(&session, move |_, event: &SessionEvent, _| {
                if let SessionEvent::Stopped(_) = event
                    && let Some(stopped) = stopped.take()
                {
                    stopped.send(()).ok();
                }
            })
        });
        stop.await.ok();
        session.update(cx, |session, cx| {
            if session.ignore_breakpoints() && !crate::breakpoints::muted(cx) {
                session.toggle_ignore_breakpoints(cx).detach();
            }
        });
        anyhow::Ok(())
    })
    .detach_and_log_err(cx);
}

/// Force Run to Cursor (⌥⌘F9): to the editor's caret line, past the breakpoints on the way.
fn force_run_to_cursor(workspace: &mut Workspace, cx: &mut Context<Workspace>) {
    let Some(running) = running_state(workspace, cx) else {
        return;
    };
    let Some(editor) = workspace.active_item_as::<editor::Editor>(cx) else {
        return;
    };
    let Some((path, row)) = crate::breakpoints::caret_line(&editor, cx) else {
        return;
    };
    let target = SourceBreakpoint {
        row,
        path,
        message: None,
        condition: None,
        hit_condition: None,
        state: BreakpointState::Enabled,
    };
    skipping_breakpoints(
        &running,
        move |running, cx| {
            if let Some(thread) = running.selected_thread_id() {
                running.session().update(cx, |session, cx| {
                    session.run_to_position(target, thread, cx)
                });
            }
        },
        cx,
    );
}

/// Smart Step Into (⇧F7, `XDebuggerSmartStepIntoHandler`): the calls of the line in a chooser,
/// stepping into the one picked; into the only one at once, a plain Step Into without any.
fn smart_step_into(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let Some(running) = running_state(workspace, cx) else {
        return;
    };
    let (Some(frame), Some(thread)) = (
        running.read(cx).opened_stack_frame(cx),
        running.read(cx).selected_thread_id(),
    ) else {
        return;
    };
    let session = running.read(cx).session().clone();
    let targets = session.update(cx, |session, cx| session.step_in_targets(frame, cx));
    cx.spawn_in(window, async move |workspace, cx| {
        let targets = targets.await.unwrap_or_default();
        workspace.update_in(cx, |workspace, window, cx| {
            match targets.as_slice() {
                [] => {
                    window.dispatch_action(debugger_ui::StepInto.boxed_clone(), cx);
                    return;
                }
                [only] => {
                    session.update(cx, |session, cx| {
                        session.step_in_target(thread, only.id, cx)
                    });
                    return;
                }
                _ => {}
            }
            let rows = targets
                .iter()
                .map(|target| crate::list_popup::Row::new(target.label.clone()))
                .collect();
            let ids: Vec<u64> = targets.iter().map(|target| target.id).collect();
            crate::list_popup::ListPopup::show(
                workspace,
                SMART_STEP_INTO_TITLE,
                rows,
                "",
                320.,
                move |ix, _, cx| {
                    if let Some(id) = ids.get(ix) {
                        session.update(cx, |session, cx| session.step_in_target(thread, *id, cx));
                    }
                },
                window,
                cx,
            );
        })
    })
    .detach_and_log_err(cx);
}

/// The title of Smart Step Into's chooser (`XDebuggerBundle`'s `smart.step.into.popup.title`).
const SMART_STEP_INTO_TITLE: &str = "Method to Step Into";

pub(crate) fn running_state(
    workspace: &Workspace,
    cx: &App,
) -> Option<Entity<debugger_ui::RunningState>> {
    let session = workspace
        .panel::<DebugPanel>(cx)?
        .read(cx)
        .active_session()?;
    Some(session.read(cx).running_state().clone())
}

/// Each Debug tool window's view, by its panel.
#[derive(Default)]
struct Views(HashMap<EntityId, Entity<DebugWindow>>);

impl Global for Views {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContentTab {
    ThreadsAndVariables,
    Console,
}

struct DebugWindow {
    panel: WeakEntity<DebugPanel>,
    tab: ContentTab,
    evaluate: Entity<InputState>,
    /// New Watch… focused the evaluate field: ⏎ adds the watch, as IntelliJ's new watch's
    /// editor commits it.
    new_watch: bool,
    /// The last expression evaluated with ⏎ and what it gave, shown over the variables.
    result: Option<(SharedString, Result<SharedString, SharedString>)>,
    hide_library_frames: bool,
    /// The session whose changes redraw the view.
    observed: Option<EntityId>,
    _session: Vec<Subscription>,
    _subscriptions: Vec<Subscription>,
    _evaluating: Task<()>,
}

impl DebugWindow {
    fn of(panel: Entity<DebugPanel>, window: &mut Window, cx: &mut App) -> Entity<Self> {
        let id = panel.entity_id();
        if let Some(view) = cx
            .try_global::<Views>()
            .and_then(|views| views.0.get(&id).cloned())
        {
            return view;
        }
        let view = cx.new(|cx| {
            let evaluate = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Evaluate expression (⏎) or add a watch (⇧⌘⏎)")
            });
            let entered = cx.subscribe_in(
                &evaluate,
                window,
                |this: &mut DebugWindow, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { secondary, shift } => {
                        match (secondary & shift) || this.new_watch {
                            true => this.add_watch(window, cx),
                            false => this.evaluate(cx),
                        }
                    }
                    InputEvent::Blur => this.new_watch = false,
                    _ => {}
                },
            );
            let redraw = cx.observe(&panel, |_, _, cx| cx.notify());
            let release = cx.observe_release(&panel, move |_, _, cx| {
                cx.default_global::<Views>().0.remove(&id);
            });
            Self {
                panel: panel.downgrade(),
                tab: ContentTab::ThreadsAndVariables,
                evaluate,
                new_watch: false,
                result: None,
                hide_library_frames: true,
                observed: None,
                _session: Vec::new(),
                _subscriptions: vec![entered, redraw, release],
                _evaluating: Task::ready(()),
            }
        });
        cx.default_global::<Views>().0.insert(id, view.clone());
        view
    }

    fn running(&self, cx: &App) -> Option<Entity<debugger_ui::RunningState>> {
        let session = self.panel.upgrade()?.read(cx).active_session()?;
        Some(session.read(cx).running_state().clone())
    }

    /// ⏎ in the evaluate field: the expression's value in the selected frame, over the variables.
    fn evaluate(&mut self, cx: &mut Context<Self>) {
        let expression = self.evaluate.read(cx).value().trim().to_owned();
        let Some(running) = self.running(cx).filter(|_| !expression.is_empty()) else {
            return;
        };
        let Some(frame) = running.read(cx).opened_stack_frame(cx) else {
            return;
        };
        let session = running.read(cx).session().clone();
        let evaluation = session.update(cx, |session, cx| {
            session.evaluate_hover_expression(frame, expression.clone(), cx)
        });
        self._evaluating = cx.spawn(async move |this, cx| {
            let response = evaluation.await;
            this.update(cx, |this, cx| {
                let value = match response {
                    Some(response) => Ok(SharedString::from(response.result)),
                    None => Err(SharedString::from("Cannot evaluate")),
                };
                this.result = Some((expression.into(), value));
                cx.notify();
            })
            .log_err();
        });
    }

    /// ⇧⌘⏎: the expression becomes a watch of the variables tree.
    fn add_watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let expression = self.evaluate.read(cx).value().trim().to_owned();
        let Some(running) = self.running(cx).filter(|_| !expression.is_empty()) else {
            return;
        };
        let Some(frame) = running.read(cx).opened_stack_frame(cx) else {
            return;
        };
        let session = running.read(cx).session().clone();
        session
            .update(cx, |session, cx| {
                session.add_watcher(expression.into(), frame, cx)
            })
            .detach_and_log_err(cx);
        self.new_watch = false;
        self.evaluate
            .update(cx, |field, cx| field.set_value("", window, cx));
    }

    /// Redraws on the active session's changes.
    fn observe_session(&mut self, cx: &mut Context<Self>) {
        let Some(running) = self.running(cx) else {
            return;
        };
        let id = running.entity_id();
        if self.observed == Some(id) {
            return;
        }
        let session = running.read(cx).session().clone();
        self.observed = Some(id);
        self.result = None;
        let variables = running.read(cx).variables().clone();
        self._session = vec![
            cx.observe(&running, |_, _, cx| cx.notify()),
            cx.observe(&session, |_, _, cx| cx.notify()),
            // the watches toolbar follows the tree's selection
            cx.observe(&variables, |_, _, cx| cx.notify()),
        ];
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let Some(panel) = self.panel.upgrade() else {
            return div().into_any_element();
        };
        let active = panel.read(cx).active_session().map(|s| s.entity_id());
        let sessions = panel.read(cx).session_items();
        let tabs: Vec<AnyElement> = sessions
            .into_iter()
            .enumerate()
            .map(|(ix, session)| {
                let id = session.entity_id();
                let title = session
                    .update(cx, |session, cx| session.title(cx))
                    .unwrap_or_else(|| "Debug".into());
                let on = active == Some(id);
                let (show, close) = (panel.downgrade(), panel.downgrade());
                let shown = session;
                h_flex()
                    .id(("debug-tab", ix))
                    .role(Role::Tab)
                    .aria_label(title.clone())
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
                    .child(
                        spork_workbench::image("runConfigurations/application", cx).size(px(16.)),
                    )
                    .child(div().max_w(px(260.)).truncate().child(title))
                    .child(
                        ActionButton::new(("close-debug-tab", ix), "general/closeSmall")
                            .tooltip("Close")
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                close
                                    .update(cx, |panel, cx| {
                                        panel.close_session_item(id, window, cx)
                                    })
                                    .log_err();
                            }),
                    )
                    .on_click(move |_, window, cx| {
                        show.update(cx, |panel, cx| {
                            panel.show_session(shown.clone(), window, cx)
                        })
                        .log_err();
                    })
                    .into_any_element()
            })
            .collect();
        let title = h_flex()
            .gap(px(12.))
            .child(workspace::frame::tool_window_title_label("Debug"))
            .child(h_flex().gap(px(2.)).children(tabs))
            .into_any_element();
        workspace::frame::tool_window_header(
            title,
            Vec::new(),
            std::sync::Arc::new(panel),
            "Debug".into(),
            cx,
        )
        .into_any_element()
    }

    fn toolbar(
        &self,
        status: Option<ThreadStatus>,
        muted: bool,
        name: &str,
        capabilities: &dap::Capabilities,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let stopped = status == Some(ThreadStatus::Stopped);
        let step_in_targets = stopped
            && capabilities
                .supports_step_in_targets_request
                .unwrap_or_default();
        let restart_frame = stopped && capabilities.supports_restart_frame.unwrap_or_default();
        let running = status == Some(ThreadStatus::Running);
        let live = status
            .is_some_and(|status| !matches!(status, ThreadStatus::Exited | ThreadStatus::Ended));
        let button = |id: &'static str,
                      icon: &'static str,
                      tooltip: String,
                      enabled: bool,
                      action: Box<dyn Action>| {
            ActionButton::new(id, icon)
                .tooltip(tooltip)
                .disabled(!enabled)
                .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
        };
        let separator = || {
            div()
                .w(px(1.))
                .h(px(20.))
                .mx(px(3.))
                .bg(Tokens::get(cx).separator)
        };
        // `XDebugger.ToolWindow.TopToolbar3.Extra`, without the Java debugger's own actions
        let more = ActionButton::new("debug-more", "general/moreVertical")
            .tooltip("More")
            .dropdown_menu(move |menu, _, _| {
                let item =
                    |label: &'static str, enabled: bool, action: Box<dyn Action>| {
                        PopupMenuItem::new(label).disabled(!enabled).on_click(
                            move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx),
                        )
                    };
                menu.item(item(
                    "Force Step Over",
                    stopped,
                    run::ForceStepOver.boxed_clone(),
                ))
                .item(item(
                    "Force Step Into",
                    stopped,
                    debugger_ui::StepInto.boxed_clone(),
                ))
                .item(item(
                    "Smart Step Into",
                    step_in_targets,
                    run::SmartStepInto.boxed_clone(),
                ))
                .separator()
                .item(item(
                    "Run to Cursor",
                    stopped,
                    editor::actions::RunToCursor.boxed_clone(),
                ))
                .item(item(
                    "Force Run to Cursor",
                    stopped,
                    run::ForceRunToCursor.boxed_clone(),
                ))
                .separator()
                .item(item(
                    "Show Execution Point",
                    live,
                    run::ShowExecutionPoint.boxed_clone(),
                ))
                .separator()
                .item(item(
                    "Evaluate Expression…",
                    stopped,
                    run::EvaluateExpression.boxed_clone(),
                ))
                .item(item(
                    "Reset Frame",
                    restart_frame,
                    run::ResetFrame.boxed_clone(),
                ))
            });
        h_flex()
            .id("debug-toolbar")
            .role(Role::Toolbar)
            .aria_label("XDebugger.ToolWindow.TopToolbar3")
            .flex_none()
            .gap(px(4.))
            .child(button(
                "debug-rerun",
                "run/restartDebug",
                format!("Rerun '{name}' (⌘R)"),
                true,
                debugger_ui::RerunSession.boxed_clone(),
            ))
            .child(button(
                "debug-stop",
                "run/stop",
                format!("Stop '{name}' (⌘F2)"),
                live,
                debugger_ui::Stop.boxed_clone(),
            ))
            .child(separator())
            .child(button(
                "debug-resume",
                "run/resume",
                "Resume Program (⌥⌘R)".into(),
                stopped,
                debugger_ui::Continue.boxed_clone(),
            ))
            .child(button(
                "debug-pause",
                "run/pause",
                "Pause Program".into(),
                running,
                debugger_ui::Pause.boxed_clone(),
            ))
            .child(button(
                "debug-step-over",
                "run/stepOver",
                "Step Over (F8)".into(),
                stopped,
                debugger_ui::StepOver.boxed_clone(),
            ))
            .child(button(
                "debug-step-into",
                "run/stepInto",
                "Step Into (F7)".into(),
                stopped,
                debugger_ui::StepInto.boxed_clone(),
            ))
            .child(button(
                "debug-step-out",
                "run/stepOut",
                "Step Out (⇧F8)".into(),
                stopped,
                debugger_ui::StepOut.boxed_clone(),
            ))
            .child(separator())
            .child(button(
                "debug-view-breakpoints",
                "run/viewBreakpoints",
                "View Breakpoints… (⇧⌘F8)".into(),
                true,
                run::ViewBreakpoints.boxed_clone(),
            ))
            .child(
                ActionButton::new("debug-mute-breakpoints", "run/muteBreakpoints")
                    .tooltip("Mute Breakpoints")
                    .selected(muted)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(run::MuteBreakpoints.boxed_clone(), cx)
                    }),
            )
            .child(more)
            .into_any_element()
    }

    fn content_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let tab =
            |id: &'static str, label: &'static str, which: ContentTab, cx: &mut Context<Self>| {
                let on = self.tab == which;
                div()
                    .id(id)
                    .role(Role::Tab)
                    .aria_label(label)
                    .aria_selected(on)
                    .h(px(28.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(on, |t| t.bg(tokens.tab_selected))
                    .when(!on, |t| t.hover(move |t| t.bg(tokens.hover)))
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.tab = which;
                        // the tab hidden may have had the focus: the tool window keeps it, so the
                        // debugger's keys still reach the workspace
                        if let Some(panel) = this.panel.upgrade() {
                            window.focus(&gpui::Focusable::focus_handle(&panel, cx), cx);
                        }
                        cx.notify();
                    }))
            };
        h_flex()
            .id("debug-content-tabs")
            .role(Role::TabList)
            .aria_label("Debug Content")
            .gap(px(2.))
            .child(tab(
                "debug-threads-variables",
                "Threads & Variables",
                ContentTab::ThreadsAndVariables,
                cx,
            ))
            .child(tab("debug-console", "Console", ContentTab::Console, cx))
            .into_any_element()
    }

    fn frames(
        &mut self,
        running: &Entity<debugger_ui::RunningState>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = Tokens::get(cx);
        let session = running.read(cx).session().clone();
        let thread_id = running.read(cx).selected_thread_id();
        let threads = session.update(cx, |session, cx| session.threads(cx));
        let status = running.read(cx).thread_status(cx);
        let thread = thread_id
            .and_then(|id| threads.iter().find(|(thread, _)| thread.id == id.0))
            .map(|(thread, status)| {
                format!(
                    "\"{}\"@{}: {}",
                    thread.name,
                    thread.id,
                    status.label().to_uppercase()
                )
            })
            .unwrap_or_default();
        let frames = match (thread_id, status) {
            (Some(thread_id), Some(ThreadStatus::Stopped)) => session
                .update(cx, |session, cx| session.stack_frames(thread_id, cx))
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let opened = running.read(cx).opened_stack_frame(cx);
        let hide = self.hide_library_frames;
        let rows: Vec<AnyElement> = frames
            .iter()
            .filter(|frame| !hide || frame.dap.source.as_ref().is_some_and(|s| s.path.is_some()))
            .map(|frame| {
                let frame = &frame.dap;
                let id = frame.id;
                let file = frame
                    .source
                    .as_ref()
                    .and_then(|s| s.name.clone())
                    .unwrap_or_default();
                let label = format!("{}:{}, {}", frame.name, frame.line, file);
                let selected = opened == Some(id);
                let running = running.downgrade();
                h_flex()
                    .id(("debug-frame", id as usize))
                    .role(Role::ListItem)
                    .aria_label(label.clone())
                    .aria_selected(selected)
                    .flex_none()
                    .h(px(24.))
                    .mx(px(6.))
                    .px(px(6.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .when(selected, |row| row.bg(tokens.selection))
                    .when(!selected, |row| {
                        row.hover(move |row| row.bg(tokens.selection_hover))
                    })
                    .child(div().truncate().child(label))
                    .on_click(move |_, window, cx| {
                        running
                            .update(cx, |running, cx| running.open_stack_frame(id, window, cx))
                            .log_err();
                    })
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        v_flex()
            .w(px(280.))
            .flex_none()
            .h_full()
            .border_r_1()
            .border_color(tokens.separator)
            .child(
                h_flex()
                    .flex_none()
                    .h(px(32.))
                    .px(px(8.))
                    .gap(px(4.))
                    .border_b_1()
                    .border_color(tokens.separator)
                    .child(
                        div()
                            .id("debug-thread")
                            .role(Role::ComboBox)
                            .aria_label(SharedString::from(thread.clone()))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(thread),
                    )
                    .child(
                        ActionButton::new("debug-hide-library-frames", "general/filter")
                            .tooltip(match hide {
                                true => "Show All Frames",
                                false => "Hide Frames from Libraries",
                            })
                            .selected(hide)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.hide_library_frames = !this.hide_library_frames;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                v_flex()
                    .id("debug-frames")
                    .role(Role::List)
                    .aria_label("Frames")
                    .flex_1()
                    .py(px(4.))
                    .overflow_y_scroll()
                    .children(rows)
                    .when(empty, |list| {
                        list.child(
                            div()
                                .px(px(12.))
                                .text_color(tokens.text_secondary)
                                .child("Frames are not available"),
                        )
                    }),
            )
            .into_any_element()
    }

    fn variables(
        &self,
        running: &Entity<debugger_ui::RunningState>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = Tokens::get(cx);
        let result = self.result.clone().map(|(expression, value)| {
            let (text, color) = match value {
                Ok(value) => (format!("result = {value}"), tokens.text),
                Err(error) => (format!("{expression}: {error}"), tokens.text_error),
            };
            div()
                .id("debug-evaluation-result")
                .role(Role::ListItem)
                .aria_label(SharedString::from(text.clone()))
                .px(px(12.))
                .h(px(24.))
                .flex()
                .items_center()
                .text_color(color)
                .child(text)
        });
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                div()
                    .flex_none()
                    .px(px(6.))
                    .py(px(4.))
                    .border_b_1()
                    .border_color(tokens.separator)
                    // the field's own keys have no ⇧⌘⏎: it adds the watch before them
                    .capture_key_down(cx.listener(
                        |this, event: &gpui::KeyDownEvent, window, cx| {
                            let keystroke = &event.keystroke;
                            if keystroke.key == "enter"
                                && keystroke.modifiers.shift
                                && keystroke.modifiers.platform
                            {
                                cx.stop_propagation();
                                this.add_watch(window, cx);
                            }
                        },
                    ))
                    .child(
                        spork_workbench::TextField::new(&self.evaluate)
                            .aria_label("Evaluate expression (⏎) or add a watch (⇧⌘⏎)")
                            .suffix(
                                ActionButton::new("debug-add-to-watches", "debugger/addToWatch")
                                    .tooltip("Add to Watches (⇧⌘⏎)")
                                    .disabled(self.evaluate.read(cx).value().trim().is_empty())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.add_watch(window, cx)
                                    })),
                            ),
                    ),
            )
            .children(result)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(self.watches_toolbar(running, cx))
                    .child(
                        div()
                            .id("debug-variables")
                            .role(Role::Group)
                            .aria_label("Variables")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(running.read(cx).variables_view()),
                    ),
            )
            .into_any_element()
    }

    /// The watches' toolbar (`XDebugger.Watches.Tree.Toolbar`), on the tree's left as IntelliJ
    /// puts it beside a vertical layout: New Watch…, Remove Watch, Move Watch Up and Down,
    /// Duplicate Watch, each greyed unless the tree's selection allows it.
    fn watches_toolbar(
        &self,
        running: &Entity<debugger_ui::RunningState>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = Tokens::get(cx);
        let session = running.read(cx).session().clone();
        let selected = running.read(cx).variables().read(cx).selected_value();
        let watches: Vec<SharedString> = session.read(cx).watchers().keys().cloned().collect();
        let (watch, position) = match &selected {
            Some((expression, true)) => (
                Some(expression.clone()),
                watches.iter().position(|w| w == expression),
            ),
            _ => (None, None),
        };
        let variable = match selected {
            Some((expression, false)) => Some(expression),
            _ => None,
        };
        let frame = running.read(cx).opened_stack_frame(cx);
        let moved = |by: isize| {
            let (session, watch) = (session.clone(), watch.clone());
            let to = position.and_then(|at| at.checked_add_signed(by));
            let enabled = to.is_some_and(|to| to < watches.len());
            (
                enabled,
                move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                    if let (Some(watch), Some(to)) = (&watch, to) {
                        session.update(cx, |session, cx| session.move_watcher(watch, to, cx));
                    }
                },
            )
        };
        let (up, move_up) = moved(-1);
        let (down, move_down) = moved(1);
        let remove = {
            let (session, watch) = (session.clone(), watch.clone());
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                if let Some(watch) = &watch {
                    session.update(cx, |session, cx| {
                        session.remove_watcher(watch.clone());
                        cx.emit(SessionEvent::Watchers);
                    });
                }
            }
        };
        // a watch's copy would be the same watch: Zed's watches are one per expression, so
        // Duplicate makes a selected variable a watch, as IntelliJ's does for a value
        let duplicate = {
            let (session, variable) = (session.clone(), variable.clone());
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                if let (Some(variable), Some(frame)) = (&variable, frame) {
                    session
                        .update(cx, |session, cx| {
                            session.add_watcher(variable.clone(), frame, cx)
                        })
                        .detach_and_log_err(cx);
                }
            }
        };
        v_flex()
            .id("debug-watches-toolbar")
            .role(Role::Toolbar)
            .aria_label("XDebugger.Watches.Tree.Toolbar")
            .flex_none()
            .w(px(32.))
            .h_full()
            .pt(px(4.))
            .items_center()
            .gap(px(4.))
            .border_r_1()
            .border_color(tokens.separator)
            .child(
                ActionButton::new("debug-new-watch", "general/add")
                    .tooltip("New Watch…")
                    .on_click(cx.listener(|this, _, window, cx| this.new_watch(window, cx))),
            )
            .child(
                ActionButton::new("debug-remove-watch", "general/remove")
                    .tooltip("Remove Watch (⌫)")
                    .disabled(watch.is_none())
                    .on_click(remove),
            )
            .child(
                ActionButton::new("debug-move-watch-up", "general/moveUp")
                    .tooltip("Move Watch Up (⌥⇧↑)")
                    .disabled(!up)
                    .on_click(move_up),
            )
            .child(
                ActionButton::new("debug-move-watch-down", "general/moveDown")
                    .tooltip("Move Watch Down (⌥⇧↓)")
                    .disabled(!down)
                    .on_click(move_down),
            )
            .child(
                ActionButton::new("debug-duplicate-watch", "general/copy")
                    .tooltip("Duplicate Watch (⌘D)")
                    .disabled(variable.is_none() || frame.is_none())
                    .on_click(duplicate),
            )
            .into_any_element()
    }

    /// New Watch…: the evaluate field takes the watch's expression, ⏎ adding it.
    fn new_watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = ContentTab::ThreadsAndVariables;
        self.evaluate.update(cx, |field, cx| {
            field.set_value("", window, cx);
            field.focus(window, cx);
        });
        self.new_watch = true;
        cx.notify();
    }
}

impl Render for DebugWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        self.observe_session(cx);
        let header = self.header(cx);
        let running = self.running(cx);
        let muted = crate::breakpoints::muted(cx);
        let body = running.map(|running| {
            let status = running.read(cx).thread_status(cx);
            let name = running
                .read(cx)
                .session()
                .read(cx)
                .label()
                .unwrap_or_default();
            let capabilities = running.read(cx).session().read(cx).capabilities().clone();
            let toolbar = self.toolbar(status, muted, &name, &capabilities, cx);
            let tabs = self.content_tabs(cx);
            let content = match self.tab {
                ContentTab::ThreadsAndVariables => h_flex()
                    .size_full()
                    .items_start()
                    .child(self.frames(&running, cx))
                    .child(self.variables(&running, cx))
                    .into_any_element(),
                ContentTab::Console => div()
                    .id("debug-console-content")
                    .role(Role::Group)
                    .aria_label("Console")
                    .size_full()
                    .child(running.read(cx).console_view())
                    .into_any_element(),
            };
            v_flex()
                .flex_1()
                .min_h_0()
                .child(
                    h_flex()
                        .flex_none()
                        .h(px(36.))
                        .px(px(5.))
                        .gap(px(12.))
                        .border_b_1()
                        .border_color(tokens.separator)
                        .child(toolbar)
                        .child(tabs),
                )
                .child(div().flex_1().min_h_0().child(content))
        });
        spork_vcs::spork_look(cx).size_full().child(
            v_flex()
                .size_full()
                .bg(tokens.island)
                .child(header)
                .children(body),
        )
    }
}

/// The active debug session, for the end-to-end tests: its thread's status, its watches with their
/// values and in their order, and whether it skips the breakpoints.
pub(crate) fn e2e_debug(workspace: &Workspace, cx: &App) -> serde_json::Value {
    let Some(running) = running_state(workspace, cx) else {
        return serde_json::Value::Null;
    };
    let running = running.read(cx);
    let session = running.session().read(cx);
    let watches: serde_json::Map<String, serde_json::Value> = session
        .watchers()
        .values()
        .map(|watch| (watch.expression.to_string(), watch.value.to_string().into()))
        .collect();
    serde_json::json!({
        "status": running.thread_status(cx).map(|status| status.label()),
        "terminated": session.is_terminated(),
        "watches": watches,
        "watch_order": session.watchers().keys().map(|w| w.to_string()).collect::<Vec<_>>(),
        "skipping_breakpoints": session.ignore_breakpoints(),
        "label": session.label().map(|label| label.to_string()),
    })
}
