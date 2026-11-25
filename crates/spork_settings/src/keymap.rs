//! The Keymap page, as IntelliJ's (`KeymapPanel`): the actions in IntelliJ's groups (Editor
//! Actions, Main Menu as the menu bar has it, Tool Windows, Other) with their shortcuts, searched
//! by name or by a shortcut, and Add Keyboard Shortcut / Remove from an action's context menu.
//! Zed's keymap is the engine: the shortcuts are the keymap's bindings, and the changes, kept
//! until the dialog applies them, go into the user keymap (`keymap.json`) as Zed's keymap editor
//! writes them (`KeymapFile::update_keybinding`).

use std::rc::Rc;

use collections::HashSet;
use gpui_kit::component::dialog::DialogEntrance;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Value, json};
use settings::{KeybindSource, KeybindUpdateOperation, KeybindUpdateTarget, KeymapFile};
use spork_workbench::{Tokens, Typography as _};
use util::ResultExt as _;

use crate::text_label;

/// Words the search finds the Keymap page by.
pub(crate) const WORDS: &str = "keymap shortcut keyboard shortcuts bindings keys macos find actions by shortcut add keyboard shortcut remove";

/// The settings the Keymap page writes through the dialog: the keymap picked.
pub(crate) const PATHS: &[&str] = &["/base_keymap"];

/// The tool windows' actions, by IntelliJ's names.
const TOOL_WINDOWS: &[(&str, &str)] = &[
    ("Project", "project_panel::ToggleFocus"),
    ("Commit", "commit_panel::ToggleFocus"),
    ("Structure", "outline_panel::ToggleFocus"),
    ("Git", "git_panel::ToggleFocus"),
    ("Terminal", "terminal_panel::ToggleFocus"),
    ("Debug", "debug_panel::ToggleFocus"),
    ("AI Chat", "agent::ToggleFocus"),
];

/// An action of the tree.
struct Entry {
    label: SharedString,
    action: Rc<dyn Action>,
    /// Where it is: "Main Menu | Edit", for the shortcut dialog.
    place: SharedString,
}

enum Item {
    Group(Group),
    Action(Entry),
}

struct Group {
    name: SharedString,
    items: Vec<Item>,
}

/// A shortcut of an action: its keystrokes as the keymap has them, the key context they work in
/// and where they come from.
#[derive(Clone)]
struct Shortcut {
    action: &'static str,
    keystrokes: Vec<KeybindingKeystroke>,
    context: Option<String>,
    arguments: Option<SharedString>,
    source: KeybindSource,
}

impl Shortcut {
    fn target(&self) -> KeybindUpdateTarget<'_> {
        KeybindUpdateTarget {
            context: self.context.as_deref(),
            keystrokes: &self.keystrokes,
            action_name: self.action,
            action_arguments: self.arguments.as_deref(),
        }
    }

    fn text(&self) -> String {
        self.keystrokes
            .iter()
            .map(keystroke_text)
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn same(&self, other: &Shortcut) -> bool {
        self.action == other.action
            && same_keys(&self.keystrokes, &other.keystrokes)
            && self.context == other.context
    }
}

/// A change to the keymap, kept until Apply.
#[derive(Clone)]
enum Edit {
    Add(Shortcut),
    Remove(Shortcut),
}

/// A row shown: a group (by its path) or an action, and how deep.
enum Row<'a> {
    Group {
        path: String,
        name: SharedString,
        open: bool,
        depth: usize,
    },
    Action {
        entry: &'a Entry,
        depth: usize,
    },
}

/// The Keyboard Shortcut dialog's (or Find Shortcut's) recording.
struct Recording {
    /// The action the shortcut is for; None when finding actions by a shortcut.
    entry: Option<(SharedString, Rc<dyn Action>, SharedString)>,
    first: Option<KeybindingKeystroke>,
    second: Option<KeybindingKeystroke>,
    second_on: bool,
    /// The second stroke's field takes the keys.
    on_second: bool,
    _intercept: Subscription,
}

