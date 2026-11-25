//! Merge / rebase / cherry-pick / revert flows, the paused-operation banner, commit menu and
//! stashes (docs/specs/M11, M06, M12).

use crate::notices::Notification;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::diff::{Diff, DiffState};
use gpui_kit::component::input::{InputState, Textarea};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_git::revision::ResetMode;
use spork_git::{GitError, OpState, Outcome, Stash};
use spork_workbench::tree::TreeItem;

use crate::util::ResultExt as _;
use crate::{RepoView, View};

impl RepoView {
    /// Runs an operation that may stop on conflicts. Conflicts switch to Local Changes with a hint.
    pub(crate) fn run_outcome(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        done: String,
        op: impl FnOnce(&std::path::Path) -> Result<Outcome, GitError> + Send + 'static,
    ) {
        self.run_op(
            window,
            cx,
            op,
            move |this, outcome, window, cx| match outcome {
                Outcome::Done => window.push_notification(Notification::success(done), cx),
                Outcome::Conflicts => {
                    this.open_left(crate::LeftTool::Commit, cx);
                    window.push_notification(
                        Notification::warning(
                            "Stopped on conflicts. Resolve the files marked U, then Continue.",
                        ),
                        cx,
                    );
                }
            },
        );
    }

    fn op_state(&self) -> OpState {
        match &self.repo {
            Some(Ok(repo)) => repo.op,
            _ => OpState::None,
        }
    }

