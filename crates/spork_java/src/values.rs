//! The variables view's actions on Java's values (IntelliJ's `XDebugger.ValueGroup` over
//! java-debug): Mark Object… (F3) labels an object by its id wherever the views show it, Jump To
//! Source (F4) opens where a frame's variable is declared, Jump To Type Source (⇧F4) the source of
//! its value's class, View as shows a value as a primitive or in hex, as an object, with its
//! `toString()` or as a collection, and Show types shows or hides the values' `{Type@id}`. The
//! proxy renders the values (`debugger::Presentation`); the app keeps each session's choices.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use debugger_ui::{ValueActions, ValueActionsHook, ValueMarkupHook};
use gpui::{
    App, Context, Entity, EntityId, Global, InteractiveElement as _, SharedString, TaskExt as _,
    Window,
};
use project::debugger::session::Session;
use util::ResultExt as _;
use workspace::Workspace;

use crate::debugger::{Presentation, Renderer, SET_PRESENTATION, proxy_of, typed_reference};

const INTEGRAL: [&str; 5] = ["int", "long", "short", "byte", "char"];
const PRIMITIVES: [&str; 8] = [
    "int", "long", "short", "byte", "char", "boolean", "float", "double",
];

/// Each Java session's View as choices and Mark Object labels, by the session.
#[derive(Default)]
struct Sessions(HashMap<EntityId, SessionValues>);

impl Global for Sessions {}

#[derive(Default)]
struct SessionValues {
    renderers: HashMap<String, Renderer>,
    /// The labels of the objects marked, by their id (java-debug's JDI unique id).
    marks: HashMap<String, SharedString>,
}

/// Show types (`DebuggerSettings.SHOW_TYPES`), on by default.
// shortcut: kept while the app runs; IntelliJ keeps it in its debugger settings
struct ShowTypes(bool);

impl Global for ShowTypes {}

fn show_types(cx: &App) -> bool {
    cx.try_global::<ShowTypes>().is_none_or(|show| show.0)
}

/// The id of the object a value shows (`{ArrayList@23}  size = 3` → `23`).
fn object_id(value: &str) -> Option<&str> {
    let reference = typed_reference(value)?;
    let (_, id) = reference.trim_end_matches('}').rsplit_once('@')?;
    Some(id)
}

pub(crate) fn init(cx: &mut App) {
    cx.set_global(ValueActionsHook(Rc::new(value_actions)));
    cx.set_global(ValueMarkupHook(Rc::new(|session, variable, cx| {
        let id = object_id(&variable.value)?;
        let label = cx
            .try_global::<Sessions>()?
            .0
            .get(&session.entity_id())?
            .marks
            .get(id)?
            .clone();
        Some((label, spork_run::object_label_color()))
    })));
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action_renderer(|div, workspace, _, cx| {
            let Some(session) = spork_run::active_debug_session(workspace, cx) else {
                return div;
            };
            if proxy_of(session.read(cx), cx).is_none() {
                return div;
            }
            div.on_action(
                cx.listener(|workspace, _: &spork_run::run::MarkObject, _, cx| {
                    mark_object(workspace, cx)
                }),
            )
            .on_action(
                cx.listener(|workspace, _: &spork_run::run::JumpToSource, window, cx| {
                    jump_to_source(workspace, window, cx).log_err();
                }),
            )
            .on_action(cx.listener(
                |workspace, _: &spork_run::run::JumpToTypeSource, window, cx| {
                    jump_to_type_source(workspace, window, cx)
                },
            ))
            .on_action(
                cx.listener(|workspace, action: &spork_run::run::ViewAs, _, cx| {
                    view_as(workspace, &action.renderer, cx)
                }),
            )
            .on_action(cx.listener(
                |workspace, _: &spork_run::run::ShowTypes, _, cx| toggle_show_types(workspace, cx),
            ))
        });
    })
    .detach();
}

