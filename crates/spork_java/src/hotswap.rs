//! HotSwap's floating toolbar (IntelliJ's `HotSwapFloatingToolbarProvider`, "Suggest HotSwap in
//! the editor when code is modified"): while a Java debug session runs and a Java file of its
//! project has changed since the classes were last reloaded, Java editors show "Code changed:"
//! with Apply HotSwap and Hide at their top right. Apply HotSwap saves the changed files, then
//! Reload Changed Classes builds and reloads them, and the toolbar says "Code has been reloaded"
//! for a moment. A change HotSwap can't apply (a class's shape changed: a method or field added,
//! removed or retyped, its supertypes; or code that doesn't parse) shows Restart Debug instead, as
//! IntelliJ's `JvmBaseSourceFileChangeCompatibilityChecker` finds them.

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

/// Whether a Java file of the project changed since its classes were last reloaded.
pub(crate) fn has_changes(project: &Entity<Project>, cx: &App) -> bool {
    !changed(project, cx).is_empty()
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

/// A Java file's class shapes (`HotSwapClassShape`): each class with its supertypes and its
/// members' declarations without their bodies; `None` when the file doesn't parse.
fn class_shapes(text: &str) -> Option<Vec<String>> {
    let mut parser = language::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .ok()?;
    let tree = parser.parse(text, None)?;
    if tree.root_node().has_error() {
        return None;
    }
    let source = text.as_bytes();
    let declaration = |node: language::Node| {
        // the declaration up to its body (a method's block, a class's members)
        let end = node
            .child_by_field_name("body")
            .map_or(node.end_byte(), |body| body.start_byte());
        let text = text.get(node.start_byte()..end).unwrap_or_default();
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let mut shapes = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "annotation_type_declaration" => shapes.push(declaration(node)),
            "method_declaration"
            | "constructor_declaration"
            | "compact_constructor_declaration"
            | "field_declaration"
            | "enum_constant" => {
                // a field's initializer is code: its declarators' names stay
                let shape = match node.kind() {
                    "field_declaration" => {
                        let mut cursor = node.walk();
                        let names: Vec<_> = node
                            .children_by_field_name("declarator", &mut cursor)
                            .filter_map(|declarator| declarator.child_by_field_name("name"))
                            .filter_map(|name| name.utf8_text(source).ok())
                            .collect();
                        let ty = node
                            .child_by_field_name("type")
                            .and_then(|ty| ty.utf8_text(source).ok())
                            .unwrap_or_default();
                        format!("field {ty} {}", names.join(","))
                    }
                    "enum_constant" => format!(
                        "constant {}",
                        node.child_by_field_name("name")
                            .and_then(|name| name.utf8_text(source).ok())
                            .unwrap_or_default()
                    ),
                    _ => declaration(node),
                };
                shapes.push(shape);
            }
            _ => {}
        }
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }
    shapes.sort();
    Some(shapes)
}

/// The changes HotSwap can't apply: a changed file whose classes changed shape, or which
/// doesn't parse.
fn not_hot_swappable(project: &Entity<Project>, cx: &App) -> bool {
    let Some(baseline) = cx
        .try_global::<Baselines>()
        .and_then(|baselines| baselines.0.get(&project.entity_id()))
    else {
        return false;
    };
    changed(project, cx).iter().any(|buffer| {
        let buffer = buffer.read(cx);
        let now = class_shapes(&buffer.text());
        let before = baseline
            .buffers
            .get(&buffer.remote_id())
            .map(|before| class_shapes(&before.text()));
        now.is_none() || before.is_some_and(|before| before != now)
    })
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
        (false, false) if not_hot_swappable(project, cx) => Some("Restart Debug"),
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
    // `xdebugger.hotswap.tooltip.restart`: the session started again applies the changes
    let restart = not_hot_swappable(&project, cx);
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
                    .aria_label(match restart {
                        true => "Restart Debug",
                        false => "Code changed:",
                    })
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(6.))
                    .py(px(4.))
                    .rounded(px(spork_workbench::tokens::metrics::RADIUS))
                    .cursor_pointer()
                    .hover(|button| button.bg(tokens.editor_button_hover))
                    .tooltip(move |window, cx| {
                        let tooltip = match restart {
                            true => gpui_kit::component::tooltip::Tooltip::element(|_, _| {
                                gpui_kit::component::v_flex()
                                    .max_w(px(300.))
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .child("Restart Debug"),
                                    )
                                    .child(
                                        "HotSwap is unlikely to apply these changes. Restart \
                                         the debug session to apply them.",
                                    )
                            }),
                            false => gpui_kit::component::tooltip::Tooltip::new("Apply HotSwap"),
                        };
                        tooltip.build(window, cx)
                    })
                    .child("Code changed:")
                    .child(
                        spork_workbench::image(
                            match restart {
                                true => "run/restartDebug",
                                false => "debugger/debuggerSync",
                            },
                            cx,
                        )
                        .size(px(16.)),
                    )
                    .on_click(move |event, window, cx| match restart {
                        true => window.dispatch_action(Box::new(debugger_ui::RerunSession), cx),
                        false => apply(event, window, cx),
                    }),
            )
            .child(
                ActionButton::new("hotswap-hide", "general/close")
                    .tooltip("Hide")
                    .on_click(hide),
            )
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::class_shapes;

    #[test]
    fn a_class_keeps_its_shape_when_only_code_changes() {
        let before = "class A extends B {\n    int n = 1;\n    int twice(int v) {\n        return v * 2;\n    }\n}\n";
        let body = before.replace("v * 2", "v * 3").replace("n = 1", "n = 2");
        assert_eq!(class_shapes(before), class_shapes(&body));
        let added = before.replace("    int twice", "    void more() {}\n    int twice");
        assert_ne!(class_shapes(before), class_shapes(&added));
        let retyped = before.replace("int twice(int v)", "int twice(long v)");
        assert_ne!(class_shapes(before), class_shapes(&retyped));
        assert_eq!(class_shapes("class A { int f( }"), None);
    }
}
