//! Opening repositories: recent list, switcher, welcome screen, Open / Clone / New (docs/specs/M01, M02).

use std::path::{Path, PathBuf};

use crate::notices::Notification;
use gpui_kit::component::Selectable as _;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;

use spork_git::GitError;

use crate::util::ResultExt as _;
use crate::{RepoView, View};

const MAX_RECENT: usize = 15;

/// The settings folder the IDE gives Spork's views (`ide::init`), away from the standalone app's.
static CONFIG_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Set once, before anything is read or written: a later call keeps the first folder.
pub(crate) fn set_config_dir(dir: PathBuf) {
    CONFIG_DIR.get_or_init(|| dir);
}

/// Spork's settings folder: `~/Library/Application Support/Spork`, `%APPDATA%\Spork`, or
/// `$XDG_CONFIG_HOME/spork` (default `~/.config/spork`); inside the IDE, the one it gave.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = CONFIG_DIR.get() {
        return Some(dir.clone());
    }
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(target_os = "macos") {
        Some(env("HOME")?.join("Library/Application Support/Spork"))
    } else if cfg!(windows) {
        Some(env("APPDATA")?.join("Spork"))
    } else {
        Some(
            env("XDG_CONFIG_HOME")
                .or_else(|| Some(env("HOME")?.join(".config")))?
                .join("spork"),
        )
    }
}

fn recent_file() -> Option<PathBuf> {
    Some(config_dir()?.join("recent.txt"))
}

fn load_from(file: &Path) -> Vec<PathBuf> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Moves `path` to the top of the list in `file` (one path per line), keeping at most 15.
fn remember_in(file: &Path, path: &Path) -> std::io::Result<()> {
    let mut list = load_from(file);
    list.retain(|p| p != path);
    list.insert(0, path.to_path_buf());
    list.truncate(MAX_RECENT);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text: String = list.iter().map(|p| format!("{}\n", p.display())).collect();
    std::fs::write(file, text)
}

/// Recently opened repositories that still exist, newest first.
pub fn recent() -> Vec<PathBuf> {
    recent_file()
        .map(|f| load_from(&f))
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.join(".git").exists())
        .collect()
}

pub fn remember(path: &Path) {
    if let Some(file) = recent_file() {
        remember_in(&file, path).log_err();
    }
}

/// Takes `path` off the recent list (the Welcome window's "Remove from Recent Projects").
pub(crate) fn forget(path: &Path) {
    if let Some(file) = recent_file() {
        forget_in(&file, path).log_err();
    }
}

fn forget_in(file: &Path, path: &Path) -> std::io::Result<()> {
    let list: String = load_from(file)
        .into_iter()
        .filter(|p| p != path)
        .map(|p| format!("{}\n", p.display()))
        .collect();
    std::fs::write(file, list)
}

