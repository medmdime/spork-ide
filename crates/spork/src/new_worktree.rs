//! The reference app's New Worktree dialog (`Git.CreateNewWorkingTree`, docs/reference-app/dialogs.md "New
//! Worktree…"): the branch to start from or a new one, the project's name and location, the
//! local setup to copy, then Create and Open Worktree.

use std::path::{Path, PathBuf};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_git::{GitError, RefKind};
use spork_workbench::controls::{combo, labeled_check};
use spork_workbench::{Tokens, Typography as _};

use crate::RepoView;
use crate::git_menu::{dialog_title, form_label};
use crate::notices::Notification;

/// What `.worktreeinclude` starts with when the dialog creates it (`GitBundle`
/// `working.tree.dialog.worktree.include.header.comment`).
const INCLUDE_HEADER: &str =
    "# The IDE fills the .worktreeinclude file with a suggestion. Review each line and edit it.\n";

/// A branch the worktree can start from.
struct Start {
    name: String,
    remote: bool,
    /// The folder of the worktree it is checked out in, if any.
    checked_out: Option<String>,
}

pub(crate) struct NewWorktreeDialog {
    view: WeakEntity<RepoView>,
    root: PathBuf,
    repository: String,
    starts: Vec<Start>,
    from: usize,
    locals: Vec<String>,
    new_branch: bool,
    branch: Entity<InputState>,
    name: Entity<InputState>,
    /// The project's name follows the branch until it is typed in.
    name_typed: bool,
    location: Entity<InputState>,
    setup_open: bool,
    copy_include: bool,
    run_script: bool,
    script: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl RepoView {
    /// Git ▸ New Worktree…, the branches popup's and a branch's: the reference app's New Worktree dialog,
    /// from `branch` or the current one.
    pub(crate) fn worktree_dialog(
        &mut self,
        branch: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(repo)) = &self.repo else { return };
        let worktree_of = |name: &str| {
            repo.worktrees
                .iter()
                .find(|w| w.branch.as_deref() == Some(name))
                .map(|w| crate::repos::display_name(&w.path))
        };
        let mut starts: Vec<Start> = repo
            .refs
            .iter()
            .filter(|r| r.kind == RefKind::Local)
            .map(|r| Start {
                name: r.name.clone(),
                remote: false,
                checked_out: worktree_of(&r.name),
            })
            .collect();
        starts.extend(
            repo.refs
                .iter()
                .filter(|r| r.kind == RefKind::Remote && !r.name.ends_with("/HEAD"))
                .map(|r| Start {
                    name: r.name.clone(),
                    remote: true,
                    checked_out: None,
                }),
        );
        let wanted = branch.or_else(|| repo.head.clone());
        let from = starts
            .iter()
            .position(|s| Some(&s.name) == wanted.as_ref())
            .unwrap_or_default();
        let locals = repo
            .refs
            .iter()
            .filter(|r| r.kind == RefKind::Local)
            .map(|r| r.name.clone())
            .collect();
        let root = repo.root.clone();
        let repository = crate::repos::display_name(&root);
        let parent = root.parent().unwrap_or(&root).display().to_string();
        let view = cx.entity().downgrade();
        let dialog = cx.new(|cx| {
            let field =
                |value: String, window: &mut Window, cx: &mut Context<NewWorktreeDialog>| {
                    cx.new(|cx| InputState::new(window, cx).default_value(value))
                };
            let (branch, name) = (
                field(String::new(), window, cx),
                field(String::new(), window, cx),
            );
            let (location, script) = (field(parent, window, cx), field(String::new(), window, cx));
            let changed = |typed: bool| {
                move |this: &mut NewWorktreeDialog,
                      _: &Entity<InputState>,
                      event: &InputEvent,
                      window: &mut Window,
                      cx: &mut Context<NewWorktreeDialog>| {
                    if matches!(event, InputEvent::Change) {
                        this.name_typed |= typed;
                        this.follow_name(window, cx);
                        cx.notify();
                    }
                }
            };
            let subscriptions = vec![
                cx.subscribe_in(&branch, window, changed(false)),
                cx.subscribe_in(&location, window, changed(false)),
                cx.subscribe_in(&script, window, changed(false)),
            ];
            let mut dialog = NewWorktreeDialog {
                view,
                root,
                repository,
                starts,
                from,
                locals,
                new_branch: false,
                branch,
                name,
                name_typed: false,
                location,
                setup_open: false,
                copy_include: false,
                run_script: false,
                script,
                _subscriptions: subscriptions,
            };
            dialog.follow_name(window, cx);
            let typed = cx.subscribe_in(&dialog.name, window, |this, name, event, _, cx| {
                // a change it didn't make itself: the user typed a name
                if matches!(event, InputEvent::Change)
                    && name.read(cx).value() != this.suggested_name(cx)
                {
                    this.name_typed = true;
                    cx.notify();
                }
            });
            dialog._subscriptions.push(typed);
            dialog
        });
        window.open_dialog(cx, move |modal, _, _| {
            modal
                .title(dialog_title("New Worktree"))
                .w(px(700.))
                .close_button(false)
                .child(dialog.clone())
        });
    }
}

