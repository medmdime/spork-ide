//! Spork IDE's projects (ROADMAP, phase 6), as IntelliJ's: the Welcome window, shown when no
//! project is open; the Recent Projects popup of the main toolbar's project widget and File ▸
//! Recent Projects; New Project and Get from Version Control; Close Project's siblings, Close All
//! Projects and Close Other Projects.

mod dialogs;
mod project_groups;
mod project_popup;
mod welcome;

use std::path::{Path, PathBuf};

use gpui::{
    Action, App, AppContext as _, Global, MenuItem, SharedString, Task, TaskExt as _, Window,
    actions,
};
use schemars::JsonSchema;
use serde::Deserialize;
use util::ResultExt as _;
use util::paths::PathExt as _;
use workspace::{
    AppState, CloseWindow, MultiWorkspace, OpenOptions, RecentWorkspace,
    SerializedWorkspaceLocation, Workspace, WorkspaceDb,
};

pub use project_popup::ProjectPopup;

actions!(
    projects,
    [
        /// Closes every project window; the Welcome window shows once the last one has closed.
        CloseAllProjects,
        /// Closes the project windows other than this one.
        CloseOtherProjects,
        /// Creates a project folder and opens it.
        NewProject,
        /// Clones a repository into a folder and opens it.
        GetFromVersionControl,
        /// Lists the recent projects to open one.
        ManageRecentProjects,
        /// Copies the selected recent project's path (the Welcome window's Copy Path, ⇧⌘C).
        CopyProjectPath,
    ]
);

/// Opens a recent project, as File ▸ Recent Projects' items do.
#[derive(PartialEq, Clone, Deserialize, JsonSchema, Action)]
#[action(namespace = projects)]
#[serde(deny_unknown_fields)]
pub struct OpenRecentProject {
    pub paths: Vec<PathBuf>,
}

/// File ▸ Recent Projects' projects, IntelliJ's: newest first, the one in front left out, each
/// by its name (its location where two share a name). The app builds its menu bar again when
/// they change.
#[derive(Clone, Default, PartialEq)]
pub struct RecentProjectsMenu(Vec<(SharedString, Vec<PathBuf>)>);

impl Global for RecentProjectsMenu {}

