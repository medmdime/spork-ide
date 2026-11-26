//! IntelliJ's breakpoints over Zed's breakpoint store: the gutter's icons (expUI's line,
//! disabled, conditional, logging, verified, invalid and muted breakpoints), the popup a right
//! click on one shows (`XLightBreakpointPropertiesPanel`: Enabled, Suspend, Condition, More), the
//! Breakpoints dialog (⇧⌘F8, `BreakpointsDialog`), Mute Breakpoints and Toggle Breakpoint Enabled.
//! The dialog lists the exception breakpoints too: a group per debug adapter ("CodeLLDB Exception
//! Breakpoints", as IntelliJ's "Java Exception Breakpoints") with the filters the adapter offers
//! (its `exceptionBreakpointFilters`), sent to its sessions and kept for its next ones as Zed keeps
//! them. A breakpoint's properties beyond the protocol's (Suspend: All or Thread, Remove once hit,
//! "Disable until hitting the following breakpoint") are kept with it in the store; the sessions
//! send a Thread breakpoint with the adapter's `thread` mode and leave out the ones waiting for
//! another, and the breakpoint a session stops at (`watch_hits`) is removed when temporary and
//! sets the ones waiting for it.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use dap::ExceptionBreakpointsFilter;
use dap::adapters::DebugAdapterName;
use dap::client::SessionId;
use editor::{BreakpointGutterIcon, BreakpointLook, BreakpointMenu, BreakpointToggle, Editor};
use gpui::TaskExt as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, Context, DismissEvent, Entity, EventEmitter, FocusHandle,
    Focusable, Global, InteractiveElement as _, IntoElement, ParentElement as _, Point, Render,
    Role, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    WindowBounds, WindowOptions, anchored, div, point, px, size,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use project::Project;
use project::debugger::breakpoint_store::{
    Breakpoint, BreakpointEditAction, BreakpointKind, BreakpointState, BreakpointStore,
    BreakpointStoreEvent, BreakpointWithPosition,
};
use project::debugger::dap_store::DapStoreEvent;
use project::debugger::dap_store::{PersistedAdapterOptions, PersistedExceptionBreakpoint};
use project::debugger::session::SessionEvent;
use serde_json::{Value, json};
use spork_workbench::controls::{
    check_box, disabled_check, disabled_radio, labeled_check, labeled_combo, labeled_radio, link,
};
use spork_workbench::{ActionButton, Tokens};
use util::ResultExt as _;
use workspace::{ModalView, Workspace};

use crate::run;

/// Mute Breakpoints: the debug sessions skip the breakpoints, which the gutter shows muted.
#[derive(Default)]
struct Muted(bool);

impl Global for Muted {}

pub(crate) fn muted(cx: &App) -> bool {
    cx.try_global::<Muted>().is_some_and(|muted| muted.0)
}

pub(crate) fn init(cx: &mut App) {
    cx.set_global(Muted::default());
    cx.set_global(BreakpointGutterIcon(Rc::new(|look, cx| {
        gutter_icon(look, muted(cx), cx)
    })));
    cx.set_global(BreakpointMenu(Rc::new(
        |editor, anchor, _, position, window, cx| show_popup(editor, anchor, position, window, cx),
    )));
}

pub(crate) fn register(workspace: &mut Workspace, cx: &mut Context<Workspace>) {
    // the languages' toggles are set by now
    default_suspend_policy(cx);
    workspace.register_action(|workspace, _: &run::ViewBreakpoints, window, cx| {
        open_dialog(workspace.project().clone(), None, window, cx)
    });
    workspace.register_action(|workspace, _: &run::EditBreakpoint, window, cx| {
        // the Breakpoints dialog's binding of the same key takes it otherwise, as IntelliJ's
        if !edit_at_caret(workspace, window, cx) {
            cx.propagate();
        }
    });
    workspace.register_action(|workspace, _: &run::ToggleTemporaryLineBreakpoint, _, cx| {
        toggle_temporary(workspace, cx)
    });
    watch_hits(workspace, cx);
    workspace.register_action(|workspace, _: &run::MuteBreakpoints, _, cx| {
        let muted = !muted(cx);
        cx.set_global(Muted(muted));
        apply_mute(workspace.project(), cx);
        cx.refresh_windows();
    });
    workspace.register_action(|workspace, _: &run::ToggleBreakpointEnabled, _, cx| {
        let Some(editor) = workspace.active_item_as::<Editor>(cx) else {
            return;
        };
        let Some((path, row)) = caret_line(&editor, cx) else {
            return;
        };
        let store = workspace.project().read(cx).breakpoint_store();
        edit(&store, &path, row, cx, |breakpoint| {
            breakpoint.state = match breakpoint.state {
                BreakpointState::Enabled => BreakpointState::Disabled,
                BreakpointState::Disabled => BreakpointState::Enabled,
            };
        });
    });
}

/// Toggle Temporary Line Breakpoint (⌥⇧⌘F8): a breakpoint removed once hit on the caret line,
/// or the line's breakpoint removed (`XBreakpointUtil.toggleLineBreakpoint`).
fn toggle_temporary(workspace: &mut Workspace, cx: &mut Context<Workspace>) {
    let Some(editor) = workspace.active_item_as::<Editor>(cx) else {
        return;
    };
    let Some((path, row)) = caret_line(&editor, cx) else {
        return;
    };
    let project = workspace.project().clone();
    let store = project.read(cx).breakpoint_store();
    if breakpoint(&store, &path, row, cx).is_some() {
        remove(&store, &path, row, cx);
        return;
    }
    let Some(project_path) = project.read(cx).find_project_path(&*path, cx) else {
        return;
    };
    let open = project.update(cx, |project, cx| project.open_buffer(project_path, cx));
    cx.spawn(async move |_, cx| {
        let buffer = open.await?;
        store.update(cx, |store, cx| {
            let position = buffer
                .read(cx)
                .snapshot()
                .anchor_after(text::Point::new(row, 0));
            let mut breakpoint = Breakpoint::new_standard();
            breakpoint.properties.temporary = true;
            breakpoint.properties.suspend_thread =
                spork_settings::debugger::settings(cx).default_suspend_thread;
            store.toggle_breakpoint(
                buffer,
                BreakpointWithPosition {
                    position,
                    bp: breakpoint,
                },
                BreakpointEditAction::Toggle,
                cx,
            );
        });
        anyhow::Ok(())
    })
    .detach_and_log_err(cx);
}

/// The breakpoint each debug session stops at, as IntelliJ's `breakpointReached` handles it: the
/// ones waiting for it are set, and a temporary one is removed. The first execution line shown
/// after a stop is where the program stopped.
fn watch_hits(workspace: &mut Workspace, cx: &mut Context<Workspace>) {
    let project = workspace.project().clone();
    let dap_store = project.read(cx).dap_store();
    let store = project.read(cx).breakpoint_store();
    let stopped: Rc<RefCell<HashSet<SessionId>>> = Rc::default();
    cx.subscribe(&dap_store, {
        let stopped = stopped.clone();
        move |_, dap_store, event, cx| {
            let DapStoreEvent::DebugClientStarted(id) = event else {
                return;
            };
            let Some(session) = dap_store.read(cx).session_by_id(*id) else {
                return;
            };
            let stopped = stopped.clone();
            cx.subscribe(&session, move |_, session, event: &SessionEvent, cx| {
                if let SessionEvent::Stopped(_) = event {
                    stopped.borrow_mut().insert(session.read(cx).session_id());
                }
            })
            .detach();
        }
    })
    .detach();
    cx.subscribe(&store, move |_, store, event, cx| {
        if !matches!(event, BreakpointStoreEvent::SetDebugLine) {
            return;
        }
        let Some(frame) = store.read(cx).active_position().cloned() else {
            return;
        };
        if !stopped.borrow_mut().remove(&frame.session_id) {
            return;
        }
        let Some(session) = dap_store.read(cx).session_by_id(frame.session_id) else {
            return;
        };
        if session.read(cx).ignore_breakpoints() {
            return;
        }
        let buffer = project
            .read(cx)
            .find_project_path(&*frame.path, cx)
            .and_then(|path| project.read(cx).get_open_buffer(&path, cx));
        let Some(buffer) = buffer else {
            return;
        };
        let row = text::ToPoint::to_point(&frame.position, &buffer.read(cx).snapshot()).row;
        let Some(hit) = breakpoint(&store, &frame.path, row, cx).filter(Breakpoint::is_enabled)
        else {
            return;
        };
        session.update(cx, |session, cx| {
            session.breakpoint_hit(frame.path.clone(), row, cx)
        });
        if hit.properties.temporary {
            remove(&store, &frame.path, row, cx);
        }
    })
    .detach();
}

/// The debug sessions skip the breakpoints, or stop skipping them, as Mute Breakpoints says;
/// also for a session that starts while they are muted.
pub(crate) fn apply_mute(project: &Entity<Project>, cx: &mut App) {
    let muted = muted(cx);
    let sessions: Vec<_> = project
        .read(cx)
        .dap_store()
        .read(cx)
        .sessions()
        .cloned()
        .collect();
    for session in sessions {
        session.update(cx, |session, cx| {
            if session.ignore_breakpoints() != muted {
                session.toggle_ignore_breakpoints(cx).detach();
            }
        });
    }
}

/// IntelliJ's breakpoint icon for how a breakpoint stands (`XBreakpointUIUtil`), a method
/// breakpoint's and a field watchpoint's of their own: a conditional one wears the question badge.
fn gutter_icon(look: BreakpointLook, muted: bool, cx: &App) -> AnyElement {
    let (kind, plain) = match (look.method, look.field) {
        (true, _) => ("breakpointMethod", "Method breakpoint"),
        (_, true) => ("breakpointField", "Field watchpoint"),
        _ => ("breakpoint", "Line breakpoint"),
    };
    let line = !look.method && !look.field;
    let state = match (muted, look.disabled, look.verified, look.logs) {
        (true, true, _, _) => "MutedDisabled",
        (true, false, _, _) if look.dependent => "MutedDependent",
        (true, false, _, _) => "Muted",
        (false, false, _, _) if look.dependent => "Dependent",
        (false, true, _, true) => "UnsuspendentDisabled",
        (false, true, _, false) => "Disabled",
        // the method and field icons have no invalid look
        (false, false, Some(false), _) if line => "Invalid",
        (false, false, Some(false), _) => "",
        (false, false, Some(true), true) => "UnsuspendentValid",
        (false, false, Some(true), false) => "Valid",
        (false, false, None, true) => "Unsuspendent",
        (false, false, None, false) => "",
    };
    let name = SharedString::from(format!("breakpoints/{kind}{state}"));
    let label = match (muted, look.disabled, look.verified, look.conditional) {
        (true, _, _, _) => "Muted breakpoint",
        (false, true, _, _) => "Disabled breakpoint",
        (false, false, Some(false), _) => "Invalid breakpoint",
        (false, false, _, _) if look.dependent => "Dependent breakpoint",
        (false, false, _, true) => "Conditional breakpoint",
        (false, false, _, false) => plain,
    };
    div()
        .id(name.clone())
        .role(Role::Image)
        .aria_label(label)
        .relative()
        .size(px(14.))
        .child(spork_workbench::image(&name, cx).size(px(14.)))
        .when(look.conditional && !look.disabled, |icon| {
            // the badge at (7, 6) of the 16 px icon, as IntelliJ's New UI layers it
            icon.child(
                spork_workbench::image("breakpoints/questionBadge", cx)
                    .absolute()
                    .left(px(6.))
                    .top(px(5.))
                    .size(px(8.)),
            )
        })
        .into_any_element()
}

