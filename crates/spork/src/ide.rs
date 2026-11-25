//! Spork's version control inside the IDE's windows (ROADMAP, phase 2): what its Git and Commit
//! tool windows show and its Git menu, set up without the standalone app's windows, menu bar,
//! shortcuts or test channel. The IDE's crate `spork_vcs` puts them in its docks.

use std::path::PathBuf;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use spork_workbench::shell::island;

use crate::shell::GitMenu;
use crate::{RepoView, View};

/// A tool window the views ask the IDE to show or hide, where the standalone app shows its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolWindow {
    Git,
    Commit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolWindowEvent {
    Show(ToolWindow),
    Hide(ToolWindow),
}

impl EventEmitter<ToolWindowEvent> for RepoView {}

/// A diff the IDE shows in an editor tab: IntelliJ's diff preview of a tool window, one tab each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffTab {
    /// The Commit tool window's: a local change (`Commit: <file>`).
    Commit,
    /// The Git tool window's: a file of the commit selected in the log (`Repository Diff: <file>`).
    Repository,
    /// The Commit tool window's Stash and Shelf tabs': a stash or a shelved changelist
    /// (`Stash@{0}: <file>`, `Shelved: <file>`).
    Saved,
}

impl DiffTab {
    fn view(self) -> View {
        match self {
            Self::Commit => View::Changes,
            Self::Repository => View::CommitDiff,
            Self::Saved => View::Stash,
        }
    }
}

/// What the views ask the IDE to open in its editor area.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdeEvent {
    ShowDiff(DiffTab),
    /// Open Repository Version: `path` (in the repository) as of `revision`, read-only.
    OpenRevision {
        path: String,
        revision: String,
        text: String,
    },
    /// Open Worktree: the folder as a project, in a window of its own.
    OpenProject(PathBuf),
    /// Jump to Source and Show Source on Double-Click: a file of the working copy in an editor
    /// tab, at its one-based `line` when given.
    OpenFile {
        path: PathBuf,
        line: Option<usize>,
    },
}

impl EventEmitter<IdeEvent> for RepoView {}

/// Sets up gpui-kit (its theme, its controls' keys), Spork's look (its themes, fonts and icons)
/// and its views' keys, with Spork's files kept in `config_dir`. Answers the key bindings this
/// added, for the IDE to bind again whenever it reloads its keymap. Those of no key context are
/// left out: such a binding outranks every context's binding of its key, so it would take keys
/// from the IDE's editor and panels.
// shortcut: that leaves out gpui-kit's inspector and the diff viewer's file and side switching
// (⌃⇧←/→, ⌃⌘←/→); bind those in the diff's key context once diffs open in the IDE's editor tabs
pub fn init(config_dir: PathBuf, cx: &mut App) -> Vec<KeyBinding> {
    cx.set_global(InIde);
    crate::repos::set_config_dir(config_dir);
    let before = cx.key_bindings().borrow().bindings().len();
    gpui_kit::init(cx);
    spork_workbench::install(cx);
    crate::shell::bind_view_keys(cx);
    crate::changes::install(cx);
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    keymap
        .bindings()
        .skip(before)
        .filter(|binding| binding.predicate().is_some())
        .cloned()
        .collect()
}

/// Set when Spork's views run inside the IDE, so their texts name the app they're in.
struct InIde;

impl gpui::Global for InIde {}

/// The app Spork's views are shown in, as their texts name it where IntelliJ's name its own.
pub(crate) fn app_name(cx: &App) -> &'static str {
    if cx.has_global::<InIde>() {
        "Spork IDE"
    } else {
        "Spork"
    }
}

/// Starts answering git's password and passphrase prompts in the app, for every git command
/// started after this call: each prompt, with where its answer goes (`None` cancels).
pub fn start_askpass() -> Option<smol::channel::Receiver<crate::askpass::Prompt>> {
    crate::askpass::start()
}

/// What the Git menu shows of a repository besides its fixed items: an operation under way
/// (Continue Rebase, Abort Merge…), conflicts (Resolve Conflicts…), a shallow clone, stashes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GitMenuState(GitMenu);

/// The reference app's Git menu, the one the standalone app shows, for the IDE's menu bar.
pub fn git_menu(state: GitMenuState) -> Menu {
    crate::shell::git_menu(state.0)
}

impl RepoView {
    /// The repository at or above `path`, for the IDE's tool windows.
    // shortcut: no window answers git's password prompts in the IDE; route them to it as the
    // standalone app does (askpass.rs) once remote operations run there
    pub fn open(path: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self::new(path, None, window, cx);
        view.ide = true;
        view
    }

    /// Shows a diff: in the IDE in its editor tab, elsewhere in the main area.
    pub(crate) fn open_diff_view(&mut self, view: View, cx: &mut Context<Self>) {
        self.view = view;
        let tab = match view {
            View::Changes => DiffTab::Commit,
            View::CommitDiff => DiffTab::Repository,
            View::Stash => DiffTab::Saved,
            _ => return,
        };
        if self.ide {
            cx.emit(IdeEvent::ShowDiff(tab));
        }
    }

