//! IntelliJ's Java Generate items (`JavaGenerateGroup1` and the platform's `GenerateGroup`) on
//! JDT LS's source actions, the way vscode-java asks for them: Constructor, Getter, Setter,
//! Getter and Setter, equals() and hashCode(), toString(), Override Methods… (⌃O), Implement
//! Methods… (⌃I) and Delegate Methods…, each choosing its members in IntelliJ's member chooser
//! (`MemberChooser`), then applying the edit JDT LS makes.

use std::rc::Rc;

use anyhow::Result;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Entity, InteractiveElement as _, IntoElement, ParentElement as _, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, Task, TaskExt as _, Window, div,
    px,
};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use language::{ToPointUtf16 as _, point_to_lsp};
use lsp::LanguageServer;
use project::{CodeAction, LspAction};
use serde_json::{Value, json};
use spork_code::{Row, Target};
use spork_workbench::controls::dialog_title;
use spork_workbench::{Button, Tokens};
use std::sync::Arc;
use workspace::Workspace;

use crate::java_project;

pub mod java {
    gpui::actions!(
        java,
        [
            /// Chooses methods of the class's supertypes to override (IntelliJ's ⌃O).
            OverrideMethods,
            /// Chooses the abstract methods of the class's supertypes to implement (⌃I).
            ImplementMethods,
            /// Chooses a field and the methods of its type to delegate to it.
            DelegateMethods,
            /// Reloads the classes changed since the last reload into the debugged program
            /// (IntelliJ's Reload Changed Classes, HotSwap), the project built first.
            ReloadChangedClasses,
        ]
    );
}

/// JDT LS's own requests (`JavaProtocolExtensions`), their parameters and answers as JSON.
macro_rules! jdtls_request {
    ($name:ident, $method:literal) => {
        enum $name {}

        impl lsp::request::Request for $name {
            type Params = Value;
            type Result = Value;
            const METHOD: &'static str = $method;
        }
    };
}

jdtls_request!(CheckConstructorsStatus, "java/checkConstructorsStatus");
jdtls_request!(GenerateConstructors, "java/generateConstructors");
jdtls_request!(
    ResolveUnimplementedAccessors,
    "java/resolveUnimplementedAccessors"
);
jdtls_request!(GenerateAccessors, "java/generateAccessors");
jdtls_request!(CheckHashCodeEqualsStatus, "java/checkHashCodeEqualsStatus");
jdtls_request!(GenerateHashCodeEquals, "java/generateHashCodeEquals");
jdtls_request!(CheckToStringStatus, "java/checkToStringStatus");
jdtls_request!(GenerateToString, "java/generateToString");
jdtls_request!(ListOverridableMethods, "java/listOverridableMethods");
jdtls_request!(AddOverridableMethods, "java/addOverridableMethods");
jdtls_request!(
    CheckDelegateMethodsStatus,
    "java/checkDelegateMethodsStatus"
);
jdtls_request!(GenerateDelegateMethods, "java/generateDelegateMethods");

async fn request<R: lsp::request::Request<Params = Value, Result = Value>>(
    server: &LanguageServer,
    params: Value,
) -> Result<Value> {
    server
        .request::<R>(params, std::time::Duration::from_secs(60))
        .await
        .into_response()
}

pub(crate) fn init(cx: &mut App) {
    spork_code::add_generate_rows("Java", rows, cx);
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action_renderer(|div, workspace, _, cx| {
            let Some(target) = java_target(workspace, cx) else {
                return div;
            };
            div.on_action(cx.listener({
                let target = target.clone();
                move |_, _: &java::OverrideMethods, window, cx| {
                    override_methods(target.clone(), false, window, cx)
                }
            }))
            .on_action(cx.listener({
                let target = target.clone();
                move |_, _: &java::ImplementMethods, window, cx| {
                    override_methods(target.clone(), true, window, cx)
                }
            }))
            .on_action(cx.listener(
                move |_, _: &java::DelegateMethods, window, cx| {
                    delegate_methods(target.clone(), window, cx)
                },
            ))
        });
    })
    .detach();
}

/// The active editor when it shows a Java file and JDT LS runs.
fn java_target(workspace: &Workspace, cx: &App) -> Option<Target> {
    let target = Target::of(workspace, cx)?;
    (is_java(&target, cx) && java_project::language_server(&target.project, cx).is_some())
        .then_some(target)
}

fn is_java(target: &Target, cx: &App) -> bool {
    target
        .buffer
        .read(cx)
        .language()
        .is_some_and(|language| language.name().as_ref() == "Java")
}

