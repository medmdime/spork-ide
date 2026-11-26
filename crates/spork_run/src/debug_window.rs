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
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Selectable as _, h_flex, v_flex};
use project::debugger::breakpoint_store::{BreakpointState, SourceBreakpoint};
use project::debugger::session::{Session, SessionEvent, ThreadId, ThreadStatus};
use spork_workbench::{ActionButton, Tokens};
use terminal::Terminal;
use util::ResultExt as _;
use workspace::Workspace;

use crate::run;
use crate::threads::{DumpThread, DumpView, ThreadInfo, ThreadsView, jvm_frame_label};

pub(crate) fn init(cx: &mut App) {
    cx.set_global(DebugToolWindow(Rc::new(|_, window, cx| {
        let panel = cx.entity();
        DebugWindow::of(panel, window, cx).into_any_element()
    })));
    cx.set_global(debugger_ui::ProgramTerminalStarted(Rc::new(
        program_console,
    )));
    cx.set_global(editor::LineNumberClick(Rc::new(run_to_line)));
}

/// A click on a line number while the program is suspended runs it to that line (IntelliJ's
/// "Click line number to perform run to cursor"); false otherwise, the click selecting the line.
fn run_to_line(
    editor: &mut editor::Editor,
    row: editor::display_map::DisplayRow,
    window: &mut Window,
    cx: &mut Context<editor::Editor>,
) -> bool {
    if !spork_settings::debugger::settings(cx).click_line_number_runs_to_cursor {
        return false;
    }
    let Some(workspace) = editor.workspace() else {
        return false;
    };
    let Some(running) = running_state(workspace.read(cx), cx) else {
        return false;
    };
    let running = running.read(cx);
    let (Some(thread), Some(ThreadStatus::Stopped)) =
        (running.selected_thread_id(), running.thread_status(cx))
    else {
        return false;
    };
    let session = running.session().clone();
    let snapshot = editor.snapshot(window, cx);
    let point = editor::display_map::DisplayPoint::new(row, 0).to_point(&snapshot.display_snapshot);
    let Some((buffer, point)) = editor.buffer().read(cx).point_to_buffer_point(point, cx) else {
        return false;
    };
    let Some(path) =
        project::debugger::breakpoint_store::BreakpointStore::abs_path_from_buffer(&buffer, cx)
    else {
        return false;
    };
    let target = SourceBreakpoint {
        row: point.row,
        path,
        message: None,
        condition: None,
        hit_condition: None,
        state: BreakpointState::Enabled,
        properties: Default::default(),
    };
    session.update(cx, |session, cx| {
        session.run_to_position(target, thread, cx)
    });
    true
}

/// The consoles of the programs debugged in terminals, by their terminal, kept while their
/// program runs: the session's entity and the terminal.
#[derive(Default)]
struct ProgramConsoles(HashMap<EntityId, (EntityId, WeakEntity<Terminal>, Subscription, Task<()>)>);

impl Global for ProgramConsoles {}

/// A program debugged in a terminal of its own has IntelliJ's console there: its command line,
/// "Connected to the target VM, address: '…', transport: 'socket'" when the debugger reaches it
/// over JDWP, its output and input, the debugger's output (a logging breakpoint's text) as it
/// comes, then "Disconnected from the target VM…" and "Process finished with exit code N".
fn program_console(session: &Entity<Session>, terminal: &Entity<Terminal>, cx: &mut App) {
    let spawned = terminal
        .read(cx)
        .task()
        .map(|task| task.spawned_task.clone());
    let Some(spawned) = spawned else {
        return;
    };
    let words: Vec<String> = spawned
        .command
        .iter()
        .chain(spawned.args.iter())
        .map(|word| match word.contains(char::is_whitespace) {
            true => format!("\"{word}\""),
            false => word.clone(),
        })
        .collect();
    // `DebugProcessImpl`'s address text, `'host:port', transport: 'socket'`
    let address = spawned
        .args
        .iter()
        .find_map(|arg| arg.strip_prefix("-agentlib:jdwp="))
        .and_then(|options| {
            options
                .split(',')
                .find_map(|option| option.strip_prefix("address="))
        })
        .map(|address| format!("'{address}', transport: 'socket'"));
    let mut text = crate::run_panel::system_text(&format!("{}\r\n", words.join(" ")), cx);
    if let Some(address) = &address {
        text.extend(crate::run_panel::system_text(
            &format!("Connected to the target VM, address: {address}\r\n"),
            cx,
        ));
    }
    terminal.update(cx, |terminal, cx| terminal.write_output(&text, cx));
    let mut seen = project::debugger::session::OutputToken(0);
    let output = {
        let terminal = terminal.downgrade();
        // the session notifies as output comes
        cx.observe(session, move |session, cx| {
            let error = Tokens::get(cx).console_error;
            let (events, token) = session.read(cx).output(seen);
            let text: Vec<u8> = events
                .filter(|event| {
                    !matches!(event.category, Some(dap::OutputEventCategory::Telemetry))
                })
                .flat_map(|event| {
                    let line = event.output.replace('\n', "\r\n");
                    match event.category {
                        Some(dap::OutputEventCategory::Stderr) => {
                            format!("{}{line}\x1b[0m", crate::run_panel::foreground(error))
                                .into_bytes()
                        }
                        _ => line.into_bytes(),
                    }
                })
                .collect();
            if token == seen {
                return;
            }
            seen = token;
            terminal
                .update(cx, |terminal, cx| terminal.write_output(&text, cx))
                .ok();
        })
    };
    let ended = terminal.read(cx).wait_for_completed_task(cx);
    let id = terminal.entity_id();
    let ending = {
        let terminal = terminal.downgrade();
        cx.spawn(async move |cx| {
            let status = ended.await;
            cx.update(|cx| {
                let mut message = String::new();
                if let Some(address) = &address {
                    message.push_str(&format!(
                        "Disconnected from the target VM, address: {address}\r\n"
                    ));
                }
                message.push_str(&format!(
                    "\r\n{}\r\n",
                    crate::run_panel::exit_message(status)
                ));
                let mut text = crate::run_panel::system_text(&message, cx);
                // a finished console has no caret, as IntelliJ's
                text.extend_from_slice(b"\x1b[?25l");
                terminal
                    .update(cx, |terminal, cx| terminal.write_output(&text, cx))
                    .ok();
                cx.default_global::<ProgramConsoles>().0.remove(&id);
            });
        })
    };
    let entry = (session.entity_id(), terminal.downgrade(), output, ending);
    cx.default_global::<ProgramConsoles>().0.insert(id, entry);
}

