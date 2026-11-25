//! Diffs and files at a revision in editor tabs, IntelliJ's default: the Commit tool window's diff
//! preview (`Commit: main.rs`), its Stash and Shelf tabs' (`Stash@{0}: main.rs`, `Shelved:
//! main.rs`) and the Git tool window's (`Repository Diff: main.rs`), one tab each and reused,
//! drawn by Spork's diff viewer; Open Repository Version's read-only editor
//! (`main.rs (a1b2c3d4)`).

use std::path::{Path, PathBuf};

use editor::Editor;
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Subscription, TaskExt as _, Window,
};
use language::Buffer;
use spork::RepoView;
use spork::ide::DiffTab;
use ui::{Icon, IconName};
use workspace::Workspace;
use workspace::item::{Item, ItemEvent};

gpui::actions!(
    vcs,
    [
        /// Closes the diff editor tab that has the focus (IntelliJ's Close Diff Editor, ⎋).
        CloseDiffEditor
    ]
);

/// ⎋ in a diff editor tab's diff (not its find field), kept with Spork's view keys so the IDE
/// binds it again with them; elsewhere a diff has no handler and the key goes on.
pub(crate) fn key_bindings() -> Vec<gpui::KeyBinding> {
    vec![gpui::KeyBinding::new(
        "escape",
        CloseDiffEditor,
        Some("Diff"),
    )]
}

/// Opens `tab` showing what `repo` has for it, or brings it forward.
pub(crate) fn show_diff(
    workspace: &mut Workspace,
    repo: &Entity<RepoView>,
    tab: DiffTab,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    // the Stash and Shelf tabs keep the focus, a pick previewing its diff
    // shortcut: Show Diff (⌘D) and a double click don't focus the diff either, as IntelliJ's do;
    // tell a preview from an opening once the views say which it is
    let focus = tab != DiffTab::Saved;
    let open = workspace
        .items_of_type::<DiffTabItem>(cx)
        .find(|item| item.read(cx).tab == tab);
    if let Some(item) = open {
        workspace.activate_item(&item, true, focus, window, cx);
        return;
    }
    let item = cx.new(|cx| DiffTabItem::new(repo.clone(), tab, cx));
    workspace.add_item_to_active_pane(Box::new(item), None, focus, window, cx);
}

/// Jump to Source: the working copy's file at `path` in an editor tab, the caret at the start
/// of its one-based `line` when given.
pub(crate) fn open_file(
    workspace: &mut Workspace,
    path: PathBuf,
    line: Option<usize>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let open = workspace.open_abs_path(path, workspace::OpenOptions::default(), window, cx);
    cx.spawn_in(window, async move |_, cx| {
        let item = open.await?;
        let (Some(line), Some(editor)) = (line, item.downcast::<Editor>()) else {
            return anyhow::Ok(());
        };
        let point = text::Point::new(line.saturating_sub(1) as u32, 0);
        editor.update_in(cx, |editor, window, cx| {
            editor.go_to_singleton_buffer_point(point, window, cx)
        })
    })
    .detach_and_log_err(cx);
}

/// Open Repository Version: `path` as of `revision` in a read-only editor, its language's
/// highlighting on.
pub(crate) fn open_revision(
    workspace: &mut Workspace,
    path: String,
    revision: String,
    text: String,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let project = workspace.project().clone();
    let languages = project.read(cx).languages().clone();
    let buffer = cx.new(|cx| {
        let buffer = Buffer::local(text, cx);
        buffer.set_language_registry(languages.clone());
        buffer
    });
    cx.spawn({
        let (buffer, path) = (buffer.clone(), PathBuf::from(&path));
        async move |_, cx| {
            let language = languages.load_language_for_file_path(&path).await.ok();
            buffer.update(cx, |buffer, cx| buffer.set_language(language, cx));
        }
    })
    .detach();
    let editor = cx.new(|cx| {
        let mut editor = Editor::for_buffer(buffer, Some(project), window, cx);
        editor.set_read_only(true);
        editor
    });
    let name = path.rsplit('/').next().unwrap_or(&path);
    let short: String = revision.chars().take(8).collect();
    let item = cx.new(|cx| RevisionTab {
        title: format!("{name} ({short})").into(),
        path: PathBuf::from(&path),
        _subscription: cx.observe(&editor, |_, _, cx| cx.notify()),
        editor,
    });
    workspace.add_item_to_active_pane(Box::new(item), None, true, window, cx);
}

/// The line-status popup's Show Diff for Lines: the file's diff against HEAD, in the Commit tab.
// shortcut: the whole file's diff, not IntelliJ's window of the change's lines alone; give the diff
// viewer a range to show once it takes one
pub(crate) fn show_file_diff(editor: &Entity<Editor>, window: &mut Window, cx: &mut App) {
    let Some(path) = file_path(editor, cx) else {
        return;
    };
    with_repo(editor, window, cx, move |repo, cx| {
        repo.update(cx, |repo, cx| repo.show_local_change(&path, cx))
    });
}

