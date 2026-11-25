//! IntelliJ's file operations in the Project tool window (ROADMAP, phase 6): New… (⌘N) with its
//! New File and New Directory name popups, Rename… (⇧F6), Move… (F6), Copy… (F5, and Paste after
//! Copy), Delete (⌫) behind IntelliJ's confirmation, and the Copy Path/Reference… popup. They take
//! the place of the inline editor and the system prompts in windows that host gpui-kit's dialogs
//! (the IDE's do; this crate's tests keep Zed's flows).

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, FontWeight,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, PathPromptOptions,
    Render, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    TaskExt as _, WeakEntity, Window, actions, div, px,
};
use gpui_kit::component::input::{Escape, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use project::{ProjectEntryId, ProjectPath, WorktreeId};
use spork_workbench::controls::{dialog_title, form_label};
use spork_workbench::{Tokens, Typography as _, popup};
use util::paths::PathExt as _;
use util::rel_path::RelPath;
use workspace::{OpenOptions, OpenVisible, SelectedEntry, Workspace};

use crate::ProjectPanel;

actions!(
    project_panel,
    [
        /// Shows IntelliJ's New popup for the selected folder: File, Scratch File, Directory.
        NewElement,
        /// Creates a scratch file of a chosen language in the IDE's config folder.
        NewScratchFile,
        /// Copies the selected file or folder into a folder, IntelliJ's Copy….
        CopyElement,
        /// Moves the selected file or folder into a folder, IntelliJ's Move….
        MoveElement,
        /// Shows IntelliJ's Copy Path/Reference… popup for the selected file or folder.
        CopyPathReference,
        /// Shows the New popup for the open file's folder (IntelliJ's New in This Directory…).
        NewElementSamePlace,
        /// Opens a plain text scratch buffer, `buffer1.txt` to `buffer5.txt` in turn (IntelliJ's
        /// New Scratch Buffer).
        NewScratchBuffer,
    ]
);

/// IntelliJ's HTML 5 file template (`HTML File.html`), its title variable at its default.
const HTML_TEMPLATE: &str = "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n    <meta charset=\"UTF-8\">\n    <title>Title</title>\n</head>\n<body>\n\n</body>\n</html>\n";

/// Whether `window` hosts gpui-kit's dialogs: the IDE's windows do (`spork_vcs`), a bare
/// workspace in this crate's tests doesn't.
pub(crate) fn hosts_dialogs(window: &Window, cx: &App) -> bool {
    gpui_kit::base::Root::find(window, cx).is_some()
}

/// A row of a chooser popup: an icon, its text, a hint on the right, what choosing it does.
pub(crate) struct Choice {
    image: Option<&'static str>,
    label: SharedString,
    hint: Option<SharedString>,
    run: Rc<dyn Fn(&mut Window, &mut App)>,
}

impl Choice {
    pub(crate) fn new(
        label: impl Into<SharedString>,
        run: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            image: None,
            label: label.into(),
            hint: None,
            run: Rc::new(run),
        }
    }

    /// A workbench icon in its own colors (`fileTypes/text`).
    pub(crate) fn with_image(mut self, image: &'static str) -> Self {
        self.image = Some(image);
        self
    }

    pub(crate) fn with_hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// IntelliJ's list popup, centered in the window: its rows, the arrows and Enter to choose, and
/// with `search` a field that filters them as IntelliJ's speed search does.
struct Chooser {
    choices: Vec<Choice>,
    search: Option<Entity<InputState>>,
    selected: usize,
    focus: FocusHandle,
    _subscription: Option<Subscription>,
}

impl Chooser {
    fn new(
        choices: Vec<Choice>,
        searchable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = searchable.then(|| cx.new(|cx| InputState::new(window, cx)));
        let _subscription = search.as_ref().map(|search| {
            cx.subscribe_in(search, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.selected = 0;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.choose(this.selected, window, cx),
                _ => {}
            })
        });
        Self {
            choices,
            search,
            selected: 0,
            focus: cx.focus_handle(),
            _subscription,
        }
    }

    /// The choices the search lets through, by their index.
    fn shown(&self, cx: &App) -> Vec<usize> {
        let query = self
            .search
            .as_ref()
            .map(|search| search.read(cx).value().trim().to_lowercase())
            .unwrap_or_default();
        (0..self.choices.len())
            .filter(|&ix| {
                query.is_empty() || self.choices[ix].label.to_lowercase().contains(&query)
            })
            .collect()
    }

    fn select_next(&mut self, step: isize, cx: &mut Context<Self>) {
        let count = self.shown(cx).len() as isize;
        if count > 0 {
            self.selected = (self.selected as isize + step).rem_euclid(count) as usize;
            cx.notify();
        }
    }

    fn choose(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(run) = self
            .shown(cx)
            .get(row)
            .and_then(|&ix| self.choices.get(ix))
            .map(|choice| choice.run.clone())
        else {
            return;
        };
        window.close_dialog(cx);
        run(window, cx);
    }

    fn focus_input(&self, window: &mut Window, cx: &mut Context<Self>) {
        match &self.search {
            Some(search) => search.update(cx, |search, cx| search.focus(window, cx)),
            None => self.focus.focus(window, cx),
        }
    }
}