/// The `CodeActionParams` JDT LS's requests take: the file and the selection.
fn context(target: &Target, cx: &App) -> Option<Value> {
    let buffer = target.buffer.read(cx);
    let path = buffer.file()?.as_local()?.abs_path(cx);
    let uri = lsp::Uri::from_file_path(path).ok()?;
    let snapshot = buffer.snapshot();
    let start = point_to_lsp(target.range.start.to_point_utf16(&snapshot));
    let end = point_to_lsp(target.range.end.to_point_utf16(&snapshot));
    Some(json!({
        "textDocument": {"uri": uri},
        "range": {"start": start, "end": end},
        "context": {"diagnostics": []},
    }))
}

/// What JDT LS says the type at the caret has to generate from.
#[derive(Default)]
struct Status {
    accessors: Vec<Value>,
    equals: Value,
    to_string: Value,
    overridable: Vec<Value>,
    delegates: Vec<Value>,
}

/// The Generate popup's rows for a Java file, in IntelliJ's order, each where the class at the
/// caret has something to generate.
fn rows(target: &Target, window: &mut Window, cx: &mut App) -> Option<Task<Vec<Row>>> {
    if !is_java(target, cx) {
        return None;
    }
    let server = java_project::language_server(&target.project, cx);
    let context = context(target, cx);
    let (Some(server), Some(context)) = (server, context) else {
        return Some(Task::ready(Vec::new()));
    };
    let shortcuts = [
        &java::OverrideMethods as &dyn gpui::Action,
        &java::ImplementMethods,
    ]
    .map(|action| spork_code::shortcut(action, &target.editor, window, cx));
    let target = target.clone();
    Some(cx.spawn(async move |_| {
        let status = status(&server, &context).await;
        let [override_key, implement_key] = shortcuts;
        let has_type = status
            .to_string
            .get("type")
            .and_then(Value::as_str)
            .is_some();
        if !has_type {
            return Vec::new();
        }
        let fields = |value: &Value| {
            value
                .get("fields")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        };
        let getters = status
            .accessors
            .iter()
            .any(|field| flag(field, "generateGetter"));
        let setters = status
            .accessors
            .iter()
            .any(|field| flag(field, "generateSetter"));
        let unimplemented = status
            .overridable
            .iter()
            .any(|method| flag(method, "unimplemented"));
        let item =
            |label: &'static str,
             run: fn(Target, Arc<LanguageServer>, Value, &mut Window, &mut App)| {
                let (target, server, context) = (target.clone(), server.clone(), context.clone());
                Row::item(label, move |window, cx| {
                    run(target.clone(), server.clone(), context.clone(), window, cx)
                })
            };
        let mut rows = vec![item(
            "Constructor",
            |target, server, context, window, cx| constructor(target, server, context, window, cx),
        )];
        if getters {
            rows.push(item("Getter", |target, server, context, window, cx| {
                accessors(target, server, context, Accessors::Getter, window, cx)
            }));
        }
        if setters {
            rows.push(item("Setter", |target, server, context, window, cx| {
                accessors(target, server, context, Accessors::Setter, window, cx)
            }));
        }
        if getters || setters {
            rows.push(item(
                "Getter and Setter",
                |target, server, context, window, cx| {
                    accessors(target, server, context, Accessors::Both, window, cx)
                },
            ));
        }
        if fields(&status.equals) > 0 {
            rows.push(item("equals() and hashCode()", equals_and_hash_code));
        }
        rows.push(item("toString()", to_string));
        rows.push(
            item("Override Methods…", |target, _, _, window, cx| {
                override_methods(target, false, window, cx)
            })
            .hint(override_key),
        );
        if unimplemented {
            rows.push(
                item("Implement Methods…", |target, _, _, window, cx| {
                    override_methods(target, true, window, cx)
                })
                .hint(implement_key),
            );
        }
        if !status.delegates.is_empty() {
            rows.push(item("Delegate Methods…", |target, _, _, window, cx| {
                delegate_methods(target, window, cx)
            }));
        }
        rows
    }))
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn list(value: &Value, key: &str) -> Vec<Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// Asks JDT LS what the type at the caret can generate, every request at once.
async fn status(server: &LanguageServer, context: &Value) -> Status {
    let mut accessors_params = context.clone();
    // GenerateGetterSetterOperation.AccessorKind's BOTH, as vscode-java sends it
    accessors_params["kind"] = json!(2);
    let (accessors, equals, to_string, overridable, delegates) = futures::join!(
        request::<ResolveUnimplementedAccessors>(server, accessors_params),
        request::<CheckHashCodeEqualsStatus>(server, context.clone()),
        request::<CheckToStringStatus>(server, context.clone()),
        request::<ListOverridableMethods>(server, context.clone()),
        request::<CheckDelegateMethodsStatus>(server, context.clone()),
    );
    let log = |result: Result<Value>| match result {
        Ok(value) => value,
        Err(error) => {
            log::warn!("JDT LS's generate status: {error:#}");
            Value::Null
        }
    };
    Status {
        accessors: log(accessors).as_array().cloned().unwrap_or_default(),
        equals: log(equals),
        to_string: log(to_string),
        overridable: list(&log(overridable), "methods"),
        delegates: list(&log(delegates), "delegateFields"),
    }
}

/// Writes generated code as IntelliJ does: indented with the editor's indentation (JDT LS's
/// generator indents with tabs, Eclipse's default, whatever the client's formatting options) and
/// without Eclipse's method body comment (`// TODO Auto-generated method stub`).
fn to_intellij_style(edit: &mut Value, tab_size: usize, hard_tabs: bool) {
    let indent = " ".repeat(tab_size);
    let reindent_text = |text: &mut Value| {
        let Some(old) = text.as_str() else { return };
        let new: Vec<String> = old
            .split('\n')
            .filter(|line| line.trim() != "// TODO Auto-generated method stub")
            .map(|line| {
                let tabs = line.len() - line.trim_start_matches('\t').len();
                match hard_tabs {
                    true => line.to_owned(),
                    false => format!("{}{}", indent.repeat(tabs), &line[tabs..]),
                }
            })
            .collect();
        *text = Value::String(new.join("\n"));
    };
    let reindent_edits = |edits: &mut Value| {
        for edit in edits.as_array_mut().into_iter().flatten() {
            if let Some(text) = edit.get_mut("newText") {
                reindent_text(text);
            }
        }
    };
    if let Some(changes) = edit.get_mut("changes").and_then(Value::as_object_mut) {
        changes.values_mut().for_each(reindent_edits);
    }
    if let Some(changes) = edit
        .get_mut("documentChanges")
        .and_then(Value::as_array_mut)
    {
        for change in changes {
            if let Some(edits) = change.get_mut("edits") {
                reindent_edits(edits);
            }
        }
    }
}

/// Applies the edit JDT LS made, as a code action of its, saving the file.
fn apply(
    target: Target,
    server: &LanguageServer,
    title: &str,
    mut edit: Value,
    window: &mut Window,
    cx: &mut App,
) {
    let settings =
        language::language_settings::LanguageSettings::for_buffer(target.buffer.read(cx), cx);
    to_intellij_style(
        &mut edit,
        settings.tab_size.get() as usize,
        settings.hard_tabs,
    );
    let edit: lsp::WorkspaceEdit = match serde_json::from_value(edit) {
        Ok(edit) => edit,
        Err(error) => {
            log::error!("JDT LS's {title} edit: {error:#}");
            return;
        }
    };
    let action = CodeAction {
        server_id: server.server_id(),
        range: target.range.clone(),
        lsp_action: LspAction::Action(Box::new(lsp::CodeAction {
            title: title.to_owned(),
            edit: Some(edit),
            ..lsp::CodeAction::default()
        })),
        resolved: true,
    };
    spork_code::apply_code_action(&target, action, window, cx);
}

/// Runs a generating request and applies its edit.
fn generate<R: lsp::request::Request<Params = Value, Result = Value>>(
    target: Target,
    server: Arc<LanguageServer>,
    title: &'static str,
    params: Value,
    window: &mut Window,
    cx: &mut App,
) {
    window
        .spawn(cx, async move |cx| {
            let edit = match request::<R>(&server, params).await {
                Ok(edit) => edit,
                // the server's refusal shows where the code was to go
                Err(error) => {
                    let editor = target.editor.downgrade();
                    let text = format!("{error:#}");
                    cx.update(|window, cx| spork_code::show_hint(&editor, text, true, window, cx))?;
                    return Err(error);
                }
            };
            if edit.is_null() {
                return Ok(());
            }
            cx.update(|window, cx| apply(target, &server, title, edit, window, cx))
        })
        .detach_and_log_err(cx);
}

/// A field as the member chooser lists it: `name:String`.
fn field_member(field: &Value) -> Member {
    Member {
        label: format!("{}:{}", text(field, "name"), text(field, "type")).into(),
        icon: "nodes/field",
    }
}

/// A method as the member chooser lists it: `equals(Object)`.
// shortcut: JDT LS gives a method's parameter types and no return type nor parameter names, where
// IntelliJ writes `equals(obj:Object):boolean`
fn method_member(method: &Value) -> Member {
    let parameters: Vec<String> = list(method, "parameters")
        .iter()
        .filter_map(|parameter| parameter.as_str().map(str::to_owned))
        .collect();
    Member {
        label: format!("{}({})", text(method, "name"), parameters.join(", ")).into(),
        icon: "nodes/method",
    }
}

/// The class's name the chooser groups its members under: the type at the caret.
fn type_group(status: &Value, members: usize) -> Group {
    Group {
        label: text(status, "type").into(),
        icon: "nodes/class",
        members: (0..members).collect(),
    }
}

/// Constructor: IntelliJ's Choose Super Class Constructor when the superclass has several, then
/// Choose Fields to Initialize by Constructor when the class has fields.
fn constructor(
    target: Target,
    server: Arc<LanguageServer>,
    context: Value,
    window: &mut Window,
    cx: &mut App,
) {
    window
        .spawn(cx, async move |cx| {
            let status = request::<CheckConstructorsStatus>(&server, context.clone()).await?;
            let constructors = list(&status, "constructors");
            let fields = list(&status, "fields");
            let type_name = request::<CheckToStringStatus>(&server, context.clone()).await?;
            cx.update(|window, cx| {
                let fields_step = move |constructor: Value, window: &mut Window, cx: &mut App| {
                    let generate_with = {
                        let (target, server, context) =
                            (target.clone(), server.clone(), context.clone());
                        move |fields: Vec<Value>, window: &mut Window, cx: &mut App| {
                            let params = json!({
                                "context": context,
                                "constructors": [constructor.clone()],
                                "fields": fields,
                            });
                            generate::<GenerateConstructors>(
                                target.clone(),
                                server.clone(),
                                "Generate Constructors",
                                params,
                                window,
                                cx,
                            )
                        }
                    };
                    if fields.is_empty() {
                        generate_with(Vec::new(), window, cx);
                        return;
                    }
                    let members: Vec<Member> = fields.iter().map(field_member).collect();
                    let groups = vec![type_group(&type_name, members.len())];
                    let fields = fields.clone();
                    MemberChooser {
                        title: "Choose Fields to Initialize by Constructor",
                        groups,
                        selected: vec![true; members.len()],
                        members,
                        allow_empty: true,
                        multiple: true,
                    }
                    .show(
                        move |chosen, window, cx| {
                            let fields = chosen
                                .iter()
                                .filter_map(|ix| fields.get(*ix).cloned())
                                .collect();
                            generate_with(fields, window, cx)
                        },
                        window,
                        cx,
                    );
                };
                match constructors.len() {
                    0 => fields_step(Value::Null, window, cx),
                    1 => fields_step(constructors[0].clone(), window, cx),
                    _ => {
                        let members: Vec<Member> = constructors.iter().map(method_member).collect();
                        let fields_step = Rc::new(fields_step);
                        MemberChooser {
                            title: "Choose Super Class Constructor",
                            groups: vec![Group {
                                label: "".into(),
                                icon: "nodes/class",
                                members: (0..members.len()).collect(),
                            }],
                            selected: (0..members.len()).map(|ix| ix == 0).collect(),
                            members,
                            allow_empty: false,
                            multiple: false,
                        }
                        .show(
                            move |chosen, window, cx| {
                                if let Some(constructor) =
                                    chosen.first().and_then(|ix| constructors.get(*ix))
                                {
                                    let fields_step = fields_step.clone();
                                    let constructor = constructor.clone();
                                    // the next dialog opens once this one has closed
                                    window.defer(cx, move |window, cx| {
                                        fields_step(constructor, window, cx)
                                    });
                                }
                            },
                            window,
                            cx,
                        );
                    }
                }
            })
        })
        .detach_and_log_err(cx);
}

#[derive(Clone, Copy, PartialEq)]
enum Accessors {
    Getter,
    Setter,
    Both,
}

/// Getter, Setter, Getter and Setter: the fields without them, in IntelliJ's Select Fields to
/// Generate… chooser.
fn accessors(
    target: Target,
    server: Arc<LanguageServer>,
    context: Value,
    kind: Accessors,
    window: &mut Window,
    cx: &mut App,
) {
    window
        .spawn(cx, async move |cx| {
            let mut params = context.clone();
            params["kind"] = json!(match kind {
                Accessors::Getter => 0,
                Accessors::Setter => 1,
                Accessors::Both => 2,
            });
            let fields = request::<ResolveUnimplementedAccessors>(&server, params).await?;
            let fields: Vec<Value> = fields
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|field| match kind {
                    Accessors::Getter => flag(field, "generateGetter"),
                    Accessors::Setter => flag(field, "generateSetter"),
                    Accessors::Both => {
                        flag(field, "generateGetter") || flag(field, "generateSetter")
                    }
                })
                .collect();
            let type_name = request::<CheckToStringStatus>(&server, context.clone()).await?;
            cx.update(|window, cx| {
                let members: Vec<Member> = fields
                    .iter()
                    .map(|field| Member {
                        label: format!("{}:{}", text(field, "fieldName"), text(field, "typeName"))
                            .into(),
                        icon: "nodes/field",
                    })
                    .collect();
                let title = match kind {
                    Accessors::Getter => "Select Fields to Generate Getters",
                    Accessors::Setter => "Select Fields to Generate Setters",
                    Accessors::Both => "Select Fields to Generate Getters and Setters",
                };
                MemberChooser {
                    title,
                    groups: vec![type_group(&type_name, members.len())],
                    selected: vec![true; members.len()],
                    members,
                    allow_empty: false,
                    multiple: true,
                }
                .show(
                    move |chosen, window, cx| {
                        let accessors: Vec<Value> = chosen
                            .iter()
                            .filter_map(|ix| fields.get(*ix).cloned())
                            .map(|mut field| {
                                if kind == Accessors::Getter {
                                    field["generateSetter"] = json!(false);
                                }
                                if kind == Accessors::Setter {
                                    field["generateGetter"] = json!(false);
                                }
                                field
                            })
                            .collect();
                        let params = json!({"context": context.clone(), "accessors": accessors});
                        generate::<GenerateAccessors>(
                            target.clone(),
                            server.clone(),
                            "Generate Getters and Setters",
                            params,
                            window,
                            cx,
                        )
                    },
                    window,
                    cx,
                );
            })
        })
        .detach_and_log_err(cx);
}