/// The file and row of the editor's caret line.
pub(crate) fn caret_line(editor: &Entity<Editor>, cx: &App) -> Option<(Arc<Path>, u32)> {
    let editor = editor.read(cx);
    let buffer = editor.buffer().read(cx);
    let snapshot = buffer.snapshot(cx);
    let head = editor.selections.newest_anchor().head();
    let (anchor, _) = snapshot.anchor_to_buffer_anchor(head)?;
    let buffer = buffer.buffer(anchor.buffer_id)?;
    let path = BreakpointStore::abs_path_from_buffer(&buffer, cx)?;
    let row = text::ToPoint::to_point(&anchor, &buffer.read(cx).snapshot()).row;
    Some((path, row))
}

/// Changes the breakpoint at `row` of `path`. Zed's edits of a message or condition remove a
/// breakpoint whose text is emptied, so the breakpoint is put back as edited instead.
fn edit(
    store: &Entity<BreakpointStore>,
    path: &Arc<Path>,
    row: u32,
    cx: &mut App,
    change: impl FnOnce(&mut Breakpoint),
) {
    store.update(cx, |store, cx| {
        let Some((buffer, old)) = store.breakpoint_at_row(path, row, cx) else {
            return;
        };
        let mut new = old.clone();
        change(&mut new.bp);
        if new == old {
            return;
        }
        store.toggle_breakpoint(buffer.clone(), old, BreakpointEditAction::Toggle, cx);
        store.toggle_breakpoint(buffer, new, BreakpointEditAction::Toggle, cx);
    });
}

/// Removes the breakpoint at `row` of `path`.
fn remove(store: &Entity<BreakpointStore>, path: &Arc<Path>, row: u32, cx: &mut App) {
    store.update(cx, |store, cx| {
        if let Some((buffer, old)) = store.breakpoint_at_row(path, row, cx) {
            store.toggle_breakpoint(buffer, old, BreakpointEditAction::Toggle, cx);
        }
    });
}

fn breakpoint(
    store: &Entity<BreakpointStore>,
    path: &Path,
    row: u32,
    cx: &App,
) -> Option<Breakpoint> {
    store
        .read(cx)
        .breakpoint_at_row(path, row, cx)
        .map(|(_, breakpoint)| breakpoint.bp)
}

/// IntelliJ's name for a line breakpoint: its file and line, `greeting.rs:2`.
fn short_name(path: &Path, row: u32) -> String {
    let file = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    format!("{file}:{}", row + 1)
}

/// IntelliJ's name for a breakpoint in the Breakpoints dialog: a line breakpoint's file and
/// line, a method's or field's own (`com.example.Debuggee.measure`, `Debuggee.counter`).
fn display_name(path: &Path, row: u32, breakpoint: &Breakpoint) -> String {
    match &breakpoint.properties.kind {
        BreakpointKind::Line => short_name(path, row),
        BreakpointKind::Method { name, .. } | BreakpointKind::Field { name, .. } => name.clone(),
    }
}

/// The dialog's group of a breakpoint: line, method, field, Kotlin function.
fn kind_index(breakpoint: &Breakpoint) -> usize {
    match breakpoint.properties.kind {
        BreakpointKind::Line => 0,
        BreakpointKind::Method { function: None, .. } => 1,
        BreakpointKind::Field { .. } => 2,
        BreakpointKind::Method {
            function: Some(_), ..
        } => 3,
    }
}

/// The text a log message without one of its own says: IntelliJ's "Breakpoint hit" message.
/// (Java's debugger writes IntelliJ's own, `Breakpoint reached at com.example.Main.main(Main.java:5)`.)
fn hit_message(path: &Path, row: u32) -> String {
    format!("{HIT} {}", short_name(path, row))
}

const HIT: &str = "Breakpoint reached at";

/// A breakpoint's log message (the debug adapter protocol's, whose `{expression}`s are
/// evaluated): whether it says the "Breakpoint hit" message, and the expression it evaluates
/// and logs (IntelliJ's "Evaluate and log").
fn log_parts(message: &str) -> (bool, Option<String>) {
    let hit = message.lines().any(|line| line.starts_with(HIT));
    let expression = message
        .lines()
        .find_map(|line| Some(line.strip_prefix('{')?.strip_suffix('}')?.to_owned()));
    (hit, expression)
}

/// The log message of the "Breakpoint hit" message and an expression to log; with neither, the
/// "Breakpoint hit" message, so a breakpoint that logs says something.
fn log_message(path: &Path, row: u32, hit: bool, expression: Option<&str>) -> Arc<str> {
    let expression = expression.filter(|expression| !expression.trim().is_empty());
    let mut lines = Vec::new();
    if hit || expression.is_none() {
        lines.push(hit_message(path, row));
    }
    if let Some(expression) = expression {
        lines.push(format!("{{{}}}", expression.trim()));
    }
    lines.join("\n").into()
}

/// IntelliJ's suspend policy (`XSuspendPolicyPanel`): Suspend, then All or Thread. A breakpoint
/// that doesn't suspend logs IntelliJ's "Breakpoint hit" message.
fn suspend_policy(
    store: &Entity<BreakpointStore>,
    path: &Arc<Path>,
    row: u32,
    current: &Breakpoint,
    cx: &App,
) -> AnyElement {
    let suspends = current.message.is_none();
    let thread = current.properties.suspend_thread;
    let toggle = {
        let (store, path) = (store.clone(), path.clone());
        move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
            edit(&store, &path, row, cx, |breakpoint| {
                breakpoint.message = match breakpoint.message {
                    Some(_) => None,
                    None => Some(hit_message(&path, row).into()),
                };
            })
        }
    };
    let policy = |label: &'static str, on: bool, value: bool| match suspends {
        false => disabled_radio(
            SharedString::from(format!("suspend-{label}")),
            label,
            on,
            cx,
        )
        .into_any_element(),
        true => {
            let (store, path) = (store.clone(), path.clone());
            labeled_radio(
                SharedString::from(format!("suspend-{label}")),
                label,
                on,
                cx,
            )
            .on_click(move |_, _, cx| {
                edit(&store, &path, row, cx, |breakpoint| {
                    breakpoint.properties.suspend_thread = value
                })
            })
            .into_any_element()
        }
    };
    // Make Default (`XSuspendPolicyPanel`): shown while the policy isn't the default
    let default_thread = spork_settings::debugger::settings(cx).default_suspend_thread;
    h_flex()
        .gap(px(12.))
        .child(
            labeled_check("breakpoint-suspend", "Suspend:", suspends, cx)
                .aria_label("Suspend")
                .on_click(toggle),
        )
        .child(policy("All", !thread, false))
        .child(policy("Thread", thread, true))
        .when(suspends && thread != default_thread, |row| {
            row.child(
                spork_workbench::Button::new("breakpoint-make-default", "Make Default").on_click(
                    move |_, _, cx| {
                        let mut settings = spork_settings::debugger::settings(cx);
                        settings.default_suspend_thread = thread;
                        spork_settings::debugger::set_settings(settings, cx).log_err();
                        cx.refresh_windows();
                    },
                ),
            )
        })
        .into_any_element()
}

/// A new line breakpoint takes the default suspend policy (Make Default): the language's own
/// toggle (Kotlin's function and lambda breakpoints) is asked first, then the gutter's and Toggle
/// Line Breakpoint's new breakpoint suspends its thread when that's the default.
fn default_suspend_policy(cx: &mut App) {
    struct Wrapped;
    impl Global for Wrapped {}
    if cx.has_global::<Wrapped>() {
        return;
    }
    cx.set_global(Wrapped);
    let previous = cx.try_global::<BreakpointToggle>().cloned();
    cx.set_global(BreakpointToggle(Rc::new(
        move |editor, anchor, window, cx| {
            if let Some(BreakpointToggle(previous)) = &previous
                && previous(editor, anchor, window, cx)
            {
                return true;
            }
            if !spork_settings::debugger::settings(cx).default_suspend_thread {
                return false;
            }
            let snapshot = editor.buffer().read(cx).snapshot(cx);
            let Some((buffer_anchor, _)) = snapshot.anchor_to_buffer_anchor(anchor) else {
                return false;
            };
            let Some(buffer) = editor.buffer().read(cx).buffer(buffer_anchor.buffer_id) else {
                return false;
            };
            let (Some(store), Some(path)) = (
                editor
                    .project()
                    .map(|project| project.read(cx).breakpoint_store()),
                BreakpointStore::abs_path_from_buffer(&buffer, cx),
            ) else {
                return false;
            };
            let row = text::ToPoint::to_point(&buffer_anchor, &buffer.read(cx).snapshot()).row;
            // one there already: Zed removes it
            if store.read(cx).breakpoint_at_row(&path, row, cx).is_some() {
                return false;
            }
            let mut breakpoint = Breakpoint::new_standard();
            breakpoint.properties.suspend_thread = true;
            editor.edit_breakpoint_at_anchor(anchor, breakpoint, BreakpointEditAction::Toggle, cx);
            true
        },
    )));
}

/// "Remove once hit" (`XBreakpointActionsPanel`'s temporary breakpoint).
fn remove_once_hit(
    store: &Entity<BreakpointStore>,
    path: &Arc<Path>,
    row: u32,
    current: &Breakpoint,
    cx: &App,
) -> AnyElement {
    let (store, path) = (store.clone(), path.clone());
    labeled_check(
        "breakpoint-temporary",
        "Remove once hit",
        current.properties.temporary,
        cx,
    )
    .on_click(move |_, _, cx| {
        edit(&store, &path, row, cx, |breakpoint| {
            breakpoint.properties.temporary = !breakpoint.properties.temporary
        })
    })
    .into_any_element()
}

