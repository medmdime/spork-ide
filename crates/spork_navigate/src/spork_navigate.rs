//! IntelliJ's search and navigation popups in the IDE's windows (ROADMAP, phase 4): Search
//! Everywhere with its tabs (⇧⇧, and Go to Class ⌘O, Go to File ⇧⌘O, Go to Symbol ⌥⌘O, Find
//! Action ⇧⌘A, Text ⌥⇧⌘E on their tab), Recent Files (⌘E), the Switcher (⌃⇥), Recent Locations
//! (⇧⌘E), File Structure (⌘F12), and Last and Next Edit Location (⇧⌘⌫); bookmarks (F3, ⌥F3, ⌘F3,
//! ⌥⌘F3, ⌃digit, the Bookmarks tool window). Built from Spork's workbench, after the standalone
//! app's Search Everywhere.

pub mod bookmarks;
mod file_structure;
mod find_in_files;
pub mod find_panel;
mod go_to;
pub mod list_popup;
pub mod navigation_bar;
mod recent_locations;
mod search_everywhere;
mod select_in;
mod switcher;

pub use find_in_files::{Occurrence, find_occurrences};
pub use find_panel::{Usages, show_usages};
pub use navigation_bar::NavigationBar;

use editor::{Editor, EditorEvent, SelectionEffects, scroll::Autoscroll};
use gpui::{Action, App, Context, Entity, Global, SharedString, TaskExt as _, Window};
use language::Point;
use project::ProjectPath;
use workspace::Workspace;
use workspace::dock::Dock;

pub mod navigate {
    use gpui::Action;
    use schemars::JsonSchema;
    use serde::Deserialize;

    gpui::actions!(
        navigate,
        [
            /// Opens Search Everywhere on its All tab.
            SearchEverywhere,
            /// Opens Search Everywhere on its Classes tab.
            GoToClass,
            /// Opens Search Everywhere on its Files tab.
            GoToFile,
            /// Opens Search Everywhere on its Symbols tab.
            GoToSymbol,
            /// Opens Search Everywhere on its Text tab.
            GoToText,
            /// Opens Search Everywhere on its Actions tab.
            FindAction,
            /// Shows the files opened lately and the tool windows.
            RecentFiles,
            /// Shows the files edited lately and the tool windows.
            RecentlyChangedFiles,
            /// Shows the places visited lately, with their code.
            RecentLocations,
            /// Shows the outline of the active editor's file.
            FileStructure,
            /// Goes back to where the last edit was made, in any file.
            LastEditLocation,
            /// Goes forward through the edit locations Last Edit Location went back from.
            NextEditLocation,
            /// Search Everywhere: shows the next tab.
            NextTab,
            /// Search Everywhere: shows the tab before.
            PreviousTab,
            /// Search Everywhere: selects the last result (Page Down, ⌘↓), IntelliJ's Next Group
            /// Element on its mixed results.
            NextGroup,
            /// Search Everywhere: selects the first result (Page Up, ⌘↑).
            PreviousGroup,
            /// Search Everywhere: completes the typed `/` command (⇥), as `/files `.
            CompleteCommand,
            /// Shows the mnemonic chooser for the caret's line: adds, changes or assigns its
            /// bookmark's mnemonic.
            ToggleBookmarkMnemonic,
            /// Shows the Bookmarks popup.
            ShowBookmarks,
            /// Shows the bookmarks with a mnemonic; typing one goes to it.
            GoToMnemonic,
            /// Edits the description of the caret line's bookmark.
            RenameBookmark,
            /// Takes the mnemonic off the caret line's bookmark.
            RemoveMnemonic,
            /// Opens the Find in Files popup.
            FindInFiles,
            /// Opens the Replace in Files popup.
            ReplaceInFiles,
            /// Opens the Go to Line:Column dialog for the active editor.
            GoToLine,
            /// Shows where the active file can be shown: the Project tool window, Bookmarks…
            SelectIn,
            /// Shows the active file's path, folder by folder; choosing one reveals it in the
            /// system's file manager.
            ShowFilePath,
            /// Goes to the next occurrence in the Find tool window.
            NextOccurrence,
            /// Goes to the previous occurrence in the Find tool window.
            PreviousOccurrence,
            /// Focuses the navigation bar on the active file's folder and lists what it holds.
            JumpToNavigationBar,
            /// Recent Files and the Switcher: closes the selected files, or takes them off the
            /// list, or closes the selected tool windows.
            DeleteRecentFiles,
            /// Shows the scratch files, to open one (IntelliJ's Show Scratch Files).
            ShowScratchFiles,
            /// Recent Files: selects the next file with errors.
            SwitcherNextProblem,
            /// Recent Files: selects the previous file with errors.
            SwitcherPreviousProblem,
        ]
    );