/// equals() and hashCode(): the fields to compare and hash, after IntelliJ's question when the
/// class has them already.
// shortcut: one chooser for both methods, where IntelliJ's wizard has a page for the template,
// one for equals()' fields, one for hashCode()'s and one for the non-null ones: JDT LS takes one
// list of fields
fn equals_and_hash_code(
    target: Target,
    server: Arc<LanguageServer>,
    context: Value,
    window: &mut Window,
    cx: &mut App,
) {
    window
        .spawn(cx, async move |cx| {
            let status = request::<CheckHashCodeEqualsStatus>(&server, context.clone()).await?;
            let fields = list(&status, "fields");
            let existing = !list(&status, "existingMethods").is_empty();
            let type_name = text(&status, "type");
            if existing {
                let answer = cx.update(|window, cx| {
                    window.prompt(
                        gpui::PromptLevel::Warning,
                        "Generate equals() and hashCode()",
                        Some(&format!(
                            "Methods 'boolean equals(Object)' and 'int hashCode()' are already defined\nfor class {type_name}. Do you want to delete them and proceed?"
                        )),
                        &["Yes", "No"],
                        cx,
                    )
                })?;
                if answer.await? != 0 {
                    return Ok(());
                }
            }
            cx.update(|window, cx| {
                let members: Vec<Member> = fields.iter().map(field_member).collect();
                MemberChooser {
                    title: "Generate equals() and hashCode()",
                    groups: vec![type_group(&status, members.len())],
                    selected: vec![true; members.len()],
                    members,
                    allow_empty: false,
                    multiple: true,
                }
                .show(
                    move |chosen, window, cx| {
                        let fields: Vec<Value> = chosen.iter().filter_map(|ix| fields.get(*ix).cloned()).collect();
                        let params = json!({"context": context.clone(), "fields": fields, "regenerate": existing});
                        generate::<GenerateHashCodeEquals>(
                            target.clone(),
                            server.clone(),
                            "Generate hashCode() and equals()",
                            params,
                            window,
                            cx,
                        )
                    },
                    window,
                    cx,
                );
            })
        })
        .detach_and_log_err(cx);
}

