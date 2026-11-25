//! Stash and shelf, as in the reference app: the Commit tool window's Shelf and Stash tabs with their
//! toolbars, menus and dialogs, the Stash dialog, and a stash's or shelved changelist's diff in
//! the main area (docs/specs/M12 §1).
// shortcut: tracked changes only; untracked files saved with the stash (its third parent) aren't listed.

use std::collections::HashSet;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Enter, Escape, InputState, Textarea};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, WindowExt as _,
    h_flex, resizable_panel, v_flex, v_resizable,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_git::shelf::{self, Shelved};
use spork_git::{GitError, Stash};
use spork_workbench::{ActionButton, FileStatus};

use crate::notices::Notification;
use crate::util::ResultExt as _;
use crate::{LeftTool, MAX_PATCH_BYTES, RepoView, View};

actions!(
    spork,
    [
        /// Git ▸ Uncommitted Changes ▸ Show Shelf.
        ShowShelf,
        /// Git ▸ Uncommitted Changes ▸ Show Git Stash.
        ShowStash,
        /// Git ▸ Uncommitted Changes ▸ Unstash Changes…: the Stash tab.
        Unstash,
        RefreshStashes,
        /// Spork's own: stashes the tracked changes and leaves them in place.
        StashSnapshot,
        /// Show Diff (⌘D) of the stash or shelved changelist picked.
        ShowSavedDiff,
        /// Unshelve… (⇧⌘U).
        Unshelve,
        /// Unshelve Silently (⌥⌘U).
        UnshelveSilently,
        /// Rename… (F2) of the shelved changelist picked.
        RenameShelf,
        /// Drop / Delete (⌦) of the stash or shelved changelist picked.
        DeleteSaved,
        /// Git ▸ Patch ▸ Apply Patch from Clipboard…
        ApplyPatchFromClipboard,
    ]
);

/// The Stash and Shelf tabs' shortcuts, as the reference app's keymap has them.
pub(crate) fn install(cx: &mut App) {
    let context = || {
        KeyBindingContextPredicate::parse("SavedPatches")
            .ok()
            .map(std::rc::Rc::new)
    };
    let bindings: Vec<KeyBinding> = [
        ("secondary-d", Box::new(ShowSavedDiff) as Box<dyn Action>),
        ("shift-secondary-u", Box::new(Unshelve)),
        ("alt-secondary-u", Box::new(UnshelveSilently)),
        ("shift-f6", Box::new(RenameShelf)),
        ("f2", Box::new(RenameShelf)),
        // the last one is the one menus show
        ("secondary-backspace", Box::new(DeleteSaved)),
        ("backspace", Box::new(DeleteSaved)),
        ("delete", Box::new(DeleteSaved)),
    ]
    .into_iter()
    .filter_map(|(keys, action)| {
        KeyBinding::load(keys, action, context(), false, None, cx).log_err()
    })
    .collect();
    cx.bind_keys(bindings);
}

/// Commands found with Find Action alone, as in the reference app.
pub(crate) fn commands() -> Vec<(SharedString, Box<dyn Action>)> {
    vec![
        ("Refresh Stashes".into(), Box::new(RefreshStashes)),
        ("Stash Snapshot…".into(), Box::new(StashSnapshot)),
    ]
}

/// The Commit tool window's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CommitTab {
    #[default]
    Commit,
    Shelf,
    Stash,
}

/// A stash's files, each with its status letter, or why they couldn't be read.
type StashFiles = Result<Vec<(char, String)>, String>;

/// What the Stash and Shelf tabs show.
pub(crate) struct Saved {
    pub(crate) tab: CommitTab,
    focus: FocusHandle,
    /// The stash picked, by id, or the shelved changelist, by name; a file in it.
    stash: Option<String>,
    shelf: Option<String>,
    file: Option<String>,
    /// The picked stash's files (by its id) once read.
    files: Option<(String, StashFiles)>,
    _files_load: Option<Task<()>>,
    /// Shelved changelists opened in the tree.
    open: HashSet<String>,
    /// Preview Diff: picking a stash or a file shows its diff in the main area.
    preview: bool,
    /// View Options: applied changelists shown; files grouped by directory, by repository.
    show_applied: bool,
    group_dirs: bool,
    group_repo: bool,
    /// The shelved changelist whose diff the main area shows (View::Stash).
    pub(crate) shelf_shown: Option<String>,
    /// The Unstash dialog: As new branch, Pop stash, Reinstate index.
    branch: Entity<InputState>,
    pop: bool,
    reinstate: bool,
    /// The Unshelve dialog's "Remove successfully applied files from the shelf".
    remove_applied: bool,
    /// The files the Unshelve dialog leaves out (unchecked).
    unshelve_skipped: HashSet<String>,
    /// Whether the window was active when last drawn, and the clipboard then held a patch.
    clipboard: std::cell::Cell<(bool, bool)>,
    /// The shelved changelist whose name the Shelf tab edits (Rename…), and its editor.
    renaming: Option<String>,
    rename: Entity<InputState>,
}

impl Saved {
    pub(crate) fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            tab: CommitTab::Commit,
            focus: cx.focus_handle(),
            stash: None,
            shelf: None,
            file: None,
            files: None,
            _files_load: None,
            open: HashSet::new(),
            preview: true,
            show_applied: false,
            group_dirs: false,
            group_repo: false,
            shelf_shown: None,
            branch: cx.new(|cx| InputState::new(window, cx)),
            pop: false,
            reinstate: false,
            remove_applied: false,
            unshelve_skipped: HashSet::new(),
            clipboard: std::cell::Cell::new((false, false)),
            renaming: None,
            rename: cx.new(|cx| InputState::new(window, cx)),
        }
    }
}

/// "On main: wip" → ("wip", "main"), as the reference app lists a stash: its message, then its branch.
fn stash_text(message: &str) -> (String, String) {
    for prefix in ["WIP on ", "On "] {
        if let Some((branch, text)) = message
            .strip_prefix(prefix)
            .and_then(|rest| rest.split_once(": "))
        {
            return (text.to_owned(), branch.to_owned());
        }
    }
    (message.to_owned(), String::new())
}

fn file_color(letter: char, cx: &App) -> Hsla {
    let status = match letter {
        'A' => FileStatus::Added,
        'D' => FileStatus::Deleted,
        'R' => FileStatus::Renamed,
        _ => FileStatus::Modified,
    };
    spork_workbench::file_status_color(status, cx)
}

/// Whether Settings › Version Control › Shelf removes what was unshelved.
fn remove_applied_setting() -> bool {
    crate::settings_window::Store::load().get("shelf.removeApplied") == Some("true")
}

