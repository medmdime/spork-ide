//! What the Git menu opens and runs, as the reference app has it: the Pull, Merge, Rebase and Reset HEAD
//! dialogs, Git Remotes, the VCS Operations popup and the paused operation's Continue / Abort.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_git::revision::ResetMode;
use spork_git::{GitError, OpState, RefKind};
use spork_hosting::Service;
use spork_workbench::controls::{ComboBox, DropDownLink, combo};
use spork_workbench::tokens::metrics;

use crate::RepoView;
use crate::notices::Notification;
use crate::shell::{
    AbortCherryPick, AbortMerge, AbortRebase, AbortRevert, CheckoutRevision, CommitMerge,
    ContextHelp, ContinueCherryPick, ContinueRebase, CopyBranchName, CopyCommitSubject,
    GetFromVersionControl, GitMenu, GitMerge, GitPull, GitRebase, GitReset, ManageRemotes,
    QuickSwitchScheme, ResolveConflicts, ShowConsoleTab, ShowFileHistory, ShowLocalChanges,
    ShowStagingArea, SkipCommit, Unshallow, VcsOperationsPopup,
};

impl RepoView {
    /// Which of the Git menu's items this repository shows now.
    pub(crate) fn git_menu_state(&self) -> GitMenu {
        let Some(Ok(repo)) = &self.repo else {
            return GitMenu::default();
        };
        GitMenu {
            merging: repo.op == OpState::Merging,
            rebasing: matches!(repo.op, OpState::Rebasing { .. }),
            cherry_picking: repo.op == OpState::CherryPicking,
            reverting: repo.op == OpState::Reverting,
            conflicts: repo.status.iter().any(|entry| entry.conflicted),
            shallow: repo.shallow,
            stashes: !repo.stashes.is_empty(),
            file: self.current_file(),
        }
    }

    /// The commit selected in the log, and its subject; `None` outside the log.
    fn selected_log_commit(&self) -> Option<(String, String)> {
        let Some(Ok(repo)) = &self.repo else {
            return None;
        };
        let commit = repo.history.commits.get(self.selected?)?;
        (self.view == crate::View::History).then(|| (commit.id.clone(), commit.subject.clone()))
    }

    /// The current branch; `None` while HEAD is detached.
    pub(crate) fn current_branch(&self) -> Option<String> {
        match &self.repo {
            Some(Ok(repo)) => repo.head.clone(),
            _ => None,
        }
    }