/// toString(): IntelliJ's Generate toString() chooser over the class's fields.
// shortcut: no Template combo or Settings: JDT LS writes the one template its preferences name
fn to_string(
    target: Target,
    server: Arc<LanguageServer>,
    context: Value,
    window: &mut Window,
    cx: &mut App,
) {
    window
        .spawn(cx, async move |cx| {
            let status = request::<CheckToStringStatus>(&server, context.clone()).await?;
            // the class's fields; JDT LS lists its getters and inherited methods too
            let fields: Vec<Value> = list(&status, "fields")
                .into_iter()
                .filter(|field| flag(field, "isField"))
                .collect();
            cx.update(|window, cx| {
                let members: Vec<Member> = fields.iter().map(field_member).collect();
                MemberChooser {
                    title: "Generate toString()",
                    groups: vec![type_group(&status, members.len())],
                    selected: vec![true; members.len()],
                    members,
                    allow_empty: true,
                    multiple: true,
                }
                .show(
                    move |chosen, window, cx| {
                        let fields: Vec<Value> = chosen
                            .iter()
                            .filter_map(|ix| fields.get(*ix).cloned())
                            .collect();
                        let params = json!({"context": context.clone(), "fields": fields});
                        generate::<GenerateToString>(
                            target.clone(),
                            server.clone(),
                            "Generate toString()",
                            params,
                            window,
                            cx,
                        )
                    },
                    window,
                    cx,
                );
            })
        })
        .detach_and_log_err(cx);
}