pub(crate) struct KeymapPage {
    groups: Vec<Item>,
    open: HashSet<String>,
    selected: Option<&'static str>,
    filter: Entity<InputState>,
    /// Find Actions by Shortcut: the actions whose shortcut starts with these keystrokes.
    shortcut_filter: Option<Vec<KeybindingKeystroke>>,
    edits: Vec<Edit>,
    recording: Option<Recording>,
    /// Shown from Find Action's ⌥⏎ (Add Shortcut…), outside Settings: the shortcut goes into the
    /// keymap at once, as IntelliJ's `KeymapPanel.addKeyboardShortcut`.
    standalone: bool,
    scroll: UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

/// IntelliJ's Add Shortcut… (`SetShortcutAction`, ⌥⏎ on an action in Find Action): the Keyboard
/// Shortcut dialog for `action` in `window`, the shortcut written into the user keymap on OK.
pub fn add_shortcut(action: &dyn Action, window: &mut Window, cx: &mut App) {
    let page = cx.new(|cx| KeymapPage::new(window, cx));
    page.update(cx, |page, cx| {
        page.standalone = true;
        let entry = page
            .entry(action.name())
            .map(|entry| {
                (
                    entry.label.clone(),
                    entry.action.clone(),
                    entry.place.clone(),
                )
            })
            .unwrap_or_else(|| {
                (
                    humanize(action.name()).into(),
                    Rc::from(action.boxed_clone()),
                    "Other".into(),
                )
            });
        page.record(Some(entry), window, cx);
    });
}

impl KeymapPage {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx));
        let _subscriptions = vec![cx.subscribe(&filter, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                // a text search ends a search by shortcut, as IntelliJ's
                this.shortcut_filter = None;
                cx.notify();
            }
        })];
        Self {
            groups: build_groups(cx),
            open: HashSet::default(),
            selected: None,
            filter,
            shortcut_filter: None,
            edits: Vec::new(),
            recording: None,
            standalone: false,
            scroll: UniformListScrollHandle::new(),
            _subscriptions,
        }
    }

    pub(crate) fn modified(&self) -> bool {
        !self.edits.is_empty()
    }

    /// Writes the changes into the user keymap, one after the other, as Zed's keymap editor.
    pub(crate) fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let edits = std::mem::take(&mut self.edits);
        if edits.is_empty() {
            return;
        }
        let fs = <dyn fs::Fs>::global(cx);
        let keyboard_mapper = cx.keyboard_mapper().clone();
        let aliases = cx.deprecated_actions_to_preferred_actions().clone();
        let os = cx.platform_info().os;
        cx.spawn_in(window, async move |_, cx| {
            let result: anyhow::Result<()> = async {
                for edit in &edits {
                    let mut contents = KeymapFile::load_keymap_file(&fs).await?;
                    let tab_size = settings::infer_json_indent_size(&contents);
                    let operation = match edit {
                        Edit::Add(shortcut) => KeybindUpdateOperation::Add {
                            source: shortcut.target(),
                            from: None,
                        },
                        Edit::Remove(shortcut) => KeybindUpdateOperation::Remove {
                            target: shortcut.target(),
                            target_keybind_source: shortcut.source,
                        },
                    };
                    contents = KeymapFile::update_keybinding(
                        operation,
                        contents,
                        tab_size,
                        keyboard_mapper.as_ref(),
                        &aliases,
                        os,
                    )?;
                    fs.write(paths::keymap_file().as_path(), contents.as_bytes())
                        .await?;
                }
                Ok(())
            }
            .await;
            if let Err(error) = result {
                cx.update(|window, cx| {
                    window.push_notification(
                        gpui_kit::component::notification::Notification::error(format!(
                            "Couldn't save the keymap: {error}"
                        )),
                        cx,
                    )
                })
                .log_err();
            }
        })
        .detach();
    }

    /// An action's shortcuts: the keymap's, with the changes not applied yet.
    fn shortcuts(&self, action: &dyn Action, cx: &App) -> Vec<Shortcut> {
        let name = action.name();
        let mut shortcuts: Vec<Shortcut> = cx
            .key_bindings()
            .borrow()
            .bindings_for_action(action)
            .map(|binding| Shortcut {
                action: name,
                keystrokes: binding.keystrokes().to_vec(),
                context: binding.predicate().map(|predicate| predicate.to_string()),
                arguments: binding.action_input(),
                source: binding
                    .meta()
                    .map(KeybindSource::from_meta)
                    .unwrap_or(KeybindSource::Unknown),
            })
            .collect();
        // the last binding of a keystroke wins: show each once
        shortcuts.reverse();
        let mut seen = HashSet::default();
        shortcuts.retain(|shortcut| seen.insert(shortcut.text()));
        shortcuts.reverse();
        for edit in &self.edits {
            match edit {
                Edit::Add(shortcut) if shortcut.action == name => shortcuts.push(shortcut.clone()),
                Edit::Remove(removed) => shortcuts.retain(|shortcut| !shortcut.same(removed)),
                Edit::Add(_) => {}
            }
        }
        shortcuts
    }

    /// The rows shown: every group open while a search narrows the tree to what it finds.
    fn rows(&self, cx: &App) -> Vec<Row<'_>> {
        let query = self.filter.read(cx).value().trim().to_lowercase();
        let searching = !query.is_empty() || self.shortcut_filter.is_some();
        let found = |entry: &Entry| -> bool {
            if !query.is_empty() && !entry.label.to_lowercase().contains(&query) {
                return false;
            }
            match &self.shortcut_filter {
                Some(keys) => self
                    .shortcuts(entry.action.as_ref(), cx)
                    .iter()
                    .any(|shortcut| {
                        shortcut.keystrokes.len() >= keys.len()
                            && same_keys(&shortcut.keystrokes[..keys.len()], keys)
                    }),
                None => true,
            }
        };
        let mut rows = Vec::new();
        fn walk<'a>(
            items: &'a [Item],
            parent: &str,
            depth: usize,
            open: &HashSet<String>,
            searching: bool,
            found: &dyn Fn(&Entry) -> bool,
            rows: &mut Vec<Row<'a>>,
        ) -> bool {
            let mut any = false;
            for item in items {
                match item {
                    Item::Group(group) => {
                        let path = format!("{parent}/{}", group.name);
                        let mut children = Vec::new();
                        let has = walk(
                            &group.items,
                            &path,
                            depth + 1,
                            open,
                            searching,
                            found,
                            &mut children,
                        );
                        if searching && !has {
                            continue;
                        }
                        any = true;
                        let is_open = searching || open.contains(&path);
                        rows.push(Row::Group {
                            path,
                            name: group.name.clone(),
                            open: is_open,
                            depth,
                        });
                        if is_open {
                            rows.extend(children);
                        }
                    }
                    Item::Action(entry) => {
                        if searching && !found(entry) {
                            continue;
                        }
                        any = true;
                        rows.push(Row::Action { entry, depth });
                    }
                }
            }
            any
        }
        walk(
            &self.groups,
            "",
            0,
            &self.open,
            searching,
            &found,
            &mut rows,
        );
        rows
    }

    fn expand_all(&mut self, open: bool, cx: &mut Context<Self>) {
        self.open.clear();
        if open {
            fn walk(items: &[Item], parent: &str, open: &mut HashSet<String>) {
                for item in items {
                    if let Item::Group(group) = item {
                        let path = format!("{parent}/{}", group.name);
                        walk(&group.items, &path, open);
                        open.insert(path);
                    }
                }
            }
            walk(&self.groups, "", &mut self.open);
        }
        cx.notify();
    }

    /// The actions bound to `keystrokes`, but `except`, by their names.
    fn conflicts(
        &self,
        keystrokes: &[KeybindingKeystroke],
        except: &str,
        cx: &App,
    ) -> Vec<(SharedString, Shortcut)> {
        let mut conflicts = Vec::new();
        fn walk(
            page: &KeymapPage,
            items: &[Item],
            keystrokes: &[KeybindingKeystroke],
            except: &str,
            cx: &App,
            conflicts: &mut Vec<(SharedString, Shortcut)>,
        ) {
            for item in items {
                match item {
                    Item::Group(group) => {
                        walk(page, &group.items, keystrokes, except, cx, conflicts)
                    }
                    Item::Action(entry) if entry.action.name() != except => {
                        for shortcut in page.shortcuts(entry.action.as_ref(), cx) {
                            if same_keys(&shortcut.keystrokes, keystrokes)
                                && !conflicts
                                    .iter()
                                    .any(|(_, s): &(SharedString, Shortcut)| s.same(&shortcut))
                            {
                                conflicts.push((entry.label.clone(), shortcut));
                            }
                        }
                    }
                    Item::Action(_) => {}
                }
            }
        }
        walk(self, &self.groups, keystrokes, except, cx, &mut conflicts);
        conflicts
    }

    /// Records keystrokes for the Keyboard Shortcut dialog (`entry`) or Find Shortcut (None):
    /// every key goes to the field taking them, but Esc (cancel), ⏎ (OK) and ⇥ (the other field).
    fn record(
        &mut self,
        entry: Option<(SharedString, Rc<dyn Action>, SharedString)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let page = cx.entity().downgrade();
        // keys reach the interceptor from any window: they go to the dialog's
        let settings_window = window.window_handle();
        let intercept = cx.intercept_keystrokes(move |event, window, cx| {
            let keystroke = event.keystroke.clone();
            let plain = !keystroke.modifiers.modified();
            // the window dispatching the keys is busy: it can't be updated again
            if window.window_handle() == settings_window {
                page.update(cx, |page, cx| page.key(keystroke, plain, window, cx))
                    .log_err();
            } else {
                settings_window
                    .update(cx, |_, window, cx| {
                        page.update(cx, |page, cx| page.key(keystroke, plain, window, cx))
                    })
                    .log_err();
            }
            cx.stop_propagation();
        });
        let finding = entry.is_none();
        self.recording = Some(Recording {
            entry,
            first: None,
            second: None,
            second_on: false,
            on_second: false,
            _intercept: intercept,
        });
        self.open_recording(finding, window, cx);
    }

    /// A key while recording: Esc cancels, ⏎ is OK, ⇥ goes to the other field, any other key
    /// is the stroke of the field taking the keys.
    fn key(
        &mut self,
        keystroke: Keystroke,
        plain: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(recording) = &mut self.recording else {
            return;
        };
        match keystroke.key.as_str() {
            "escape" if plain => {
                self.recording = None;
                window.close_dialog(cx);
            }
            "enter" if plain => self.finish(window, cx),
            "tab" if plain && recording.second_on => recording.on_second = !recording.on_second,
            "" => {}
            _ => {
                // the keys alone: the keymap file gets no character
                let stroke = KeybindingKeystroke::from_keystroke(Keystroke {
                    key_char: None,
                    ..keystroke
                });
                match recording.on_second {
                    true => recording.second = Some(stroke),
                    false => recording.first = Some(stroke),
                }
                if recording.entry.is_none() {
                    self.shortcut_filter = self.recorded();
                }
            }
        }
        cx.notify();
    }

    fn open_recording(&mut self, finding: bool, window: &mut Window, cx: &mut Context<Self>) {
        if finding {
            self.filter
                .update(cx, |filter, cx| filter.set_value("", window, cx));
        }
        let page = cx.entity();
        window.open_dialog(cx, move |dialog, _, cx| {
            let content = page.read(cx).recording_content(&page, cx);
            let title = match finding {
                true => "Find Shortcut",
                false => "Keyboard Shortcut",
            };
            let page_ok = page.clone();
            let page_close = page.clone();
            let ready = page.read(cx).recorded().is_some();
            // the buttons in the content, which assistive technology names after the dialog
            let buttons = (!finding).then(|| {
                h_flex()
                    .w_full()
                    .pt(px(8.))
                    .gap(px(12.))
                    .justify_end()
                    .child(
                        spork_workbench::Button::new("keyboard-shortcut-cancel", "Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        spork_workbench::Button::new("keyboard-shortcut-ok", "OK")
                            .primary(true)
                            .disabled(!ready)
                            .on_click(move |_, window, cx| {
                                page_ok.update(cx, |page, cx| page.finish(window, cx))
                            }),
                    )
            });
            dialog
                .entrance(DialogEntrance::None)
                .title(title)
                .width(px(380.))
                .child(
                    v_flex()
                        .id("keyboard-shortcut")
                        .role(Role::Group)
                        .aria_label(SharedString::from(title))
                        .child(content)
                        .children(buttons),
                )
                .on_close(move |_, _, cx| {
                    page_close.update(cx, |page, cx| {
                        page.recording = None;
                        cx.notify();
                    })
                })
        });
        cx.notify();
    }

    fn recorded(&self) -> Option<Vec<KeybindingKeystroke>> {
        let recording = self.recording.as_ref()?;
        let mut keys = vec![recording.first.clone()?];
        if recording.second_on
            && let Some(second) = &recording.second
        {
            keys.push(second.clone());
        }
        Some(keys)
    }

    /// The Keyboard Shortcut dialog's OK: the shortcut added; when other actions have it,
    /// IntelliJ's question whether to take it from them.
    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(keys) = self.recorded() else { return };
        let Some((_, action, _)) = self.recording.as_ref().and_then(|r| r.entry.clone()) else {
            // Find Shortcut: the filter stays, the popup closes
            self.recording = None;
            window.close_dialog(cx);
            return;
        };
        let conflicts = self.conflicts(&keys, action.name(), cx);
        self.recording = None;
        window.close_dialog(cx);
        let context = binding_context(action.as_ref(), cx);
        let added = Shortcut {
            action: action.name(),
            keystrokes: keys,
            context,
            arguments: None,
            source: KeybindSource::User,
        };
        if conflicts.is_empty() {
            self.add_edits(vec![Edit::Add(added)], window, cx);
            return;
        }
        let page = cx.entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let (remove, keep) = (page.clone(), page.clone());
            let (added_remove, added_keep) = (added.clone(), added.clone());
            let conflicts = conflicts.clone();
            dialog
                .entrance(DialogEntrance::None)
                .title("Warning")
                .width(px(420.))
                .child(
                v_flex()
                    .id("shortcut-warning")
                    .role(Role::Group)
                    .aria_label(SharedString::from("Warning"))
                    .gap(px(16.))
                    .child(text_label(
                        "shortcut-warning-text",
                        "This shortcut is already assigned to other actions. Do you want to remove the other assignments?",
                    ))
                    .child(
                        h_flex()
                            .gap(px(12.))
                            .justify_end()
                            .child(
                                spork_workbench::Button::new("shortcut-cancel", "Cancel")
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            )
                            .child(spork_workbench::Button::new("shortcut-keep", "Keep").on_click(
                                move |_, window, cx| {
                                    keep.update(cx, |page, cx| {
                                        page.add_edits(
                                            vec![Edit::Add(added_keep.clone())],
                                            window,
                                            cx,
                                        );
                                    });
                                    window.close_dialog(cx);
                                },
                            ))
                            .child(
                                spork_workbench::Button::new("shortcut-remove", "Remove")
                                    .primary(true)
                                    .on_click(move |_, window, cx| {
                                        remove.update(cx, |page, cx| {
                                            let mut edits: Vec<Edit> = conflicts
                                                .iter()
                                                .map(|(_, shortcut)| {
                                                    Edit::Remove(shortcut.clone())
                                                })
                                                .collect();
                                            edits.push(Edit::Add(added_remove.clone()));
                                            page.add_edits(edits, window, cx);
                                        });
                                        window.close_dialog(cx);
                                    }),
                            ),
                    ),
            )
        });
    }

    /// What the Keyboard Shortcut dialog or Find Shortcut shows: the action and where it is, the
    /// strokes' fields, and the actions the shortcut is already assigned to.
    fn recording_content(&self, page: &Entity<Self>, cx: &App) -> AnyElement {
        let Some(recording) = &self.recording else {
            return div().into_any_element();
        };
        let tokens = Tokens::get(cx);
        let stroke_field = |id: &'static str,
                            stroke: &Option<KeybindingKeystroke>,
                            active: bool,
                            enabled: bool,
                            second: bool| {
            let page = page.clone();
            div()
                .id(id)
                .role(Role::TextInput)
                .aria_label(SharedString::from(match second {
                    true => "Second stroke",
                    false => "First stroke",
                }))
                .aria_value(SharedString::from(
                    stroke.as_ref().map(keystroke_text).unwrap_or_default(),
                ))
                .h(px(spork_workbench::tokens::metrics::CONTROL_HEIGHT))
                .w_full()
                .px(px(8.))
                .flex()
                .items_center()
                .rounded(px(spork_workbench::tokens::metrics::RADIUS))
                .bg(tokens.control_bg)
                .border_1()
                .border_color(tokens.control_border)
                .when(active && enabled, |field| {
                    field.border_2().border_color(tokens.focus)
                })
                .when(!enabled, |field| field.text_color(tokens.text_disabled))
                .cursor_pointer()
                .child(stroke.as_ref().map(keystroke_text).unwrap_or_default())
                .on_click(move |_, _, cx| {
                    page.update(cx, |page, cx| {
                        if let Some(recording) = &mut page.recording
                            && (!second || recording.second_on)
                        {
                            recording.on_second = second;
                        }
                        cx.notify();
                    })
                })
        };
        let second_check = {
            let page = page.clone();
            let on = recording.second_on;
            spork_workbench::controls::labeled_check("second-stroke", "Second stroke:", on, cx)
                .on_click(move |_, _, cx| {
                    page.update(cx, |page, cx| {
                        if let Some(recording) = &mut page.recording {
                            recording.second_on = !on;
                            recording.on_second = !on;
                            if on {
                                recording.second = None;
                            }
                        }
                        if page.recording.as_ref().is_some_and(|r| r.entry.is_none()) {
                            page.shortcut_filter = page.recorded();
                        }
                        cx.notify();
                    })
                })
        };
        let header = recording.entry.as_ref().map(|(label, _, place)| {
            h_flex().gap(px(4.)).child(label.clone()).child(
                div()
                    .text_color(tokens.text_secondary)
                    .child(format!("in {place}")),
            )
        });
        let conflicts = match (&recording.entry, self.recorded()) {
            (Some((_, action, _)), Some(keys)) => self.conflicts(&keys, action.name(), cx),
            _ => Vec::new(),
        };
        v_flex()
            .gap(px(8.))
            .children(header)
            .child(stroke_field(
                "first-stroke",
                &recording.first,
                !recording.on_second,
                true,
                false,
            ))
            .child(second_check)
            .child(stroke_field(
                "second-stroke-field",
                &recording.second,
                recording.on_second,
                recording.second_on,
                true,
            ))
            .when(!conflicts.is_empty(), |content| {
                content.child(
                    v_flex()
                        .gap(px(4.))
                        .child(
                            h_flex()
                                .gap(px(4.))
                                .child(
                                    spork_workbench::icon("general/warningDialog")
                                        .size(px(16.))
                                        .text_color(tokens.text_warning),
                                )
                                .child("Already assigned to:"),
                        )
                        .children(
                            conflicts
                                .iter()
                                .map(|(label, _)| div().pl(px(20.)).child(label.clone())),
                        ),
                )
            })
            .into_any_element()
    }

    fn row(&self, row: &Row<'_>, ix: usize, query: &str, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        match row {
            Row::Group {
                path,
                name,
                open,
                depth,
            } => {
                let target = path.clone();
                let open = *open;
                h_flex()
                    .id(("keymap-group", ix))
                    .role(Role::TreeItem)
                    .aria_label(name.clone())
                    .aria_expanded(open)
                    .h(px(ROW_HEIGHT))
                    .pl(px(ROW_INSET + *depth as f32 * INDENT))
                    .gap(px(4.))
                    .cursor_pointer()
                    .hover(move |row| row.bg(tokens.selection_hover))
                    .rounded(px(SELECTION_RADIUS))
                    .child(
                        spork_workbench::icon(match open {
                            true => "general/chevronDown",
                            false => "general/chevronRight",
                        })
                        .size(px(16.))
                        .text_color(tokens.icon),
                    )
                    .child(spork_workbench::image("nodes/folder", cx).size(px(16.)))
                    .child(highlighted(name.clone(), query, cx))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.open.remove(&target) {
                            this.open.insert(target.clone());
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            }
            Row::Action { entry, depth } => {
                let shortcuts = self.shortcuts(entry.action.as_ref(), cx);
                let name = entry.action.name();
                let selected = self.selected == Some(name);
                let modified = self.edits.iter().any(|edit| match edit {
                    Edit::Add(shortcut) | Edit::Remove(shortcut) => shortcut.action == name,
                }) || shortcuts
                    .iter()
                    .any(|shortcut| shortcut.source == KeybindSource::User);
                let entry_data = (
                    entry.label.clone(),
                    entry.action.clone(),
                    entry.place.clone(),
                );
                let bindable = cx.build_action(name, None).is_ok();
                let page = cx.entity();
                let menu_shortcuts = shortcuts.clone();
                let menu_entry = entry_data.clone();
                h_flex()
                    .id(("keymap-action", ix))
                    .role(Role::TreeItem)
                    .aria_label(entry.label.clone())
                    .aria_selected(selected)
                    .aria_description(SharedString::from(
                        shortcuts
                            .iter()
                            .map(Shortcut::text)
                            .collect::<Vec<_>>()
                            .join(" or "),
                    ))
                    .w_full()
                    .h(px(ROW_HEIGHT))
                    .pl(px(ROW_INSET + *depth as f32 * INDENT + 20.))
                    .pr(px(19.))
                    .gap(px(6.))
                    .rounded(px(SELECTION_RADIUS))
                    .when(selected, |row| row.bg(tokens.selection))
                    .when(!selected, |row| {
                        row.hover(move |row| row.bg(tokens.selection_hover))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .when(modified, |label| label.text_color(tokens.text_link))
                            .child(highlighted(entry.label.clone(), query, cx)),
                    )
                    .child(shortcut_chips(&shortcuts, cx))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, _, cx| {
                            this.selected = Some(name);
                            cx.notify();
                        }),
                    )
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        this.selected = Some(name);
                        if event.click_count() == 2 && bindable {
                            this.record(Some(entry_data.clone()), window, cx);
                        }
                        cx.notify();
                    }))
                    .context_menu(move |menu, _, _| {
                        let add = {
                            let page = page.clone();
                            let entry = menu_entry.clone();
                            PopupMenuItem::new("Add Keyboard Shortcut")
                                .disabled(!bindable)
                                .on_click(move |_, window, cx| {
                                    let entry = entry.clone();
                                    page.update(cx, |page, cx| {
                                        page.record(Some(entry), window, cx)
                                    });
                                })
                        };
                        let menu = menu.item(add);
                        if menu_shortcuts.is_empty() {
                            return menu;
                        }
                        menu_shortcuts
                            .iter()
                            .fold(menu.separator(), |menu, shortcut| {
                                let page = page.clone();
                                let removed = shortcut.clone();
                                menu.item(
                                    PopupMenuItem::new(format!("Remove {}", shortcut.text()))
                                        .on_click(move |_, _, cx| {
                                            page.update(cx, |page, cx| {
                                                page.remove(removed.clone());
                                                cx.notify();
                                            })
                                        }),
                                )
                            })
                    })
                    .into_any_element()
            }
        }
    }

    /// Removes a shortcut: a change not applied yet is dropped, the keymap's is removed on Apply.
    /// The dialog's changes: kept for Apply in Settings, written at once when standalone.
    fn add_edits(&mut self, edits: Vec<Edit>, window: &mut Window, cx: &mut Context<Self>) {
        self.edits.extend(edits);
        if self.standalone {
            self.apply(window, cx);
        }
        cx.notify();
    }

    fn remove(&mut self, shortcut: Shortcut) {
        let before = self.edits.len();
        self.edits
            .retain(|edit| !matches!(edit, Edit::Add(added) if added.same(&shortcut)));
        if self.edits.len() == before {
            self.edits.push(Edit::Remove(shortcut));
        }
    }

    fn entry(&self, name: &str) -> Option<&Entry> {
        fn find<'a>(items: &'a [Item], name: &str) -> Option<&'a Entry> {
            items.iter().find_map(|item| match item {
                Item::Group(group) => find(&group.items, name),
                Item::Action(entry) if entry.action.name() == name => Some(entry),
                Item::Action(_) => None,
            })
        }
        find(&self.groups, name)
    }

    pub(crate) fn e2e_state(&self, cx: &App) -> Value {
        let shortcuts = |name: &str| -> Vec<String> {
            self.entry(name)
                .map(|entry| {
                    self.shortcuts(entry.action.as_ref(), cx)
                        .iter()
                        .map(Shortcut::text)
                        .collect()
                })
                .unwrap_or_default()
        };
        let rows: Vec<String> = self
            .rows(cx)
            .iter()
            .take(40)
            .map(|row| match row {
                Row::Group { name, .. } => format!("▸ {name}"),
                Row::Action { entry, .. } => entry.label.to_string(),
            })
            .collect();
        json!({
            "selected": self.selected,
            "selected_shortcuts": self.selected.map(shortcuts),
            "edits": self.edits.len(),
            "rows": rows,
            "recording": self.recording.as_ref().map(|r| json!({
                "first": r.first.as_ref().map(keystroke_text),
                "second": r.second.as_ref().map(keystroke_text),
                "conflicts": match (&r.entry, self.recorded()) {
                    (Some((_, action, _)), Some(keys)) => self
                        .conflicts(&keys, action.name(), cx)
                        .into_iter()
                        .map(|(label, _)| label.to_string())
                        .collect::<Vec<_>>(),
                    _ => Vec::new(),
                },
            })),
            "shortcut_filter": self.shortcut_filter.as_ref().map(|keys| keys.iter().map(keystroke_text).collect::<Vec<_>>()),
        })
    }
}

