//! New ▸ Kotlin Class/File (`Kotlin.NewFile`): IntelliJ's New Kotlin Class/File popup, a name
//! over the kinds Class, File, Interface, Data class, Enum class, Annotation, Kotlin script and
//! Object, each written with IntelliJ's internal template (an empty File Header).

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Role, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div, px,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use project_panel::NewItem;
use spork_workbench::controls::dialog_title;
use spork_workbench::{Tokens, Typography as _, popup};

/// A kind of the popup: its name, icon, extension and what the file says after its package line
/// (`{name}` the class's name); a script has no package and no folders in its name.
struct Kind {
    name: &'static str,
    icon: &'static str,
    extension: &'static str,
    body: &'static str,
    script: bool,
}

const fn kind(name: &'static str, icon: &'static str, body: &'static str) -> Kind {
    Kind {
        name,
        icon,
        extension: "kt",
        body,
        script: false,
    }
}

/// IntelliJ's kinds in its order (the sealed ones are behind a registry key, off by default).
const KINDS: [Kind; 8] = [
    kind("Class", "kotlin/classKotlin", "class {name} {\n}\n"),
    kind("File", "fileTypes/kotlin", ""),
    kind(
        "Interface",
        "kotlin/interfaceKotlin",
        "interface {name} {\n}\n",
    ),
    kind("Data class", "kotlin/classKotlin", "data class {name}()\n"),
    kind(
        "Enum class",
        "kotlin/enumKotlin",
        "enum class {name} {\n}\n",
    ),
    kind(
        "Annotation",
        "kotlin/annotationKotlin",
        "annotation class {name}()\n",
    ),
    Kind {
        name: "Kotlin script",
        icon: "kotlin/kotlinScript",
        extension: "kts",
        body: "#!/usr/bin/env kotlin\n\n",
        script: true,
    },
    kind("Object", "kotlin/objectKotlin", "object {name} {\n}\n"),
];

/// Kotlin Class/File in a folder of a source root, after Java Class.
pub(crate) fn new_items(source_root: &Path, folder: &Path) -> Vec<NewItem> {
    let (root, folder) = (source_root.to_owned(), folder.to_owned());
    vec![NewItem {
        label: "Kotlin Class/File".into(),
        run: Rc::new(move |window: &mut Window, cx: &mut App| {
            show_popup(root.clone(), folder.clone(), window, cx)
        }),
        after: false,
    }]
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

/// IntelliJ's check of the name (`NewKotlinFileNameValidator`): its error, if any.
fn name_error(name: &str) -> Option<&'static str> {
    if name.trim().is_empty() {
        return Some("Name can't be empty");
    }
    let parts: Vec<&str> = name.split(['/', '\\', '.']).collect();
    if parts.iter().any(|part| part.trim().is_empty()) {
        return Some("Name can't have empty parts");
    }
    if parts
        .iter()
        .any(|part| part.trim().chars().all(|c| c == '_'))
    {
        return Some("Name can't consist only of underscores");
    }
    None
}

/// The file a name of a kind makes in `folder`, and its text: a class's dots and slashes make
/// folders (`model.Point`), a file's and a script's slashes; a Kotlin extension typed is dropped.
// shortcut: the last part names the file, where IntelliJ keeps a trailing lower-case part with the
// class before it (`Foo.bar`); follow it if someone types such names
fn kotlin_file(source_root: &Path, folder: &Path, name: &str, kind: &Kind) -> (PathBuf, String) {
    let name = [".gradle.kts", ".main.kts", ".kts", ".kt"]
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(name)
        .trim();
    let separators: &[char] = match kind.script || kind.name == "File" {
        true => &['/', '\\'],
        false => &['/', '\\', '.'],
    };
    let mut parts: Vec<&str> = name.split(separators).collect();
    let file = parts.pop().unwrap_or(name);
    let folder = parts
        .iter()
        .fold(folder.to_owned(), |path, part| path.join(part));
    let header = match kind.script {
        true => String::new(),
        false => package_of(source_root, &folder)
            .map(|package| format!("package {package}\n\n"))
            .unwrap_or_default(),
    };
    let class = file.split('.').next().unwrap_or(file);
    let text = format!("{header}{}", kind.body.replace("{name}", class));
    (folder.join(format!("{file}.{}", kind.extension)), text)
}

/// IntelliJ's New Kotlin Class/File popup: the name over the kinds; ↑ and ↓ choose the kind, ⏎
/// makes the file, an invalid or taken name says why under the field.
struct NewFilePopup {
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
            |this: &mut NewFilePopup, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.confirm(window, cx),
                InputEvent::Change if this.error.is_some() => {
                    this.error = None;
                    cx.notify();
                }
                _ => {}
            },
        );
        NewFilePopup {
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
            .title(dialog_title("New Kotlin Class/File"))
            .on_ok(|_, _, _| false)
            .w(px(400.))
            .centered()
            .overlay(false)
            .close_button(false)
            .child(popup.clone())
    });
    input.update(cx, |input, cx| input.focus(window, cx));
}

impl NewFilePopup {
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.input.read(cx).value().trim().to_owned();
        if let Some(error) = name_error(&name) {
            self.error = Some(error.into());
            cx.notify();
            return;
        }
        let (path, text) = kotlin_file(&self.source_root, &self.folder, &name, &KINDS[self.kind]);
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
        spork_java::create_and_open(path, text, window, cx);
    }

    fn step(&mut self, step: isize, cx: &mut Context<Self>) {
        self.kind = (self.kind as isize + step).rem_euclid(KINDS.len() as isize) as usize;
        cx.notify();
    }
}

impl Render for NewFilePopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        v_flex()
            .id("new-kotlin-file")
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
                    .id("new-kotlin-file-kinds")
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
    use super::{KINDS, kotlin_file, name_error};
    use std::path::Path;

    #[test]
    fn writes_each_kind_in_its_package() {
        let root = Path::new("/p/fixture/src/main/kotlin");
        let folder = root.join("com/example");
        let (path, text) = kotlin_file(root, &folder, "model.Point", &KINDS[3]);
        assert_eq!(path, root.join("com/example/model/Point.kt"));
        assert_eq!(text, "package com.example.model\n\ndata class Point()\n");
        let (path, text) = kotlin_file(root, &folder, "Utils.kt", &KINDS[1]);
        assert_eq!(path, root.join("com/example/Utils.kt"));
        assert_eq!(text, "package com.example\n\n");
        let (path, text) = kotlin_file(root, &folder, "build", &KINDS[6]);
        assert_eq!(path, root.join("com/example/build.kts"));
        assert_eq!(text, "#!/usr/bin/env kotlin\n\n");
        let (_, text) = kotlin_file(root, root, "Shape", &KINDS[0]);
        assert_eq!(text, "class Shape {\n}\n");
    }

    #[test]
    fn checks_the_name_as_intellij() {
        assert_eq!(name_error(" "), Some("Name can't be empty"));
        assert_eq!(name_error("a..B"), Some("Name can't have empty parts"));
        assert_eq!(
            name_error("__"),
            Some("Name can't consist only of underscores")
        );
        assert_eq!(name_error("model.Point"), None);
    }
}