/// What the variables popup offers for a value of a Java session (a frame's own variables are
/// at depth 1, under their scope).
fn value_actions(
    session: &Entity<Session>,
    variable: &dap::Variable,
    depth: usize,
    cx: &App,
) -> Option<ValueActions> {
    proxy_of(session.read(cx), cx)?;
    let values = cx
        .try_global::<Sessions>()
        .and_then(|sessions| sessions.0.get(&session.entity_id()));
    let ty = variable.type_.as_deref().unwrap_or_default();
    let chosen = variable
        .evaluate_name
        .as_ref()
        .and_then(|expression| values?.renderers.get(expression))
        .copied();
    let object = variable.variables_reference > 0 && ty != "String";
    let shown =
        |renderer: Renderer, auto: bool| (chosen == Some(renderer)) || (chosen.is_none() && auto);
    let detail = typed_reference(&variable.value)
        .map(|reference| variable.value[reference.len()..].trim_start())
        .unwrap_or(&variable.value);
    let renderers: Vec<(SharedString, bool)> = match variable.evaluate_name {
        None => Vec::new(),
        Some(_) if INTEGRAL.contains(&ty) => vec![
            ("Primitive".into(), shown(Renderer::Primitive, true)),
            ("Hex".into(), shown(Renderer::Hex, false)),
        ],
        // java-debug's logical view of a collection is its own: its elements, not its fields
        Some(_) if detail.starts_with("size = ") => vec![
            ("Collection".into(), shown(Renderer::Collection, true)),
            ("toString".into(), shown(Renderer::ToString, false)),
        ],
        Some(_) if object => vec![
            (
                "Object".into(),
                shown(Renderer::Object, !detail.starts_with('"')),
            ),
            (
                "toString".into(),
                shown(Renderer::ToString, detail.starts_with('"')),
            ),
        ],
        Some(_) => Vec::new(),
    };
    let marked = object_id(&variable.value)
        .map(|id| values.is_some_and(|values| values.marks.contains_key(id)));
    Some(ValueActions {
        mark: marked,
        // shortcut: a frame's own variables; a field of a value inside the tree isn't found
        jump_to_source: depth <= 1 && variable.evaluate_name.is_some(),
        jump_to_type_source: !ty.is_empty() && !PRIMITIVES.contains(&ty),
        renderers,
        show_types: Some(show_types(cx)),
    })
}

/// The selected value of the Debug tool window's variables, with the session.
fn selected(workspace: &Workspace, cx: &App) -> Option<(Entity<Session>, dap::Variable)> {
    let running = spork_run::running_state(workspace, cx)?;
    let (variable, _) = running.read(cx).variables().read(cx).selected_variable()?;
    Some((running.read(cx).session().clone(), variable))
}

fn redraw_variables(workspace: &Workspace, cx: &mut App) {
    if let Some(running) = spork_run::running_state(workspace, cx) {
        running
            .read(cx)
            .variables()
            .clone()
            .update(cx, |_, cx| cx.notify());
    }
}

/// Mark Object… (F3): "Select Object Label", then the object shows its label; Unmark Object
/// takes it off.
fn mark_object(workspace: &mut Workspace, cx: &mut Context<Workspace>) {
    let Some((session, variable)) = selected(workspace, cx) else {
        return;
    };
    let Some(id) = object_id(&variable.value).map(str::to_owned) else {
        return;
    };
    let key = session.entity_id();
    let marked = cx
        .default_global::<Sessions>()
        .0
        .entry(key)
        .or_default()
        .marks
        .remove(&id)
        .is_some();
    if marked {
        redraw_variables(workspace, cx);
        return;
    }
    let weak = workspace.weak_handle();
    spork_run::ask_object_label(
        Rc::new(move |label, cx| {
            cx.default_global::<Sessions>()
                .0
                .entry(key)
                .or_default()
                .marks
                .insert(id.clone(), label.into());
            weak.update(cx, |workspace, cx| redraw_variables(workspace, cx))
                .log_err();
        }),
        cx,
    );
}

/// The session's View as and Show types for its proxy, then its values taken again.
fn present(session: &Entity<Session>, cx: &mut App) {
    let Some(proxy) = proxy_of(session.read(cx), cx) else {
        return;
    };
    let presentation = Presentation {
        show_types: show_types(cx),
        renderers: cx
            .try_global::<Sessions>()
            .and_then(|sessions| sessions.0.get(&session.entity_id()))
            .map(|values| values.renderers.clone())
            .unwrap_or_default(),
    };
    let session = session.downgrade();
    cx.spawn(async move |cx| {
        proxy
            .request(SET_PRESENTATION, serde_json::to_value(presentation)?)
            .await?;
        session.update(cx, |session, cx| session.refresh_variables(cx))
    })
    .detach_and_log_err(cx);
}

