//! Spork's version control in the IDE's windows (ROADMAP, phase 2): for the git repository of a
//! workspace's first folder, the Git tool window (Spork's log) in the bottom dock and the Commit
//! tool window in the left dock, where IntelliJ's New UI has them; a workspace with no repository
//! has neither. The Git menu is Spork's (`git_menu`).

use std::borrow::Cow;

pub mod annotate;
mod line_status;
pub use line_status::{ShowNextChangeMarker, ShowPrevChangeMarker};
mod revision_tabs;

use gpui::{
    Action, AnyElement, App, AppContext as _, AssetSource, Context, Entity, EventEmitter,
    FocusHandle, Focusable, Global, InteractiveElement as _, IntoElement, KeyBinding, Menu,
    ParentElement as _, Pixels, Render, SharedString, Styled as _, Subscription, TaskExt as _,
    Window, px,
};
use gpui_kit::component::{Root, Theme};
use spork::RepoView;
use spork::ide::{GitMenuState, IdeEvent, ToolWindow, ToolWindowEvent};
use ui::utils::WithRemSize;
use util::ResultExt as _;
use workspace::Workspace;
use workspace::dock::{DockPosition, Panel, PanelEvent};

pub mod git_panel {
    gpui::actions!(
        git_panel,
        [
            /// Opens the Git tool window and focuses it, or gives the focus back to the editor.
            ToggleFocus
        ]
    );
}

pub mod commit_panel {
    gpui::actions!(
        commit_panel,
        [
            /// Opens the Commit tool window and focuses it, or gives the focus back to the editor.
            ToggleFocus
        ]
    );
}

/// The keys of Spork's views, bound again each time the IDE loads its keymap anew.
struct ViewKeys(Vec<KeyBinding>);

impl Global for ViewKeys {}

/// Sets up Spork's views, its files kept apart in the IDE's config folder, and adds its tool
/// windows to each workspace whose first folder is in a git repository.
pub fn init(cx: &mut App) {
    let mut keys = spork::ide::init(paths::config_dir().join("spork"), cx);
    keys.extend(revision_tabs::key_bindings());
    cx.set_global(ViewKeys(keys));
    cx.set_global(GitMenu::default());
    line_status::init(cx);
    annotate::init(cx);
    answer_prompts(cx);
    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        let Some(window) = window else { return };
        workspace.register_action(|workspace, _: &git_panel::ToggleFocus, window, cx| {
            workspace.toggle_panel_focus::<GitPanel>(window, cx);
        });
        workspace.register_action(|workspace, _: &commit_panel::ToggleFocus, window, cx| {
            workspace.toggle_panel_focus::<CommitPanel>(window, cx);
        });
        let blank = cx.new(|_| Blank);
        let root = cx.new(|cx| Root::new(blank, window, cx));
        Root::host(&root, window, cx);
        let overlay = cx.new(|cx| Overlay {
            root,
            _popup: cx.observe_global::<line_status::LineStatusPopup>(|_, cx| cx.notify()),
        });
        workspace.set_overlay(overlay.into(), cx);
        let project = workspace.project().clone();
        cx.subscribe_in(&project, window, |workspace, _, event, window, cx| {
            if let project::Event::WorktreeAdded(_) = event {
                add_tool_windows(workspace, window, cx);
            }
        })
        .detach();
        add_tool_windows(workspace, window, cx);
    })
    .detach();
}

/// The repository view of the workspace's Git tool window, if its first folder is in one.
pub fn repo_view(workspace: &Workspace, cx: &App) -> Option<Entity<RepoView>> {
    workspace
        .panel::<GitPanel>(cx)
        .map(|panel| panel.read(cx).repo.clone())
}

/// What the end-to-end tests read of version control in `workspace`: `diff_top`, the line at the
/// top of the active diff tab's diff (`src/main.rs:12`).
pub fn e2e_state(workspace: &Workspace, cx: &App) -> serde_json::Value {
    serde_json::json!({ "diff_top": revision_tabs::e2e_diff_top(workspace, cx) })
}

/// Whether the file at `abs_path` has a local change, which the Commit tool window lists.
pub fn has_local_change(workspace: &Workspace, abs_path: &std::path::Path, cx: &App) -> bool {
    repo_view(workspace, cx).is_some_and(|repo| repo.read(cx).has_local_change(abs_path))
}

