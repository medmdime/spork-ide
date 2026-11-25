//! The log's right-hand column, as in IntelliJ: the selected commit's changed files as a tree
//! ("16 files", folders with their counts) above its details, the changes' toolbar and menu
//! (Vcs.Log.ChangesBrowser.Toolbar, Vcs.Log.ChangesBrowser.Popup), and the diff preview at the
//! bottom of the log or right of the changes. A double-click opens the diff as an editor tab,
//! at that file.

use std::collections::{BTreeMap, HashSet};

use gpui_kit::component::diff::{Diff, DiffFileStatus, DiffState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{
    ActiveTheme as _, Selectable as _, WindowExt as _, h_flex, h_resizable, resizable_panel,
    v_flex, v_resizable,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::history::ROW_HEIGHT;
use crate::log_toolbar::{GroupByDirectory, LogOption, header, option_item};
use crate::notices::Notification;
use crate::prefs::Prefs;
use crate::{RepoView, View};

actions!(
    spork,
    [
        /// Vcs.EditSource: the picked changed file in your editor.
        EditSource,
    ]
);

const INDENT: f32 = 18.;

#[derive(Default)]
struct Folder {
    folders: BTreeMap<String, Folder>,
    files: Vec<(String, String, DiffFileStatus)>,
}

impl Folder {
    fn count(&self) -> usize {
        self.files.len() + self.folders.values().map(Folder::count).sum::<usize>()
    }
}

pub(crate) enum Row {
    Folder {
        path: String,
        name: String,
        count: usize,
        depth: usize,
    },
    File {
        path: String,
        name: String,
        status: DiffFileStatus,
        depth: usize,
    },
}

impl Row {
    /// The row one level further in, under the repository's node.
    fn deeper(self) -> Self {
        match self {
            Row::Folder {
                path,
                name,
                count,
                depth,
            } => Row::Folder {
                path,
                name,
                count,
                depth: depth + 1,
            },
            Row::File {
                path,
                name,
                status,
                depth,
            } => Row::File {
                path,
                name,
                status,
                depth: depth + 1,
            },
        }
    }
}

/// The status git's `--name-status` letter stands for.
pub(crate) fn file_status(letter: char) -> DiffFileStatus {
    match letter {
        'A' => DiffFileStatus::Added,
        'D' => DiffFileStatus::Deleted,
        'R' => DiffFileStatus::Renamed,
        'C' => DiffFileStatus::Copied,
        _ => DiffFileStatus::Modified,
    }
}

/// The repository's node in the tree (Group By ▸ Repository), named so no path can clash.
const REPOSITORY_NODE: &str = "\0repository";

/// The files as tree rows: folders first, a folder holding only one folder shown as one row
/// ("src/main"), files by name.
pub(crate) fn rows(files: &[(String, DiffFileStatus)], collapsed: &HashSet<String>) -> Vec<Row> {
    let mut root = Folder::default();
    for (path, status) in files {
        let mut parts: Vec<&str> = path.split('/').collect();
        let name = parts.pop().unwrap_or_default();
        let folder = parts
            .into_iter()
            .fold(&mut root, |f, p| f.folders.entry(p.to_owned()).or_default());
        folder.files.push((name.to_owned(), path.clone(), *status));
    }
    let mut out = Vec::new();
    fn walk(
        folder: &Folder,
        prefix: &str,
        depth: usize,
        collapsed: &HashSet<String>,
        out: &mut Vec<Row>,
    ) {
        for (name, mut child) in &folder.folders {
            let mut name = name.clone();
            while child.files.is_empty()
                && let [(sub, grandchild)] = child.folders.iter().collect::<Vec<_>>()[..]
            {
                name = format!("{name}/{sub}");
                child = grandchild;
            }
            let path = format!("{prefix}{name}");
            out.push(Row::Folder {
                path: path.clone(),
                name,
                count: child.count(),
                depth,
            });
            if !collapsed.contains(&path) {
                walk(child, &format!("{path}/"), depth + 1, collapsed, out);
            }
        }
        let mut files: Vec<_> = folder.files.iter().collect();
        files.sort_by_key(|(name, _, _)| name.to_lowercase());
        for (name, path, status) in files {
            out.push(Row::File {
                path: path.clone(),
                name: name.clone(),
                status: *status,
                depth,
            });
        }
    }
    walk(&root, "", 1, collapsed, &mut out);
    out
}

fn files_label(n: usize) -> String {
    match n {
        1 => "1 file".into(),
        n => format!("{n} files"),
    }
}

impl RepoView {
    /// The right-hand column of the log for the selected commit (or the two compared).
    pub(crate) fn commit_side(&mut self, cx: &mut Context<Self>) -> Div {
        let details_shown = self.prefs.log_details;
        if self.selected.is_none() {
            // IntelliJ's empty panel: the toolbar and the files above, the details below
            let toolbar = self.changes_toolbar(cx);
            let muted = cx.theme().muted_foreground;
            let empty = |text: &'static str| {
                div()
                    .size_full()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(muted)
                    .child(text)
            };
            let files = v_flex()
                .size_full()
                .child(toolbar)
                .child(empty("Select commit to view changes"));
            if !details_shown {
                return files;
            }
            return v_flex().size_full().child(
                gpui_kit::component::v_resizable("commit-side")
                    .child(gpui_kit::component::resizable_panel().child(files))
                    .child(
                        gpui_kit::component::resizable_panel()
                            .size(px(230.))
                            .child(empty("Commit details")),
                    ),
            );
        }
        let files = self.commit_files_tree(cx);
        if !details_shown {
            return files;
        }
        let details = self.commit_summary(cx);
        v_flex().size_full().child(
            gpui_kit::component::v_resizable("commit-side")
                .child(gpui_kit::component::resizable_panel().child(files))
                .child(
                    gpui_kit::component::resizable_panel()
                        .size(px(230.))
                        .child(details),
                ),
        )
    }

    /// The log's panes: the commits (with the branches pane), the selected commit's changes and
    /// details, and Show Diff Preview's diff under them all or right of the changes.
    pub(crate) fn log_panes(&mut self, list: Div, cx: &mut Context<Self>) -> AnyElement {
        let side = self.commit_side(cx);
        let list = div().size_full().overflow_hidden().child(list);
        if !self.prefs.log_view.diff_preview {
            return h_resizable("log-split")
                .child(resizable_panel().child(list))
                .child(resizable_panel().size(px(240.)).child(side))
                .into_any_element();
        }
        // the changes beside it are the file list: the diff alone, at the picked file
        let preview = match self.selected {
            Some(_) => v_flex()
                .id("diff-preview")
                .role(Role::Group)
                .aria_label("Diff Preview")
                .size_full()
                .child(self.diff_bar("preview", &self.diff, cx))
                .child(self.diff_view(&self.diff, cx).flex_1().min_h_0())
                .into_any_element(),
            None => div()
                .id("diff-preview")
                .role(Role::Group)
                .aria_label("Diff Preview")
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child("Select commit to view changes")
                .into_any_element(),
        };
        match self.prefs.log_view.diff_preview_right {
            true => h_resizable("log-split-preview-right")
                .child(resizable_panel().child(list))
                .child(resizable_panel().size(px(240.)).child(side))
                .child(resizable_panel().size(px(420.)).child(preview))
                .into_any_element(),
            false => v_resizable("log-split-preview-bottom")
                .child(
                    resizable_panel().child(
                        h_resizable("log-split")
                            .child(resizable_panel().child(list))
                            .child(resizable_panel().size(px(240.)).child(side)),
                    ),
                )
                .child(resizable_panel().size(px(280.)).child(preview))
                .into_any_element(),
        }
    }

    /// Create Patch…: the commit's changes to the picked file, saved where you choose.
    fn create_patch_from_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(Ok(repo)), Some((commit, file))) = (&self.repo, self.picked_change()) else {
            return;
        };
        let name = format!("{}.patch", file.rsplit('/').next().unwrap_or(&file));
        let target = cx.prompt_for_new_path(&repo.root, Some(&name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(dest))) = target.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.run_op(
                    window,
                    cx,
                    move |root| {
                        let patch = spork_git::revision::paths_patch(
                            root,
                            &commit,
                            std::slice::from_ref(&file),
                        )?;
                        std::fs::write(&dest, patch).map_err(|e| spork_git::GitError::Other {
                            code: None,
                            stderr: e.to_string(),
                        })?;
                        Ok(dest)
                    },
                    |_, dest, window, cx| {
                        window.push_notification(
                            Notification::success(format!("Patch saved to {}", dest.display())),
                            cx,
                        )
                    },
                )
            })
            .ok();
        })
        .detach();
    }

    /// Opens the selected commit's diff as an editor tab, at `path` when given.
    pub(crate) fn open_commit_diff(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        self.open_diff_view(View::CommitDiff, cx);
        if let Some(path) = path {
            self.diff.update(cx, |s, cx| s.scroll_to_file(&path, cx));
            self.commit_file_selected = Some(path);
        }
        cx.notify();
    }

    /// `files` as the tree's rows: under their folders, or alone by name (Group By ▸ Directory).
    fn file_rows(&self, files: &[(String, DiffFileStatus)]) -> Vec<Row> {
        if self.prefs.log_changes_by_directory {
            return rows(files, &self.commit_tree_collapsed);
        }
        // Group By ▸ Directory off: the files alone, by name, their folder beside them
        let mut flat: Vec<Row> = files
            .iter()
            .map(|(path, status)| Row::File {
                name: path.rsplit('/').next().unwrap_or(path).to_owned(),
                path: path.clone(),
                status: *status,
                depth: 1,
            })
            .collect();
        flat.sort_by_key(|r| match r {
            Row::File { name, .. } => name.to_lowercase(),
            Row::Folder { name, .. } => name.to_lowercase(),
        });
        flat
    }

    /// Show Only Affected Changes: `files` under the Paths filter alone.
    fn affected(&self, files: &mut Vec<(String, DiffFileStatus)>) {
        if let (true, Some(under)) = (
            self.prefs.log_view.only_affected,
            self.log_filters.path.as_deref(),
        ) {
            let under = under.trim_end_matches('/');
            files.retain(|(path, _)| {
                path == under || path.strip_prefix(under).is_some_and(|r| r.starts_with('/'))
            });
        }
    }

    /// The selected merge's changes, loaded once per merge.
    fn merge_changes(&mut self, id: &str, parents: &[String], cx: &mut Context<Self>) {
        let (Some(Ok(repo)), loaded) = (&self.repo, &self.log_ui.merge) else {
            return;
        };
        if loaded.as_ref().is_some_and(|(at, _)| at == id) {
            return;
        }
        let (root, id, parents) = (repo.root.clone(), id.to_owned(), parents.to_vec());
        self.log_ui.merge = None;
        self.log_ui._merge_load = Some(cx.spawn(async move |this, cx| {
            let merge = id.clone();
            let changes = cx
                .background_spawn(async move { spork_git::merge_changes(&root, &merge, &parents) })
                .await
                .map_err(|e| e.to_string());
            this.update(cx, |this, cx| {
                this.log_ui.merge = Some((id, changes));
                cx.notify();
            })
            .ok();
        }));
    }

    /// The changes tree's rows, each with the merge parent its file's change is against: a
    /// commit's files; a merge's resolved files, or (Show Changes to Parents) each parent's
    /// "Changes to …" node with its files. `None` while a merge's changes load.
    fn log_changes_rows(&mut self, cx: &mut Context<Self>) -> Option<(Vec<(Row, usize)>, usize)> {
        let parents = match (&self.repo, self.selected) {
            (Some(Ok(repo)), Some(ix)) => repo
                .history
                .commits
                .get(ix)
                .map(|c| (c.id.clone(), c.parents.clone())),
            _ => None,
        };
        let Some((id, parents)) = parents.filter(|(_, p)| p.len() > 1) else {
            let mut files = self.diff_files(cx);
            self.affected(&mut files);
            let count = files.len();
            let tree = self.file_rows(&files).into_iter().map(|r| (r, 0)).collect();
            return Some((tree, count));
        };
        self.merge_changes(&id, &parents, cx);
        let changes = match &self.log_ui.merge {
            Some((_, Ok(changes))) => changes.clone(),
            Some((_, Err(_))) => spork_git::MergeChanges::default(),
            None => return None,
        };
        if !self.prefs.log_view.changes_to_parents {
            let mut files: Vec<(String, DiffFileStatus)> = changes
                .resolved
                .into_iter()
                .map(|p| (p, DiffFileStatus::Modified))
                .collect();
            self.affected(&mut files);
            let count = files.len();
            let tree = self.file_rows(&files).into_iter().map(|r| (r, 0)).collect();
            return Some((tree, count));
        }
        let mut tree = Vec::new();
        let mut count = 0;
        for (k, (parent, files)) in changes.parents.into_iter().enumerate() {
            let mut files: Vec<(String, DiffFileStatus)> = files
                .into_iter()
                .map(|(status, path)| (path, file_status(status)))
                .collect();
            self.affected(&mut files);
            count += files.len();
            let node = format!("\0parent{k}");
            let open = !self.commit_tree_collapsed.contains(&node);
            tree.push((
                Row::Folder {
                    path: node,
                    name: format!("Changes to {}", &parent[..8.min(parent.len())]),
                    count: files.len(),
                    depth: 1,
                },
                k,
            ));
            if open {
                tree.extend(self.file_rows(&files).into_iter().map(|r| (r.deeper(), k)));
            }
        }
        Some((tree, count))
    }

    fn commit_files_tree(&mut self, cx: &mut Context<Self>) -> Div {
        let changes = self.log_changes_rows(cx);
        let toolbar = self.changes_toolbar(cx);
        let theme = cx.theme();
        let (muted, hover, selected_bg) =
            (theme.muted_foreground, theme.list_hover, theme.list_active);
        let is_merge = match (&self.repo, self.selected) {
            (Some(Ok(repo)), Some(ix)) => repo
                .history
                .commits
                .get(ix)
                .is_some_and(|c| c.parents.len() > 1),
            _ => false,
        };
        let (mut tree, count) = match changes {
            Some((tree, count)) if !tree.is_empty() => (tree, count),
            empty => {
                let link = spork_workbench::controls::link(
                    "show-changes-to-parents",
                    "Show changes to parents",
                    cx,
                )
                .role(Role::Link)
                .aria_label("Show changes to parents")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_log_option(LogOption::ChangesToParents, window, cx)
                }));
                let filtered = self.prefs.log_view.only_affected && self.log_filters.path.is_some();
                let status = match (&empty, &self.details) {
                    (None, _) | (_, None) => "Loading…",
                    _ if filtered => "No changes that affect selected paths.",
                    _ if is_merge => "No merged conflicts.",
                    _ => "No changes",
                };
                return v_flex().size_full().child(toolbar).child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .text_color(muted)
                        .child(
                            div()
                                .id("changes-status")
                                .role(Role::Label)
                                .aria_label(status)
                                .child(status),
                        )
                        .when(is_merge && empty.is_some() && !filtered, |d| d.child(link)),
                );
            }
        };
        let by_directory = self.prefs.log_changes_by_directory;
        // Group By ▸ Repository: the repository's node above its files
        if self.prefs.log_view.by_repository {
            let open = !self.commit_tree_collapsed.contains(REPOSITORY_NODE);
            let name = match &self.repo {
                Some(Ok(repo)) => crate::repos::display_name(&repo.root),
                _ => String::new(),
            };
            tree = match open {
                true => tree.into_iter().map(|(r, k)| (r.deeper(), k)).collect(),
                false => Vec::new(),
            };
            tree.insert(
                0,
                (
                    Row::Folder {
                        path: REPOSITORY_NODE.to_owned(),
                        name,
                        count,
                        depth: 1,
                    },
                    0,
                ),
            );
        }
        let commit = self.selected_commit_id();
        let view = cx.entity();
        let splittable = self.changes_splittable(count);
        // no root row, as in IntelliJ: the top folders start at the left
        let row = |id: ElementId, depth: usize| {
            h_flex()
                .id(id)
                .h(ROW_HEIGHT)
                .flex_none()
                .mx_1()
                .pl(px(6. + INDENT * depth.saturating_sub(1) as f32))
                .pr_2()
                .gap_1()
                .rounded(px(4.))
        };
        let rows = tree
            .into_iter()
            .enumerate()
            .map(|(ix, (r, parent))| match r {
                Row::Folder {
                    path,
                    name,
                    count,
                    depth,
                } => {
                    let open = !self.commit_tree_collapsed.contains(&path);
                    row(("commit-tree-dir", ix).into(), depth)
                        .role(Role::TreeItem)
                        .aria_label(name.clone())
                        .aria_expanded(open)
                        .hover(move |r| r.bg(hover))
                        .child(
                            spork_workbench::icon(if open {
                                "general/chevronDown"
                            } else {
                                "general/chevronRight"
                            })
                            .size(px(16.))
                            .text_color(muted),
                        )
                        .child(spork_workbench::image("nodes/folder", cx))
                        .child(div().flex_none().pl_0p5().child(name))
                        .child(div().text_color(muted).child(files_label(count)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.commit_tree_collapsed.remove(&path) {
                                this.commit_tree_collapsed.insert(path.clone());
                            }
                            cx.notify();
                        }))
                        .into_any_element()
                }
                Row::File {
                    path,
                    name,
                    status,
                    depth,
                } => {
                    use spork_workbench::FileStatus;
                    let color = spork_workbench::file_status_color(
                        match status {
                            DiffFileStatus::Added | DiffFileStatus::Copied => FileStatus::Added,
                            DiffFileStatus::Deleted => FileStatus::Deleted,
                            DiffFileStatus::Renamed => FileStatus::Renamed,
                            _ => FileStatus::Modified,
                        },
                        cx,
                    );
                    let selected = self.commit_file_selected.as_ref() == Some(&path);
                    let menu = changes_menu(
                        &view,
                        commit.clone(),
                        path.clone(),
                        (is_merge, splittable),
                        &self.prefs,
                    );
                    let pick = path.clone();
                    row(("commit-tree-file", ix).into(), depth)
                        .role(Role::TreeItem)
                        .aria_label(name.clone())
                        .aria_selected(selected)
                        // a right-click picks the file the menu is about, as IntelliJ's tree does
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, _, _, cx| {
                                this.commit_file_selected = Some(pick.clone());
                                cx.notify();
                            }),
                        )
                        .when(selected, |r| r.bg(selected_bg))
                        .when(!selected, |r| r.hover(move |r| r.bg(hover)))
                        .child(div().w(px(16.)).flex_none())
                        .child(spork_workbench::image(
                            spork_workbench::file_icon(&name),
                            cx,
                        ))
                        .child(
                            div()
                                .min_w_0()
                                .pl_0p5()
                                .truncate()
                                .text_color(color)
                                .child(name),
                        )
                        .when(!by_directory, |r| {
                            let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir).to_owned();
                            r.child(div().min_w_0().truncate().text_color(muted).child(dir))
                        })
                        .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                            // a merge's file under another parent: the diff against that one
                            if this.merge_parent != parent
                                && let Some(ix) = this.selected
                            {
                                this.merge_parent = parent;
                                this.select(ix, cx);
                            }
                            this.commit_file_selected = Some(path.clone());
                            if event.click_count() == 2 || this.view == View::CommitDiff {
                                return this.open_commit_diff(Some(path.clone()), cx);
                            }
                            if this.prefs.log_view.diff_preview {
                                this.diff.update(cx, |s, cx| s.scroll_to_file(&path, cx));
                            }
                            cx.notify();
                        }))
                        .context_menu(menu)
                        .into_any_element()
                }
            });
        v_flex().size_full().child(toolbar).child(
            v_flex()
                .id("commit-tree")
                .flex_1()
                .min_h_0()
                .pb_1()
                .children(rows)
                .overflow_y_scrollbar(),
        )
    }

    /// IntelliJ's toolbar over a commit's changes: Show Diff, then Vcs.Log.ChangesBrowser.Toolbar
    /// (Revert Selected Changes, History Up to Here with a file picked, Show Only Affected Changes
    /// with a Paths filter, View Options); Expand All and Collapse All on the right.
    fn changes_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let (commit, file) = (self.selected_commit_id(), self.commit_file_selected.clone());
        let picked = commit.is_some() && file.is_some();
        let view = cx.entity();
        let prefs = self.prefs.clone();
        let only_affected = self.prefs.log_view.only_affected;
        h_flex()
            .h(px(32.))
            .flex_none()
            .px_1p5()
            .gap_0p5()
            .child(
                spork_workbench::ActionButton::new("commit-show-diff", "vcs/diff")
                    .tooltip("Show Diff (⌘D)")
                    .aria_label("Show Diff")
                    .disabled(commit.is_none())
                    .on_click(cx.listener(|this, _, _, cx| {
                        let path = this.commit_file_selected.clone();
                        this.open_commit_diff(path, cx)
                    })),
            )
            .child(
                h_flex()
                    .id("changes-browser-toolbar")
                    .role(Role::Toolbar)
                    .aria_label("VCS Log Changes Browser Toolbar")
                    .gap_0p5()
                    // Vcs.RepositoryChangesBrowserToolbar
                    .child(
                        spork_workbench::ActionButton::new("commit-revert-file", "vcs/revert")
                            .tooltip("Revert Selected Changes")
                            .aria_label("Revert Selected Changes")
                            .disabled(!picked)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.revert_selected_changes(window, cx)
                            })),
                    )
                    .when(picked, |bar| {
                        bar.child(
                            spork_workbench::ActionButton::new(
                                "commit-file-history",
                                "general/history",
                            )
                            .tooltip("History Up to Here")
                            .aria_label("History Up to Here")
                            .on_click(cx.listener(|this, _, _, cx| this.history_up_to_here(cx))),
                        )
                    })
                    .when(self.log_filters.path.is_some(), |bar| {
                        bar.child(
                            spork_workbench::ActionButton::new(
                                "commit-only-affected",
                                "nodes/folder",
                            )
                            .tooltip("Show Only Affected Changes")
                            .aria_label("Show Only Affected Changes")
                            .selected(only_affected)
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.toggle_log_option(LogOption::OnlyAffected, window, cx)
                                },
                            )),
                        )
                    })
                    .child(
                        spork_workbench::ActionButton::new("commit-presentation", "general/show")
                            .popup_marker()
                            .tooltip("View Options")
                            .aria_label("View Options")
                            .dropdown_menu(move |menu, window, cx| {
                                changes_view_options(menu, &view, &prefs, window, cx)
                            }),
                    ),
            )
            .child(div().flex_1())
            .child(
                spork_workbench::ActionButton::new("commit-expand", "general/expandAll")
                    .tooltip("Expand All")
                    .aria_label("Expand All")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.commit_tree_collapsed.clear();
                        cx.notify();
                    })),
            )
            .child(
                spork_workbench::ActionButton::new("commit-collapse", "general/collapseAll")
                    .tooltip("Collapse All")
                    .aria_label("Collapse All")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let all: Vec<String> = rows(&this.diff_files(cx), &HashSet::new())
                            .into_iter()
                            .filter_map(|r| match r {
                                Row::Folder { path, .. } => Some(path),
                                Row::File { .. } => None,
                            })
                            .chain([REPOSITORY_NODE.to_owned()])
                            .collect();
                        this.commit_tree_collapsed.extend(all);
                        cx.notify();
                    })),
            )
    }

    /// The picked file and the selected commit, for the actions on a commit's changes.
    fn picked_change(&self) -> Option<(String, String)> {
        Some((
            self.selected_commit_id()?,
            self.commit_file_selected.clone()?,
        ))
    }

    /// Revert Selected Changes: takes the commit's changes to the file back out of the work tree.
    fn revert_selected_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((commit, file)) = self.picked_change() else {
            return;
        };
        self.run_op(
            window,
            cx,
            move |root| {
                spork_git::revision::revert_paths(root, &commit, std::slice::from_ref(&file))
                    .map(|()| format!("Reverted the changes to {file}"))
            },
            |_, message, window, cx| window.push_notification(Notification::success(message), cx),
        )
    }

    /// Cherry-Pick Selected Changes: applies the commit's changes to the file to the work tree.
    fn apply_selected_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((commit, file)) = self.picked_change() else {
            return;
        };
        self.run_op(
            window,
            cx,
            move |root| {
                spork_git::revision::apply_paths(root, &commit, std::slice::from_ref(&file))
                    .map(|()| format!("Applied the changes to {file}"))
            },
            |_, message, window, cx| window.push_notification(Notification::success(message), cx),
        )
    }

    /// Whether the selected commit's changes can be split off it: it is on the current branch
    /// with only plain commits after it, isn't the first one, and has other changes left.
    fn changes_splittable(&self, files: usize) -> bool {
        let (Some(Ok(repo)), Some(id)) = (&self.repo, self.selected_commit_id()) else {
            return false;
        };
        let Some(head) = repo
            .refs
            .iter()
            .find(|r| r.is_head && r.kind == spork_git::RefKind::Local)
        else {
            return false;
        };
        let by_id: std::collections::HashMap<&str, &spork_git::Commit> = repo
            .history
            .commits
            .iter()
            .map(|c| (c.id.as_str(), c))
            .collect();
        let mut at = by_id.get(head.commit.as_str()).copied();
        while let Some(commit) = at.filter(|c| c.parents.len() == 1) {
            if commit.id == id {
                return files > 1;
            }
            at = by_id.get(commit.parents[0].as_str()).copied();
        }
        false
    }

    /// Drop Selected Changes: the picked file's changes out of the commit, the commits after
    /// it replayed.
    fn drop_selected_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((commit, file)) = self.picked_change() else {
            return;
        };
        self.run_outcome(window, cx, "Changes dropped".into(), move |root| {
            spork_git::drop_changes(root, &commit, std::slice::from_ref(&file))
        })
    }

    /// Extract Selected Changes to Separate Commit…: asks the new commit's message, then moves
    /// the picked file's changes into it, right after the commit.
    fn extract_selected_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some((commit, file)), Some(Ok(repo)), Some(ix)) =
            (self.picked_change(), &self.repo, self.selected)
        else {
            return;
        };
        let Some(author) = repo.history.commits.get(ix).map(|c| c.author.clone()) else {
            return;
        };
        let short: String = commit.chars().take(8).collect();
        let description = format!("Changes extracted from commit {short} by {author}");
        self.reword.update(cx, |s, cx| s.set_value("", window, cx));
        let (view, input) = (cx.entity(), self.reword.clone());
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, input, commit, file) =
                (view.clone(), input.clone(), commit.clone(), file.clone());
            dialog
                .title("New Commit Message")
                .description(
                    v_flex()
                        .id("new-commit-message")
                        .role(Role::Group)
                        .aria_label("New Commit Message")
                        .gap_2()
                        .child(description.clone())
                        .child(gpui_kit::component::input::Textarea::new(&input).h(px(140.))),
                )
                .confirm()
                .on_ok(move |_, window, cx| {
                    let message = input.read(cx).value().to_string();
                    let (commit, file) = (commit.clone(), file.clone());
                    view.update(cx, |this, cx| {
                        this.run_outcome(window, cx, "Changes extracted".into(), move |root| {
                            spork_git::extract_changes(
                                root,
                                &commit,
                                std::slice::from_ref(&file),
                                &message,
                            )
                        })
                    });
                    true
                })
        });
    }

    /// Get: the file as of the commit, in the work tree.
    fn get_change_version(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((commit, file)) = self.picked_change() else {
            return;
        };
        self.run_op(
            window,
            cx,
            move |root| spork_git::file::restore(root, &file, &commit).map(|()| file),
            |_, file, window, cx| {
                window.push_notification(
                    Notification::success(format!("Got {file} from the commit")),
                    cx,
                )
            },
        )
    }

    /// History Up to Here: the picked file's history, from the selected commit back.
    fn history_up_to_here(&mut self, cx: &mut Context<Self>) {
        if let Some((commit, path)) = self.picked_change() {
            self.open_file_from(path, false, commit, cx)
        }
    }

    /// Edit Source: the picked file, as it is now, in your editor.
    pub(crate) fn edit_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let in_log = matches!(self.view, View::History | View::CommitDiff);
        let (Some(Ok(repo)), Some(file), true) = (&self.repo, &self.commit_file_selected, in_log)
        else {
            // ⌘↓ and F4 elsewhere are theirs
            return cx.propagate();
        };
        let path = repo.root.join(file);
        self.open_in_editor(Some(path), window, cx)
    }

    /// Compare with Local / Compare Before with Local: the file as of the commit (or its parent)
    /// against the work tree.
    fn compare_change_with_local(
        &mut self,
        before: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((commit, file)) = self.picked_change() else {
            return;
        };
        let rev = if before {
            format!("{commit}^")
        } else {
            commit.clone()
        };
        let short: String = commit.chars().take(8).collect();
        let title = format!("{file} ({short}{} vs local)", if before { "^" } else { "" });
        self.run_op(
            window,
            cx,
            move |root| spork_git::file::diff_with_local(root, &rev, &file),
            move |this, patch, window, cx| {
                let diff = cx.new(|cx| DiffState::new([], cx));
                this.set_patch(&diff, &patch, cx);
                let height = (window.viewport_size().height * 0.75).max(px(300.));
                let muted = cx.theme().muted_foreground;
                let empty = patch.trim().is_empty();
                window.open_dialog(cx, move |dialog, _, _| {
                    let body = match empty {
                        true => div()
                            .text_color(muted)
                            .child("Contents are identical")
                            .into_any_element(),
                        false => div()
                            .h(height)
                            .child(Diff::new(&diff).size_full())
                            .into_any_element(),
                    };
                    dialog.width(px(1100.)).title(title.clone()).child(body)
                });
            },
        )
    }

    fn diff_files(&self, cx: &App) -> Vec<(String, DiffFileStatus)> {
        self.diff
            .read(cx)
            .files()
            .iter()
            .map(|f| (f.path().to_string(), f.status()))
            .collect()
    }

    /// Under the files: the commit's message, who made it and when, and its labels.
    fn commit_summary(&mut self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (muted, mono, link) = (
            theme.muted_foreground,
            theme.mono_font_family.clone(),
            theme.link,
        );
        let pane = v_flex();
        let (Some(ix), Some(Ok(repo))) = (self.selected, &self.repo) else {
            return v_flex();
        };
        let commit = repo.history.commits[ix].clone();
        let labels = repo
            .history
            .labels
            .get(&commit.id)
            .cloned()
            .unwrap_or_default();
        if let Some(other) = self.compare {
            let commits = &repo.history.commits;
            let (newer, older) = (&commits[ix.min(other)], &commits[ix.max(other)]);
            let short = |id: &str| id.chars().take(8).collect::<String>();
            let title = format!("Changes from {} to {}", short(&older.id), short(&newer.id));
            let actions = (self.compare_run.len() > 1).then(|| self.run_actions(cx));
            return v_flex()
                .size_full()
                .p(px(14.))
                .gap_2()
                .child(
                    div()
                        .font_family(mono)
                        .font_weight(FontWeight::BOLD)
                        .child(title),
                )
                .children(actions);
        }
        let Some(Ok(details)) = &self.details else {
            return v_flex();
        };
        let message = details.message.trim_end().to_owned();
        let (subject, body) = message.split_once('\n').unwrap_or((&message, ""));
        let when = {
            use chrono::{Local, TimeZone as _};
            Local
                .timestamp_opt(commit.time, 0)
                .single()
                .map(|t| t.format("on %d/%m/%Y at %H:%M").to_string())
                .unwrap_or_default()
        };
        let committed = match details.committer == commit.author {
            true => format!("committed on {}", details.committer_date),
            false => format!(
                "committed by {} <{}> on {}",
                details.committer, details.committer_email, details.committer_date
            ),
        };
        let kinds = |l: &crate::history::Label| match (l.is_head, l.kind) {
            (true, _) => spork_workbench::RefLabel::Head,
            (_, spork_git::RefKind::Local) => spork_workbench::RefLabel::Local,
            (_, spork_git::RefKind::Remote) => spork_workbench::RefLabel::Remote,
            (_, spork_git::RefKind::Tag) => spork_workbench::RefLabel::Tag,
        };
        let pane = pane
            .child(
                v_flex()
                    .px(px(14.))
                    .pt(px(14.))
                    .pb(px(10.))
                    .font_family(mono)
                    // issue keys and #123 link to the tracker, as IntelliJ links them
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .child(TextView::markdown(
                                "commit-subject",
                                self.link_issues(subject),
                            )),
                    )
                    .when(!body.trim().is_empty(), |d| {
                        d.child(div().pt_2().child(TextView::markdown(
                            "commit-body",
                            self.link_issues(body.trim()),
                        )))
                    }),
            )
            .child(
                v_flex()
                    .px(px(14.))
                    .pb(px(10.))
                    .gap_0p5()
                    .child(
                        div()
                            .child(format!(
                                "{} {} ",
                                &commit.id[..8.min(commit.id.len())],
                                commit.author
                            ))
                            .child(
                                div()
                                    .text_color(link)
                                    .child(format!("<{}>", details.author_email)),
                            )
                            .child(format!(" {when}"))
                            .flex()
                            .flex_wrap(),
                    )
                    .child(div().text_color(muted).child(committed)),
            )
            .when(!labels.is_empty(), |p| {
                p.child(
                    h_flex()
                        .px(px(14.))
                        .pb(px(14.))
                        .gap_1()
                        .flex_wrap()
                        .children(
                            labels
                                .iter()
                                .map(|l| crate::history::label_badge(&[kinds(l)], &l.name, cx)),
                        ),
                )
            });
        v_flex().size_full().child(
            div()
                .id("commit-summary")
                .size_full()
                .child(pane)
                .overflow_y_scrollbar(),
        )
    }
}

