//! The Project tool window, as in IntelliJ: the repository's folders and files as a tree under
//! its root, names in the color of their change, file-type icons. A folder is read when it is
//! opened, and read again after the working copy changes.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::RepoView;
use crate::history::ROW_HEIGHT;

const INDENT: f32 = 18.;

/// What a menu item does to the view.
type MenuAction = Box<dyn Fn(&mut RepoView, &mut Window, &mut Context<RepoView>)>;

#[derive(Default)]
pub(crate) struct ProjectTree {
    /// Folders open, relative to the root; the root is `""`.
    open: HashSet<PathBuf>,
    /// What each folder read holds: `(name, is a folder)`, folders first.
    read: HashMap<PathBuf, Vec<(String, bool)>>,
    selected: Option<PathBuf>,
    scroll: UniformListScrollHandle,
    /// The root starts open: set once it has been opened the first time.
    started: bool,
}

impl ProjectTree {
    /// Forget what the folders held: they are read again when next shown.
    pub(crate) fn invalidate(&mut self) {
        self.read.clear();
    }
}

struct Row {
    rel: PathBuf,
    depth: usize,
    dir: bool,
    name: String,
}

/// A folder's entries, as IntelliJ sorts them: folders first, then files, by name ignoring case.
/// `.git` is left out.
fn read_folder(path: &Path) -> Vec<(String, bool)> {
    let mut entries: Vec<(String, bool)> = std::fs::read_dir(path)
        .map(|dir| {
            dir.flatten()
                .map(|e| {
                    let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
                    (e.file_name().to_string_lossy().into_owned(), is_dir)
                })
                .filter(|(name, _)| name != ".git")
                .collect()
        })
        .unwrap_or_default();
    entries.sort_by_key(|(name, is_dir)| (!is_dir, name.to_lowercase()));
    entries
}

impl ProjectTree {
    fn rows(&mut self, root: &Path) -> Vec<Row> {
        if !self.started {
            self.open.insert(PathBuf::new());
            self.started = true;
        }
        let name = root.file_name().map_or_else(
            || root.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let mut rows = vec![Row {
            rel: PathBuf::new(),
            depth: 0,
            dir: true,
            name,
        }];
        if self.open.contains(Path::new("")) {
            self.walk(root, Path::new(""), 1, &mut rows);
        }
        rows
    }

    fn walk(&mut self, root: &Path, dir: &Path, depth: usize, rows: &mut Vec<Row>) {
        let entries = self
            .read
            .entry(dir.to_path_buf())
            .or_insert_with(|| read_folder(&root.join(dir)))
            .clone();
        for (name, is_dir) in entries {
            let rel = dir.join(&name);
            let open = is_dir && self.open.contains(&rel);
            rows.push(Row {
                rel: rel.clone(),
                depth,
                dir: is_dir,
                name,
            });
            if open {
                self.walk(root, &rel, depth + 1, rows);
            }
        }
    }
}

impl RepoView {
    /// The Project tool window's content.
    pub(crate) fn project_window(&mut self, cx: &mut Context<Self>) -> Div {
        let Some(Ok(repo)) = &self.repo else {
            return v_flex();
        };
        let count = self.project.rows(&repo.root.clone()).len();
        let collapse = Button::new("project-collapse")
            .ghost()
            .small()
            .icon(spork_workbench::icon("general/collapseAll"))
            .tooltip("Collapse All")
            .on_click(cx.listener(|this, _, _, cx| {
                this.project.open.retain(|p| p.as_os_str().is_empty());
                cx.notify();
            }));
        let list = uniform_list(
            "project-tree",
            count,
            cx.processor(|this, range, _, cx| this.project_rows(range, cx)),
        );
        v_flex()
            .size_full()
            .child(
                spork_workbench::shell::tool_window_header("Project").child(
                    spork_workbench::shell::tool_window_actions()
                        .child(collapse)
                        .child(self.hide_left_button(crate::LeftTool::Project, cx)),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(crate::history::scrolled(list, &self.project.scroll)),
            )
    }

    fn project_rows(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(Ok(repo)) = &self.repo else {
            return Vec::new();
        };
        let root = repo.root.clone();
        // each changed file's color, and its folders shown as modified
        let mut colors: HashMap<PathBuf, Hsla> = HashMap::new();
        let mut changed_dirs: HashSet<PathBuf> = HashSet::new();
        for entry in repo.status.iter().filter(|e| !e.ignored()) {
            let (_, color) = crate::changes::status_letter(entry, false, cx);
            let path = PathBuf::from(&entry.path);
            changed_dirs.extend(path.ancestors().skip(1).map(Path::to_path_buf));
            colors.insert(path, color);
        }
        let modified =
            spork_workbench::file_status_color(spork_workbench::FileStatus::Modified, cx);
        let rows = self.project.rows(&root);
        let theme = cx.theme();
        let (muted, hover, selected_bg) =
            (theme.muted_foreground, theme.list_hover, theme.list_active);
        range
            .filter_map(|ix| rows.get(ix).map(|row| (ix, row)))
            .map(|(ix, row)| {
                let open = row.dir && self.project.open.contains(&row.rel);
                let selected = self.project.selected.as_ref() == Some(&row.rel);
                let color = colors.get(&row.rel).copied().or_else(|| {
                    (row.dir && !row.rel.as_os_str().is_empty() && changed_dirs.contains(&row.rel))
                        .then_some(modified)
                });
                let icon = match row.dir {
                    true => spork_workbench::image("nodes/folder", cx),
                    false => spork_workbench::image(spork_workbench::file_icon(&row.name), cx),
                };
                let root_path = (row.depth == 0).then(|| crate::repos::tilde(&root));
                let (rel, abs) = (row.rel.clone(), root.join(&row.rel));
                let (is_dir, toggle_rel) = (row.dir, row.rel.clone());
                let chevron = row.dir.then(|| {
                    div()
                        .id(("project-chevron", ix))
                        .flex_none()
                        .text_color(muted)
                        .child(
                            spork_workbench::icon(if open {
                                "general/chevronDown"
                            } else {
                                "general/chevronRight"
                            })
                            .size(px(16.)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_project_folder(&toggle_rel, cx)
                        }))
                });
                h_flex()
                    .id(("project-row", ix))
                    .h(ROW_HEIGHT)
                    .mx_1()
                    .pl(px(6. + INDENT * row.depth as f32))
                    .pr_2()
                    .gap_1()
                    .rounded(px(4.))
                    .when(selected, |r| r.bg(selected_bg))
                    .when(!selected, |r| r.hover(move |r| r.bg(hover)))
                    .child(match chevron {
                        Some(c) => c.into_any_element(),
                        None => div().w(px(16.)).flex_none().into_any_element(),
                    })
                    .child(icon)
                    .child(
                        div()
                            .flex_none()
                            .pl_0p5()
                            .when(row.depth == 0, |d| d.font_weight(FontWeight::SEMIBOLD))
                            .when_some(color, |d, c| d.text_color(c))
                            .child(row.name.clone()),
                    )
                    .children(
                        root_path.map(|p| div().min_w_0().truncate().text_color(muted).child(p)),
                    )
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        this.project.selected = Some(rel.clone());
                        if event.click_count() == 2 {
                            match is_dir {
                                true => this.toggle_project_folder(&rel, cx),
                                // IntelliJ opens it in its editor; Spork hands it to yours
                                false => this.open_in_editor(Some(abs.clone()), window, cx),
                            }
                        }
                        cx.notify();
                    }))
                    .context_menu({
                        let menu = project_menu(
                            cx.entity(),
                            row.rel.clone(),
                            root.join(&row.rel),
                            row.dir,
                        );
                        move |m, window, cx| menu(m, window, cx)
                    })
                    .into_any_element()
            })
            .collect()
    }

    fn toggle_project_folder(&mut self, rel: &Path, cx: &mut Context<Self>) {
        if !self.project.open.remove(rel) {
            self.project.open.insert(rel.to_path_buf());
        }
        cx.notify();
    }
}