/// IntelliJ's Select In ▸ Commit: shows the Commit tool window with the file at `abs_path`
/// selected.
pub fn select_in_commit(
    workspace: &mut Workspace,
    abs_path: &std::path::Path,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(repo) = repo_view(workspace, cx) else {
        return;
    };
    if repo.update(cx, |repo, cx| repo.select_local_change(abs_path, cx)) {
        workspace.focus_panel::<CommitPanel>(window, cx);
    }
}

/// git runs the IDE's binary as its GIT_ASKPASS and SSH_ASKPASS: when this is that run, asks the
/// IDE and exits with git's answer.
pub fn answer_askpass_if_asked() {
    if std::env::var_os(spork_git::askpass::HELPER_ENV).is_some() {
        std::process::exit(spork_git::askpass::helper_main());
    }
}

/// git's password and passphrase prompts, asked in the window in front with Spork's dialog
/// (IntelliJ's), the repository's view answering them.
fn answer_prompts(cx: &mut App) {
    let Some(prompts) = spork::ide::start_askpass() else {
        return;
    };
    cx.spawn(async move |cx| {
        while let Ok((prompt, reply)) = prompts.recv().await {
            cx.update(|cx| {
                let front = cx
                    .active_window()
                    .or_else(|| cx.windows().into_iter().next())
                    .and_then(|window| window.downcast::<workspace::MultiWorkspace>());
                let Some(window) = front else {
                    // nobody to ask: git's command fails as cancelled
                    return;
                };
                window
                    .update(cx, |multi, window, cx| {
                        let repo = multi
                            .workspace()
                            .read(cx)
                            .panel::<GitPanel>(cx)
                            .map(|panel| panel.read(cx).repo.clone());
                        if let Some(repo) = repo {
                            repo.update(cx, |repo, cx| repo.ask(prompt, reply, window, cx));
                        }
                    })
                    .log_err();
            });
        }
    })
    .detach();
}

/// Binds the keys of Spork's views. The IDE calls it each time it clears its keymap, before
/// binding its own, so that its bindings win where both bind a key in the same context.
pub fn bind_keys(cx: &mut App) {
    if let Some(keys) = cx.try_global::<ViewKeys>() {
        let keys = keys.0.clone();
        cx.bind_keys(keys);
    }
}

/// What the Git menu shows of the repository in front; the app builds its menu bar again when
/// it changes (an operation starting or ending, conflicts).
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct GitMenu(GitMenuState);

impl Global for GitMenu {}

/// Spork's Git menu, for the IDE's menu bar, with IntelliJ's Annotate with Git Blame in Current
/// File, which needs an editor.
pub fn git_menu(cx: &App) -> Menu {
    let state = cx.try_global::<GitMenu>().copied().unwrap_or_default();
    let mut menu = spork::ide::git_menu(state.0);
    if let Some(gpui::MenuItem::Submenu(file)) = menu
        .items
        .iter_mut()
        .find(|item| matches!(item, gpui::MenuItem::Submenu(menu) if menu.name.as_ref() == "Current File"))
    {
        let at = file
            .items
            .iter()
            .position(|item| matches!(item, gpui::MenuItem::Separator))
            .map_or(file.items.len(), |separator| separator + 1);
        file.items
            .insert(at, gpui::MenuItem::action("Annotate with Git Blame", git::Blame));
    }
    menu
}

/// The main toolbar's VCS Group for `workspace`, once its repository has loaded: Update Project,
/// Push…, the VCS widget (the branch, opening the branches popup), the Merge/Rebase widget.
pub fn vcs_widget(workspace: &Entity<Workspace>, cx: &mut App) -> Option<AnyElement> {
    let repo = workspace
        .read(cx)
        .panel::<GitPanel>(cx)?
        .read(cx)
        .repo
        .clone();
    let group = repo.update(cx, |repo, cx| repo.toolbar_vcs_group(cx))?;
    Some(spork_look(cx).child(group).into_any_element())
}

/// The IDE's assets, and Spork's for the paths only Spork has (its icons, gpui-kit's).
pub struct Assets<A>(pub A);

impl<A: AssetSource> AssetSource for Assets<A> {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        match spork_workbench::Assets::serves(path) {
            true => spork_workbench::Assets.load(path),
            false => self.0.load(path),
        }
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        match spork_workbench::Assets::serves(path) {
            true => spork_workbench::Assets.list(path),
            false => self.0.list(path),
        }
    }
}