/// IntelliJ's dependent breakpoint (`XMasterBreakpointPanel`): "Disable until hitting the
/// following breakpoint:" `<None>` or another breakpoint, then After hit: Disable again or Leave
/// enabled.
fn master_breakpoint(
    store: &Entity<BreakpointStore>,
    path: &Arc<Path>,
    row: u32,
    current: &Breakpoint,
    cx: &App,
) -> AnyElement {
    let master = current.properties.master.clone();
    let others: Vec<(Arc<Path>, u32)> = store
        .read(cx)
        .all_source_breakpoints(cx)
        .into_iter()
        .flat_map(|(file, breakpoints)| {
            let mut rows: Vec<u32> = breakpoints.iter().map(|b| b.row).collect();
            rows.sort_unstable();
            rows.into_iter().map(move |row| (file.clone(), row))
        })
        .filter(|other| *other != (path.clone(), row))
        .collect();
    let current_name = master
        .as_ref()
        .map(|(file, row)| short_name(file, *row))
        .unwrap_or_else(|| "<None>".to_owned());
    let choose = {
        let (store, path) = (store.clone(), path.clone());
        move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
            std::iter::once(None)
                .chain(others.iter().cloned().map(Some))
                .fold(menu, |menu, choice| {
                    let label = choice
                        .as_ref()
                        .map(|(file, row)| short_name(file, *row))
                        .unwrap_or_else(|| "<None>".to_owned());
                    let (store, path) = (store.clone(), path.clone());
                    menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                        let choice = choice.clone();
                        edit(&store, &path, row, cx, |breakpoint| {
                            breakpoint.properties.master = choice
                        })
                    }))
                })
        }
    };
    let after = |label: &'static str, on: bool, leave: bool| match master.is_some() {
        false => disabled_radio(
            SharedString::from(format!("after-hit-{label}")),
            label,
            on,
            cx,
        )
        .into_any_element(),
        true => {
            let (store, path) = (store.clone(), path.clone());
            labeled_radio(
                SharedString::from(format!("after-hit-{label}")),
                label,
                on,
                cx,
            )
            .on_click(move |_, _, cx| {
                edit(&store, &path, row, cx, |breakpoint| {
                    breakpoint.properties.leave_enabled = leave
                })
            })
            .into_any_element()
        }
    };
    let leave = current.properties.leave_enabled;
    v_flex()
        .gap(px(6.))
        .child("Disable until hitting the following breakpoint:")
        .child(div().pl(px(20.)).child(labeled_combo(
            "breakpoint-master",
            "Disable until hitting the following breakpoint:",
            current_name,
            220.,
            false,
            choose,
        )))
        .child(
            h_flex()
                .pl(px(20.))
                .gap(px(12.))
                .child("After hit:")
                .child(after("Disable again", !leave, false))
                .child(after("Leave enabled", leave, true)),
        )
        .into_any_element()
}

/// A method breakpoint's or field watchpoint's Watch (`MethodBreakpointPropertiesPanel`,
/// `FieldBreakpointPropertiesPanel`): a method's entry, as java-debug stops at it, its exit greyed
/// (java-debug has no method exit) and Emulated (the line breakpoint of its first line); a
/// field's access and modification.
fn watch_panel(
    store: &Entity<BreakpointStore>,
    path: &Arc<Path>,
    row: u32,
    current: &Breakpoint,
    cx: &App,
) -> Option<AnyElement> {
    let checks = match &current.properties.kind {
        BreakpointKind::Line => return None,
        BreakpointKind::Method { .. } => vec![
            disabled_check("watch-entry", "Method entry", true, cx).into_any_element(),
            disabled_check("watch-exit", "Method exit", false, cx).into_any_element(),
            disabled_check("watch-emulated", "Emulated", true, cx).into_any_element(),
        ],
        BreakpointKind::Field {
            access,
            modification,
            ..
        } => {
            let toggle = |id: &'static str, label: &'static str, on: bool, access: bool| {
                let (store, path) = (store.clone(), path.clone());
                labeled_check(id, label, on, cx)
                    .on_click(move |_, _, cx| {
                        edit(&store, &path, row, cx, |breakpoint| {
                            if let BreakpointKind::Field {
                                access: read,
                                modification: write,
                                ..
                            } = &mut breakpoint.properties.kind
                            {
                                match access {
                                    true => *read = !*read,
                                    false => *write = !*write,
                                }
                            }
                        })
                    })
                    .into_any_element()
            };
            vec![
                toggle("watch-access", "Field access", *access, true),
                toggle(
                    "watch-modification",
                    "Field modification",
                    *modification,
                    false,
                ),
            ]
        }
    };
    Some(
        v_flex()
            .gap(px(6.))
            .child(div().pt(px(6.)).child("Watch"))
            .child(v_flex().pl(px(20.)).gap(px(6.)).children(checks))
            .into_any_element(),
    )
}

/// Edit Breakpoint: the popup of the caret line's breakpoint, under that line; false when the
/// line has none.
fn edit_at_caret(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> bool {
    let Some(editor) = workspace.active_item_as::<Editor>(cx) else {
        return false;
    };
    let Some((path, row)) = caret_line(&editor, cx) else {
        return false;
    };
    let project = workspace.project().clone();
    let store = project.read(cx).breakpoint_store();
    if breakpoint(&store, &path, row, cx).is_none() {
        return false;
    }
    let position = editor
        .update(cx, |editor, cx| {
            let snapshot = editor.snapshot(window, cx);
            let head = editor.selections.newest_anchor().head();
            let row = editor::display_map::ToDisplayPoint::to_display_point(&head, &snapshot).row();
            editor.text_origin_below_row(row)
        })
        .unwrap_or_default();
    workspace.toggle_modal(window, cx, |window, cx| {
        BreakpointPopup::new(project, path, row, position, window, cx)
    });
    true
}

/// A right click on a breakpoint: IntelliJ's popup under the pointer.
fn show_popup(
    editor: &mut Editor,
    anchor: editor::Anchor,
    position: Point<gpui::Pixels>,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> bool {
    let Some(workspace) = editor.workspace() else {
        return false;
    };
    let snapshot = editor.buffer().read(cx).snapshot(cx);
    let Some((anchor, _)) = snapshot.anchor_to_buffer_anchor(anchor) else {
        return false;
    };
    let Some(buffer) = editor.buffer().read(cx).buffer(anchor.buffer_id) else {
        return false;
    };
    let Some(path) = BreakpointStore::abs_path_from_buffer(&buffer, cx) else {
        return false;
    };
    let row = text::ToPoint::to_point(&anchor, &buffer.read(cx).snapshot()).row;
    let project = workspace.read(cx).project().clone();
    // the workspace opens it once the editor's update is over
    window.defer(cx, move |window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.toggle_modal(window, cx, |window, cx| {
                BreakpointPopup::new(project, path, row, position, window, cx)
            });
        });
    });
    true
}

/// IntelliJ's breakpoint popup: Enabled, Suspend, Condition, the More link and Done.
struct BreakpointPopup {
    project: Entity<Project>,
    path: Arc<Path>,
    row: u32,
    position: Point<gpui::Pixels>,
    condition: Entity<InputState>,
    focus: FocusHandle,
    _subscription: Subscription,
}

impl BreakpointPopup {
    fn new(
        project: Entity<Project>,
        path: Arc<Path>,
        row: u32,
        position: Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = project.read(cx).breakpoint_store();
        let text = breakpoint(&store, &path, row, cx)
            .and_then(|breakpoint| breakpoint.condition)
            .map(|condition| condition.to_string())
            .unwrap_or_default();
        let condition = cx.new(|cx| {
            let mut field = InputState::new(window, cx);
            field.set_value(text, window, cx);
            field
        });
        let _subscription = cx.subscribe_in(&condition, window, |this, _, event, _, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.done(cx);
            }
        });
        let focus = cx.focus_handle();
        condition.update(cx, |field, cx| field.focus(window, cx));
        Self {
            project,
            path,
            row,
            position,
            condition,
            focus,
            _subscription,
        }
    }

    fn store(&self, cx: &App) -> Entity<BreakpointStore> {
        self.project.read(cx).breakpoint_store()
    }

    /// Done (or ⏎): the condition as typed, then the popup closes.
    fn done(&mut self, cx: &mut Context<Self>) {
        let condition = self.condition.read(cx).value().trim().to_owned();
        edit(&self.store(cx), &self.path, self.row, cx, |breakpoint| {
            breakpoint.condition = (!condition.is_empty()).then(|| condition.into());
        });
        cx.emit(DismissEvent);
    }

    /// More (⇧⌘F8): the Breakpoints dialog on this breakpoint, in the popup's place.
    fn more(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.done(cx);
        open_dialog(
            self.project.clone(),
            Some((self.path.clone(), self.row)),
            window,
            cx,
        );
    }
}

impl EventEmitter<DismissEvent> for BreakpointPopup {}

impl ModalView for BreakpointPopup {}

impl Focusable for BreakpointPopup {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.condition.focus_handle(cx)
    }
}

impl Render for BreakpointPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let store = self.store(cx);
        let Some(current) = breakpoint(&store, &self.path, self.row, cx) else {
            cx.emit(DismissEvent);
            return div().into_any_element();
        };
        let (path, row) = (self.path.clone(), self.row);
        let enabled = current.is_enabled();
        let toggle_enabled = {
            let (store, path) = (store.clone(), path.clone());
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                edit(&store, &path, row, cx, |breakpoint| {
                    breakpoint.state = match breakpoint.state {
                        BreakpointState::Enabled => BreakpointState::Disabled,
                        BreakpointState::Disabled => BreakpointState::Enabled,
                    };
                })
            }
        };
        // the light variant (`XLightBreakpointPropertiesPanel`): the other panels when they are set
        let temporary = current
            .properties
            .temporary
            .then(|| remove_once_hit(&store, &path, row, &current, cx));
        let master = current
            .properties
            .master
            .is_some()
            .then(|| master_breakpoint(&store, &path, row, &current, cx));
        let suspend = suspend_policy(&store, &path, row, &current, cx);
        let pass_count = current.hit_condition;
        anchored()
            .position(self.position + point(px(0.), px(8.)))
            .snap_to_window_with_margin(px(8.))
            .child(
                spork_vcs::spork_look(cx).child(
                    spork_workbench::popup::surface(cx)
                        .id("breakpoint-popup")
                        .role(Role::Dialog)
                        .aria_label("Breakpoint")
                        .key_context("BreakpointPopup")
                        .track_focus(&self.focus)
                        .occlude()
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_action(cx.listener(|this, _: &menu::Cancel, _, cx| this.done(cx)))
                        .on_action(cx.listener(|this, _: &run::ViewBreakpoints, window, cx| {
                            this.more(window, cx)
                        }))
                        .w(px(360.))
                        .p(px(12.))
                        .gap(px(8.))
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::BOLD)
                                .child(short_name(&self.path, self.row)),
                        )
                        .child(
                            labeled_check("breakpoint-enabled", "Enabled", enabled, cx)
                                .on_click(toggle_enabled),
                        )
                        .child(suspend)
                        .child("Condition:")
                        .child(
                            spork_workbench::TextField::new(&self.condition)
                                .aria_label("Condition"),
                        )
                        .children(temporary)
                        .children(master)
                        .when_some(pass_count, |popup, count| {
                            popup.child(format!("Pass count: {count}"))
                        })
                        .child(
                            h_flex()
                                .justify_between()
                                .child(link("breakpoint-more", "More (⇧⌘F8)", cx).on_click(
                                    cx.listener(|this, _, window, cx| this.more(window, cx)),
                                ))
                                .child(
                                    spork_workbench::Button::new("breakpoint-done", "Done")
                                        .on_click(cx.listener(|this, _, _, cx| this.done(cx))),
                                ),
                        )
                        .text_color(tokens.text),
                ),
            )
            .into_any_element()
    }
}