/// Settings › Version Control › Git › Combine stashes and shelves in one tab.
fn combined() -> bool {
    crate::settings_window::Store::load().get("git.combineStashShelf") == Some("true")
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// A tree row of the tabs, found by its label.
fn row(
    id: impl Into<ElementId>,
    label: SharedString,
    depth: usize,
    on: bool,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (active, hover) = (theme.list_active, theme.list_hover);
    h_flex()
        .id(id)
        .role(Role::TreeItem)
        .aria_label(label)
        .aria_selected(on)
        .h(crate::history::ROW_HEIGHT)
        .mx_1()
        .pl(px(6. + 18. * depth as f32))
        .pr_2()
        .gap_1p5()
        .rounded(px(4.))
        .when(on, |r| r.bg(active))
        .when(!on, |r| r.hover(move |r| r.bg(hover)))
}

/// A root node of the combined tab: Stashes or Shelves.
fn group_row(id: &'static str, label: &'static str, cx: &App) -> AnyElement {
    row(id, label.into(), 0, false, cx)
        .font_weight(FontWeight::SEMIBOLD)
        .child(label)
        .into_any_element()
}

/// A toolbar button, labelled with its tooltip.
fn tool(id: &'static str, icon: &'static str, tip: &'static str) -> ActionButton {
    ActionButton::new(id, icon)
        .tooltip(tip)
        .role(Role::Button)
        .aria_label(tip)
}

fn centered(text: impl Into<SharedString>, cx: &App) -> Div {
    v_flex()
        .flex_1()
        .items_center()
        .justify_center()
        .p_4()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

impl RepoView {
    fn shelves(&self) -> &[Shelved] {
        match &self.repo {
            Some(Ok(repo)) => &repo.shelves,
            _ => &[],
        }
    }

    fn stashes(&self) -> &[Stash] {
        match &self.repo {
            Some(Ok(repo)) => &repo.stashes,
            _ => &[],
        }
    }

    /// The tab showing: Shelf and Stash only while they have something; combined, the Stash tab
    /// shows both.
    pub(crate) fn commit_tab(&self) -> CommitTab {
        let (shelf, stash) = (!self.shelves().is_empty(), !self.stashes().is_empty());
        match self.saved.tab {
            CommitTab::Shelf | CommitTab::Stash if (shelf || stash) && combined() => {
                CommitTab::Stash
            }
            CommitTab::Shelf if shelf => CommitTab::Shelf,
            CommitTab::Stash if stash => CommitTab::Stash,
            _ => CommitTab::Commit,
        }
    }

    /// The Commit tool window's tabs, or None while it has only Commit (shown as its title).
    pub(crate) fn commit_tabs(&self, cx: &mut Context<Self>) -> Option<Div> {
        let (shelf, stash) = (!self.shelves().is_empty(), !self.stashes().is_empty());
        if !shelf && !stash {
            return None;
        }
        let combined = combined();
        let current = self.commit_tab();
        let tab =
            |id: &'static str, label: &'static str, tab: CommitTab, cx: &mut Context<Self>| {
                spork_workbench::shell::header_tab(id, label, current == tab, cx)
                    .role(Role::Tab)
                    .aria_label(label)
                    .aria_selected(current == tab)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.saved.tab = tab;
                        cx.notify();
                    }))
            };
        Some(
            h_flex()
                .gap_1()
                .child(tab("tab-commit", "Commit", CommitTab::Commit, cx))
                .when(shelf && !combined, |h| {
                    h.child(tab("tab-shelf", "Shelf", CommitTab::Shelf, cx))
                })
                .when(stash && !combined, |h| {
                    h.child(tab("tab-stash", "Stash", CommitTab::Stash, cx))
                })
                .when(combined, |h| {
                    h.child(tab(
                        "tab-stash",
                        "Stashes and Shelves",
                        CommitTab::Stash,
                        cx,
                    ))
                }),
        )
    }

    /// The Shelf or Stash tab's content, under the Commit tool window's header.
    pub(crate) fn saved_tab(&self, cx: &mut Context<Self>) -> Div {
        let content = match self.commit_tab() {
            CommitTab::Shelf => self.shelf_tab(cx),
            _ => self.stash_tab(cx),
        };
        v_flex()
            .flex_1()
            .min_h_0()
            .track_focus(&self.saved.focus)
            .key_context("SavedPatches")
            .child(content)
    }

    /// Opens the Commit tool window on `tab`, read again: the watcher misses some stash changes.
    pub(crate) fn show_saved(
        &mut self,
        tab: CommitTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reload(cx);
        self.saved.tab = tab;
        self.open_left(LeftTool::Commit, cx);
        self.saved.focus.focus(window, cx);
        cx.notify();
    }

    /// The Stash and Shelf tabs' actions, hooked on the view.
    pub(crate) fn saved_actions(&self, el: Div, cx: &mut Context<Self>) -> Div {
        // Apply Patch from Clipboard… is greyed without a patch on the clipboard.
        // shortcut: the clipboard is read when the window becomes active (or by Spork's own
        // copies), so text copied elsewhere shows only after that; read it as the menu opens if
        // GPUI allows it. The tests' clipboard is read each time.
        let (was_active, mut patch) = self.saved.clipboard.get();
        if self.window_active && !was_active || crate::e2e::active() {
            patch = crate::e2e::clipboard_text(cx).is_some_and(|text| shelf::is_patch(&text));
        }
        self.saved.clipboard.set((self.window_active, patch));
        el.when(patch, |el| {
            el.on_action(cx.listener(|this, _: &ApplyPatchFromClipboard, w, cx| {
                this.apply_clipboard_patch(w, cx)
            }))
        })
        .on_action(
            cx.listener(|this, _: &ShowShelf, w, cx| this.show_saved(CommitTab::Shelf, w, cx)),
        )
        .on_action(
            cx.listener(|this, _: &ShowStash, w, cx| this.show_saved(CommitTab::Stash, w, cx)),
        )
        .on_action(cx.listener(|this, _: &Unstash, w, cx| this.show_saved(CommitTab::Stash, w, cx)))
        .on_action(cx.listener(|this, _: &RefreshStashes, _, cx| this.reload(cx)))
        .on_action(cx.listener(|this, _: &StashSnapshot, w, cx| this.snapshot_dialog(w, cx)))
        .on_action(cx.listener(|this, _: &ShowSavedDiff, w, cx| this.show_saved_diff(w, cx)))
        .on_action(cx.listener(|this, _: &Unshelve, w, cx| {
            if let Some(list) = this.picked_shelf() {
                this.unshelve_dialog(list, w, cx)
            }
        }))
        .on_action(cx.listener(|this, _: &UnshelveSilently, w, cx| {
            if let Some(list) = this.picked_shelf() {
                let remove = remove_applied_setting();
                this.unshelve(list, remove, w, cx)
            }
        }))
        .on_action(cx.listener(|this, _: &RenameShelf, w, cx| {
            // the name is edited in the Shelf tab's tree
            if let Some(list) = this
                .picked_shelf()
                .filter(|_| this.commit_tab() == CommitTab::Shelf)
            {
                this.rename_shelf(list, w, cx)
            }
        }))
        .on_action(
            cx.listener(|this, _: &DeleteSaved, w, cx| match this.commit_tab() {
                CommitTab::Shelf => {
                    if let Some(list) = this.picked_shelf() {
                        this.delete_shelf(list, w, cx)
                    }
                }
                CommitTab::Stash => {
                    if let Some(stash) = this.picked_stash() {
                        this.confirm_drop(stash, w, cx)
                    } else if let Some(list) = this.picked_combined_shelf() {
                        this.delete_shelf(list, w, cx)
                    }
                }
                CommitTab::Commit => {}
            }),
        )
    }

    /// The shelved changelist picked in the combined tab, where a stash or a shelf is picked and
    /// only the changelists neither applied nor deleted are listed.
    fn picked_combined_shelf(&self) -> Option<Shelved> {
        self.picked_stash()
            .is_none()
            .then(|| self.picked_shelf())
            .flatten()
            .filter(|l| !l.recycled && !l.deleted && combined())
    }

    fn picked_stash(&self) -> Option<Stash> {
        let id = self.saved.stash.as_ref()?;
        self.stashes().iter().find(|s| &s.id == id).cloned()
    }

    fn picked_shelf(&self) -> Option<Shelved> {
        let name = self.saved.shelf.as_ref()?;
        self.shelves().iter().find(|s| &s.name == name).cloned()
    }

    /// Show Diff: the picked stash's or shelved changelist's diff in the main area.
    fn show_saved_diff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.commit_tab() {
            CommitTab::Stash => {
                if let Some(stash) = self.picked_stash() {
                    self.show_stash(stash, cx)
                } else if let Some(list) = self.picked_combined_shelf() {
                    self.show_shelf(list, window, cx)
                }
            }
            CommitTab::Shelf => {
                if let Some(list) = self.picked_shelf() {
                    self.show_shelf(list, window, cx)
                }
            }
            CommitTab::Commit => {}
        }
    }

    // ---- the Stash tab

    fn pick_stash(
        &mut self,
        stash: &Stash,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.saved.focus.focus(window, cx);
        // the combined tab picks a stash or a shelf
        if combined() {
            self.saved.shelf = None;
        }
        if self.saved.stash.as_deref() != Some(stash.id.as_str()) {
            self.saved.stash = Some(stash.id.clone());
            self.saved.file = None;
            self.load_stash_files(stash.id.clone(), cx);
        }
        if open || self.saved.preview {
            self.show_stash(stash.clone(), cx);
        }
        cx.notify();
    }

    fn load_stash_files(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let root = repo.root.clone();
        self.saved.files = None;
        self.saved._files_load = Some(cx.spawn(async move |this, cx| {
            let files = cx
                .background_spawn({
                    let id = id.clone();
                    async move {
                        spork_git::commit_details(&root, &id, Some(&format!("{id}^1")), false)
                            .map(|d| shelf::files(&d.patch))
                            .map_err(|e| e.to_string())
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.saved.files = Some((id, files));
                cx.notify();
            })
            .ok();
        }));
    }

    fn stash_tab(&self, cx: &mut Context<Self>) -> Div {
        let combined = combined();
        let stashes = self.stashes().to_vec();
        let picked = self.picked_stash();
        let picked_shelf = self.picked_combined_shelf();
        let view = cx.entity();
        let muted = cx.theme().muted_foreground;
        // combined, stashes and shelves each go under their root node
        let depth = usize::from(combined);
        let mut rows: Vec<AnyElement> = Vec::new();
        if combined && !stashes.is_empty() {
            rows.push(group_row("saved-stashes", "Stashes", cx));
        }
        rows.extend(stashes.iter().enumerate().map(|(ix, stash)| {
            let (text, branch) = stash_text(&stash.message);
            let on = picked.as_ref().is_some_and(|p| p.id == stash.id);
            let (pick, right_pick) = (stash.clone(), stash.clone());
            row(("stash-row", ix), text.clone().into(), depth, on, cx)
                .child(
                    spork_workbench::icon("vcs/shelve")
                        .size(px(16.))
                        .flex_none(),
                )
                .child(div().flex_1().min_w_0().truncate().child(text))
                .child(div().flex_none().text_xs().text_color(muted).child(format!(
                    "{branch}  {}",
                    crate::history::relative_age(stash.time, now())
                )))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, window, cx| {
                        this.pick_stash(&right_pick, false, window, cx)
                    }),
                )
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    this.pick_stash(&pick, event.click_count() == 2, window, cx)
                }))
                .context_menu(stash_menu(view.clone(), stash.clone()))
                .into_any_element()
        }));
        if combined {
            let lists: Vec<Shelved> = self
                .shelves()
                .iter()
                .filter(|l| !l.deleted && !l.recycled)
                .cloned()
                .collect();
            if !lists.is_empty() {
                rows.push(group_row("saved-shelves", "Shelves", cx));
            }
            rows.extend(lists.iter().enumerate().map(|(ix, list)| {
                let on = picked_shelf.as_ref().is_some_and(|p| p.name == list.name);
                let (pick, right_pick) = (list.name.clone(), list.name.clone());
                row(
                    ("saved-shelf", ix),
                    list.description.clone().into(),
                    1,
                    on,
                    cx,
                )
                .child(spork_workbench::icon("vcs/patch").size(px(16.)).flex_none())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(list.description.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(crate::history::relative_age(list.date / 1000, now())),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, window, cx| {
                        this.saved.stash = None;
                        this.pick_shelf(&right_pick, None, false, window, cx)
                    }),
                )
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    this.saved.stash = None;
                    let open = event.click_count() == 2 || this.saved.preview;
                    this.pick_shelf(&pick, None, open, window, cx)
                }))
                .context_menu(saved_shelf_menu(view.clone(), list.clone()))
                .into_any_element()
            }));
        }
        let list = v_flex()
            .id("stash-list")
            .role(Role::Tree)
            .aria_label("Stashes")
            .size_full()
            .py_1()
            .overflow_y_scrollbar()
            .children(rows);
        let changes = self.stash_changes(picked.as_ref(), picked_shelf.as_ref(), cx);
        let busy = self.busy || (picked.is_none() && picked_shelf.is_none());
        let apply = |id: &'static str, label: &'static str, pop: bool, cx: &mut Context<Self>| {
            let (picked, picked_shelf) = (picked.clone(), picked_shelf.clone());
            spork_workbench::Button::new(id, label)
                .role(Role::Button)
                .aria_label(label)
                .disabled(busy)
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(stash) = &picked {
                        this.stash_apply(stash.index, pop, window, cx)
                    } else if let Some(list) = &picked_shelf {
                        this.unshelve(list.clone(), pop, window, cx)
                    }
                }))
        };
        let buttons = h_flex()
            .flex_none()
            .px_2()
            .py_2()
            .gap_2()
            .child(apply("stash-apply-button", "Apply", false, cx))
            .child(apply("stash-pop-button", "Pop", true, cx))
            .child(
                div()
                    .id("stash-help")
                    .text_color(muted)
                    .tooltip(|window, cx| {
                        Tooltip::new(
                            "Apply — apply and keep the stash in the list.\nPop — apply and remove the stash from the list.",
                        )
                        .build(window, cx)
                    })
                    .child(spork_workbench::icon("general/help").size(px(16.))),
            );
        v_flex().size_full().child(
            div().flex_1().min_h_0().child(
                v_resizable("stash-split")
                    .child(resizable_panel().child(list))
                    .child(
                        resizable_panel().size(px(240.)).child(
                            v_flex()
                                .size_full()
                                .overflow_hidden()
                                .border_t_1()
                                .border_color(spork_workbench::Tokens::get(cx).separator)
                                .child(changes)
                                .child(buttons),
                        ),
                    ),
            ),
        )
    }

    /// A file row of the changes under the Stash tab.
    fn saved_file_row(
        &self,
        id: impl Into<ElementId>,
        letter: char,
        path: &str,
        on: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
        row(id, name.to_owned().into(), 0, on, cx)
            .child(spork_workbench::image(spork_workbench::file_icon(name), cx))
            .child(
                div()
                    .flex_none()
                    .text_color(file_color(letter, cx))
                    .child(name.to_owned()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(cx.theme().muted_foreground)
                    .child(dir.to_owned()),
            )
    }

    /// The picked stash's (combined, or shelved changelist's) files, under the toolbar with
    /// Preview Diff.
    fn stash_changes(
        &self,
        picked: Option<&Stash>,
        shelf: Option<&Shelved>,
        cx: &mut Context<Self>,
    ) -> Div {
        let toolbar = h_flex()
            .id("saved-changes-toolbar")
            .role(Role::Toolbar)
            .aria_label("Saved Patches Changes Toolbar")
            .h(px(32.))
            .flex_none()
            .px_2()
            .gap_0p5()
            .child(
                tool("saved-preview", "actions/preview", "Preview Diff")
                    .selected(self.saved.preview)
                    .aria_toggled(if self.saved.preview {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.saved.preview = !this.saved.preview;
                        cx.notify();
                    })),
            );
        let view = cx.entity();
        let body = match (picked, shelf, &self.saved.files) {
            (None, Some(list), _) => {
                let files = self
                    .tab_root()
                    .and_then(|root| shelf::patch(&root, list).ok())
                    .map(|p| shelf::files(&p))
                    .unwrap_or_default();
                let rows = files.iter().enumerate().map(|(ix, (letter, path))| {
                    let on = self.saved.file.as_deref() == Some(path.as_str());
                    let (pick, right_pick) = ((list.name.clone(), path.clone()), path.clone());
                    self.saved_file_row(("saved-shelf-file", ix), *letter, path, on, cx)
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, _, _, cx| {
                                this.saved.file = Some(right_pick.clone());
                                cx.notify();
                            }),
                        )
                        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                            let open = event.click_count() == 2 || this.saved.preview;
                            let (name, path) = pick.clone();
                            this.pick_shelf(&name, Some(path), open, window, cx)
                        }))
                        .context_menu(saved_shelf_file_menu(
                            view.clone(),
                            list.clone(),
                            path.clone(),
                        ))
                });
                div().flex_1().min_h_0().child(
                    v_flex()
                        .id("stash-files")
                        .size_full()
                        .overflow_y_scrollbar()
                        .children(rows),
                )
            }
            (None, _, _) => centered("Select stash or shelf to view files", cx),
            (Some(stash), _, Some((id, Ok(files)))) if *id == stash.id => {
                let rows = files.iter().enumerate().map(|(ix, (letter, path))| {
                    let on = self.saved.file.as_deref() == Some(path.as_str());
                    let (pick, right_pick) = ((stash.clone(), path.clone()), path.clone());
                    self.saved_file_row(("stash-file", ix), *letter, path, on, cx)
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, _, _, cx| {
                                this.saved.file = Some(right_pick.clone());
                                cx.notify();
                            }),
                        )
                        .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                            let (stash, path) = pick.clone();
                            this.pick_stash_file(stash, path, event.click_count() == 2, cx)
                        }))
                        .context_menu(stash_file_menu(view.clone(), stash.clone(), path.clone()))
                });
                div().flex_1().min_h_0().child(
                    v_flex()
                        .id("stash-files")
                        .size_full()
                        .overflow_y_scrollbar()
                        .children(rows),
                )
            }
            (Some(stash), _, Some((id, Err(e)))) if *id == stash.id => centered(e.clone(), cx),
            _ => centered("Loading…", cx),
        };
        v_flex().flex_1().min_h_0().child(toolbar).child(body)
    }

    fn pick_stash_file(&mut self, stash: Stash, path: String, open: bool, cx: &mut Context<Self>) {
        self.saved.file = Some(path.clone());
        if open || self.saved.preview {
            let shown = self
                .stash_shown
                .as_ref()
                .is_some_and(|(s, _)| s.id == stash.id)
                && self.view == View::Stash;
            match shown {
                // the IDE's diff tab comes forward
                true => self.open_diff_view(View::Stash, cx),
                false => self.show_stash(stash, cx),
            }
            self.diff.update(cx, |s, cx| s.scroll_to_file(&path, cx));
        }
        cx.notify();
    }

    pub(crate) fn stash_apply(
        &mut self,
        index: usize,
        pop: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_outcome(
            window,
            cx,
            "Stash successfully applied".into(),
            move |root| spork_git::stash::unstash(root, index, pop, false),
        )
    }

    /// Drop: the reference app asks first.
    fn confirm_drop(&mut self, stash: Stash, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        let name = format!("stash@{{{}}}", stash.index);
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let view = view.clone();
            let index = stash.index;
            dialog
                .title(format!("Remove Stash {name}?"))
                .description(format!(
                    "Do you want to remove {name}?\n\"{}\"",
                    stash.message
                ))
                .confirm()
                .ok_text("Yes")
                .cancel_text("No")
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        this.run_op(
                            window,
                            cx,
                            move |root| spork_git::stash_drop(root, index),
                            |_, _, _, _| {},
                        )
                    });
                    true
                })
        });
    }

    fn confirm_clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let view = view.clone();
            dialog
                .title("Remove All Stashes?")
                .description("Remove all stashes? This cannot be undone.")
                .confirm()
                .ok_text("Yes")
                .cancel_text("No")
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        this.run_op(window, cx, spork_git::stash::clear, |_, _, _, _| {})
                    });
                    true
                })
        });
    }

    /// Unstash…: pop or apply the stash, its index too, or as a new branch.
    fn unstash_dialog(&mut self, stash: Stash, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let root_name = repo
            .root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let branch = repo.head.clone().unwrap_or_else(|| "HEAD".into());
        self.saved.pop = false;
        self.saved.reinstate = false;
        self.saved
            .branch
            .update(cx, |s, cx| s.set_value("", window, cx));
        let view = cx.entity();
        let muted = cx.theme().muted_foreground;
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let (pop, reinstate) = (view.read(cx).saved.pop, view.read(cx).saved.reinstate);
            let input = view.read(cx).saved.branch.clone();
            let new_branch = input.read(cx).value().trim().to_owned();
            let ok = match (new_branch.is_empty(), pop) {
                (false, _) => "Branch",
                (true, true) => "Pop Stash",
                (true, false) => "Apply Stash",
            };
            let check = |id: &'static str,
                         label: &'static str,
                         on: bool,
                         set: fn(&mut RepoView, bool),
                         cx: &mut App| {
                let view = view.clone();
                spork_workbench::controls::labeled_check(id, label, on, cx)
                    .role(Role::CheckBox)
                    .aria_label(label)
                    .aria_toggled(if on { Toggled::True } else { Toggled::False })
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            set(this, !on);
                            cx.notify();
                        })
                    })
            };
            let (view, index) = (view.clone(), stash.index);
            dialog
                .title(format!("Unstash Changes in {root_name}"))
                .description(
                    v_flex()
                        .gap_3()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(div().text_color(muted).child("Current branch:"))
                                .child(branch.clone()),
                        )
                        .child(
                            h_flex().gap_2().child("As new branch:").child(
                                div()
                                    .flex_1()
                                    .child(spork_workbench::TextField::new(&input)),
                            ),
                        )
                        .child(
                            h_flex()
                                .gap_4()
                                .child(check(
                                    "unstash-pop",
                                    "Pop stash",
                                    pop,
                                    |this, on| this.saved.pop = on,
                                    cx,
                                ))
                                .child(check(
                                    "unstash-index",
                                    "Reinstate index",
                                    reinstate,
                                    |this, on| this.saved.reinstate = on,
                                    cx,
                                )),
                        ),
                )
                .confirm()
                .ok_text(ok)
                .on_ok(move |_, window, cx| {
                    let new_branch = new_branch.clone();
                    view.update(cx, |this, cx| {
                        let (pop, reinstate) = (this.saved.pop, this.saved.reinstate);
                        this.run_outcome(
                            window,
                            cx,
                            "Stash successfully applied".into(),
                            move |root| match new_branch.is_empty() {
                                true => spork_git::stash::unstash(root, index, pop, reinstate),
                                false => spork_git::stash::branch_from(root, index, &new_branch)
                                    .map(|()| spork_git::Outcome::Done),
                            },
                        )
                    });
                    true
                })
        });
    }

    /// Compare with Local or Compare Before with Local: the stashed file (or the one it was made
    /// on) against the working tree, in the main area.
    fn compare_stash_file(
        &mut self,
        stash: Stash,
        path: String,
        before: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rev = match before {
            true => format!("{}^1", stash.id),
            false => stash.id.clone(),
        };
        self.run_op(
            window,
            cx,
            move |root| {
                spork_git::run(root, &["diff", &rev, "--", &path])
                    .map(|out| String::from_utf8_lossy(&out).into_owned())
            },
            move |this, patch, _, cx| {
                this.selected = None;
                this.details = None;
                this.saved.shelf_shown = None;
                this.stash_shown = Some((stash, Some(Ok(()))));
                this.open_diff_view(View::Stash, cx);
                let diff = this.diff.clone();
                this.set_patch(&diff, &patch, cx);
                cx.notify();
            },
        )
    }

    // ---- the Shelf tab

    fn pick_shelf(
        &mut self,
        name: &str,
        file: Option<String>,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.saved.focus.focus(window, cx);
        self.saved.shelf = Some(name.to_owned());
        self.saved.file = file.clone();
        if let Some(list) = self.picked_shelf().filter(|_| open) {
            self.show_shelf(list, window, cx);
            if let Some(path) = file {
                self.diff.update(cx, |s, cx| s.scroll_to_file(&path, cx));
            }
        }
        cx.notify();
    }

    fn shelf_tab(&self, cx: &mut Context<Self>) -> Div {
        let lists: Vec<Shelved> = self
            .shelves()
            .iter()
            .filter(|l| !l.deleted && (self.saved.show_applied || !l.recycled))
            .cloned()
            .collect();
        let deleted: Vec<Shelved> = self
            .shelves()
            .iter()
            .filter(|l| l.deleted)
            .cloned()
            .collect();
        let root = self.tab_root().unwrap_or_default();
        let view = cx.entity();
        let show_applied = self.saved.show_applied;
        let (group_dirs, group_repo) = (self.saved.group_dirs, self.saved.group_repo);
        let options_view = view.clone();
        let toolbar = h_flex()
            .h(px(32.))
            .flex_none()
            .px_2()
            .gap_0p5()
            .child(
                tool("shelf-unshelve", "vcs/unshelve", "Unshelve Silently")
                    .disabled(self.saved.shelf.is_none() || self.busy)
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(list) = this.picked_shelf() {
                            this.unshelve(list, remove_applied_setting(), window, cx)
                        }
                    })),
            )
            .child(
                tool("shelf-diff", "vcs/diff", "Show Diff")
                    .disabled(self.saved.shelf.is_none())
                    .on_click(cx.listener(|this, _, window, cx| this.show_saved_diff(window, cx))),
            )
            .child(
                tool("shelf-view-options", "general/show", "View Options")
                    .popup_marker()
                    .dropdown_menu(move |menu, window, cx| {
                        let group_view = options_view.clone();
                        menu.submenu("Group By", window, cx, move |menu, _, _| {
                            menu.label("Group By")
                                .item(view_toggle(&group_view, "Directory", group_dirs, |this| {
                                    this.saved.group_dirs = !this.saved.group_dirs
                                }))
                                .item(view_toggle(&group_view, "Repository", group_repo, |this| {
                                    this.saved.group_repo = !this.saved.group_repo
                                }))
                        })
                        .separator()
                        .label("Show")
                        .item(view_toggle(
                            &options_view,
                            "Show/Hide Applied Shelved Changes",
                            show_applied,
                            |this| this.saved.show_applied = !this.saved.show_applied,
                        ))
                    }),
            )
            .child(
                div()
                    .w(px(1.))
                    .h(px(18.))
                    .mx_1()
                    .bg(spork_workbench::Tokens::get(cx).separator),
            )
            .child(
                tool("shelf-expand", "general/expandAll", "Expand All").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.saved.open = this.shelves().iter().map(|l| l.name.clone()).collect();
                        this.saved.open.insert(String::new());
                        cx.notify();
                    },
                )),
            )
            .child(
                tool("shelf-collapse", "general/collapseAll", "Collapse All").on_click(
                    cx.listener(|this, _, _, cx| {
                        this.saved.open.clear();
                        cx.notify();
                    }),
                ),
            );
        let mut rows: Vec<AnyElement> = Vec::new();
        for list in &lists {
            self.shelf_rows(list, 0, &root, &view, &mut rows, cx);
        }
        if !deleted.is_empty() {
            // the "Recently Deleted" node is keyed by the empty name, which no changelist has
            let open = self.saved.open.contains("");
            rows.push(
                row("shelf-deleted", "Recently Deleted".into(), 0, false, cx)
                    .child(chevron(open))
                    .child("Recently Deleted")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if !this.saved.open.remove("") {
                            this.saved.open.insert(String::new());
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
            );
            if open {
                for list in &deleted {
                    self.shelf_rows(list, 1, &root, &view, &mut rows, cx);
                }
            }
        }
        let tree = v_flex()
            .id("shelf-tree")
            .role(Role::Tree)
            .aria_label("Shelves")
            .flex_1()
            .min_h_0()
            .py_1()
            .overflow_y_scrollbar()
            .children(rows)
            .context_menu(shelf_menu(view.clone(), None, None));
        v_flex().size_full().child(toolbar).child(tree)
    }

    /// A shelved changelist's row, then its files' while it is open.
    fn shelf_rows(
        &self,
        list: &Shelved,
        depth: usize,
        root: &std::path::Path,
        view: &Entity<RepoView>,
        rows: &mut Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let open = self.saved.open.contains(&list.name);
        let on =
            self.saved.shelf.as_deref() == Some(list.name.as_str()) && self.saved.file.is_none();
        let (pick, toggle, right) = (list.name.clone(), list.name.clone(), list.name.clone());
        let dim = list.recycled || list.deleted;
        rows.push(
            row(
                SharedString::from(format!("shelf-{}", list.name)),
                list.description.clone().into(),
                depth,
                on,
                cx,
            )
            .child(
                div()
                    .id(SharedString::from(format!("shelf-chevron-{}", list.name)))
                    .child(chevron(open))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if !this.saved.open.remove(&toggle) {
                            this.saved.open.insert(toggle.clone());
                        }
                        cx.notify();
                    })),
            )
            .child(spork_workbench::icon("vcs/patch").size(px(16.)).flex_none())
            .map(
                |r| match self.saved.renaming.as_deref() == Some(list.name.as_str()) {
                    // Rename… edits the name in the tree: Enter keeps it, Escape leaves it
                    true => r.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .capture_action(cx.listener(|this, _: &Enter, window, cx| {
                                cx.stop_propagation();
                                this.shelf_finish_rename(true, window, cx)
                            }))
                            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                                cx.stop_propagation();
                                this.shelf_finish_rename(false, window, cx)
                            }))
                            .child(
                                spork_workbench::TextField::new(&self.saved.rename)
                                    .aria_label("Shelved changelist name"),
                            ),
                    ),
                    false => r
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .when(dim, |d| d.text_color(muted))
                                .child(list.description.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(muted)
                                .child(crate::history::relative_age(list.date / 1000, now())),
                        ),
                },
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, window, cx| {
                    this.pick_shelf(&right, None, false, window, cx)
                }),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if event.click_count() == 2 && !this.saved.open.remove(&pick) {
                    this.saved.open.insert(pick.clone());
                }
                this.pick_shelf(&pick, None, false, window, cx)
            }))
            .context_menu(shelf_menu(view.clone(), Some(list.clone()), None))
            .into_any_element(),
        );
        if !open {
            return;
        }
        let files = shelf::patch(root, list)
            .map(|p| shelf::files(&p))
            .unwrap_or_default();
        let mut depth = depth + 1;
        if self.saved.group_repo {
            let name = root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            rows.push(
                row(
                    SharedString::from(format!("shelf-repo-{}", list.name)),
                    name.clone().into(),
                    depth,
                    false,
                    cx,
                )
                .child(
                    spork_workbench::icon("vcs/changelist")
                        .size(px(16.))
                        .flex_none(),
                )
                .child(name)
                .into_any_element(),
            );
            depth += 1;
        }
        let mut last_dir: Option<String> = None;
        for (ix, (letter, path)) in files.iter().enumerate() {
            let (dir, name) = path
                .rsplit_once('/')
                .map_or((String::new(), path.clone()), |(d, n)| {
                    (d.to_owned(), n.to_owned())
                });
            let file_depth = match self.saved.group_dirs && !dir.is_empty() {
                true => {
                    if last_dir.as_deref() != Some(dir.as_str()) {
                        rows.push(
                            row(
                                SharedString::from(format!("shelf-dir-{}-{ix}", list.name)),
                                dir.clone().into(),
                                depth,
                                false,
                                cx,
                            )
                            .child(spork_workbench::image("nodes/folder", cx))
                            .child(dir.clone())
                            .into_any_element(),
                        );
                        last_dir = Some(dir.clone());
                    }
                    depth + 1
                }
                false => depth,
            };
            let on = self.saved.shelf.as_deref() == Some(list.name.as_str())
                && self.saved.file.as_deref() == Some(path.as_str());
            let (pick, right) = (
                (list.name.clone(), path.clone()),
                (list.name.clone(), path.clone()),
            );
            rows.push(
                row(
                    SharedString::from(format!("shelf-file-{}-{ix}", list.name)),
                    name.clone().into(),
                    file_depth,
                    on,
                    cx,
                )
                .child(spork_workbench::image(
                    spork_workbench::file_icon(&name),
                    cx,
                ))
                .child(
                    div()
                        .flex_none()
                        .text_color(file_color(*letter, cx))
                        .child(name),
                )
                .when(!self.saved.group_dirs, |r| {
                    r.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(muted)
                            .child(dir.clone()),
                    )
                })
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, window, cx| {
                        this.pick_shelf(&right.0, Some(right.1.clone()), false, window, cx)
                    }),
                )
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    let open = event.click_count() == 2 || this.saved.preview;
                    this.pick_shelf(&pick.0, Some(pick.1.clone()), open, window, cx)
                }))
                .context_menu(shelf_menu(
                    view.clone(),
                    Some(list.clone()),
                    Some(path.clone()),
                ))
                .into_any_element(),
            );
        }
    }

    /// Show Diff of a shelved changelist: its patch in the main area.
    fn show_shelf(&mut self, list: Shelved, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.tab_root() else { return };
        match shelf::patch(&root, &list) {
            Ok(patch) => {
                self.selected = None;
                self.details = None;
                self.stash_shown = None;
                self.saved.shelf_shown = Some(list.name.clone());
                self.open_diff_view(View::Stash, cx);
                let diff = self.diff.clone();
                let patch = if patch.len() <= MAX_PATCH_BYTES {
                    patch.as_str()
                } else {
                    ""
                };
                self.set_patch(&diff, patch, cx);
                if let Some(path) = &self.saved.file {
                    diff.update(cx, |s, cx| s.scroll_to_file(path, cx));
                }
                cx.notify();
            }
            Err(e) => window.push_notification(Notification::error(e.to_string()), cx),
        }
    }

    /// Compare with Local: the picked file (or every file of the changelist) as shelved against
    /// the working tree's.
    fn compare_shelf_file(&mut self, list: Shelved, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.saved.file.clone();
        self.run_op(
            window,
            cx,
            move |root| {
                let paths: Vec<&str> = path.as_deref().into_iter().collect();
                shelf::compare_with_local(root, &list, &paths).map(|diff| (list, diff))
            },
            |this, (list, diff), _, cx| {
                this.selected = None;
                this.details = None;
                this.stash_shown = None;
                this.saved.shelf_shown = Some(list.name);
                this.open_diff_view(View::Stash, cx);
                let state = this.diff.clone();
                this.set_patch(&state, &diff, cx);
                cx.notify();
            },
        )
    }

    fn unshelve(
        &mut self,
        list: Shelved,
        remove: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_outcome(window, cx, "Changes unshelved".into(), move |root| {
            shelf::unshelve(root, &list, remove)
        })
    }

    /// Unshelve, or Unshelve and Remove, of some files of a shelved changelist.
    fn shelf_unshelve_files(
        &mut self,
        list: Shelved,
        paths: Vec<String>,
        remove: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_outcome(window, cx, "Changes unshelved".into(), move |root| {
            let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
            shelf::unshelve_files(root, &list, &paths, remove)
        })
    }

    /// Unshelve…: the changelist's files, each checked to unshelve it, and whether what applies
    /// leaves the shelf.
    fn unshelve_dialog(&mut self, list: Shelved, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.tab_root() else { return };
        let files = shelf::patch(&root, &list)
            .map(|p| shelf::files(&p))
            .unwrap_or_default();
        self.saved.remove_applied = remove_applied_setting();
        self.saved.unshelve_skipped.clear();
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let remove = view.read(cx).saved.remove_applied;
            let skipped = view.read(cx).saved.unshelve_skipped.clone();
            let shelved = files.clone();
            let toggle_view = view.clone();
            let label = "Remove successfully applied files from the shelf";
            let (view, list) = (view.clone(), list.clone());
            dialog
                .title("Unshelve Changes")
                .description(
                    v_flex()
                        .gap_3()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(list.description.clone()),
                        )
                        .child(v_flex().gap_1().children(files.iter().enumerate().map(
                            |(ix, (letter, path))| {
                                let (view, toggled) = (view.clone(), path.clone());
                                h_flex()
                                    .gap_2()
                                    .child(
                                        spork_workbench::controls::labeled_check(
                                            format!("unshelve-file-{ix}"),
                                            "",
                                            !skipped.contains(path),
                                            cx,
                                        )
                                        .aria_label(path.clone())
                                        .on_click(
                                            move |_, _, cx| {
                                                view.update(cx, |this, cx| {
                                                    let skipped = &mut this.saved.unshelve_skipped;
                                                    if !skipped.remove(&toggled) {
                                                        skipped.insert(toggled.clone());
                                                    }
                                                    cx.notify();
                                                })
                                            },
                                        ),
                                    )
                                    .child(
                                        div()
                                            .text_color(file_color(*letter, cx))
                                            .child(path.clone()),
                                    )
                            },
                        )))
                        .child(
                            spork_workbench::controls::labeled_check(
                                "unshelve-remove",
                                label,
                                remove,
                                cx,
                            )
                            .role(Role::CheckBox)
                            .aria_label(label)
                            .aria_toggled(if remove {
                                Toggled::True
                            } else {
                                Toggled::False
                            })
                            .on_click(move |_, _, cx| {
                                toggle_view.update(cx, |this, cx| {
                                    this.saved.remove_applied = !this.saved.remove_applied;
                                    cx.notify();
                                })
                            }),
                        ),
                )
                .confirm()
                .ok_text("Unshelve Changes")
                .on_ok(move |_, window, cx| {
                    let list = list.clone();
                    view.update(cx, |this, cx| {
                        let remove = this.saved.remove_applied;
                        let skipped = std::mem::take(&mut this.saved.unshelve_skipped);
                        let checked: Vec<String> = shelved
                            .iter()
                            .map(|(_, path)| path.clone())
                            .filter(|path| !skipped.contains(path))
                            .collect();
                        match (skipped.is_empty(), checked.is_empty()) {
                            (true, _) => this.unshelve(list, remove, window, cx),
                            (false, false) => {
                                this.shelf_unshelve_files(list, checked, remove, window, cx)
                            }
                            (false, true) => {}
                        }
                    });
                    true
                })
        });
    }

    /// Restore: an applied or deleted changelist back among the others.
    fn restore_shelf(&mut self, list: Shelved, window: &mut Window, cx: &mut Context<Self>) {
        self.run_op(
            window,
            cx,
            move |root| {
                shelf::update(
                    root,
                    &Shelved {
                        recycled: false,
                        deleted: false,
                        ..list
                    },
                )
            },
            |_, _, _, _| {},
        )
    }

    /// Rename…: the changelist's name becomes editable in the tree.
    fn rename_shelf(&mut self, list: Shelved, window: &mut Window, cx: &mut Context<Self>) {
        self.saved.renaming = Some(list.name.clone());
        let description = list.description;
        self.saved.rename.update(cx, |state, cx| {
            state.set_value(description, window, cx);
            state.focus(window, cx);
        });
        cx.notify();
    }

    /// Ends Rename…: `keep` writes the name typed, unless it is empty or unchanged.
    fn shelf_finish_rename(&mut self, keep: bool, window: &mut Window, cx: &mut Context<Self>) {
        let list = self
            .saved
            .renaming
            .take()
            .and_then(|name| self.shelves().iter().find(|l| l.name == name).cloned());
        self.saved.focus.focus(window, cx);
        cx.notify();
        let text = self.saved.rename.read(cx).value().trim().to_owned();
        let Some(list) = list.filter(|l| keep && !text.is_empty() && text != l.description) else {
            return;
        };
        self.run_op(
            window,
            cx,
            move |root| {
                shelf::update(
                    root,
                    &Shelved {
                        description: text,
                        ..list
                    },
                )
            },
            |_, _, _, _| {},
        )
    }

    /// Delete: the changelist goes to Recently Deleted; deleted there, it is gone.
    fn delete_shelf(&mut self, list: Shelved, window: &mut Window, cx: &mut Context<Self>) {
        let message = format!(
            "the '{}' changelist deleted from shelf successfully",
            list.description
        );
        self.run_op(
            window,
            cx,
            move |root| match list.deleted {
                true => shelf::remove(root, &list),
                false => shelf::update(
                    root,
                    &Shelved {
                        deleted: true,
                        ..list
                    },
                ),
            },
            move |_, _, window, cx| window.push_notification(Notification::success(message), cx),
        )
    }

    /// Create Patch…: the changelist's patch saved where the user picks.
    fn save_shelf_patch(&mut self, list: Shelved, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.tab_root() else { return };
        let picked = cx.prompt_for_new_path(&root, Some(&format!("{}.patch", list.name)));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(file))) = picked.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.run_op(
                    window,
                    cx,
                    move |root| {
                        let patch = shelf::patch(root, &list)?;
                        std::fs::write(&file, patch)
                            .map(|()| file)
                            .map_err(|e| GitError::Other {
                                code: None,
                                stderr: e.to_string(),
                            })
                    },
                    |_, file, window, cx| {
                        window.push_notification(
                            Notification::success(format!(
                                "Successfully created patch {}",
                                file.display()
                            )),
                            cx,
                        )
                    },
                )
            })
            .ok();
        })
        .detach();
    }

    fn copy_shelf_patch(&mut self, list: Shelved, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.tab_root() else { return };
        match shelf::patch(&root, &list) {
            Ok(patch) => {
                crate::e2e::write_clipboard(patch, cx);
                self.saved.clipboard.set((self.window_active, true));
                window.push_notification(Notification::success("Patch copied to clipboard"), cx)
            }
            Err(e) => window.push_notification(Notification::error(e.to_string()), cx),
        }
    }

    /// Import Patches…: patch files picked become shelved changelists.
    fn import_patches(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(files) = crate::e2e::take_picked() {
            return self.shelf_import(files, window, cx);
        }
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Import Patches".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(files))) = picked.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| this.shelf_import(files, window, cx))
                .ok();
        })
        .detach();
    }

    fn shelf_import(
        &mut self,
        files: Vec<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.saved.tab = CommitTab::Shelf;
        self.run_op(
            window,
            cx,
            move |root| {
                files
                    .iter()
                    .try_for_each(|file| shelf::import(root, file).map(drop))
            },
            |_, _, _, _| {},
        )
    }

    // ---- dialogs reached from the Git menu

    /// Apply Patch from Clipboard…: the patch on the clipboard applied to the working tree.
    fn apply_clipboard_patch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(patch) = crate::e2e::clipboard_text(cx) else {
            return;
        };
        self.run_outcome(window, cx, "Patch applied".into(), move |root| {
            shelf::apply(root, &patch, &[])
        })
    }

    /// The reference app's Stash dialog: the root, the branch, a message, Keep index, Include untracked.
    pub(crate) fn stash_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let (root, branch) = (
            repo.root.display().to_string(),
            repo.head.clone().unwrap_or_else(|| "HEAD".into()),
        );
        self.stash_message
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.stash_untracked = false;
        self.stash_keep = false;
        let (view, input) = (cx.entity(), self.stash_message.clone());
        let muted = cx.theme().muted_foreground;
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let (keep, untracked) = (view.read(cx).stash_keep, view.read(cx).stash_untracked);
            let check = |id: &'static str,
                         label: &'static str,
                         on: bool,
                         set: fn(&mut RepoView, bool),
                         cx: &mut App| {
                let view = view.clone();
                spork_workbench::controls::labeled_check(id, label, on, cx)
                    .role(Role::CheckBox)
                    .aria_label(label)
                    .aria_toggled(if on { Toggled::True } else { Toggled::False })
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            set(this, !on);
                            cx.notify();
                        })
                    })
            };
            let field = |label: &'static str| div().w(px(110.)).flex_none().child(label);
            let (view, input) = (view.clone(), input.clone());
            let root_label = root.clone();
            dialog
                .title("Stash")
                .width(px(560.))
                .description(
                    v_flex()
                        .gap_3()
                        .child(h_flex().gap_2().child(field("Git Root:")).child(
                            div().flex_1().child(spork_workbench::controls::combo(
                                "stash-root",
                                root.clone(),
                                false,
                                move |menu, _, _| {
                                    menu.item(PopupMenuItem::new(root_label.clone()).checked(true))
                                },
                            )),
                        ))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(field("Current Branch:"))
                                .child(branch.clone()),
                        )
                        .child(
                            h_flex().justify_between().child("Message:").child(
                                spork_workbench::icon("general/history")
                                    .size(px(16.))
                                    .text_color(muted),
                            ),
                        )
                        .child(Textarea::new(&input).h(px(80.)))
                        .child(
                            h_flex()
                                .gap_4()
                                .child(check(
                                    "stash-keep-index",
                                    "Keep index",
                                    keep,
                                    |this, on| this.stash_keep = on,
                                    cx,
                                ))
                                .child(check(
                                    "stash-untracked",
                                    "Include untracked",
                                    untracked,
                                    |this, on| this.stash_untracked = on,
                                    cx,
                                )),
                        ),
                )
                .confirm()
                .ok_text("Create Stash")
                .on_ok(move |_, window, cx| {
                    let message = input.read(cx).value().to_string();
                    view.update(cx, |this, cx| {
                        let (keep, untracked) = (this.stash_keep, this.stash_untracked);
                        this.run_op(
                            window,
                            cx,
                            move |root| spork_git::stash::save(root, &message, keep, untracked),
                            |_, _, _, _| {},
                        )
                    });
                    true
                })
        });
    }

    /// Spork's snapshot: the tracked changes saved as a stash, left in place.
    fn snapshot_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input_dialog(
            "Stash Snapshot".into(),
            "Saves your changes to tracked files as a stash and leaves them in place. Message (optional):".into(),
            String::new(),
            |root, message| match spork_git::stash::snapshot(root, &message)? {
                true => Ok("Snapshot saved; your changes stay in place".to_owned()),
                false => Err(GitError::Other {
                    code: None,
                    stderr: "No changes to tracked files to save.".into(),
                }),
            },
            window,
            cx,
        )
    }

    // ---- the main area

    pub(crate) fn show_stash(&mut self, stash: Stash, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let (root, ignore_ws, id) = (
            repo.root.clone(),
            self.prefs.ignore_whitespace,
            stash.id.clone(),
        );
        // the commit pane shares the diff state: a selected commit would show the stash's diff
        self.selected = None;
        self.details = None;
        self.saved.shelf_shown = None;
        self.stash_shown = Some((stash, None));
        self.open_diff_view(View::Stash, cx);
        self._details_load = Some(cx.spawn(async move |this, cx| {
            let details = cx
                .background_spawn(async move {
                    // against the commit it was made on: what `stash apply` brings back
                    spork_git::commit_details(&root, &id, Some(&format!("{id}^1")), ignore_ws)
                })
                .await;
            this.update(cx, |this, cx| {
                let patch = match &details {
                    Ok(d) if d.patch.len() <= MAX_PATCH_BYTES => d.patch.as_str(),
                    _ => "",
                };
                let diff = this.diff.clone();
                this.set_patch(&diff, patch, cx);
                if this.whole_file {
                    diff.update(cx, |state, cx| state.expand_unchanged(cx));
                }
                if let Some((_, loaded)) = &mut this.stash_shown {
                    *loaded = Some(details.map(drop).map_err(|e| e.to_string()));
                }
                if let Some(path) = &this.saved.file {
                    diff.update(cx, |s, cx| s.scroll_to_file(path, cx));
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// The editor tab's title while the main area shows a shelved changelist.
    pub(crate) fn shelf_shown_title(&self) -> Option<SharedString> {
        let name = self.saved.shelf_shown.as_ref()?;
        let list = self.shelves().iter().find(|l| &l.name == name)?;
        Some(format!("Shelved: {}", list.description).into())
    }

    /// The diff of the stash or shelved changelist shown: its bar, its files and the diff.
    fn saved_diff(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .flex_1()
            .min_h_0()
            .size_full()
            .child(self.diff_bar("stash", &self.diff, cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.changed_files(cx))
                    .child(self.diff_view(&self.diff, cx)),
            )
    }

    /// The IDE's diff preview tab of a stash or shelved changelist: the diff alone, as IntelliJ's
    /// `SavedPatchesEditorDiffPreview` shows it.
    pub(crate) fn saved_diff_tab(&self, cx: &mut Context<Self>) -> Div {
        match &self.stash_shown {
            _ if self.saved.shelf_shown.is_some() => self.saved_diff(cx),
            Some((_, None)) => centered("Loading stash…", cx),
            Some((_, Some(Err(e)))) => centered(e.clone(), cx),
            Some((_, Some(Ok(())))) => self.saved_diff(cx),
            None => v_flex(),
        }
    }

    /// The diff preview tab's title, IntelliJ's `getDiffPreviewTitle`: `Stash@{0}: main.rs`,
    /// `Shelved: main.rs`, the file picked in the tab or the diff's first.
    pub(crate) fn saved_diff_title(&self, cx: &App) -> SharedString {
        let file = self
            .saved
            .file
            .clone()
            .or_else(|| {
                let diff = self.diff.read(cx);
                diff.files().first().map(|file| file.path().to_string())
            })
            .map(|path| path.rsplit('/').next().unwrap_or(&path).to_owned());
        let shown = match (&self.saved.shelf_shown, &self.stash_shown) {
            (Some(_), _) => Some(("Shelved".to_owned(), "Shelved Version")),
            (None, Some((stash, _))) => Some((format!("Stash@{{{}}}", stash.index), "Stash")),
            (None, None) => None,
        };
        match (shown, file) {
            (Some((tag, _)), Some(file)) => format!("{tag}: {file}").into(),
            (Some((_, empty)), None) => empty.into(),
            (None, _) => "Stashes and Shelves".into(),
        }
    }

    pub(crate) fn stash_view(&self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        if self.saved.shelf_shown.is_some() {
            return self.saved_diff(cx);
        }
        let Some((stash, loaded)) = &self.stash_shown else {
            return v_flex();
        };
        let index = stash.index;
        let apply =
            |id: &'static str, label: &'static str, pop: bool| {
                Button::new(id)
                    .small()
                    .label(label)
                    .disabled(self.busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.stash_apply(index, pop, window, cx)
                    }))
            };
        let header = h_flex()
            .flex_none()
            .px_4()
            .py_3()
            .gap_2()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(stash.message.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("stash@{{{index}}}")),
                    ),
            )
            .child(apply("stash-apply", "Apply", false))
            .child(apply("stash-pop", "Apply and remove", true))
            .child(
                Button::new("stash-drop")
                    .small()
                    .ghost()
                    .label("Drop…")
                    .disabled(self.busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.confirm_drop_stash(index, window, cx)
                    })),
            );
        let body = match loaded {
            None => centered("Loading stash…", cx),
            Some(Err(e)) => centered(e.clone(), cx),
            Some(Ok(())) => self.saved_diff(cx),
        };
        v_flex().size_full().child(header).child(body)
    }
}