/// Adds the Git and Commit tool windows once the first folder is known to be in a repository.
fn add_tool_windows(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    if workspace.panel::<GitPanel>(cx).is_some() {
        return;
    }
    let Some(folder) = workspace
        .project()
        .read(cx)
        .visible_worktrees(cx)
        .next()
        .map(|worktree| worktree.read(cx).abs_path().to_path_buf())
    else {
        return;
    };
    cx.spawn_in(window, async move |workspace, cx| {
        let in_repository = cx
            .background_spawn({
                let folder = folder.clone();
                async move { spork_git::toplevel(&folder).is_ok() }
            })
            .await;
        if !in_repository {
            return;
        }
        workspace
            .update_in(cx, |workspace, window, cx| {
                // another folder added meanwhile may have added them
                if workspace.panel::<GitPanel>(cx).is_some() {
                    return;
                }
                let repo = cx.new(|cx| RepoView::open(folder, window, cx));
                let git = cx.new(|cx| GitPanel::new(repo.clone(), cx));
                let commit = cx.new(|cx| CommitPanel::new(repo.clone(), cx));
                workspace.add_panel(git, window, cx);
                workspace.add_panel(commit, window, cx);
                // the Git menu and the views' keys reach the view wherever the focus is
                let view = repo.downgrade();
                workspace.register_action_renderer(move |el, _, _, cx| match view.upgrade() {
                    Some(repo) => repo.update(cx, |repo, cx| repo.with_actions(el, cx)),
                    None => el,
                });
                cx.subscribe_in(
                    &repo,
                    window,
                    |workspace, repo, event, window, cx| match event {
                        IdeEvent::ShowDiff(tab) => {
                            revision_tabs::show_diff(workspace, repo, *tab, window, cx)
                        }
                        IdeEvent::OpenRevision {
                            path,
                            revision,
                            text,
                        } => revision_tabs::open_revision(
                            workspace,
                            path.clone(),
                            revision.clone(),
                            text.clone(),
                            window,
                            cx,
                        ),
                        IdeEvent::OpenFile { path, line } => {
                            revision_tabs::open_file(workspace, path.clone(), *line, window, cx)
                        }
                        IdeEvent::OpenProject(path) => {
                            let options = workspace::OpenOptions {
                                open_mode: workspace::OpenMode::NewWindow,
                                ..workspace::OpenOptions::default()
                            };
                            workspace::open_paths(
                                std::slice::from_ref(path),
                                workspace.app_state().clone(),
                                options,
                                cx,
                            )
                            .detach_and_log_err(cx);
                        }
                    },
                )
                .detach();
                // the menu bar follows the repository of the window in front
                cx.observe_in(&repo, window, |_, repo, window, cx| {
                    let state = GitMenu(repo.read(cx).ide_git_menu());
                    let in_front = cx
                        .active_window()
                        .is_none_or(|front| front == window.window_handle());
                    if in_front && cx.try_global::<GitMenu>() != Some(&state) {
                        cx.set_global(state);
                    }
                })
                .detach();
                cx.subscribe_in(
                    &repo,
                    window,
                    |workspace, _, event, window, cx| match event {
                        ToolWindowEvent::Show(ToolWindow::Git) => {
                            workspace.open_panel::<GitPanel>(window, cx)
                        }
                        ToolWindowEvent::Show(ToolWindow::Commit) => {
                            workspace.open_panel::<CommitPanel>(window, cx)
                        }
                        ToolWindowEvent::Hide(ToolWindow::Git) => {
                            workspace.close_panel::<GitPanel>(window, cx)
                        }
                        ToolWindowEvent::Hide(ToolWindow::Commit) => {
                            workspace.close_panel::<CommitPanel>(window, cx)
                        }
                    },
                )
                .detach();
            })
            .log_err();
    })
    .detach();
}

/// Spork's text as the standalone app draws it, whatever the IDE's is around it: gpui-kit's rem
/// size, font and color (the standalone app's window root sets them).
pub fn spork_look(cx: &App) -> WithRemSize {
    let theme = Theme::global(cx);
    WithRemSize::new(theme.font_size)
        .font(gpui::font(theme.font_family.clone()))
        .text_size(gpui::rems(1.))
        .line_height(gpui::phi())
        .text_color(theme.foreground)
}

