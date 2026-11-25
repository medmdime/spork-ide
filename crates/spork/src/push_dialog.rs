//! The reference app's Push Commits dialog (`Vcs.Push`, docs/reference-app/dialogs.md "Push…"): where the branch
//! goes, the commits it takes along and their files, Push tags, then Cancel and Push, whose
//! arrow offers Force Push.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui_kit::component::diff::{Diff, DiffFileStatus, DiffState};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Selectable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_git::remote::PushOptions;
use spork_git::{Commit, GitError, RefKind};
use spork_workbench::Tokens;
use spork_workbench::controls::{DropDownLink, OptionButton, combo, labeled_check, link};

use crate::RepoView;
use crate::commit_side::{Row, file_status, rows};
use crate::notices::Notification;
use crate::util::ResultExt as _;

/// The tree's indent per level and the dialog's size, as the reference app shows it (800 × 527).
const INDENT: f32 = 18.;
const WIDTH: f32 = 800.;
const BODY_HEIGHT: f32 = 420.;

/// Push tags: `All` (`--tags`) or `Current Branch` (`--follow-tags`).
const TAG_MODES: [&str; 2] = ["All", "Current Branch"];

/// What is selected on the left: the repository's row (all its commits) or one commit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Picked {
    Repository,
    Commit(usize),
}

pub(crate) struct PushDialog {
    view: WeakEntity<RepoView>,
    root: PathBuf,
    repository: String,
    branch: String,
    remotes: Vec<String>,
    remote: String,
    target: Entity<InputState>,
    editing_target: bool,
    /// Whether the branch tracks one already: a push of one that doesn't sets its upstream.
    has_upstream: bool,
    /// The commits a push sends and whether the target branch exists; `None` while loading.
    outgoing: Option<Result<(Vec<Commit>, bool), String>>,
    picked: Picked,
    files: Vec<(String, DiffFileStatus)>,
    /// Each file's newest picked commit that changed it: the one its diff comes from.
    file_commits: HashMap<String, String>,
    file: Option<String>,
    collapsed: HashSet<String>,
    by_directory: bool,
    preview: bool,
    diff: Entity<DiffState>,
    push_tags: bool,
    tag_mode: usize,
    _load: Option<Task<()>>,
    _files: Option<Task<()>>,
    _patch: Option<Task<()>>,
    _subscription: Subscription,
}