/// A View Options item that flips one of the Shelf tab's options.
fn view_toggle(
    view: &Entity<RepoView>,
    label: &'static str,
    on: bool,
    flip: fn(&mut RepoView),
) -> PopupMenuItem {
    let view = view.clone();
    PopupMenuItem::new(label)
        .checked(on)
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                flip(this);
                cx.notify();
            })
        })
}

fn chevron(open: bool) -> Icon {
    spork_workbench::icon(if open {
        "general/chevronDown"
    } else {
        "general/chevronRight"
    })
    .size(px(16.))
}

type MenuBuilder = Box<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

/// A stash's menu (the reference app's Vcs.SavedPatches.ContextMenu on a stash).
fn stash_menu(view: Entity<RepoView>, stash: Stash) -> MenuBuilder {
    Box::new(move |menu, _, _| {
        let item =
            |label: &'static str,
             run: fn(&mut RepoView, Stash, &mut Window, &mut Context<RepoView>)| {
                let (view, stash) = (view.clone(), stash.clone());
                PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    view.update(cx, |this, cx| run(this, stash.clone(), window, cx))
                })
            };
        menu.item(item("Pop", |this, s, w, cx| {
            this.stash_apply(s.index, true, w, cx)
        }))
        .item(item("Apply", |this, s, w, cx| {
            this.stash_apply(s.index, false, w, cx)
        }))
        .item(item("Unstash…", |this, s, w, cx| {
            this.unstash_dialog(s, w, cx)
        }))
        .menu("Drop", Box::new(DeleteSaved))
        .item(item("Clear", |this, _, w, cx| this.confirm_clear(w, cx)))
        .separator()
        .menu("Show Diff", Box::new(ShowSavedDiff))
        .item(item(
            "Show Diff in a New Tab or Window",
            |this, s, _, cx| this.show_stash(s, cx),
        ))
        .item(item("Compare with Local", |this, s, w, cx| {
            this.run_op(
                w,
                cx,
                move |root| {
                    spork_git::run(root, &["diff", &s.id])
                        .map(|out| (s, String::from_utf8_lossy(&out).into_owned()))
                },
                |this, (stash, patch), _, cx| {
                    this.selected = None;
                    this.details = None;
                    this.saved.shelf_shown = None;
                    this.stash_shown = Some((stash, Some(Ok(()))));
                    this.open_diff_view(View::Stash, cx);
                    let diff = this.diff.clone();
                    this.set_patch(&diff, &patch, cx);
                    cx.notify();
                },
            )
        }))
    })
}