/// The changes' View Options (Vcs.Log.ChangesBrowser.PresentationSettings): Group By ▸, then
/// Configure Layout inline under its "Layout" heading. Filter By shows only with filters to
/// offer, which a log's changes have none of.
fn changes_view_options(
    menu: PopupMenu,
    view: &Entity<RepoView>,
    prefs: &Prefs,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let item = |label: &'static str, option: LogOption| option_item(view, label, option, prefs);
    let (preview, right) = (
        prefs.log_view.diff_preview,
        prefs.log_view.diff_preview_right,
    );
    let group_by = {
        let (view, prefs) = (view.clone(), prefs.clone());
        move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
            header(menu, "Group By")
                .item(
                    option_item(&view, "Directory", LogOption::ByDirectory, &prefs)
                        .action(Box::new(GroupByDirectory)),
                )
                .item(option_item(
                    &view,
                    "Repository",
                    LogOption::ByRepository,
                    &prefs,
                ))
        }
    };
    let location = {
        let view = view.clone();
        move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
            let place = |label: &'static str, on_right: bool| {
                let view = view.clone();
                PopupMenuItem::new(label)
                    .checked(right == on_right)
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| this.place_diff_preview(on_right, cx))
                    })
            };
            menu.item(place("Bottom", false)).item(place("Right", true))
        }
    };
    menu.submenu("Group By", window, cx, group_by)
        .separator()
        .map(|menu| header(menu, "Layout"))
        .item(item("Show Details", LogOption::Details))
        .item(item("Show Diff Preview", LogOption::DiffPreview))
        .when(preview, |menu| {
            menu.submenu("Diff Preview Location", window, cx, location)
        })
}