    /// Goes to the bookmark with this mnemonic (IntelliJ's ⌃digit).
    #[derive(PartialEq, Clone, Deserialize, JsonSchema, Default, Action)]
    #[action(namespace = navigate)]
    #[serde(deny_unknown_fields)]
    pub struct GoToBookmark {
        pub mnemonic: char,
    }

    /// Bookmarks the caret's line with this mnemonic, or takes its bookmark off when it has it
    /// (IntelliJ's ⌃⇧digit).
    #[derive(PartialEq, Clone, Deserialize, JsonSchema, Default, Action)]
    #[action(namespace = navigate)]
    #[serde(deny_unknown_fields)]
    pub struct ToggleMnemonic {
        pub mnemonic: char,
    }

    /// Opens the Switcher: the open files and the tool windows, chosen while ⌃ stays down.
    #[derive(PartialEq, Clone, Deserialize, JsonSchema, Default, Action)]
    #[action(namespace = navigate)]
    #[serde(deny_unknown_fields)]
    pub struct Switcher {
        /// Selects the last file instead of the one before the active one.
        #[serde(default)]
        pub select_last: bool,
    }
}

/// IntelliJ's status bar message (`StatusBar.setInfo`), the navigation bar shows it after the
/// path: "Occurrence 2 of 5".
#[derive(Default)]
pub(crate) struct StatusInfo(pub(crate) Option<SharedString>);

impl gpui::Global for StatusInfo {}

pub(crate) fn set_status_info(text: impl Into<SharedString>, cx: &mut App) {
    cx.set_global(StatusInfo(Some(text.into())));
}

/// Registers the popups' actions on every workspace and starts noting edit locations.
pub fn init(cx: &mut App) {
    cx.set_global(EditLocations::default());
    cx.set_global(StatusInfo::default());
    cx.set_global(editor::BookmarkGutterIcon(std::rc::Rc::new(
        bookmarks::gutter_icon,
    )));
    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        if let Some(window) = window {
            bookmarks::add_panel(workspace, window, cx);
        }
        bookmarks::register(workspace);
        find_in_files::register(workspace);
        find_panel::register(workspace);
        go_to::register(workspace);
        workspace.register_action(|_, _: &navigate::JumpToNavigationBar, window, cx| {
            after_update(window, cx, |workspace, window, cx| {
                navigation_bar::jump(&workspace, true, window, cx)
            });
        });
        select_in::register(workspace);
        search_everywhere::register(workspace);
        switcher::register(workspace);
        recent_locations::register(workspace);
        file_structure::register(workspace);
        workspace::frame::register_stretch_actions(workspace);
        workspace.register_action(|workspace, _: &navigate::LastEditLocation, window, cx| {
            step_through_edits(workspace, true, window, cx)
        });
        workspace.register_action(|workspace, _: &navigate::NextEditLocation, window, cx| {
            step_through_edits(workspace, false, window, cx)
        });
        // IntelliJ's Back and Forward go through the editors' history wherever the focus is: a
        // pane handles them itself, this from a tool window
        workspace.register_action(|workspace, _: &workspace::GoBack, window, cx| {
            let pane = workspace.active_pane().downgrade();
            workspace.go_back(pane, window, cx).detach_and_log_err(cx);
        });
        workspace.register_action(|workspace, _: &workspace::GoForward, window, cx| {
            let pane = workspace.active_pane().downgrade();
            workspace
                .go_forward(pane, window, cx)
                .detach_and_log_err(cx);
        });
    })
    .detach();
    cx.observe_new(|editor: &mut Editor, _, cx| {
        if !editor.mode().is_full() {
            return;
        }
        cx.subscribe_self(|editor: &mut Editor, event: &EditorEvent, cx| {
            if let EditorEvent::Edited { .. } = event {
                note_edit(editor, cx);
            }
        })
        .detach();
    })
    .detach();
}

/// A place in a file: an edit location, or one the navigation history kept.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Place {
    pub(crate) path: ProjectPath,
    pub(crate) point: Point,
}

/// IntelliJ's changed places (`IdeDocumentHistory`): where edits were made, the last one last,
/// one per line; and how far Last Edit Location went back through them.
#[derive(Default)]
struct EditLocations {
    places: Vec<Place>,
    stepped_back_to: Option<usize>,
}

impl Global for EditLocations {}

/// How many edit locations are kept, as IntelliJ's `ide.max.recent.changed.places` default.
const MAX_EDIT_LOCATIONS: usize = 50;

/// The file an editor shows, when it shows one.
fn editor_path(editor: &Editor, cx: &App) -> Option<ProjectPath> {
    let buffer = editor.buffer().read(cx).as_singleton()?;
    let file = buffer.read(cx).file()?;
    Some(ProjectPath {
        worktree_id: file.worktree_id(cx),
        path: file.path().clone(),
    })
}