/// The Breakpoints dialog's one window.
struct OpenDialog(gpui::AnyWindowHandle);

impl Global for OpenDialog {}

/// Opens the Breakpoints dialog on the project's breakpoints, `selected` (or the first) shown.
fn open_dialog(
    project: Entity<Project>,
    selected: Option<(Arc<Path>, u32)>,
    _: &mut Window,
    cx: &mut App,
) {
    if let Some(OpenDialog(handle)) = cx.try_global::<OpenDialog>().map(|d| OpenDialog(d.0))
        && handle
            .update(cx, |root, window, cx| {
                window.activate_window();
                if let Some(dialog) =
                    root.downcast::<gpui_kit::component::Root>()
                        .ok()
                        .and_then(|root| {
                            root.read(cx)
                                .view()
                                .clone()
                                .downcast::<BreakpointsDialog>()
                                .ok()
                        })
                {
                    dialog.update(cx, |dialog, cx| {
                        dialog.select(selected.clone(), window, cx);
                    });
                }
            })
            .is_ok()
    {
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(None, size(px(820.), px(540.)), cx)),
        window_min_size: Some(size(px(640.), px(400.))),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some("Breakpoints".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.), px(8.))),
        }),
        ..Default::default()
    };
    cx.defer(move |cx| {
        match gpui_kit::open_window(options, cx, move |window, cx| {
            crate::configurations_dialog::follow_ide_theme(window, cx);
            cx.new(|cx| {
                let mut dialog = BreakpointsDialog::new(project, window, cx);
                dialog.select(selected, window, cx);
                dialog
            })
        }) {
            Ok((handle, _)) => cx.set_global(OpenDialog(handle)),
            Err(error) => log::error!("failed to open Breakpoints: {error:#}"),
        }
    });
}

/// The exception filters each adapter offered when a session of it started, by the adapter.
#[derive(Default)]
struct ExceptionFilters(BTreeMap<SharedString, Vec<ExceptionBreakpointsFilter>>);

impl Global for ExceptionFilters {}

/// The adapters whose exception filters are IntelliJ's "Any exception" breakpoint: its caught and
/// uncaught filters, by the adapter.
#[derive(Default)]
struct AnyException(BTreeMap<SharedString, (String, String)>);

impl Global for AnyException {}

/// An adapter's exception filters, listed before its first session; with `any` (its caught and
/// uncaught filters), as IntelliJ's "Any exception" breakpoint with its Caught exception and
/// Uncaught exception notifications.
pub fn register_exception_filters(
    adapter: &'static str,
    filters: Vec<ExceptionBreakpointsFilter>,
    any: Option<(&str, &str)>,
    cx: &mut App,
) {
    cx.default_global::<ExceptionFilters>()
        .0
        .insert(adapter.into(), filters);
    if let Some((caught, uncaught)) = any {
        cx.default_global::<AnyException>()
            .0
            .insert(adapter.into(), (caught.into(), uncaught.into()));
    }
}

/// The selection of an adapter's "Any exception" row.
const ANY_EXCEPTION: &str = "any exception";

/// The selection of an exception breakpoint of a class: this, then its class.
const CLASS_EXCEPTION: &str = "class:";

/// A Java exception breakpoint of a class (IntelliJ's `JavaExceptionBreakpointProperties`): the
/// class's JVM name, whether it is on, its Caught and Uncaught notifications and class filters.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExceptionClassBreakpoint {
    pub class: String,
    pub enabled: bool,
    pub caught: bool,
    pub uncaught: bool,
    /// The class filters as IntelliJ's field writes them, `com.example.* -com.example.impl.*`
    /// (where the exception is thrown); `None` while "Class filters" is unchecked.
    #[serde(default)]
    pub filters: Option<String>,
}

/// An adapter's exception breakpoints beyond its filters: "Any exception"'s class filters and
/// the breakpoints of a class.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExceptionBreakpoints {
    #[serde(default)]
    pub any_filters: Option<String>,
    #[serde(default)]
    pub classes: Vec<ExceptionClassBreakpoint>,
}

/// The adapters' exception breakpoints of a class, kept for every project as Zed keeps an
/// adapter's exception filters; the adapters that send them watch this global.
#[derive(Default)]
pub struct ExceptionClasses(pub BTreeMap<SharedString, ExceptionBreakpoints>);

impl Global for ExceptionClasses {}

const EXCEPTION_CLASSES_KEY: &str = "spork_exception_breakpoints";

/// The adapters whose exception breakpoints can be of a class, with their class search.
#[derive(Default)]
struct ClassSearches(BTreeMap<SharedString, crate::class_chooser::ClassSearch>);

impl Global for ClassSearches {}

/// An adapter's exception breakpoints can be of a class, the chooser finding them by `search`.
pub fn register_exception_classes(
    adapter: &'static str,
    search: crate::class_chooser::ClassSearch,
    cx: &mut App,
) {
    cx.default_global::<ClassSearches>()
        .0
        .insert(adapter.into(), search);
}

fn has_exception_classes(adapter: &str, cx: &App) -> bool {
    cx.try_global::<ClassSearches>()
        .is_some_and(|searches| searches.0.contains_key(adapter))
}

/// An adapter's exception breakpoints of a class, read from the database the first time.
pub fn exception_breakpoints(adapter: &str, cx: &mut App) -> ExceptionBreakpoints {
    if !cx.has_global::<ExceptionClasses>() {
        let saved = db::kvp::KeyValueStore::global(cx)
            .read_kvp(EXCEPTION_CLASSES_KEY)
            .log_err()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).log_err())
            .unwrap_or_default();
        cx.set_global(ExceptionClasses(saved));
    }
    cx.global::<ExceptionClasses>()
        .0
        .get(adapter)
        .cloned()
        .unwrap_or_default()
}

/// Changes an adapter's exception breakpoints of a class, kept in the database.
fn edit_exception_breakpoints(
    adapter: &SharedString,
    change: impl FnOnce(&mut ExceptionBreakpoints),
    cx: &mut App,
) {
    let mut breakpoints = exception_breakpoints(adapter, cx);
    change(&mut breakpoints);
    let mut all = cx.global::<ExceptionClasses>().0.clone();
    all.insert(adapter.clone(), breakpoints);
    let json = serde_json::to_string(&all);
    cx.set_global(ExceptionClasses(all));
    let store = db::kvp::KeyValueStore::global(cx);
    cx.background_spawn(async move { store.write_kvp(EXCEPTION_CLASSES_KEY.into(), json?).await })
        .detach_and_log_err(cx);
}

/// Changes the exception breakpoint of `class`.
fn edit_exception_class(
    adapter: &SharedString,
    class: &str,
    change: impl FnOnce(&mut ExceptionClassBreakpoint),
    cx: &mut App,
) {
    edit_exception_breakpoints(
        adapter,
        |breakpoints| {
            if let Some(breakpoint) = breakpoints.classes.iter_mut().find(|b| b.class == class) {
                change(breakpoint)
            }
        },
        cx,
    );
}

/// Turns an adapter's filters on or off, each that isn't already.
fn set_exception_filters(
    project: &Entity<Project>,
    adapter: &SharedString,
    filters: &[(String, bool)],
    on: bool,
    cx: &mut App,
) {
    for (filter, enabled) in filters {
        if *enabled != on {
            toggle_exception_filter(project, adapter, filter, cx);
        }
    }
}

/// An adapter's exception filters as the dialog lists them: (filter, enabled), from its sessions'
/// when one runs, from the options its next sessions take otherwise.
fn exception_filters(
    project: &Entity<Project>,
    cx: &mut App,
) -> Vec<(SharedString, Vec<(ExceptionBreakpointsFilter, bool)>)> {
    let dap_store = project.read(cx).dap_store();
    let mut live: BTreeMap<SharedString, Vec<(ExceptionBreakpointsFilter, bool)>> = BTreeMap::new();
    for session in dap_store.read(cx).sessions() {
        let session = session.read(cx);
        let filters: Vec<_> = session
            .exception_breakpoints()
            .map(|(filter, enabled)| (filter.clone(), *enabled))
            .collect();
        if !filters.is_empty() && !session.is_terminated() {
            live.insert(session.adapter().0, filters);
        }
    }
    let known = &mut cx.default_global::<ExceptionFilters>().0;
    for (adapter, filters) in &live {
        known.insert(
            adapter.clone(),
            filters.iter().map(|(filter, _)| filter.clone()).collect(),
        );
    }
    let known = known.clone();
    known
        .into_iter()
        .map(|(adapter, filters)| {
            let enabled = match live.remove(&adapter) {
                Some(filters) => filters,
                None => {
                    let options = dap_store.read(cx).adapter_options(&adapter);
                    filters
                        .into_iter()
                        .map(|filter| {
                            let enabled = options
                                .as_ref()
                                .and_then(|options| {
                                    options.exception_breakpoints.get(&filter.filter)
                                })
                                .map(|persisted| persisted.enabled)
                                .unwrap_or_else(|| filter.default.unwrap_or_default());
                            (filter, enabled)
                        })
                        .collect()
                }
            };
            (adapter, enabled)
        })
        .collect()
}

/// Turns an adapter's exception filter on or off: in its sessions, and for its next ones (Zed's
/// adapter options, in the database where Zed keeps them).
fn toggle_exception_filter(
    project: &Entity<Project>,
    adapter: &SharedString,
    filter: &str,
    cx: &mut App,
) {
    let current = exception_filters(project, cx);
    let Some(filters) = current
        .iter()
        .find_map(|(name, filters)| (name == adapter).then_some(filters))
    else {
        return;
    };
    let dap_store = project.read(cx).dap_store();
    let sessions: Vec<_> = dap_store
        .read(cx)
        .sessions()
        .filter(|session| {
            let session = session.read(cx);
            &session.adapter().0 == adapter && !session.is_terminated()
        })
        .cloned()
        .collect();
    for session in &sessions {
        session.update(cx, |session, cx| {
            session.toggle_exception_breakpoint(filter, cx)
        });
    }
    let exception_breakpoints = filters
        .iter()
        .map(|(known, enabled)| {
            let enabled = match known.filter == filter {
                true => !enabled,
                false => *enabled,
            };
            (
                known.filter.clone(),
                PersistedExceptionBreakpoint { enabled },
            )
        })
        .collect();
    let options = PersistedAdapterOptions {
        exception_breakpoints,
    };
    let value = serde_json::to_string(&options);
    dap_store.update(cx, |dap_store, _| {
        dap_store.set_adapter_options(DebugAdapterName(adapter.clone()), options)
    });
    // Zed's key, which its sessions read their adapter's options from
    let key = format!("debug_adapter_`{adapter}`_persistence");
    let store = db::kvp::KeyValueStore::global(cx);
    cx.background_spawn(async move { store.write_kvp(key, value?).await })
        .detach_and_log_err(cx);
}