impl Render for Chooser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let shown = self.shown(cx);
        let rows = shown.iter().enumerate().map(|(row, &ix)| {
            let choice = &self.choices[ix];
            let selected = row == self.selected;
            popup::row(("choice", ix), selected, cx)
                .role(Role::MenuItem)
                .aria_label(choice.label.clone())
                .children(choice.image.map(|image| spork_workbench::image(image, cx)))
                .child(div().flex_none().child(choice.label.clone()))
                // a long hint (a path) is cut at its end, as IntelliJ's
                .children(choice.hint.clone().map(|hint| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .pl(px(10.))
                        .flex()
                        .justify_end()
                        .text_color(match selected {
                            true => tokens.text,
                            false => tokens.text_secondary,
                        })
                        .child(div().truncate().child(hint))
                }))
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if this.selected != row {
                        this.selected = row;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| this.choose(row, window, cx)))
        });
        v_flex()
            .id("chooser")
            .role(Role::Menu)
            .track_focus(&self.focus)
            // "menu": Zed's keys for lists (↑ ↓ ↩) reach it before the workspace's
            .key_context("Chooser menu")
            .on_action(
                cx.listener(|this, _: &menu::SelectPrevious, _, cx| this.select_next(-1, cx)),
            )
            .on_action(cx.listener(|this, _: &menu::SelectNext, _, cx| this.select_next(1, cx)))
            .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| {
                this.choose(this.selected, window, cx)
            }))
            .on_action(cx.listener(|_, _: &menu::Cancel, window, cx| window.close_dialog(cx)))
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                cx.stop_propagation();
                this.select_next(-1, cx)
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                cx.stop_propagation();
                this.select_next(1, cx)
            }))
            .capture_action(cx.listener(|_, _: &Escape, window, cx| {
                cx.stop_propagation();
                window.close_dialog(cx)
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "up" => this.select_next(-1, cx),
                    "down" => this.select_next(1, cx),
                    "enter" => this.choose(this.selected, window, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .children(self.search.as_ref().map(|search| {
                div().pb(px(4.)).child(
                    spork_workbench::TextField::new(search)
                        .in_popup()
                        .aria_label("Search")
                        .prefix(spork_workbench::icon("general/search").size(px(16.))),
                )
            }))
            .child(
                v_flex()
                    .id("choices")
                    .max_h(px(420.))
                    .overflow_y_scroll()
                    .children(rows),
            )
    }
}

/// Shows `choices` in IntelliJ's list popup titled `title`, centered in the window.
pub(crate) fn open_chooser(
    title: &'static str,
    choices: Vec<Choice>,
    searchable: bool,
    width: f32,
    window: &mut Window,
    cx: &mut App,
) {
    let chooser = cx.new(|cx| Chooser::new(choices, searchable, window, cx));
    let content = chooser.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(dialog_title(title))
            .on_ok(|_, _, _| false)
            .w(px(width))
            .centered()
            .overlay(false)
            .close_button(false)
            .child(content.clone())
    });
    chooser.update(cx, |chooser, cx| chooser.focus_input(window, cx));
}

/// IntelliJ's name popup (`NewItemSimplePopupPanel`): a field under its title; Enter checks the
/// name and either says what is wrong under the field or goes ahead.
struct NamePopup {
    input: Entity<InputState>,
    error: Option<String>,
    check: Rc<dyn Fn(&str, &App) -> Option<String>>,
    done: Rc<dyn Fn(String, &mut Window, &mut App)>,
    _subscription: Subscription,
}

impl NamePopup {
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.input.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return;
        }
        match (self.check)(&name, cx) {
            Some(error) => {
                self.error = Some(error);
                cx.notify();
            }
            None => {
                window.close_dialog(cx);
                (self.done)(name, window, cx);
            }
        }
    }
}

impl Render for NamePopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        v_flex()
            .gap(px(6.))
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
    }
}

fn open_name_popup(
    title: &'static str,
    check: impl Fn(&str, &App) -> Option<String> + 'static,
    done: impl Fn(String, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let input = cx.new(|cx| InputState::new(window, cx));
    let popup = cx.new(|cx| {
        let _subscription = cx.subscribe_in(
            &input,
            window,
            |this: &mut NamePopup, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.confirm(window, cx),
                InputEvent::Change if this.error.is_some() => {
                    this.error = None;
                    cx.notify();
                }
                _ => {}
            },
        );
        NamePopup {
            input: input.clone(),
            error: None,
            check: Rc::new(check),
            done: Rc::new(done),
            _subscription,
        }
    });
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(dialog_title(title))
            .on_ok(|_, _, _| false)
            .w(px(400.))
            .centered()
            .overlay(false)
            .close_button(false)
            .child(popup.clone())
    });
    input.update(cx, |input, cx| input.focus(window, cx));
}

/// The folder a new entry goes in: the selected folder, the selected file's folder, or the last
/// root; with its entry and its path in the worktree.
fn target_folder(
    panel: &ProjectPanel,
    cx: &App,
) -> Option<(WorktreeId, ProjectEntryId, Arc<RelPath>)> {
    let project = panel.project.read(cx);
    let (worktree, entry_id) = match panel.selection {
        Some(selection) => (
            project.worktree_for_id(selection.worktree_id, cx)?,
            panel.resolve_entry(selection.entry_id),
        ),
        None => {
            let root = panel.state.last_worktree_root_id?;
            (project.worktree_for_entry(root, cx)?, root)
        }
    };
    let worktree = worktree.read(cx);
    let mut entry = worktree.entry_for_id(entry_id)?;
    if !entry.is_dir() {
        entry = worktree.entry_for_path(entry.path.parent()?)?;
    }
    Some((worktree.id(), entry.id, entry.path.clone()))
}