/// A file's or folder's right-click menu in the Project tool window.
fn project_menu(
    view: Entity<RepoView>,
    rel: PathBuf,
    abs: PathBuf,
    dir: bool,
) -> impl Fn(
    gpui_kit::component::menu::PopupMenu,
    &mut Window,
    &mut Context<gpui_kit::component::menu::PopupMenu>,
) -> gpui_kit::component::menu::PopupMenu
+ 'static {
    move |menu, window, cx| {
        let path = rel.to_string_lossy().into_owned();
        let item = |label: &str, f: MenuAction| {
            let view = view.clone();
            PopupMenuItem::new(label.to_owned())
                .on_click(move |_, window, cx| view.update(cx, |this, cx| f(this, window, cx)))
        };
        let file = !dir && !path.is_empty();
        let copy = |label: &str, text: String| {
            PopupMenuItem::new(label.to_owned()).on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
            })
        };
        let absolute = abs.display().to_string();
        let name = abs
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let (relative, edit) = (path.clone(), abs.clone());
        let (history, blame) = (path.clone(), path);
        let (reveal, open) = (abs.clone(), abs.clone());
        // IntelliJ's order and words (docs/reference-spec.md §9), for what a git client can do
        menu.submenu("Copy Path/Reference…", window, cx, move |menu, _, _| {
            menu.item(copy("Absolute Path", absolute.clone()))
                .item(copy("File Name", name.clone()))
                .separator()
                .item(copy("Path From Repository Root", relative.clone()))
        })
        .when(file, |menu| {
            menu.item(item(
                "Jump to Source",
                Box::new(move |this, window, cx| {
                    this.open_in_editor(Some(edit.clone()), window, cx)
                }),
            ))
        })
        .separator()
        .submenu("Open In", window, cx, move |menu, _, _| {
            let (reveal, open) = (reveal.clone(), open.clone());
            menu.item(
                PopupMenuItem::new("Finder").on_click(move |_, _, cx| cx.reveal_path(&reveal)),
            )
            .item(
                PopupMenuItem::new("Associated Application")
                    .on_click(move |_, _, cx| cx.open_with_system(&open)),
            )
            .item(PopupMenuItem::new("Terminal").on_click(|_, window, cx| {
                window.dispatch_action(Box::new(crate::shell::OpenTerminal), cx)
            }))
        })
        .separator()
        .when(file, |menu| {
            menu.item(item(
                "Annotate with Git Blame",
                Box::new(move |this, _, cx| this.open_file(blame.clone(), true, cx)),
            ))
            .item(item(
                "Show History",
                Box::new(move |this, _, cx| this.open_file(history.clone(), false, cx)),
            ))
        })
        .item(item(
            "Reload from Disk",
            Box::new(|this, _, cx| {
                this.project.invalidate();
                cx.notify()
            }),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::read_folder;

    #[test]
    fn folders_first_then_files_by_name_without_git() {
        let dir = std::env::temp_dir().join(format!("spork-project-{}", std::process::id()));
        for d in [".git", "src", "Docs"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        for f in ["b.txt", "A.md", ".env"] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        let names: Vec<String> = read_folder(&dir).into_iter().map(|(n, _)| n).collect();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(names, ["Docs", "src", ".env", "A.md", "b.txt"]);
    }
}