    /// The Git menu's own actions. Those that act on a file listen only while Local Changes has
    /// one selected, so the menu greys them out otherwise, as the reference app does.
    pub(crate) fn git_menu_actions(&self, el: Div, cx: &mut Context<Self>) -> Div {
        el.on_action(cx.listener(|this, _: &GitPull, w, cx| this.pull_dialog(w, cx)))
            .on_action(cx.listener(|this, _: &GitMerge, w, cx| this.git_merge_dialog(w, cx)))
            .on_action(cx.listener(|this, _: &GitRebase, w, cx| this.git_rebase_dialog(w, cx)))
            .on_action(cx.listener(|this, _: &GitReset, w, cx| this.reset_head_dialog(w, cx)))
            .on_action(cx.listener(|this, _: &ManageRemotes, w, cx| this.remotes_dialog(w, cx)))
            .on_action(
                cx.listener(|this, _: &GetFromVersionControl, w, cx| this.clone_dialog(w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &QuickSwitchScheme, w, cx| this.quick_switch_scheme(w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &VcsOperationsPopup, w, cx| this.vcs_operations_popup(w, cx)),
            )
            .on_action(cx.listener(|this, _: &Unshallow, window, cx| {
                this.run_op(window, cx, spork_git::unshallow, |_, _, window, cx| {
                    window.push_notification(
                        Notification::success("The repository was converted to a complete one"),
                        cx,
                    )
                })
            }))
            .on_action(cx.listener(|this, _: &CommitMerge, window, cx| {
                this.run_outcome(window, cx, "Merge committed".into(), spork_git::continue_op)
            }))
            .on_action(cx.listener(|this, _: &ContinueRebase, window, cx| {
                this.run_outcome(window, cx, "Rebased".into(), spork_git::continue_op)
            }))
            .on_action(cx.listener(|this, _: &ContinueCherryPick, window, cx| {
                this.run_outcome(window, cx, "Cherry-picked".into(), spork_git::continue_op)
            }))
            .on_action(cx.listener(|this, _: &SkipCommit, window, cx| {
                this.run_outcome(window, cx, "Skipped".into(), spork_git::skip_op)
            }))
            .on_action(cx.listener(|this, _: &AbortMerge, w, cx| this.confirm_abort(w, cx)))
            .on_action(cx.listener(|this, _: &AbortRebase, w, cx| this.confirm_abort(w, cx)))
            .on_action(cx.listener(|this, _: &AbortRevert, w, cx| this.confirm_abort(w, cx)))
            .on_action(cx.listener(|this, _: &AbortCherryPick, w, cx| this.confirm_abort(w, cx)))
            .on_action(cx.listener(|this, _: &ResolveConflicts, _, cx| {
                this.open_left(crate::LeftTool::Commit, cx)
            }))
            .on_action(cx.listener(|this, _: &CopyBranchName, _, cx| {
                if let Some(branch) = this.current_branch() {
                    cx.write_to_clipboard(ClipboardItem::new_string(branch));
                }
            }))
            .on_action(cx.listener(|this, _: &ShowLocalChanges, w, cx| {
                this.show_saved(crate::stash_view::CommitTab::Commit, w, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowConsoleTab, _, cx| {
                this.show_console = true;
                this.show_terminal = false;
                this.worktrees_tab = false;
                if this.ide {
                    cx.emit(crate::ide::ToolWindowEvent::Show(
                        crate::ide::ToolWindow::Git,
                    ));
                }
                cx.notify();
            }))
            // greyed while the Commit tool window shows changelists, as the reference app has it
            .when(self.staging_area_shown(), |el| {
                el.on_action(cx.listener(|this, _: &ShowStagingArea, w, cx| {
                    this.show_saved(crate::stash_view::CommitTab::Commit, w, cx)
                }))
            })
            // the help of the tool window showing; Spork has help for the Worktrees tab alone
            .when(self.show_console && self.worktrees_tab, |el| {
                el.on_action(|_: &ContextHelp, _, cx| cx.open_url(crate::worktrees::HELP))
            })
            // the log's selected commit; the reference app greys both out without one
            .when_some(self.selected_log_commit(), |el, (id, subject)| {
                el.on_action(cx.listener(move |_, _: &CopyCommitSubject, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(subject.clone()))
                }))
                .on_action(cx.listener(
                    move |this, _: &CheckoutRevision, window, cx| {
                        let id = id.clone();
                        this.run_op(
                            window,
                            cx,
                            move |root| spork_git::branch::checkout_detached(root, &id),
                            |_, _, _, _| {},
                        )
                    },
                ))
            })
            .when_some(self.change_selected.clone(), |el, selected| {
                el.on_action(cx.listener(move |this, _: &ShowFileHistory, _, cx| {
                    this.open_file(selected.path.clone(), false, cx)
                }))
            })
    }
}

/// `(flag, what it does)` for Modify options, in the reference app's order and words (`GitBundle`
/// `pull.option.*`, `merge.option.*`, `rebase.option.*`).
type GitOption = (&'static str, &'static str);

const PULL_OPTIONS: &[GitOption] = &[
    (
        "--rebase",
        "Rebase the current branch on top of incoming changes",
    ),
    ("--ff-only", "Merge only if it can be fast-forwarded"),
    (
        "--no-ff",
        "Create a merge commit even if it can be fast-forwarded",
    ),
    ("--squash", "Create a single commit for all pulled changes"),
    ("--no-commit", "Merge, but do not commit the result"),
    (
        "--no-verify",
        "Bypass the pre-merge and commit message hooks",
    ),
];

const MERGE_OPTIONS: &[GitOption] = &[
    (
        "--no-ff",
        "Create a merge commit even if it can be fast-forwarded",
    ),
    ("--ff-only", "Merge only if it can be fast-forwarded"),
    ("--squash", "Create a single commit for all merged changes"),
    ("--no-commit", "Do not commit the merge result"),
    (
        "--no-verify",
        "Bypass the pre-merge and commit message hooks",
    ),
    (
        "--allow-unrelated-histories",
        "Allow merging histories that do not share a common ancestor.",
    ),
];

// shortcut: the reference app also offers --onto, --interactive, --root, another branch to rebase and
// --(no-)autosquash, which need more fields; add them with the dialog's second row
const REBASE_OPTIONS: &[GitOption] = &[
    ("--rebase-merges", "Recreate commits topology"),
    ("--keep-empty", "Do not remove empty commits during rebase"),
    (
        "--update-refs",
        "Auto-update branches pointing to rebased commits",
    ),
];

/// What a Pull, Merge or Rebase dialog is set to: the remote, the branch or revision, and the
/// options Modify options added, in the order the reference app lists them.
struct GitCommand {
    remote: String,
    target: Entity<InputState>,
    options: Vec<&'static str>,
}

impl GitCommand {
    fn toggle(&mut self, flag: &'static str, all: &[GitOption]) {
        match self.options.iter().position(|o| *o == flag) {
            Some(ix) => {
                self.options.remove(ix);
            }
            None => {
                self.options.push(flag);
                self.options
                    .sort_by_key(|o| all.iter().position(|(flag, _)| flag == o));
            }
        }
    }
}

/// What a git command dialog was set to when its button was clicked.
struct Chosen {
    remote: String,
    target: String,
    options: Vec<&'static str>,
}

/// How one git command dialog differs from the others.
struct CommandDialog {
    title: String,
    /// `pull`, `merge`, `rebase`
    command: &'static str,
    options: &'static [GitOption],
    options_title: &'static str,
    ok: &'static str,
    /// The branch field's choices (the Pull dialog lists its remote's), and its text when empty.
    branches: Vec<String>,
    placeholder: &'static str,
    /// The Pull dialog's remotes; empty, no remote field.
    remotes: Vec<String>,
}

impl RepoView {
    /// Local branches other than the current one, then remote branches, as the reference app's Merge and
    /// Rebase dialogs list them.
    fn other_branches(&self) -> Vec<String> {
        let Some(Ok(repo)) = &self.repo else {
            return Vec::new();
        };
        let named = |kind: RefKind| {
            repo.refs
                .iter()
                .filter(move |r| r.kind == kind && !r.is_head && !r.name.ends_with("/HEAD"))
                .map(|r| r.name.clone())
        };
        named(RefKind::Local)
            .chain(named(RefKind::Remote))
            .collect()
    }

    /// `remote`'s branches without its name, for the Pull dialog's branch field.
    fn remote_branches(&self, remote: &str) -> Vec<String> {
        let Some(Ok(repo)) = &self.repo else {
            return Vec::new();
        };
        let prefix = format!("{remote}/");
        repo.refs
            .iter()
            .filter(|r| r.kind == RefKind::Remote)
            .filter_map(|r| r.name.strip_prefix(&prefix).map(str::to_owned))
            .filter(|name| name != "HEAD")
            .collect()
    }

    /// The reference app's Pull dialog: `git pull`, the remote, its branch (the upstream's at first).
    pub(crate) fn pull_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let Some(into) = repo.head.clone() else {
            return window
                .push_notification(Notification::info("Check out a branch to pull into"), cx);
        };
        let upstream = repo
            .refs
            .iter()
            .find(|r| r.is_head && r.kind == RefKind::Local)
            .and_then(|head| head.upstream.as_deref())
            .and_then(|up| self.split_remote_branch(up));
        let remotes: Vec<String> = repo.remotes.iter().map(|r| r.name.clone()).collect();
        let (remote, branch) =
            upstream.unwrap_or_else(|| (self.default_remote().unwrap_or_default(), into.clone()));
        let dialog = CommandDialog {
            title: format!("Pull to {into}"),
            command: "pull",
            options: PULL_OPTIONS,
            options_title: "Add Pull Options",
            ok: "Pull",
            branches: Vec::new(),
            placeholder: "Specify branch",
            remotes,
        };
        self.open_command_dialog(
            dialog,
            remote,
            branch,
            window,
            cx,
            |this, chosen, window, cx| {
                let Chosen {
                    remote,
                    target,
                    options,
                } = chosen;
                this.remote_running = Some(crate::RemoteOp::Pull);
                let done = format!("Pulled {remote}/{target}");
                this.run_outcome(window, cx, done, move |root| {
                    spork_git::run_command(root, "pull", &options, &[&remote, &target])
                })
            },
        );
    }

    /// The reference app's Merge dialog: `git merge` and the branch to bring into the current one.
    pub(crate) fn git_merge_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(into) = self.current_branch() else {
            return window
                .push_notification(Notification::info("Check out a branch to merge into"), cx);
        };
        let branches = self.other_branches();
        let first = branches.first().cloned().unwrap_or_default();
        let dialog = CommandDialog {
            title: format!("Merge into {into}"),
            command: "merge",
            options: MERGE_OPTIONS,
            options_title: "Add Merge Options",
            ok: "Merge",
            branches,
            placeholder: "Specify branch",
            remotes: Vec::new(),
        };
        self.open_command_dialog(
            dialog,
            String::new(),
            first,
            window,
            cx,
            move |this, chosen, window, cx| {
                let Chosen {
                    target, options, ..
                } = chosen;
                let done = format!("Merged {target} to {into}");
                this.run_outcome(window, cx, done, move |root| {
                    spork_git::run_command(root, "merge", &options, &[&target])
                })
            },
        );
    }