/// IntelliJ's check of a new file's or folder's name (`checkFileNameInput`): `a/b/c` makes the
/// folders on the way; an existing file on the way, or the whole path already there, is an
/// error. `kind_at` tells whether a path of the worktree is a folder, a file, or nothing.
fn check_name(
    input: &str,
    folder: &RelPath,
    kind_at: impl Fn(&RelPath) -> Option<bool>,
) -> Option<String> {
    let tokens: Vec<&str> = input.split(['/', '\\']).filter(|t| !t.is_empty()).collect();
    let mut path = folder.to_rel_path_buf();
    for (ix, token) in tokens.iter().enumerate() {
        let last = ix + 1 == tokens.len();
        if (*token == "." || *token == "..") && last {
            return Some(format!("'{token}' is an invalid file name"));
        }
        match *token {
            "." => continue,
            ".." => {
                if !path.pop() {
                    return Some(format!("Invalid directory: '{}/..'", folder.as_unix_str()));
                }
                continue;
            }
            _ => {}
        }
        if path.push_component(token).is_err() {
            return Some(format!("'{token}' is an invalid file name"));
        }
        match kind_at(path.as_rel_path()) {
            Some(false) => return Some(format!("A file with the name '{token}' already exists")),
            Some(true) if last => {
                return Some(format!(
                    "A directory with the name '{token}' already exists"
                ));
            }
            _ => {}
        }
    }
    None
}

/// [`check_name`] against the panel's worktree.
fn check_new_name(
    panel: &WeakEntity<ProjectPanel>,
    worktree_id: WorktreeId,
    folder: &RelPath,
    input: &str,
    cx: &App,
) -> Option<String> {
    let panel = panel.upgrade()?;
    let worktree = panel
        .read(cx)
        .project
        .read(cx)
        .worktree_for_id(worktree_id, cx)?;
    let worktree = worktree.read(cx);
    check_name(input, folder, |path| {
        worktree.entry_for_path(path).map(|entry| entry.is_dir())
    })
}

/// New…: IntelliJ's New popup for the target folder.
pub(crate) fn new_element(window: &mut Window, cx: &mut Context<ProjectPanel>) {
    let weak = cx.entity().downgrade();
    let entry = |is_dir: bool| {
        let weak = weak.clone();
        move |window: &mut Window, cx: &mut App| {
            weak.update(cx, |panel, cx| new_entry(panel, is_dir, window, cx))
                .ok();
        }
    };
    let choices = vec![
        Choice::new("File", entry(false)).with_image("fileTypes/text"),
        Choice::new("Scratch File", |window, cx| {
            window.dispatch_action(Box::new(NewScratchFile), cx)
        })
        .with_image("fileTypes/scratch")
        .with_hint("⇧⌘N"),
        Choice::new("Directory", entry(true)).with_image("nodes/folder"),
        Choice::new("HTML File", {
            let weak = weak.clone();
            move |window, cx| {
                weak.update(cx, |panel, cx| new_html_file(panel, window, cx))
                    .ok();
            }
        })
        .with_image("fileTypes/html"),
    ];
    open_chooser("New", choices, false, 320., window, cx);
}

/// HTML File: IntelliJ's name popup, then `<name>.html` from its HTML 5 template, opened.
fn new_html_file(panel: &mut ProjectPanel, window: &mut Window, cx: &mut Context<ProjectPanel>) {
    let Some((worktree_id, _, folder)) = target_folder(panel, cx) else {
        return;
    };
    let with_extension = |name: &str| match Path::new(name).extension() {
        Some(_) => name.to_owned(),
        None => format!("{name}.html"),
    };
    let weak = cx.entity().downgrade();
    let check = {
        let (weak, folder) = (weak.clone(), folder.clone());
        move |input: &str, cx: &App| {
            check_new_name(&weak, worktree_id, &folder, &with_extension(input), cx)
        }
    };
    let done = move |name: String, window: &mut Window, cx: &mut App| {
        let Some((fs, workspace, root)) = weak
            .update(cx, |panel, cx| {
                let worktree = panel.project.read(cx).worktree_for_id(worktree_id, cx)?;
                Some((
                    panel.fs.clone(),
                    panel.workspace.clone(),
                    worktree.read(cx).absolutize(&folder),
                ))
            })
            .ok()
            .flatten()
        else {
            return;
        };
        let path = root.join(with_extension(&name));
        window
            .spawn(cx, async move |cx| {
                if let Some(parent) = path.parent() {
                    fs.create_dir(parent).await?;
                }
                fs.create_file(&path, Default::default()).await?;
                fs.save(&path, &HTML_TEMPLATE.into(), Default::default())
                    .await?;
                workspace
                    .update_in(cx, |workspace, window, cx| {
                        workspace.open_abs_path(
                            path,
                            OpenOptions {
                                visible: Some(OpenVisible::All),
                                ..OpenOptions::default()
                            },
                            window,
                            cx,
                        )
                    })?
                    .await?;
                anyhow::Ok(())
            })
            .detach_and_log_err(cx);
    };
    open_name_popup("New HTML File", check, done, window, cx);
}