/// A stashed file's menu (the reference app's Vcs.SavedPatches.ChangesBrowser.ContextMenu on a stash).
fn stash_file_menu(view: Entity<RepoView>, stash: Stash, path: String) -> MenuBuilder {
    Box::new(move |menu, _, _| {
        let item = |label: &'static str, run: fn(&mut RepoView, Stash, String, &mut Window, &mut Context<RepoView>)| {
            let (view, stash, path) = (view.clone(), stash.clone(), path.clone());
            PopupMenuItem::new(label).on_click(move |_, window, cx| {
                view.update(cx, |this, cx| run(this, stash.clone(), path.clone(), window, cx))
            })
        };
        menu.item(item("Compare with Local", |this, s, p, w, cx| {
            this.compare_stash_file(s, p, false, w, cx)
        }))
        .item(item("Compare Before with Local", |this, s, p, w, cx| {
            this.compare_stash_file(s, p, true, w, cx)
        }))
        .item(item("Edit Source", |this, _, p, w, cx| {
            let file = this.tab_root().map(|root| root.join(&p));
            this.open_in_editor(file, w, cx)
        }))
        .item(item("Open Repository Version", |_, _, _, w, cx| {
            window_not_yet("Open Repository Version", w, cx)
        }))
        .item(item("Cherry-Pick Selected Changes", |this, s, p, w, cx| {
            this.run_op(
                w,
                cx,
                move |root| spork_git::stash::apply_file(root, &s.id, &p),
                |_, _, _, _| {},
            )
        }))
    })
}