    /// A diff tab's content: the diff bar and the diff, as the main area shows them.
    pub fn diff_tab(
        &mut self,
        tab: DiffTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.note_focus(window, cx);
        // the diff's bar and tools read the view, which is the tab's while it draws
        let shown = std::mem::replace(&mut self.view, tab.view());
        let content = match tab {
            DiffTab::Commit => self.change_diff_view(cx),
            DiffTab::Repository if self.selected.is_some() => {
                let diff = match self.commit_files.is_some() {
                    true => self.commit_files_view(cx),
                    false => self.diff_body(cx),
                };
                v_flex()
                    .size_full()
                    .child(self.details_tabs(cx))
                    .child(diff)
            }
            DiffTab::Repository => v_flex(),
            // the log's selected commit and a stash share the diff: the one picked last shows
            DiffTab::Saved if self.selected.is_none() => self.saved_diff_tab(cx),
            DiffTab::Saved => v_flex(),
        };
        self.view = shown;
        island(cx).size_full().child(content).into_any_element()
    }

    /// A diff tab's title, IntelliJ's: `Commit: main.rs`, `Repository Diff: main.rs`,
    /// `Stash@{0}: main.rs`, `Shelved: main.rs`.
    pub fn diff_tab_title(&self, tab: DiffTab, cx: &App) -> SharedString {
        let path = match tab {
            DiffTab::Commit => self.change_selected.as_ref().map(|change| &change.path),
            DiffTab::Repository => self.commit_file_selected.as_ref(),
            DiffTab::Saved => None,
        };
        let name = path.map(|path| path.rsplit('/').next().unwrap_or(path));
        match (tab, name) {
            (DiffTab::Commit, Some(name)) => format!("Commit: {name}").into(),
            (DiffTab::Commit, None) => "Commit".into(),
            (DiffTab::Repository, Some(name)) => format!("Repository Diff: {name}").into(),
            (DiffTab::Repository, None) => "Repository Diff".into(),
            (DiffTab::Saved, _) => self.saved_diff_title(cx),
        }
    }

    /// The diff a diff tab draws: the tab gives it the focus, so the diff's keys (F7, ⇧F7)
    /// work as soon as it opens, as IntelliJ's diff editor does.
    pub fn diff_tab_focus(&self, tab: DiffTab, cx: &App) -> FocusHandle {
        match tab {
            DiffTab::Commit => self.change_diff.focus_handle(cx),
            DiffTab::Repository | DiffTab::Saved => self.diff.focus_handle(cx),
        }
    }

    /// The line at the top of a diff tab's diff (`src/main.rs:12`), for the end-to-end tests.
    pub fn diff_tab_top_line(&self, tab: DiffTab, cx: &App) -> Option<String> {
        let state = match tab {
            DiffTab::Commit => &self.change_diff,
            DiffTab::Repository | DiffTab::Saved => &self.diff,
        };
        let top = state.read(cx).top_line()?;
        Some(format!("{}:{}", top.path(), top.line()))
    }

    /// Open Worktree and Create and Open Worktree: in the IDE a project window of its own,
    /// elsewhere a window of Spork's.
    pub(crate) fn open_worktree(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match self.ide {
            true => cx.emit(IdeEvent::OpenProject(path)),
            false => {
                crate::open_repo_window(vec![path], None, cx);
            }
        }
    }

    /// The diff of the file at `path` against HEAD in the Commit diff tab, as Show Diff does for
    /// a local change.
    pub fn show_local_change(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let Some(path) = path
            .strip_prefix(&repo.root)
            .ok()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
        else {
            return;
        };
        if !repo.status.iter().any(|entry| entry.path == path) {
            return;
        }
        self.select_change(
            Some(crate::changes::ChangeSelection {
                path,
                staged: false,
            }),
            cx,
        );
        self.open_diff_view(View::Changes, cx);
        cx.notify();
    }

    /// The repository path of the file at `path` when it has a local change (the Commit tool
    /// window lists it).
    fn changed_path(&self, path: &std::path::Path) -> Option<String> {
        let Some(Ok(repo)) = &self.repo else {
            return None;
        };
        let path = path
            .strip_prefix(&repo.root)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/");
        repo.status
            .iter()
            .any(|entry| entry.path == path)
            .then_some(path)
    }

    /// Whether the file at `path` has a local change: IntelliJ's Select In ▸ Commit offers it.
    pub fn has_local_change(&self, path: &std::path::Path) -> bool {
        self.changed_path(path).is_some()
    }

    /// Selects the file at `path` among the local changes, as IntelliJ's Select In ▸ Commit does;
    /// false when it has none.
    pub fn select_local_change(&mut self, path: &std::path::Path, cx: &mut Context<Self>) -> bool {
        let Some(path) = self.changed_path(path) else {
            return false;
        };
        let staged = match &self.repo {
            Some(Ok(repo)) => !repo
                .status
                .iter()
                .any(|entry| entry.path == path && entry.has_unstaged()),
            _ => false,
        };
        self.select_change(Some(crate::changes::ChangeSelection { path, staged }), cx);
        cx.notify();
        true
    }