/// New in This Directory… (⌃⌥N): the New popup for the open file's folder.
pub(crate) fn new_in_this_directory(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if !hosts_dialogs(window, cx) {
        return;
    }
    let Some(panel) = workspace.panel::<ProjectPanel>(cx) else {
        return;
    };
    let Some(open_file) = workspace
        .active_item(cx)
        .and_then(|item| item.project_path(cx))
    else {
        return;
    };
    panel.update(cx, |panel, cx| {
        let Some(entry) = panel.project.read(cx).entry_for_path(&open_file, cx) else {
            return;
        };
        panel.selection = Some(SelectedEntry {
            worktree_id: open_file.worktree_id,
            entry_id: entry.id,
        });
        new_element(window, cx)
    });
}

/// New File and New Directory: IntelliJ's name popup, then the entry made where the inline
/// editor would have made it, selected, and a file opened.
pub(crate) fn new_entry(
    panel: &mut ProjectPanel,
    is_dir: bool,
    window: &mut Window,
    cx: &mut Context<ProjectPanel>,
) {
    let Some((worktree_id, folder_id, folder)) = target_folder(panel, cx) else {
        return;
    };
    let weak = cx.entity().downgrade();
    let check = {
        let weak = weak.clone();
        move |input: &str, cx: &App| check_new_name(&weak, worktree_id, &folder, input, cx)
    };
    let done = move |name: String, window: &mut Window, cx: &mut App| {
        weak.update(cx, |panel, cx| {
            panel.selection = Some(SelectedEntry {
                worktree_id,
                entry_id: folder_id,
            });
            panel.add_entry(is_dir, window, cx);
            panel
                .filename_editor
                .update(cx, |editor, cx| editor.set_text(name, window, cx));
            if let Some(task) = panel.confirm_edit(true, window, cx) {
                task.detach_and_log_err(cx);
            }
        })
        .ok();
    };
    let title = if is_dir { "New Directory" } else { "New File" };
    open_name_popup(title, check, done, window, cx);
}

/// File ▸ New ▸ File or Directory with the focus outside the Project tool window: IntelliJ's
/// name popup for the open file's folder, else the project's.
pub(crate) fn new_from_workspace(
    workspace: &mut Workspace,
    is_dir: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if !hosts_dialogs(window, cx) {
        return;
    }
    let Some(panel) = workspace.panel::<ProjectPanel>(cx) else {
        return;
    };
    let open_file = workspace
        .active_item(cx)
        .and_then(|item| item.project_path(cx));
    panel.update(cx, |panel, cx| {
        let entry = open_file.as_ref().and_then(|path| {
            let entry = panel.project.read(cx).entry_for_path(path, cx)?;
            Some(SelectedEntry {
                worktree_id: path.worktree_id,
                entry_id: entry.id,
            })
        });
        if entry.is_some() {
            panel.selection = entry;
        }
        new_entry(panel, is_dir, window, cx)
    });
}

/// The selected entry (a folded folder's own entry), with its worktree's id and absolute path.
fn selected(panel: &ProjectPanel, cx: &App) -> Option<(SelectedEntry, project::Entry, PathBuf)> {
    let (worktree, entry) = panel.selected_sub_entry(cx)?;
    let worktree = worktree.read(cx);
    let selection = SelectedEntry {
        worktree_id: worktree.id(),
        entry_id: entry.id,
    };
    Some((selection, entry.clone(), worktree.absolutize(&entry.path)))
}

/// `file 'x'` or `directory 'x'`, as IntelliJ's dialogs name what they act on.
fn kind(entry: &project::Entry) -> &'static str {
    if entry.is_dir() { "directory" } else { "file" }
}

/// IntelliJ's dialog buttons, right-aligned: Cancel then the default one.
fn buttons(
    ok: &'static str,
    ready: bool,
    on_ok: impl Fn(&mut Window, &mut App) + 'static,
) -> gpui::Div {
    h_flex()
        .pt(px(8.))
        .gap(px(8.))
        .justify_end()
        .child(
            spork_workbench::Button::new("dialog-cancel", "Cancel")
                .on_click(|_, window, cx| window.close_dialog(cx)),
        )
        .child(
            spork_workbench::Button::new("dialog-ok", ok)
                .primary(true)
                .disabled(!ready)
                .on_click(move |_, window, cx| on_ok(window, cx)),
        )
}

/// What IntelliJ says when the name is taken.
fn taken(path: &Path, name: &str) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(match metadata.is_dir() {
        true => format!("A directory with the name '{name}' already exists"),
        false => format!("A file with the name '{name}' already exists"),
    })
}