/// Override Methods… and Implement Methods…: the supertypes' methods by the class that declares
/// them, IntelliJ's Select Methods to Override / to Implement.
fn override_methods(target: Target, implement: bool, window: &mut Window, cx: &mut App) {
    let (Some(server), Some(context)) = (
        java_project::language_server(&target.project, cx),
        context(&target, cx),
    ) else {
        return;
    };
    window
        .spawn(cx, async move |cx| {
            let answer = request::<ListOverridableMethods>(&server, context.clone()).await?;
            let methods: Vec<Value> = list(&answer, "methods")
                .into_iter()
                .filter(|method| flag(method, "unimplemented") == implement)
                .collect();
            cx.update(|window, cx| {
                if methods.is_empty() {
                    let editor = target.editor.downgrade();
                    let hint = match implement {
                        true => "No methods to implement have been found",
                        false => "No methods to override have been found",
                    };
                    spork_code::show_hint(&editor, hint, true, window, cx);
                    return;
                }
                // grouped by the class that declares them, in JDT LS's order
                let mut groups: Vec<Group> = Vec::new();
                for (ix, method) in methods.iter().enumerate() {
                    // `java.lang.Comparable<T>` is listed as IntelliJ lists it, without its type
                    // arguments
                    let declaring = text(method, "declaringClass");
                    let declaring = declaring.split('<').next().unwrap_or_default().to_owned();
                    match groups
                        .iter_mut()
                        .find(|group| group.label.as_ref() == declaring)
                    {
                        Some(group) => group.members.push(ix),
                        None => groups.push(Group {
                            label: declaring.into(),
                            icon: match text(method, "declaringClassType").as_str() {
                                "interface" => "nodes/interface",
                                _ => "nodes/class",
                            },
                            members: vec![ix],
                        }),
                    }
                }
                let members: Vec<Member> = methods
                    .iter()
                    .map(|method| Member {
                        icon: match flag(method, "unimplemented") {
                            true => "nodes/methodAbstract",
                            false => "nodes/method",
                        },
                        ..method_member(method)
                    })
                    .collect();
                // Implement selects them all; Override the first
                let selected = (0..members.len()).map(|ix| implement || ix == 0).collect();
                MemberChooser {
                    title: match implement {
                        true => "Select Methods to Implement",
                        false => "Select Methods to Override",
                    },
                    groups,
                    selected,
                    members,
                    allow_empty: false,
                    multiple: true,
                }
                .show(
                    move |chosen, window, cx| {
                        let methods: Vec<Value> = chosen
                            .iter()
                            .filter_map(|ix| methods.get(*ix).cloned())
                            .collect();
                        let params =
                            json!({"context": context.clone(), "overridableMethods": methods});
                        generate::<AddOverridableMethods>(
                            target.clone(),
                            server.clone(),
                            "Override/Implement Methods",
                            params,
                            window,
                            cx,
                        )
                    },
                    window,
                    cx,
                );
            })
        })
        .detach_and_log_err(cx);
}