/// The key context a new shortcut of `action` works in: that of its bindings in its own
/// namespace's context ("Editor" for `editor::…`), else its narrowest one; none when unbound.
fn binding_context(action: &dyn Action, cx: &App) -> Option<String> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let contexts: Vec<String> = keymap
        .bindings_for_action(action)
        .filter_map(|binding| binding.predicate().map(|predicate| predicate.to_string()))
        .collect();
    let own = action
        .name()
        .split_once("::")
        .map(|(namespace, _)| humanize_words(namespace).replace(' ', ""))
        .unwrap_or_default();
    contexts
        .iter()
        .filter(|context| !own.is_empty() && context.contains(&own))
        .min_by_key(|context| context.len())
        .or_else(|| contexts.iter().min_by_key(|context| context.len()))
        .cloned()
}

/// IntelliJ's keymap tree rows: 24 px, 16 px a level, rounded selection inset 12 px.
const ROW_HEIGHT: f32 = 24.;
const INDENT: f32 = 16.;
const ROW_INSET: f32 = 12.;
const SELECTION_RADIUS: f32 = 8.;

impl Render for KeymapPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let count = self.rows(cx).len();
        let list = uniform_list(
            "keymap-actions",
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                let query = this.filter.read(cx).value().trim().to_lowercase();
                let rows = this.rows(cx);
                range
                    .filter_map(|ix| rows.get(ix).map(|row| this.row(row, ix, &query, cx)))
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        let filtering = self.shortcut_filter.is_some();
        v_flex()
            .size_full()
            .gap(px(8.))
            .child(
                h_flex()
                    .gap(px(4.))
                    .child(
                        spork_workbench::ActionButton::new("keymap-expand", "general/expandAll")
                            .tooltip("Expand All")
                            .on_click(cx.listener(|this, _, _, cx| this.expand_all(true, cx))),
                    )
                    .child(
                        spork_workbench::ActionButton::new(
                            "keymap-collapse",
                            "general/collapseAll",
                        )
                        .tooltip("Collapse All")
                        .on_click(cx.listener(|this, _, _, cx| this.expand_all(false, cx))),
                    )
                    .child(div().flex_1())
                    .child(
                        div().w(relative(0.5)).child(
                            spork_workbench::TextField::new(&self.filter)
                                .aria_label("Search actions")
                                .prefix(
                                    spork_workbench::icon("general/searchWithHistory")
                                        .w(px(20.))
                                        .h(px(16.)),
                                )
                                .cleanable(true),
                        ),
                    )
                    .child(
                        spork_workbench::ActionButton::new(
                            "keymap-find-shortcut",
                            "actions/shortcutFilter",
                        )
                        .tooltip("Find Actions by Shortcut")
                        .on_click(cx.listener(|this, _, window, cx| this.record(None, window, cx))),
                    )
                    .when(filtering, |row| {
                        row.child(
                            spork_workbench::ActionButton::new(
                                "keymap-clear-filter",
                                "actions/close",
                            )
                            .tooltip("Clear Filtering")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.shortcut_filter = None;
                                cx.notify();
                            })),
                        )
                    }),
            )
            .child(
                div()
                    .id("keymap-tree")
                    .role(Role::Tree)
                    .aria_label(SharedString::from("Actions"))
                    .flex_1()
                    .min_h_0()
                    .border_1()
                    .border_color(tokens.separator)
                    .py(px(4.))
                    .child(list),
            )
    }
}

