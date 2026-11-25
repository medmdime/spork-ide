//! IntelliJ's Java file templates in New (`CreateClassAction`, `fileTemplates/internal`): Java
//! Class, its New Java Class popup (a name, then Class, Interface, Record, Enum, Annotation or
//! Exception), package-info.java in a package and module-info.java in a source root.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Role, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, TaskExt as _, Window, div, px,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use project_panel::NewItem;
use spork_workbench::controls::dialog_title;
use spork_workbench::{Tokens, Typography as _, popup};
use util::ResultExt as _;
use workspace::{OpenOptions, Workspace};

/// A kind of the New Java Class popup: its name, icon and what the file says.
struct Kind {
    name: &'static str,
    icon: &'static str,
    /// The declaration after the package line, `{name}` the class's name.
    body: &'static str,
}

/// The kinds a Java 16+ project offers (records need 16), IntelliJ's internal templates with an
/// empty File Header.
const KINDS: [Kind; 6] = [
    Kind {
        name: "Class",
        icon: "nodes/class",
        body: "public class {name} {\n}\n",
    },
    Kind {
        name: "Interface",
        icon: "nodes/interface",
        body: "public interface {name} {\n}\n",
    },
    Kind {
        name: "Record",
        icon: "nodes/record",
        body: "public record {name}() {\n}\n",
    },
    Kind {
        name: "Enum",
        icon: "nodes/enum",
        body: "public enum {name} {\n}\n",
    },
    Kind {
        name: "Annotation",
        icon: "nodes/annotation",
        body: "public @interface {name} {\n}\n",
    },
    Kind {
        name: "Exception",
        icon: "nodes/exception",
        body: "public class {name} extends RuntimeException {\n  public {name}(String message) {\n    super(message);\n  }\n}\n",
    },
];

/// Another language's items of New in a source root (`spork_kotlin`'s Kotlin Class/File), after
/// Java Class.
static MORE_NEW_ITEMS: std::sync::OnceLock<fn(&Path, &Path) -> Vec<NewItem>> =
    std::sync::OnceLock::new();

/// Adds a language's items to New in a source root: `items(source_root, folder)`.
pub fn add_new_items(items: fn(&Path, &Path) -> Vec<NewItem>) {
    if MORE_NEW_ITEMS.set(items).is_err() {
        log::error!("a language's New items were added twice");
    }
}

/// What New offers in a folder of a source root: Java Class; package-info.java in a package
/// that has none, module-info.java in a source root that has none.
pub(crate) fn new_items(source_root: &Path, folder: &Path) -> Vec<NewItem> {
    let mut items = vec![NewItem {
        label: "Java Class".into(),
        run: {
            let (root, folder) = (source_root.to_owned(), folder.to_owned());
            Rc::new(move |window: &mut Window, cx: &mut App| {
                show_popup(root.clone(), folder.clone(), window, cx)
            })
        },
        after: false,
    }];
    if let Some(more) = MORE_NEW_ITEMS.get() {
        items.extend(more(source_root, folder));
    }
    let package = package_of(source_root, folder);
    let file = |name: &'static str, text: String| {
        let path = folder.join(name);
        NewItem {
            label: name.into(),
            run: Rc::new(move |window: &mut Window, cx: &mut App| {
                create_and_open(path.clone(), text.clone(), window, cx)
            }),
            after: true,
        }
    };
    match package {
        Some(package) if !folder.join("package-info.java").exists() => {
            items.push(file("package-info.java", format!("package {package};\n")))
        }
        None if !folder.join("module-info.java").exists() => {
            let module = module_name(source_root);
            items.push(file(
                "module-info.java",
                format!("module {module} {{\n}}\n"),
            ))
        }
        _ => {}
    }
    items
}

/// The package of a folder under a source root (`com.example`), `None` for the root itself.
fn package_of(source_root: &Path, folder: &Path) -> Option<String> {
    let relative = folder.strip_prefix(source_root).ok()?;
    let parts: Vec<String> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join("."))
}

/// A module's name from the project's folder, as IntelliJ makes one of its module's name: the
/// characters a Java name can't have become dots.
fn module_name(source_root: &Path) -> String {
    // src/main/java is three folders under the project
    let project = source_root
        .ancestors()
        .nth(3)
        .and_then(|project| project.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "module".into());
    project
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '.'
            }
        })
        .collect()
}

const KEYWORDS: [&str; 50] = [
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "try",
    "void",
    "volatile",
    "while",
];

/// Whether `name` is a Java qualified name: identifiers between dots, no keyword.
fn is_qualified_name(name: &str) -> bool {
    name.split('.').all(|part| {
        let mut chars = part.chars();
        chars
            .next()
            .is_some_and(|first| first.is_alphabetic() || first == '_' || first == '$')
            && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
            && !KEYWORDS.contains(&part)
            && !["true", "false", "null"].contains(&part)
    })
}

/// The file a class `name` (`Foo`, `sub.Foo`) of a kind makes in `folder`, and its text.
fn class_file(source_root: &Path, folder: &Path, name: &str, kind: &Kind) -> (PathBuf, String) {
    let mut parts: Vec<&str> = name.split('.').collect();
    let class = parts.pop().unwrap_or(name);
    let folder = parts
        .iter()
        .fold(folder.to_owned(), |path, part| path.join(part));
    let header = package_of(source_root, &folder)
        .map(|package| format!("package {package};\n\n"))
        .unwrap_or_default();
    let text = format!("{header}{}", kind.body.replace("{name}", class));
    (folder.join(format!("{class}.java")), text)
}

