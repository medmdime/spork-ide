//! IntelliJ's scratch files (ROADMAP, phase 6): kept in the IDE's config folder (`scratches/`),
//! not in the project, and shown in the Project tool window under "Scratches and Consoles" ▸
//! "Scratches". New Scratch File (⇧⌘N) asks for the file's language first.
//!
//! The tree shows them through an invisible worktree on the config folder, filtered to its
//! `scratches` folder: added once that folder exists, so a project without scratches (and every
//! test on a fake file system) has no such worktree.

use std::path::{Path, PathBuf};

use anyhow::Result;
use gpui::{
    App, AppContext as _, Context, Entity, Global, SharedString, Task, TaskExt as _, WeakEntity,
    Window,
};
use project::{Project, Worktree};
use util::rel_path::RelPath;
use workspace::{OpenOptions, OpenVisible, Workspace};

use crate::file_operations::{Choice, open_chooser};

/// The scratches folder's name in the config folder, IntelliJ's.
const FOLDER: &str = "scratches";

/// The worktree root: the IDE's config folder.
fn root() -> &'static Path {
    paths::config_dir()
}

pub(crate) fn folder() -> PathBuf {
    root().join(FOLDER)
}

/// The invisible worktree that shows the scratches, once added.
pub(crate) fn worktree(project: &Project, cx: &App) -> Option<Entity<Worktree>> {
    project.worktrees(cx).find(|worktree| {
        let worktree = worktree.read(cx);
        !worktree.is_visible() && worktree.abs_path().as_ref() == root()
    })
}

/// Whether the tree shows `path` of the scratches worktree: its root, the scratches folder and
/// what is in it, not the rest of the config folder.
pub(crate) fn shows(path: &RelPath) -> bool {
    path.is_empty() || path.components().next() == Some(FOLDER)
}

/// The name IntelliJ gives the scratches worktree's rows: its root and the scratches folder.
pub(crate) fn label(path: &RelPath) -> Option<&'static str> {
    match path.as_unix_str() {
        "" => Some("Scratches and Consoles"),
        FOLDER => Some("Scratches"),
        _ => None,
    }
}

/// IntelliJ's icon for those two rows.
pub(crate) const ICON: &str = "icons/file_icons/scratches.svg";

/// Adds the scratches worktree to a local project whose config folder has a scratches folder.
pub(crate) fn show_in(project: &Entity<Project>, cx: &mut App) -> Task<Result<()>> {
    let (local, fs) = {
        let project = project.read(cx);
        (project.is_local(), project.fs().clone())
    };
    if !local || worktree(project.read(cx), cx).is_some() {
        return Task::ready(Ok(()));
    }
    let project = project.downgrade();
    cx.spawn(async move |cx| {
        if !fs.is_dir(&folder()).await {
            return Ok(());
        }
        let create = project.update(cx, |project, cx| {
            project.find_or_create_worktree(root(), false, cx)
        })?;
        let (worktree, _) = create.await?;
        cx.update(|cx| keep(project, worktree, cx));
        Ok(())
    })
}

/// The scratches worktrees, held: a project keeps an invisible worktree only while something
/// holds it (an open file's buffer does), and the tree must show this one throughout.
#[derive(Default)]
struct Held(Vec<(WeakEntity<Project>, Entity<Worktree>)>);

impl Global for Held {}

fn keep(project: WeakEntity<Project>, worktree: Entity<Worktree>, cx: &mut App) {
    let held = &mut cx.default_global::<Held>().0;
    held.retain(|(project, _)| project.upgrade().is_some());
    held.push((project, worktree));
}

/// New Scratch File: IntelliJ's popup of languages (Plain text first), then `scratch.<ext>`,
/// `scratch_1.<ext>`… in the scratches folder, opened in an editor.
pub(crate) fn new_scratch_file(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let languages = workspace.app_state().languages.clone();
    let mut names: Vec<SharedString> = languages
        .language_names()
        .into_iter()
        .map(|name| SharedString::from(name.as_ref().to_owned()))
        .filter(|name| name.as_ref() != "Plain Text")
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    let extension = |name: &str| {
        languages
            .available_language_for_name(name)
            .and_then(|language| language.matcher().path_suffixes.first().cloned())
            .map_or_else(|| "txt".to_owned(), |suffix| suffix.to_string())
    };
    let weak = cx.entity().downgrade();
    let choices = std::iter::once(("Plain text".into(), "txt".to_owned()))
        .chain(names.iter().map(|name| (name.clone(), extension(name))))
        .map(|(label, extension)| {
            let weak = weak.clone();
            let icon = spork_workbench::file_icon(&format!("scratch.{extension}"));
            Choice::new(label, move |window, cx| {
                weak.update(cx, |workspace, cx| {
                    create(workspace, extension.clone(), window, cx)
                })
                .ok();
            })
            .with_image(icon)
        })
        .collect();
    open_chooser("New Scratch File", choices, true, 320., window, cx);
}

/// How many scratch buffers New Scratch Buffer goes through (`ide.scratch.buffers`).
const BUFFERS: usize = 5;

/// The last scratch buffer opened, 1 to 5.
#[derive(Default)]
struct LastBuffer(usize);

impl gpui::Global for LastBuffer {}

/// New Scratch Buffer: the next of `buffer1.txt` to `buffer5.txt` in the scratches folder, made
/// if missing (a buffer kept otherwise, as IntelliJ's `create_if_missing`), opened.
pub(crate) fn new_scratch_buffer(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let last = &mut cx.default_global::<LastBuffer>().0;
    *last = *last % BUFFERS + 1;
    let name = format!("buffer{last}.txt");
    open_scratch(workspace, move || folder().join(name), window, cx);
}

/// The first free scratch name for `extension`, IntelliJ's: `scratch.txt`, then `scratch_1.txt`.
fn free_name(folder: &Path, extension: &str) -> PathBuf {
    (0..)
        .map(|n| match n {
            0 => folder.join(format!("scratch.{extension}")),
            n => folder.join(format!("scratch_{n}.{extension}")),
        })
        .find(|path| !path.exists())
        .unwrap_or_else(|| folder.join(format!("scratch.{extension}")))
}

fn create(
    workspace: &mut Workspace,
    extension: String,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    open_scratch(
        workspace,
        move || free_name(&folder(), &extension),
        window,
        cx,
    );
}

/// Opens the scratch file `path` names (asked once the scratches folder is there), made empty if
/// missing.
fn open_scratch(
    workspace: &mut Workspace,
    path: impl FnOnce() -> PathBuf + Send + 'static,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let fs = workspace.app_state().fs.clone();
    let project = workspace.project().clone();
    cx.spawn_in(window, async move |workspace, cx| {
        fs.create_dir(&folder()).await?;
        let path = cx.background_spawn(async move { path() }).await;
        if !fs.is_file(&path).await {
            fs.create_file(&path, Default::default()).await?;
        }
        cx.update(|_, cx| show_in(&project, cx))?.await?;
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.open_abs_path(
                    path,
                    OpenOptions {
                        visible: Some(OpenVisible::None),
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
}

#[cfg(test)]
mod tests {
    use super::free_name;

    #[test]
    fn scratch_names_follow_intellij() {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let first = free_name(folder.path(), "txt");
        assert_eq!(first, folder.path().join("scratch.txt"));
        std::fs::write(&first, "").expect("the first scratch");
        assert_eq!(
            free_name(folder.path(), "txt"),
            folder.path().join("scratch_1.txt")
        );
        assert_eq!(
            free_name(folder.path(), "json"),
            folder.path().join("scratch.json")
        );
    }
}