/// Delegate Methods…: IntelliJ's Select Target to Generate Delegates for, then Select Methods to
/// Generate Delegates for.
fn delegate_methods(target: Target, window: &mut Window, cx: &mut App) {
    let (Some(server), Some(context)) = (
        java_project::language_server(&target.project, cx),
        context(&target, cx),
    ) else {
        return;
    };
    window
        .spawn(cx, async move |cx| {
            let answer = request::<CheckDelegateMethodsStatus>(&server, context.clone()).await?;
            let delegates = list(&answer, "delegateFields");
            cx.update(|window, cx| {
                if delegates.is_empty() {
                    let editor = target.editor.downgrade();
                    spork_code::show_hint(&editor, "No target to delegate to has been found", true, window, cx);
                    return;
                }
                let members: Vec<Member> = delegates
                    .iter()
                    .map(|delegate| field_member(delegate.get("field").unwrap_or(&Value::Null)))
                    .collect();
                let methods_step = move |delegate: Value, window: &mut Window, cx: &mut App| {
                    let field = delegate.get("field").cloned().unwrap_or_default();
                    let methods = list(&delegate, "delegateMethods");
                    let members: Vec<Member> = methods.iter().map(method_member).collect();
                    let (target, server, context) = (target.clone(), server.clone(), context.clone());
                    MemberChooser {
                        title: "Select Methods to Generate Delegates for",
                        groups: vec![Group {
                            label: text(&field, "type").into(),
                            icon: "nodes/class",
                            members: (0..members.len()).collect(),
                        }],
                        selected: vec![false; members.len()],
                        members,
                        allow_empty: false,
                        multiple: true,
                    }
                    .show(
                        move |chosen, window, cx| {
                            let entries: Vec<Value> = chosen
                                .iter()
                                .filter_map(|ix| methods.get(*ix))
                                .map(|method| json!({"field": field.clone(), "delegateMethod": method}))
                                .collect();
                            let params = json!({"context": context.clone(), "delegateEntries": entries});
                            generate::<GenerateDelegateMethods>(
                                target.clone(),
                                server.clone(),
                                "Generate Delegate Methods",
                                params,
                                window,
                                cx,
                            )
                        },
                        window,
                        cx,
                    );
                };
                let methods_step = Rc::new(methods_step);
                MemberChooser {
                    title: "Select Target to Generate Delegates for",
                    groups: vec![Group {
                        label: "".into(),
                        icon: "nodes/class",
                        members: (0..members.len()).collect(),
                    }],
                    selected: (0..members.len()).map(|ix| ix == 0).collect(),
                    members,
                    allow_empty: false,
                    multiple: false,
                }
                .show(
                    move |chosen, window, cx| {
                        if let Some(delegate) = chosen.first().and_then(|ix| delegates.get(*ix)) {
                            let methods_step = methods_step.clone();
                            let delegate = delegate.clone();
                            window.defer(cx, move |window, cx| methods_step(delegate, window, cx));
                        }
                    },
                    window,
                    cx,
                );
            })
        })
        .detach_and_log_err(cx);
}