impl NewWorktreeDialog {
    fn start(&self) -> Option<&Start> {
        self.starts.get(self.from)
    }

    fn text(field: &Entity<InputState>, cx: &App) -> String {
        field.read(cx).value().trim().to_owned()
    }

    /// The local branch the worktree checks out: the new one, or the picked one (a remote
    /// branch's own name).
    fn local_branch(&self, cx: &App) -> Option<String> {
        if self.new_branch {
            return Some(Self::text(&self.branch, cx)).filter(|b| !b.is_empty());
        }
        let start = self.start()?;
        Some(match start.remote {
            true => start
                .name
                .split_once('/')
                .map_or(start.name.clone(), |(_, b)| b.to_owned()),
            false => start.name.clone(),
        })
    }

    /// The reference app's project name: the repository's, a dash and the branch's last part.
    fn suggested_name(&self, cx: &App) -> String {
        let branch = self.local_branch(cx).unwrap_or_default();
        let leaf = branch.rsplit('/').next().unwrap_or(&branch);
        match leaf.is_empty() {
            true => self.repository.clone(),
            false => format!("{}-{leaf}", self.repository),
        }
    }

    fn follow_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.name_typed {
            return;
        }
        let suggested = self.suggested_name(cx);
        if Self::text(&self.name, cx) != suggested {
            self.name
                .update(cx, |name, cx| name.set_value(suggested, window, cx));
        }
    }

    fn target(&self, cx: &App) -> PathBuf {
        Path::new(&Self::text(&self.location, cx)).join(Self::text(&self.name, cx))
    }

    /// What is wrong, by the field it shows under: the branch, the project name, the location.
    fn problems(&self, cx: &App) -> [Option<String>; 3] {
        let branch = match (self.new_branch, self.start(), self.local_branch(cx)) {
            (_, None, _) => Some("Select a branch to create the worktree".to_owned()),
            (true, _, None) => {
                Some("A name for the branch in the worktree must be provided explicitly".into())
            }
            (true, _, Some(b)) if self.locals.contains(&b) => {
                Some(format!("Local branch {b} already exists"))
            }
            (false, Some(start), _) if start.checked_out.is_some() => {
                Some("Selected branch is already checked out in another worktree".into())
            }
            (false, Some(start), Some(b)) if start.remote && self.locals.contains(&b) => {
                Some(format!(
                    "Local branch {b} already exists, provide explicit name for the branch in the worktree"
                ))
            }
            _ => None,
        };
        let name = Self::text(&self.name, cx);
        let name = (name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || " _-.".contains(c)))
        .then(|| {
            "Name should contain only letters, digits, spaces, '_', '-', '.' characters".into()
        });
        let target = self.target(cx);
        let location = match () {
            _ if Self::text(&self.location, cx).is_empty() => {
                Some("Provide a location for the worktree".to_owned())
            }
            _ if target.is_file() => Some(format!("'{}' is a file", target.display())),
            _ if target
                .read_dir()
                .is_ok_and(|mut entries| entries.next().is_some()) =>
            {
                Some(format!("'{}' is not empty", target.display()))
            }
            _ if self.run_script && Self::text(&self.script, cx).is_empty() => {
                Some("Provide a path to the setup script".to_owned())
            }
            _ => None,
        };
        [branch, name, location]
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.problems(cx).iter().any(Option::is_some) {
            return;
        }
        let (Some(view), Some(start), Some(local)) =
            (self.view.upgrade(), self.start(), self.local_branch(cx))
        else {
            return;
        };
        let (start, remote) = (start.name.clone(), start.remote);
        let new = self.new_branch || remote;
        let path = self.target(cx);
        let (copy, script) = (
            self.copy_include,
            self.run_script.then(|| Self::text(&self.script, cx)),
        );
        window.close_dialog(cx);
        view.update(cx, |this, cx| {
            let created = path.clone();
            this.run_op(
                window,
                cx,
                move |root| {
                    match new {
                        true => spork_git::submodule::add_worktree_new_branch(
                            root, &path, &local, &start,
                        )?,
                        false => spork_git::add_worktree(root, &path, &local)?,
                    }
                    if copy {
                        copy_included(root, &path)?;
                    }
                    if let Some(script) = script {
                        run_setup_script(&script, &path)?;
                    }
                    Ok(local)
                },
                move |this, local, window, cx| {
                    window.push_notification(
                        Notification::success(format!(
                            "The worktree for '{local}' is at {}.",
                            created.display()
                        )),
                        cx,
                    );
                    this.open_worktree(created, cx);
                },
            )
        });
    }

    /// Create / Edit beside "Copy files listed in .worktreeinclude": the file in your editor,
    /// written first with the reference app's header when there is none.
    fn edit_include(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.root.join(".worktreeinclude");
        if !path.exists()
            && let Err(e) = std::fs::write(&path, INCLUDE_HEADER)
        {
            return window.push_notification(Notification::error(e.to_string()), cx);
        }
        if let Some(view) = self.view.upgrade() {
            view.update(cx, |this, cx| this.open_in_editor(Some(path), window, cx));
        }
    }

    fn setup_group(&self, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let on = usize::from(self.copy_include) + usize::from(self.run_script);
        let header = h_flex()
            .id("worktree-setup")
            .role(Role::Button)
            .aria_label("Copy local project setup")
            .aria_expanded(self.setup_open)
            .gap_2()
            .cursor_pointer()
            .child(
                spork_workbench::icon(match self.setup_open {
                    true => "general/chevronDown",
                    false => "general/chevronRight",
                })
                .size(px(16.))
                .text_color(tokens.icon),
            )
            .child("Copy local project setup")
            .child(
                div()
                    .px(px(6.))
                    .rounded(px(8.))
                    .bg(tokens.badge_beta)
                    .text_color(tokens.badge_beta_text)
                    .text_ui_xsmall()
                    .child("Beta"),
            )
            .child(div().flex_1().h(px(1.)).bg(tokens.separator))
            .on_click(cx.listener(|this, _, _, cx| {
                this.setup_open ^= true;
                cx.notify();
            }));
        let summary = div()
            .id("worktree-setup-summary")
            .role(Role::Label)
            .aria_label(format!("{on} of 2 settings on"))
            .pl(px(24.))
            .text_color(tokens.text_muted)
            .child(format!("{on} of 2 settings on"));
        let include_exists = self.root.join(".worktreeinclude").exists();
        let comment = |text: &'static str| {
            div()
                .pl(px(24.))
                .text_color(tokens.text_muted)
                .text_ui_small()
                .child(text)
        };
        let open = self.setup_open.then(|| {
            v_flex()
                .pl(px(24.))
                .gap_2()
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            labeled_check(
                                "worktree-include",
                                "Copy files listed in .worktreeinclude",
                                self.copy_include,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.copy_include ^= true;
                                cx.notify();
                            })),
                        )
                        .child(
                            spork_workbench::Button::new(
                                "worktree-include-edit",
                                match include_exists {
                                    true => "Edit",
                                    false => "Create",
                                },
                            )
                            .on_click(cx.listener(|this, _, window, cx| this.edit_include(window, cx))),
                        ),
                )
                .child(comment(
                    "List one file pattern per line, using .gitignore syntax.",
                ))
                .child(
                    labeled_check(
                        "worktree-script",
                        "Run setup script",
                        self.run_script,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.run_script ^= true;
                        cx.notify();
                    })),
                )
                .child(
                    h_flex()
                        .pl(px(24.))
                        .gap_2()
                        .child(form_label("Script:"))
                        .child(
                            spork_workbench::TextField::new(&self.script)
                                .disabled(!self.run_script)
                                .flex_1(),
                        ),
                )
                .child(comment(
                    "The IDE runs the script once, with the new worktree path as the argument, before it opens the project. The worktree stays even if the script fails.",
                ))
        });
        v_flex()
            .gap_2()
            .child(header)
            .when_some(open, |group, open| group.child(open))
            .when(!self.setup_open, |group| group.child(summary))
    }
}