/// The groups of IntelliJ's keymap tree: Editor Actions, Main Menu (the menu bar's menus, with
/// their texts), Tool Windows, Version Control Systems, Other (by namespace).
fn build_groups(cx: &App) -> Vec<Item> {
    let names: Vec<&'static str> = cx.all_action_names().to_vec();
    let mut used: HashSet<&'static str> = HashSet::default();
    let mut main_menu = Vec::new();
    if let Some(menus) = cx.get_menus() {
        for menu in &menus {
            if menu.name.is_empty() {
                continue;
            }
            let group = menu_group(menu, "Main Menu", &mut used);
            if !group.items.is_empty() {
                main_menu.push(Item::Group(group));
            }
        }
    }
    let build = |name: &'static str| cx.build_action(name, None).ok().map(Rc::<dyn Action>::from);
    let mut tool_windows = Vec::new();
    for (label, name) in TOOL_WINDOWS {
        if let Some(name) = names.iter().find(|n| **n == *name)
            && let Some(action) = build(name)
        {
            used.insert(name);
            tool_windows.push(Item::Action(Entry {
                label: (*label).into(),
                action,
                place: "Tool Windows".into(),
            }));
        }
    }
    let mut editor = Vec::new();
    let mut vcs = Vec::new();
    let mut other: std::collections::BTreeMap<String, Vec<Item>> = Default::default();
    for name in names {
        if used.contains(name) || name.starts_with("zed_actions") {
            continue;
        }
        let Some((namespace, _)) = name.split_once("::") else {
            continue;
        };
        let Some(action) = build(name) else { continue };
        let label = humanize(name);
        let (items, place) = match namespace {
            "editor" => (&mut editor, "Editor Actions".to_owned()),
            "vcs" | "git" | "git_panel" | "commit_panel" => {
                (&mut vcs, "Version Control Systems".to_owned())
            }
            namespace => {
                let group = humanize_words(namespace);
                let place = format!("Other | {group}");
                (other.entry(group).or_default(), place)
            }
        };
        items.push(Item::Action(Entry {
            label: label.into(),
            action,
            place: place.into(),
        }));
    }
    let sort = |items: &mut Vec<Item>| {
        items.sort_by(|a, b| match (a, b) {
            (Item::Action(a), Item::Action(b)) => a.label.cmp(&b.label),
            _ => std::cmp::Ordering::Equal,
        })
    };
    sort(&mut editor);
    sort(&mut vcs);
    let other = other
        .into_iter()
        .map(|(name, mut items)| {
            sort(&mut items);
            Item::Group(Group {
                name: name.into(),
                items,
            })
        })
        .collect();
    [
        ("Editor Actions", editor),
        ("Main Menu", main_menu),
        ("Tool Windows", tool_windows),
        ("Version Control Systems", vcs),
        ("Other", other),
    ]
    .into_iter()
    .map(|(name, items)| {
        Item::Group(Group {
            name: name.into(),
            items,
        })
    })
    .collect()
}