/// Writes the debugger's own text (a logging breakpoint's) in the console of a session's program,
/// before the program goes on; false when the program has no console of its own.
pub fn write_program_console(session: &Entity<Session>, text: &str, cx: &mut App) -> bool {
    let terminal = cx.try_global::<ProgramConsoles>().and_then(|consoles| {
        consoles
            .0
            .values()
            .find(|(id, ..)| *id == session.entity_id())
            .and_then(|(_, terminal, ..)| terminal.upgrade())
    });
    let Some(terminal) = terminal else {
        return false;
    };
    let text = text.replace('\n', "\r\n");
    terminal.update(cx, |terminal, cx| {
        terminal.write_output(text.as_bytes(), cx)
    });
    true
}

/// A language's answer about a stopped session's threads, evaluated in a frame.
pub type ThreadsTask<T> = fn(&Entity<Session>, u64, &mut App) -> Task<anyhow::Result<Vec<T>>>;

/// What a debug adapter's language adds to the Debug tool window (Java's comes from
/// `spork_java`).
#[derive(Clone, Copy)]
pub struct AdapterHooks {
    /// IntelliJ's label of a frame as the threads view shows it, `hello:9, Greeting
    /// (com.example)`.
    pub frame_label: fn(&dap::StackFrame, &ThreadsView) -> String,
    /// Force Step Into: a step into past the stepping filters.
    pub force_step_into: fn(&Entity<Session>, ThreadId, &mut App),
    /// The threads' groups and states (Show thread groups).
    pub thread_infos: Option<ThreadsTask<ThreadInfo>>,
    /// Get Thread Dump's and Export Threads…' threads.
    pub thread_dump: Option<ThreadsTask<DumpThread>>,
    /// The class of a frame, for Add Stepping Filter….
    pub frame_class: Option<fn(&dap::StackFrame) -> Option<String>>,
}

/// The adapters' hooks, by adapter name.
#[derive(Default)]
struct Hooks(HashMap<SharedString, AdapterHooks>);

impl Global for Hooks {}

pub fn register_adapter_hooks(adapter: &'static str, hooks: AdapterHooks, cx: &mut App) {
    cx.default_global::<Hooks>().0.insert(adapter.into(), hooks);
}