    /// Banner shown while git has paused a merge / rebase / cherry-pick / revert (M11 §0).
    pub(crate) fn op_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        let state = self.op_state();
        let title = match state {
            OpState::None => return None,
            OpState::Merging => "Merge in progress".to_owned(),
            OpState::Rebasing { step, total } => {
                format!("Rebase in progress, commit {step} of {total}")
            }
            OpState::CherryPicking => "Cherry-pick in progress".to_owned(),
            OpState::Reverting => "Revert in progress".to_owned(),
            OpState::Applying { step, total } => {
                format!("Applying patches, {step} of {total}")
            }
        };
        let conflicts = match &self.repo {
            Some(Ok(repo)) => repo.status.iter().filter(|e| e.conflicted).count(),
            _ => 0,
        };
        let hint = match conflicts {
            0 => "No conflicts left: continue when ready.".to_owned(),
            1 => "1 conflicted file to resolve.".to_owned(),
            n => format!("{n} conflicted files to resolve."),
        };
        let theme = cx.theme();
        let can_skip = !matches!(state, OpState::Merging);
        Some(
            h_flex()
                .flex_none()
                .px_4()
                .py_2()
                .gap_3()
                .justify_between()
                .bg(theme.yellow.opacity(0.12))
                .border_b_1()
                .border_color(theme.yellow.opacity(0.3))
                .child(
                    v_flex()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(hint),
                        ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("op-abort")
                                .small()
                                .ghost()
                                .label("Abort")
                                .disabled(self.busy)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.confirm_abort(window, cx)
                                })),
                        )
                        .when(can_skip, |h| {
                            h.child(
                                Button::new("op-skip")
                                    .small()
                                    .ghost()
                                    .label("Skip commit")
                                    .disabled(self.busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.run_outcome(
                                            window,
                                            cx,
                                            "Skipped".into(),
                                            spork_git::skip_op,
                                        )
                                    })),
                            )
                        })
                        .child(
                            Button::new("op-continue")
                                .small()
                                .primary()
                                .label("Continue")
                                .disabled(self.busy || conflicts > 0)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.run_outcome(
                                        window,
                                        cx,
                                        "Done".into(),
                                        spork_git::continue_op,
                                    )
                                })),
                        ),
                ),
        )
    }

    pub(crate) fn confirm_abort(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let view = view.clone();
            dialog
                .title("Abort the operation?")
                .description("Your branch goes back to how it was before it started. Resolved conflicts are lost.")
                .confirm()
                .ok_text("Abort")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        this.run_op(window, cx, spork_git::abort_op, |_, _, window, cx| {
                            window.push_notification(Notification::info("Aborted"), cx)
                        })
                    });
                    true
                })
        });
    }

    /// Takes one side of a conflicted file whole (M11 §4).
    pub(crate) fn resolve_conflict(
        &mut self,
        path: String,
        ours: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_op(
            window,
            cx,
            move |root| spork_git::resolve_with(root, &path, ours),
            |_, _, _, _| {},
        );
    }

    /// A dialog with one text field; `op` gets the trimmed text and returns the success message,
    /// or an `https://` page to open. The confirm button repeats the title ("Rename stash").
    pub(crate) fn input_dialog(
        &mut self,
        title: String,
        hint: String,
        value: String,
        op: impl Fn(&std::path::Path, String) -> Result<String, GitError> + Clone + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.field
            .update(cx, |s, cx| s.set_value(value, window, cx));
        let (view, input) = (cx.entity(), self.field.clone());
        let muted = cx.theme().muted_foreground;
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, input, op) = (view.clone(), input.clone(), op.clone());
            dialog
                .title(title.clone())
                .description(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().text_color(muted).child(hint.clone()))
                        .child(spork_workbench::TextField::new(&input)),
                )
                .confirm()
                .ok_text(title.clone())
                .on_ok(move |_, window, cx| {
                    let (text, op) = (input.read(cx).value().trim().to_owned(), op.clone());
                    view.update(cx, |this, cx| {
                        this.run_op(
                            window,
                            cx,
                            move |root| op(root, text),
                            |_, msg, window, cx| {
                                // an op may answer with a page to open, e.g. a new pull request
                                if msg.starts_with("https://") {
                                    cx.open_url(&msg);
                                }
                                window.push_notification(Notification::success(msg), cx)
                            },
                        )
                    });
                    true
                })
        });
    }

    /// `origin`, else the first remote; where tags are pushed to and deleted from.
    pub(crate) fn default_remote(&self) -> Option<String> {
        let Some(Ok(repo)) = &self.repo else {
            return None;
        };
        let names = || repo.remotes.iter().map(|r| r.name.clone());
        names().find(|n| n == "origin").or_else(|| names().next())
    }

    /// Push was rejected because the remote branch moved on: pull first, or overwrite it safely (M09 §3).
    pub(crate) fn push_rejected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (pull_view, force_view) = (view.clone(), view.clone());
            dialog
                .title("Push rejected")
                .description(
                    v_flex()
                        .gap_3()
                        .child("The remote branch has commits you don't have. Pull them first, or overwrite the remote branch. Force push uses --force-with-lease: it refuses if anyone pushed since your last fetch.")
                        .child(
                            Button::new("force-with-lease")
                                .danger()
                                .small()
                                .label("Force push (with lease)")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    force_view.update(cx, |this, cx| {
                                        this.run_op(window, cx, spork_git::push_force_with_lease, |_, msg, window, cx| {
                                            window.push_notification(Notification::success(msg), cx)
                                        })
                                    })
                                }),
                        ),
                )
                .confirm()
                .ok_text("Pull")
                .on_ok(move |_, window, cx| {
                    pull_view.update(cx, |this, cx| this.remote_op(crate::RemoteOp::Pull, window, cx));
                    true
                })
        });
    }

    /// Repository menu: apply a `.patch` file as commits (`git am`) or as changes (`git apply`) (M06 §3).
    pub(crate) fn apply_patch_dialog(
        &mut self,
        commit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Apply patch".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut picked))) = paths.await else {
                return;
            };
            let Some(file) = picked.pop() else { return };
            this.update_in(cx, |this, window, cx| {
                let done = if commit {
                    "Patch applied as commits"
                } else {
                    "Patch applied to your changes"
                };
                this.run_outcome(window, cx, done.into(), move |root| {
                    spork_git::apply_mailbox(root, &file, commit)
                })
            })
            .ok();
        })
        .detach();
    }

    /// Repository ▸ Undo Last Change: names what will be taken back and asks first.
    pub(crate) fn undo_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let Some((what, undoable)) = spork_git::revision::last_change(&repo.root) else {
            return window.push_notification(Notification::info("Nothing to undo yet"), cx);
        };
        if !undoable {
            return window.push_notification(
                Notification::info(format!("Spork can't undo \u{201c}{what}\u{201d}")),
                cx,
            );
        }
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let view = view.clone();
            dialog
                .title("Undo the last change?")
                .description(format!(
                    "\u{201c}{what}\u{201d}\n\nThe branch goes back to where it was before; commits it leaves behind stay in View ▸ Show Lost Commits. Uncommitted changes are kept, or git refuses."
                ))
                .confirm()
                .ok_text("Undo")
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        this.run_op(window, cx, spork_git::revision::undo_last, |_, msg, window, cx| {
                            window.push_notification(Notification::success(msg), cx)
                        })
                    });
                    true
                })
        });
    }

    /// The commit the current branch points at; `None` when HEAD is detached.
    pub(crate) fn head_commit(&self) -> Option<String> {
        let Some(Ok(repo)) = &self.repo else {
            return None;
        };
        Some(repo.refs.iter().find(|r| r.is_head)?.commit.clone())
    }

    /// Moves the current branch to `id` (M04 §7); a hard reset asks first.
    fn reset_to(
        &mut self,
        id: String,
        mode: ResetMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let short: String = id.chars().take(7).collect();
        let run = move |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            this.run_op(
                window,
                cx,
                move |root| spork_git::revision::reset(root, &id, mode),
                move |_, _, window, cx| {
                    window.push_notification(Notification::success(format!("Reset to {short}")), cx)
                },
            )
        };
        if mode != ResetMode::Hard {
            return run(self, window, cx);
        }
        let changed = match &self.repo {
            Some(Ok(repo)) => repo.status.iter().filter(|e| !e.untracked()).count(),
            _ => 0,
        };
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, run) = (view.clone(), run.clone());
            dialog
                .title("Hard reset?")
                .description(format!(
                    "Discards uncommitted changes in {changed} tracked file(s) for good. Commits after this one are only reachable from the reflog afterwards."
                ))
                .confirm()
                .ok_text("Reset and discard")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let run = run.clone();
                    view.update(cx, |this, cx| run(this, window, cx));
                    true
                })
        });
    }

    /// "Edit Commit Message" for `id`. HEAD is amended without adding staged changes; an older
    /// commit is reworded by replaying the commits after it.
    fn reword_dialog(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let is_head = self.head_commit().as_deref() == Some(id.as_str());
        let Some(Ok(repo)) = &self.repo else { return };
        let message = spork_git::commit_message(&repo.root, &id).unwrap_or_default();
        let author = repo
            .history
            .commits
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.author.clone())
            .unwrap_or_default();
        let short: String = id.chars().take(8).collect();
        let label = format!("Edit message for commit {short} by {author}");
        self.reword
            .update(cx, |s, cx| s.set_value(message, window, cx));
        let (view, input) = (cx.entity(), self.reword.clone());
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, input) = (view.clone(), input.clone());
            let id = id.clone();
            dialog
                .title("Edit Commit Message")
                .description(
                    v_flex()
                        .id("edit-commit-message")
                        .role(Role::Group)
                        .aria_label("Edit Commit Message")
                        .gap_2()
                        .child(label.clone())
                        .child(Textarea::new(&input).h(px(140.))),
                )
                .confirm()
                .on_ok(move |_, window, cx| {
                    let message = input.read(cx).value().to_string();
                    view.update(cx, |this, cx| {
                        let id = id.clone();
                        this.run_outcome(window, cx, "Commit message changed".into(), move |root| {
                            if is_head {
                                spork_git::revision::reword_head(root, &message)
                                    .map(|()| spork_git::Outcome::Done)
                            } else {
                                spork_git::reword(root, &id, &message)
                            }
                        })
                    });
                    true
                })
        });
        self.reword.update(cx, |s, cx| s.focus(window, cx));
    }

    /// Asks for a folder and writes the commit there as a `.patch` file (M06 §2).
    pub(crate) fn save_patch_dialog(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Save patch in".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut picked))) = paths.await else {
                return;
            };
            let Some(dir) = picked.pop() else { return };
            this.update_in(cx, |this, window, cx| {
                this.run_op(
                    window,
                    cx,
                    move |root| spork_git::revision::save_patch(root, &id, &dir),
                    |_, file, window, cx| {
                        window.push_notification(
                            Notification::success(format!("Saved {}", file.display())),
                            cx,
                        )
                    },
                )
            })
            .ok();
        })
        .detach();
    }

    /// Cherry-pick, Revert and Save as patches for the commits two selected ones span (M06 §1).
    pub(crate) fn run_actions(&self, cx: &mut Context<Self>) -> Div {
        let n = self.compare_run.len();
        let button =
            |id: &'static str,
             label: String,
             act: fn(&mut Self, Vec<String>, &mut Window, &mut Context<Self>)| {
                Button::new(id)
                    .xsmall()
                    .label(label)
                    .disabled(self.busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        act(this, this.compare_run.clone(), window, cx)
                    }))
            };
        h_flex()
            .gap_1()
            .pt_1()
            .child(button(
                "run-pick",
                format!("Cherry-pick {n} commits"),
                |this, run, window, cx| {
                    let done = format!("Cherry-picked {} commits", run.len());
                    this.run_outcome(window, cx, done, move |root| {
                        spork_git::revision::cherry_pick_many(root, &run)
                    })
                },
            ))
            .child(button(
                "run-squash",
                format!("Squash {n} into one"),
                |this, run, window, cx| {
                    this.run_outcome(window, cx, "Squashed".into(), move |root| {
                        spork_git::meld(root, &run, true)
                    })
                },
            ))
            .child(button(
                "run-drop",
                format!("Drop {n}…"),
                |this, run, window, cx| this.confirm_drop_commits(run, window, cx),
            ))
            .child(button(
                "run-revert",
                format!("Revert {n} commits"),
                |this, run, window, cx| {
                    let done = format!("Reverted {} commits", run.len());
                    this.run_outcome(window, cx, done, move |root| {
                        spork_git::revision::revert_many(root, &run)
                    })
                },
            ))
            .child(button(
                "run-patches",
                "Save as patches…".into(),
                |_, run, window, cx| {
                    let dir = cx.prompt_for_paths(PathPromptOptions {
                        files: false,
                        directories: true,
                        multiple: false,
                        prompt: Some("Save patches in".into()),
                    });
                    cx.spawn_in(window, async move |this, cx| {
                        let Ok(Ok(Some(mut picked))) = dir.await else {
                            return;
                        };
                        let Some(dir) = picked.pop() else { return };
                        this.update_in(cx, |this, window, cx| {
                            this.run_op(
                                window,
                                cx,
                                move |root| spork_git::revision::save_patches(root, &run, &dir),
                                |_, n, window, cx| {
                                    window.push_notification(
                                        Notification::success(format!("Saved {n} patches")),
                                        cx,
                                    )
                                },
                            )
                        })
                        .ok();
                    })
                    .detach();
                },
            ))
    }

    /// Folds or unfolds the branch merge commit `id` brought in (M05 §2); kept per repository.
    pub(crate) fn fold_merge(
        &mut self,
        id: String,
        fold: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(repo)) = &self.repo else { return };
        let folded = repo.history.folded.contains_key(&id);
        let is_merge = repo
            .history
            .commits
            .iter()
            .any(|c| c.id == id && c.parents.len() > 1);
        if fold == folded || (fold && !is_merge) {
            return;
        }
        self.run_op(
            window,
            cx,
            move |root| {
                if fold {
                    spork_git::config::add_local(root, "spork.folded", &id)
                } else {
                    spork_git::config::remove_local(root, "spork.folded", &id)
                }
            },
            |_, (), _, _| {},
        )
    }

    /// ← / → on the selected commit: fold or unfold its merged branch.
    pub(crate) fn fold_selected(
        &mut self,
        fold: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = self.selected_commit_id() {
            self.fold_merge(id, fold, window, cx)
        }
    }

    /// Asks before removing `ids` from the current branch's history.
    pub(crate) fn confirm_drop_commits(
        &mut self,
        ids: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        let count = ids.len();
        let (title, what) = match count {
            1 => ("Drop Commit", "commit"),
            _ => ("Drop Commits", "commits"),
        };
        let branch = match &self.repo {
            Some(Ok(repo)) => repo
                .refs
                .iter()
                .find(|r| r.is_head && r.kind == spork_git::RefKind::Local)
                .map(|r| r.name.clone()),
            _ => None,
        };
        let question = match branch {
            Some(branch) => {
                format!("Do you want to drop {count} {what} from the {branch} branch?")
            }
            None => format!(
                "The repository is in the detached HEAD state. Do you want to drop {count} {what}?"
            ),
        };
        let done = format!("Dropped {count} {what}");
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, ids, done) = (view.clone(), ids.clone(), done.clone());
            dialog
                .title(title)
                .description(
                    div()
                        .id("drop-commits")
                        .role(Role::Group)
                        .aria_label(title)
                        .child(question.clone()),
                )
                .confirm()
                .ok_text("Drop")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let (ids, done) = (ids.clone(), done.clone());
                    view.update(cx, |this, cx| {
                        this.run_outcome(window, cx, done, move |root| {
                            spork_git::drop_commits(root, &ids)
                        })
                    });
                    true
                })
        });
    }

    /// The log's right-click menu of a commit: the reference app's `Vcs.Log.ContextMenu`, its items, order,
    /// separators, shortcuts and enabled states. Spork's own commit commands are in Find Action
    /// (`log_commands`).
    pub(crate) fn commit_menu(
        view: Entity<RepoView>,
        id: String,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        move |menu, window, cx| {
            let short: String = id.chars().take(8).collect();
            let facts = view.read(cx).commit_facts(&id);
            let web = view.read(cx).commit_web_url(&id);
            let item =
                |label: &str, f: fn(&mut RepoView, String, &mut Window, &mut Context<RepoView>)| {
                    let (view, id) = (view.clone(), id.clone());
                    PopupMenuItem::new(label.to_owned()).on_click(move |_, window, cx| {
                        view.update(cx, |this, cx| f(this, id.clone(), window, cx))
                    })
                };
            let go_to = |label: &str, target: Option<String>| {
                let view = view.clone();
                PopupMenuItem::new(label.to_owned())
                    .disabled(target.is_none())
                    .on_click(move |_, _, cx| {
                        if let Some(target) = &target {
                            view.update(cx, |this, cx| this.go_to_commit(target, cx))
                        }
                    })
            };
            // rewriting history works on the current branch's own commits, merges left out
            let editable = facts.in_branch && !facts.merge;
            let both = facts.other.clone().map(|other| vec![id.clone(), other]);
            let drop = {
                let (view, ids) = (
                    view.clone(),
                    both.clone().unwrap_or_else(|| vec![id.clone()]),
                );
                let label = if ids.len() == 1 {
                    "Drop Commit"
                } else {
                    "Drop Commits"
                };
                PopupMenuItem::new(label)
                    .disabled(!editable)
                    .on_click(move |_, window, cx| {
                        let ids = ids.clone();
                        view.update(cx, |this, cx| this.confirm_drop_commits(ids, window, cx))
                    })
            };
            menu.item(
                item("Copy Revision Number", |_, id, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(id))
                })
                .icon(spork_workbench::icon("general/copy"))
                .action(Box::new(CopyRevisionNumber)),
            )
            .item(
                item("Create Patch…", |this, id, window, cx| {
                    this.save_patch_dialog(id, window, cx)
                })
                .icon(spork_workbench::icon("vcs/patch")),
            )
            .item(
                item("Cherry-Pick", |this, id, window, cx| {
                    let short: String = id.chars().take(7).collect();
                    this.run_outcome(window, cx, format!("Cherry-picked {short}"), move |root| {
                        spork_git::cherry_pick(root, &id)
                    })
                })
                .icon(spork_workbench::icon("dvcs/cherryPick")),
            )
            .separator()
            // Checkout ▸ the branches that can be checked out here, then the revision; just the
            // revision when there are none
            .map(|menu| match facts.branches.is_empty() {
                true => menu.item(checkout_detached_item(
                    &view,
                    &id,
                    "Checkout Revision".into(),
                )),
                false => menu.submenu("Checkout", window, cx, {
                    let (view, branches, id, label) = (
                        view.clone(),
                        facts.branches.clone(),
                        id.clone(),
                        format!("Checkout Revision '{short}'"),
                    );
                    move |menu, _, _| {
                        branches
                            .iter()
                            .fold(menu, |menu, branch| {
                                let (view, branch) = (view.clone(), branch.clone());
                                menu.item(PopupMenuItem::new(branch.clone()).on_click(
                                    move |_, window, cx| {
                                        view.update(cx, |this, cx| {
                                            this.ref_action(
                                                crate::branches::Action::Checkout,
                                                branch.clone(),
                                                window,
                                                cx,
                                            )
                                        })
                                    },
                                ))
                            })
                            .item(checkout_detached_item(&view, &id, label.clone()))
                    }
                }),
            })
            .item(item("Show Repository at Revision", |this, id, _, cx| {
                this.show_repository_at(&id, cx)
            }))
            .when_some(both.clone(), |menu, ids| {
                let view = view.clone();
                menu.item(
                    PopupMenuItem::new("Compare Versions")
                        .icon(spork_workbench::icon("vcs/diff"))
                        .on_click(move |_, window, cx| {
                            let ids = ids.clone();
                            view.update(cx, |this, cx| this.compare_versions(ids, window, cx))
                        }),
                )
            })
            .item(item("Compare with Local", |this, id, window, cx| {
                this.compare_with_local(id, window, cx)
            }))
            .separator()
            .item(
                item("Reset Current Branch to Here…", |this, id, window, cx| {
                    this.reset_dialog(id, window, cx)
                })
                .icon(spork_workbench::icon("vcs/revert")),
            )
            .item(item("Revert Commit", |this, id, window, cx| {
                let short: String = id.chars().take(7).collect();
                this.run_outcome(window, cx, format!("Reverted {short}"), move |root| {
                    spork_git::revert(root, &id)
                })
            }))
            // the newest commit of the current branch: back to its parent, the changes kept
            .item(
                item("Undo Commit…", |this, id, window, cx| {
                    this.undo_commit(id, window, cx)
                })
                .disabled(!(facts.is_head && !facts.merge && facts.parent.is_some())),
            )
            .separator()
            .item(
                item("Edit Commit Message…", |this, id, window, cx| {
                    this.reword_dialog(id, window, cx)
                })
                .action(Box::new(EditCommitMessage))
                .disabled(!editable),
            )
            .item(
                item("Fixup…", |this, id, window, cx| {
                    this.commit_into(&id, "fixup!", window, cx)
                })
                .disabled(!editable || !facts.has_changes),
            )
            .item(
                item("Squash Into…", |this, id, window, cx| {
                    this.commit_into(&id, "squash!", window, cx)
                })
                .disabled(!editable || !facts.has_changes),
            )
            .item(drop)
            .when_some(both, |menu, ids| {
                let view = view.clone();
                menu.item(
                    PopupMenuItem::new("Squash Commits…")
                        .disabled(!editable)
                        .on_click(move |_, window, cx| {
                            let ids = ids.clone();
                            view.update(cx, |this, cx| this.squash_commits_dialog(ids, window, cx))
                        }),
                )
            })
            .item(
                item(
                    "Interactively Rebase from Here…",
                    |this, id, window, cx| this.start_interactive_rebase(id, window, cx),
                )
                .disabled(!editable),
            )
            .item(
                item("Push All up to Here…", |this, id, window, cx| {
                    this.push_up_to_dialog(id, window, cx)
                })
                .disabled(!facts.in_branch),
            )
            .separator()
            .item(
                item("New Branch…", |this, id, window, cx| {
                    this.new_branch_dialog(id, window, cx)
                })
                .action(Box::new(crate::shell::NewBranch)),
            )
            .item(item("New Tag…", |this, id, window, cx| {
                this.log_new_tag_dialog(id, window, cx)
            }))
            .separator()
            .item(
                go_to("Go to Child Commit", facts.child.clone()).action(Box::new(GoToChildCommit)),
            )
            .item(go_to("Go to Parent Commit", facts.parent).action(Box::new(GoToParentCommit)))
            // GitHub's and GitLab's open-in-browser groups both read "View in browser" here
            // (GithubBundle open.on.github.action, GitLabBundle group.GitLab.Open.In.Browser);
            // "Open on GitHub" is Find Action's text only
            .when_some(web, |menu, (url, service)| {
                // shortcut: GitLab's item goes without its logo, which is in color while a menu
                // item takes a mono icon only; give it one once menu items take images
                let item = PopupMenuItem::new("View in browser")
                    .when(service == "GitHub", |item| {
                        item.icon(spork_workbench::icon("vcs/vendors/github"))
                    })
                    .on_click(move |_, _, cx| cx.open_url(&url));
                menu.separator().item(item)
            })
            .map(|menu| {
                RepoView::custom_items(
                    menu,
                    &view,
                    crate::custom::Target::Commit,
                    Some(id.clone()),
                    cx,
                )
            })
        }
    }

    /// What the log's menu needs to know of commit `id`.
    fn commit_facts(&self, id: &str) -> CommitFacts {
        let Some(Ok(repo)) = &self.repo else {
            return CommitFacts::default();
        };
        let commits = &repo.history.commits;
        let commit = commits.iter().find(|c| c.id == id);
        let head = repo
            .refs
            .iter()
            .find(|r| r.is_head && r.kind == spork_git::RefKind::Local);
        // a branch another worktree has can't be checked out here
        let checked_out: Vec<&str> = repo
            .worktrees
            .iter()
            .filter_map(|w| w.branch.as_deref())
            .collect();
        let branches = repo
            .refs
            .iter()
            .filter(|r| r.kind == spork_git::RefKind::Local && r.commit == id && !r.is_head)
            .filter(|r| !checked_out.contains(&r.name.as_str()))
            .map(|r| r.name.clone())
            .collect();
        let row = |ix: Option<usize>| ix.and_then(|ix| commits.get(ix)).map(|c| c.id.as_str());
        let (selected, compared) = (row(self.selected), row(self.compare));
        let other = match (selected, compared) {
            (Some(a), Some(b)) if a == id => Some(b.to_owned()),
            (Some(a), Some(b)) if b == id => Some(a.to_owned()),
            _ => None,
        };
        CommitFacts {
            branches,
            parent: commit.and_then(|c| c.parents.first().cloned()),
            child: commits
                .iter()
                .find(|c| c.parents.iter().any(|p| p == id))
                .map(|c| c.id.clone()),
            is_head: head.is_some_and(|h| h.commit == id),
            merge: commit.is_some_and(|c| c.parents.len() > 1),
            in_branch: head.is_some_and(|h| reaches(commits, &h.commit, id)),
            has_changes: repo.status.iter().any(|e| !e.untracked()),
            other,
        }
    }

    /// Where ⌥⌘N starts a branch: the selected commit while the log has the focus (the reference app's
    /// Git.CreateNewBranch.FromCommit), else HEAD.
    pub(crate) fn log_new_branch_start(&self, window: &Window) -> String {
        match self.selected_commit_id() {
            Some(id) if self.view == View::History && self.focus.is_focused(window) => id,
            _ => "HEAD".into(),
        }
    }

    /// Selects commit `id` in the log and scrolls it into view.
    pub(crate) fn go_to_commit(&mut self, id: &str, cx: &mut Context<Self>) {
        let ix = match &self.repo {
            Some(Ok(repo)) => repo.history.commits.iter().position(|c| c.id == id),
            _ => None,
        };
        if let Some(ix) = ix {
            self.select(ix, cx);
            self.history_scroll
                .scroll_to_item(ix, ScrollStrategy::Center);
        }
    }

    /// Show Repository at Revision: the commit selected, its files as of then (the All files tab).
    fn show_repository_at(&mut self, id: &str, cx: &mut Context<Self>) {
        self.go_to_commit(id, cx);
        self.show_commit_files(cx);
    }

    /// The two commits `ids` older first, as the log lists them (newest first).
    fn log_older_first(&self, mut ids: Vec<String>) -> Vec<String> {
        if let Some(Ok(repo)) = &self.repo {
            let row = |id: &String| repo.history.commits.iter().position(|c| c.id == *id);
            ids.sort_by_key(|id| std::cmp::Reverse(row(id)));
        }
        ids
    }

    /// Compare Versions: everything that changed between the two selected commits, in the reference app's
    /// "Changes Between {older} and {newer}" window.
    fn compare_versions(&mut self, ids: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Ok([older, newer]) = <[String; 2]>::try_from(self.log_older_first(ids)) else {
            return;
        };
        let short = |id: &str| id.chars().take(8).collect::<String>();
        let title = format!("Changes Between {} and {}", short(&older), short(&newer));
        let ignore_ws = self.prefs.ignore_whitespace;
        self.run_op(
            window,
            cx,
            move |root| spork_git::commit_details(root, &newer, Some(&older), ignore_ws),
            move |this, details, window, cx| {
                let diff = cx.new(|cx| DiffState::new([], cx));
                this.set_patch(&diff, &details.patch, cx);
                let height = (window.viewport_size().height * 0.75).max(px(300.));
                window.open_dialog(cx, move |dialog, _, _| {
                    dialog.width(px(1100.)).title(title.clone()).child(
                        div()
                            .id("compare-versions")
                            .role(Role::Group)
                            .aria_label(title.clone())
                            .h(height)
                            .child(Diff::new(&diff).size_full()),
                    )
                });
            },
        )
    }

    /// Squash Commits…: the reference app's "Squashed Commit Message" with the commits' messages, oldest
    /// first, then the commits made one with it.
    fn squash_commits_dialog(
        &mut self,
        ids: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(repo)) = &self.repo else { return };
        let ids = self.log_older_first(ids);
        let message = ids
            .iter()
            .map(|id| spork_git::commit_message(&repo.root, id).unwrap_or_default())
            .map(|m| m.trim_end().to_owned())
            .collect::<Vec<_>>()
            .join("\n\n");
        self.reword
            .update(cx, |s, cx| s.set_value(message, window, cx));
        let (view, input) = (cx.entity(), self.reword.clone());
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, input, ids) = (view.clone(), input.clone(), ids.clone());
            dialog
                .title("Squashed Commit Message")
                .description(
                    v_flex()
                        .id("squashed-commit-message")
                        .role(Role::Group)
                        .aria_label("Squashed Commit Message")
                        .gap_2()
                        .child("Edit message for squashed commit")
                        .child(Textarea::new(&input).h(px(140.))),
                )
                .confirm()
                .on_ok(move |_, window, cx| {
                    let message = input.read(cx).value().to_string();
                    let ids = ids.clone();
                    view.update(cx, |this, cx| {
                        // GitBundle rebase.log.squash.success.notification.title
                        this.run_outcome(window, cx, "Commits Squashed".into(), move |root| {
                            spork_git::squash(root, &ids, &message)
                        })
                    });
                    true
                })
        });
        self.reword.update(cx, |s, cx| s.focus(window, cx));
    }

    /// Compare with Local: what differs between the commit and the files on disk now.
    fn compare_with_local(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let short: String = id.chars().take(8).collect();
        self.run_op(
            window,
            cx,
            move |root| spork_git::revision::diff_with_local(root, &id),
            move |this, patch, window, cx| {
                let diff = cx.new(|cx| DiffState::new([], cx));
                this.set_patch(&diff, &patch, cx);
                let height = (window.viewport_size().height * 0.75).max(px(300.));
                let title = format!("Difference between {short} and local version");
                window.open_dialog(cx, move |dialog, _, _| {
                    dialog.width(px(1100.)).title(title.clone()).child(
                        div()
                            .id("compare-with-local")
                            .role(Role::Group)
                            .aria_label(title.clone())
                            .h(height)
                            .child(Diff::new(&diff).size_full()),
                    )
                });
            },
        )
    }

    /// "Git Reset": moves the current branch to `id`, its changes kept as the mode picked says.
    fn reset_dialog(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let mode = cx.new(|_| ResetMode::Mixed);
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let picked = *mode.read(cx);
            let choice = |label: &'static str, about: &'static str, this: ResetMode, cx: &App| {
                let mode = mode.clone();
                v_flex()
                    .gap_0p5()
                    .child(
                        spork_workbench::controls::labeled_radio(
                            label,
                            label,
                            picked == this,
                            cx,
                        )
                        .role(Role::RadioButton)
                        .aria_label(label)
                        .aria_toggled(match picked == this {
                            true => Toggled::True,
                            false => Toggled::False,
                        })
                        .on_click(move |_, _, cx| {
                            mode.update(cx, |mode, cx| {
                                *mode = this;
                                cx.notify();
                            })
                        }),
                    )
                    .child(
                        div()
                            .pl(px(20.))
                            .text_color(cx.theme().muted_foreground)
                            .child(about),
                    )
            };
            let (view, mode, id) = (view.clone(), mode.clone(), id.clone());
            dialog
                .title("Git Reset")
                .description(
                    v_flex()
                        .id("git-reset")
                        .role(Role::Group)
                        .aria_label("Git Reset")
                        .gap_3()
                        .child(
                            "This will reset the current branch head to the selected commit,\nand update the working tree and the index according to the selected mode:",
                        )
                        .child(choice(
                            "Soft",
                            "Files won't change, differences will be staged for commit.",
                            ResetMode::Soft,
                            cx,
                        ))
                        .child(choice(
                            "Mixed",
                            "Files won't change, differences won't be staged.",
                            ResetMode::Mixed,
                            cx,
                        ))
                        .child(choice(
                            "Hard",
                            "Files will be reverted to the state of the selected commit.\nWarning: any local changes will be lost.",
                            ResetMode::Hard,
                            cx,
                        ))
                        .child(choice(
                            "Keep",
                            "Files will be reverted to the state of the selected commit,\nbut local changes will be kept intact.",
                            ResetMode::Keep,
                            cx,
                        )),
                )
                .confirm()
                .ok_text("Reset")
                .on_ok(move |_, window, cx| {
                    let (mode, id) = (*mode.read(cx), id.clone());
                    view.update(cx, |this, cx| {
                        this.run_op(
                            window,
                            cx,
                            move |root| spork_git::revision::reset(root, &id, mode),
                            |_, (), window, cx| {
                                window.push_notification(
                                    Notification::success("Reset successful"),
                                    cx,
                                )
                            },
                        )
                    });
                    true
                })
        });
    }

    /// Undo Commit…: the branch's newest commit taken back, its changes left staged.
    fn undo_commit(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let parent = match &self.repo {
            Some(Ok(repo)) => repo
                .history
                .commits
                .iter()
                .find(|c| c.id == id)
                .and_then(|c| c.parents.first().cloned()),
            _ => None,
        };
        if let Some(parent) = parent {
            self.reset_to(parent, ResetMode::Soft, window, cx)
        }
    }

    /// Fixup… and Squash Into…: the Commit tool window, its message `fixup! <subject>` (or
    /// `squash!`), for git's autosquash to fold the next commit into `id`.
    fn commit_into(&mut self, id: &str, prefix: &str, window: &mut Window, cx: &mut Context<Self>) {
        let subject = match &self.repo {
            Some(Ok(repo)) => repo
                .history
                .commits
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.subject.clone()),
            _ => None,
        };
        let Some(subject) = subject else { return };
        self.open_left(crate::LeftTool::Commit, cx);
        self.set_message(format!("{prefix} {subject}"), window, cx);
    }

    /// Push All up to Here…: the current branch pushed to its upstream as far as `id`.
    fn push_up_to_dialog(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let Some(head) = repo
            .refs
            .iter()
            .find(|r| r.is_head && r.kind == spork_git::RefKind::Local)
        else {
            return;
        };
        let branch = head.name.clone();
        // the upstream's remote and branch, else the default remote and the same name
        let (remote, dest) = head
            .upstream
            .as_deref()
            .and_then(|upstream| upstream.split_once('/'))
            .map(|(remote, dest)| (remote.to_owned(), dest.to_owned()))
            .or_else(|| self.default_remote().map(|remote| (remote, branch.clone())))
            .unwrap_or_else(|| ("origin".into(), branch.clone()));
        let short: String = id.chars().take(8).collect();
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, id, remote, dest) = (view.clone(), id.clone(), remote.clone(), dest.clone());
            dialog
                .title(format!("Push Commits to {remote}"))
                .description(
                    div()
                        .id("push-up-to")
                        .role(Role::Group)
                        .aria_label(format!("Push Commits to {remote}"))
                        .child(format!("{branch} → {remote} : {dest}, up to {short}")),
                )
                .confirm()
                .ok_text("Push")
                .on_ok(move |_, window, cx| {
                    let (id, remote, dest) = (id.clone(), remote.clone(), dest.clone());
                    view.update(cx, |this, cx| {
                        this.remote_running = Some(crate::RemoteOp::Push);
                        this.run_op(
                            window,
                            cx,
                            move |root| {
                                spork_git::remote::push_commit(root, &remote, &id, &dest)
                                    .map(|()| format!("Pushed to {remote}/{dest}"))
                            },
                            |_, msg, window, cx| {
                                window.push_notification(Notification::success(msg), cx)
                            },
                        )
                    });
                    true
                })
        });
    }

    /// New Tag… on a commit of the log: "Create New Tag On <commit>", a name and nothing else.
    pub(crate) fn log_new_tag_dialog(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| InputState::new(window, cx));
        let view = cx.entity();
        let short: String = id.chars().take(8).collect();
        let title = format!("Create New Tag On {short}");
        window.open_alert_dialog(cx, {
            let name = name.clone();
            move |dialog, _, _| {
                let (view, name, id) = (view.clone(), name.clone(), id.clone());
                dialog
                    .title(title.clone())
                    .description(
                        v_flex()
                            .id("new-tag-dialog")
                            .role(Role::Group)
                            .aria_label(title.clone())
                            .gap_2()
                            .child("Enter the name of new tag")
                            .child(spork_workbench::TextField::new(&name)),
                    )
                    .confirm()
                    .on_ok(move |_, window, cx| {
                        let (tag, id) = (name.read(cx).value().trim().to_owned(), id.clone());
                        view.update(cx, |this, cx| {
                            this.run_op(
                                window,
                                cx,
                                move |root| spork_git::create_tag(root, &tag, &id, ""),
                                |_, (), _, _| {},
                            )
                        });
                        true
                    })
            }
        });
        name.update(cx, |s, cx| s.focus(window, cx));
    }

    /// Runs `f` on the log's selected commit; elsewhere the key goes on to whoever wants it.
    fn on_selected_commit(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Self, String, &mut Window, &mut Context<Self>),
    ) {
        match self
            .selected_commit_id()
            .filter(|_| self.view == View::History)
        {
            Some(id) => f(self, id, window, cx),
            None => cx.propagate(),
        }
    }

    /// The log menu's shortcuts and Spork's own commit commands (Find Action), on the selected
    /// commit.
    pub(crate) fn log_actions(&self, el: Div, cx: &mut Context<Self>) -> Div {
        el.on_action(cx.listener(|this, _: &CopyRevisionNumber, window, cx| {
            this.on_selected_commit(window, cx, |_, id, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(id))
            })
        }))
        .on_action(cx.listener(|this, _: &EditCommitMessage, window, cx| {
            this.on_selected_commit(window, cx, |this, id, window, cx| {
                let facts = this.commit_facts(&id);
                if facts.in_branch && !facts.merge {
                    this.reword_dialog(id, window, cx)
                }
            })
        }))
        .on_action(cx.listener(|this, _: &GoToChildCommit, window, cx| {
            this.on_selected_commit(window, cx, |this, id, _, cx| {
                if let Some(child) = this.commit_facts(&id).child {
                    this.go_to_commit(&child, cx)
                }
            })
        }))
        .on_action(cx.listener(|this, _: &GoToParentCommit, window, cx| {
            this.on_selected_commit(window, cx, |this, id, _, cx| {
                if let Some(parent) = this.commit_facts(&id).parent {
                    this.go_to_commit(&parent, cx)
                }
            })
        }))
        .on_action(
            cx.listener(|this, _: &CherryPickWithReference, window, cx| {
                this.on_selected_commit(window, cx, |this, id, window, cx| {
                    this.run_outcome(window, cx, "Cherry-picked".into(), move |root| {
                        spork_git::revision::cherry_pick_with(root, &id, true, false)
                    })
                })
            }),
        )
        .on_action(cx.listener(|this, _: &ApplyCommitChanges, window, cx| {
            this.on_selected_commit(window, cx, |this, id, window, cx| {
                this.run_outcome(
                    window,
                    cx,
                    "Changes applied and staged".into(),
                    move |root| spork_git::revision::cherry_pick_with(root, &id, false, true),
                )
            })
        }))
        .on_action(cx.listener(|this, _: &RebaseOntoCommit, window, cx| {
            this.on_selected_commit(window, cx, |this, id, window, cx| {
                this.rebase_dialog(id, window, cx)
            })
        }))
        .on_action(cx.listener(|this, _: &MoveBranchHere, window, cx| {
            this.on_selected_commit(window, cx, |this, id, window, cx| {
                this.move_branch_here_dialog(id, window, cx)
            })
        }))
        .on_action(cx.listener(|this, _: &BisectFromHere, window, cx| {
            this.on_selected_commit(window, cx, |this, id, window, cx| {
                this.start_bisect(id, window, cx)
            })
        }))
        .on_action(cx.listener(|this, _: &ExplainCommit, window, cx| {
            this.on_selected_commit(window, cx, |this, id, window, cx| {
                let subject = this.commit_subject(&id).unwrap_or_default();
                this.ai_start(crate::ai::AiTask::Explain(id, subject), window, cx)
            })
        }))
        .on_action(cx.listener(|this, _: &ReviewCommit, window, cx| {
            this.on_selected_commit(window, cx, |this, id, window, cx| {
                let subject = this.commit_subject(&id).unwrap_or_default();
                this.ai_start(crate::ai::AiTask::Review(id, subject), window, cx)
            })
        }))
        // The reference app has it in Find Action only; the pane's own button folds it too
        .on_action(cx.listener(|this, _: &HideGitBranches, _, cx| {
            this.branches_open = false;
            cx.notify();
        }))
    }

    fn commit_subject(&self, id: &str) -> Option<String> {
        let Some(Ok(repo)) = &self.repo else {
            return None;
        };
        let commit = repo.history.commits.iter().find(|c| c.id == id)?;
        Some(commit.subject.clone())
    }

    /// Move Branch Here (Find Action): asks which branch, then points it at `id`.
    fn move_branch_here_dialog(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let movable: Vec<String> = repo
            .refs
            .iter()
            .filter(|r| r.kind == spork_git::RefKind::Local && !r.is_head && r.commit != id)
            .map(|r| r.name.clone())
            .collect();
        let short: String = id.chars().take(7).collect();
        let hint = format!("The branch to move to {short}: {}", movable.join(", "));
        let first = movable.first().cloned().unwrap_or_default();
        self.input_dialog(
            "Move Branch".into(),
            hint,
            first,
            move |root, branch| {
                spork_git::revision::move_branch(root, &branch, &id)
                    .map(|()| format!("Moved {branch}"))
            },
            window,
            cx,
        )
    }

    /// Sidebar entries for the saved stashes, with Apply / Pop / Drop menus (M12 §1).
    pub(crate) fn stash_items(&self, stashes: &[Stash], cx: &mut Context<Self>) -> Vec<TreeItem> {
        let view = cx.entity();
        stashes
            .iter()
            .map(|s| {
                let (index, view, message) = (s.index, view.clone(), s.message.clone());
                let label = s
                    .message
                    .split_once(": ")
                    .map_or(s.message.as_str(), |(_, m)| m)
                    .to_owned();
                let (shown, click_view, stash) = (
                    self.view == View::Stash
                        && self.stash_shown.as_ref().is_some_and(|(t, _)| t.id == s.id),
                    view.clone(),
                    s.clone(),
                );
                TreeItem::new(label)
                    .icon(IconName::Archive)
                    .active(shown)
                    .on_click(move |_, _, cx| {
                        click_view.update(cx, |this, cx| this.show_stash(stash.clone(), cx))
                    })
                    .context_menu(stash_menu(view, index, message))
            })
            .collect()
    }

    pub(crate) fn confirm_drop_stash(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let view = view.clone();
            dialog
                .title("Delete this stash?")
                .description("Its changes are discarded and can't be recovered from Spork.")
                .confirm()
                .ok_text("Delete")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        this.run_op(
                            window,
                            cx,
                            move |root| spork_git::stash_drop(root, index),
                            |_, _, _, _| {},
                        )
                    });
                    true
                })
        });
    }
}