/// The menu on a commit's changed file (Vcs.Log.ChangesBrowser.Popup): Vcs.RepositoryChangesBrowserMenu,
/// then Show Changes to Parents for a merge. `splittable`: Extract and Drop can rewrite the
/// commit (`changes_splittable`).
fn changes_menu(
    view: &Entity<RepoView>,
    commit: Option<String>,
    path: String,
    (is_merge, splittable): (bool, bool),
    prefs: &Prefs,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    let (view, prefs) = (view.clone(), prefs.clone());
    move |menu, _, _| {
        let parents = option_item(
            &view,
            "Show Changes to Parents",
            LogOption::ChangesToParents,
            &prefs,
        );
        let run = |label: &'static str,
                   f: fn(&mut RepoView, &mut Window, &mut Context<RepoView>)| {
            let view = view.clone();
            PopupMenuItem::new(label)
                .on_click(move |_, window, cx| view.update(cx, |this, cx| f(this, window, cx)))
        };
        let open_version = {
            let (view, commit, path) = (view.clone(), commit.clone(), path.clone());
            PopupMenuItem::new("Open Repository Version").on_click(move |_, window, cx| {
                let Some(commit) = commit.clone() else { return };
                view.update(cx, |this, cx| {
                    this.open_version(commit, path.clone(), window, cx)
                })
            })
        };
        menu.item(run("Compare with Local", |this, w, cx| {
            this.compare_change_with_local(false, w, cx)
        }))
        .item(run("Compare Before with Local", |this, w, cx| {
            this.compare_change_with_local(true, w, cx)
        }))
        .item(run("Edit Source", RepoView::edit_source).action(Box::new(EditSource)))
        .item(open_version)
        .separator()
        .item(run(
            "Revert Selected Changes",
            RepoView::revert_selected_changes,
        ))
        .item(run(
            "Cherry-Pick Selected Changes",
            RepoView::apply_selected_changes,
        ))
        .item(
            run(
                "Extract Selected Changes to Separate Commit…",
                RepoView::extract_selected_changes,
            )
            .disabled(!splittable),
        )
        .item(run("Drop Selected Changes", RepoView::drop_selected_changes).disabled(!splittable))
        .item(run("Create Patch…", RepoView::create_patch_from_changes))
        .item(run("Get", RepoView::get_change_version))
        .item(run("History Up to Here", |this, _, cx| {
            this.history_up_to_here(cx)
        }))
        .when(is_merge, |menu| menu.item(parents))
    }
}

#[cfg(test)]
mod tests {
    use super::{Row, rows};
    use gpui_kit::component::diff::DiffFileStatus;
    use std::collections::HashSet;

    #[test]
    fn folders_join_single_chains_and_count_files() {
        let m = DiffFileStatus::Modified;
        let files = [
            ("apps/api/package.json".to_owned(), m),
            ("apps/mobile/app.json".to_owned(), m),
            ("src/org/build/A.kt".to_owned(), m),
            ("bun.lock".to_owned(), m),
        ];
        let shown: Vec<String> = rows(&files, &HashSet::new())
            .into_iter()
            .map(|r| match r {
                Row::Folder {
                    name, count, depth, ..
                } => format!("{depth}:{name}/{count}"),
                Row::File { name, depth, .. } => format!("{depth}:{name}"),
            })
            .collect();
        assert_eq!(
            shown,
            [
                "1:apps/2",
                "2:api/1",
                "3:package.json",
                "2:mobile/1",
                "3:app.json",
                "1:src/org/build/1",
                "2:A.kt",
                "1:bun.lock",
            ]
        );
    }
}
