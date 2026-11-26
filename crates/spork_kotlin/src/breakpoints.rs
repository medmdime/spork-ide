//! IntelliJ's Kotlin breakpoints beyond a line's, as far as the Kotlin language server's debugger
//! takes them: a function breakpoint on the line that declares a function (IntelliJ's Kotlin
//! Function Breakpoint, sent as the adapter's function breakpoint), and on a line with lambdas
//! IntelliJ's Set Breakpoint popup: Line, each lambda, Line and Lambdas (the adapter's breakpoints
//! at the lambdas' columns). Property watchpoints aren't here: the server's debugger has no data
//! breakpoints.

use std::rc::Rc;

use editor::{Anchor, BreakpointToggle, Editor};
use gpui::{App, Context, Window};
use project::debugger::breakpoint_store::{
    Breakpoint, BreakpointEditAction, BreakpointKind, BreakpointProperties, BreakpointStore,
    BreakpointWithPosition,
};
use spork_navigate::list_popup::{Row, RowIcon, show_list_popup};
use workspace::Workspace;

/// What a line can have a breakpoint on beyond the line itself.
#[derive(Debug, Default, PartialEq)]
struct LineVariants {
    /// Each lambda whose body starts on the line: its body's 1-based column and its text.
    lambdas: Vec<(u64, String)>,
    /// The function the line declares without code of its own, by the JVM name the adapter
    /// takes (`com.example.MainKt.greet`).
    function: Option<String>,
}

fn text_of<'a>(node: tree_sitter::Node<'_>, text: &'a str) -> &'a str {
    text.get(node.byte_range()).unwrap_or_default()
}

fn child_of_kind<'a>(node: tree_sitter::Node<'a>, kind: &str) -> Option<tree_sitter::Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

/// The class a declaration compiles into: its enclosing classes and objects (`Outer$Inner`, a
/// companion's `$Companion`), or the file's facade for a top-level one.
fn jvm_class(node: tree_sitter::Node<'_>, text: &str, file_name: &str) -> String {
    let mut classes = Vec::new();
    let mut parent = node.parent();
    while let Some(ancestor) = parent {
        match ancestor.kind() {
            "class_declaration" | "object_declaration" => {
                if let Some(name) = child_of_kind(ancestor, "type_identifier") {
                    classes.push(text_of(name, text).to_owned());
                }
            }
            "companion_object" => classes.push(
                child_of_kind(ancestor, "type_identifier").map_or("Companion".to_owned(), |name| {
                    text_of(name, text).to_owned()
                }),
            ),
            _ => {}
        }
        parent = ancestor.parent();
    }
    if classes.is_empty() {
        return crate::run::facade(file_name, text);
    }
    classes.reverse();
    classes.join("$")
}

/// The function declared around the 0-based `row` of a Kotlin file: its name's row and the JVM
/// name the adapter takes (`com.example.MainKt.greet`).
fn function_around(text: &str, row: usize, file_name: &str) -> Option<(usize, String)> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_kotlin::LANGUAGE.into())
        .ok()?;
    let tree = parser.parse(text, None)?;
    let mut innermost = None;
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.start_position().row > row || node.end_position().row < row {
            continue;
        }
        if node.kind() == "function_declaration" {
            innermost = Some(node);
        }
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }
    let function = innermost?;
    let name = child_of_kind(function, "simple_identifier")?;
    let package = child_of_kind(tree.root_node(), "package_header")
        .and_then(|header| child_of_kind(header, "identifier"))
        .map(|identifier| text_of(identifier, text).to_owned());
    let class = jvm_class(function, text, file_name);
    let qualified = format!("{class}.{}", text_of(name, text));
    let qualified = match package {
        Some(package) => format!("{package}.{qualified}"),
        None => qualified,
    };
    Some((name.start_position().row, qualified))
}