/// Right-click menu of a stash, in the sidebar and in the commit list (M12 §1).
pub(crate) fn stash_menu(
    view: Entity<RepoView>,
    index: usize,
    message: String,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, _, _| {
        let item = |label: &str, pop: Option<bool>| {
            let view = view.clone();
            PopupMenuItem::new(label.to_owned()).on_click(move |_, window, cx| {
                view.update(cx, |this, cx| match pop {
                    Some(pop) => this.run_outcome(
                        window,
                        cx,
                        if pop {
                            "Stash applied and removed"
                        } else {
                            "Stash applied"
                        }
                        .into(),
                        move |root| spork_git::stash_apply(root, index, pop),
                    ),
                    None => this.confirm_drop_stash(index, window, cx),
                })
            })
        };
        let (rename_view, branch_view, message) = (view.clone(), view.clone(), message.clone());
        menu.item(item("Apply", Some(false)))
                                .item(item("Apply and remove (pop)", Some(true)))
                                .item(PopupMenuItem::new("New branch from stash…").on_click(
                                    move |_, window, cx| {
                                        branch_view.update(cx, |this, cx| {
                                            this.input_dialog(
                                                "Create branch from stash".into(),
                                                "Checks out a new branch at the commit the stash was made on and applies the stash there.".into(),
                                                String::new(),
                                                move |root, name| {
                                                    spork_git::stash::branch_from(root, index, &name)
                                                        .map(|()| format!("Created {name} from the stash"))
                                                },
                                                window,
                                                cx,
                                            )
                                        })
                                    },
                                ))
                                .item(PopupMenuItem::new("Rename…").on_click(move |_, window, cx| {
                                    let message = message.clone();
                                    rename_view.update(cx, |this, cx| {
                                        this.input_dialog(
                                            "Rename stash".into(),
                                            "The renamed stash moves to the top of the list.".into(),
                                            message,
                                            move |root, text| {
                                                spork_git::stash::rename(root, index, &text)
                                                    .map(|()| "Stash renamed".to_owned())
                                            },
                                            window,
                                            cx,
                                        )
                                    })
                                }))
                                .item(PopupMenuItem::separator())
                                .item(item("Delete…", None))
    }
}