fn note_edit(editor: &Editor, cx: &mut Context<Editor>) {
    let Some(path) = editor_path(editor, cx) else {
        return;
    };
    let point = editor
        .selections
        .newest::<Point>(&editor.display_snapshot(cx))
        .head();
    let edits = cx.default_global::<EditLocations>();
    edits.stepped_back_to = None;
    // an edit next to the last one moves that location rather than adding one
    if let Some(last) = edits.places.last_mut()
        && last.path == path
        && last.point.row.abs_diff(point.row) <= 1
    {
        last.point = point;
        return;
    }
    edits
        .places
        .retain(|place| place.path != path || place.point.row != point.row);
    edits.places.push(Place { path, point });
    if edits.places.len() > MAX_EDIT_LOCATIONS {
        edits.places.remove(0);
    }
}

/// The edit locations in `workspace`'s project, the last one last.
pub(crate) fn edit_locations(workspace: &Workspace, cx: &App) -> Vec<Place> {
    let project = workspace.project().read(cx);
    cx.try_global::<EditLocations>()
        .map(|edits| {
            edits
                .places
                .iter()
                .filter(|place| {
                    project
                        .worktree_for_id(place.path.worktree_id, cx)
                        .is_some()
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Last Edit Location (`back`) and Next Edit Location: the edit location before (or after) the
/// one gone to last, skipping the one the caret is on.
fn step_through_edits(
    workspace: &mut Workspace,
    back: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let places = edit_locations(workspace, cx);
    if places.is_empty() {
        return;
    }
    let here = current_place(workspace, cx);
    let is_here = |place: &Place| {
        here.as_ref()
            .is_some_and(|here| here.path == place.path && here.point.row == place.point.row)
    };
    let step = |ix: usize| match back {
        true => ix.checked_sub(1),
        false => Some(ix + 1),
    };
    let edits = cx.default_global::<EditLocations>();
    let mut ix = match edits.stepped_back_to {
        Some(ix) => step(ix),
        None if back => Some(places.len() - 1),
        None => None,
    };
    if let Some(at) = ix
        && places.get(at).is_some_and(is_here)
    {
        ix = step(at);
    }
    let Some(place) = ix.and_then(|ix| places.get(ix)).cloned() else {
        return;
    };
    let ix = ix.unwrap_or_default();
    edits.stepped_back_to = (ix + 1 < places.len()).then_some(ix);
    open_at(workspace, place.path, Some(place.point), window, cx);
}

/// Where the active editor's caret is.
pub(crate) fn current_place(workspace: &Workspace, cx: &mut App) -> Option<Place> {
    let editor = workspace.active_item_as::<Editor>(cx)?;
    editor.update(cx, |editor, cx| {
        let path = editor_path(editor, cx)?;
        let point = editor
            .selections
            .newest::<Point>(&editor.display_snapshot(cx))
            .head();
        Some(Place { path, point })
    })
}

/// Opens `path` in the active pane, its caret at `point`.
pub fn open_at(
    workspace: &mut Workspace,
    path: ProjectPath,
    point: Option<Point>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let open = workspace.open_path(path, None, true, window, cx);
    cx.spawn_in(window, async move |_, cx| {
        let item = open.await?;
        if let Some(point) = point
            && let Some(editor) = item.downcast::<Editor>()
        {
            editor.update_in(cx, |editor, window, cx| {
                editor.change_selections(
                    SelectionEffects::scroll(Autoscroll::center()),
                    window,
                    cx,
                    |selections| selections.select_ranges([point..point]),
                );
            })?;
        }
        anyhow::Ok(())
    })
    .detach_and_log_err(cx);
}

/// An action's shortcut as IntelliJ writes it on macOS (`⇧⌘O`, `⇧ ⇧`), or on other systems
/// (`Ctrl+Shift+N`).
pub(crate) fn shortcut_text(action: &dyn Action, window: &Window) -> Option<SharedString> {
    let binding = window.highest_precedence_binding_for_action(action)?;
    Some(spork_workbench::popup::keystrokes_text(binding.keystrokes()).into())
}

/// The workspace's tool windows: each dock's panels, with IntelliJ's titles and icons. Not while
/// the workspace is being updated: panels read it to describe themselves.
pub(crate) fn tool_windows(
    workspace: &Entity<Workspace>,
    window: &Window,
    cx: &App,
) -> Vec<ToolWindow> {
    let docks: Vec<Entity<Dock>> = workspace
        .read(cx)
        .all_docks()
        .into_iter()
        .cloned()
        .collect();
    let mut tool_windows: Vec<ToolWindow> = docks
        .iter()
        .flat_map(|dock| {
            let dock = dock.read(cx);
            let open = dock
                .is_open()
                .then(|| dock.visible_panel().map(|panel| panel.panel_id()))
                .flatten();
            dock.panels()
                .filter(|panel| panel.enabled(cx))
                .map(|panel| ToolWindow {
                    title: workspace::frame::tool_window_title(panel.as_ref(), window, cx),
                    icon: workspace::frame::tool_window_icon(panel.as_ref()),
                    zed_icon: panel.icon(window, cx),
                    shortcut: shortcut_text(panel.toggle_action(window, cx).as_ref(), window),
                    toggle_action: panel.toggle_action(window, cx),
                    position: panel.position(window, cx),
                    panel_id: panel.panel_id(),
                    open: open == Some(panel.panel_id()),
                })
                .collect::<Vec<_>>()
        })
        .collect();
    tool_windows.sort_by(|a, b| a.title.cmp(&b.title));
    tool_windows
}

/// Shows the tool window of the panel named `name` (its persistent name) and focuses it, as
/// choosing it in IntelliJ's Switcher does; false when the workspace has no such panel.
pub(crate) fn show_tool_window(
    workspace: &mut Workspace,
    name: &str,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> bool {
    let found = workspace.all_docks().into_iter().find_map(|dock| {
        let dock = dock.read(cx);
        let panel = dock
            .panels()
            .find(|panel| panel.persistent_name() == name)?;
        let open =
            dock.is_open() && dock.visible_panel().map(|p| p.panel_id()) == Some(panel.panel_id());
        Some((dock.position(), panel.panel_id(), open))
    });
    let Some((position, panel_id, open)) = found else {
        return false;
    };
    ToolWindow::activate_panel(position, panel_id, open, workspace, window, cx);
    true
}

/// Runs `f` once the workspace's update is over: what reads panels can't run inside it.
pub(crate) fn after_update(
    window: &mut Window,
    cx: &mut Context<Workspace>,
    f: impl FnOnce(Entity<Workspace>, &mut Window, &mut App) + 'static,
) {
    let workspace = cx.entity();
    window.defer(cx, move |window, cx| f(workspace, window, cx));
}

pub(crate) struct ToolWindow {
    pub(crate) title: SharedString,
    /// IntelliJ's icon, or Zed's for a panel IntelliJ has none for.
    pub(crate) icon: Option<&'static str>,
    pub(crate) zed_icon: Option<ui::IconName>,
    pub(crate) shortcut: Option<SharedString>,
    pub(crate) toggle_action: Box<dyn Action>,
    pub(crate) position: workspace::dock::DockPosition,
    pub(crate) panel_id: gpui::EntityId,
    pub(crate) open: bool,
}

impl ToolWindow {
    /// Shows the tool window and gives it the focus, as choosing it in IntelliJ's Switcher does.
    pub(crate) fn activate_panel(
        position: workspace::dock::DockPosition,
        panel_id: gpui::EntityId,
        open: bool,
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        if open {
            let dock = workspace.dock_at_position(position).clone();
            if let Some(panel) = dock.read(cx).visible_panel().cloned() {
                panel.activation_focus_handle(cx).focus(window, cx);
            }
            return;
        }
        workspace.toggle_tool_window(position, panel_id, window, cx);
    }

    pub(crate) fn icon_element(&self, cx: &App) -> gpui::AnyElement {
        use gpui::{IntoElement as _, Styled as _};
        let tokens = spork_workbench::Tokens::get(cx);
        match (self.icon, self.zed_icon) {
            (Some(icon), _) => spork_workbench::icon(icon)
                .size(gpui::px(16.))
                .text_color(tokens.icon)
                .into_any_element(),
            (None, Some(icon)) => ui::Icon::new(icon)
                .color(ui::Color::Custom(tokens.icon))
                .into_any_element(),
            (None, None) => gpui::div().size(gpui::px(16.)).into_any_element(),
        }
    }
}

/// The files recently opened in `workspace`, the latest first.
pub(crate) fn recent_files(workspace: &Workspace, cx: &App) -> Vec<ProjectPath> {
    workspace
        .recent_navigation_history_iter(cx)
        .map(|(path, _)| path)
        .collect()
}

/// A file's name colour for its version control status, IntelliJ's `FileStatus` colour as the
/// Project tool window and the editor tabs draw it (modified, added, unversioned, ignored…); none
/// for a file without a status.
pub(crate) fn file_status_color(
    project: &Entity<project::Project>,
    path: &ProjectPath,
    cx: &App,
) -> Option<gpui::Hsla> {
    let project = project.read(cx);
    let entry = project.entry_for_path(path, cx)?;
    let status = project
        .git_store()
        .read(cx)
        .display_status_for_project_path(path, cx)
        .map(|status| status.summary())
        .unwrap_or_default();
    let color = editor::items::entry_git_aware_label_color(status, entry.is_ignored, true);
    (color != ui::Color::Default).then(|| color.color(cx))
}