/// Rename…: IntelliJ's Rename dialog for a file or folder; Refactor renames it as the inline
/// editor would.
pub(crate) fn rename(
    panel: &mut ProjectPanel,
    window: &mut Window,
    cx: &mut Context<ProjectPanel>,
) {
    let Some((selection, entry, abs_path)) = selected(panel, cx) else {
        return;
    };
    let old_name = entry
        .path
        .file_name()
        .map(str::to_owned)
        .or_else(|| {
            abs_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    let input = cx.new(|cx| InputState::new(window, cx).default_value(old_name.clone()));
    let error = cx.new(|_| None::<String>);
    let weak = cx.entity().downgrade();
    let label = format!("Rename {} '{old_name}' to:", kind(&entry));
    let refactor = {
        let (input, error, old_name) = (input.clone(), error.clone(), old_name.clone());
        let parent = abs_path.parent().map(Path::to_path_buf);
        Rc::new(move |window: &mut Window, cx: &mut App| {
            let name = input.read(cx).value().trim().to_owned();
            if name.is_empty() || name == old_name {
                return;
            }
            let problem = if name.contains(['/', '\\']) {
                Some("Name is not a valid file name".to_owned())
            } else {
                parent
                    .as_ref()
                    .and_then(|parent| taken(&parent.join(&name), &name))
            };
            if let Some(problem) = problem {
                error.update(cx, |error, cx| {
                    *error = Some(problem);
                    cx.notify();
                });
                window.refresh();
                return;
            }
            window.close_dialog(cx);
            weak.update(cx, |panel, cx| {
                panel.selection = Some(selection);
                panel.rename_impl(None, window, cx);
                panel
                    .filename_editor
                    .update(cx, |editor, cx| editor.set_text(name, window, cx));
                if let Some(task) = panel.confirm_edit(true, window, cx) {
                    task.detach_and_log_err(cx);
                }
            })
            .ok();
        })
    };
    window
        .subscribe(&input, cx, {
            let (refactor, error) = (refactor.clone(), error.clone());
            move |_, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => refactor(window, cx),
                InputEvent::Change => {
                    error.update(cx, |error, _| *error = None);
                    window.refresh();
                }
                _ => {}
            }
        })
        .detach();
    let field = input.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let tokens = Tokens::get(cx);
        let ready = !field.read(cx).value().trim().is_empty();
        let refactor = refactor.clone();
        dialog
            .title(dialog_title("Rename"))
            .on_ok(|_, _, _| false)
            .w(px(480.))
            .close_button(false)
            .child(
                v_flex()
                    .gap(px(10.))
                    .child(form_label(label.clone()))
                    .child(spork_workbench::TextField::new(&field).aria_label(label.clone()))
                    .children(error.read(cx).clone().map(|error| {
                        div()
                            .text_ui_small()
                            .text_color(tokens.text_error)
                            .child(error)
                    }))
                    .child(buttons("Refactor", ready, move |window, cx| {
                        refactor(window, cx)
                    })),
            )
    });
    input.update(cx, |input, cx| input.focus(window, cx));
}

/// Delete: IntelliJ's confirmation (`DeleteUtil`), then the entries go as the panel's Delete or
/// Trash would.
pub(crate) fn delete(
    panel: &mut ProjectPanel,
    trash: bool,
    window: &mut Window,
    cx: &mut Context<ProjectPanel>,
) {
    let entries = panel.disjoint_effective_entries_excluding_roots(cx);
    let project = panel.project.read(cx);
    let found: Vec<project::Entry> = entries
        .iter()
        .filter_map(|selection| {
            let worktree = project.worktree_for_id(selection.worktree_id, cx)?;
            worktree.read(cx).entry_for_id(selection.entry_id).cloned()
        })
        .collect();
    let named: Vec<(String, bool)> = found
        .iter()
        .map(|entry| {
            (
                entry.path.file_name().unwrap_or_default().to_owned(),
                entry.is_dir(),
            )
        })
        .collect();
    let Some(message) = delete_message(&named) else {
        return;
    };
    let weak = cx.entity().downgrade();
    let on_delete = Rc::new(move |window: &mut Window, cx: &mut App| {
        window.close_dialog(cx);
        weak.update(cx, |panel, cx| panel.remove(trash, true, window, cx))
            .ok();
    });
    window.open_dialog(cx, move |dialog, _, cx| {
        let (on_delete, on_enter) = (on_delete.clone(), on_delete.clone());
        dialog
            .title(dialog_title("Delete"))
            .w(px(420.))
            .close_button(false)
            .on_ok(move |_, window, cx| {
                on_enter(window, cx);
                false
            })
            .child(
                v_flex()
                    .gap(px(12.))
                    .child(
                        h_flex()
                            .items_start()
                            .gap(px(12.))
                            .child(
                                spork_workbench::image("general/questionDialog", cx).size(px(32.)),
                            )
                            .child(
                                div()
                                    .id("dialog-message")
                                    .role(Role::Label)
                                    .aria_label(message.clone())
                                    .child(message.clone()),
                            ),
                    )
                    .child(buttons("Delete", true, move |window, cx| {
                        on_delete(window, cx)
                    })),
            )
    });
}

/// IntelliJ's question before a delete (`DeleteUtil.generateDeleteWarningMessage`), for entries
/// given by name and whether each is a folder.
fn delete_message(entries: &[(String, bool)]) -> Option<String> {
    const FOLDER_WARNING: &str = "You might not be able to fully undo this operation!";
    let message = match entries {
        [] => return None,
        [(name, is_dir)] => {
            let kind = if *is_dir { "directory" } else { "file" };
            let mut message = format!("Delete {kind} \"{name}\"?");
            if *is_dir {
                message.push_str(&format!(
                    "\nAll files and subdirectories in \"{name}\" will be deleted.\n{FOLDER_WARNING}"
                ));
            }
            message
        }
        entries => {
            let dirs = entries.iter().filter(|(_, is_dir)| *is_dir).count();
            let files = entries.len() - dirs;
            let count = |n: usize, one: &str, many: &str| match n {
                1 => format!("1 {one}"),
                n => format!("{n} {many}"),
            };
            let what = match (files, dirs) {
                (0, dirs) => count(dirs, "directory", "directories"),
                (files, 0) => count(files, "file", "files"),
                (files, dirs) => format!(
                    "{} and {}",
                    count(files, "file", "files"),
                    count(dirs, "directory", "directories")
                ),
            };
            let mut message = format!("Delete {what}?");
            if dirs > 0 {
                message.push_str(&format!(
                    "\nAll files and subdirectories in the selected directory(s) will be deleted.\n{FOLDER_WARNING}"
                ));
            }
            message
        }
    };
    Some(message)
}

