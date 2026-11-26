//! Java's method breakpoints and field watchpoints, as IntelliJ's: Toggle Method Breakpoint and
//! Toggle Field Watchpoint in a Java editor or on a field in the variables, kept in the breakpoint
//! store as breakpoints of their kind on the declaration's line. java-debug stops at a method's
//! first line for a breakpoint on its declaration (IntelliJ's emulated method breakpoint, entry
//! only: java-debug has no method exit) and watches a field by its class and name.

use std::time::Duration;

use anyhow::{Context as _, anyhow};
use gpui::{App, AppContext as _, Context, Entity, Task, TaskExt as _};
use project::Project;
use project::debugger::breakpoint_store::{
    Breakpoint, BreakpointEditAction, BreakpointKind, BreakpointStore, BreakpointWithPosition,
};
use project::debugger::session::Session;
use util::ResultExt as _;
use workspace::Workspace;

use crate::java_project::JavaProject;

pub(crate) fn init(cx: &mut App) {
    cx.set_global(debugger_ui::FieldWatchpointHook(std::rc::Rc::new(
        |session, data_id, cx| toggle_field_of_data_id(session, data_id, cx),
    )));
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action(
            |workspace, _: &spork_run::run::ToggleMethodBreakpoint, _, cx| {
                // another language's editor (Kotlin's) has its own
                if !toggle_member_breakpoint(workspace, false, cx) {
                    cx.propagate();
                }
            },
        );
        workspace.register_action(
            |workspace, _: &spork_run::run::ToggleFieldWatchpoint, _, cx| {
                toggle_member_breakpoint(workspace, true, cx);
            },
        );
    })
    .detach();
}

/// Toggle Method Breakpoint and Toggle Field Watchpoint (`ToggleMethodBreakpoint`,
/// `ToggleFieldBreakpoint`) in a Java editor: a breakpoint of that kind on the declaration line of
/// the method or field at the caret, or the breakpoint of that line removed; false outside a Java
/// editor.
fn toggle_member_breakpoint(
    workspace: &mut Workspace,
    field: bool,
    cx: &mut Context<Workspace>,
) -> bool {
    let Some(editor) = workspace.active_item_as::<editor::Editor>(cx) else {
        return false;
    };
    let editor = editor.read(cx);
    let multibuffer = editor.buffer().read(cx);
    let head = editor.selections.newest_anchor().head();
    let Some((anchor, _)) = multibuffer.snapshot(cx).anchor_to_buffer_anchor(head) else {
        return false;
    };
    let Some(buffer) = multibuffer.buffer(anchor.buffer_id) else {
        return false;
    };
    let is_java = buffer
        .read(cx)
        .language()
        .is_some_and(|language| language.name().as_ref() == "Java");
    if !is_java {
        return false;
    }
    let snapshot = buffer.read(cx).snapshot();
    let offset = text::ToOffset::to_offset(&anchor, &snapshot);
    let Some((row, kind)) = member_at(&snapshot, offset, field) else {
        return true;
    };
    let store = workspace.project().read(cx).breakpoint_store();
    toggle_kind(&store, buffer, row, kind, cx);
    true
}

/// The breakpoint of `row` removed, or one of `kind` set there.
fn toggle_kind(
    store: &Entity<BreakpointStore>,
    buffer: Entity<language::Buffer>,
    row: u32,
    kind: BreakpointKind,
    cx: &mut App,
) {
    let Some(path) = BreakpointStore::abs_path_from_buffer(&buffer, cx) else {
        return;
    };
    store.update(cx, |store, cx| {
        let breakpoint = match store.breakpoint_at_row(&path, row, cx) {
            Some((_, existing)) => existing,
            None => {
                let mut breakpoint = Breakpoint::new_standard();
                breakpoint.properties.kind = kind;
                BreakpointWithPosition {
                    position: buffer
                        .read(cx)
                        .snapshot()
                        .anchor_after(text::Point::new(row, 0)),
                    bp: breakpoint,
                }
            }
        };
        store.toggle_breakpoint(buffer, breakpoint, BreakpointEditAction::Toggle, cx);
    });
}

/// The method (or field) declared around `offset`, or on its line: its name's row and the
/// breakpoint kind watching it, `com.example.Debuggee.measure` or `Debuggee.counter` as IntelliJ
/// names them, the field's java-debug data id `com.example.Debuggee#counter`.
fn member_at(
    snapshot: &language::BufferSnapshot,
    offset: usize,
    field: bool,
) -> Option<(u32, BreakpointKind)> {
    let wanted: &[&str] = match field {
        true => &["field_declaration"],
        false => &["method_declaration", "constructor_declaration"],
    };
    let find = |offset: usize| {
        let mut node = snapshot.syntax_ancestor(offset..offset)?;
        loop {
            if wanted.contains(&node.kind()) {
                return Some(node);
            }
            node = node.parent()?;
        }
    };
    let row = snapshot.offset_to_point(offset).row;
    let line_start = snapshot.point_to_offset(text::Point::new(
        row,
        snapshot.indent_size_for_line(row).len,
    ));
    let member = find(offset).or_else(|| find(line_start))?;
    let name_node = match field {
        true => member
            .child_by_field_name("declarator")?
            .child_by_field_name("name")?,
        false => member.child_by_field_name("name")?,
    };
    let text =
        |node: language::Node| -> String { snapshot.text_for_range(node.byte_range()).collect() };
    let name = text(name_node);
    let mut classes = Vec::new();
    let mut parent = member.parent();
    while let Some(node) = parent {
        if matches!(
            node.kind(),
            "class_declaration"
                | "record_declaration"
                | "enum_declaration"
                | "interface_declaration"
        ) {
            classes.push(text(node.child_by_field_name("name")?));
        }
        parent = node.parent();
    }
    classes.reverse();
    let package = crate::application::package_of(&snapshot.text())
        .map(|package| format!("{package}."))
        .unwrap_or_default();
    let row = name_node.start_position().row as u32;
    let kind = match field {
        true => BreakpointKind::Field {
            name: format!("{}.{name}", classes.last()?),
            data_id: format!("{package}{}#{name}", classes.join("$")),
            access: false,
            modification: true,
        },
        false => BreakpointKind::Method {
            name: format!("{package}{}.{name}", classes.join(".")),
            function: None,
        },
    };
    Some((row, kind))
}