/// View as ▸ a renderer, or Auto (`""`): the selected value shown so.
fn view_as(workspace: &mut Workspace, renderer: &str, cx: &mut Context<Workspace>) {
    let Some((session, variable)) = selected(workspace, cx) else {
        return;
    };
    let Some(expression) = variable.evaluate_name else {
        return;
    };
    let renderer = match renderer {
        "Primitive" => Some(Renderer::Primitive),
        "Hex" => Some(Renderer::Hex),
        "Object" => Some(Renderer::Object),
        "toString" => Some(Renderer::ToString),
        "Collection" => Some(Renderer::Collection),
        _ => None,
    };
    let renderers = &mut cx
        .default_global::<Sessions>()
        .0
        .entry(session.entity_id())
        .or_default()
        .renderers;
    match renderer {
        Some(renderer) => renderers.insert(expression, renderer),
        None => renderers.remove(&expression),
    };
    present(&session, cx);
}

/// Show types: every Java session's values with or without their `{Type@id}`.
fn toggle_show_types(workspace: &mut Workspace, cx: &mut Context<Workspace>) {
    let show = !show_types(cx);
    cx.set_global(ShowTypes(show));
    let sessions: Vec<_> = workspace
        .project()
        .read(cx)
        .dap_store()
        .read(cx)
        .sessions()
        .filter(|session| proxy_of(session.read(cx), cx).is_some())
        .cloned()
        .collect();
    for session in sessions {
        present(&session, cx);
    }
}

/// Jump To Source: the selected variable's declaration in its frame's source (a local, a
/// parameter, or the class's field of that name).
fn jump_to_source(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Result<()> {
    let running = spork_run::running_state(workspace, cx).context("no debug session")?;
    let (variable, _) = running
        .read(cx)
        .variables()
        .read(cx)
        .selected_variable()
        .context("no variable selected")?;
    let frame_id = running
        .read(cx)
        .opened_stack_frame(cx)
        .context("no frame")?;
    let thread = running.read(cx).selected_thread_id().context("no thread")?;
    let session = running.read(cx).session().clone();
    let frame = session
        .update(cx, |session, cx| session.stack_frames(thread, cx))?
        .into_iter()
        .find(|frame| frame.dap.id == frame_id)
        .context("the frame is gone")?;
    let path = frame
        .dap
        .source
        .and_then(|source| source.path)
        .filter(|path| !path.starts_with("jdt://"))
        .context("the frame's class has no source of the project")?;
    let text = std::fs::read_to_string(&path)?;
    let frame_row = (frame.dap.line as usize).saturating_sub(1);
    let row = declaration_row(&text, &variable.name, frame_row)
        .context("the declaration isn't in the frame's source")?;
    crate::endpoints::open_at(workspace, path.into(), row as u32, window, cx);
    Ok(())
}

/// The row of the declaration of `name` that a line of a Java file sees: the nearest local or
/// parameter above it in its method (or lambda), else the field of that name.
fn declaration_row(text: &str, name: &str, frame_row: usize) -> Option<usize> {
    let mut parser = language::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .ok()?;
    let tree = parser.parse(text, None)?;
    let source = text.as_bytes();
    let is_name = |node: language::Node| node.utf8_text(source).ok() == Some(name);
    let (mut local, mut field) = (None::<usize>, None::<usize>);
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
        let names: Vec<language::Node> = match node.kind() {
            "variable_declarator"
            | "formal_parameter"
            | "catch_formal_parameter"
            | "enhanced_for_statement"
            | "resource"
            | "spread_parameter" => node.child_by_field_name("name").into_iter().collect(),
            "lambda_expression" => match node.child_by_field_name("parameters") {
                Some(parameter) if parameter.kind() == "identifier" => vec![parameter],
                Some(parameters) if parameters.kind() == "inferred_parameters" => {
                    let mut cursor = parameters.walk();
                    parameters
                        .children(&mut cursor)
                        .filter(|child| child.kind() == "identifier")
                        .collect()
                }
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        for declared in names.into_iter().filter(|declared| is_name(*declared)) {
            let row = declared.start_position().row;
            if node
                .parent()
                .is_some_and(|parent| parent.kind() == "field_declaration")
            {
                field = field.or(Some(row));
                continue;
            }
            // its scope: the method, constructor or lambda around it, which must hold the line
            let mut scope = node.parent();
            while let Some(around) = scope {
                if matches!(
                    around.kind(),
                    "method_declaration"
                        | "constructor_declaration"
                        | "compact_constructor_declaration"
                        | "lambda_expression"
                        | "static_initializer"
                ) {
                    break;
                }
                scope = around.parent();
            }
            let sees = scope.is_some_and(|scope| {
                scope.start_position().row <= frame_row && frame_row <= scope.end_position().row
            });
            if sees && row <= frame_row && local.is_none_or(|nearest| row > nearest) {
                local = Some(row);
            }
        }
    }
    local.or(field)
}

/// Jump To Type Source (⇧F4): the class of the selected value, found by its name among JDT LS's
/// classes and opened (a library's or the JDK's read-only, as a frame's).
fn jump_to_type_source(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(running) = spork_run::running_state(workspace, cx) else {
        return;
    };
    let Some((variable, _)) = running.read(cx).variables().read(cx).selected_variable() else {
        return;
    };
    let Some(expression) = variable.evaluate_name else {
        return;
    };
    let frame = running.read(cx).opened_stack_frame(cx);
    let session = running.read(cx).session().clone();
    let project = workspace.project().clone();
    let Some(server) = crate::java_project::language_server(&project, cx) else {
        return;
    };
    let class_name = session.update(cx, |session, cx| {
        session.evaluate_expression(format!("({expression}).getClass().getName()"), frame, cx)
    });
    cx.spawn_in(window, async move |workspace, cx| {
        let class = class_name.await?.result;
        let class = class.trim_matches('"').to_owned();
        let simple = class.rsplit(['.', '$']).next().unwrap_or(&class).to_owned();
        let dotted = class.replace('$', ".");
        let symbols = server
            .request::<lsp::request::WorkspaceSymbolRequest>(
                lsp::WorkspaceSymbolParams {
                    query: simple.clone(),
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
                    symbol.name == simple
                        && symbol
                            .container_name
                            .as_deref()
                            .map(|container| format!("{container}.{}", symbol.name))
                            == Some(dotted.clone())
                })
                .map(|symbol| symbol.location),
            _ => None,
        }
        .with_context(|| format!("no source for {class}"))?;
        let uri = location.uri.to_string();
        let line = location.range.start.line;
        let path = match uri.starts_with("jdt://") {
            true => {
                let source = cx
                    .update(|_, cx| crate::debugger::class_source(&uri, &project, cx))?
                    .context("JDT LS can't show the class")?;
                source.await?.to_path_buf()
            }
            false => location
                .uri
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("not a file: {uri}"))?,
        };
        // JDT LS places a library's class at its file's start: the line declaring it
        let line = match line {
            0 => std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| declaring_line(&text, &simple))
                .unwrap_or(0),
            line => line,
        };
        workspace.update_in(cx, |workspace, window, cx| {
            crate::endpoints::open_at(workspace, path, line, window, cx)
        })
    })
    .detach_and_log_err(cx);
}