impl RepoView {
    /// Push… (⇧⌘K): the current branch's Push Commits dialog.
    pub(crate) fn push_commits_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.current_branch() {
            Some(branch) => self.push_dialog(branch, None, window, cx),
            None => window.push_notification(Notification::info("Check out a branch to push"), cx),
        }
    }

    /// Commit and Push…, once committed: the Push dialog, as Settings › Git › Show Push dialog
    /// for Commit and Push has it (only for a protected branch with its second box), else the
    /// push straight away.
    pub(crate) fn push_after_commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = crate::settings_window::Store::load();
        let dialog = settings.get("git.pushDialog") != Some("false");
        let protected_only = settings.get("git.pushDialogProtected") == Some("true");
        let to = self
            .current_branch()
            .and_then(|branch| self.upstream_of(&branch));
        let protected = to
            .and_then(|up| self.split_remote_branch(&up))
            .is_some_and(|(_, target)| PushDialog::protected(&target));
        match dialog && (!protected_only || protected) {
            true => self.push_commits_dialog(window, cx),
            false => self.remote_op(crate::RemoteOp::Push, window, cx),
        }
    }

    /// Push… (⇧⌘K) and a branch's Push…: the reference app's Push Commits dialog for `branch`, to `to` (a
    /// remote and branch) when given, else to its upstream, else the same name on the default
    /// remote.
    pub(crate) fn push_dialog(
        &mut self,
        branch: String,
        to: Option<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(repo)) = &self.repo else { return };
        if repo.remotes.is_empty() {
            return window
                .push_notification(Notification::error("This repository has no remote."), cx);
        }
        let upstream = repo
            .refs
            .iter()
            .find(|r| r.kind == RefKind::Local && r.name == branch)
            .and_then(|r| r.upstream.clone());
        let (remote, target) = to
            .or_else(|| {
                upstream
                    .as_deref()
                    .and_then(|up| self.split_remote_branch(up))
            })
            .unwrap_or_else(|| (self.default_remote().unwrap_or_default(), branch.clone()));
        let (root, remotes) = (
            repo.root.clone(),
            repo.remotes.iter().map(|r| r.name.clone()).collect(),
        );
        let repository = crate::repos::display_name(&root);
        let view = cx.entity().downgrade();
        let dialog = cx.new(|cx| {
            let target = cx.new(|cx| InputState::new(window, cx).default_value(target));
            let subscription =
                cx.subscribe_in(&target, window, |this: &mut PushDialog, _, event, _, cx| {
                    match event {
                        InputEvent::PressEnter { .. } | InputEvent::Blur => {
                            this.editing_target = false;
                            this.load(cx);
                        }
                        _ => {}
                    }
                });
            let mut dialog = PushDialog {
                view,
                root,
                repository,
                branch,
                remotes,
                remote,
                target,
                editing_target: false,
                has_upstream: upstream.is_some(),
                outgoing: None,
                picked: Picked::Repository,
                files: Vec::new(),
                file_commits: HashMap::new(),
                file: None,
                collapsed: HashSet::new(),
                by_directory: true,
                preview: true,
                diff: cx.new(|cx| DiffState::new([], cx)),
                push_tags: false,
                tag_mode: 0,
                _load: None,
                _files: None,
                _patch: None,
                _subscription: subscription,
            };
            dialog.load(cx);
            dialog
        });
        let title = SharedString::from(format!("Push Commits to {}", dialog.read(cx).repository));
        window.open_dialog(cx, move |modal, _, _| {
            modal
                .title(crate::git_menu::dialog_title(&title))
                .w(px(WIDTH))
                .close_button(false)
                .child(dialog.clone())
        });
    }
}

impl PushDialog {
    fn target_name(&self, cx: &App) -> String {
        self.target.read(cx).value().trim().to_owned()
    }

    /// The commits a push sends now, read again as the remote or the target changes.
    fn load(&mut self, cx: &mut Context<Self>) {
        let (root, remote, branch, target) = (
            self.root.clone(),
            self.remote.clone(),
            self.branch.clone(),
            self.target_name(cx),
        );
        self.outgoing = None;
        self._load = Some(cx.spawn(async move |this, cx| {
            let outgoing = cx
                .background_spawn(async move {
                    spork_git::remote::outgoing(&root, &remote, &branch, &target)
                })
                .await;
            this.update(cx, |this, cx| {
                this.outgoing = Some(outgoing.map_err(|e| e.to_string()));
                this.pick(Picked::Repository, cx);
            })
            .ok();
        }));
        cx.notify();
    }

    fn commits(&self) -> &[Commit] {
        match &self.outgoing {
            Some(Ok((commits, _))) => commits,
            _ => &[],
        }
    }

    /// The commits the right side shows the files of: the picked one, or all of them.
    fn picked_commits(&self) -> Vec<String> {
        match self.picked {
            Picked::Repository => self.commits().iter().map(|c| c.id.clone()).collect(),
            Picked::Commit(ix) => self
                .commits()
                .get(ix)
                .map(|c| c.id.clone())
                .into_iter()
                .collect(),
        }
    }