    /// The reference app's Rebase dialog: `git rebase` and the branch or hash to rebase onto, empty at
    /// first.
    pub(crate) fn git_rebase_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog = CommandDialog {
            title: "Rebase".into(),
            command: "rebase",
            options: REBASE_OPTIONS,
            options_title: "Add Rebase Options",
            ok: "Rebase",
            branches: self.other_branches(),
            placeholder: "branch or hash",
            remotes: Vec::new(),
        };
        self.open_command_dialog(
            dialog,
            String::new(),
            String::new(),
            window,
            cx,
            |this, chosen, window, cx| {
                let Chosen {
                    target, options, ..
                } = chosen;
                this.run_outcome(window, cx, format!("Rebased onto {target}"), move |root| {
                    spork_git::run_command(root, "rebase", &options, &[&target])
                })
            },
        );
    }

    /// The dialog the reference app shows for Pull, Merge and Rebase: the command and its fields in one
    /// row, the options added under it, help, Modify options, Cancel and the command's button.
    fn open_command_dialog(
        &mut self,
        dialog: CommandDialog,
        remote: String,
        target: String,
        window: &mut Window,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Self, Chosen, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(dialog.placeholder)
                .default_value(target)
        });
        let state = cx.new(|_| GitCommand {
            remote,
            target: input,
            options: Vec::new(),
        });
        let (view, run, dialog) = (cx.entity(), Rc::new(run), Rc::new(dialog));
        window.open_dialog(cx, move |modal, _, cx| {
            let tokens = spork_workbench::Tokens::get(cx);
            let command = state.read(cx);
            let target = command.target.read(cx).value().trim().to_owned();
            let ready =
                !target.is_empty() && (dialog.remotes.is_empty() || !command.remote.is_empty());
            let remote_field = (!dialog.remotes.is_empty()).then(|| {
                let (state, remotes) = (state.clone(), dialog.remotes.clone());
                combo(
                    "git-remote",
                    command.remote.clone(),
                    false,
                    move |menu, _, _| {
                        remotes.iter().fold(menu, |menu, remote| {
                            let (state, remote) = (state.clone(), remote.clone());
                            menu.item(PopupMenuItem::new(remote.clone()).on_click(
                                move |_, window, cx| {
                                    state.update(cx, |this, _| this.remote = remote.clone());
                                    window.refresh();
                                },
                            ))
                        })
                    },
                )
            });
            let branches = match dialog.remotes.is_empty() {
                true => dialog.branches.clone(),
                false => view.read(cx).remote_branches(&command.remote),
            };
            let input = command.target.clone();
            let picker = spork_workbench::ActionButton::new("git-branches", "general/chevronDown")
                .dropdown_menu(move |menu, _, _| {
                    let menu = menu.scrollable(true).max_h(px(420.));
                    branches.iter().fold(menu, |menu, branch| {
                        let (input, branch) = (input.clone(), branch.clone());
                        menu.item(PopupMenuItem::new(branch.clone()).on_click(
                            move |_, window, cx| {
                                input.update(cx, |input, cx| {
                                    input.set_value(branch.clone(), window, cx)
                                });
                            },
                        ))
                    })
                });
            let label = format!("git {}", dialog.command);
            let row = h_flex()
                .gap_1()
                .child(
                    div()
                        .id("git-command")
                        .role(Role::Label)
                        .aria_label(label.clone())
                        .h(px(metrics::CONTROL_HEIGHT))
                        .px_2()
                        .flex()
                        .flex_none()
                        .items_center()
                        .rounded(px(metrics::RADIUS))
                        .border_1()
                        .border_color(tokens.control_border)
                        .child(label),
                )
                .children(remote_field)
                .child(
                    spork_workbench::TextField::new(&command.target)
                        .suffix(picker)
                        .flex_1(),
                );
            let chips: Vec<_> = command
                .options
                .iter()
                .map(|flag| {
                    let (state, flag, all) = (state.clone(), *flag, dialog.options);
                    h_flex()
                        .id(SharedString::from(format!("option{flag}")))
                        .role(Role::Button)
                        .aria_label(flag)
                        .gap_1()
                        .pl_2()
                        .pr_1()
                        .rounded(px(metrics::RADIUS))
                        .border_1()
                        .border_color(tokens.control_border)
                        .cursor_pointer()
                        .child(flag)
                        .child(spork_workbench::icon("general/closeSmall").size(px(16.)))
                        .on_click(move |_, window, cx| {
                            state.update(cx, |this, _| this.toggle(flag, all));
                            window.refresh();
                        })
                })
                .collect();
            let options_menu = {
                let (state, dialog) = (state.clone(), dialog.clone());
                DropDownLink::new("modify-options", "Modify options").dropdown_menu(
                    move |menu, _, cx| {
                        let chosen = state.read(cx).options.clone();
                        let menu = menu.label(dialog.options_title);
                        dialog.options.iter().fold(menu, |menu, (flag, text)| {
                            let (state, flag, all) = (state.clone(), *flag, dialog.options);
                            menu.item(
                                PopupMenuItem::new(format!("{flag}  {text}"))
                                    .checked(chosen.contains(&flag))
                                    .on_click(move |_, window, cx| {
                                        state.update(cx, |this, _| this.toggle(flag, all));
                                        window.refresh();
                                    }),
                            )
                        })
                    },
                )
            };
            let help = format!("https://git-scm.com/docs/git-{}", dialog.command);
            let (state, view, run) = (state.clone(), view.clone(), run.clone());
            let footer = h_flex()
                .gap_2()
                .child(
                    spork_workbench::ActionButton::new("git-help", "general/questionMark")
                        .tooltip("Show help contents")
                        .on_click(move |_, _, cx| cx.open_url(&help)),
                )
                .child(options_menu)
                .child(div().flex_1())
                .child(
                    spork_workbench::Button::new("git-cancel", "Cancel")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .child(
                    spork_workbench::Button::new("git-ok", dialog.ok)
                        .primary(true)
                        .disabled(!ready)
                        .on_click(move |_, window, cx| {
                            let command = state.read(cx);
                            let chosen = Chosen {
                                remote: command.remote.clone(),
                                target: command.target.read(cx).value().trim().to_owned(),
                                options: command.options.clone(),
                            };
                            window.close_dialog(cx);
                            view.update(cx, |this, cx| run(this, chosen, window, cx));
                        }),
                );
            modal
                .title(dialog_title(&dialog.title))
                .w(px(560.))
                .close_button(false)
                .child(
                    v_flex()
                        .gap_3()
                        .child(row)
                        .when(!chips.is_empty(), |body| {
                            body.child(h_flex().flex_wrap().gap_1().children(chips))
                        })
                        .child(footer),
                )
        });
    }
}

/// The reference app's reset types, in its order: the combo's text and git's mode.
const RESET_TYPES: [(&str, ResetMode); 3] = [
    ("Mixed", ResetMode::Mixed),
    ("Soft", ResetMode::Soft),
    ("Hard", ResetMode::Hard),
];

/// The tooltip of Reset Type (`GitBundle` `reset.type.tooltip`), without its HTML.
const RESET_TYPE_TIP: &str = "The reset type (see also git reset man page):\nMixed resets index but not working tree\nSoft leaves index and working tree, just moves head pointer\nHard resets index and working tree. Changes in the working tree will be lost";