/// `path` with the home folder shown as `~`, as IntelliJ shows project paths.
pub(crate) fn tilde(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home
        .as_deref()
        .and_then(|home| path.strip_prefix(home).ok())
    {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// The branch checked out in the repository at `root`, read from its HEAD file without running
/// git (a worktree's `.git` file points at its HEAD); None when detached or unreadable.
pub(crate) fn head_branch(root: &Path) -> Option<String> {
    let dot_git = root.join(".git");
    let git_dir = match std::fs::read_to_string(&dot_git) {
        Ok(link) => root.join(link.strip_prefix("gitdir:")?.trim()),
        Err(_) => dot_git,
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    branch_of_head(&head).map(str::to_owned)
}

/// "ref: refs/heads/main" → main; a detached HEAD (a hash) → None.
fn branch_of_head(head: &str) -> Option<&str> {
    head.trim().strip_prefix("ref: refs/heads/")
}

pub(crate) fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

impl RepoView {
    /// Shows another repository in this window, starting fresh.
    pub(crate) fn switch_repo(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.requested = path;
        self.repo = None;
        self.selected = None;
        self.details = None;
        self.change_selected = None;
        self.rebase_plan = None;
        self.view = View::History;
        self._watcher = None;
        self.reload(cx);
    }

    /// Native folder picker, then `then(folder)`.
    pub(crate) fn pick_folder(
        &mut self,
        prompt: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, PathBuf, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(mut picked))) = paths.await
                && let Some(folder) = picked.pop()
            {
                this.update_in(cx, |this, window, cx| then(this, folder, window, cx))
                    .ok();
            }
        })
        .detach();
    }

    pub(crate) fn open_repo_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pick_folder(
            "Open",
            window,
            cx,
            |this, folder, window, cx| match spork_git::toplevel(&folder) {
                Err(GitError::NotARepo) => this.offer_init(folder, window, cx),
                _ => this.switch_repo(folder, cx),
            },
        );
    }

    /// The folder picked to open isn't a repository: offer to create one there (M01 §3).
    fn offer_init(&mut self, folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, folder) = (view.clone(), folder.clone());
            dialog
                .title("Not a git repository")
                .description(format!(
                    "{} isn't a git repository. Create one here? Its files stay as they are, untracked until you commit them.",
                    folder.display()
                ))
                .confirm()
                .ok_text("Create repository")
                .on_ok(move |_, window, cx| {
                    let (target, folder) = (folder.clone(), folder.clone());
                    view.update(cx, |this, cx| {
                        this.run_op(
                            window,
                            cx,
                            move |_| spork_git::init(&target),
                            move |this, _, window, cx| {
                                window.push_notification(Notification::success("Repository created"), cx);
                                this.switch_repo(folder, cx);
                            },
                        )
                    });
                    true
                })
        });
    }

    pub(crate) fn new_repo_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pick_folder(
            "Create repository here",
            window,
            cx,
            |this, folder, window, cx| {
                let target = folder.clone();
                this.run_op(
                    window,
                    cx,
                    move |_| spork_git::init(&target),
                    move |this, _, window, cx| {
                        window.push_notification(Notification::success("Repository created"), cx);
                        this.switch_repo(folder, cx);
                    },
                );
            },
        );
    }

    /// IntelliJ's project widget: the repository's badge and name; a click opens the project
    /// popup under it.
    pub(crate) fn repo_switcher(&self, name: String, cx: &mut Context<Self>) -> impl IntoElement {
        let slot = self
            .tab_root()
            .as_deref()
            .map_or(0, spork_workbench::shell::project_slot);
        let chip = spork_workbench::ToolbarDropdown::new("repo-switcher")
            .selected(self.project_popup_open)
            .child(spork_workbench::shell::project_badge(&name, slot))
            .child(name)
            .on_click(cx.listener(|this, _, window, cx| this.toggle_project_popup(window, cx)));
        let bounds = self.project_chip.clone();
        // IntelliJ opens it 1 px under the toolbar, its edge 5 px left of the widget's
        let popup = self
            .project_popup
            .as_ref()
            .filter(|_| self.project_popup_open)
            .map(|(popup, _)| {
                div().absolute().top_full().left_0().child(
                    deferred(
                        anchored()
                            .snap_to_window_with_margin(px(8.))
                            .child(div().mt(px(6.)).ml(px(-5.)).child(popup.clone())),
                    )
                    .with_priority(1),
                )
            });
        div()
            .relative()
            .child(chip)
            .child(
                canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .children(popup)
    }

    pub(crate) fn toggle_project_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project_popup_open {
            self.project_popup_open = false;
            return cx.notify();
        }
        let popup = match &self.project_popup {
            Some((popup, _)) => popup.clone(),
            None => {
                let (view, trigger) = (cx.entity().downgrade(), self.project_chip.clone());
                let popup =
                    cx.new(|cx| crate::project_popup::ProjectPopup::new(view, trigger, window, cx));
                let subscription = cx.subscribe(&popup, |this, _, _: &DismissEvent, cx| {
                    this.project_popup_open = false;
                    cx.notify();
                });
                self.project_popup = Some((popup.clone(), subscription));
                popup
            }
        };
        let projects = self.popup_projects(cx.entity_id(), window, cx);
        popup.update(cx, |popup, cx| popup.reset(projects, window, cx));
        self.project_popup_open = true;
        cx.notify();
    }

    /// Shown when no repository is open (or the folder isn't one): IntelliJ's empty welcome
    /// page, then the recent repositories.
    pub(crate) fn welcome(
        &self,
        problem: Option<String>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        use crate::welcome::{big_button, empty_state};
        let theme = cx.theme();
        let (muted, hover) = (theme.muted_foreground, theme.list_hover);
        let buttons = [
            big_button(
                "tab-new",
                "welcome/createNewProjectTab",
                &["New Project"],
                cx,
            )
            .on_click(cx.listener(|this, _, window, cx| this.new_repo_dialog(window, cx))),
            big_button("tab-open", "welcome/open", &["Open"], cx)
                .on_click(cx.listener(|this, _, window, cx| this.open_repo_dialog(window, cx))),
            big_button(
                "tab-clone",
                "welcome/fromVCSTab",
                &["Clone", "Repository"],
                cx,
            )
            .on_click(cx.listener(|this, _, window, cx| this.clone_dialog(window, cx))),
        ];
        let recent = self.recent.iter().take(8).enumerate().map(|(ix, path)| {
            let target = path.clone();
            h_flex()
                .id(("recent", ix))
                .w(px(420.))
                .p(px(4.))
                .gap_2()
                .rounded(px(4.))
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .on_click(cx.listener(move |this, _, _, cx| this.switch_repo(target.clone(), cx)))
                .child(spork_workbench::shell::project_badge(
                    &display_name(path),
                    spork_workbench::shell::project_slot(path),
                ))
                .child(display_name(path))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(muted)
                        .child(tilde(path)),
                )
        });
        empty_state(problem, buttons, cx).child(v_flex().mt(px(40.)).gap_0p5().children(recent))
    }
}