    fn pick(&mut self, picked: Picked, cx: &mut Context<Self>) {
        self.picked = picked;
        self.file = None;
        let (root, ids) = (self.root.clone(), self.picked_commits());
        self._files = Some(cx.spawn(async move |this, cx| {
            let files = cx
                .background_spawn(async move {
                    // oldest first, so the newest commit that changed a file names it
                    let mut files: Vec<(String, DiffFileStatus, String)> = Vec::new();
                    for id in ids.iter().rev() {
                        for (letter, path) in spork_git::commit_files(&root, id)? {
                            files.retain(|(p, ..)| *p != path);
                            files.push((path, file_status(letter), id.clone()));
                        }
                    }
                    Ok::<_, GitError>(files)
                })
                .await;
            this.update(cx, |this, cx| {
                let files = files.log_err().unwrap_or_default();
                this.files = files.iter().map(|(p, s, _)| (p.clone(), *s)).collect();
                this.file_commits = files.into_iter().map(|(p, _, id)| (p, id)).collect();
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn pick_file(&mut self, path: String, cx: &mut Context<Self>) {
        self.file = Some(path.clone());
        let (root, id) = (self.root.clone(), self.file_commits.get(&path).cloned());
        if let (Some(id), true) = (id, self.preview) {
            let (diff, view) = (self.diff.clone(), self.view.clone());
            self._patch = Some(cx.spawn(async move |_, cx| {
                let patch = cx
                    .background_spawn(async move {
                        spork_git::run(&root, &["show", "--format=", &id, "--", &path])
                    })
                    .await;
                if let (Some(patch), Some(view)) = (patch.log_err(), view.upgrade()) {
                    let patch = String::from_utf8_lossy(&patch).into_owned();
                    view.update(cx, |view, cx| view.set_patch(&diff, &patch, cx));
                }
            }));
        }
        cx.notify();
    }

    /// Whether the reference app refuses a force push to `target`: one of Settings › Git's protected
    /// branches.
    fn protected(target: &str) -> bool {
        let patterns = crate::settings_window::Store::load()
            .get("git.protectedBranches")
            .unwrap_or("master;main")
            .to_owned();
        // shortcut: exact names, where IntelliJ also takes regular expressions; match them
        // once someone lists one
        patterns.split(';').any(|p| p.trim() == target)
    }

    fn push(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.view.upgrade() else {
            return;
        };
        let (remote, branch, target) = (
            self.remote.clone(),
            self.branch.clone(),
            self.target_name(cx),
        );
        if target.is_empty() {
            return;
        }
        let settings = crate::settings_window::Store::load();
        let safe = settings.get("advanced.git.use.push.force.with.lease") != Some("false");
        let ask =
            settings.get("advanced.vcs.allow.force.push.without.confirmation") != Some("true");
        let (commits, exists) = match &self.outgoing {
            Some(Ok((commits, exists))) => (commits.len(), *exists),
            _ => (0, false),
        };
        let opts = PushOptions {
            set_upstream: !self.has_upstream,
            tags: self.push_tags && self.tag_mode == 0,
            follow_tags: self.push_tags && self.tag_mode == 1,
            force_with_lease: force && safe,
            force: force && !safe,
        };
        window.close_dialog(cx);
        let done = match (force, exists, commits) {
            (true, ..) => format!("Force pushed {branch} to {remote}/{target}"),
            (false, false, _) => format!("Pushed {branch} to new branch {remote}/{target}"),
            (false, true, 0) => "Everything is up to date".to_owned(),
            (false, true, 1) => format!("Pushed 1 commit to {remote}/{target}"),
            (false, true, n) => format!("Pushed {n} commits to {remote}/{target}"),
        };
        let run = move |this: &mut RepoView, window: &mut Window, cx: &mut Context<RepoView>| {
            let (remote, branch, target, done) =
                (remote.clone(), branch.clone(), target.clone(), done.clone());
            this.remote_running = Some(crate::RemoteOp::Push);
            this.run_op(
                window,
                cx,
                // rejected: the remote has commits we don't; asked about next, not an error
                move |root| match spork_git::remote::push_to(root, &remote, &branch, &target, opts)
                {
                    Err(GitError::NonFastForward) => Ok(None),
                    other => other.map(|()| Some(done)),
                },
                |this, done, window, cx| match done {
                    Some(done) => window.push_notification(Notification::success(done), cx),
                    None => this.push_rejected(window, cx),
                },
            )
        };
        let message = format!(
            "You're going to force push to {}/{}. It may overwrite commits at the remote. Are you sure you want to proceed?",
            self.remote,
            self.target_name(cx)
        );
        view.update(cx, |this, cx| match force && ask {
            true => this.confirm_dialog("Force Push", message, "Force Push", window, cx, run),
            false => run(this, window, cx),
        });
    }

    /// The repository's row: `main → origin : main`, the remote and the target links to change.
    fn repository_row(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let tokens = Tokens::get(cx);
        let selected = self.picked == Picked::Repository;
        let remotes = self.remotes.clone();
        let entity = cx.entity().downgrade();
        let remote = DropDownLink::new("push-remote", self.remote.clone())
            .without_chevron()
            .underline()
            .dropdown_menu(move |menu, _, _| {
                remotes.iter().fold(menu, |menu, remote| {
                    let (entity, remote) = (entity.clone(), remote.clone());
                    menu.item(
                        PopupMenuItem::new(remote.clone()).on_click(move |_, _, cx| {
                            if let Some(entity) = entity.upgrade() {
                                entity.update(cx, |this, cx| {
                                    this.remote = remote.clone();
                                    this.load(cx);
                                })
                            }
                        }),
                    )
                })
            });
        let target = match self.editing_target {
            true => div()
                .w(px(180.))
                .child(spork_workbench::TextField::new(&self.target).small())
                .into_any_element(),
            false => link("push-target", self.target_name(cx), cx)
                .underline()
                .on_click(cx.listener(|this, _, window, cx| {
                    this.editing_target = true;
                    this.target
                        .update(cx, |target, cx| target.focus(window, cx));
                    cx.notify();
                }))
                .into_any_element(),
        };
        let new = matches!(&self.outgoing, Some(Ok((_, false))));
        h_flex()
            .id("push-repository")
            .role(Role::TreeItem)
            .aria_label(format!(
                "{} → {} : {}",
                self.branch,
                self.remote,
                self.target_name(cx)
            ))
            .aria_selected(selected)
            .h(px(28.))
            .flex_none()
            .px_2()
            .gap_1()
            .when(selected, |row| row.bg(tokens.selection_inactive))
            .child(self.branch.clone())
            .child("→")
            .child(remote)
            .child(":")
            .child(target)
            .when(new, |row| {
                row.child(
                    div()
                        .id("push-new")
                        .role(Role::Label)
                        .aria_label("New")
                        .text_color(tokens.text_success)
                        .child("New"),
                )
            })
            .on_click(cx.listener(|this, _, _, cx| this.pick(Picked::Repository, cx)))
    }

    fn commit_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let tokens = Tokens::get(cx);
        self.commits()
            .iter()
            .enumerate()
            .map(|(ix, commit)| {
                let selected = self.picked == Picked::Commit(ix);
                h_flex()
                    .id(("push-commit", ix))
                    .role(Role::TreeItem)
                    .aria_label(commit.subject.clone())
                    .aria_selected(selected)
                    .h(px(24.))
                    .flex_none()
                    .pl(px(8. + INDENT))
                    .pr_2()
                    .when(selected, |row| row.bg(tokens.selection_inactive))
                    .child(div().min_w_0().truncate().child(commit.subject.clone()))
                    .on_click(cx.listener(move |this, _, _, cx| this.pick(Picked::Commit(ix), cx)))
                    .into_any_element()
            })
            .collect()
    }

    /// The right side's toolbar: Show Diff, View Options, Edit Source, Preview Diff; Expand All
    /// and Collapse All on the right.
    fn changes_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let file = self.file.is_some();
        let (entity, by_directory) = (cx.entity().downgrade(), self.by_directory);
        h_flex()
            .h(px(32.))
            .flex_none()
            .px_1()
            .gap_0p5()
            .child(
                spork_workbench::ActionButton::new("push-show-diff", "vcs/diff")
                    .tooltip("Show Diff (⌘D)")
                    .disabled(!file)
                    .on_click(cx.listener(|this, _, window, cx| this.show_diff(window, cx))),
            )
            .child(
                spork_workbench::ActionButton::new("push-view-options", "general/show")
                    .popup_marker()
                    .tooltip("View Options")
                    .dropdown_menu(move |menu, _, _| {
                        let entity = entity.clone();
                        menu.label("Group By").item(
                            PopupMenuItem::new("Directory")
                                .checked(by_directory)
                                .on_click(move |_, _, cx| {
                                    if let Some(entity) = entity.upgrade() {
                                        entity.update(cx, |this, cx| {
                                            this.by_directory ^= true;
                                            cx.notify();
                                        })
                                    }
                                }),
                        )
                    }),
            )
            .child(
                spork_workbench::ActionButton::new("push-edit-source", "general/edit")
                    .tooltip("Edit Source (⌘↓)")
                    .disabled(!file)
                    .on_click(cx.listener(|this, _, window, cx| this.edit_source(window, cx))),
            )
            .child(
                spork_workbench::ActionButton::new("push-preview", "general/previewVertically")
                    .tooltip("Preview Diff")
                    .selected(self.preview)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.preview ^= true;
                        cx.notify();
                    })),
            )
            .child(div().flex_1())
            .child(
                spork_workbench::ActionButton::new("push-expand", "general/expandAll")
                    .tooltip("Expand All")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.collapsed.clear();
                        cx.notify();
                    })),
            )
            .child(
                spork_workbench::ActionButton::new("push-collapse", "general/collapseAll")
                    .tooltip("Collapse All")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let folders =
                            rows(&this.files, &HashSet::new())
                                .into_iter()
                                .filter_map(|row| match row {
                                    Row::Folder { path, .. } => Some(path),
                                    Row::File { .. } => None,
                                });
                        this.collapsed.extend(folders);
                        cx.notify();
                    })),
            )
    }

    /// Show Diff: the file's diff in the commit that changed it, in the main window.
    fn show_diff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(path), Some(view)) = (self.file.clone(), self.view.upgrade()) else {
            return;
        };
        let Some(id) = self.file_commits.get(&path).cloned() else {
            return;
        };
        window.close_dialog(cx);
        view.update(cx, |this, cx| {
            this.reveal_commit(&id, window, cx);
            this.open_commit_diff(Some(path), cx);
        });
    }

    fn edit_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(path), Some(view)) = (self.file.clone(), self.view.upgrade()) else {
            return;
        };
        let path = self.root.join(path);
        view.update(cx, |this, cx| this.open_in_editor(Some(path), window, cx));
    }

    fn file_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let tokens = Tokens::get(cx);
        let tree = match self.by_directory {
            true => rows(&self.files, &self.collapsed),
            false => self
                .files
                .iter()
                .map(|(path, status)| Row::File {
                    name: path.rsplit('/').next().unwrap_or(path).to_owned(),
                    path: path.clone(),
                    status: *status,
                    depth: 1,
                })
                .collect(),
        };
        tree.into_iter()
            .enumerate()
            .map(|(ix, row)| {
                let line = |depth: usize| {
                    h_flex()
                        .id(("push-file", ix))
                        .role(Role::TreeItem)
                        .h(px(24.))
                        .flex_none()
                        .pl(px(6. + INDENT * depth.saturating_sub(1) as f32))
                        .pr_2()
                        .gap_1()
                };
                match row {
                    Row::Folder {
                        path, name, depth, ..
                    } => {
                        let open = !self.collapsed.contains(&path);
                        line(depth)
                            .aria_label(name.clone())
                            .aria_expanded(open)
                            .child(
                                spork_workbench::icon(match open {
                                    true => "general/chevronDown",
                                    false => "general/chevronRight",
                                })
                                .size(px(16.))
                                .text_color(tokens.icon),
                            )
                            .child(spork_workbench::image("nodes/folder", cx))
                            .child(name)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.collapsed.remove(&path) {
                                    this.collapsed.insert(path.clone());
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
                        let selected = self.file.as_ref() == Some(&path);
                        let dir = path.rsplit_once('/').map(|(dir, _)| dir.to_owned());
                        line(depth)
                            .aria_label(name.clone())
                            .aria_selected(selected)
                            .when(selected, |row| row.bg(tokens.selection_inactive))
                            .child(div().w(px(16.)).flex_none())
                            .child(spork_workbench::image(
                                spork_workbench::file_icon(&name),
                                cx,
                            ))
                            .child(div().min_w_0().truncate().text_color(color).child(name))
                            .when_some(dir.filter(|_| !self.by_directory), |row, dir| {
                                row.child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(tokens.text_muted)
                                        .child(dir),
                                )
                            })
                            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                                this.pick_file(path.clone(), cx);
                                if event.click_count() == 2 {
                                    this.show_diff(window, cx);
                                }
                            }))
                            .into_any_element()
                    }
                }
            })
            .collect()
    }

    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let entity = cx.entity().downgrade();
        let protected = Self::protected(&self.target_name(cx));
        let ready = matches!(&self.outgoing, Some(Ok(_))) && !self.target_name(cx).is_empty();
        let force = {
            let entity = entity.clone();
            move |menu: gpui_kit::component::menu::PopupMenu, _: &mut Window, _: &mut Context<_>| {
                let entity = entity.clone();
                menu.item(
                    PopupMenuItem::new("Force Push")
                        .disabled(protected)
                        .on_click(move |_, window, cx| {
                            if let Some(entity) = entity.upgrade() {
                                entity.update(cx, |this, cx| this.push(true, window, cx))
                            }
                        }),
                )
            }
        };
        let modes = {
            move |menu: gpui_kit::component::menu::PopupMenu, _: &mut Window, _: &mut Context<_>| {
                TAG_MODES.iter().enumerate().fold(menu, |menu, (ix, mode)| {
                    let entity = entity.clone();
                    menu.item(PopupMenuItem::new(*mode).on_click(move |_, _, cx| {
                        if let Some(entity) = entity.upgrade() {
                            entity.update(cx, |this, cx| {
                                this.tag_mode = ix;
                                cx.notify();
                            })
                        }
                    }))
                })
            }
        };
        h_flex()
            .pt_3()
            .gap_2()
            .child(
                spork_workbench::ActionButton::new("push-help", "general/questionMark")
                    .tooltip("Show help contents")
                    .on_click(|_, _, cx| cx.open_url("https://git-scm.com/docs/git-push")),
            )
            .child(
                labeled_check("push-tags", "Push tags:", self.push_tags, cx).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.push_tags ^= true;
                        cx.notify();
                    },
                )),
            )
            .child(div().w(px(160.)).child(combo(
                "push-tags-mode",
                TAG_MODES[self.tag_mode],
                !self.push_tags,
                modes,
            )))
            .child(div().flex_1())
            .child(
                spork_workbench::Button::new("push-cancel", "Cancel")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                OptionButton::new("push-ok", "Push", force)
                    .disabled(!ready)
                    .on_click(cx.listener(|this, _, window, cx| this.push(false, window, cx))),
            )
    }
}