impl RepoView {
    /// The reference app's Reset Head dialog: the root, the current branch, the reset type (Mixed at
    /// first) and the commit to reset to (HEAD), which Validate checks.
    pub(crate) fn reset_head_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let root = SharedString::from(repo.root.display().to_string());
        let branch = repo.head.clone().unwrap_or_else(|| "(no branch)".into());
        let commit = cx.new(|cx| InputState::new(window, cx).default_value("HEAD"));
        let mode = cx.new(|_| 0usize);
        let view = cx.entity();
        window.open_dialog(cx, move |modal, _, cx| {
            let label = |text: &'static str| form_label(text).w(px(150.));
            let current = *mode.read(cx);
            let (mode_menu, view_validate, view_reset) = (mode.clone(), view.clone(), view.clone());
            let (commit_validate, commit_reset, mode_reset) =
                (commit.clone(), commit.clone(), mode.clone());
            let types = combo(
                "reset-type",
                RESET_TYPES.get(current).map_or("Mixed", |(name, _)| *name),
                false,
                move |menu, _, _| {
                    RESET_TYPES
                        .iter()
                        .enumerate()
                        .fold(menu, |menu, (ix, (name, _))| {
                            let mode = mode_menu.clone();
                            menu.item(PopupMenuItem::new(*name).on_click(move |_, window, cx| {
                                mode.update(cx, |mode, _| *mode = ix);
                                window.refresh();
                            }))
                        })
                },
            );
            let form = v_flex()
                .gap_2()
                .child(
                    h_flex().child(label("Git Root:")).child(
                        ComboBox::new("git-root", root.clone())
                            .flex_1()
                            .min_w_0()
                            .dropdown_menu({
                                let root = root.clone();
                                move |menu, _, _| menu.item(PopupMenuItem::new(root.clone()))
                            }),
                    ),
                )
                .child(
                    h_flex()
                        .h(px(metrics::CONTROL_HEIGHT))
                        .child(label("Current Branch:"))
                        .child(
                            div()
                                .id("current-branch")
                                .role(Role::Label)
                                .aria_label(branch.clone())
                                .child(branch.clone()),
                        ),
                )
                .child(
                    h_flex().child(label("Reset Type:")).child(
                        div()
                            .id("reset-type-tip")
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(RESET_TYPE_TIP)
                                    .build(window, cx)
                            })
                            .child(types),
                    ),
                )
                .child(
                    h_flex()
                        .child(label("To Commit:"))
                        .child(spork_workbench::TextField::new(&commit).flex_1().mr_2())
                        .child(
                            spork_workbench::Button::new("reset-validate", "Validate").on_click(
                                move |_, window, cx| {
                                    let expr = commit_validate.read(cx).value().trim().to_owned();
                                    view_validate.update(cx, |this, cx| {
                                        this.run_op(
                                            window,
                                            cx,
                                            move |root| {
                                                spork_git::revision::resolve_commit(root, &expr)
                                            },
                                            |_, (id, subject), window, cx| {
                                                let short: String = id.chars().take(8).collect();
                                                window.push_notification(
                                                    Notification::info(format!(
                                                        "{short} {subject}"
                                                    )),
                                                    cx,
                                                )
                                            },
                                        )
                                    })
                                },
                            ),
                        ),
                );
            let footer = h_flex()
                .gap_2()
                .child(
                    spork_workbench::ActionButton::new("reset-help", "general/questionMark")
                        .tooltip("Show help contents")
                        .on_click(|_, _, cx| cx.open_url("https://git-scm.com/docs/git-reset")),
                )
                .child(div().flex_1())
                .child(
                    spork_workbench::Button::new("reset-cancel", "Cancel")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .child(
                    spork_workbench::Button::new("reset-ok", "Reset")
                        .primary(true)
                        .on_click(move |_, window, cx| {
                            let expr = commit_reset.read(cx).value().trim().to_owned();
                            let mode = RESET_TYPES
                                .get(*mode_reset.read(cx))
                                .map_or(ResetMode::Mixed, |(_, mode)| *mode);
                            window.close_dialog(cx);
                            view_reset.update(cx, |this, cx| {
                                this.run_op(
                                    window,
                                    cx,
                                    move |root| {
                                        // what was typed is resolved first, so it can't pass
                                        // for one of reset's options
                                        let (id, _) =
                                            spork_git::revision::resolve_commit(root, &expr)?;
                                        spork_git::revision::reset(root, &id, mode)
                                    },
                                    |_, _, _, _| {},
                                )
                            })
                        }),
                );
            modal
                .title(dialog_title("Reset Head"))
                .w(px(640.))
                .close_button(false)
                .child(v_flex().gap_4().child(form).child(footer))
        });
    }

    /// The reference app's Git Remotes dialog: every remote's name and URL, with Add, Remove and Edit.
    pub(crate) fn remotes_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = cx.new(|_| None::<String>);
        let view = cx.entity();
        window.open_dialog(cx, move |modal, _, cx| {
            let tokens = spork_workbench::Tokens::get(cx);
            let remotes = match &view.read(cx).repo {
                Some(Ok(repo)) => repo.remotes.clone(),
                _ => Vec::new(),
            };
            let chosen = selected
                .read(cx)
                .clone()
                .filter(|name| remotes.iter().any(|r| &r.name == name));
            let (add_view, remove_view, edit_view) = (view.clone(), view.clone(), view.clone());
            let (remove_name, edit_remote) = (
                chosen.clone(),
                chosen
                    .as_ref()
                    .and_then(|name| remotes.iter().find(|r| &r.name == name).cloned()),
            );
            let toolbar = h_flex()
                .p_1()
                .gap_1()
                .border_b_1()
                .border_color(tokens.separator)
                .child(
                    spork_workbench::ActionButton::new("remote-add", "general/add")
                        .tooltip("Add")
                        .on_click(move |_, window, cx| {
                            add_view
                                .update(cx, |this, cx| this.define_remote_dialog(None, window, cx))
                        }),
                )
                .child(
                    spork_workbench::ActionButton::new("remote-remove", "general/remove")
                        .tooltip("Remove")
                        .disabled(remove_name.is_none())
                        .on_click(move |_, window, cx| {
                            if let Some(name) = remove_name.clone() {
                                remove_view.update(cx, |this, cx| {
                                    this.remove_remote_dialog(name, window, cx)
                                })
                            }
                        }),
                )
                .child(
                    spork_workbench::ActionButton::new("remote-edit", "general/edit")
                        .tooltip("Edit")
                        .disabled(edit_remote.is_none())
                        .on_click(move |_, window, cx| {
                            if let Some(remote) = edit_remote.clone() {
                                edit_view.update(cx, |this, cx| {
                                    this.define_remote_dialog(Some(remote), window, cx)
                                })
                            }
                        }),
                );
            let cell = |text: SharedString, width: Option<f32>| {
                div()
                    .px_2()
                    .truncate()
                    .map(|cell| match width {
                        Some(width) => cell.w(px(width)).flex_none(),
                        None => cell.flex_1().min_w_0(),
                    })
                    .child(text)
            };
            let header = h_flex()
                .h(px(metrics::ROW_HEIGHT))
                .border_b_1()
                .border_color(tokens.separator)
                .text_color(tokens.text_secondary)
                .child(
                    cell("Name".into(), Some(150.))
                        .border_r_1()
                        .border_color(tokens.separator),
                )
                .child(cell("URL".into(), None));
            let rows = remotes.iter().enumerate().map(|(ix, remote)| {
                let is_selected = chosen.as_deref() == Some(remote.name.as_str());
                let (selected, name) = (selected.clone(), remote.name.clone());
                h_flex()
                    .id(("remote", ix))
                    .role(Role::Row)
                    .aria_label(remote.name.clone())
                    .aria_selected(is_selected)
                    .h(px(metrics::ROW_HEIGHT))
                    .when(is_selected, |row| row.bg(tokens.selection))
                    .child(cell(remote.name.clone().into(), Some(150.)))
                    .child(cell(remote.url.clone().into(), None))
                    .on_click(move |_, window, cx| {
                        selected.update(cx, |selected, _| *selected = Some(name.clone()));
                        window.refresh();
                    })
            });
            let table = v_flex()
                .h(px(220.))
                .border_1()
                .border_color(tokens.control_border)
                .child(toolbar)
                .child(header)
                .child(
                    div()
                        .id("remotes")
                        .role(Role::Table)
                        .aria_label("Remote List")
                        .flex_1()
                        .overflow_y_scroll()
                        .children(rows),
                );
            modal
                .title(dialog_title("Git Remotes"))
                .w(px(460.))
                .close_button(false)
                .child(
                    v_flex().gap_4().child(table).child(
                        h_flex().justify_end().child(
                            spork_workbench::Button::new("remotes-ok", "OK")
                                .primary(true)
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        ),
                    ),
                )
        });
    }

    /// The reference app's Define Remote dialog, to add a remote or, given one, change it.
    fn define_remote_dialog(
        &mut self,
        remote: Option<spork_git::remote::Remote>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(repo)) = &self.repo else { return };
        let taken: Vec<String> = repo
            .remotes
            .iter()
            .map(|r| r.name.clone())
            .filter(|name| remote.as_ref().is_none_or(|r| &r.name != name))
            .collect();
        // The reference app suggests origin for a repository's first remote
        let (name, url) = match &remote {
            Some(remote) => (remote.name.clone(), remote.url.clone()),
            None if taken.is_empty() => ("origin".into(), String::new()),
            None => (String::new(), String::new()),
        };
        let push = remote
            .as_ref()
            .and_then(|r| spork_git::remote::push_url(&repo.root, &r.name))
            .unwrap_or_default();
        let name = cx.new(|cx| InputState::new(window, cx).default_value(name));
        let url = cx.new(|cx| InputState::new(window, cx).default_value(url));
        let error = cx.new(|_| None::<&'static str>);
        let view = cx.entity();
        window.open_dialog(cx, move |modal, _, cx| {
            let tokens = spork_workbench::Tokens::get(cx);
            let label = |text: &'static str| form_label(text).w(px(50.));
            let (view, name, url, error, taken) = (
                view.clone(),
                name.clone(),
                url.clone(),
                error.clone(),
                taken.clone(),
            );
            let (old, push) = (remote.as_ref().map(|r| r.name.clone()), push.clone());
            let message = *error.read(cx);
            modal
                .title(dialog_title("Define Remote"))
                .w(px(460.))
                .close_button(false)
                .child(
                    v_flex()
                        .gap_2()
                        .child(
                            h_flex()
                                .child(label("Name:"))
                                .child(spork_workbench::TextField::new(&name).flex_1()),
                        )
                        .child(
                            h_flex()
                                .child(label("URL:"))
                                .child(spork_workbench::TextField::new(&url).flex_1()),
                        )
                        .children(message.map(|message| {
                            div()
                                .id("remote-error")
                                .role(Role::Label)
                                .aria_label(message)
                                .text_color(tokens.text_error)
                                .child(message)
                        }))
                        .child(
                            h_flex()
                                .pt_2()
                                .gap_2()
                                .justify_end()
                                .child(
                                    spork_workbench::Button::new("remote-cancel", "Cancel")
                                        .on_click(|_, window, cx| window.close_dialog(cx)),
                                )
                                .child(
                                    spork_workbench::Button::new("remote-ok", "OK")
                                        .primary(true)
                                        .on_click(move |_, window, cx| {
                                            let new = name.read(cx).value().trim().to_owned();
                                            let address = url.read(cx).value().trim().to_owned();
                                            let problem = if new.is_empty() {
                                                Some("Remote name cannot be empty")
                                            } else if address.is_empty() {
                                                Some("Remote URL cannot be empty")
                                            } else if taken.contains(&new) {
                                                Some("Remote name is already used")
                                            } else {
                                                None
                                            };
                                            if problem.is_some() {
                                                error.update(cx, |error, _| *error = problem);
                                                return window.refresh();
                                            }
                                            window.close_dialog(cx);
                                            let (old, push) = (old.clone(), push.clone());
                                            view.update(cx, |this, cx| {
                                                this.run_op(
                                                    window,
                                                    cx,
                                                    move |root| match old {
                                                        Some(old) => spork_git::remote::edit(
                                                            root, &old, &new, &address, &push,
                                                        ),
                                                        None => spork_git::remote::add(
                                                            root, &new, &address,
                                                        ),
                                                    },
                                                    |_, _, _, _| {},
                                                )
                                            })
                                        }),
                                ),
                        ),
                )
        });
    }

    /// The reference app's Remove Remote confirmation.
    fn remove_remote_dialog(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let message = format!("Remove remote {name}?");
        self.confirm_dialog(
            "Remove Remote",
            message,
            "Remove",
            window,
            cx,
            move |this, window, cx| {
                let name = name.clone();
                this.run_op(
                    window,
                    cx,
                    move |root| spork_git::remote::remove(root, &name),
                    |_, _, _, _| {},
                )
            },
        );
    }

    /// The reference app's yes/no question: `title`, `message`, Cancel and `yes`, which runs `on_yes`.
    pub(crate) fn confirm_dialog(
        &mut self,
        title: &'static str,
        message: String,
        yes: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
        on_yes: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let (view, on_yes, message) = (cx.entity(), Rc::new(on_yes), SharedString::from(message));
        window.open_dialog(cx, move |modal, _, _| {
            let (view, on_yes) = (view.clone(), on_yes.clone());
            modal
                .title(dialog_title(title))
                .w(px(420.))
                .close_button(false)
                .child(
                    v_flex()
                        .gap_4()
                        .child(
                            div()
                                .id("confirm-message")
                                .role(Role::Label)
                                .aria_label(message.clone())
                                .child(message.clone()),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .justify_end()
                                .child(
                                    spork_workbench::Button::new("confirm-cancel", "Cancel")
                                        .on_click(|_, window, cx| window.close_dialog(cx)),
                                )
                                .child(
                                    spork_workbench::Button::new("confirm-yes", yes)
                                        .primary(true)
                                        .on_click(move |_, window, cx| {
                                            window.close_dialog(cx);
                                            view.update(cx, |this, cx| on_yes(this, window, cx))
                                        }),
                                ),
                        ),
                )
        });
    }
}