/// "Checkout Revision 'abc12345'": the commit checked out on its own (detached HEAD).
fn checkout_detached_item(view: &Entity<RepoView>, id: &str, label: String) -> PopupMenuItem {
    let (view, id) = (view.clone(), id.to_owned());
    PopupMenuItem::new(label).on_click(move |_, window, cx| {
        let id = id.clone();
        view.update(cx, |this, cx| {
            this.run_op(
                window,
                cx,
                move |root| spork_git::branch::checkout_detached(root, &id),
                |_, _, window, cx| {
                    window.push_notification(
                        Notification::info("Detached HEAD: create a branch to keep new commits"),
                        cx,
                    )
                },
            )
        })
    })
}

actions!(
    spork,
    [
        CopyRevisionNumber,
        EditCommitMessage,
        GoToChildCommit,
        GoToParentCommit,
        CherryPickWithReference,
        ApplyCommitChanges,
        RebaseOntoCommit,
        MoveBranchHere,
        BisectFromHere,
        ExplainCommit,
        ReviewCommit,
        HideGitBranches,
    ]
);

/// The log menu's shortcuts, the reference app's macOS keymap; `secondary` is Cmd on macOS.
pub(crate) fn install(cx: &mut App) {
    let in_view = || {
        KeyBindingContextPredicate::parse("Spork")
            .ok()
            .map(std::rc::Rc::new)
    };
    let keys: Vec<KeyBinding> = [
        (
            "alt-shift-secondary-c",
            Box::new(CopyRevisionNumber) as Box<dyn Action>,
        ),
        // the binding bound last is the one a menu shows: F2, as the reference app does
        ("shift-f6", Box::new(EditCommitMessage)),
        ("f2", Box::new(EditCommitMessage)),
        ("left", Box::new(GoToChildCommit)),
        ("right", Box::new(GoToParentCommit)),
    ]
    .into_iter()
    .filter_map(|(keys, action)| {
        KeyBinding::load(keys, action, in_view(), false, None, cx).log_err()
    })
    .collect();
    cx.bind_keys(keys);
}