fn hooks(session: &Session, cx: &App) -> Option<AdapterHooks> {
    cx.try_global::<Hooks>()?
        .0
        .get(session.adapter().0.as_ref())
        .copied()
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
        let thread_dump = hooks(session, cx).is_some_and(|hooks| hooks.thread_dump.is_some());
        div.on_action(
            cx.listener(|workspace, _: &run::ShowExecutionPoint, window, cx| {
                if let Some(running) = running_state(workspace, cx) {
                    running.update(cx, |running, cx| running.show_execution_point(window, cx));
                }
            }),
        )
        .when(stopped && thread_dump, |div| {
            div.on_action(
                cx.listener(|workspace, _: &run::GetThreadDump, window, cx| {
                    let view = workspace.panel::<DebugPanel>(cx).and_then(|panel| {
                        cx.try_global::<Views>()?.0.get(&panel.entity_id()).cloned()
                    });
                    if let Some(view) = view {
                        view.update(cx, |view, cx| view.get_thread_dump(window, cx));
                    }
                }),
            )
        })
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
            .on_action(cx.listener(|workspace, _: &run::Inspect, window, cx| {
                crate::evaluate_dialog::inspect(workspace, window, cx)
            }))
            .on_action(cx.listener(|workspace, _: &run::ForceStepOver, _, cx| {
                if let Some(running) = running_state(workspace, cx) {
                    skipping_breakpoints(&running, |running, cx| running.step_over(cx), cx);
                }
            }))
            .on_action(cx.listener(|workspace, _: &run::ForceRunToCursor, _, cx| {
                force_run_to_cursor(workspace, cx)
            }))
            .on_action(
                cx.listener(|workspace, _: &run::ForceStepInto, window, cx| {
                    let Some(running) = running_state(workspace, cx) else {
                        return;
                    };
                    let session = running.read(cx).session().clone();
                    let force = hooks(session.read(cx), cx).map(|hooks| hooks.force_step_into);
                    match (force, running.read(cx).selected_thread_id()) {
                        (Some(force), Some(thread)) => force(&session, thread, cx),
                        _ => window.dispatch_action(debugger_ui::StepInto.boxed_clone(), cx),
                    }
                }),
            )
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
        properties: Default::default(),
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
    // the frame's line, as the execution line shows it (counted from 1, as the adapter does)
    let line = workspace
        .project()
        .read(cx)
        .breakpoint_store()
        .read(cx)
        .active_position()
        .and_then(|position| {
            let buffer = workspace
                .project()
                .read(cx)
                .find_project_path(&*position.path, cx)
                .and_then(|path| workspace.project().read(cx).get_open_buffer(&path, cx))?;
            let snapshot = buffer.read(cx).snapshot();
            Some(u64::from(text::ToPoint::to_point(&position.position, &snapshot).row) + 1)
        });
    let targets = session.update(cx, |session, cx| session.step_in_targets(frame, cx));
    cx.spawn_in(window, async move |workspace, cx| {
        let mut targets = targets.await.unwrap_or_default();
        // IntelliJ's targets: the calls of the frame's line (java-debug adds the loop's condition),
        // in the order they run, an argument's call before the call it is in
        targets.retain(|target| line.is_none() || target.line.is_none() || target.line == line);
        targets.sort_by_key(|target| (target.end_line.or(target.line), target.end_column));
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
            // IntelliJ's in-place chooser, its popup where a call has no place on the line
            if crate::smart_step_into::SmartStepChooser::show(
                workspace,
                session.clone(),
                thread,
                &targets,
                window,
                cx,
            ) {
                return;
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

/// The debug session the Debug tool window shows.
pub fn active_debug_session(workspace: &Workspace, cx: &App) -> Option<Entity<Session>> {
    Some(running_state(workspace, cx)?.read(cx).session().clone())
}

pub fn running_state(workspace: &Workspace, cx: &App) -> Option<Entity<debugger_ui::RunningState>> {
    let session = workspace
        .panel::<DebugPanel>(cx)?
        .read(cx)
        .active_session()?;
    Some(session.read(cx).running_state().clone())
}

/// The thread chooser's and the frames list's rows.
struct FrameRows {
    /// The selected thread, `"main"@1: STOPPED`.
    thread: String,
    threads: Vec<String>,
    thread_ids: Vec<ThreadId>,
    /// Each row's frame, or the number of library frames it folds.
    shown: Vec<Result<dap::StackFrame, usize>>,
    labels: Vec<String>,
}

fn frame_rows(
    running: &Entity<debugger_ui::RunningState>,
    hide_library_frames: bool,
    infos: &HashMap<String, ThreadInfo>,
    cx: &mut App,
) -> FrameRows {
    let view = crate::threads::threads_view(cx);
    let session = running.read(cx).session().clone();
    let thread_id = running.read(cx).selected_thread_id();
    let mut threads = session.update(cx, |session, cx| session.threads(cx));
    // Move current thread to the top
    if view.current_thread_on_top
        && let Some(at) = threads
            .iter()
            .position(|(thread, _)| Some(ThreadId(thread.id)) == thread_id)
    {
        let current = threads.remove(at);
        threads.insert(0, current);
    }
    let status = running.read(cx).thread_status(cx);
    // `JavaDebuggerBundle`'s `label.thread.node` and, with the thread groups,
    // `label.thread.node.in.group`; the Kotlin server names a thread so itself, with its state:
    // `"main"@1 in group "main": RUNNING`
    let thread_label = |(thread, status): &(dap::Thread, ThreadStatus)| {
        if thread.name.starts_with('"') {
            return thread.name.clone();
        }
        match infos.get(&thread.name).filter(|_| view.show_thread_groups) {
            Some(info) => format!(
                "\"{}\"@{} in group \"{}\": {}",
                thread.name,
                thread.id,
                info.group,
                crate::threads::thread_status(&info.state)
            ),
            None => format!(
                "\"{}\"@{}: {}",
                thread.name,
                thread.id,
                status.label().to_uppercase()
            ),
        }
    };
    let thread = thread_id
        .and_then(|id| threads.iter().find(|(thread, _)| thread.id == id.0))
        .map(thread_label)
        .unwrap_or_default();
    let frames = match (thread_id, status) {
        (Some(thread_id), Some(ThreadStatus::Stopped)) => session
            .update(cx, |session, cx| session.stack_frames(thread_id, cx))
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let label = hooks(session.read(cx), cx).map(|hooks| hooks.frame_label);
    let root = session
        .read(cx)
        .worktree()
        .map(|worktree| worktree.read(cx).abs_path());
    // a library's frame: its source isn't the project's (Java's are `jdt://` classes)
    let in_project = |frame: &dap::StackFrame| {
        let path = frame.source.as_ref().and_then(|s| s.path.as_deref());
        match (path, &root) {
            (Some(path), Some(root)) => std::path::Path::new(path).starts_with(root),
            _ => false,
        }
    };
    // IntelliJ folds each run of hidden library frames into one row; the top frame shows, and
    // the separators between a stack and its async stack trace
    let mut shown: Vec<Result<dap::StackFrame, usize>> = Vec::new();
    // shortcut: a synthetic method is known by the `$` in its name (`lambda$1`, `access$000`);
    // ask the adapter once one tells
    let synthetic = |frame: &dap::StackFrame| {
        let method = frame.name.split('(').next().unwrap_or_default();
        method
            .rsplit('.')
            .next()
            .is_some_and(|method| method.contains('$'))
    };
    let frames = frames
        .into_iter()
        .map(|frame| frame.dap)
        .filter(|frame| view.show_synthetic_frames || !synthetic(frame));
    for (ix, frame) in frames.enumerate() {
        match (
            hide_library_frames && ix > 0 && !in_project(&frame) && !is_separator(&frame),
            shown.last_mut(),
        ) {
            (true, Some(Err(hidden))) => *hidden += 1,
            (true, _) => shown.push(Err(1)),
            (false, _) => shown.push(Ok(frame)),
        }
    }
    let labels = shown
        .iter()
        .map(|row| match row {
            Ok(frame) if is_separator(frame) => frame.name.clone(),
            Ok(frame) => match label {
                Some(label) => label(frame, &view),
                // the Kotlin server's stack trace names
                None => match jvm_frame(&frame.name) {
                    Some((method, class, package)) => {
                        let source = frame.source.as_ref().and_then(|s| s.name.as_deref());
                        jvm_frame_label(
                            method,
                            None,
                            frame.line as i64,
                            class,
                            package,
                            source,
                            &view,
                        )
                    }
                    None => {
                        let file = frame.source.as_ref().and_then(|s| s.name.clone());
                        let file = file.unwrap_or_default();
                        format!("{}:{}, {file}", frame.name, frame.line)
                    }
                },
            },
            // `XDebuggerBundle`'s `label.folded.frames`
            Err(1) => "1 hidden frame".to_owned(),
            Err(hidden) => format!("{hidden} hidden frames"),
        })
        .collect();
    FrameRows {
        thread,
        thread_ids: threads
            .iter()
            .map(|(thread, _)| ThreadId(thread.id))
            .collect(),
        threads: threads.iter().map(thread_label).collect(),
        shown,
        labels,
    }
}

/// A frame that only separates a stack from the async stack trace under it (the Kotlin language
/// server's "Async stack trace" between a coroutine's frames and the frames that started it).
fn is_separator(frame: &dap::StackFrame) -> bool {
    frame.presentation_hint == Some(dap::StackFramePresentationHint::Label)
}

/// The active session's thread, threads and frames as the Debug tool window lists them, for the
/// end-to-end tests (read from the session, not the window, which macOS may not draw).
pub fn e2e_frames(workspace: &Entity<Workspace>, cx: &mut App) -> serde_json::Value {
    let Some(running) = running_state(workspace.read(cx), cx) else {
        return serde_json::Value::Null;
    };
    let hide = workspace
        .read(cx)
        .panel::<DebugPanel>(cx)
        .and_then(|panel| cx.try_global::<Views>()?.0.get(&panel.entity_id()).cloned())
        .is_none_or(|view| view.read(cx).hide_library_frames);
    let infos = workspace
        .read(cx)
        .panel::<DebugPanel>(cx)
        .and_then(|panel| cx.try_global::<Views>()?.0.get(&panel.entity_id()).cloned())
        .map(|view| view.read(cx).thread_infos.clone())
        .unwrap_or_default();
    let rows = frame_rows(&running, hide, &infos, cx);
    serde_json::json!({
        "thread": rows.thread,
        "threads": rows.threads,
        "frames": rows.labels,
    })
}

/// Each Debug tool window's view, by its panel.
#[derive(Default)]
struct Views(HashMap<EntityId, Entity<DebugWindow>>);

impl Global for Views {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContentTab {
    ThreadsAndVariables,
    Console,
    /// A thread dump's tab, by its place among the dumps.
    Dump(usize),
}

/// How the Threads & Variables tab shows the threads and frames (`XDebugFramesAndThreadsLayoutOptions`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum FramesLayout {
    /// The thread chooser over the frames.
    Combined,
    /// The threads as a tree, the current one open on its frames.
    Threads,
    /// The threads beside the frames.
    SideBySide,
    /// The frames alone.
    FramesOnly,
    /// Don't Show.
    Hidden,
}

/// The Debug tool window's Layout Settings, IntelliJ's by default; kept for the next sessions as
/// IntelliJ keeps a runner's layout.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct DebugLayout {
    frames: FramesLayout,
    variables: bool,
    console: bool,
    separate_watches: bool,
    tab_labels: bool,
}

impl Default for DebugLayout {
    fn default() -> Self {
        Self {
            frames: FramesLayout::Combined,
            variables: true,
            console: true,
            separate_watches: false,
            tab_labels: true,
        }
    }
}

struct DebugLayoutGlobal(DebugLayout);

impl Global for DebugLayoutGlobal {}

const DEBUG_LAYOUT_KEY: &str = "spork_debug_layout";

fn debug_layout(cx: &mut App) -> DebugLayout {
    if let Some(layout) = cx.try_global::<DebugLayoutGlobal>() {
        return layout.0;
    }
    let layout: DebugLayout = db::kvp::KeyValueStore::global(cx)
        .read_kvp(DEBUG_LAYOUT_KEY)
        .log_err()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).log_err())
        .unwrap_or_default();
    cx.set_global(DebugLayoutGlobal(layout));
    cx.set_global(debugger_ui::SeparateWatches(layout.separate_watches));
    layout
}

fn set_debug_layout(layout: DebugLayout, cx: &mut App) {
    let separate = cx
        .try_global::<debugger_ui::SeparateWatches>()
        .is_some_and(|separate| separate.0);
    cx.set_global(DebugLayoutGlobal(layout));
    if separate != layout.separate_watches {
        cx.set_global(debugger_ui::SeparateWatches(layout.separate_watches));
        // the variables trees take the watches in or out
        let views: Vec<_> = cx
            .try_global::<Views>()
            .map(|views| views.0.values().cloned().collect())
            .unwrap_or_default();
        for view in views {
            if let Some(running) = view.read(cx).running(cx) {
                let variables = running.read(cx).variables().clone();
                variables.update(cx, |variables, cx| variables.rebuild(cx));
            }
        }
    }
    let json = serde_json::to_string(&layout);
    let store = db::kvp::KeyValueStore::global(cx);
    cx.background_spawn(async move { store.write_kvp(DEBUG_LAYOUT_KEY.into(), json?).await })
        .detach_and_log_err(cx);
    cx.refresh_windows();
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
    /// The watch selected in Separate Watches' view.
    selected_watch: Option<SharedString>,
    /// The thread dumps' tabs.
    dumps: Vec<Entity<DumpView>>,
    /// The stopped threads' groups and states, by name, while Show thread groups wants them;
    /// `None` until asked at this stop.
    thread_infos: HashMap<String, ThreadInfo>,
    infos_asked: bool,
    _thread_infos: Task<()>,
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
                selected_watch: None,
                dumps: Vec::new(),
                thread_infos: HashMap::default(),
                infos_asked: false,
                _thread_infos: Task::ready(()),
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
        self.thread_infos.clear();
        self.infos_asked = false;
        self._session = vec![
            cx.observe(&running, |_, _, cx| cx.notify()),
            cx.observe(&session, |_, _, cx| cx.notify()),
            // a stop's threads are asked about again
            cx.subscribe(&session, |this, _, event: &SessionEvent, cx| {
                if let SessionEvent::Stopped(_) = event {
                    this.thread_infos.clear();
                    this.infos_asked = false;
                    cx.notify();
                }
            }),
            // the watches toolbar follows the tree's selection
            cx.observe(&variables, |_, _, cx| cx.notify()),
        ];
    }

    /// Show thread groups: the stopped threads' groups and states, asked once per stop of the
    /// language that tells them.
    fn ask_thread_infos(
        &mut self,
        running: &Entity<debugger_ui::RunningState>,
        cx: &mut Context<Self>,
    ) {
        if self.infos_asked || !crate::threads::threads_view(cx).show_thread_groups {
            return;
        }
        let session = running.read(cx).session().clone();
        let (Some(ask), Some(frame), Some(ThreadStatus::Stopped)) = (
            hooks(session.read(cx), cx).and_then(|hooks| hooks.thread_infos),
            running.read(cx).opened_stack_frame(cx),
            running.read(cx).thread_status(cx),
        ) else {
            return;
        };
        self.infos_asked = true;
        let infos = ask(&session, frame, cx);
        self._thread_infos = cx.spawn(async move |this, cx| {
            let Some(infos) = infos.await.log_err() else {
                return;
            };
            this.update(cx, |this, cx| {
                this.thread_infos = infos
                    .into_iter()
                    .map(|info| (info.name.clone(), info))
                    .collect();
                cx.notify();
            })
            .log_err();
        });
    }

    /// Get Thread Dump: the threads in a new "Dump HH:MM:SS" tab, shown.
    fn get_thread_dump(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(running) = self.running(cx) else {
            return;
        };
        let session = running.read(cx).session().clone();
        let (Some(dump), Some(frame)) = (
            hooks(session.read(cx), cx).and_then(|hooks| hooks.thread_dump),
            running.read(cx).opened_stack_frame(cx),
        ) else {
            return;
        };
        let folder = session
            .read(cx)
            .worktree()
            .map(|worktree| worktree.read(cx).abs_path().to_path_buf());
        let threads = dump(&session, frame, cx);
        cx.spawn_in(window, async move |this, cx| {
            let threads = threads.await?;
            this.update_in(cx, |this, window, cx| {
                let view = cx.new(|cx| DumpView::new(threads, folder, window, cx));
                this.dumps.push(view);
                this.tab = ContentTab::Dump(this.dumps.len() - 1);
                cx.notify();
            })
        })
        .detach_and_log_err(cx);
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
        update: Option<ActionButton>,
        dumps: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let stopped = status == Some(ThreadStatus::Stopped);
        let step_in_targets = stopped
            && capabilities
                .supports_step_in_targets_request
                .unwrap_or_default();
        let restart_frame = stopped && capabilities.supports_restart_frame.unwrap_or_default();
        let thread_dump = stopped && dumps;
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
                    run::ForceStepInto.boxed_clone(),
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
                .item(item(
                    "Get Thread Dump",
                    thread_dump,
                    run::GetThreadDump.boxed_clone(),
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
            .children(update.filter(|_| live))
            .child(
                button(
                    "debug-stop",
                    "run/stop",
                    format!("Stop '{name}' (⌘F2)"),
                    live,
                    debugger_ui::Stop.boxed_clone(),
                )
                // a debugged test's run ends with its session
                .on_click(|_, window, cx| {
                    if let Some(terminal) =
                        Workspace::for_window(window, cx).and_then(|workspace| {
                            crate::run_panel::debugged_run(workspace.read(cx), cx)
                        })
                    {
                        terminal.update(cx, |terminal, _| terminal.kill_active_task());
                    }
                    window.dispatch_action(debugger_ui::Stop.boxed_clone(), cx)
                }),
            )
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
        let layout = debug_layout(cx);
        // Show Tab Labels off: a tab with an icon shows it alone (`RunnerContentUi`)
        let tab =
            |id: &'static str, label: &'static str, which: ContentTab, cx: &mut Context<Self>| {
                let on = self.tab == which;
                let icon = match which {
                    ContentTab::Console => Some("fileTypes/shell"),
                    _ => None,
                };
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
                    .map(|t| match icon.filter(|_| !layout.tab_labels) {
                        Some(icon) => t
                            .child(spork_workbench::image(icon, cx).size(px(16.)))
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(label).build(window, cx)
                            }),
                        None => t.child(label),
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.tab = which;
                        // the program's console takes the input, as IntelliJ's; otherwise the
                        // tab hidden may have had the focus: the tool window keeps it, so the
                        // debugger's keys still reach the workspace
                        let terminal = this
                            .running(cx)
                            .and_then(|running| running.read(cx).program_terminal(cx))
                            .filter(|_| which == ContentTab::Console);
                        match (terminal, this.panel.upgrade()) {
                            (Some(terminal), _) => {
                                window.focus(&gpui::Focusable::focus_handle(&terminal, cx), cx)
                            }
                            (None, Some(panel)) => {
                                window.focus(&gpui::Focusable::focus_handle(&panel, cx), cx)
                            }
                            (None, None) => {}
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
            .when(layout.console, |tabs| {
                tabs.child(tab("debug-console", "Console", ContentTab::Console, cx))
            })
            .children(self.dumps.iter().enumerate().map(|(ix, dump)| {
                let title = dump.read(cx).title.clone();
                let on = self.tab == ContentTab::Dump(ix);
                h_flex()
                    .id(("debug-dump-tab", ix))
                    .role(Role::Tab)
                    .aria_label(title.clone())
                    .aria_selected(on)
                    .h(px(28.))
                    .pl(px(10.))
                    .pr(px(4.))
                    .gap(px(2.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(on, |t| t.bg(tokens.tab_selected))
                    .when(!on, |t| t.hover(move |t| t.bg(tokens.hover)))
                    .child(title)
                    .child(
                        ActionButton::new(("close-dump-tab", ix), "general/closeSmall")
                            .tooltip("Close")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.dumps.remove(ix);
                                this.tab = ContentTab::ThreadsAndVariables;
                                cx.notify();
                            })),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tab = ContentTab::Dump(ix);
                        cx.notify();
                    }))
            }))
            .child(self.layout_button(layout))
            .into_any_element()
    }

    /// Layout Settings (`RunnerContentUi`'s view actions): Threads & Frames ▸ its layouts or Don't
    /// Show, Variables, Console, Separate Watches, Show Tab Labels, Restore Default Layout.
    fn layout_button(&self, layout: DebugLayout) -> impl IntoElement {
        ActionButton::new("debug-layout-settings", "general/layout")
            .tooltip("Layout Settings")
            .dropdown_menu(move |menu, window, cx| {
                let set = |change: fn(&mut DebugLayout)| {
                    move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                        let mut layout = debug_layout(cx);
                        change(&mut layout);
                        set_debug_layout(layout, cx);
                    }
                };
                menu.submenu("Threads & Frames", window, cx, move |menu, _, _| {
                    let frames = |label: &'static str, frames: FramesLayout| {
                        PopupMenuItem::new(label)
                            .checked(layout.frames == frames)
                            .disabled(frames == FramesLayout::Hidden && !layout.variables)
                            .on_click(move |_, _, cx| {
                                let mut layout = debug_layout(cx);
                                layout.frames = frames;
                                set_debug_layout(layout, cx);
                            })
                    };
                    menu.item(frames("Combined", FramesLayout::Combined))
                        .item(frames("Threads", FramesLayout::Threads))
                        .item(frames("Side-by-Side", FramesLayout::SideBySide))
                        .item(frames("Show Frames Only", FramesLayout::FramesOnly))
                        .item(frames("Don't Show", FramesLayout::Hidden))
                })
                .item(
                    PopupMenuItem::new("Variables")
                        .checked(layout.variables)
                        .disabled(layout.frames == FramesLayout::Hidden)
                        .on_click(set(|layout| layout.variables = !layout.variables)),
                )
                .item(
                    PopupMenuItem::new("Console")
                        .checked(layout.console)
                        .on_click(set(|layout| layout.console = !layout.console)),
                )
                .separator()
                .item(
                    PopupMenuItem::new("Separate Watches")
                        .checked(layout.separate_watches)
                        .on_click(set(|layout| {
                            layout.separate_watches = !layout.separate_watches
                        })),
                )
                .separator()
                .item(
                    PopupMenuItem::new("Show Tab Labels")
                        .checked(layout.tab_labels)
                        .on_click(set(|layout| layout.tab_labels = !layout.tab_labels)),
                )
                .item(
                    PopupMenuItem::new("Restore Default Layout")
                        .on_click(set(|layout| *layout = DebugLayout::default())),
                )
            })
    }

    fn frames(
        &mut self,
        running: &Entity<debugger_ui::RunningState>,
        layout: DebugLayout,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = Tokens::get(cx);
        self.ask_thread_infos(running, cx);
        let FrameRows {
            thread,
            threads,
            thread_ids,
            shown,
            labels,
        } = frame_rows(running, self.hide_library_frames, &self.thread_infos, cx);
        let stack = labels
            .iter()
            .map(|label| format!("{label}\n"))
            .collect::<String>();
        let menu = self.frames_menu(running, stack, cx);
        let opened = running.read(cx).opened_stack_frame(cx);
        let rows: Vec<AnyElement> = shown
            .iter()
            .zip(labels)
            .enumerate()
            .map(|(ix, (row, label))| {
                let Ok(frame) = row else {
                    return h_flex()
                        .id(("debug-hidden-frames", ix))
                        .role(Role::ListItem)
                        .aria_label(SharedString::from(label.clone()))
                        .flex_none()
                        .h(px(24.))
                        .mx(px(6.))
                        .px(px(6.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_color(tokens.text_secondary)
                        .hover(move |row| row.bg(tokens.selection_hover))
                        .child(div().truncate().child(label))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.hide_library_frames = false;
                            cx.notify();
                        }))
                        .into_any_element();
                };
                // IntelliJ's separator with its caption over the async stack trace
                // (`frame.panel.async.stacktrace`)
                if is_separator(frame) {
                    return h_flex()
                        .id(("debug-frame-separator", ix))
                        .role(Role::Heading)
                        .aria_label(SharedString::from(label.clone()))
                        .flex_none()
                        .h(px(24.))
                        .mx(px(6.))
                        .px(px(6.))
                        .gap(px(6.))
                        .text_color(tokens.text_secondary)
                        .child(div().flex_none().child(label))
                        .child(div().flex_1().h(px(1.)).bg(tokens.separator))
                        .into_any_element();
                }
                let id = frame.id;
                let selected = opened == Some(id);
                let running = running.downgrade();
                h_flex()
                    .id(("debug-frame", id as usize))
                    .role(Role::ListItem)
                    .aria_label(SharedString::from(label.clone()))
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
        let hide = self.hide_library_frames;
        let empty = rows.is_empty();
        let current = running.read(cx).selected_thread_id();
        // a thread's row for the Threads and Side-by-Side layouts: a click shows its frames
        let thread_row = |ix: usize, label: &String, cx: &mut Context<Self>| {
            let id = thread_ids.get(ix).copied();
            let selected = id.is_some() && id == current;
            let running = running.downgrade();
            h_flex()
                .id(("debug-thread-row", ix))
                .role(Role::TreeItem)
                .aria_label(SharedString::from(label.clone()))
                .aria_selected(selected)
                .aria_expanded(selected)
                .flex_none()
                .h(px(24.))
                .mx(px(6.))
                .px(px(6.))
                .gap(px(4.))
                .rounded(px(4.))
                .cursor_pointer()
                .when(selected, |row| row.bg(tokens.selection))
                .when(!selected, |row| {
                    row.hover(move |row| row.bg(tokens.selection_hover))
                })
                .child(spork_workbench::image("debugger/threadRunning", cx).size(px(16.)))
                .child(div().truncate().child(label.clone()))
                .on_click(move |_, window, cx| {
                    if let Some(id) = id {
                        running
                            .update(cx, |running, cx| running.choose_thread(id, window, cx))
                            .log_err();
                    }
                })
                .into_any_element()
        };
        let not_available = || {
            div()
                .px(px(12.))
                .text_color(tokens.text_secondary)
                .child("Frames are not available")
        };
        let hide_button = ActionButton::new("debug-hide-library-frames", "general/filter")
            // a toggle, as IntelliJ's (`hide.library.frames.tooltip`)
            .tooltip("Hide Frames from Libraries")
            .selected(hide)
            .on_click(cx.listener(|this, _, _, cx| {
                this.hide_library_frames = !this.hide_library_frames;
                cx.notify();
            }));
        let header =
            h_flex()
                .flex_none()
                .h(px(32.))
                .px(px(8.))
                .gap(px(4.))
                .border_b_1()
                .border_color(tokens.separator)
                .child(div().flex_1().min_w_0().when(
                    layout.frames == FramesLayout::Combined,
                    |slot| {
                        slot.child(self.thread_chooser(
                            running,
                            thread,
                            threads.clone(),
                            thread_ids.clone(),
                        ))
                    },
                ))
                .child(hide_button);
        let list = |id: &'static str, label: &'static str| {
            v_flex()
                .id(id)
                .role(Role::List)
                .aria_label(label)
                .flex_1()
                .min_w_0()
                .py(px(4.))
                .overflow_y_scroll()
        };
        let body = match layout.frames {
            FramesLayout::SideBySide => {
                let thread_rows: Vec<AnyElement> = threads
                    .iter()
                    .enumerate()
                    .map(|(ix, label)| thread_row(ix, label, cx))
                    .collect();
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(
                        list("debug-threads", "Threads")
                            .h_full()
                            .border_r_1()
                            .border_color(tokens.separator)
                            .children(thread_rows),
                    )
                    .child(
                        list("debug-frames", "Frames")
                            .h_full()
                            .children(rows)
                            .when(empty, |list| list.child(not_available()))
                            .context_menu(menu),
                    )
                    .into_any_element()
            }
            // each thread a node, the current one open on its frames
            FramesLayout::Threads => {
                let mut tree: Vec<AnyElement> = Vec::new();
                let mut frames = Some(rows);
                for (ix, label) in threads.iter().enumerate() {
                    tree.push(thread_row(ix, label, cx));
                    if thread_ids.get(ix).copied() == current
                        && let Some(frames) = frames.take()
                    {
                        tree.extend(
                            frames
                                .into_iter()
                                .map(|frame| div().pl(px(20.)).child(frame).into_any_element()),
                        );
                    }
                }
                list("debug-frames", "Frames")
                    .role(Role::Tree)
                    .children(tree)
                    .context_menu(menu)
                    .into_any_element()
            }
            _ => list("debug-frames", "Frames")
                .children(rows)
                .when(empty, |list| list.child(not_available()))
                .context_menu(menu)
                .into_any_element(),
        };
        let width = match layout.frames {
            FramesLayout::SideBySide => 480.,
            FramesLayout::Threads => 340.,
            _ => 280.,
        };
        v_flex()
            .map(|pane| match layout.variables {
                true => pane.w(px(width)).flex_none(),
                false => pane.flex_1(),
            })
            .h_full()
            .border_r_1()
            .border_color(tokens.separator)
            .child(header)
            .child(body)
            .into_any_element()
    }

    /// The thread chooser over the frames (IntelliJ's threads combo box): each thread, the one
    /// picked showing its frames.
    fn thread_chooser(
        &self,
        running: &Entity<debugger_ui::RunningState>,
        thread: String,
        threads: Vec<String>,
        ids: Vec<ThreadId>,
    ) -> AnyElement {
        let running = running.downgrade();
        let empty = threads.is_empty();
        // named by the thread it shows, as assistive technology reads a combo box
        let name = thread.clone();
        spork_workbench::controls::labeled_combo(
            "debug-thread",
            name,
            thread,
            0.,
            empty,
            move |menu, _, _| {
                threads.iter().zip(&ids).fold(menu, |menu, (label, id)| {
                    let (running, id) = (running.clone(), *id);
                    menu.item(
                        PopupMenuItem::new(label.clone()).on_click(move |_, window, cx| {
                            running
                                .update(cx, |running, cx| running.choose_thread(id, window, cx))
                                .log_err();
                        }),
                    )
                })
            },
        )
    }

    /// The frames' popup (`XDebugger.Frames.Tree.Popup`): Reset Frame, Copy Stack and Hide Frames
    /// from Libraries; Throw Exception and Force Return (java-debug has neither), Async Stack
    /// Traces, Customize Threads View…, Add Stepping Filter… and Export Threads… greyed.
    fn frames_menu(
        &self,
        running: &Entity<debugger_ui::RunningState>,
        stack: String,
        cx: &mut Context<Self>,
    ) -> impl Fn(
        gpui_kit::component::menu::PopupMenu,
        &mut Window,
        &mut Context<gpui_kit::component::menu::PopupMenu>,
    ) -> gpui_kit::component::menu::PopupMenu
    + 'static {
        let reset = running.read(cx).thread_status(cx) == Some(ThreadStatus::Stopped)
            && running
                .read(cx)
                .session()
                .read(cx)
                .capabilities()
                .supports_restart_frame
                .unwrap_or_default();
        let hide = self.hide_library_frames;
        let this = cx.entity().downgrade();
        let session = running.read(cx).session().clone();
        let stopped = running.read(cx).thread_status(cx) == Some(ThreadStatus::Stopped);
        let adapter = hooks(session.read(cx), cx);
        let frame = running.read(cx).opened_stack_frame(cx);
        // the opened frame's class, for Add Stepping Filter…
        let class = frame.and_then(|frame| {
            let thread = running.read(cx).selected_thread_id()?;
            let frames = session
                .update(cx, |session, cx| session.stack_frames(thread, cx))
                .ok()?;
            let frame = frames.into_iter().find(|f| f.dap.id == frame)?;
            (adapter?.frame_class?)(&frame.dap)
        });
        let dump = adapter
            .and_then(|hooks| hooks.thread_dump)
            .zip(frame)
            .filter(|_| stopped);
        let folder = session
            .read(cx)
            .worktree()
            .map(|worktree| worktree.read(cx).abs_path().to_path_buf());
        move |menu, _, _| {
            let greyed = |label: &'static str| PopupMenuItem::new(label).disabled(true);
            let stack = stack.clone();
            let this = this.clone();
            let class = class.clone();
            let (session, folder) = (session.clone(), folder.clone());
            menu.item(PopupMenuItem::new("Reset Frame").disabled(!reset).on_click(
                |_, window, cx| window.dispatch_action(run::ResetFrame.boxed_clone(), cx),
            ))
            .item(greyed("Throw Exception"))
            .item(greyed("Force Return"))
            .separator()
            .item(
                PopupMenuItem::new("Copy Stack")
                    .disabled(stack.is_empty())
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(stack.clone()))
                    }),
            )
            .separator()
            .item(
                PopupMenuItem::new("Hide Frames from Libraries")
                    .checked(hide)
                    .on_click(move |_, _, cx| {
                        this.update(cx, |this, cx| {
                            this.hide_library_frames = !this.hide_library_frames;
                            cx.notify();
                        })
                        .log_err();
                    }),
            )
            .item(greyed("Async Stack Traces"))
            .separator()
            .item(
                PopupMenuItem::new("Customize Threads View…")
                    .on_click(|_, _, cx| crate::threads::customize_threads_view(cx)),
            )
            .item(
                PopupMenuItem::new("Add Stepping Filter…")
                    .disabled(class.is_none())
                    .on_click(move |_, _, cx| {
                        if let Some(class) = &class {
                            add_stepping_filter(class.clone(), cx)
                        }
                    }),
            )
            .item(
                PopupMenuItem::new("Export Threads…")
                    .disabled(dump.is_none())
                    .on_click(move |_, _, cx| {
                        if let Some((dump, frame)) = dump {
                            let threads = dump(&session, frame, cx);
                            let folder = folder.clone();
                            cx.spawn(async move |cx| {
                                let threads = threads.await?;
                                cx.update(|cx| {
                                    crate::threads::export_preview(
                                        folder,
                                        crate::threads::dump_text(&threads),
                                        cx,
                                    )
                                });
                                anyhow::Ok(())
                            })
                            .detach_and_log_err(cx);
                        }
                    }),
            )
        }
    }

    fn variables(
        &self,
        running: &Entity<debugger_ui::RunningState>,
        layout: DebugLayout,
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
                    .when(!layout.separate_watches, |pane| {
                        let selected = running.read(cx).variables().read(cx).selected_value();
                        pane.child(self.watches_toolbar(running, selected, cx))
                    })
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
        selected: Option<(SharedString, bool)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = Tokens::get(cx);
        let session = running.read(cx).session().clone();
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

    /// Separate Watches' view (`XSplitterWatchesViewImpl` beside the variables): its toolbar and
    /// the watches, each `expression = value`, a click selecting one for the toolbar.
    fn separate_watches(
        &self,
        running: &Entity<debugger_ui::RunningState>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = Tokens::get(cx);
        let session = running.read(cx).session().clone();
        let watches: Vec<(SharedString, SharedString)> = session
            .read(cx)
            .watchers()
            .values()
            .map(|watch| (watch.expression.clone(), watch.value.clone()))
            .collect();
        let selected = self
            .selected_watch
            .clone()
            .filter(|watch| watches.iter().any(|(expression, _)| expression == watch));
        let toolbar =
            self.watches_toolbar(running, selected.clone().map(|watch| (watch, true)), cx);
        let rows: Vec<AnyElement> = watches
            .into_iter()
            .enumerate()
            .map(|(ix, (expression, value))| {
                let label = SharedString::from(format!("{expression} = {value}"));
                let on = selected.as_ref() == Some(&expression);
                h_flex()
                    .id(("debug-watch", ix))
                    .role(Role::TreeItem)
                    .aria_label(label.clone())
                    .aria_selected(on)
                    .flex_none()
                    .h(px(24.))
                    .mx(px(6.))
                    .px(px(6.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .when(on, |row| row.bg(tokens.selection))
                    .when(!on, |row| {
                        row.hover(move |row| row.bg(tokens.selection_hover))
                    })
                    .child(div().truncate().child(label))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected_watch = Some(expression.clone());
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        v_flex()
            .id("debug-watches")
            .role(Role::Group)
            .aria_label("Watches")
            .w(px(300.))
            .flex_none()
            .h_full()
            .border_l_1()
            .border_color(tokens.separator)
            .child(
                div()
                    .flex_none()
                    .h(px(32.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(tokens.separator)
                    .child("Watches"),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(toolbar)
                    .child(
                        v_flex()
                            .id("debug-watches-tree")
                            .role(Role::Tree)
                            .aria_label("Watches")
                            .flex_1()
                            .min_w_0()
                            .py(px(4.))
                            .overflow_y_scroll()
                            .children(rows)
                            // `debugger.no.watches`
                            .when(empty, |tree| {
                                tree.child(
                                    div()
                                        .id("debug-no-watches")
                                        .role(Role::Label)
                                        .aria_label("No watches")
                                        .px(px(12.))
                                        .text_color(tokens.text_secondary)
                                        .child("No watches"),
                                )
                            }),
                    ),
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        self.observe_session(cx);
        let workspace = Workspace::for_window(window, cx);
        let header = self.header(cx);
        let running = self.running(cx);
        let muted = crate::breakpoints::muted(cx);
        let layout = debug_layout(cx);
        if self.tab == ContentTab::Console && !layout.console {
            self.tab = ContentTab::ThreadsAndVariables;
        }
        let body = running.map(|running| {
            let status = running.read(cx).thread_status(cx);
            let name = running
                .read(cx)
                .session()
                .read(cx)
                .label()
                .unwrap_or_default();
            let capabilities = running.read(cx).session().read(cx).capabilities().clone();
            let session = running.read(cx).session().clone();
            let update = workspace.as_ref().and_then(|workspace| {
                crate::services::update_button(workspace, &name, Some(session), cx)
            });
            let dumps = hooks(running.read(cx).session().read(cx), cx)
                .is_some_and(|hooks| hooks.thread_dump.is_some());
            let toolbar = self.toolbar(status, muted, &name, &capabilities, update, dumps, cx);
            let tabs = self.content_tabs(cx);
            let content = match self.tab {
                ContentTab::ThreadsAndVariables => {
                    let frames = (layout.frames != FramesLayout::Hidden)
                        .then(|| self.frames(&running, layout, cx));
                    let variables = layout
                        .variables
                        .then(|| self.variables(&running, layout, cx));
                    let watches = layout
                        .separate_watches
                        .then(|| self.separate_watches(&running, cx));
                    h_flex()
                        .size_full()
                        .items_start()
                        .children(frames)
                        .children(variables)
                        .children(watches)
                        .into_any_element()
                }
                ContentTab::Dump(ix) => match self.dumps.get(ix) {
                    Some(dump) => dump.clone().into_any_element(),
                    None => div().into_any_element(),
                },
                // a debugged test's tree and consoles, as IntelliJ's test runner in the session
                ContentTab::Console => div()
                    .id("debug-console-content")
                    .role(Role::Group)
                    .aria_label("Console")
                    .size_full()
                    .child(
                        match (
                            debugged_tests(window, &name, cx),
                            running.read(cx).program_terminal(cx),
                        ) {
                            (Some(tests), _) => tests.into_any_element(),
                            (None, Some(terminal)) => terminal.into_any_element(),
                            (None, None) => running.read(cx).console_view().into_any_element(),
                        },
                    )
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

/// The tests of the debugged run a session of that name attached to.
fn debugged_tests(
    window: &Window,
    name: &str,
    cx: &App,
) -> Option<Entity<crate::test_runner::TestRunView>> {
    let workspace = Workspace::for_window(window, cx)?;
    crate::run_panel::debugged_tests(workspace.read(cx), name, cx).map(|(tests, _)| tests)
}

/// Add Stepping Filter… (`AddSteppingFilterAction`): IntelliJ's question with the frame's class,
/// then the pattern among Settings › Stepping's "Do not step into the classes".
fn add_stepping_filter(class: String, cx: &mut App) {
    spork_settings::debugger::ask_text(
        "Add Stepping Filter",
        None,
        class,
        Rc::new(|pattern, cx| {
            let mut settings = spork_settings::debugger::settings(cx);
            if !settings
                .step_filters
                .iter()
                .any(|(known, _)| *known == pattern)
            {
                settings.step_filters.push((pattern, true));
            }
            spork_settings::debugger::set_settings(settings, cx).log_err();
        }),
        cx,
    );
}

/// The active debug session, for the end-to-end tests: its thread's status, its watches with their
/// values and in their order, whether it skips the breakpoints, the thread and frames shown, and
/// the variables tree's rows.
/// A JVM frame as a stack trace writes it (`at com.example.MainKt.greet(Main.kt:13)`, the
/// Kotlin language server's names): its method, class and package, which IntelliJ's frames show
/// as `greet:13, MainKt (com.example)`.
fn jvm_frame(name: &str) -> Option<(&str, &str, Option<&str>)> {
    let (qualified, _) = name.strip_prefix("at ")?.split_once('(')?;
    let (class, method) = qualified.rsplit_once('.')?;
    Some(match class.rsplit_once('.') {
        Some((package, class)) => (method, class, Some(package)),
        None => (method, class, None),
    })
}

pub(crate) fn e2e_debug(workspace: &Workspace, cx: &App) -> serde_json::Value {
    let Some(running) = running_state(workspace, cx) else {
        return serde_json::Value::Null;
    };
    let variables = running.read(cx).variables().read(cx).rows(cx);
    let label = running
        .read(cx)
        .session()
        .read(cx)
        .label()
        .unwrap_or_default();
    let tests = crate::run_panel::debugged_tests(workspace, &label, cx);
    let console: Vec<String> = match running.read(cx).program_terminal(cx) {
        // a debugged test's run: its terminal's text
        _ if tests.is_some() => tests
            .as_ref()
            .map(|(_, terminal)| terminal.read(cx).get_content())
            .unwrap_or_default()
            .lines()
            .map(|line| line.trim_end().to_owned())
            .collect(),
        // the terminal's own text: its view may not have been drawn
        Some(view) => view
            .read(cx)
            .terminal()
            .read(cx)
            .get_content()
            .lines()
            .map(|line| line.trim_end().to_owned())
            .collect(),
        None => {
            let session = running.read(cx).session().read(cx);
            let (events, _) = session.output(project::debugger::session::OutputToken(0));
            events
                .flat_map(|event| event.output.lines().map(str::to_owned).collect::<Vec<_>>())
                .collect()
        }
    };
    let console = console.join("\n").trim_end().to_owned();

    let running = running.read(cx);
    let session = running.session().read(cx);
    let watches: serde_json::Map<String, serde_json::Value> = session
        .watchers()
        .values()
        .map(|watch| (watch.expression.to_string(), watch.value.to_string().into()))
        .collect();
    let view = workspace
        .panel::<DebugPanel>(cx)
        .and_then(|panel| cx.try_global::<Views>()?.0.get(&panel.entity_id()).cloned());
    let dumps: Vec<String> = view
        .as_ref()
        .map(|view| {
            view.read(cx)
                .dumps
                .iter()
                .map(|dump| dump.read(cx).title.to_string())
                .collect()
        })
        .unwrap_or_default();
    let dump = view.as_ref().and_then(|view| match view.read(cx).tab {
        ContentTab::Dump(ix) => view.read(cx).dumps.get(ix)?.read(cx).selected_text(cx),
        _ => None,
    });
    let layout = cx
        .try_global::<DebugLayoutGlobal>()
        .map(|layout| layout.0)
        .unwrap_or_default();
    serde_json::json!({
        "layout": serde_json::to_value(layout).unwrap_or_default(),
        "dumps": dumps,
        "dump": dump,
        "status": running.thread_status(cx).map(|status| status.label()),
        "terminated": session.is_terminated(),
        "watches": watches,
        "watch_order": session.watchers().keys().map(|w| w.to_string()).collect::<Vec<_>>(),
        "skipping_breakpoints": session.ignore_breakpoints(),
        "label": session.label().map(|label| label.to_string()),
        "variables": variables,
        "console": console,
        "tests": tests.map(|(tests, _)| tests.read(cx).e2e_state(cx)),
    })
}

#[cfg(test)]
mod tests {
    use super::jvm_frame;

    #[test]
    fn reads_a_jvm_frame() {
        assert_eq!(
            jvm_frame("at com.example.MainKt.greet(Main.kt:13)"),
            Some(("greet", "MainKt", Some("com.example")))
        );
        assert_eq!(
            jvm_frame("at MainKt.main(Main.kt:4)"),
            Some(("main", "MainKt", None))
        );
        assert_eq!(jvm_frame("Greeting.hello(String)"), None);
    }
}
