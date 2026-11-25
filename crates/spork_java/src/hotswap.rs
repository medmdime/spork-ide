//! HotSwap's floating toolbar (IntelliJ's `HotSwapFloatingToolbarProvider`, "Suggest HotSwap in
//! the editor when code is modified"): while a Java debug session runs and a Java file of its
//! project has changed since the classes were last reloaded, Java editors show "Code changed:"
//! with Apply HotSwap and Hide at their top right. Apply HotSwap saves the changed files, then
//! Reload Changed Classes builds and reloads them, and the toolbar says "Code has been reloaded"
//! for a moment.

use std::time::{Duration, Instant};

use collections::{HashMap, HashSet};
use editor::Editor;
use gpui::{
    AnyElement, App, Context, Entity, EntityId, Global, InteractiveElement as _, IntoElement as _,
    ParentElement as _, Role, StatefulInteractiveElement as _, Styled as _, TaskExt as _, Window,
    div, px,
};
use language::Buffer;
use project::Project;
use spork_workbench::{ActionButton, Tokens};
use text::BufferId;
use workspace::Workspace;

use crate::debugger::{ProxyHandle, proxy_of};

/// How long "Code has been reloaded" shows.
const SUCCESS_SHOWN: Duration = Duration::from_secs(3);

/// Each project's Java files as they were when its classes were last reloaded (or its session
/// started), and when that was.
#[derive(Default)]
struct Baselines(HashMap<EntityId, Baseline>);

impl Global for Baselines {}

struct Baseline {
    buffers: HashMap<BufferId, text::BufferSnapshot>,
    reloaded: Option<Instant>,
}

pub(crate) fn init(cx: &mut App) {
    spork_editor::add_floating_toolbar(std::rc::Rc::new(toolbar), cx);
}

fn is_java(buffer: &Buffer, cx: &App) -> bool {
    buffer
        .file()
        .is_some_and(|file| file.file_name(cx).ends_with(".java"))
}

/// The project's Java files as they are now count as unchanged: when its session starts, or its
/// classes are reloaded (`reloaded`).
pub(crate) fn mark_unchanged(project: &Entity<Project>, reloaded: bool, cx: &mut App) {
    let buffers = project
        .read(cx)
        .buffer_store()
        .read(cx)
        .buffers()
        .filter(|buffer| is_java(buffer.read(cx), cx))
        .map(|buffer| {
            let buffer = buffer.read(cx);
            (buffer.remote_id(), buffer.text_snapshot())
        })
        .collect();
    cx.default_global::<Baselines>().0.insert(
        project.entity_id(),
        Baseline {
            buffers,
            reloaded: reloaded.then(Instant::now),
        },
    );
}

/// The project's Java files changed since its baseline.
fn changed(project: &Entity<Project>, cx: &App) -> Vec<Entity<Buffer>> {
    let Some(baseline) = cx
        .try_global::<Baselines>()
        .and_then(|baselines| baselines.0.get(&project.entity_id()))
    else {
        return Vec::new();
    };
    project
        .read(cx)
        .buffer_store()
        .read(cx)
        .buffers()
        .filter(|buffer| {
            let buffer = buffer.read(cx);
            is_java(buffer, cx)
                && match baseline.buffers.get(&buffer.remote_id()) {
                    Some(before) => buffer.text_snapshot().has_edits_since(before.version()),
                    None => buffer.is_dirty(),
                }
        })
        .collect()
}

/// The running Java debug session's proxy in a project.
fn java_session(project: &Entity<Project>, cx: &App) -> Option<ProxyHandle> {
    project
        .read(cx)
        .dap_store()
        .read(cx)
        .sessions()
        .filter(|session| !session.read(cx).is_terminated())
        .find_map(|session| proxy_of(session.read(cx), cx))
}