/// The log's commands only Find Action lists: Spork's own on the selected commit, which
/// the reference app's menu doesn't have, and the reference app's Hide Git Branches.
pub(crate) fn log_commands() -> Vec<(SharedString, Box<dyn Action>)> {
    vec![
        (
            "Fold Merged Branch".into(),
            Box::new(crate::shell::FoldMerge),
        ),
        (
            "Unfold Merged Branch".into(),
            Box::new(crate::shell::UnfoldMerge),
        ),
        (
            "Cherry-Pick with Reference (-x)".into(),
            Box::new(CherryPickWithReference),
        ),
        (
            "Apply Changes without Committing".into(),
            Box::new(ApplyCommitChanges),
        ),
        (
            "Rebase Current Branch onto Selected Commit…".into(),
            Box::new(RebaseOntoCommit),
        ),
        (
            "Move Branch to Selected Commit…".into(),
            Box::new(MoveBranchHere),
        ),
        (
            "Rename Branch of Selected Commit…".into(),
            Box::new(crate::shell::RenameSelectedBranch),
        ),
        ("Bisect from Here (Good)".into(), Box::new(BisectFromHere)),
        ("Explain Commit with AI…".into(), Box::new(ExplainCommit)),
        ("Review Commit with AI…".into(), Box::new(ReviewCommit)),
        ("Hide Git Branches".into(), Box::new(HideGitBranches)),
    ]
}