fn window_not_yet(label: &str, window: &mut Window, cx: &mut App) {
    window.push_notification(
        Notification::info(format!("{label} isn't in Spork yet.")),
        cx,
    )
}

/// The Shelf tab's menu (the reference app's ShelvedChangesPopupMenu): on a changelist, one of its files,
/// or the empty tree, where only Import Patches… applies.
fn shelf_menu(view: Entity<RepoView>, list: Option<Shelved>, file: Option<String>) -> MenuBuilder {
    Box::new(move |menu, _, _| {
        let none = list.is_none();
        let item =
            |label: &'static str,
             disabled: bool,
             run: fn(&mut RepoView, Shelved, &mut Window, &mut Context<RepoView>)| {
                let (view, list) = (view.clone(), list.clone());
                PopupMenuItem::new(label)
                    .disabled(disabled)
                    .on_click(move |_, window, cx| {
                        if let Some(list) = list.clone() {
                            view.update(cx, |this, cx| run(this, list, window, cx))
                        }
                    })
            };
        let restorable = list.as_ref().is_some_and(|l| l.recycled || l.deleted);
        let import_view = view.clone();
        let mut menu = menu
            .menu_with_disabled("Unshelve…", Box::new(Unshelve), none)
            .item(item("Restore", !restorable, |this, l, w, cx| {
                this.restore_shelf(l, w, cx)
            }))
            .menu_with_disabled("Show Diff", Box::new(ShowSavedDiff), none);
        // these act on a file: the reference app hides them elsewhere
        if file.is_some() {
            menu = menu
                // The reference app's Diff.ShowStandaloneDiff; its frontend menu's "Show Diff in a New
                // Window" doesn't show in the reference app
                .item(item(
                    "Show Diff in a New Tab or Window",
                    false,
                    |this, l, w, cx| this.show_shelf(l, w, cx),
                ))
                .item(item("Compare with Local", false, |this, l, w, cx| {
                    this.compare_shelf_file(l, w, cx)
                }));
        }
        menu.item(item("Create Patch…", none, |this, l, w, cx| {
            this.save_shelf_patch(l, w, cx)
        }))
        .item(item(
            "Copy as Patch to Clipboard",
            none,
            |this, l, w, cx| this.copy_shelf_patch(l, w, cx),
        ))
        .item(
            PopupMenuItem::new("Import Patches…").on_click(move |_, window, cx| {
                import_view.update(cx, |this, cx| this.import_patches(window, cx))
            }),
        )
        .separator()
        .menu_with_disabled("Rename…", Box::new(RenameShelf), none)
        .menu_with_disabled("Delete…", Box::new(DeleteSaved), none)
    })
}