/// What the end-to-end tests read: the toolbar's text in the active editor, if it shows.
pub(crate) fn e2e_state(workspace: &Workspace, cx: &App) -> Option<&'static str> {
    let project = workspace.project();
    java_session(project, cx)?;
    let reloaded = cx
        .try_global::<Baselines>()
        .and_then(|baselines| baselines.0.get(&project.entity_id()))
        .and_then(|baseline| baseline.reloaded)
        .is_some_and(|at| at.elapsed() < SUCCESS_SHOWN);
    match (reloaded, changed(project, cx).is_empty()) {
        (true, _) => Some("Code has been reloaded"),
        (false, false) => Some("Code changed:"),
        (false, true) => None,
    }
}

fn toolbar(
    editor: &mut Editor,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> Option<AnyElement> {
    if !spork_settings::debugger::settings(cx).suggest_hotswap {
        return None;
    }
    let buffer = editor.buffer().read(cx).as_singleton()?;
    if !is_java(buffer.read(cx), cx) {
        return None;
    }
    let workspace = editor.workspace()?;
    let project = workspace.read(cx).project().clone();
    let proxy = java_session(&project, cx)?;
    let tokens = Tokens::get(cx);
    let reloaded_at = cx
        .try_global::<Baselines>()
        .and_then(|baselines| baselines.0.get(&project.entity_id()))
        .and_then(|baseline| baseline.reloaded);
    let reloaded = reloaded_at.is_some_and(|at| at.elapsed() < SUCCESS_SHOWN);
    let changes = changed(&project, cx);
    if !reloaded && changes.is_empty() {
        return None;
    }
    if let Some(at) = reloaded_at.filter(|_| reloaded) {
        // the success hides by itself
        let left = SUCCESS_SHOWN.saturating_sub(at.elapsed());
        cx.spawn_in(window, async move |editor, cx| {
            cx.background_executor().timer(left).await;
            editor.update(cx, |_, cx| cx.notify())
        })
        .detach_and_log_err(cx);
    }
    let surface = div()
        .id("hotswap-toolbar")
        .role(Role::Toolbar)
        .aria_label("Hot Swap")
        .flex()
        .items_center()
        .gap(px(2.))
        .p(px(2.))
        .rounded(px(spork_workbench::tokens::metrics::RADIUS))
        .bg(tokens.popup)
        .border_1()
        .border_color(tokens.popup_border)
        .text_color(tokens.text);
    if reloaded {
        return Some(
            surface
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .px(px(6.))
                        .py(px(4.))
                        .child("Code has been reloaded")
                        .child(spork_workbench::image("status/success", cx).size(px(16.))),
                )
                .into_any_element(),
        );
    }
    let apply = {
        let (workspace, project) = (workspace.downgrade(), project.clone());
        move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
            let buffers: HashSet<_> = changed(&project, cx).into_iter().collect();
            let save = project.update(cx, |project, cx| project.save_buffers(buffers, cx));
            let (workspace, proxy) = (workspace.clone(), proxy.clone());
            cx.spawn(async move |cx| {
                save.await?;
                workspace.update(cx, |workspace, cx| {
                    crate::debugger::reload_changed_classes(workspace, proxy, cx)
                })
            })
            .detach_and_log_err(cx);
        }
    };
    let hide = move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
        mark_unchanged(&project, false, cx)
    };
    Some(
        surface
            .child(
                div()
                    .id("hotswap-apply")
                    .role(Role::Button)
                    .aria_label("Code changed:")
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(6.))
                    .py(px(4.))
                    .rounded(px(spork_workbench::tokens::metrics::RADIUS))
                    .cursor_pointer()
                    .hover(|button| button.bg(tokens.editor_button_hover))
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new("Apply HotSwap")
                            .build(window, cx)
                    })
                    .child("Code changed:")
                    .child(spork_workbench::image("debugger/debuggerSync", cx).size(px(16.)))
                    .on_click(apply),
            )
            .child(
                ActionButton::new("hotswap-hide", "general/close")
                    .tooltip("Hide")
                    .on_click(hide),
            )
            .into_any_element(),
    )
}