/// A hosting service and its host, whose account's repositories a Clone source lists.
type Hosted = Option<(Service, &'static str)>;

/// The Clone dialog's GitHub and GitLab sources, which Git ▸ GitHub / GitLab ▸ Clone Repository
/// from… open it on.
pub(crate) const CLONE_GITHUB: usize = 1;
pub(crate) const CLONE_GITLAB: usize = 3;

/// The Clone dialog's left column: where the repository comes from, and its icon.
const CLONE_SOURCES: [(&str, Hosted, &str); 4] = [
    ("Repository URL", None, "general/vcs"),
    (
        "GitHub",
        Some((Service::GitHub, "github.com")),
        "github/pullRequests",
    ),
    (
        "GitHub Enterprise",
        Some((Service::GitHub, "")),
        "github/pullRequests",
    ),
    (
        "GitLab",
        Some((Service::GitLab, "gitlab.com")),
        "gitlab/gitLabToolWindow",
    ),
];

/// Where the reference app's Clone dialog suggests cloning to: the first source folder (Settings), else
/// `~/SporkProjects`, as the reference app suggests its own projects folder.
fn clone_parent(prefs: &crate::prefs::Prefs) -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    prefs
        .source_folders
        .split(';')
        .map(str::trim)
        .find(|folder| !folder.is_empty())
        .map(|folder| match folder.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => PathBuf::from(folder),
        })
        .unwrap_or_else(|| home.join("SporkProjects"))
}