fn menu_group(menu: &OwnedMenu, parent: &str, used: &mut HashSet<&'static str>) -> Group {
    let place = format!("{parent} | {}", menu.name);
    let mut items = Vec::new();
    for item in &menu.items {
        match item {
            OwnedMenuItem::Action { name, action, .. } => {
                used.insert(action.name());
                items.push(Item::Action(Entry {
                    label: name
                        .trim_end_matches('…')
                        .trim_end_matches("...")
                        .to_owned()
                        .into(),
                    action: Rc::from(action.boxed_clone()),
                    place: place.clone().into(),
                }));
            }
            OwnedMenuItem::Submenu(submenu) => {
                let group = menu_group(submenu, &place, used);
                if !group.items.is_empty() {
                    items.push(Item::Group(group));
                }
            }
            _ => {}
        }
    }
    Group {
        name: menu.name.clone(),
        items,
    }
}

/// An action's name as words: `editor::MoveLineUp` is "Move Line Up".
fn humanize(name: &str) -> String {
    let (_, name) = name.split_once("::").unwrap_or(("", name));
    let mut words = String::new();
    for (ix, c) in name.char_indices() {
        let after_lower =
            ix > 0 && name[..ix].ends_with(|p: char| p.is_lowercase() || p.is_ascii_digit());
        if c.is_uppercase() && after_lower {
            words.push(' ');
        }
        words.push(c);
    }
    words
}