impl Render for PushDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let empty = |text: SharedString| {
            div()
                .id("push-empty")
                .role(Role::Label)
                .aria_label(text.clone())
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(tokens.text_muted)
                .child(text)
        };
        let changes = match &self.outgoing {
            None => empty("Loading…".into()).into_any_element(),
            Some(Err(e)) => empty(e.clone().into()).into_any_element(),
            Some(Ok(_)) if self.picked_commits().is_empty() => {
                empty("No commits selected".into()).into_any_element()
            }
            Some(Ok(_)) => {
                let rows = self.file_rows(cx);
                let list = v_flex()
                    .id("push-files")
                    .role(Role::Tree)
                    .aria_label("Changes")
                    .flex_1()
                    .min_h_0()
                    .children(rows)
                    .overflow_y_scrollbar();
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .child(list)
                    .when(self.preview && self.file.is_some(), |side| {
                        side.child(
                            div()
                                .h(px(BODY_HEIGHT / 2.))
                                .flex_none()
                                .border_t_1()
                                .border_color(tokens.separator)
                                .child(Diff::new(&self.diff).size_full()),
                        )
                    })
                    .into_any_element()
            }
        };
        let left = v_flex()
            .id("push-tree")
            .role(Role::Tree)
            .aria_label("Push Commits")
            .flex_1()
            .min_w_0()
            .h_full()
            .border_r_1()
            .border_color(tokens.separator)
            .child(self.repository_row(cx))
            .children(self.commit_rows(cx))
            .overflow_y_scrollbar();
        let right = v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(self.changes_toolbar(cx))
            .child(changes);
        v_flex()
            .child(
                h_flex()
                    .w_full()
                    .h(px(BODY_HEIGHT))
                    .border_b_1()
                    .border_color(tokens.separator)
                    .child(left)
                    .child(right),
            )
            .child(self.footer(cx))
    }
}