impl RepoView {
    /// The reference app's Clone dialog (Git ▸ Clone…, Get from Version Control…): the repository's URL,
    /// the folder it goes in (which follows the URL until edited), a shallow clone's depth.
    pub(crate) fn clone_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clone_dialog_on(0, window, cx)
    }

    /// The Clone dialog open on `source` (a `CLONE_SOURCES` index). A hosting service's source
    /// without an account asks to log in, as the reference app's does.
    pub(crate) fn clone_dialog_on(
        &mut self,
        source: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let parent = clone_parent(&self.prefs);
        let source = cx.new(|_| source);
        // read once: git config is a process each time and the dialog redraws often
        // shortcut: an account added while the dialog is open shows once it opens again
        let users: Vec<Vec<String>> = CLONE_SOURCES
            .iter()
            .map(|(_, from, _)| {
                from.map_or_else(Vec::new, |(service, host)| {
                    crate::hosting::clone_source_users(service, host)
                })
            })
            .collect();
        let url = cx.new(|cx| InputState::new(window, cx));
        let folder =
            cx.new(|cx| InputState::new(window, cx).default_value(parent.display().to_string()));
        let depth = cx.new(|cx| InputState::new(window, cx).default_value("1"));
        let shallow = cx.new(|_| false);
        // the folder follows the URL, as the reference app's does
        cx.subscribe_in(&url, window, {
            let folder = folder.clone();
            move |_, url, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    let name = spork_git::repo_name_from_url(&url.read(cx).value());
                    let path = parent.join(name).display().to_string();
                    folder.update(cx, |folder, cx| folder.set_value(path, window, cx));
                }
            }
        })
        .detach();
        let view = cx.entity();
        window.open_dialog(cx, move |modal, _, cx| {
            let tokens = spork_workbench::Tokens::get(cx);
            let shown = *source.read(cx);
            let sources = CLONE_SOURCES.iter().zip(&users).enumerate().map(
                |(ix, ((name, from, icon), users))| {
                    let (view, url, source) = (view.clone(), url.clone(), source.clone());
                    let signed_in = !users.is_empty();
                    let accounts = match signed_in {
                        true => users.join(", "),
                        false => "No accounts".to_owned(),
                    };
                    h_flex()
                        .id(("clone-source", ix))
                        .role(Role::Tab)
                        .aria_label(*name)
                        .aria_selected(ix == shown)
                        .px_3()
                        .py_2()
                        .gap_2()
                        .items_start()
                        .cursor_pointer()
                        .when(ix == shown, |row| row.bg(tokens.selection))
                        .child(spork_workbench::icon(icon).size(px(16.)))
                        .child(
                            v_flex()
                                .child(div().font_weight(FontWeight::SEMIBOLD).child(*name))
                                .when(from.is_some(), |source| {
                                    source.child(
                                        div()
                                            .id(("clone-accounts", ix))
                                            .role(Role::Label)
                                            .aria_label(accounts.clone())
                                            .text_color(tokens.text_secondary)
                                            .child(accounts),
                                    )
                                }),
                        )
                        .on_click(move |_, window, cx| {
                            source.update(cx, |shown, cx| {
                                *shown = ix;
                                cx.notify();
                            });
                            window.refresh();
                            // an account's repositories, to pick the one to clone
                            if let Some((service, host)) = from.filter(|_| signed_in) {
                                let url = url.clone();
                                view.update(cx, |this, cx| {
                                    this.account_repos(service, host, url, window, cx)
                                })
                            }
                        })
                },
            );
            let login = CLONE_SOURCES
                .get(shown)
                .and_then(|(_, from, _)| *from)
                .filter(|_| users.get(shown).is_none_or(Vec::is_empty))
                .map(|(service, host)| clone_login(&view, service, host, cx));
            let label = |text: &'static str| form_label(text).w(px(90.));
            let shallow_on = *shallow.read(cx);
            // The reference app's GitLab login leaves Clone enabled, its GitHub ones don't
            let ready = match &login {
                Some(_) => shown == CLONE_GITLAB,
                None => !url.read(cx).value().trim().is_empty(),
            };
            let toggle = shallow.clone();
            let (open_view, browse_view, browse_folder) =
                (view.clone(), view.clone(), folder.clone());
            let (ok_view, ok_url, ok_folder, ok_depth, ok_shallow) = (
                view.clone(),
                url.clone(),
                folder.clone(),
                depth.clone(),
                shallow.clone(),
            );
            let form = v_flex()
                .flex_1()
                .p_4()
                .gap_3()
                .child(
                    h_flex()
                        .gap_2()
                        .child(form_label("Version control:"))
                        .child(combo("clone-vcs", "Git", false, |menu, _, _| {
                            menu.item(PopupMenuItem::new("Git"))
                        }))
                        .child(
                            spork_workbench::controls::link(
                                "clone-plugins",
                                "More via plugins…",
                                cx,
                            )
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                open_view.update(cx, |this, cx| {
                                    this.settings_page(Some("Plugins".into()), window, cx)
                                })
                            }),
                        ),
                )
                .child(
                    h_flex()
                        .child(label("URL:"))
                        .child(spork_workbench::TextField::new(&url).flex_1()),
                )
                .child(
                    h_flex().child(label("Clone into:")).child(
                        spork_workbench::TextField::new(&folder)
                            .suffix(
                                spork_workbench::ActionButton::new("clone-browse", "nodes/folder")
                                    .tooltip("Browse…")
                                    .on_click(move |_, window, cx| {
                                        let folder = browse_folder.clone();
                                        browse_view.update(cx, |this, cx| {
                                            this.pick_folder(
                                                "Clone into",
                                                window,
                                                cx,
                                                move |_, picked, window, cx| {
                                                    folder.update(cx, |folder, cx| {
                                                        folder.set_value(
                                                            picked.display().to_string(),
                                                            window,
                                                            cx,
                                                        )
                                                    })
                                                },
                                            )
                                        })
                                    }),
                            )
                            .flex_1(),
                    ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            spork_workbench::controls::labeled_check(
                                "clone-shallow",
                                "Shallow clone with a history truncated to",
                                shallow_on,
                                cx,
                            )
                            .on_click(move |_, window, cx| {
                                toggle.update(cx, |on, _| *on = !*on);
                                window.refresh();
                            }),
                        )
                        .child(
                            div()
                                .w(px(64.))
                                .when(!shallow_on, |field| field.opacity(0.5))
                                .child(spork_workbench::TextField::new(&depth)),
                        )
                        .child("commits"),
                );
            let footer = h_flex()
                .p_3()
                .gap_2()
                .justify_end()
                .border_t_1()
                .border_color(tokens.separator)
                .child(
                    spork_workbench::Button::new("clone-cancel", "Cancel")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .child(
                    spork_workbench::Button::new("clone-ok", "Clone")
                        .primary(true)
                        .disabled(!ready)
                        .on_click(move |_, window, cx| {
                            let url = ok_url.read(cx).value().trim().to_owned();
                            if url.is_empty() {
                                return;
                            }
                            let dest = PathBuf::from(ok_folder.read(cx).value().trim());
                            let depth = ok_depth.read(cx).value().trim().parse::<u32>().ok();
                            let depth = depth.filter(|_| *ok_shallow.read(cx));
                            let (Some(parent), Some(name)) = (
                                dest.parent().map(PathBuf::from),
                                dest.file_name().map(|n| n.to_string_lossy().into_owned()),
                            ) else {
                                return;
                            };
                            window.close_dialog(cx);
                            ok_view.update(cx, |this, cx| {
                                this.run_op(
                                    window,
                                    cx,
                                    move |_| {
                                        std::fs::create_dir_all(&parent).map_err(|e| {
                                            GitError::Other {
                                                code: None,
                                                stderr: e.to_string(),
                                            }
                                        })?;
                                        spork_git::clone(&url, &parent, &name, "", depth)
                                    },
                                    |this, dest, _, cx| this.switch_repo(dest, cx),
                                )
                            })
                        }),
                );
            modal
                .title(div().px_4().pt_3().child(dialog_title("Clone Repository")))
                .w(px(820.))
                .p_0()
                .close_button(false)
                .child(
                    v_flex()
                        .h(px(520.))
                        .child(
                            h_flex()
                                .flex_1()
                                .items_start()
                                .child(
                                    v_flex()
                                        .w(px(200.))
                                        .h_full()
                                        .border_r_1()
                                        .border_color(tokens.separator)
                                        .children(sources),
                                )
                                .map(|pane| match login {
                                    Some(login) => pane.child(login),
                                    None => pane.child(form),
                                }),
                        )
                        .child(footer),
                )
        });
    }
}