impl RepoView {
    pub(crate) fn reveal_repo(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(Ok(repo)) = &self.repo {
            cx.reveal_path(&repo.root);
        }
    }

    /// Opens the terminal chosen in Preferences (or the system's) at the repository root (M16 §4).
    /// Opens `path` (the repository when `None`) in the editor from Preferences.
    #[allow(
        clippy::disallowed_methods,
        reason = "starts the user's editor without waiting for it"
    )]
    pub(crate) fn open_in_editor(
        &mut self,
        path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(repo)) = &self.repo else { return };
        let path = path.unwrap_or_else(|| repo.root.clone());
        // the IDE is the editor: its own tabs, the repository being its project already
        if self.ide {
            if path.is_file() {
                cx.emit(crate::ide::IdeEvent::OpenFile { path, line: None });
            }
            return;
        }
        let Some(mut cmd) = self.prefs.editor_command(&path) else {
            return window.push_notification(
                Notification::info("Choose your editor in Preferences first"),
                cx,
            );
        };
        if let Err(e) = cmd.spawn() {
            window.push_notification(
                Notification::error(format!("Couldn't open the editor: {e}")),
                cx,
            );
        }
    }

    #[allow(
        clippy::disallowed_methods,
        reason = "starts the user's terminal without waiting for it"
    )]
    pub(crate) fn open_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let root = repo.root.clone();
        let mut cmd = self.prefs.terminal_command(&root);
        if let Err(e) = cmd.spawn() {
            window.push_notification(
                Notification::error(format!("Couldn't open a terminal: {e}")),
                cx,
            );
        }
    }
}

/// Repositories under `roots`, looking `depth` folders deep; a repository's own subfolders,
/// hidden folders, `node_modules` and `target` are skipped (M01 §3 source folders).
pub(crate) fn scan(roots: &[PathBuf], depth: usize) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if dir.join(".git").exists() {
            return out.push(dir.to_path_buf());
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        if depth == 0 {
            return;
        }
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                walk(&entry.path(), depth - 1, out);
            }
        }
    }
    let mut out = Vec::new();
    for root in roots {
        walk(root, depth, &mut out);
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    // explicit: `super::*` would pull in gpui's `test` attribute, which shadows #[test]
    use super::{MAX_RECENT, branch_of_head, forget_in, load_from, remember_in, scan};
    use std::path::{Path, PathBuf};

    #[test]
    fn forgetting_takes_one_project_off() {
        let file = std::env::temp_dir().join(format!("spork-forget-{}.txt", std::process::id()));
        for p in ["/a", "/b", "/c"] {
            remember_in(&file, Path::new(p)).unwrap();
        }
        forget_in(&file, Path::new("/b")).unwrap();
        assert_eq!(load_from(&file), [PathBuf::from("/c"), PathBuf::from("/a")]);
        std::fs::remove_file(file).ok();
    }

    #[test]
    fn head_names_the_branch_unless_detached() {
        assert_eq!(branch_of_head("ref: refs/heads/feat/x\n"), Some("feat/x"));
        assert_eq!(
            branch_of_head("4f2c1d0e9b8a7f6e5d4c3b2a1f0e9d8c7b6a5f4e\n"),
            None
        );
    }

    #[test]
    fn recent_list_moves_to_top_dedupes_and_caps() {
        let tmp = std::env::temp_dir().join(format!("spork-recent-{}", std::process::id()));
        let file = tmp.join("nested/recent.txt");
        for i in 0..20 {
            remember_in(&file, Path::new(&format!("/repos/{i}"))).unwrap();
        }
        remember_in(&file, Path::new("/repos/7")).unwrap();
        let list = load_from(&file);
        assert_eq!(list.len(), MAX_RECENT);
        assert_eq!(list[0], PathBuf::from("/repos/7"));
        assert_eq!(
            list.iter()
                .filter(|p| p.as_path() == Path::new("/repos/7"))
                .count(),
            1
        );
        assert_eq!(list[1], PathBuf::from("/repos/19"));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn scans_source_folders() {
        let root = std::env::temp_dir().join(format!("spork-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in [
            "a/.git",
            "a/sub/.git",
            "group/b/.git",
            "node_modules/c/.git",
            "deep/x/y/z/.git",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let found = scan(std::slice::from_ref(&root), 2);
        assert_eq!(found, [root.join("a"), root.join("group/b")]);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