/// A folder field's button: picks a folder for `field`.
fn browse(field: Entity<InputState>) -> spork_workbench::ActionButton {
    spork_workbench::ActionButton::new("browse-folder", "nodes/folder")
        .tooltip("Browse…")
        .on_click(move |_, window, cx| {
            let picked = cx.prompt_for_paths(PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: None,
            });
            let field = field.clone();
            window
                .spawn(cx, async move |cx| {
                    if let Ok(Ok(Some(mut paths))) = picked.await
                        && let Some(folder) = paths.pop()
                    {
                        cx.update(|window, cx| {
                            field.update(cx, |field, cx| {
                                field.set_value(folder.display().to_string(), window, cx)
                            })
                        })
                        .ok();
                    }
                })
                .detach();
        })
}

/// A field's text as a path, `~` for the home folder.
fn path_of(text: &str) -> PathBuf {
    match text.trim().strip_prefix("~/") {
        Some(rest) => util::paths::home_dir().join(rest),
        None => PathBuf::from(text.trim()),
    }
}

/// Copy… (F5, and Paste after Copy): IntelliJ's Copy dialog for the selected entry, into its own
/// folder or `target`.
pub(crate) fn copy_to(
    panel: &mut ProjectPanel,
    source: Option<SelectedEntry>,
    target: Option<PathBuf>,
    window: &mut Window,
    cx: &mut Context<ProjectPanel>,
) {
    let found = match source {
        Some(source) => panel
            .project
            .read(cx)
            .worktree_for_id(source.worktree_id, cx)
            .and_then(|worktree| {
                let worktree = worktree.read(cx);
                let entry = worktree.entry_for_id(source.entry_id)?.clone();
                let abs_path = worktree.absolutize(&entry.path);
                Some((source, entry, abs_path))
            }),
        None => selected(panel, cx),
    };
    let Some((_, entry, abs_path)) = found else {
        return;
    };
    let Some(parent) = abs_path.parent().map(Path::to_path_buf) else {
        return;
    };
    let name = abs_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let new_name = cx.new(|cx| InputState::new(window, cx).default_value(name));
    let folder = cx.new(|cx| {
        InputState::new(window, cx).default_value(target.unwrap_or(parent).display().to_string())
    });
    let open_copy = cx.new(|_| !entry.is_dir());
    let error = cx.new(|_| None::<String>);
    let (fs, workspace) = (panel.fs.clone(), panel.workspace.clone());
    let heading = format!("Copy {} {}", kind(&entry), abs_path.compact().display());
    let is_dir = entry.is_dir();
    let ok = {
        let (new_name, folder, open_copy, error) = (
            new_name.clone(),
            folder.clone(),
            open_copy.clone(),
            error.clone(),
        );
        Rc::new(move |window: &mut Window, cx: &mut App| {
            let name = new_name.read(cx).value().trim().to_owned();
            let folder = path_of(&folder.read(cx).value());
            let problem = if name.is_empty() {
                Some("No new name specified".to_owned())
            } else if name.contains(['/', '\\']) {
                Some("Name is not a valid file name".to_owned())
            } else if folder.as_os_str().is_empty() {
                Some("No target directory specified".to_owned())
            } else {
                taken(&folder.join(&name), &name)
            };
            if let Some(problem) = problem {
                error.update(cx, |error, _| *error = Some(problem));
                window.refresh();
                return;
            }
            window.close_dialog(cx);
            let (fs, source, dest) = (fs.clone(), abs_path.clone(), folder.join(&name));
            let open = *open_copy.read(cx) && !is_dir;
            let workspace = workspace.clone();
            window
                .spawn(cx, async move |cx| {
                    fs.create_dir(&folder).await?;
                    match is_dir {
                        true => {
                            fs.create_dir(&dest).await?;
                            fs::copy_recursive(fs.as_ref(), &source, &dest, Default::default())
                                .await?
                        }
                        false => fs.copy_file(&source, &dest, Default::default()).await?,
                    }
                    if open {
                        workspace
                            .update_in(cx, |workspace, window, cx| {
                                workspace.open_abs_path(
                                    dest,
                                    OpenOptions {
                                        visible: Some(OpenVisible::All),
                                        ..OpenOptions::default()
                                    },
                                    window,
                                    cx,
                                )
                            })?
                            .await?;
                    }
                    anyhow::Ok(())
                })
                .detach_and_log_err(cx);
        })
    };
    for field in [&new_name, &folder] {
        on_enter(field, ok.clone(), window, cx);
    }
    let focused = new_name.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let tokens = Tokens::get(cx);
        let open_on = *open_copy.read(cx);
        let toggle = open_copy.clone();
        let ok = ok.clone();
        let label = |text: &'static str| form_label(text).w(px(110.));
        dialog
            .title(dialog_title("Copy"))
            .on_ok(|_, _, _| false)
            .w(px(560.))
            .close_button(false)
            .child(
                v_flex()
                    .gap(px(10.))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(heading.clone()),
                    )
                    .child(
                        h_flex().child(label("New name:")).child(
                            spork_workbench::TextField::new(&new_name)
                                .aria_label("New name:")
                                .flex_1(),
                        ),
                    )
                    .child(
                        h_flex().child(label("To directory:")).child(
                            spork_workbench::TextField::new(&folder)
                                .aria_label("To directory:")
                                .suffix(browse(folder.clone()))
                                .flex_1(),
                        ),
                    )
                    .children(error.read(cx).clone().map(|error| {
                        div()
                            .text_ui_small()
                            .text_color(tokens.text_error)
                            .child(error)
                    }))
                    .when(!is_dir, |form| {
                        form.child(
                            spork_workbench::controls::labeled_check(
                                "copy-open",
                                "Open copy in editor",
                                open_on,
                                cx,
                            )
                            .on_click(move |_, window, cx| {
                                toggle.update(cx, |on, _| *on = !*on);
                                window.refresh();
                            }),
                        )
                    })
                    .child(buttons("OK", true, move |window, cx| ok(window, cx))),
            )
    });
    focused.update(cx, |input, cx| input.focus(window, cx));
}