/// A Clone source's pane while it has no account: the reference app's "Log In to GitHub" and its ways in.
fn clone_login(view: &Entity<RepoView>, service: Service, host: &'static str, cx: &App) -> Div {
    use spork_workbench::Typography as _;
    let enterprise = service == Service::GitHub && host.is_empty();
    let title = match (service, enterprise) {
        (Service::GitLab, _) => "Log In to GitLab",
        (_, true) => "Log In to GitHub Enterprise",
        _ => "Log In to GitHub",
    };
    let connect = move |view: &Entity<RepoView>, host: &'static str| {
        let view = view.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            view.update(cx, |this, cx| {
                this.connect_service(service, host.to_owned(), window, cx)
            })
        }
    };
    let button = match (service, enterprise) {
        (Service::GitLab, _) => {
            spork_workbench::controls::OptionButton::new("clone-login", "Log In to gitlab.com…", {
                let view = view.clone();
                move |menu, _, _| {
                    menu.item(
                        PopupMenuItem::new("Log In to Custom Server…").on_click(connect(&view, "")),
                    )
                }
            })
            .plain()
            .on_click(connect(view, host))
            .into_any_element()
        }
        (_, true) => spork_workbench::Button::new("clone-login", "Log In to GitHub Enterprise…")
            .on_click(connect(view, host))
            .into_any_element(),
        _ => {
            let view = view.clone();
            spork_workbench::Button::new("clone-login", "Log In via GitHub…")
                .on_click(move |_, window, cx| {
                    view.update(cx, |this, cx| this.log_in_via_github(window, cx))
                })
                .into_any_element()
        }
    };
    v_flex()
        .flex_1()
        .p_4()
        .gap_3()
        .child(
            div()
                .id("clone-login-title")
                .role(Role::Heading)
                .aria_label(title)
                .text_h2()
                .child(title),
        )
        .child(h_flex().gap_3().child(button).when(!enterprise, |row| {
            row.child(form_label("or")).child(
                spork_workbench::controls::link("clone-use-token", "Use token…", cx)
                    .on_click(connect(view, host)),
            )
        }))
}

/// What a row of the VCS Operations popup does.
type Run = Rc<dyn Fn(&mut RepoView, &mut Window, &mut Context<RepoView>)>;

/// A row of the VCS Operations popup.
struct Operation {
    label: SharedString,
    icon: Option<&'static str>,
    shortcut: Option<&'static str>,
    /// `None`: greyed out.
    run: Option<Run>,
}

impl Operation {
    fn new(label: impl Into<SharedString>, icon: Option<&'static str>) -> Self {
        Self {
            label: label.into(),
            icon,
            shortcut: None,
            run: None,
        }
    }

    fn shortcut(mut self, shortcut: &'static str) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    /// Runs `run` when `enabled`.
    fn runs(
        mut self,
        enabled: bool,
        run: impl Fn(&mut RepoView, &mut Window, &mut Context<RepoView>) + 'static,
    ) -> Self {
        self.run = enabled.then(|| Rc::new(run) as Rc<_>);
        self
    }
}

impl RepoView {
    /// The reference app's VCS Operations popup (⌃V): the common Git actions for where you are, the first
    /// ten numbered, a digit running its row.
    pub(crate) fn vcs_operations_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let file = self.change_selected.clone();
        let tracked: Vec<String> = repo
            .status
            .iter()
            .filter(|entry| !entry.untracked() && !entry.ignored() && !entry.conflicted)
            .map(|entry| entry.path.clone())
            .collect();
        let has_stashes = !repo.stashes.is_empty();
        let untracked_file = file.as_ref().filter(|file| {
            repo.status
                .iter()
                .any(|entry| entry.path == file.path && entry.untracked())
        });
        let history = match &file {
            Some(file) => format!("Show History for {}", file_name(&file.path)),
            None => "Show History".into(),
        };
        let (history_path, add_path) = (
            file.as_ref().map(|f| f.path.clone()),
            untracked_file.map(|f| f.path.clone()),
        );
        let rows: Vec<Option<Operation>> = vec![
            Some(
                Operation::new("Commit…", Some("vcs/commit"))
                    .shortcut("⌘K")
                    .runs(true, |this, window, cx| this.show_commit_window(window, cx)),
            ),
            // The reference app's staging-area Commit…, shown greyed while that area is off
            Some(Operation::new("Commit…", None)),
            Some(
                Operation::new("Rollback…", Some("general/undo"))
                    .shortcut("⌥⌘Z")
                    .runs(!tracked.is_empty(), |this, window, cx| {
                        this.rollback(window, cx)
                    }),
            ),
            Some(Operation::new(history, Some("general/history")).runs(
                history_path.is_some(),
                move |this, _, cx| {
                    if let Some(path) = history_path.clone() {
                        this.open_file(path, false, cx)
                    }
                },
            )),
            Some(
                Operation::new("Show Diff", Some("vcs/diff"))
                    .shortcut("⌘D")
                    .runs(file.is_some(), |this, _, cx| {
                        this.open_diff_view(crate::View::Changes, cx);
                        cx.notify();
                    }),
            ),
            None,
            Some(
                Operation::new("Branches…", Some("general/vcs")).runs(true, |this, window, cx| {
                    this.toggle_branches_popup(window, cx)
                }),
            ),
            Some(
                Operation::new("Push…", Some("vcs/push"))
                    .shortcut("⇧⌘K")
                    .runs(true, |this, window, cx| {
                        this.push_commits_dialog(window, cx)
                    }),
            ),
            Some(
                Operation::new("Stash Changes…", None)
                    .runs(true, |this, window, cx| this.stash_dialog(window, cx)),
            ),
            Some(
                Operation::new("Unstash Changes…", None).runs(has_stashes, |this, window, cx| {
                    this.show_saved(crate::stash_view::CommitTab::Stash, window, cx)
                }),
            ),
            None,
            Some(
                Operation::new("Worktrees…", Some("vcs/worktree"))
                    .runs(true, |this, _, cx| this.show_worktrees(cx)),
            ),
            Some(Operation::new("Add to VCS", None).runs(
                add_path.is_some(),
                move |this, window, cx| {
                    if let Some(path) = add_path.clone() {
                        this.run_op(
                            window,
                            cx,
                            move |root| spork_git::stage(root, &[&path]),
                            |_, _, _, _| {},
                        )
                    }
                },
            )),
            None,
            // The reference app's local history, which Spork doesn't keep
            Some(Operation::new("Show History…", None)),
        ];
        let list = QuickList {
            title: Some("VCS Operations"),
            group: Some("Git"),
            numbered: true,
        };
        self.quick_list(list, rows, window, cx);
    }