/// A member the chooser lists.
#[derive(Clone)]
struct Member {
    label: SharedString,
    icon: &'static str,
}

/// The members under one node: the class declaring them, which a nameless group leaves out.
#[derive(Clone)]
struct Group {
    label: SharedString,
    icon: &'static str,
    members: Vec<usize>,
}

/// IntelliJ's member chooser (`MemberChooser`): the members under their class, chosen by
/// selecting them (⌘-click adds one, ⇧-click a run), Select None where choosing none is allowed,
/// Cancel and OK.
// shortcut: no toolbar (Sort Alphabetically, Show Classes, Expand and Collapse All) and no
// options below the tree (Insert @Override, Copy JavaDoc): JDT LS's requests take none
struct MemberChooser {
    title: &'static str,
    groups: Vec<Group>,
    members: Vec<Member>,
    selected: Vec<bool>,
    allow_empty: bool,
    multiple: bool,
}

/// The chooser's state while it shows.
struct ChooserState {
    selected: Vec<bool>,
    anchor: Option<usize>,
}

impl MemberChooser {
    fn show(
        self,
        on_ok: impl Fn(Vec<usize>, &mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut App,
    ) {
        let state: Entity<ChooserState> = cx.new(|_| ChooserState {
            selected: self.selected.clone(),
            anchor: self.selected.iter().position(|on| *on),
        });
        let chosen = {
            let state = state.clone();
            move |cx: &App| -> Vec<usize> {
                state
                    .read(cx)
                    .selected
                    .iter()
                    .enumerate()
                    .filter_map(|(ix, on)| on.then_some(ix))
                    .collect()
            }
        };
        let on_ok = Rc::new(on_ok);
        let chooser = Rc::new(self);
        window.open_dialog(cx, move |dialog, _, cx| {
            let tokens = Tokens::get(cx);
            let chooser = chooser.clone();
            let selected = state.read(cx).selected.clone();
            let none = !selected.iter().any(|on| *on);
            let confirm = {
                let (on_ok, chosen) = (on_ok.clone(), chosen.clone());
                move |window: &mut Window, cx: &mut App| {
                    let chosen = chosen(cx);
                    window.close_dialog(cx);
                    on_ok(chosen, window, cx);
                }
            };
            let can_confirm = chooser.allow_empty || !none;
            let rows = chooser.groups.iter().flat_map(|group| {
                let header = (!group.label.is_empty()).then(|| {
                    h_flex()
                        .id(SharedString::from(format!("member-group-{}", group.label)))
                        .role(Role::TreeItem)
                        .aria_label(group.label.clone())
                        .aria_expanded(true)
                        .h(px(24.))
                        .px(px(8.))
                        .gap(px(4.))
                        .child(spork_workbench::icon("general/chevronDown").size(px(16.)))
                        .child(spork_workbench::image(group.icon, cx).size(px(16.)))
                        .child(group.label.clone())
                        .into_any_element()
                });
                let indent = if group.label.is_empty() { 8. } else { 44. };
                let members = group.members.iter().map(|ix| {
                    let ix = *ix;
                    let member = &chooser.members[ix];
                    let on = selected.get(ix).copied().unwrap_or(false);
                    let multiple = chooser.multiple;
                    let state = state.clone();
                    h_flex()
                        .id(("member", ix))
                        .role(Role::TreeItem)
                        .aria_label(member.label.clone())
                        .aria_selected(on)
                        .h(px(24.))
                        .mx(px(4.))
                        .pl(px(indent))
                        .gap(px(6.))
                        .rounded(px(4.))
                        .when(on, |row| row.bg(tokens.selection))
                        .when(!on, |row| row.hover(|row| row.bg(tokens.hover)))
                        .child(spork_workbench::image(member.icon, cx).size(px(16.)))
                        .child(member.label.clone())
                        .on_click(move |event, window, cx| {
                            let modifiers = event.modifiers();
                            state.update(cx, |state, _| {
                                match (multiple, modifiers.platform, modifiers.shift, state.anchor)
                                {
                                    (true, true, _, _) => {
                                        state.selected[ix] = !state.selected[ix];
                                        state.anchor = Some(ix);
                                    }
                                    (true, _, true, Some(anchor)) => {
                                        let run = anchor.min(ix)..=anchor.max(ix);
                                        for (other, on) in state.selected.iter_mut().enumerate() {
                                            *on = run.contains(&other);
                                        }
                                    }
                                    _ => {
                                        for (other, on) in state.selected.iter_mut().enumerate() {
                                            *on = other == ix;
                                        }
                                        state.anchor = Some(ix);
                                    }
                                }
                            });
                            window.refresh();
                        })
                        .into_any_element()
                });
                header.into_iter().chain(members).collect::<Vec<_>>()
            });
            let ok = {
                let confirm = confirm.clone();
                move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| confirm(window, cx)
            };
            dialog
                .title(dialog_title(chooser.title))
                .w(px(560.))
                .close_button(false)
                .on_ok({
                    let confirm = confirm.clone();
                    move |_, window, cx| {
                        if can_confirm {
                            confirm(window, cx);
                        }
                        false
                    }
                })
                .child(
                    v_flex()
                        .id("member-chooser")
                        .role(Role::Group)
                        .aria_label(chooser.title)
                        .gap(px(12.))
                        .child(
                            div()
                                .id("member-chooser-tree")
                                .role(Role::Tree)
                                .aria_label("Members")
                                .h(px(320.))
                                .py(px(4.))
                                .border_1()
                                .border_color(tokens.separator)
                                .overflow_y_scroll()
                                .children(rows),
                        )
                        .child(
                            h_flex()
                                .gap(px(8.))
                                .when(chooser.allow_empty && chooser.multiple, |row| {
                                    let state = state.clone();
                                    row.child(
                                        Button::new("member-chooser-select-none", "Select None")
                                            .on_click(move |_, window, cx| {
                                                state.update(cx, |state, _| {
                                                    state
                                                        .selected
                                                        .iter_mut()
                                                        .for_each(|on| *on = false)
                                                });
                                                window.refresh();
                                            }),
                                    )
                                })
                                .child(div().flex_1())
                                .child(
                                    Button::new("member-chooser-cancel", "Cancel")
                                        .on_click(|_, window, cx| window.close_dialog(cx)),
                                )
                                .child(
                                    Button::new("member-chooser-ok", "OK")
                                        .primary(true)
                                        .disabled(!can_confirm)
                                        .on_click(ok),
                                ),
                        ),
                )
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{field_member, method_member};
    use serde_json::json;

    #[test]
    fn indents_generated_code_with_spaces() {
        let mut edit = json!({"changes": {"file:///A.java": [{"newText": "\t@Override\n\tint a() {\n\t\t// TODO Auto-generated method stub\n\t\treturn 1;\n\t}"}]}});
        super::to_intellij_style(&mut edit, 4, false);
        assert_eq!(
            edit["changes"]["file:///A.java"][0]["newText"],
            "    @Override\n    int a() {\n        return 1;\n    }"
        );
    }

    #[test]
    fn lists_members_as_the_chooser_writes_them() {
        assert_eq!(
            field_member(&json!({"name": "name", "type": "String"}))
                .label
                .as_ref(),
            "name:String"
        );
        assert_eq!(
            method_member(&json!({"name": "equals", "parameters": ["Object"]}))
                .label
                .as_ref(),
            "equals(Object)"
        );
    }
}