/// Move… (F6): IntelliJ's Move dialog; the entry moves within its worktree as a drag would,
/// elsewhere on the disk.
pub(crate) fn move_to(
    panel: &mut ProjectPanel,
    window: &mut Window,
    cx: &mut Context<ProjectPanel>,
) {
    let Some((selection, entry, abs_path)) = selected(panel, cx) else {
        return;
    };
    let Some(parent) = abs_path.parent().map(Path::to_path_buf) else {
        return;
    };
    let name = abs_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let folder =
        cx.new(|cx| InputState::new(window, cx).default_value(parent.display().to_string()));
    let open_moved = cx.new(|_| false);
    let error = cx.new(|_| None::<String>);
    let weak = cx.entity().downgrade();
    let heading = format!("Move {} {}", kind(&entry), abs_path.compact().display());
    let is_dir = entry.is_dir();
    let ok = {
        let (folder, open_moved, error) = (folder.clone(), open_moved.clone(), error.clone());
        Rc::new(move |window: &mut Window, cx: &mut App| {
            let folder = path_of(&folder.read(cx).value());
            let dest = folder.join(&name);
            let problem = if folder.as_os_str().is_empty() {
                Some("No target directory specified".to_owned())
            } else if folder.starts_with(&abs_path) {
                Some(format!("Cannot move '{name}' into itself"))
            } else if dest == abs_path {
                None
            } else {
                taken(&dest, &name)
            };
            if let Some(problem) = problem {
                error.update(cx, |error, _| *error = Some(problem));
                window.refresh();
                return;
            }
            window.close_dialog(cx);
            if dest == abs_path {
                return;
            }
            let open = *open_moved.read(cx) && !is_dir;
            weak.update(cx, |panel, cx| {
                move_entry(panel, selection, abs_path.clone(), dest, open, window, cx)
            })
            .ok();
        })
    };
    on_enter(&folder, ok.clone(), window, cx);
    let focused = folder.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let tokens = Tokens::get(cx);
        let open_on = *open_moved.read(cx);
        let toggle = open_moved.clone();
        let ok = ok.clone();
        dialog
            .title(dialog_title("Move"))
            .on_ok(|_, _, _| false)
            .w(px(560.))
            .close_button(false)
            .child(
                v_flex()
                    .gap(px(10.))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(heading.clone()),
                    )
                    .child(
                        h_flex()
                            .child(form_label("To directory:").w(px(110.)))
                            .child(
                                spork_workbench::TextField::new(&folder)
                                    .aria_label("To directory:")
                                    .suffix(browse(folder.clone()))
                                    .flex_1(),
                            ),
                    )
                    .children(error.read(cx).clone().map(|error| {
                        div()
                            .text_ui_small()
                            .text_color(tokens.text_error)
                            .child(error)
                    }))
                    .when(!is_dir, |form| {
                        form.child(
                            spork_workbench::controls::labeled_check(
                                "move-open",
                                "Open in editor",
                                open_on,
                                cx,
                            )
                            .on_click(move |_, window, cx| {
                                toggle.update(cx, |on, _| *on = !*on);
                                window.refresh();
                            }),
                        )
                    })
                    .child(buttons("Refactor", true, move |window, cx| ok(window, cx))),
            )
    });
    focused.update(cx, |input, cx| input.focus(window, cx));
}

/// Enter in `field` does what the dialog's default button does.
fn on_enter(
    field: &Entity<InputState>,
    ok: Rc<dyn Fn(&mut Window, &mut App)>,
    window: &mut Window,
    cx: &mut App,
) {
    window
        .subscribe(field, cx, move |_, event: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                ok(window, cx)
            }
        })
        .detach();
}