/// Toggle Method Breakpoint in a Kotlin editor: the function breakpoint of the function around
/// the caret on its name's line, or the breakpoint there removed; false in another file's editor.
fn toggle_method_breakpoint(workspace: &mut Workspace, cx: &mut Context<Workspace>) -> bool {
    let Some(editor) = workspace.active_item_as::<Editor>(cx) else {
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
    let is_kotlin = buffer
        .read(cx)
        .language()
        .is_some_and(|language| language.name().as_ref() == "Kotlin");
    let Some(path) = BreakpointStore::abs_path_from_buffer(&buffer, cx).filter(|_| is_kotlin)
    else {
        return false;
    };
    let snapshot = buffer.read(cx).snapshot();
    let row = text::ToPoint::to_point(&anchor, &snapshot).row as usize;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Some((row, function)) = function_around(&snapshot.text(), row, &file_name) else {
        return true;
    };
    let store = workspace.project().read(cx).breakpoint_store();
    store.update(cx, |store, cx| {
        let breakpoint = match store.breakpoint_at_row(&path, row as u32, cx) {
            Some((_, existing)) => existing,
            None => BreakpointWithPosition {
                position: snapshot.anchor_after(text::Point::new(row as u32, 0)),
                bp: Breakpoint {
                    properties: BreakpointProperties {
                        kind: BreakpointKind::Method {
                            name: function.clone(),
                            function: Some(function),
                        },
                        ..BreakpointProperties::default()
                    },
                    ..Breakpoint::new_standard()
                },
            },
        };
        store.toggle_breakpoint(buffer, breakpoint, BreakpointEditAction::Toggle, cx);
    });
    true
}

/// The breakpoint variants of the 0-based `row` of a Kotlin file.
fn line_variants(text: &str, row: usize, file_name: &str) -> LineVariants {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_kotlin::LANGUAGE.into())
        .is_err()
    {
        return LineVariants::default();
    }
    let Some(tree) = parser.parse(text, None) else {
        return LineVariants::default();
    };
    let line_start = text
        .split_inclusive('\n')
        .take(row)
        .map(str::len)
        .sum::<usize>();
    let mut variants = LineVariants::default();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.start_position().row > row || node.end_position().row < row {
            continue;
        }
        match node.kind() {
            "lambda_literal" => {
                // a lambda that starts on the line with code on it
                if let Some(body) = child_of_kind(node, "statements")
                    && node.start_position().row == row
                    && body.start_position().row == row
                {
                    let column = text
                        .get(line_start..body.start_byte())
                        .map_or(0, |prefix| prefix.chars().count());
                    let body_text: String = text_of(body, text).chars().take(100).collect();
                    variants.lambdas.push((column as u64 + 1, body_text));
                }
            }
            "function_declaration" => {
                let name = child_of_kind(node, "simple_identifier");
                // a block body whose code starts on a later line: the line is the declaration's
                let block = child_of_kind(node, "function_body")
                    .filter(|body| text_of(*body, text).starts_with('{'));
                let code_later = block.is_some_and(|block| {
                    child_of_kind(block, "statements")
                        .is_none_or(|statements| statements.start_position().row > row)
                });
                if let Some(name) = name
                    && name.start_position().row == row
                    && code_later
                {
                    let package = child_of_kind(tree.root_node(), "package_header")
                        .and_then(|header| child_of_kind(header, "identifier"))
                        .map(|identifier| text_of(identifier, text).to_owned());
                    let class = jvm_class(node, text, file_name);
                    let function = format!("{class}.{}", text_of(name, text));
                    variants.function = Some(match package {
                        Some(package) => format!("{package}.{function}"),
                        None => function,
                    });
                }
            }
            _ => {}
        }
        let mut cursor = node.walk();
        let mut children: Vec<_> = node.children(&mut cursor).collect();
        children.reverse();
        stack.extend(children);
    }
    variants.lambdas.sort_by_key(|(column, _)| *column);
    variants
}