/// Writes a new file (its folders too) and opens it in the editor.
pub fn create_and_open(path: PathBuf, text: String, window: &mut Window, cx: &mut App) {
    let Some(workspace) = Workspace::for_window(window, cx) else {
        return;
    };
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&path, text));
    if written.log_err().is_none() {
        return;
    }
    workspace.update(cx, |workspace, cx| {
        workspace
            .open_abs_path(path, OpenOptions::default(), window, cx)
            .detach_and_log_err(cx);
    });
}

/// IntelliJ's New Java Class popup: the name over the kinds; ↑ and ↓ choose the kind, ⏎ makes
/// the class, an invalid or taken name says why under the field.
struct NewClassPopup {
    source_root: PathBuf,
    folder: PathBuf,
    input: Entity<InputState>,
    kind: usize,
    error: Option<String>,
    _subscription: Subscription,
}

fn show_popup(source_root: PathBuf, folder: PathBuf, window: &mut Window, cx: &mut App) {
    let input = cx.new(|cx| InputState::new(window, cx));
    let popup = cx.new(|cx| {
        let _subscription = cx.subscribe_in(
            &input,
            window,
            |this: &mut NewClassPopup, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.confirm(window, cx),
                InputEvent::Change if this.error.is_some() => {
                    this.error = None;
                    cx.notify();
                }
                _ => {}
            },
        );
        NewClassPopup {
            source_root,
            folder,
            input: input.clone(),
            kind: 0,
            error: None,
            _subscription,
        }
    });
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(dialog_title("New Java Class"))
            .on_ok(|_, _, _| false)
            .w(px(400.))
            .centered()
            .overlay(false)
            .close_button(false)
            .child(popup.clone())
    });
    input.update(cx, |input, cx| input.focus(window, cx));
}

impl NewClassPopup {
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.input.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return;
        }
        if !is_qualified_name(&name) {
            self.error = Some("This is not a valid Java qualified name".into());
            cx.notify();
            return;
        }
        let (path, text) = class_file(&self.source_root, &self.folder, &name, &KINDS[self.kind]);
        if path.exists() {
            self.error = Some(format!(
                "File '{}' already exists.",
                path.file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_default()
            ));
            cx.notify();
            return;
        }
        window.close_dialog(cx);
        create_and_open(path, text, window, cx);
    }

    fn step(&mut self, step: isize, cx: &mut Context<Self>) {
        self.kind = (self.kind as isize + step).rem_euclid(KINDS.len() as isize) as usize;
        cx.notify();
    }
}

impl Render for NewClassPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        v_flex()
            .id("new-java-class")
            .gap(px(6.))
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                match event.keystroke.key.as_str() {
                    "up" => this.step(-1, cx),
                    "down" => this.step(1, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(spork_workbench::TextField::new(&self.input).aria_label("Name"))
            .children(self.error.clone().map(|error| {
                div()
                    .id("name-error")
                    .role(Role::Alert)
                    .aria_label(error.clone())
                    .text_ui_small()
                    .text_color(tokens.text_error)
                    .child(error)
            }))
            .child(
                v_flex()
                    .id("new-java-class-kinds")
                    .role(Role::List)
                    .aria_label("Kinds")
                    .children(KINDS.iter().enumerate().map(|(ix, kind)| {
                        let selected = ix == self.kind;
                        popup::row(("kind", ix), selected, cx)
                            .role(Role::ListItem)
                            .aria_label(SharedString::from(kind.name))
                            .aria_selected(selected)
                            .child(
                                h_flex()
                                    .gap(px(6.))
                                    .child(spork_workbench::image(kind.icon, cx))
                                    .child(kind.name),
                            )
                            .on_click(cx.listener(
                                move |this, event: &gpui::ClickEvent, window, cx| {
                                    this.kind = ix;
                                    if event.click_count() == 2 {
                                        this.confirm(window, cx);
                                    }
                                    cx.notify();
                                },
                            ))
                            .when(selected, |row| row.text_color(tokens.text))
                    })),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{KINDS, class_file, is_qualified_name, module_name, package_of};
    use std::path::Path;

    #[test]
    fn makes_a_class_in_its_package() {
        let root = Path::new("/p/fixture/src/main/java");
        let folder = root.join("com/example");
        assert_eq!(package_of(root, &folder).as_deref(), Some("com.example"));
        assert_eq!(package_of(root, root), None);
        let (path, text) = class_file(root, &folder, "util.Strings", &KINDS[0]);
        assert_eq!(path, root.join("com/example/util/Strings.java"));
        assert_eq!(
            text,
            "package com.example.util;\n\npublic class Strings {\n}\n"
        );
        assert_eq!(module_name(root), "fixture");
    }

    #[test]
    fn checks_a_qualified_name() {
        assert!(is_qualified_name("Person"));
        assert!(is_qualified_name("model.Person"));
        assert!(!is_qualified_name("class"));
        assert!(!is_qualified_name("1st"));
        assert!(!is_qualified_name("a..b"));
    }
}