/// The Breakpoints dialog: the line breakpoints and the adapters' exception breakpoints under
/// their groups, with checkboxes, beside the selected one's properties; Done closes it. Changes
/// apply at once, as IntelliJ's.
struct BreakpointsDialog {
    project: Entity<Project>,
    selected: Option<(Arc<Path>, u32)>,
    /// The exception filter selected (its adapter and id), in place of a line breakpoint.
    selected_exception: Option<(SharedString, String)>,
    condition: Entity<InputState>,
    log: Entity<InputState>,
    pass_count: Entity<InputState>,
    /// The selected exception breakpoint's class filters.
    class_filters: Entity<InputState>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

/// Which field of the selected breakpoint a text box edits.
#[derive(Clone, Copy)]
enum Text {
    Condition,
    Log,
    PassCount,
    ClassFilters,
}

impl BreakpointsDialog {
    fn new(project: Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut field = |cx: &mut Context<Self>| cx.new(|cx| InputState::new(window, cx));
        let (condition, log, pass_count, class_filters) =
            (field(cx), field(cx), field(cx), field(cx));
        let store = project.read(cx).breakpoint_store();
        let dap_store = project.read(cx).dap_store();
        let mut subscriptions = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
            // the exception filters come with a session
            cx.observe(&dap_store, |_, _, cx| cx.notify()),
        ];
        for (input, text) in [
            (&condition, Text::Condition),
            (&log, Text::Log),
            (&pass_count, Text::PassCount),
            (&class_filters, Text::ClassFilters),
        ] {
            subscriptions.push(
                cx.subscribe_in(input, window, move |this, _, event, _, cx| {
                    if let InputEvent::Change = event {
                        this.apply(text, cx);
                    }
                }),
            );
        }
        // Esc reaches the dialog before a field has the focus
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            project,
            selected: None,
            selected_exception: None,
            condition,
            log,
            pass_count,
            class_filters,
            focus,
            _subscriptions: subscriptions,
        }
    }

    fn store(&self, cx: &App) -> Entity<BreakpointStore> {
        self.project.read(cx).breakpoint_store()
    }

    /// The breakpoints as listed: by file, then line.
    fn rows(&self, cx: &App) -> Vec<(Arc<Path>, u32, Breakpoint)> {
        let store = self.store(cx);
        let all = store.read(cx).all_source_breakpoints(cx);
        all.into_iter()
            .flat_map(|(path, breakpoints)| {
                let mut breakpoints = breakpoints;
                breakpoints.sort_by_key(|breakpoint| breakpoint.row);
                breakpoints.into_iter().map(move |source| {
                    (
                        path.clone(),
                        source.row,
                        Breakpoint {
                            message: source.message,
                            hit_condition: source.hit_condition,
                            condition: source.condition,
                            state: source.state,
                            properties: source.properties,
                        },
                    )
                })
            })
            .collect()
    }

    /// Shows a breakpoint's properties, its texts in the fields.
    fn select(
        &mut self,
        selected: Option<(Arc<Path>, u32)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = selected.or_else(|| {
            self.rows(cx)
                .first()
                .map(|(path, row, _)| (path.clone(), *row))
        });
        let current = selected
            .as_ref()
            .and_then(|(path, row)| breakpoint(&self.store(cx), path, *row, cx));
        let text = |value: Option<Arc<str>>| value.map(|v| v.to_string()).unwrap_or_default();
        let (condition, log, pass_count) = match current {
            Some(breakpoint) => (
                text(breakpoint.condition),
                breakpoint
                    .message
                    .and_then(|message| log_parts(&message).1)
                    .unwrap_or_default(),
                text(breakpoint.hit_condition),
            ),
            None => Default::default(),
        };
        // the texts set before the selection moves apply to the breakpoint selected
        self.selected = None;
        self.selected_exception = None;
        self.condition
            .update(cx, |field, cx| field.set_value(condition, window, cx));
        self.log
            .update(cx, |field, cx| field.set_value(log, window, cx));
        self.pass_count
            .update(cx, |field, cx| field.set_value(pass_count, window, cx));
        self.selected = selected;
        cx.notify();
    }

    /// A text box changed: the selected breakpoint takes its text.
    fn apply(&mut self, text: Text, cx: &mut Context<Self>) {
        if let Text::ClassFilters = text {
            self.apply_class_filters(None, cx);
            return;
        }
        let Some((path, row)) = self.selected.clone() else {
            return;
        };
        let value = match text {
            Text::Condition => &self.condition,
            Text::Log => &self.log,
            Text::PassCount | Text::ClassFilters => &self.pass_count,
        }
        .read(cx)
        .value()
        .trim()
        .to_owned();
        let Some(current) = breakpoint(&self.store(cx), &path, row, cx) else {
            return;
        };
        // a text applies while its box is checked: a condition or a pass count that is set, a
        // log message on a breakpoint that logs
        let value = (!value.is_empty()).then(|| Arc::<str>::from(value));
        edit(&self.store(cx), &path, row, cx, |breakpoint| match text {
            Text::Condition if current.condition.is_some() => breakpoint.condition = value,
            Text::PassCount if current.hit_condition.is_some() => breakpoint.hit_condition = value,
            Text::Log => {
                if let Some((hit, Some(_))) = current.message.as_deref().map(log_parts) {
                    breakpoint.message = Some(log_message(&path, row, hit, value.as_deref()));
                }
            }
            _ => {}
        });
    }

    /// Selects an exception breakpoint: an adapter's filter, its "Any exception" or the breakpoint
    /// of a class, its class filters in the field.
    fn select_exception(
        &mut self,
        adapter: SharedString,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let breakpoints = exception_breakpoints(&adapter, cx);
        let filters = match id.strip_prefix(CLASS_EXCEPTION) {
            Some(class) => breakpoints
                .classes
                .iter()
                .find(|breakpoint| breakpoint.class == class)
                .and_then(|breakpoint| breakpoint.filters.clone()),
            None => breakpoints.any_filters,
        };
        self.selected = None;
        self.selected_exception = None;
        self.class_filters.update(cx, |field, cx| {
            field.set_value(filters.unwrap_or_default(), window, cx)
        });
        self.selected_exception = Some((adapter, id));
        cx.notify();
    }

    /// The class filters of the selected exception breakpoint: the field's text while "Class
    /// filters" is checked; `check` turns it on or off.
    fn apply_class_filters(&mut self, check: Option<bool>, cx: &mut Context<Self>) {
        let Some((adapter, id)) = self.selected_exception.clone() else {
            return;
        };
        let typed = self.class_filters.read(cx).value().trim().to_owned();
        let set = move |filters: &mut Option<String>| match check {
            Some(true) => *filters = Some(typed),
            Some(false) => *filters = None,
            None if filters.is_some() => *filters = Some(typed),
            None => {}
        };
        match id.strip_prefix(CLASS_EXCEPTION) {
            Some(class) => edit_exception_class(
                &adapter,
                class,
                |breakpoint| set(&mut breakpoint.filters),
                cx,
            ),
            None => edit_exception_breakpoints(
                &adapter,
                |breakpoints| set(&mut breakpoints.any_filters),
                cx,
            ),
        }
        cx.notify();
    }

    /// "Class filters:" with its field, for the selected exception breakpoint.
    fn class_filters_field(&self, filters: Option<&String>, cx: &mut Context<Self>) -> AnyElement {
        let on = filters.is_some();
        v_flex()
            .gap(px(6.))
            .child(
                labeled_check("details-class-filters", "Class filters:", on, cx).on_click(
                    cx.listener(move |this, _, _, cx| this.apply_class_filters(Some(!on), cx)),
                ),
            )
            .child(div().pl(px(20.)).child(
                spork_workbench::TextField::new(&self.class_filters).aria_label("Class filters"),
            ))
            .into_any_element()
    }

    /// "Breakpoint hit" message and "Evaluate and log": each adds its part to the log message or
    /// takes it out.
    fn check_log(&mut self, hit: Option<bool>, evaluate: Option<bool>, cx: &mut Context<Self>) {
        let Some((path, row)) = self.selected.clone() else {
            return;
        };
        let typed = self.log.read(cx).value().trim().to_owned();
        edit(&self.store(cx), &path, row, cx, |breakpoint| {
            let (was_hit, expression) = match &breakpoint.message {
                Some(message) => log_parts(message),
                None => (false, None),
            };
            let hit = hit.unwrap_or(was_hit);
            let expression = match evaluate {
                Some(true) => Some(typed.clone()),
                Some(false) => None,
                None => expression,
            };
            let logs = hit || expression.as_deref().is_some_and(|e| !e.is_empty());
            breakpoint.message = (logs || evaluate == Some(true))
                .then(|| log_message(&path, row, hit, expression.as_deref()));
        });
        cx.notify();
    }

    /// A checkbox of the properties: on, its text box's text applies; off, it doesn't.
    fn check(&mut self, text: Text, on: bool, cx: &mut Context<Self>) {
        let Some((path, row)) = self.selected.clone() else {
            return;
        };
        let read = |field: &Entity<InputState>, cx: &App| {
            let value = field.read(cx).value().trim().to_owned();
            (!value.is_empty()).then(|| Arc::<str>::from(value))
        };
        let (condition, pass_count) = (read(&self.condition, cx), read(&self.pass_count, cx));
        // shortcut: a breakpoint that logs doesn't suspend (the debug adapter protocol's log
        // points), so Suspend and the log options exclude each other
        edit(&self.store(cx), &path, row, cx, |breakpoint| match text {
            Text::Condition => {
                breakpoint.condition = on.then(|| condition.unwrap_or_else(|| "true".into()))
            }
            Text::PassCount => {
                breakpoint.hit_condition = on.then(|| pass_count.unwrap_or_else(|| "1".into()))
            }
            // a breakpoint that doesn't suspend says it was hit, as IntelliJ's
            Text::Log => breakpoint.message = on.then(|| log_message(&path, row, true, None)),
            Text::ClassFilters => {}
        });
        cx.notify();
    }

    fn tree(
        &self,
        rows: &[(Arc<Path>, u32, Breakpoint)],
        exceptions: &[(SharedString, Vec<(ExceptionBreakpointsFilter, bool)>)],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = Tokens::get(cx);
        let store = self.store(cx);
        // IntelliJ's groups by breakpoint type, the line breakpoints first
        let mut groups: Vec<AnyElement> = Vec::new();
        for (group_ix, (title, icon, kind)) in [
            ("Line Breakpoints", "breakpoints/breakpoint", 0),
            ("Java Method Breakpoints", "breakpoints/breakpointMethod", 1),
            ("Java Field Watchpoints", "breakpoints/breakpointField", 2),
            // `KotlinDebuggerCoreBundle`'s `function.breakpoint.tab.title`
            (
                "Kotlin Function Breakpoints",
                "breakpoints/breakpointMethod",
                3,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let rows: Vec<_> = rows
                .iter()
                .enumerate()
                .filter(|(_, (_, _, breakpoint))| kind_index(breakpoint) == kind)
                .collect();
            if rows.is_empty() {
                continue;
            }
            let all_on = match rows.iter().filter(|(_, (_, _, b))| b.is_enabled()).count() {
                0 => Some(false),
                n if n == rows.len() => Some(true),
                _ => None,
            };
            let group_store = store.clone();
            let group_rows: Vec<_> = rows
                .iter()
                .map(|(_, (path, row, _))| (path.clone(), *row))
                .collect();
            groups.push(
                h_flex()
                    .id(("breakpoints-group", group_ix))
                    .role(Role::TreeItem)
                    .aria_label(title)
                    .aria_expanded(true)
                    .h(px(24.))
                    .px(px(6.))
                    .gap(px(4.))
                    .child(
                        spork_workbench::icon("general/chevronDown")
                            .size(px(16.))
                            .text_color(tokens.icon),
                    )
                    .child(
                        check_box(("breakpoints-group-check", group_ix), all_on, cx)
                            .role(Role::CheckBox)
                            .aria_label(title)
                            .aria_toggled(match all_on {
                                Some(true) => gpui::Toggled::True,
                                Some(false) => gpui::Toggled::False,
                                None => gpui::Toggled::Mixed,
                            })
                            .on_click(move |_, _, cx| {
                                let on = all_on != Some(true);
                                for (path, row) in &group_rows {
                                    edit(&group_store, path, *row, cx, |breakpoint| {
                                        breakpoint.state = match on {
                                            true => BreakpointState::Enabled,
                                            false => BreakpointState::Disabled,
                                        }
                                    });
                                }
                            }),
                    )
                    .child(spork_workbench::image(icon, cx).size(px(16.)))
                    .child(title)
                    .into_any_element(),
            );
            for (ix, (path, row, breakpoint)) in rows {
                let name = display_name(path, *row, breakpoint);
                let selected = self.selected.as_ref() == Some(&(path.clone(), *row));
                let (check_store, check_path, check_row) = (store.clone(), path.clone(), *row);
                let pick = (path.clone(), *row);
                let look = BreakpointLook {
                    disabled: breakpoint.is_disabled(),
                    conditional: breakpoint.condition.is_some()
                        || breakpoint.hit_condition.is_some(),
                    logs: breakpoint.message.is_some(),
                    verified: None,
                    dependent: breakpoint.properties.master.is_some(),
                    method: kind == 1,
                    field: kind == 2,
                };
                groups.push(
                    h_flex()
                        .id(("breakpoint-row", ix))
                        .role(Role::TreeItem)
                        .aria_label(SharedString::from(name.clone()))
                        .aria_selected(selected)
                        .h(px(24.))
                        .mx(px(6.))
                        .pl(px(26.))
                        .pr(px(6.))
                        .gap(px(4.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .when(selected, |row| row.bg(tokens.selection))
                        .when(!selected, |row| {
                            row.hover(move |row| row.bg(tokens.selection_hover))
                        })
                        .child(
                            check_box(("breakpoint-check", ix), Some(breakpoint.is_enabled()), cx)
                                .role(Role::CheckBox)
                                .aria_label(SharedString::from(name.clone()))
                                .aria_toggled(breakpoint.is_enabled().into())
                                .on_click(move |_, _, cx| {
                                    cx.stop_propagation();
                                    edit(&check_store, &check_path, check_row, cx, |breakpoint| {
                                        breakpoint.state = match breakpoint.state {
                                            BreakpointState::Enabled => BreakpointState::Disabled,
                                            BreakpointState::Disabled => BreakpointState::Enabled,
                                        };
                                    });
                                }),
                        )
                        .child(gutter_icon(look, false, cx))
                        .child(name)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select(Some(pick.clone()), window, cx)
                        }))
                        .into_any_element(),
                );
            }
        }
        let mut exception_groups: Vec<AnyElement> = Vec::new();
        for (group_ix, (adapter, filters)) in exceptions.iter().enumerate() {
            let title = SharedString::from(format!("{adapter} Exception Breakpoints"));
            let any = cx
                .try_global::<AnyException>()
                .and_then(|any| any.0.get(adapter).cloned());
            let classes = match any {
                Some(_) => exception_breakpoints(adapter, cx).classes,
                None => Vec::new(),
            };
            let any_on = filters.iter().any(|(_, on)| *on);
            // "Any exception" is one breakpoint, on while one of its notifications is
            let states: Vec<bool> = match any {
                Some(_) => std::iter::once(any_on)
                    .chain(classes.iter().map(|class| class.enabled))
                    .collect(),
                None => filters.iter().map(|(_, on)| *on).collect(),
            };
            let all_on = match states.iter().filter(|on| **on).count() {
                0 => Some(false),
                n if n == states.len() => Some(true),
                _ => None,
            };
            let (project, group_adapter) = (self.project.clone(), adapter.clone());
            let group_filters: Vec<(String, bool)> = filters
                .iter()
                .map(|(filter, on)| (filter.filter.clone(), *on))
                .collect();
            exception_groups.push(
                h_flex()
                    .id(("exception-breakpoints-group", group_ix))
                    .role(Role::TreeItem)
                    .aria_label(title.clone())
                    .aria_expanded(true)
                    .h(px(24.))
                    .px(px(6.))
                    .gap(px(4.))
                    .child(
                        spork_workbench::icon("general/chevronDown")
                            .size(px(16.))
                            .text_color(tokens.icon),
                    )
                    .child(
                        check_box(("exception-group-check", group_ix), all_on, cx)
                            .role(Role::CheckBox)
                            .aria_label(title.clone())
                            .aria_toggled(match all_on {
                                Some(true) => gpui::Toggled::True,
                                Some(false) => gpui::Toggled::False,
                                None => gpui::Toggled::Mixed,
                            })
                            .on_click(move |_, _, cx| {
                                let on = all_on != Some(true);
                                set_exception_filters(
                                    &project,
                                    &group_adapter,
                                    &group_filters,
                                    on,
                                    cx,
                                );
                                edit_exception_breakpoints(
                                    &group_adapter,
                                    |breakpoints| {
                                        for class in &mut breakpoints.classes {
                                            class.enabled = on;
                                        }
                                    },
                                    cx,
                                );
                            }),
                    )
                    .child(spork_workbench::image("breakpoints/breakpoint", cx).size(px(16.)))
                    .child(title)
                    .into_any_element(),
            );
            // IntelliJ's "Any exception" (`breakpoint.any.exception.display.name`): on while one
            // of its notifications is
            if any.is_some() {
                let on = any_on;
                let id = (adapter.clone(), ANY_EXCEPTION.to_owned());
                let selected = self.selected_exception.as_ref() == Some(&id);
                let (project, check_adapter) = (self.project.clone(), adapter.clone());
                let filters: Vec<(String, bool)> = filters
                    .iter()
                    .map(|(filter, on)| (filter.filter.clone(), *on))
                    .collect();
                exception_groups.push(
                    h_flex()
                        .id(SharedString::from(format!("exception-{adapter}-any")))
                        .role(Role::TreeItem)
                        .aria_label("Any exception")
                        .aria_selected(selected)
                        .h(px(24.))
                        .mx(px(6.))
                        .pl(px(26.))
                        .pr(px(6.))
                        .gap(px(4.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .when(selected, |row| row.bg(tokens.selection))
                        .when(!selected, |row| {
                            row.hover(move |row| row.bg(tokens.selection_hover))
                        })
                        .child(
                            check_box(
                                SharedString::from(format!("exception-check-{adapter}-any")),
                                Some(on),
                                cx,
                            )
                            .role(Role::CheckBox)
                            .aria_label("Any exception")
                            .aria_toggled(on.into())
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                set_exception_filters(&project, &check_adapter, &filters, !on, cx);
                            }),
                        )
                        // IntelliJ's exception breakpoint icon (`Db_exception_breakpoint`)
                        .child(
                            spork_workbench::image(
                                match on {
                                    true => "breakpoints/breakpointException",
                                    false => "breakpoints/breakpointExceptionDisabled",
                                },
                                cx,
                            )
                            .size(px(14.)),
                        )
                        .child("Any exception")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_exception(id.0.clone(), id.1.clone(), window, cx)
                        }))
                        .into_any_element(),
                );
                for (class_ix, class) in classes.into_iter().enumerate() {
                    exception_groups.push(self.exception_class_row(
                        adapter,
                        group_ix * 1000 + class_ix,
                        class,
                        cx,
                    ));
                }
                continue;
            }
            for (filter, on) in filters {
                let id = (adapter.clone(), filter.filter.clone());
                let selected = self.selected_exception.as_ref() == Some(&id);
                let label = SharedString::from(filter.label.clone());
                let (project, check_adapter, check_filter) =
                    (self.project.clone(), adapter.clone(), filter.filter.clone());
                exception_groups.push(
                    h_flex()
                        .id(SharedString::from(format!(
                            "exception-{adapter}-{}",
                            filter.filter
                        )))
                        .role(Role::TreeItem)
                        .aria_label(label.clone())
                        .aria_selected(selected)
                        .h(px(24.))
                        .mx(px(6.))
                        .pl(px(26.))
                        .pr(px(6.))
                        .gap(px(4.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .when(selected, |row| row.bg(tokens.selection))
                        .when(!selected, |row| {
                            row.hover(move |row| row.bg(tokens.selection_hover))
                        })
                        .child(
                            check_box(
                                SharedString::from(format!(
                                    "exception-check-{adapter}-{}",
                                    filter.filter
                                )),
                                Some(*on),
                                cx,
                            )
                            .role(Role::CheckBox)
                            .aria_label(label.clone())
                            .aria_toggled((*on).into())
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                toggle_exception_filter(
                                    &project,
                                    &check_adapter,
                                    &check_filter,
                                    cx,
                                );
                            }),
                        )
                        .child(
                            spork_workbench::image(
                                match on {
                                    true => "breakpoints/breakpoint",
                                    false => "breakpoints/breakpointDisabled",
                                },
                                cx,
                            )
                            .size(px(14.)),
                        )
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.selected = None;
                            this.selected_exception = Some(id.clone());
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
        }
        v_flex()
            .id("breakpoints-tree")
            .role(Role::Tree)
            .aria_label("Breakpoints")
            .flex_1()
            .py(px(4.))
            .overflow_y_scroll()
            .children(groups)
            .children(exception_groups)
            .into_any_element()
    }

    /// An exception breakpoint of a class under its adapter's group: its check, its icon and its
    /// class (`JavaExceptionBreakpointType`'s display text).
    fn exception_class_row(
        &self,
        adapter: &SharedString,
        ix: usize,
        class: ExceptionClassBreakpoint,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = Tokens::get(cx);
        let id = (adapter.clone(), format!("{CLASS_EXCEPTION}{}", class.class));
        let selected = self.selected_exception.as_ref() == Some(&id);
        let label = SharedString::from(class.class.clone());
        let (check_adapter, check_class) = (adapter.clone(), class.class.clone());
        h_flex()
            .id(("exception-class", ix))
            .role(Role::TreeItem)
            .aria_label(label.clone())
            .aria_selected(selected)
            .h(px(24.))
            .mx(px(6.))
            .pl(px(26.))
            .pr(px(6.))
            .gap(px(4.))
            .rounded(px(4.))
            .cursor_pointer()
            .when(selected, |row| row.bg(tokens.selection))
            .when(!selected, |row| {
                row.hover(move |row| row.bg(tokens.selection_hover))
            })
            .child(
                check_box(("exception-class-check", ix), Some(class.enabled), cx)
                    .role(Role::CheckBox)
                    .aria_label(label.clone())
                    .aria_toggled(class.enabled.into())
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        edit_exception_class(
                            &check_adapter,
                            &check_class,
                            |b| b.enabled = !b.enabled,
                            cx,
                        )
                    }),
            )
            .child(
                spork_workbench::image(
                    match class.enabled {
                        true => "breakpoints/breakpointException",
                        false => "breakpoints/breakpointExceptionDisabled",
                    },
                    cx,
                )
                .size(px(14.)),
            )
            .child(label)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_exception(id.0.clone(), id.1.clone(), window, cx)
            }))
            .into_any_element()
    }

    /// An exception breakpoint of a class's properties (`ExceptionBreakpointPropertiesPanel`):
    /// "Exception breakpoint for …", Enabled, Class filters, then its Notifications, Caught exception
    /// and Uncaught exception, one of which stays checked as IntelliJ's does.
    fn class_exception_details(
        &self,
        adapter: SharedString,
        class: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let breakpoint = exception_breakpoints(&adapter, cx)
            .classes
            .into_iter()
            .find(|breakpoint| breakpoint.class == class)?;
        let title = format!("Exception breakpoint for {}", breakpoint.class);
        let edit_with = |change: fn(&mut ExceptionClassBreakpoint)| {
            let (adapter, class) = (adapter.clone(), breakpoint.class.clone());
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                edit_exception_class(&adapter, &class, change, cx)
            }
        };
        let filters = self.class_filters_field(breakpoint.filters.as_ref(), cx);
        Some(
            v_flex()
                .id("breakpoint-details")
                .role(Role::Group)
                .aria_label("Properties")
                .flex_1()
                .min_w_0()
                .p(px(16.))
                .gap(px(10.))
                .child(
                    div()
                        .id("breakpoint-title")
                        .role(Role::Heading)
                        .aria_label(SharedString::from(title.clone()))
                        .font_weight(gpui::FontWeight::BOLD)
                        .child(title),
                )
                .child(
                    labeled_check("details-enabled", "Enabled", breakpoint.enabled, cx)
                        .on_click(edit_with(|b| b.enabled = !b.enabled)),
                )
                .child(filters)
                .child(div().pt(px(6.)).child("Notifications"))
                .child(
                    v_flex()
                        .pl(px(20.))
                        .gap(px(6.))
                        .child(
                            labeled_check(
                                "details-caught",
                                "Caught exception",
                                breakpoint.caught,
                                cx,
                            )
                            .on_click(edit_with(|b| {
                                b.caught = !b.caught;
                                b.uncaught |= !b.caught;
                            })),
                        )
                        .child(
                            labeled_check(
                                "details-uncaught",
                                "Uncaught exception",
                                breakpoint.uncaught,
                                cx,
                            )
                            .on_click(edit_with(|b| {
                                b.uncaught = !b.uncaught;
                                b.caught |= !b.uncaught;
                            })),
                        ),
                )
                .into_any_element(),
        )
    }

    /// "Any exception"'s properties (IntelliJ's exception breakpoint panel): Enabled and its
    /// Notifications, Caught exception and Uncaught exception, each one of the adapter's filters.
    /// A breakpoint that is off shows both notifications, as IntelliJ's new one has them.
    // shortcut: Enabled is its notifications' being on, so turning it off forgets which were;
    // keep the breakpoint's own state once the dialog persists its own breakpoints
    fn any_exception_details(
        &self,
        exceptions: &[(SharedString, Vec<(ExceptionBreakpointsFilter, bool)>)],
        adapter: SharedString,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (caught, uncaught) = cx.try_global::<AnyException>()?.0.get(&adapter)?.clone();
        let filters: Vec<(String, bool)> = exceptions
            .iter()
            .find(|(name, _)| name == &adapter)?
            .1
            .iter()
            .map(|(filter, on)| (filter.filter.clone(), *on))
            .collect();
        let on = |id: &str| filters.iter().any(|(filter, on)| filter == id && *on);
        let enabled = on(&caught) || on(&uncaught);
        let notification = |label: &'static str, filter: String, cx: &mut Context<Self>| {
            let id = SharedString::from(format!("details-{filter}"));
            if !enabled {
                return disabled_check(id, label, true, cx).into_any_element();
            }
            let (project, adapter, checked) = (self.project.clone(), adapter.clone(), on(&filter));
            labeled_check(id, label, checked, cx)
                .on_click(move |_, _, cx| toggle_exception_filter(&project, &adapter, &filter, cx))
                .into_any_element()
        };
        let toggle_enabled = {
            let (project, adapter, filters) =
                (self.project.clone(), adapter.clone(), filters.clone());
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                set_exception_filters(&project, &adapter, &filters, !enabled, cx)
            }
        };
        // where the adapter takes them (its exceptions can be of a class)
        let class_filters = has_exception_classes(&adapter, cx).then(|| {
            let any_filters = exception_breakpoints(&adapter, cx).any_filters;
            self.class_filters_field(any_filters.as_ref(), cx)
        });
        Some(
            v_flex()
                .id("breakpoint-details")
                .role(Role::Group)
                .aria_label("Properties")
                .flex_1()
                .min_w_0()
                .p(px(16.))
                .gap(px(10.))
                .child(
                    // `exception.breakpoint.description.any`
                    div()
                        .id("breakpoint-title")
                        .role(Role::Heading)
                        .aria_label("Any exception breakpoint")
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("Any exception breakpoint"),
                )
                .child(
                    labeled_check("details-enabled", "Enabled", enabled, cx)
                        .on_click(toggle_enabled),
                )
                .children(class_filters)
                .child(div().pt(px(6.)).child("Notifications"))
                .child(
                    v_flex()
                        .pl(px(20.))
                        .gap(px(6.))
                        .child(notification("Caught exception", caught, cx))
                        .child(notification("Uncaught exception", uncaught, cx)),
                )
                .into_any_element(),
        )
    }

    /// An exception filter's properties: its title and Enabled.
    fn exception_details(
        &self,
        exceptions: &[(SharedString, Vec<(ExceptionBreakpointsFilter, bool)>)],
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (adapter, id) = self.selected_exception.clone()?;
        if id == ANY_EXCEPTION {
            return self.any_exception_details(exceptions, adapter, cx);
        }
        if let Some(class) = id.strip_prefix(CLASS_EXCEPTION) {
            return self.class_exception_details(adapter, class, cx);
        }
        let (filter, on) = exceptions
            .iter()
            .find(|(name, _)| name == &adapter)?
            .1
            .iter()
            .find(|(filter, _)| filter.filter == id)?
            .clone();
        let project = self.project.clone();
        Some(
            v_flex()
                .id("breakpoint-details")
                .role(Role::Group)
                .aria_label("Properties")
                .flex_1()
                .min_w_0()
                .p(px(16.))
                .gap(px(10.))
                .child(
                    div()
                        .id("breakpoint-title")
                        .role(Role::Heading)
                        .aria_label(SharedString::from(filter.label.clone()))
                        .font_weight(gpui::FontWeight::BOLD)
                        .child(filter.label),
                )
                .child(
                    labeled_check("details-enabled", "Enabled", on, cx).on_click(
                        move |_, _, cx| toggle_exception_filter(&project, &adapter, &id, cx),
                    ),
                )
                .into_any_element(),
        )
    }

    fn details(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let current = self.selected.as_ref().and_then(|(path, row)| {
            breakpoint(&self.store(cx), path, *row, cx).map(|b| (path.clone(), *row, b))
        });
        let Some((path, row, current)) = current else {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(tokens.text_secondary)
                .child("Select a breakpoint")
                .into_any_element();
        };
        let store = self.store(cx);
        let toggle_enabled = {
            let (store, path) = (store.clone(), path.clone());
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                edit(&store, &path, row, cx, |breakpoint| {
                    breakpoint.state = match breakpoint.state {
                        BreakpointState::Enabled => BreakpointState::Disabled,
                        BreakpointState::Disabled => BreakpointState::Enabled,
                    };
                })
            }
        };
        let suspend = suspend_policy(&store, &path, row, &current, cx);
        let temporary = remove_once_hit(&store, &path, row, &current, cx);
        let master = master_breakpoint(&store, &path, row, &current, cx);
        let watch = watch_panel(&store, &path, row, &current, cx);
        let title = match &current.properties.kind {
            BreakpointKind::Line => format!("{}, Line breakpoint", short_name(&path, row)),
            // `KotlinDebuggerCoreBundle`'s `function.breakpoint.description`
            BreakpointKind::Method {
                name,
                function: Some(_),
            } => format!("{name}, Function breakpoint"),
            BreakpointKind::Method { name, .. } => format!("{name}, Method breakpoint"),
            BreakpointKind::Field { name, .. } => format!("{name}, Field watchpoint"),
        };
        let has_condition = current.condition.is_some();
        let (hit, evaluates) = match &current.message {
            Some(message) => {
                let (hit, expression) = log_parts(message);
                (hit, expression.is_some())
            }
            None => (false, false),
        };
        let counts = current.hit_condition.is_some();
        v_flex()
            .id("breakpoint-details")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .p(px(16.))
            .gap(px(10.))
            .child(
                div()
                    .id("breakpoint-title")
                    .role(Role::Heading)
                    .aria_label(SharedString::from(title.clone()))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(title),
            )
            .child(
                labeled_check("details-enabled", "Enabled", current.is_enabled(), cx)
                    .on_click(toggle_enabled),
            )
            .child(suspend)
            .child(
                labeled_check("details-condition", "Condition:", has_condition, cx).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.check(Text::Condition, !has_condition, cx)
                    }),
                ),
            )
            .child(
                div().pl(px(20.)).child(
                    spork_workbench::TextField::new(&self.condition).aria_label("Condition"),
                ),
            )
            .child(h_flex().gap(px(12.)).child("Log:").child(
                labeled_check("details-log", "\"Breakpoint hit\" message", hit, cx).on_click(
                    cx.listener(move |this, _, _, cx| this.check_log(Some(!hit), None, cx)),
                ),
            ))
            .child(
                labeled_check("details-evaluate-log", "Evaluate and log:", evaluates, cx).on_click(
                    cx.listener(move |this, _, _, cx| this.check_log(None, Some(!evaluates), cx)),
                ),
            )
            .child(
                div().pl(px(20.)).child(
                    spork_workbench::TextField::new(&self.log).aria_label("Evaluate and log"),
                ),
            )
            .child(temporary)
            .child(master)
            .child(
                labeled_check("details-pass-count", "Pass count:", counts, cx).on_click(
                    cx.listener(move |this, _, _, cx| this.check(Text::PassCount, !counts, cx)),
                ),
            )
            .child(
                div().pl(px(20.)).child(
                    spork_workbench::TextField::new(&self.pass_count).aria_label("Pass count"),
                ),
            )
            .children(watch)
            .into_any_element()
    }
}