impl Render for NewWorktreeDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let [branch_problem, name_problem, location_problem] = self.problems(cx);
        let label = |text: &'static str| form_label(text).w(px(130.)).flex_none();
        let under = |problem: Option<String>, color: Hsla| {
            problem.map(|problem| {
                div()
                    .id(SharedString::from(problem.clone()))
                    .role(Role::Label)
                    .aria_label(problem.clone())
                    .pl(px(130.))
                    .text_color(color)
                    .child(problem)
            })
        };
        let entity = cx.entity().downgrade();
        let starts: Vec<(String, Option<String>)> = self
            .starts
            .iter()
            .map(|s| (s.name.clone(), s.checked_out.clone()))
            .collect();
        let shown = self
            .start()
            .map_or(String::new(), |s| match &s.checked_out {
                Some(worktree) => format!("{} {worktree}", s.name),
                None => s.name.clone(),
            });
        let from = combo("worktree-from", shown, false, move |menu, _, _| {
            starts
                .iter()
                .enumerate()
                .fold(menu, |menu, (ix, (name, worktree))| {
                    let entity = entity.clone();
                    let text = match worktree {
                        Some(worktree) => format!("{name} {worktree}"),
                        None => name.clone(),
                    };
                    menu.item(PopupMenuItem::new(text).on_click(move |_, window, cx| {
                        if let Some(entity) = entity.upgrade() {
                            entity.update(cx, |this, cx| {
                                this.from = ix;
                                this.follow_name(window, cx);
                                cx.notify();
                            })
                        }
                    }))
                })
        });
        let view = self.view.clone();
        let location_field = self.location.clone();
        let browse = spork_workbench::ActionButton::new("worktree-browse", "nodes/folder")
            .tooltip("Browse… (⇧⏎)")
            .on_click(move |_, window, cx| {
                let location = location_field.clone();
                if let Some(view) = view.upgrade() {
                    view.update(cx, |this, cx| {
                        this.pick_folder(
                            "Worktree Location",
                            window,
                            cx,
                            move |_, picked, window, cx| {
                                location.update(cx, |location, cx| {
                                    location.set_value(picked.display().to_string(), window, cx)
                                })
                            },
                        )
                    })
                }
            });
        let target = self.target(cx).display().to_string();
        let footer = h_flex()
            .pt_2()
            .gap_2()
            .child(
                spork_workbench::ActionButton::new("worktree-help", "general/questionMark")
                    .tooltip("Show help contents")
                    .on_click(|_, _, cx| cx.open_url("https://git-scm.com/docs/git-worktree")),
            )
            .child(div().flex_1())
            .child(
                spork_workbench::Button::new("worktree-cancel", "Cancel")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                spork_workbench::Button::new("worktree-ok", "Create and Open Worktree")
                    .primary(true)
                    .on_click(cx.listener(|this, _, window, cx| this.create(window, cx))),
            );
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .child(label("From branch:"))
                    .child(div().flex_1().child(from)),
            )
            // The reference app greys the branch's, as a comment under its combo
            .children(under(branch_problem, tokens.text_muted))
            .child(
                h_flex()
                    .child(
                        div().w(px(130.)).flex_none().child(
                            labeled_check(
                                "worktree-new-branch",
                                "New branch:",
                                self.new_branch,
                                cx,
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.new_branch ^= true;
                                    this.follow_name(window, cx);
                                    cx.notify();
                                },
                            )),
                        ),
                    )
                    .child(
                        spork_workbench::TextField::new(&self.branch)
                            .aria_label("New branch:")
                            .disabled(!self.new_branch)
                            .flex_1(),
                    ),
            )
            .child(
                h_flex().pt_2().child(label("Project name:")).child(
                    spork_workbench::TextField::new(&self.name)
                        .aria_label("Project name:")
                        .flex_1(),
                ),
            )
            .children(under(name_problem, tokens.text_error))
            .child(
                h_flex().child(label("Location:")).child(
                    spork_workbench::TextField::new(&self.location)
                        .aria_label("Location:")
                        .suffix(browse)
                        .flex_1(),
                ),
            )
            .children(under(location_problem, tokens.text_error))
            .child(
                div()
                    .id("worktree-where")
                    .role(Role::Label)
                    .aria_label(format!("The worktree will be created in: {target}"))
                    .pl(px(130.))
                    .text_color(tokens.text_muted)
                    .child(format!("The worktree will be created in:\n{target}")),
            )
            .child(self.setup_group(cx))
            .child(footer)
    }
}

/// Copies the files `.worktreeinclude` lists from `root` into the new worktree at `to`.
fn copy_included(root: &Path, to: &Path) -> Result<(), GitError> {
    let io = |e: std::io::Error| GitError::Other {
        code: None,
        stderr: e.to_string(),
    };
    for file in spork_git::worktree_include_files(root)? {
        let dest = to.join(&file);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::copy(root.join(&file), dest).map_err(io)?;
    }
    Ok(())
}

/// Runs the setup script with the new worktree's path, as the reference app does before it opens it.
#[allow(
    clippy::disallowed_methods,
    reason = "runs inside run_op, on a background thread like the git runner"
)]
fn run_setup_script(script: &str, worktree: &Path) -> Result<(), GitError> {
    let out = std::process::Command::new(script)
        .arg(worktree)
        .current_dir(worktree)
        .output()
        .map_err(|e| GitError::Other {
            code: None,
            stderr: format!("Setup script failed: {e}"),
        })?;
    match out.status.success() {
        true => Ok(()),
        false => Err(GitError::Other {
            code: out.status.code(),
            stderr: format!(
                "Setup script failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        }),
    }
}