    /// One of the reference app's quick lists (VCS Operations, Quick Switch Scheme): its title and group,
    /// the rows, the first ten numbered if asked, a digit running its row.
    fn quick_list(
        &mut self,
        list: QuickList,
        rows: Vec<Option<Operation>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = Rc::new(rows);
        let numbered: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter_map(|(ix, row)| row.as_ref().map(|_| ix))
            .take(if list.numbered { 10 } else { 0 })
            .collect();
        let focus = cx.focus_handle();
        let view = cx.entity();
        let keys_focus = focus.clone();
        window.open_dialog(cx, move |modal, _, cx| {
            let tokens = spork_workbench::Tokens::get(cx);
            let run = |ix: usize, view: &Entity<RepoView>, rows: &Rc<Vec<Option<Operation>>>| {
                let action = rows.get(ix).and_then(|row| row.as_ref()?.run.clone());
                let view = view.clone();
                move |window: &mut Window, cx: &mut App| {
                    if let Some(action) = action.clone() {
                        window.close_dialog(cx);
                        view.update(cx, |this, cx| action(this, window, cx));
                    }
                }
            };
            let rows_shown = rows.iter().enumerate().map(|(ix, row)| {
                let Some(row) = row else {
                    return spork_workbench::popup::separator(cx).into_any_element();
                };
                let number = numbered
                    .iter()
                    .position(|n| *n == ix)
                    .map(|n| ((n + 1) % 10).to_string());
                let color = match row.run.is_some() {
                    true => tokens.text,
                    false => tokens.text_disabled,
                };
                let click = run(ix, &view, &rows);
                spork_workbench::popup::row(("vcs-operation", ix), false, cx)
                    .role(Role::MenuItem)
                    .aria_label(row.label.clone())
                    .text_color(color)
                    .when(row.run.is_none(), |row| row.cursor_default())
                    .when(row.run.is_some(), |el| {
                        el.on_click(move |_, window, cx| click(window, cx))
                    })
                    .child(
                        div().w(px(16.)).flex_none().children(
                            row.icon
                                .map(|icon| spork_workbench::icon(icon).size(px(16.))),
                        ),
                    )
                    .when(list.numbered, |row| {
                        row.child(
                            div()
                                .w(px(10.))
                                .flex_none()
                                .text_color(tokens.text_secondary)
                                .children(number),
                        )
                    })
                    .child(row.label.clone())
                    .children(
                        row.shortcut
                            .map(|keys| spork_workbench::popup::hint(keys, false, cx)),
                    )
                    .into_any_element()
            });
            let (keys_view, keys_rows, keys_numbered) =
                (view.clone(), rows.clone(), numbered.clone());
            let menu = v_flex()
                .id("quick-list")
                .role(Role::Menu)
                .when_some(list.title, |menu, title| menu.aria_label(title))
                .track_focus(&keys_focus)
                .pt_1()
                .pb_1()
                .on_key_down(move |event: &KeyDownEvent, window, cx| {
                    let digit = event.keystroke.key.parse::<usize>().ok();
                    let ix = digit
                        .map(|d| (d + 9) % 10)
                        .and_then(|n| keys_numbered.get(n).copied());
                    if let Some(ix) = ix {
                        cx.stop_propagation();
                        run(ix, &keys_view, &keys_rows)(window, cx);
                    }
                })
                .when_some(list.title, |menu, title| {
                    menu.child(
                        div()
                            .pb_1()
                            .flex()
                            .justify_center()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                })
                .when_some(list.group, |menu, group| {
                    menu.child(spork_workbench::popup::labeled_separator(group, true, cx))
                })
                .children(rows_shown);
            modal.p_0().w(px(300.)).close_button(false).child(menu)
        });
        focus.focus(window, cx);
    }
}

impl RepoView {
    /// The reference app's Quick Switch Scheme (⌃`): what to switch, each opening its own list.
    pub(crate) fn quick_switch_scheme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let page = |path: &'static str| {
            move |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                this.settings_page(Some(path.into()), window, cx)
            }
        };
        let rows = vec![
            Some(
                Operation::new("Theme", None)
                    .runs(true, |this, window, cx| this.theme_list(window, cx)),
            ),
            Some(
                Operation::new("Editor Color Scheme", None)
                    .runs(true, |this, window, cx| this.color_scheme_list(window, cx)),
            ),
            Some(Operation::new("Code Style Scheme", None).runs(true, page("Editor/Code Style"))),
            Some(Operation::new("Keymap", None).runs(true, page("Keymap"))),
            Some(
                Operation::new("View Mode", None).runs(true, |this, window, cx| {
                    let rows = vec![Some(
                        Operation::new("Enter Full Screen", None)
                            .runs(true, |_, window, _| window.toggle_fullscreen()),
                    )];
                    this.quick_list(QuickList::plain(), rows, window, cx)
                }),
            ),
            Some(
                Operation::new("Layout", None).runs(true, |this, window, cx| {
                    let rows = vec![Some(Operation::new("Restore Default Layout", None).runs(
                        true,
                        |this, _, cx| {
                            this.left_tools = vec![crate::LeftTool::Project];
                            this.show_console = false;
                            this.show_terminal = false;
                            this.show_notifications = false;
                            cx.notify();
                        },
                    ))];
                    this.quick_list(QuickList::plain(), rows, window, cx)
                }),
            ),
            Some(Operation::new("Zoom", None).runs(true, |this, window, cx| {
                let step = |label: &'static str, step: i32| {
                    Some(
                        Operation::new(label, None)
                            .runs(true, move |this, window, cx| this.zoom(step, window, cx)),
                    )
                };
                let rows = vec![
                    step("Zoom In", 1),
                    step("Zoom Out", -1),
                    step("Reset Zoom", 0),
                ];
                this.quick_list(QuickList::plain(), rows, window, cx)
            })),
        ];
        self.quick_list(QuickList::plain(), rows, window, cx);
    }

    /// Quick Switch Scheme's Editor Color Scheme: the reference app's bundled schemes in its scheme combo's
    /// order, the one in use ticked.
    fn color_scheme_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = crate::settings_editor::current_scheme(cx);
        let rows = crate::settings_editor::SCHEMES
            .iter()
            .map(|&name| {
                Some(
                    Operation::new(name, (name == current).then_some("actions/checked"))
                        .runs(true, move |_, _, _| {
                            crate::settings_editor::set_scheme(name)
                        }),
                )
            })
            .collect();
        self.quick_list(QuickList::plain(), rows, window, cx);
    }

    /// Quick Switch Scheme's Theme: the reference app's two themes, the current one ticked.
    fn theme_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dark = gpui_kit::component::ActiveTheme::theme(&**cx)
            .mode
            .is_dark();
        let theme = |label: &'static str, on: bool, appearance: crate::prefs::Appearance| {
            Some(
                Operation::new(label, on.then_some("actions/checked"))
                    .runs(true, move |this, window, cx| {
                        this.set_appearance(appearance, window, cx)
                    }),
            )
        };
        let rows = vec![
            theme("Islands Dark", dark, crate::prefs::Appearance::Dark),
            theme("Islands Light", !dark, crate::prefs::Appearance::Light),
        ];
        self.quick_list(QuickList::plain(), rows, window, cx);
    }
}

impl QuickList {
    /// No title, no group, no numbers.
    fn plain() -> Self {
        Self {
            title: None,
            group: None,
            numbered: false,
        }
    }
}

/// How a quick list looks: its title, its group's name, numbered rows.
#[derive(Clone, Copy)]
struct QuickList {
    title: Option<&'static str>,
    group: Option<&'static str>,
    numbered: bool,
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// A form's label ("To Commit:"), in front of its field.
pub(crate) fn form_label(text: impl Into<SharedString>) -> Stateful<Div> {
    let text = text.into();
    div()
        .id(text.clone())
        .role(Role::Label)
        .aria_label(text.clone())
        .flex_none()
        .child(text)
}

/// A dialog's title, which accessibility reads as its heading.
pub(crate) fn dialog_title(title: &str) -> impl IntoElement + use<> {
    let title = SharedString::from(title.to_owned());
    div()
        .id("dialog-title")
        .role(Role::Heading)
        .aria_label(title.clone())
        .child(title)
}