/// Moves `source` to `dest`: a rename of the entry when both are in its worktree (the open
/// editors follow), the file system's otherwise.
fn move_entry(
    panel: &mut ProjectPanel,
    selection: SelectedEntry,
    source: PathBuf,
    dest: PathBuf,
    open: bool,
    window: &mut Window,
    cx: &mut Context<ProjectPanel>,
) {
    let project = panel.project.clone();
    let in_worktree = project
        .read(cx)
        .find_project_path(&dest, cx)
        .filter(|path| path.worktree_id == selection.worktree_id);
    let fs = panel.fs.clone();
    let workspace = panel.workspace.clone();
    cx.spawn_in(window, async move |_, cx| {
        match in_worktree {
            Some(ProjectPath { worktree_id, path }) => {
                project
                    .update(cx, |project, cx| {
                        project.rename_entry(selection.entry_id, (worktree_id, path).into(), cx)
                    })
                    .await?;
            }
            None => {
                fs.rename(&source, &dest, fs::RenameOptions::default())
                    .await?;
            }
        }
        if open {
            workspace
                .update_in(cx, |workspace, window, cx| {
                    workspace.open_abs_path(
                        dest,
                        OpenOptions {
                            visible: Some(OpenVisible::All),
                            ..OpenOptions::default()
                        },
                        window,
                        cx,
                    )
                })?
                .await?;
        }
        anyhow::Ok(())
    })
    .detach_and_log_err(cx);
}

/// Paste after Copy: IntelliJ asks with its Copy dialog, into the selected folder.
pub(crate) fn paste_copy(
    panel: &mut ProjectPanel,
    source: SelectedEntry,
    window: &mut Window,
    cx: &mut Context<ProjectPanel>,
) {
    let target = target_folder(panel, cx).and_then(|(worktree_id, _, folder)| {
        let worktree = panel.project.read(cx).worktree_for_id(worktree_id, cx)?;
        Some(worktree.read(cx).absolutize(&folder))
    });
    copy_to(panel, Some(source), target, window, cx);
}

/// Show History for the selected file or folder.
pub(crate) fn show_history(
    panel: &mut ProjectPanel,
    window: &mut Window,
    cx: &mut Context<ProjectPanel>,
) {
    let Some((_, _, abs_path)) = selected(panel, cx) else {
        return;
    };
    panel
        .workspace
        .update(cx, |workspace, cx| {
            spork_local_history::show_history(abs_path, workspace, window, cx)
        })
        .ok();
}

/// Copy Path/Reference…: IntelliJ's Copy popup, each kind of path with what it would copy.
pub(crate) fn copy_path_reference(
    panel: &mut ProjectPanel,
    window: &mut Window,
    cx: &mut Context<ProjectPanel>,
) {
    let Some((selection, entry, abs_path)) = selected(panel, cx) else {
        return;
    };
    let project = panel.project.read(cx);
    let project_path = ProjectPath {
        worktree_id: selection.worktree_id,
        path: entry.path.clone(),
    };
    let repository_path = project
        .git_store()
        .read(cx)
        .repository_and_path_for_project_path(&project_path, cx)
        .map(|(_, repo_path)| repo_path.as_unix_str().to_owned());
    let name = abs_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut paths = vec![
        ("Absolute Path", abs_path.display().to_string()),
        ("File Name", name),
        (
            "Path From Content Root",
            entry.path.as_unix_str().to_owned(),
        ),
    ];
    paths.extend(repository_path.map(|path| ("Path From Repository Root", path)));
    let choices = paths
        .into_iter()
        .map(|(label, value)| {
            let copied = value.clone();
            Choice::new(label, move |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()))
            })
            .with_hint(value)
        })
        .collect();
    open_chooser("Copy", choices, false, 560., window, cx);
}

#[cfg(test)]
mod tests {
    use super::{check_name, delete_message};
    use util::rel_path::RelPath;

    #[test]
    fn new_names_are_checked_as_intellij_does() {
        let src = RelPath::from_unix_str("src").expect("a relative path");
        // src/main.rs is a file, src/bin a folder
        let kind_at = |path: &RelPath| match path.as_unix_str() {
            "src/main.rs" => Some(false),
            "src/bin" => Some(true),
            _ => None,
        };
        assert_eq!(check_name("lib.rs", src, kind_at), None);
        assert_eq!(check_name("bin/tool.rs", src, kind_at), None);
        assert_eq!(
            check_name("main.rs", src, kind_at),
            Some("A file with the name 'main.rs' already exists".into())
        );
        assert_eq!(
            check_name("main.rs/x", src, kind_at),
            Some("A file with the name 'main.rs' already exists".into())
        );
        assert_eq!(
            check_name("bin", src, kind_at),
            Some("A directory with the name 'bin' already exists".into())
        );
        assert_eq!(
            check_name("a/..", src, kind_at),
            Some("'..' is an invalid file name".into())
        );
    }

    #[test]
    fn delete_asks_as_intellij_does() {
        assert_eq!(delete_message(&[]), None);
        assert_eq!(
            delete_message(&[("README.md".into(), false)]).as_deref(),
            Some("Delete file \"README.md\"?")
        );
        assert_eq!(
            delete_message(&[("src".into(), true)]).as_deref(),
            Some(
                "Delete directory \"src\"?\nAll files and subdirectories in \"src\" will be deleted.\nYou might not be able to fully undo this operation!"
            )
        );
        assert_eq!(
            delete_message(&[("a".into(), false), ("b".into(), false)]).as_deref(),
            Some("Delete 2 files?")
        );
    }
}