impl BreakpointsDialog {
    /// The toolbar's + (`XBreakpointsDialog`'s add action): the breakpoint types added here, an
    /// exception breakpoint of a class through IntelliJ's class chooser; line breakpoints are set
    /// in the gutter, method breakpoints and field watchpoints in the editor (their Add dialogs
    /// aren't built: greyed).
    fn add_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let adapters: Vec<SharedString> = cx
            .try_global::<ClassSearches>()
            .map(|searches| searches.0.keys().cloned().collect())
            .unwrap_or_default();
        let java = adapters.iter().any(|adapter| adapter == "Java");
        let (project, this) = (self.project.clone(), cx.entity().downgrade());
        ActionButton::new("breakpoints-add", "general/add")
            .tooltip("Add")
            .disabled(adapters.is_empty())
            .dropdown_menu(move |menu, _, _| {
                let greyed = |label: &'static str| PopupMenuItem::new(label).disabled(true);
                let menu = match java {
                    true => menu
                        .item(greyed("Java Method Breakpoints"))
                        .item(greyed("Java Field Watchpoints")),
                    false => menu,
                };
                adapters.iter().fold(menu, |menu, adapter| {
                    let (adapter, project, this) = (adapter.clone(), project.clone(), this.clone());
                    menu.item(
                        PopupMenuItem::new(format!("{adapter} Exception Breakpoints")).on_click(
                            move |_, _, cx| {
                                add_exception_class(&adapter, &project, this.clone(), cx)
                            },
                        ),
                    )
                })
            })
    }
}