/// Toggle Field Watchpoint on a field in the variables: a field watchpoint on its declaration,
/// found by JDT LS among the workspace's symbols; false when it isn't the project's (Zed's data
/// breakpoint of the session then).
fn toggle_field_of_data_id(session: &Entity<Session>, data_id: &str, cx: &mut App) -> bool {
    let Some((class, field)) = data_id.split_once('#') else {
        return false;
    };
    let Some(worktree) = session.read(cx).worktree() else {
        return false;
    };
    let Some(java) = JavaProject::containing(&worktree.read(cx).abs_path(), cx) else {
        return false;
    };
    let (Some(server), project) = (
        java.read(cx).language_server(cx),
        java.read(cx).project.clone(),
    ) else {
        return false;
    };
    let store = project.read(cx).breakpoint_store();
    let (class, field, data_id) = (class.to_owned(), field.to_owned(), data_id.to_owned());
    cx.spawn(async move |cx| {
        let simple = class.rsplit(['.', '$']).next().unwrap_or(&class).to_owned();
        let symbols = server
            .request::<lsp::request::WorkspaceSymbolRequest>(
                lsp::WorkspaceSymbolParams {
                    query: field.clone(),
                    ..Default::default()
                },
                Duration::from_secs(30),
            )
            .await
            .into_response()?;
        let location = match symbols {
            Some(lsp::WorkspaceSymbolResponse::Flat(symbols)) => symbols
                .into_iter()
                .find(|symbol| {
                    symbol.kind == lsp::SymbolKind::FIELD
                        && symbol.name == field
                        && symbol.container_name.as_deref() == Some(simple.as_str())
                })
                .map(|symbol| symbol.location),
            _ => None,
        }
        .context("the field isn't the project's")?;
        let path = location
            .uri
            .to_file_path()
            .map_err(|_| anyhow!("not a file: {}", location.uri))?;
        let buffer = project
            .update(cx, |project, cx| project.open_local_buffer(&path, cx))
            .await?;
        cx.update(|cx| {
            toggle_kind(
                &store,
                buffer,
                location.range.start.line,
                BreakpointKind::Field {
                    name: format!("{simple}.{field}"),
                    data_id,
                    access: false,
                    modification: true,
                },
                cx,
            )
        });
        anyhow::Ok(())
    })
    .detach_and_log_err(cx);
    true
}

/// The exception classes the Breakpoints dialog's class chooser lists: JDT LS's classes whose
/// name holds the text, from the project, its libraries and the JDK, by their JVM name.
// shortcut: a class is known as a Throwable by its name (…Exception, …Error, Throwable); ask JDT
// LS's type hierarchy once a project names one otherwise
pub(crate) fn exception_classes(
    project: &Entity<Project>,
    query: String,
    cx: &mut App,
) -> Task<Vec<String>> {
    let Some(server) = crate::java_project::language_server(project, cx) else {
        return Task::ready(Vec::new());
    };
    cx.background_spawn(async move {
        let symbols = server
            .request::<lsp::request::WorkspaceSymbolRequest>(
                lsp::WorkspaceSymbolParams {
                    query,
                    ..Default::default()
                },
                Duration::from_secs(30),
            )
            .await
            .into_response()
            .log_err()
            .flatten();
        let classes: Vec<(String, Option<String>)> = match symbols {
            Some(lsp::WorkspaceSymbolResponse::Flat(symbols)) => symbols
                .into_iter()
                .filter(|symbol| symbol.kind == lsp::SymbolKind::CLASS)
                .map(|symbol| (symbol.name, symbol.container_name))
                .collect(),
            Some(lsp::WorkspaceSymbolResponse::Nested(symbols)) => symbols
                .into_iter()
                .filter(|symbol| symbol.kind == lsp::SymbolKind::CLASS)
                .map(|symbol| (symbol.name, symbol.container_name))
                .collect(),
            None => Vec::new(),
        };
        classes
            .into_iter()
            .filter(|(name, _)| is_throwable_name(name))
            .map(
                |(name, container)| match container.filter(|c| !c.is_empty()) {
                    Some(container) => format!("{container}.{name}"),
                    None => name,
                },
            )
            .collect()
    })
}

fn is_throwable_name(name: &str) -> bool {
    name.ends_with("Exception") || name.ends_with("Error") || name == "Throwable"
}