/// File ▸ Recent Projects' items: the recent projects, then Manage Projects….
pub fn recent_projects_menu(cx: &App) -> Vec<MenuItem> {
    let mut items: Vec<MenuItem> = cx
        .try_global::<RecentProjectsMenu>()
        .map(|menu| {
            menu.0
                .iter()
                .map(|(label, paths)| {
                    MenuItem::action(
                        label.clone(),
                        OpenRecentProject {
                            paths: paths.clone(),
                        },
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if !items.is_empty() {
        items.push(MenuItem::separator());
    }
    items.push(MenuItem::action("Manage Projects…", ManageRecentProjects));
    items
}

/// Reads the recent projects again for File ▸ Recent Projects, leaving out `front`, the
/// folders of the project in front.
fn refresh_recent_projects_menu(front: Vec<PathBuf>, cx: &mut App) {
    let Some(app_state) = AppState::try_global(cx) else {
        return;
    };
    let (db, fs) = (WorkspaceDb::global(cx), app_state.fs.clone());
    let recent =
        cx.background_spawn(async move { db.recent_project_workspaces(fs.as_ref()).await });
    cx.spawn(async move |cx| {
        let recent = recent.await.log_err();
        let projects: Vec<Vec<PathBuf>> = recent
            .unwrap_or_default()
            .into_iter()
            .filter(|workspace| matches!(workspace.location, SerializedWorkspaceLocation::Local))
            .map(|workspace| workspace.paths.paths().to_vec())
            .filter(|paths| !paths.is_empty() && *paths != front)
            .collect();
        let menu = RecentProjectsMenu(menu_labels(projects));
        cx.update(|cx| {
            if cx.try_global::<RecentProjectsMenu>() != Some(&menu) {
                cx.set_global(menu);
            }
        });
    })
    .detach();
}

/// Each project's item text: its name, or `~/…` where another has the same name.
fn menu_labels(projects: Vec<Vec<PathBuf>>) -> Vec<(SharedString, Vec<PathBuf>)> {
    let name = |paths: &[PathBuf]| {
        paths
            .iter()
            .map(|path| display_name(path))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let names: Vec<String> = projects.iter().map(|paths| name(paths)).collect();
    projects
        .into_iter()
        .zip(&names)
        .map(|(paths, label)| {
            let label = match names.iter().filter(|other| *other == label).count() {
                1 => label.clone(),
                _ => paths
                    .iter()
                    .map(|path| path.compact().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join(", "),
            };
            (label.into(), paths)
        })
        .collect()
}

pub fn init(cx: &mut App) {
    // the Welcome window has no workspace to answer it
    cx.on_action(|action: &OpenRecentProject, cx| open_project(action.paths.clone(), cx));
    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        workspace.register_action(|_, _: &CloseAllProjects, window, cx| {
            close_projects(true, window, cx)
        });
        workspace.register_action(|_, _: &CloseOtherProjects, window, cx| {
            close_projects(false, window, cx)
        });
        workspace.register_action(|_, _: &NewProject, window, cx| dialogs::new_project(window, cx));
        workspace.register_action(|_, _: &GetFromVersionControl, window, cx| {
            dialogs::get_from_vcs(window, cx)
        });
        workspace.register_action(|_, _: &ManageRecentProjects, window, cx| {
            project_popup::manage(window, cx)
        });
        workspace.register_action(|_, action: &OpenRecentProject, _, cx| {
            open_project(action.paths.clone(), cx)
        });
        let Some(window) = window else { return };
        // File ▸ Recent Projects lists the projects but the one in front
        cx.observe_window_activation(window, |workspace, window, cx| {
            if window.is_window_active() {
                refresh_recent_projects_menu(project_folders(workspace, cx), cx);
            }
        })
        .detach();
        // a project opened from anywhere (the Welcome window, the command line, the Dock) takes
        // the Welcome window's place
        if workspace.visible_worktrees(cx).next().is_some() {
            welcome::close(cx);
        }
        let project = workspace.project().clone();
        cx.subscribe_in(&project, window, |workspace, _, event, _, cx| {
            if let project::Event::WorktreeAdded(_) = event
                && workspace.visible_worktrees(cx).next().is_some()
            {
                welcome::close(cx);
                refresh_recent_projects_menu(project_folders(workspace, cx), cx);
            }
        })
        .detach();
    })
    .detach();
    cx.on_window_closed(|cx, closed| welcome::on_window_closed(closed, cx))
        .detach();
}

/// Shows the Welcome window: the IDE calls it at startup when it has no project to reopen.
pub fn show_welcome(cx: &mut App) {
    welcome::show(cx);
    refresh_recent_projects_menu(Vec::new(), cx);
}

/// A project window's folders, as the recent projects list them.
fn project_folders(workspace: &Workspace, cx: &App) -> Vec<PathBuf> {
    workspace
        .visible_worktrees(cx)
        .map(|worktree| worktree.read(cx).abs_path().to_path_buf())
        .collect()
}

/// Close Project: the window closes (asking to save its changes first), and the Welcome window
/// shows once no project window is left.
pub fn close_project(window: &mut Window, cx: &mut App) {
    window.dispatch_action(Box::new(CloseWindow), cx);
}

/// Closes the project windows, `all` or all but `window`'s; each asks to save its changes.
fn close_projects(all: bool, window: &mut Window, cx: &mut App) {
    let this = window.window_handle().window_id();
    for handle in cx.windows() {
        if handle.window_id() == this || handle.downcast::<MultiWorkspace>().is_none() {
            continue;
        }
        handle
            .update(cx, |_, window, cx| {
                window.dispatch_action(Box::new(CloseWindow), cx)
            })
            .log_err();
    }
    if all {
        close_project(window, cx);
    }
}

/// Whether a project window is open.
fn any_project_window(cx: &App) -> bool {
    cx.windows()
        .iter()
        .any(|handle| handle.downcast::<MultiWorkspace>().is_some())
}

/// Opens `paths` as a project in a window of its own, or brings forward the window that has it.
pub(crate) fn open_project(paths: Vec<PathBuf>, cx: &mut App) {
    let Some(app_state) = AppState::try_global(cx) else {
        return;
    };
    let options = OpenOptions {
        add_dirs_to_sidebar: false,
        ..OpenOptions::default()
    };
    workspace::open_paths(&paths, app_state, options, cx).detach_and_log_err(cx);
}

/// A recent project, as the Welcome window and the popup list it.
#[derive(Clone)]
pub(crate) struct RecentProject {
    pub paths: Vec<PathBuf>,
    pub name: SharedString,
    /// The folder, `~/…`.
    pub location: SharedString,
    pub branch: Option<SharedString>,
    pub slot: usize,
    /// Its entry in the recent projects, which Remove from Recent Projects… deletes.
    workspace: Option<RecentWorkspace>,
}

impl RecentProject {
    fn new(workspace: RecentWorkspace) -> Option<Self> {
        let paths = workspace.paths.paths().to_vec();
        Self::from_paths(paths, Some(workspace))
    }

    /// A project by its folders; asks git for the branch.
    pub(crate) fn from_paths(
        paths: Vec<PathBuf>,
        workspace: Option<RecentWorkspace>,
    ) -> Option<Self> {
        let first = paths.first()?;
        let name = paths
            .iter()
            .map(|path| display_name(path))
            .collect::<Vec<_>>()
            .join(", ");
        Some(Self {
            name: name.into(),
            location: first.compact().to_string_lossy().into_owned().into(),
            branch: spork_git::head_branch(first)
                .ok()
                .flatten()
                .map(SharedString::from),
            slot: spork_workbench::shell::project_slot(first),
            paths,
            workspace,
        })
    }

    /// IntelliJ's speed search: by name and by path.
    pub fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || self.name.to_lowercase().contains(query)
            || self.location.to_lowercase().contains(query)
    }
}

/// A folder's name, or its path when it has none (`/`).
pub(crate) fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The recent local projects, newest first, with their branches (git is asked in the
/// background).
pub(crate) fn load_recent_projects(cx: &App) -> Task<Vec<RecentProject>> {
    let Some(app_state) = AppState::try_global(cx) else {
        return Task::ready(Vec::new());
    };
    let (db, fs) = (WorkspaceDb::global(cx), app_state.fs.clone());
    cx.background_spawn(async move {
        db.recent_project_workspaces(fs.as_ref())
            .await
            .log_err()
            .unwrap_or_default()
            .into_iter()
            .filter(|workspace| matches!(workspace.location, SerializedWorkspaceLocation::Local))
            .filter_map(RecentProject::new)
            .collect()
    })
}

/// Takes `project` off the recent projects, as Remove from Recent Projects… does.
pub(crate) fn forget_recent_project(project: &RecentProject, cx: &App) -> Task<()> {
    let Some(app_state) = AppState::try_global(cx) else {
        return Task::ready(());
    };
    let Some(workspace) = project.workspace.clone() else {
        return Task::ready(());
    };
    let (db, fs) = (WorkspaceDb::global(cx), app_state.fs.clone());
    let delete = cx.background_spawn(async move {
        db.delete_recent_workspace_group(&workspace, fs.as_ref())
            .await
            .log_err();
    });
    cx.spawn(async move |cx| {
        delete.await;
        cx.update(|cx| refresh_recent_projects_menu(front_project_folders(cx), cx));
    })
}

/// The folders of the project whose window is in front, none for another window.
fn front_project_folders(cx: &App) -> Vec<PathBuf> {
    cx.active_window()
        .and_then(|window| window.downcast::<MultiWorkspace>())
        .and_then(|window| window.read(cx).ok())
        .map(|multi| project_folders(multi.workspace().read(cx), cx))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::menu_labels;
    use std::path::PathBuf;

    #[test]
    fn names_a_project_by_its_location_where_two_share_its_name() {
        let labels = menu_labels(vec![
            vec![PathBuf::from("/a/app")],
            vec![PathBuf::from("/b/app")],
            vec![PathBuf::from("/c/tool")],
        ]);
        let labels: Vec<&str> = labels.iter().map(|(label, _)| label.as_ref()).collect();
        assert_eq!(labels, ["/a/app", "/b/app", "tool"]);
    }
}