/// + ▸ Java Exception Breakpoints: IntelliJ's "Enter Exception Class" chooser, then the class's
/// breakpoint (both notifications on, as IntelliJ makes it) selected in the dialog.
fn add_exception_class(
    adapter: &SharedString,
    project: &Entity<Project>,
    dialog: gpui::WeakEntity<BreakpointsDialog>,
    cx: &mut App,
) {
    let Some(search) = cx
        .try_global::<ClassSearches>()
        .and_then(|searches| searches.0.get(adapter).cloned())
    else {
        return;
    };
    let adapter = adapter.clone();
    let chosen = move |class: String, cx: &mut App| {
        edit_exception_breakpoints(
            &adapter,
            |breakpoints| {
                if !breakpoints.classes.iter().any(|b| b.class == class) {
                    breakpoints.classes.push(ExceptionClassBreakpoint {
                        class: class.clone(),
                        enabled: true,
                        caught: true,
                        uncaught: true,
                        filters: None,
                    });
                }
            },
            cx,
        );
        let Some(dialog) = dialog.upgrade() else {
            return;
        };
        let id = format!("{CLASS_EXCEPTION}{class}");
        let adapter = adapter.clone();
        if let Some(OpenDialog(handle)) = cx.try_global::<OpenDialog>().map(|d| OpenDialog(d.0)) {
            handle
                .update(cx, |_, window, cx| {
                    window.activate_window();
                    dialog.update(cx, |dialog, cx| {
                        dialog.select_exception(adapter, id, window, cx)
                    })
                })
                .log_err();
        }
    };
    crate::class_chooser::open(
        "Enter Exception Class",
        project.clone(),
        search,
        Rc::new(chosen),
        cx,
    );
}