/// The content of the window's gpui-kit Root: the IDE's workspace is drawn under it.
struct Blank;

impl Render for Blank {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

/// The window's gpui-kit Root, drawn over the workspace: Spork's dialogs, toasts and tooltips.
struct Overlay {
    root: Entity<Root>,
    _popup: Subscription,
}

impl Render for Overlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // the workspace drawn is the one whose Root the window's dialogs go to
        if Root::find(window, cx).as_ref() != Some(&self.root) {
            Root::host(&self.root, window, cx);
        }
        spork_look(cx)
            .absolute()
            .inset_0()
            .child(self.root.clone())
            .children(line_status::render(window, cx))
    }
}

/// The Git tool window (⌘9): Spork's log, with its branches pane, graph and details.
pub struct GitPanel {
    repo: Entity<RepoView>,
    _subscription: Subscription,
}

impl GitPanel {
    fn new(repo: Entity<RepoView>, cx: &mut Context<Self>) -> Self {
        let _subscription = cx.observe(&repo, |_, _, cx| cx.notify());
        Self {
            repo,
            _subscription,
        }
    }
}

impl Render for GitPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.focus_handle(cx);
        let content = self
            .repo
            .update(cx, |repo, cx| repo.git_tool_window(window, cx));
        spork_look(cx).size_full().child(
            gpui::div()
                .size_full()
                .track_focus(&focus)
                .key_context("Spork")
                .child(content),
        )
    }
}

impl Focusable for GitPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.repo.read(cx).focus_handle(cx)
    }
}

impl EventEmitter<PanelEvent> for GitPanel {}

impl Panel for GitPanel {
    fn persistent_name() -> &'static str {
        "Git"
    }

    fn panel_key() -> &'static str {
        "GitToolWindow"
    }

    fn position(&self, _: &Window, _: &App) -> DockPosition {
        DockPosition::Bottom
    }

    // shortcut: the tool window stays where IntelliJ puts it; moving it comes with phase 3's
    // stripes, which keep where the user dragged it
    fn position_is_valid(&self, position: DockPosition) -> bool {
        position == DockPosition::Bottom
    }

    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}

    fn default_size(&self, _: &Window, _: &App) -> Pixels {
        px(320.)
    }

    fn icon(&self, _: &Window, _: &App) -> Option<ui::IconName> {
        Some(ui::IconName::GitBranch)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some("Git")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(git_panel::ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        9
    }
}

/// The Commit tool window (⌘0): the changes, the commit message, Commit and Amend.
pub struct CommitPanel {
    repo: Entity<RepoView>,
    focus_handle: FocusHandle,
    _subscription: Subscription,
}

impl CommitPanel {
    fn new(repo: Entity<RepoView>, cx: &mut Context<Self>) -> Self {
        let _subscription = cx.observe(&repo, |_, _, cx| cx.notify());
        Self {
            repo,
            focus_handle: cx.focus_handle(),
            _subscription,
        }
    }
}

impl Render for CommitPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self
            .repo
            .update(cx, |repo, cx| repo.commit_tool_window(window, cx));
        spork_look(cx).size_full().child(
            gpui::div()
                .size_full()
                .track_focus(&self.focus_handle)
                .key_context("Spork")
                .child(content),
        )
    }
}

impl Focusable for CommitPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for CommitPanel {}

impl Panel for CommitPanel {
    fn persistent_name() -> &'static str {
        "Commit"
    }

    fn panel_key() -> &'static str {
        "CommitToolWindow"
    }

    fn activation_focus_handle(&self, cx: &App) -> FocusHandle {
        self.repo
            .read(cx)
            .commit_focus_handle()
            .unwrap_or_else(|| self.focus_handle.clone())
    }

    fn position(&self, _: &Window, _: &App) -> DockPosition {
        DockPosition::Left
    }

    // shortcut: as the Git tool window's, until phase 3's stripes
    fn position_is_valid(&self, position: DockPosition) -> bool {
        position == DockPosition::Left
    }

    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}

    fn default_size(&self, _: &Window, _: &App) -> Pixels {
        px(285.)
    }

    fn icon(&self, _: &Window, _: &App) -> Option<ui::IconName> {
        Some(ui::IconName::GitCommit)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some("Commit")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(commit_panel::ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        8
    }
}