/// The line declaring the class (or interface, enum, record) `name` in a Java file.
fn declaring_line(text: &str, name: &str) -> Option<u32> {
    text.lines()
        .position(|line| {
            let code = line.trim_start();
            if code.starts_with("//") || code.starts_with('*') || code.starts_with("/*") {
                return false;
            }
            let mut words = line.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '$');
            let mut kind = false;
            words.any(|word| {
                let found = kind && word == name;
                kind = matches!(word, "class" | "interface" | "enum" | "record");
                found
            })
        })
        .map(|row| row as u32)
}

#[cfg(test)]
mod tests {
    use super::{declaration_row, declaring_line, object_id};

    #[test]
    fn finds_the_line_declaring_a_class() {
        let text = "class Outer {
    // a class ListN
    static final class ListN<E> extends X {
";
        assert_eq!(declaring_line(text, "ListN"), Some(2));
        assert_eq!(declaring_line(text, "Outer"), Some(0));
    }

    #[test]
    fn finds_the_declaration_a_line_sees() {
        let text = "class A {\n    static int counter;\n    void run(String name) {\n        int i = 0;\n        names.forEach(name -> use(name));\n        int i2 = i;\n    }\n}\n";
        assert_eq!(declaration_row(text, "i", 5), Some(3));
        assert_eq!(declaration_row(text, "name", 3), Some(2));
        assert_eq!(declaration_row(text, "name", 4), Some(4));
        assert_eq!(declaration_row(text, "counter", 5), Some(1));
        assert_eq!(declaration_row(text, "i2", 3), None);
        assert_eq!(object_id("{ArrayList@23}  size = 3"), Some("23"));
        assert_eq!(object_id("\"text\""), None);
    }
}