impl Focusable for BreakpointsDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for BreakpointsDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let rows = self.rows(cx);
        if self.selected_exception.is_none()
            && self.selected.as_ref().is_some_and(|selected| {
                !rows
                    .iter()
                    .any(|(path, row, _)| (path, row) == (&selected.0, &selected.1))
            })
        {
            self.selected = rows.first().map(|(path, row, _)| (path.clone(), *row));
        }
        let selected = self.selected.clone();
        // a breakpoint of a class goes; "Any exception" and an adapter's filters stay
        let class = self.selected_exception.as_ref().and_then(|(adapter, id)| {
            let class = id.strip_prefix(CLASS_EXCEPTION)?;
            Some((adapter.clone(), class.to_owned()))
        });
        let store = self.store(cx);
        let exceptions = exception_filters(&self.project, cx);
        let tree = self.tree(&rows, &exceptions, cx);
        let details = match self.exception_details(&exceptions, cx) {
            Some(details) => details,
            None => self.details(cx),
        };
        v_flex()
            .id("breakpoints-dialog")
            .key_context("BreakpointsDialog")
            .track_focus(&self.focus)
            .size_full()
            .bg(tokens.panel)
            .text_color(tokens.text)
            .on_key_down(|event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    window.remove_window();
                    cx.stop_propagation();
                }
            })
            .child(spork_workbench::shell::window_title("Breakpoints", cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(
                        v_flex()
                            .w(px(300.))
                            .flex_none()
                            .h_full()
                            .border_r_1()
                            .border_color(tokens.separator)
                            .child(
                                h_flex()
                                    .id("breakpoints-toolbar")
                                    .role(Role::Toolbar)
                                    .aria_label("Breakpoints Toolbar")
                                    .flex_none()
                                    .h(px(32.))
                                    .px(px(6.))
                                    .gap(px(4.))
                                    .child(self.add_button(cx))
                                    .child(
                                        ActionButton::new("breakpoints-remove", "general/remove")
                                            .tooltip("Remove")
                                            .disabled(selected.is_none() && class.is_none())
                                            .on_click(move |_, _, cx| {
                                                if let Some((path, row)) = &selected {
                                                    remove(&store, path, *row, cx);
                                                }
                                                if let Some((adapter, class)) = &class {
                                                    edit_exception_breakpoints(
                                                        adapter,
                                                        |breakpoints| {
                                                            breakpoints
                                                                .classes
                                                                .retain(|b| &b.class != class)
                                                        },
                                                        cx,
                                                    );
                                                }
                                            }),
                                    ),
                            )
                            .child(tree),
                    )
                    .child(details),
            )
            .child(
                h_flex()
                    .flex_none()
                    .justify_end()
                    .px(px(12.))
                    .py(px(8.))
                    .border_t_1()
                    .border_color(tokens.separator)
                    .child(
                        spork_workbench::Button::new("breakpoints-done", "Done")
                            .primary(true)
                            .on_click(|_, window, _| window.remove_window()),
                    ),
            )
    }
}

/// The project's breakpoints, for the end-to-end tests: `greeting.rs:2` and how each stands.
pub(crate) fn e2e_breakpoints(workspace: &Workspace, cx: &App) -> Value {
    let store = workspace.project().read(cx).breakpoint_store();
    let all = store.read(cx).all_source_breakpoints(cx);
    all.into_iter()
        .flat_map(|(path, mut breakpoints)| {
            breakpoints.sort_by_key(|breakpoint| breakpoint.row);
            breakpoints.into_iter().map(move |breakpoint| {
                json!({
                    "at": short_name(&path, breakpoint.row),
                    "enabled": breakpoint.state.is_enabled(),
                    "condition": breakpoint.condition.as_deref(),
                    "log": breakpoint.message.as_deref(),
                    "pass_count": breakpoint.hit_condition.as_deref(),
                    "suspend": match (breakpoint.message.is_none(), breakpoint.properties.suspend_thread) {
                        (false, _) => "none",
                        (true, false) => "all",
                        (true, true) => "thread",
                    },
                    "temporary": breakpoint.properties.temporary,
                    "depends_on": breakpoint.properties.master.as_ref().map(|(path, row)| short_name(path, *row)),
                    "kind": match &breakpoint.properties.kind {
                        BreakpointKind::Line => "line",
                        BreakpointKind::Method { .. } => "method",
                        BreakpointKind::Field { .. } => "field",
                    },
                    "name": match &breakpoint.properties.kind {
                        BreakpointKind::Line => None,
                        BreakpointKind::Method { name, .. } | BreakpointKind::Field { name, .. } => Some(name.as_str()),
                    },
                    "function": breakpoint.properties.function(),
                    "columns": breakpoint.properties.columns,
                })
            })
        })
        .collect()
}

/// The running sessions' exception filters, for the end-to-end tests: by adapter, each filter's
/// label and whether it is on.
pub(crate) fn e2e_exceptions(workspace: &Workspace, cx: &App) -> Value {
    let dap_store = workspace.project().read(cx).dap_store();
    let sessions: serde_json::Map<String, Value> = dap_store
        .read(cx)
        .sessions()
        .map(|session| session.read(cx))
        .filter(|session| !session.is_terminated())
        .map(|session| {
            let filters: serde_json::Map<String, Value> = session
                .exception_breakpoints()
                .map(|(filter, enabled)| (filter.label.clone(), (*enabled).into()))
                .collect();
            (session.adapter().0.to_string(), filters.into())
        })
        .collect();
    sessions.into()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{log_message, log_parts, short_name};

    #[test]
    fn a_log_message_says_the_hit_and_logs_an_expression() {
        let path = Path::new("/p/src/Main.java");
        let both = log_message(path, 4, true, Some("name.length()"));
        assert_eq!(&*both, "Breakpoint reached at Main.java:5\n{name.length()}");
        assert_eq!(log_parts(&both), (true, Some("name.length()".into())));
        let expression = log_message(path, 4, false, Some("x"));
        assert_eq!(log_parts(&expression), (false, Some("x".into())));
        // with nothing to log, the "Breakpoint hit" message
        assert_eq!(
            log_parts(&log_message(path, 4, false, Some(" "))),
            (true, None)
        );
    }

    #[test]
    fn a_breakpoint_is_named_by_its_file_and_line() {
        assert_eq!(
            short_name(Path::new("/p/src/greeting.rs"), 1),
            "greeting.rs:2"
        );
    }
}