/// A new breakpoint on a Kotlin line: a function breakpoint on a function's declaration, IntelliJ's
/// Set Breakpoint popup on a line with lambdas; any other, the line's as Zed sets it.
fn toggle(
    editor: &mut Editor,
    anchor: Anchor,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> bool {
    let snapshot = editor.buffer().read(cx).snapshot(cx);
    let Some((buffer_anchor, _)) = snapshot.anchor_to_buffer_anchor(anchor) else {
        return false;
    };
    let Some(buffer) = editor.buffer().read(cx).buffer(buffer_anchor.buffer_id) else {
        return false;
    };
    let is_kotlin = buffer
        .read(cx)
        .language()
        .is_some_and(|language| language.name().as_ref() == "Kotlin");
    let (Some(store), Some(path), true) = (
        editor
            .project()
            .map(|project| project.read(cx).breakpoint_store()),
        BreakpointStore::abs_path_from_buffer(&buffer, cx),
        is_kotlin,
    ) else {
        return false;
    };
    let text_snapshot = buffer.read(cx).snapshot();
    let row = text::ToPoint::to_point(&buffer_anchor, &text_snapshot).row;
    // one there already: Zed removes it
    if store.read(cx).breakpoint_at_row(&path, row, cx).is_some() {
        return false;
    }
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let variants = line_variants(&text_snapshot.text(), row as usize, &file_name);
    // a breakpoint of these properties (a function breakpoint, a line's lambdas)
    let breakpoint = |properties: BreakpointProperties| Breakpoint {
        properties,
        ..Breakpoint::new_standard()
    };
    if let Some(function) = variants.function {
        // IntelliJ's Kotlin function breakpoint: the method breakpoint, named as the debugger
        // takes it
        let properties = BreakpointProperties {
            kind: BreakpointKind::Method {
                name: function.clone(),
                function: Some(function),
            },
            ..BreakpointProperties::default()
        };
        editor.edit_breakpoint_at_anchor(
            anchor,
            breakpoint(properties),
            BreakpointEditAction::Toggle,
            cx,
        );
        return true;
    }
    if variants.lambdas.is_empty() {
        return false;
    }
    let Some(workspace) = editor.workspace() else {
        return false;
    };
    let this = cx.entity();
    let lambdas: Vec<u64> = variants.lambdas.iter().map(|(column, _)| *column).collect();
    let choose = move |columns: Vec<Option<u64>>| {
        let this = this.downgrade();
        move |_: &mut Window, cx: &mut App| {
            let properties = BreakpointProperties {
                columns: columns.clone(),
                ..BreakpointProperties::default()
            };
            this.update(cx, |editor, cx| {
                editor.edit_breakpoint_at_anchor(
                    anchor,
                    breakpoint(properties),
                    BreakpointEditAction::Toggle,
                    cx,
                )
            })
            .ok();
        }
    };
    // IntelliJ's variants: Line, each lambda, then Line and Lambda(s)
    let breakpoint_icon = Some(RowIcon::Colored("breakpoints/breakpoint"));
    let mut rows = vec![Row::item("Line", choose(Vec::new())).icon(breakpoint_icon)];
    for (column, body) in &variants.lambdas {
        rows.push(
            Row::item(body.clone(), choose(vec![Some(*column)]))
                .icon(Some(RowIcon::Colored("nodes/lambda"))),
        );
    }
    let all: Vec<Option<u64>> = std::iter::once(None)
        .chain(lambdas.iter().copied().map(Some))
        .collect();
    // `JavaDebuggerBundle`'s `breakpoint.variant.text.line.and.lambda`
    let text = match lambdas.len() {
        1 => "Line and Lambda",
        _ => "Line and Lambdas",
    };
    rows.push(Row::item(text, choose(all)).icon(breakpoint_icon));
    let editor = cx.entity();
    let workspace = workspace.downgrade();
    // the popup reads the editor, which is being updated: it opens after
    window.defer(cx, move |window, cx| {
        // `XDebuggerBundle`'s `popup.title.set.breakpoint`
        show_list_popup(
            &workspace,
            &editor,
            "Set Breakpoint",
            rows,
            false,
            window,
            cx,
        );
    });
    true
}

pub(crate) fn init(cx: &mut App) {
    cx.set_global(BreakpointToggle(Rc::new(toggle)));
    // the server's filters, IntelliJ's "Any exception" before the first session
    let filter = |filter: &str, label: &str| dap::ExceptionBreakpointsFilter {
        filter: filter.into(),
        label: label.into(),
        description: None,
        default: Some(false),
        supports_condition: None,
        condition_description: None,
    };
    spork_run::register_exception_filters(
        spork_run::configurations::KOTLIN,
        vec![
            filter("caught", "Caught Exceptions"),
            filter("uncaught", "Uncaught Exceptions"),
        ],
        Some(("caught", "uncaught")),
        cx,
    );
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action(
            |workspace, _: &spork_run::run::ToggleMethodBreakpoint, _, cx| {
                if !toggle_method_breakpoint(workspace, cx) {
                    cx.propagate();
                }
            },
        );
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::line_variants;

    #[test]
    fn finds_the_function_around_a_line() {
        let text = "package com.example\n\nfun greet(name: String): String {\n    val g = Greeting(name)\n    return g.text()\n}\n";
        assert_eq!(
            super::function_around(text, 4, "Main.kt"),
            Some((2, "com.example.MainKt.greet".to_owned()))
        );
        assert_eq!(super::function_around(text, 0, "Main.kt"), None);
    }

    #[test]
    fn finds_a_lines_lambdas_and_the_function_it_declares() {
        let text = "package com.example\n\nsuspend fun work(n: Int): Int {\n    delay(10)\n    return n * 2\n}\n\nfun main() = runBlocking {\n    val doubled = async { work(21) }\n    listOf(1).map { it * 2 }.forEach { println(it) }\n}\n\nclass Tool {\n    companion object {\n        fun make() {\n            println()\n        }\n    }\n}\n";
        let work = line_variants(text, 2, "Main.kt");
        assert_eq!(work.function.as_deref(), Some("com.example.MainKt.work"));
        assert!(work.lambdas.is_empty());
        // an expression body has code on its line: no function breakpoint
        assert_eq!(line_variants(text, 7, "Main.kt").function, None);
        assert_eq!(
            line_variants(text, 8, "Main.kt").lambdas,
            [(27, "work(21)".to_owned())]
        );
        assert_eq!(
            line_variants(text, 9, "Main.kt").lambdas,
            [(21, "it * 2".to_owned()), (40, "println(it)".to_owned())]
        );
        assert_eq!(
            line_variants(text, 14, "Main.kt").function.as_deref(),
            Some("com.example.Tool$Companion.make")
        );
        assert_eq!(line_variants(text, 3, "Main.kt"), Default::default());
    }
}