/// A namespace as words: `project_panel` is "Project Panel".
fn humanize_words(name: &str) -> String {
    name.split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether two shortcuts are the same keys: a key typed carries the character it types, the
/// keymap's don't.
fn same_keys(a: &[KeybindingKeystroke], b: &[KeybindingKeystroke]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(a, b)| a.modifiers() == b.modifiers() && a.key() == b.key())
}

/// A keystroke as macOS writes it: ⌃⌥⇧⌘ then the key, `⇧⌘A`.
fn keystroke_text(keystroke: &KeybindingKeystroke) -> String {
    keystroke_parts(keystroke).concat()
}

/// A keystroke's modifiers one symbol each, then its key.
fn keystroke_parts(keystroke: &KeybindingKeystroke) -> Vec<String> {
    let modifiers = keystroke.modifiers();
    let mut parts = Vec::new();
    for (on, symbol) in [
        (modifiers.control, "⌃"),
        (modifiers.alt, "⌥"),
        (modifiers.shift, "⇧"),
        (modifiers.platform, "⌘"),
        (modifiers.function, "fn"),
    ] {
        if on {
            parts.push(symbol.to_owned());
        }
    }
    let key = keystroke.key();
    let key = match key {
        "enter" => "⏎".to_owned(),
        "escape" => "⎋".to_owned(),
        "backspace" => "⌫".to_owned(),
        "delete" => "⌦".to_owned(),
        "tab" => "⇥".to_owned(),
        "space" => "Space".to_owned(),
        "up" => "↑".to_owned(),
        "down" => "↓".to_owned(),
        "left" => "←".to_owned(),
        "right" => "→".to_owned(),
        "home" => "↖".to_owned(),
        "end" => "↘".to_owned(),
        "pageup" => "⇞".to_owned(),
        "pagedown" => "⇟".to_owned(),
        key if key.chars().count() == 1 => key.to_uppercase(),
        key => {
            let mut chars = key.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        }
    };
    if !key.is_empty() {
        parts.push(key);
    }
    parts
}