/// An annotated line's Show Diff: its commit's diff at the file, in the Repository Diff tab.
pub(crate) fn show_commit_file(
    editor: &Entity<Editor>,
    sha: &str,
    path: &Path,
    window: &mut Window,
    cx: &mut App,
) {
    let (sha, path) = (sha.to_owned(), path.to_owned());
    with_repo(editor, window, cx, move |repo, cx| {
        repo.update(cx, |repo, cx| repo.show_commit_diff_at(&sha, &path, cx))
    });
}

/// The file `editor` shows, on disk.
fn file_path(editor: &Entity<Editor>, cx: &App) -> Option<PathBuf> {
    let buffer = editor.read(cx).buffer().read(cx).as_singleton()?;
    let file = buffer.read(cx).file()?.as_local()?.abs_path(cx);
    Some(file)
}

/// Runs `f` on the repository of `editor`'s workspace, once that workspace isn't being updated.
fn with_repo(
    editor: &Entity<Editor>,
    window: &mut Window,
    cx: &mut App,
    f: impl FnOnce(&Entity<RepoView>, &mut App) + 'static,
) {
    let Some(workspace) = editor.read(cx).workspace() else {
        return;
    };
    window.defer(cx, move |_, cx| {
        let repo = workspace
            .read(cx)
            .panel::<crate::GitPanel>(cx)
            .map(|panel| panel.read(cx).repo.clone());
        if let Some(repo) = repo {
            f(&repo, cx);
        }
    });
}

/// The line at the top of the active diff tab's diff, for the end-to-end tests.
pub(crate) fn e2e_diff_top(workspace: &Workspace, cx: &App) -> Option<String> {
    let item = workspace.active_item_as::<DiffTabItem>(cx)?;
    let item = item.read(cx);
    item.repo.read(cx).diff_tab_top_line(item.tab, cx)
}

/// A diff preview tab, Spork's diff viewer in it.
pub(crate) struct DiffTabItem {
    repo: Entity<RepoView>,
    tab: DiffTab,
    focus_handle: FocusHandle,
    _subscription: Subscription,
}

impl DiffTabItem {
    fn new(repo: Entity<RepoView>, tab: DiffTab, cx: &mut Context<Self>) -> Self {
        Self {
            _subscription: cx.observe(&repo, |_, _, cx| cx.notify()),
            repo,
            tab,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl Render for DiffTabItem {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.tab;
        let content: AnyElement = self
            .repo
            .update(cx, |repo, cx| repo.diff_tab(tab, window, cx));
        crate::spork_look(cx).size_full().child(
            gpui::div()
                .size_full()
                .track_focus(&self.focus_handle)
                .key_context("Spork")
                .on_action(
                    cx.listener(|_, _: &CloseDiffEditor, _, cx| cx.emit(ItemEvent::CloseItem)),
                )
                .child(content),
        )
    }
}

impl Focusable for DiffTabItem {
    // the diff itself, so its keys (F7, ⇧F7, ⎋) work at once
    // shortcut: an image's or a submodule's change draws no diff, so its tab takes no keys; give
    // those views a focus of their own once they get keys
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.repo.read(cx).diff_tab_focus(self.tab, cx)
    }
}

impl EventEmitter<ItemEvent> for DiffTabItem {}

impl Item for DiffTabItem {
    type Event = ItemEvent;

    fn to_item_events(event: &Self::Event, f: &mut dyn FnMut(ItemEvent)) {
        f(*event)
    }

    fn tab_content_text(&self, _: usize, cx: &App) -> SharedString {
        self.repo.read(cx).diff_tab_title(self.tab, cx)
    }

    fn tab_icon(&self, _: &Window, _: &App) -> Option<Icon> {
        Some(Icon::new(IconName::Diff))
    }
}

/// A file as of a revision, read-only.
pub(crate) struct RevisionTab {
    editor: Entity<Editor>,
    title: SharedString,
    path: PathBuf,
    _subscription: Subscription,
}

impl Render for RevisionTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.editor.clone()
    }
}

impl Focusable for RevisionTab {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.editor.focus_handle(cx)
    }
}

impl EventEmitter<()> for RevisionTab {}

impl Item for RevisionTab {
    type Event = ();

    fn tab_content_text(&self, _: usize, _: &App) -> SharedString {
        self.title.clone()
    }

    fn tab_icon(&self, _: &Window, cx: &App) -> Option<Icon> {
        file_icons::FileIcons::get_icon(&self.path, cx).map(Icon::from_path)
    }
}