/// What the log's menu needs to know of the commit it opened on.
#[derive(Default)]
struct CommitFacts {
    /// Local branches at the commit that can be checked out: not the current one, nor one
    /// another worktree has.
    branches: Vec<String>,
    parent: Option<String>,
    child: Option<String>,
    is_head: bool,
    merge: bool,
    /// In the current branch's history, where commits can be edited.
    in_branch: bool,
    /// Tracked files have changes to commit.
    has_changes: bool,
    /// The other selected commit, when two are and this is one of them.
    other: Option<String>,
}

/// Whether `target` is `from` or one of its ancestors among the loaded commits.
fn reaches(commits: &[spork_git::Commit], from: &str, target: &str) -> bool {
    let parents: std::collections::HashMap<&str, &[String]> = commits
        .iter()
        .map(|c| (c.id.as_str(), c.parents.as_slice()))
        .collect();
    let mut seen = std::collections::HashSet::new();
    let mut next = vec![from];
    while let Some(id) = next.pop() {
        if id == target {
            return true;
        }
        if seen.insert(id)
            && let Some(up) = parents.get(id)
        {
            next.extend(up.iter().map(String::as_str));
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::reaches;

    fn commit(id: &str, parents: &[&str]) -> spork_git::Commit {
        spork_git::Commit {
            id: id.into(),
            parents: parents.iter().map(|p| (*p).into()).collect(),
            author: String::new(),
            email: String::new(),
            time: 0,
            commit_time: 0,
            subject: String::new(),
        }
    }

    #[test]
    fn reaches_ancestors_through_merges_only() {
        // m merges b into a; c is on another line
        let commits = [
            commit("c", &["a"]),
            commit("m", &["a", "b"]),
            commit("b", &["root"]),
            commit("a", &["root"]),
            commit("root", &[]),
        ];
        assert!(reaches(&commits, "m", "m"));
        assert!(reaches(&commits, "m", "b"));
        assert!(reaches(&commits, "m", "root"));
        assert!(!reaches(&commits, "m", "c"));
        assert!(!reaches(&commits, "b", "a"));
    }
}