/// IntelliJ's New UI shortcut chips: one per key, "or" between an action's shortcuts.
fn shortcut_chips(shortcuts: &[Shortcut], cx: &App) -> Div {
    let tokens = Tokens::get(cx);
    let chip = |text: String| {
        div()
            .h(px(18.))
            .min_w(px(20.))
            .px(px(4.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .border_1()
            .border_color(tokens.shortcut_border)
            .text_ui_small()
            .font_weight(FontWeight::BOLD)
            .text_color(tokens.shortcut_text)
            .child(text)
    };
    let mut row = h_flex().flex_none().gap(px(8.));
    for (ix, shortcut) in shortcuts.iter().enumerate() {
        if ix > 0 {
            row = row.child(
                div()
                    .text_ui_small()
                    .text_color(tokens.text_secondary)
                    .child("or"),
            );
        }
        let mut keys = h_flex().gap(px(3.));
        for (stroke_ix, keystroke) in shortcut.keystrokes.iter().enumerate() {
            if stroke_ix > 0 {
                keys = keys.child(div().w(px(4.)));
            }
            for part in keystroke_parts(keystroke) {
                keys = keys.child(chip(part));
            }
        }
        row = row.child(keys);
    }
    row
}

/// A tree row's text, what the search found in it highlighted.
fn highlighted(text: SharedString, query: &str, cx: &App) -> AnyElement {
    let lower = text.to_lowercase();
    match (query.is_empty(), lower.find(query)) {
        (false, Some(at)) if lower.len() == text.len() => StyledText::new(text)
            .with_highlights([(
                at..at + query.len(),
                HighlightStyle {
                    background_color: Some(Tokens::get(cx).search_match),
                    ..Default::default()
                },
            )])
            .into_any_element(),
        _ => text.into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::{humanize, humanize_words};

    #[test]
    fn action_names_read_as_words() {
        assert_eq!(humanize("editor::MoveLineUp"), "Move Line Up");
        assert_eq!(humanize("editor::GoToHunk"), "Go To Hunk");
        assert_eq!(humanize_words("project_panel"), "Project Panel");
    }
}