    /// The diff of the file at `path` in commit `id`, in the Repository Diff tab (an annotated
    /// line's Show Diff): the commit selected in the log, its diff at the file.
    pub fn show_commit_diff_at(
        &mut self,
        id: &str,
        path: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(repo)) = &self.repo else { return };
        let Some(path) = path
            .strip_prefix(&repo.root)
            .ok()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
        else {
            return;
        };
        self.go_to_commit(id, cx);
        self.open_commit_diff(Some(path), cx);
    }

    /// Open Repository Version in the IDE: the file's text for a read-only editor tab.
    pub(crate) fn open_ide_revision(
        &mut self,
        id: String,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let spec = format!("{id}:{path}");
        self.run_op(
            window,
            cx,
            move |root| spork_git::file::blob(root, &spec),
            move |_, bytes, _, cx| {
                cx.emit(IdeEvent::OpenRevision {
                    path,
                    revision: id,
                    text: String::from_utf8_lossy(&bytes).into_owned(),
                })
            },
        );
    }

    /// The Git tool window: the log, its branches pane, graph and details under the Log tab, or
    /// what took the tab's place (a diff, a file's history, a stash).
    pub fn git_tool_window(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.note_focus(window, cx);
        // diffs show in editor tabs: the tool window keeps the log under them
        let shown = self.view;
        if matches!(shown, View::Changes | View::CommitDiff | View::Stash) {
            self.view = View::History;
        }
        let panel = self.main_panel(cx);
        self.view = shown;
        h_flex()
            .size_full()
            .child(panel)
            .children(self.search_everywhere_overlay(window))
            .children(self.compare_popup_overlay(window))
            .into_any_element()
    }

    /// The Commit tool window: the changes, the commit message, Commit and Amend.
    pub fn commit_tool_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.note_focus(window, cx);
        island(cx)
            .size_full()
            .child(self.commit_window(cx))
            .into_any_element()
    }

    /// What the Git menu shows for this repository now.
    pub fn ide_git_menu(&self) -> GitMenuState {
        GitMenuState(self.git_menu_state())
    }

    /// The main toolbar's VCS Group (Update Project, Push…, the VCS widget and its branches
    /// popup, the Merge/Rebase widget), once the repository has loaded.
    pub fn toolbar_vcs_group(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        matches!(self.repo, Some(Ok(_))).then(|| self.vcs_group(cx).into_any_element())
    }

    /// What opening the Commit tool window focuses: its changes tree, once it shows (the
    /// repository loaded, the Commit tab open).
    pub fn commit_focus_handle(&self) -> Option<FocusHandle> {
        let shown = matches!(self.repo, Some(Ok(_)))
            && self.commit_tab() == crate::stash_view::CommitTab::Commit;
        shown.then(|| self.commit_ui.focus.clone())
    }
}

/// Spork's Settings pages (Version Control's) inside the IDE's Settings dialog: they edit
/// Spork's own preferences and git's configuration, which its tool windows read, and keep their
/// changes until the dialog applies them.
pub struct SettingsPages(Entity<crate::settings_window::SettingsWindow>);

impl SettingsPages {
    /// The pages of the project whose Git tool window shows `repo` in the window `project`,
    /// drawn in `window`.
    pub fn new(
        repo: Option<(Entity<RepoView>, AnyWindowHandle)>,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let prefs = repo
            .as_ref()
            .map_or_else(crate::prefs::Prefs::load, |(repo, _)| {
                repo.read(cx).prefs.clone()
            });
        let project = repo.map(|(repo, project)| (repo.downgrade(), project));
        Self(
            cx.new(|cx| {
                crate::settings_window::SettingsWindow::embedded(project, prefs, window, cx)
            }),
        )
    }

    /// Shows the page at `path` of Spork's tree ("Version Control/Git").
    pub fn show(&self, path: &str, cx: &mut App) {
        self.0.update(cx, |pages, cx| pages.show(path, cx));
    }

    /// The page shown, to draw in the dialog.
    pub fn view(&self) -> AnyView {
        self.0.clone().into()
    }

    pub fn modified(&self, cx: &App) -> bool {
        self.0.read(cx).modified(cx)
    }

    /// Calls `changed` on the view whose context this is whenever a page changes, then notifies
    /// it: what the view keeps of the change is kept even while its window isn't drawn.
    pub fn observe<V: 'static>(
        &self,
        cx: &mut Context<V>,
        changed: impl Fn(&mut V, &mut Context<V>) + 'static,
    ) -> Subscription {
        cx.observe(&self.0, move |view, _, cx| {
            changed(view, cx);
            cx.notify()
        })
    }

    pub fn apply(&self, window: &mut Window, cx: &mut App) {
        self.0.update(cx, |pages, cx| pages.apply(window, cx));
    }
}

/// The log's focus: the Git tool window tracks it, so the log's keys reach the view.
impl Focusable for RepoView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