/// A shelved changelist's menu in the combined tab: the reference app's Vcs.SavedPatches.ContextMenu, its
/// Vcs.Shelf.Operations.ContextMenu first.
fn saved_shelf_menu(view: Entity<RepoView>, list: Shelved) -> MenuBuilder {
    Box::new(move |menu, _, _| {
        let item =
            |label: &'static str,
             run: fn(&mut RepoView, Shelved, &mut Window, &mut Context<RepoView>)| {
                let (view, list) = (view.clone(), list.clone());
                PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    view.update(cx, |this, cx| run(this, list.clone(), window, cx))
                })
            };
        menu.item(item("Apply", |this, l, w, cx| {
            this.unshelve(l, false, w, cx)
        }))
        .item(item("Pop", |this, l, w, cx| this.unshelve(l, true, w, cx)))
        .menu("Unshelve…", Box::new(Unshelve))
        .menu("Delete", Box::new(DeleteSaved))
        // the tab lists neither applied nor deleted changelists
        .item(
            item("Restore", |this, l, w, cx| this.restore_shelf(l, w, cx))
                .disabled(!list.recycled && !list.deleted),
        )
        .item(item("Import Patches…", |this, _, w, cx| {
            this.import_patches(w, cx)
        }))
        .separator()
        .menu("Show Diff", Box::new(ShowSavedDiff))
        .item(item(
            "Show Diff in a New Tab or Window",
            |this, l, w, cx| this.show_shelf(l, w, cx),
        ))
        .item(item("Compare with Local", |this, l, w, cx| {
            this.compare_shelf_file(l, w, cx)
        }))
    })
}

/// A shelved file's menu in the combined tab: the reference app's Vcs.SavedPatches.ChangesBrowser.ContextMenu,
/// with Vcs.Shelf.ChangesBrowser.ContextMenu after Compare with Local.
fn saved_shelf_file_menu(view: Entity<RepoView>, list: Shelved, path: String) -> MenuBuilder {
    Box::new(move |menu, _, _| {
        let item = |label: &'static str, remove: Option<bool>| {
            let (view, list, path) = (view.clone(), list.clone(), path.clone());
            PopupMenuItem::new(label).on_click(move |_, window, cx| {
                let (list, path) = (list.clone(), path.clone());
                view.update(cx, |this, cx| match remove {
                    Some(remove) => this.shelf_unshelve_files(list, vec![path], remove, window, cx),
                    None => this.compare_shelf_file(list, window, cx),
                })
            })
        };
        menu.item(item("Compare with Local", None))
            .item(item("Unshelve", Some(false)))
            .item(item("Unshelve and Remove", Some(true)))
    })
}
