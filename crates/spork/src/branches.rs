//! Branch operations from the branch menus of the branches popup and the log (docs/specs/M04).

use crate::notices::Notification;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::command::{CommandItem, CommandState};
use gpui_kit::component::input::{InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, WindowExt as _, h_flex,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_git::{MergeKind, OpState, Ref, RefKind};

use crate::RepoView;
use crate::history::Label;

#[derive(Clone, Copy)]
pub(crate) enum Action {
    Checkout,
    CheckoutRemote,
    NewBranchFrom,
    Delete,
    Merge,
    Rebase,
    DeleteTag,
    Worktree,
    Rename,
    SetUpstream,
    CopyName,
    DeleteRemote,
    CheckoutDetached,
    PushAllTags,
    DeleteTagRemote,
    PullRequest,
    TagDetails,
    PushTo,
    ReleaseNotes,
    Star,
    FastForward,
    Hide,
    /// Add to, or take out of, the branches the graph shows alone.
    Only,
    OpenWorktree,
    /// Check the branch out and rebase it onto the current one, in one step.
    CheckoutRebase,
    /// Check the branch out after fetching its tracked branch and fast-forwarding to it.
    CheckoutUpdate,
    CompareWith,
    DiffWithLocal,
    PullRebase,
    PullMerge,
    /// Spork's merge dialog, with its fast-forward, merge-commit and squash choices.
    MergeDialog,
    /// The reference app's Push Tag: the tag to the default remote.
    PushTag,
}

/// Favorites from the start, as in IntelliJ (GitBranchManager): the usual default branches.
const DEFAULT_FAVORITES: [&str; 4] = ["main", "master", "origin/main", "origin/master"];

/// The starred branches: `spork.favorite`, and the default ones unless in `spork.notFavorite`.
pub(crate) fn favorites(root: &std::path::Path) -> Vec<String> {
    let excluded = spork_git::config::get_all(root, "spork.notFavorite");
    let mut favorites = spork_git::config::get_all(root, "spork.favorite");
    favorites.extend(
        DEFAULT_FAVORITES
            .iter()
            .filter(|name| !excluded.iter().any(|e| e == *name))
            .map(|name| (*name).to_owned()),
    );
    favorites
}

/// Right-click menu for a branch, remote branch or tag, and its submenu in the branches popup:
/// the reference app's `Git.Branch` group, in its order and words (docs/reference-spec.md §9). Spork's own
/// branch commands are in Find Action ([`commands`]); git-flow and custom commands still show
/// here when the repository has them.
pub fn ref_menu(
    r: &Ref,
    view: Entity<RepoView>,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    let r = r.clone();
    move |menu, window, cx| build_ref_menu(&r, &view, menu, window, cx)
}

fn build_ref_menu(
    r: &Ref,
    view: &Entity<RepoView>,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(Ok(repo)) = &view.read(cx).repo else {
        return menu;
    };
    let current = repo
        .refs
        .iter()
        .find(|r| r.is_head && r.kind == RefKind::Local)
        .map(|r| r.name.clone());
    let in_worktree = repo
        .worktrees
        .iter()
        .any(|w| !w.is_main && w.branch.as_deref() == Some(&r.name));
    let remotes: Vec<String> = repo.remotes.iter().map(|r| r.name.clone()).collect();
    let tracked = r.upstream.clone().filter(|_| !r.upstream_gone);
    let tracked_ref = tracked
        .as_ref()
        .and_then(|up| {
            repo.refs
                .iter()
                .find(|r| r.kind == RefKind::Remote && &r.name == up)
        })
        .cloned();
    let (x, cur) = (quote(&r.name), quote(current.as_deref().unwrap_or("HEAD")));
    let item = |label: &str, action: Action| {
        let (view, name) = (view.clone(), r.name.clone());
        PopupMenuItem::new(label.to_owned()).on_click(move |_, window, cx| {
            view.update(cx, |this, cx| {
                this.ref_action(action, name.clone(), window, cx)
            })
        })
    };
    let (local, remote, tag) = (
        r.kind == RefKind::Local,
        r.kind == RefKind::Remote,
        r.kind == RefKind::Tag,
    );
    let other = !r.is_head;
    let (checkout, delete) = match r.kind {
        RefKind::Local => (Action::Checkout, Action::Delete),
        RefKind::Remote => (Action::CheckoutRemote, Action::DeleteRemote),
        RefKind::Tag => (Action::CheckoutDetached, Action::DeleteTag),
    };
    let menu = menu
        .when(other, |menu| menu.item(item("Checkout", checkout)))
        .when(local && in_worktree, |menu| {
            menu.item(item("Open Worktree…", Action::OpenWorktree))
        })
        .when(!tag, |menu| {
            menu.item(item(
                &format!("New Branch from {x}…"),
                Action::NewBranchFrom,
            ))
        })
        .when(other && !tag, |menu| {
            menu.item(item(
                &format!("Checkout and Rebase onto {cur}"),
                Action::CheckoutRebase,
            ))
        })
        .when(local && other && tracked.is_some(), |menu| {
            menu.item(item("Checkout and Update", Action::CheckoutUpdate))
        })
        .separator()
        .when(other && !tag, |menu| {
            menu.item(item(&format!("Compare with {cur}"), Action::CompareWith))
        })
        .item(item("Show Diff with Working Tree", Action::DiffWithLocal))
        .separator()
        .when(other && !tag, |menu| {
            menu.item(item(&format!("Rebase {cur} onto {x}"), Action::Rebase))
        })
        .when(other, |menu| {
            menu.item(item(&format!("Merge {x} into {cur}"), Action::Merge))
        })
        .separator()
        .item(item(
            &match other {
                true => format!("New Worktree from {x}…"),
                false => "New Worktree…".to_owned(),
            },
            Action::Worktree,
        ))
        .separator()
        .when(local, |menu| {
            menu.item(item("Update", Action::FastForward).disabled(tracked.is_none()))
                .item(item("Push…", Action::PushTo).action(Box::new(crate::shell::Push)))
        })
        .when_some(tracked_ref.filter(|_| local), |menu, up| {
            let view = view.clone();
            menu.submenu(
                format!("Tracked Branch {}", quote(&up.name)),
                window,
                cx,
                move |menu, window, cx| build_ref_menu(&up, &view, menu, window, cx),
            )
        })
        .when(remote, |menu| {
            menu.item(item(
                &format!("Pull into {cur} Using Rebase"),
                Action::PullRebase,
            ))
            .item(item(
                &format!("Pull into {cur} Using Merge"),
                Action::PullMerge,
            ))
        })
        .separator()
        .when(tag, |menu| {
            remotes.iter().fold(menu, |menu, remote| {
                let (view, name, remote) = (view.clone(), r.name.clone(), remote.clone());
                menu.item(PopupMenuItem::new(format!("Push to {remote}")).on_click(
                    move |_, window, cx| {
                        let (name, remote) = (name.clone(), remote.clone());
                        view.update(cx, |this, cx| this.push_tag(name, remote, window, cx))
                    },
                ))
            })
        })
        .separator()
        .when(local, |menu| {
            menu.item(
                item("Rename…", Action::Rename)
                    .action(Box::new(crate::shell::RenameSelectedBranch)),
            )
        })
        .when(other, |menu| {
            menu.item(item("Delete", delete).action(Box::new(crate::shell::DeleteSelectedBranch)))
        });
    if !local {
        return menu;
    }
    let menu = RepoView::finish_item(menu, view, &r.name, cx);
    RepoView::custom_items(
        menu,
        view,
        crate::custom::Target::Branch,
        Some(r.name.clone()),
        cx,
    )
}

/// A name in a menu label: quoted and cut at 40 characters, as IntelliJ's.
fn quote(name: &str) -> String {
    let cut: String = name.chars().take(40).collect();
    format!("'{cut}'")
}

impl RepoView {
    /// The local branch on the selected commit that Delete / F2 act on: the current branch
    /// first when it is there, else the first one.
    fn selected_branch(&self, current_ok: bool) -> Option<Ref> {
        let (Some(Ok(repo)), Some(id)) = (&self.repo, self.selected_commit_id()) else {
            return None;
        };
        let mut here: Vec<&Ref> = repo
            .refs
            .iter()
            .filter(|r| r.kind == RefKind::Local && r.commit == id)
            .filter(|r| current_ok || !r.is_head)
            .collect();
        here.sort_by_key(|r| !r.is_head);
        here.first().map(|r| (*r).clone())
    }

    /// Delete key in the commit list: delete the selected commit's branch (asks first).
    pub(crate) fn delete_selected_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.selected_branch(false) {
            Some(r) if self.view == crate::View::History => self.delete_dialog(r.name, window, cx),
            _ => cx.propagate(),
        }
    }

    /// F2 in the commit list: rename the selected commit's branch.
    pub(crate) fn rename_selected_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.selected_branch(true) {
            Some(r) if self.view == crate::View::History => self.rename_dialog(r.name, window, cx),
            _ => cx.propagate(),
        }
    }

    pub(crate) fn ref_action(
        &mut self,
        action: Action,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        match action {
            Action::Checkout => self.run_op(
                window,
                cx,
                // refused because local changes would be overwritten: asked about next, not an error
                move |root| match spork_git::checkout(root, &name) {
                    Ok(()) => Ok(Ok(format!("Switched to {name}"))),
                    Err(spork_git::GitError::Other { stderr, .. })
                        if stderr.contains("would be overwritten") =>
                    {
                        Ok(Err(name))
                    }
                    Err(e) => Err(e),
                },
                |this, result, window, cx| match result {
                    Ok(message) => notify_success(this, message, window, cx),
                    Err(name) => this.offer_stash_checkout(name, window, cx),
                },
            ),
            Action::CheckoutRemote => self.run_op(
                window,
                cx,
                move |root| {
                    spork_git::checkout_remote(root, &name)
                        .map(|local| format!("Switched to {local}"))
                },
                notify_success,
            ),
            Action::NewBranchFrom => self.new_branch_dialog(name, window, cx),
            Action::Rename => self.rename_dialog(name, window, cx),
            Action::PullRequest => self.pull_request_dialog(name, window, cx),
            Action::PushTo => self.push_dialog(name, None, window, cx),
            // The reference app's Update: fetch the tracked branch, then fast-forward to it
            Action::FastForward => {
                let remote = self.tracked_remote(&name);
                self.run_op(
                    window,
                    cx,
                    move |root| {
                        if let Some(remote) = &remote {
                            spork_git::remote::fetch(root, remote)?;
                        }
                        spork_git::branch::fast_forward(root, &name)
                            .map(|()| format!("Updated {name}"))
                    },
                    notify_success,
                )
            }
            Action::CheckoutUpdate => {
                let remote = self.tracked_remote(&name);
                self.run_op(
                    window,
                    cx,
                    move |root| {
                        if let Some(remote) = &remote {
                            spork_git::remote::fetch(root, remote)?;
                        }
                        spork_git::branch::fast_forward(root, &name)?;
                        spork_git::checkout(root, &name).map(|()| format!("Checked out {name}"))
                    },
                    notify_success,
                )
            }
            Action::CheckoutRebase => {
                let Some(onto) = self.current_branch() else {
                    return window.push_notification(
                        Notification::error("Rebase is not possible in the detached HEAD state"),
                        cx,
                    );
                };
                let remote = self.split_remote_branch(&name).is_some()
                    && matches!(&self.repo, Some(Ok(repo)) if repo.refs.iter().any(|r| r.kind == RefKind::Remote && r.name == name));
                let done = format!("Checked out {name} and rebased it onto {onto}");
                self.run_outcome(window, cx, done, move |root| {
                    let branch = match remote {
                        true => spork_git::checkout_remote(root, &name)?,
                        false => name,
                    };
                    stopped_on_conflicts(
                        root,
                        spork_git::run(root, &["rebase", "--autostash", &onto, &branch]),
                    )
                })
            }
            Action::OpenWorktree => {
                let path = match &self.repo {
                    Some(Ok(repo)) => repo
                        .worktrees
                        .iter()
                        .find(|w| w.branch.as_deref() == Some(&name))
                        .map(|w| w.path.clone()),
                    _ => None,
                };
                if let Some(path) = path {
                    self.open_worktree(path, cx);
                }
            }
            Action::PullRebase | Action::PullMerge => {
                let Some((remote, branch)) = self.split_remote_branch(&name) else {
                    return;
                };
                let into = self.current_branch().unwrap_or_else(|| "HEAD".into());
                let opts = spork_git::PullOptions {
                    rebase: matches!(action, Action::PullRebase),
                    autostash: true,
                };
                let done = format!("Pulled {name} into {into}");
                self.run_outcome(window, cx, done, move |root| {
                    spork_git::pull_from(root, &remote, &branch, opts)
                })
            }
            Action::CompareWith => self.compare_with_current(name, window, cx),
            Action::DiffWithLocal => self.diff_with_working_tree(name, window, cx),
            Action::MergeDialog => self.merge_dialog(name, window, cx),
            Action::PushTag => match self.default_remote() {
                Some(remote) => self.push_tag(name, remote, window, cx),
                None => window
                    .push_notification(Notification::error("This repository has no remote."), cx),
            },
            Action::Star => {
                let starred = match &self.repo {
                    Some(Ok(repo)) => repo.favorites.contains(&name),
                    _ => false,
                };
                self.run_op(
                    window,
                    cx,
                    move |root| {
                        // the default branches are unstarred by listing them apart
                        let (key, add) = match DEFAULT_FAVORITES.contains(&name.as_str()) {
                            true => ("spork.notFavorite", starred),
                            false => ("spork.favorite", !starred),
                        };
                        match add {
                            true => spork_git::config::add_local(root, key, &name),
                            false => spork_git::config::remove_local(root, key, &name),
                        }
                    },
                    |_, (), _, _| {},
                )
            }
            Action::Hide => {
                let hidden = match &self.repo {
                    Some(Ok(repo)) => repo.hidden.contains(&name),
                    _ => false,
                };
                self.run_op(
                    window,
                    cx,
                    move |root| {
                        if hidden {
                            spork_git::config::remove_local(root, "spork.hidden", &name)
                        } else {
                            spork_git::config::add_local(root, "spork.hidden", &name)
                        }
                    },
                    |_, (), _, _| {},
                )
            }
            Action::Only => {
                let shown = match &self.repo {
                    Some(Ok(repo)) => repo.only.contains(&name),
                    _ => false,
                };
                self.run_op(
                    window,
                    cx,
                    move |root| {
                        if shown {
                            spork_git::config::remove_local(root, "spork.only", &name)
                        } else {
                            spork_git::config::add_local(root, "spork.only", &name)
                        }
                    },
                    |_, (), _, _| {},
                )
            }
            Action::ReleaseNotes => {
                self.ai_start(crate::ai::AiTask::ReleaseNotes(name), window, cx)
            }
            Action::TagDetails => self.run_op(
                window,
                cx,
                move |root| spork_git::revision::tag_info(root, &name).map(|t| (name, t)),
                |_, (name, t), window, cx| {
                    let muted = cx.theme().muted_foreground;
                    let short: String = t.target.chars().take(7).collect();
                    let who = if t.annotated {
                        format!("{} · {}", t.tagger, t.date)
                    } else {
                        "Lightweight tag: a name only, no tagger or message".to_owned()
                    };
                    window.open_alert_dialog(cx, move |dialog, _, _| {
                        dialog.title(format!("Tag {name}")).description(
                            v_flex()
                                .gap_2()
                                .text_sm()
                                .child(div().text_color(muted).child(who.clone()))
                                .child(format!("On commit {short}"))
                                .child(t.message.clone()),
                        )
                    });
                },
            ),
            Action::PushAllTags | Action::DeleteTagRemote => {
                let Some(remote) = self.default_remote() else {
                    window.push_notification(
                        Notification::error("This repository has no remote."),
                        cx,
                    );
                    return;
                };
                if matches!(action, Action::PushAllTags) {
                    return self.run_op(
                        window,
                        cx,
                        move |root| {
                            spork_git::remote::push_tags(root, &remote)
                                .map(|()| format!("Pushed all tags to {remote}"))
                        },
                        notify_success,
                    );
                }
                window.open_alert_dialog(cx, move |dialog, _, _| {
                    let (view, tag, remote) = (view.clone(), name.clone(), remote.clone());
                    dialog
                        .title(format!("Delete tag {tag} on {remote}?"))
                        .description(
                            "Removes the tag on the server for everyone. The local tag is kept.",
                        )
                        .confirm()
                        .ok_text("Delete on remote")
                        .ok_variant(ButtonVariant::Danger)
                        .on_ok(move |_, window, cx| {
                            let (tag, remote) = (tag.clone(), remote.clone());
                            view.update(cx, |this, cx| {
                                this.run_op(
                                    window,
                                    cx,
                                    move |root| {
                                        spork_git::remote::delete_tag(root, &remote, &tag)
                                            .map(|()| format!("Deleted {tag} on {remote}"))
                                    },
                                    notify_success,
                                )
                            });
                            true
                        })
                });
            }
            Action::SetUpstream => self.upstream_dialog(name, window, cx),
            Action::CopyName => {
                cx.write_to_clipboard(ClipboardItem::new_string(name));
                window.push_notification(Notification::info("Name copied"), cx);
            }
            Action::CheckoutDetached => self.run_op(
                window,
                cx,
                move |root| {
                    spork_git::branch::checkout_detached(root, &name)
                        .map(|()| format!("Checked out {name} (detached HEAD)"))
                },
                notify_success,
            ),
            Action::DeleteRemote => self.delete_remote_dialog(name, window, cx),
            Action::Worktree => self.new_worktree_dialog(name, window, cx),
            // The reference app deletes a tag at once; git's message keeps what it pointed at
            Action::DeleteTag => self.run_op(
                window,
                cx,
                move |root| {
                    let out = spork_git::run(root, &["tag", "--delete", &name])?;
                    Ok(String::from_utf8_lossy(&out).trim().to_owned())
                },
                notify_success,
            ),
            Action::Merge => {
                let into = self.current_branch().unwrap_or_else(|| "HEAD".into());
                let done = format!("Merged {name} to {into}");
                self.run_outcome(window, cx, done, move |root| spork_git::merge(root, &name))
            }
            Action::Rebase => {
                let Some(current) = self.current_branch() else {
                    return window.push_notification(
                        Notification::error("Rebase is not possible in the detached HEAD state"),
                        cx,
                    );
                };
                let done = format!("Rebased {current} onto {name}");
                self.run_outcome(window, cx, done, move |root| spork_git::rebase(root, &name))
            }
            // The reference app deletes at once; a branch with commits merged nowhere asks first
            Action::Delete => self.delete_branch(name, None, false, window, cx),
        }
    }

    /// The remote `branch` tracks a branch of.
    fn tracked_remote(&self, branch: &str) -> Option<String> {
        let upstream = self.upstream_of(branch)?;
        self.split_remote_branch(&upstream)
            .map(|(remote, _)| remote)
    }

    /// A tag's "Push to <remote>".
    pub(crate) fn push_tag(
        &mut self,
        tag: String,
        remote: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_op(
            window,
            cx,
            move |root| {
                spork_git::push_tag(root, &remote, &tag)
                    .map(|()| format!("Pushed tag {tag} to {remote}"))
            },
            notify_success,
        )
    }

    /// "Delete" on a remote branch, as the reference app asks it: the local branches tracking it can go too.
    fn delete_remote_dialog(
        &mut self,
        branch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let trackers: Vec<String> = match &self.repo {
            Some(Ok(repo)) => repo
                .refs
                .iter()
                .filter(|r| {
                    r.kind == RefKind::Local && !r.is_head && r.upstream.as_deref() == Some(&branch)
                })
                .map(|r| r.name.clone())
                .collect(),
            _ => Vec::new(),
        };
        self.delete_remote_too = false;
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let also = view.read(cx).delete_remote_too;
            let (check_view, ok_view, branch, trackers) =
                (view.clone(), view.clone(), branch.clone(), trackers.clone());
            let label = match trackers.as_slice() {
                [one] => format!("Delete tracking local branch {one} as well"),
                _ => "Delete tracking local branches:".to_owned(),
            };
            dialog
                .title("Delete Remote Branch")
                .description(
                    v_flex()
                        .gap_2()
                        .child(format!("Delete remote branch {branch}?"))
                        .when(!trackers.is_empty(), |d| {
                            d.child(
                                Checkbox::new("delete-tracking-too")
                                    .label(label)
                                    .checked(also)
                                    .on_click(move |checked, _, cx| {
                                        check_view.update(cx, |this, cx| {
                                            this.delete_remote_too = *checked;
                                            cx.notify();
                                        })
                                    }),
                            )
                        }),
                )
                .confirm()
                .ok_text("Delete")
                .on_ok(move |_, window, cx| {
                    let (branch, trackers) = (branch.clone(), trackers.clone());
                    ok_view.update(cx, |this, cx| {
                        let locals = match this.delete_remote_too {
                            true => trackers,
                            false => Vec::new(),
                        };
                        this.run_op(
                            window,
                            cx,
                            move |root| {
                                spork_git::branch::delete_remote(root, &branch)?;
                                for local in &locals {
                                    spork_git::branch::force_delete(root, local)?;
                                }
                                Ok(match locals.is_empty() {
                                    true => format!("Deleted remote branch {branch}"),
                                    false => format!(
                                        "Deleted remote branch {branch}. Also deleted local {}: {}",
                                        if locals.len() == 1 {
                                            "branch"
                                        } else {
                                            "branches"
                                        },
                                        locals.join(", ")
                                    ),
                                })
                            },
                            notify_success,
                        )
                    });
                    true
                })
        });
    }

    /// The reference app's New Branch dialog ("Create New Branch"): the name, Checkout branch and
    /// Overwrite existing branch; `start` is a branch, tag or commit ("HEAD" for New Branch…).
    pub(crate) fn new_branch_dialog(
        &mut self,
        start: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(repo)) = &self.repo else { return };
        let existing: Vec<String> = repo
            .refs
            .iter()
            .filter(|r| r.kind == RefKind::Local)
            .map(|r| r.name.clone())
            .collect();
        // IntelliJ starts from the branch's own name: the current one, or the remote's
        let suggested = match start.as_str() {
            "HEAD" => repo.head.clone().unwrap_or_default(),
            name => match repo.refs.iter().find(|r| r.name == name).map(|r| r.kind) {
                Some(RefKind::Local) => name.to_owned(),
                Some(RefKind::Remote) => self
                    .split_remote_branch(name)
                    .map_or_else(|| name.to_owned(), |(_, branch)| branch),
                _ => String::new(),
            },
        };
        let input = cx.new(|cx| InputState::new(window, cx));
        input.update(cx, |state, cx| {
            state.set_value(suggested, window, cx);
            state.focus(window, cx);
        });
        let form = std::rc::Rc::new(std::cell::Cell::new(NewBranchForm {
            checkout: true,
            overwrite: false,
            refused: false,
        }));
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let state = form.get();
            let name = input.read(cx).value().trim().to_owned();
            let error = match () {
                _ if name.is_empty() => Some("Specify name for the new branch".to_owned()),
                _ if name == "HEAD" => Some("HEAD is a reserved keyword".to_owned()),
                _ if existing.contains(&name) && !state.overwrite => {
                    Some("Change the name or overwrite existing branch".to_owned())
                }
                _ => None,
            };
            let toggle =
                |id: &'static str, label: &'static str, on: bool, flip: fn(&mut NewBranchForm)| {
                    let (form, view) = (form.clone(), view.clone());
                    Checkbox::new(id)
                        .label(label)
                        .checked(on)
                        .on_click(move |_, _, cx| {
                            let mut state = form.get();
                            flip(&mut state);
                            state.refused = false;
                            form.set(state);
                            view.update(cx, |_, cx| cx.notify());
                        })
                };
            let danger = cx.theme().danger;
            let (ok_view, ok_form, ok_input, start) =
                (view.clone(), form.clone(), input.clone(), start.clone());
            dialog
                .title("Create New Branch")
                .description(
                    v_flex()
                        .gap_2()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(div().flex_none().child("Branch Name:"))
                                .child(
                                    spork_workbench::TextField::new(&input)
                                        .aria_label("Branch Name:")
                                        .flex_1(),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap_4()
                                .child(toggle(
                                    "new-branch-checkout",
                                    "Checkout branch",
                                    state.checkout,
                                    |s| s.checkout ^= true,
                                ))
                                .child(toggle(
                                    "new-branch-overwrite",
                                    "Overwrite existing branch",
                                    state.overwrite,
                                    |s| s.overwrite ^= true,
                                )),
                        )
                        .children(
                            error
                                .clone()
                                .filter(|_| state.refused)
                                .map(|error| div().text_sm().text_color(danger).child(error)),
                        ),
                )
                .confirm()
                .ok_text("Create")
                .on_ok(move |_, window, cx| {
                    let state = ok_form.get();
                    let name = ok_input.read(cx).value().trim().to_owned();
                    if error.is_some() {
                        ok_form.set(NewBranchForm {
                            refused: true,
                            ..state
                        });
                        ok_view.update(cx, |_, cx| cx.notify());
                        return false;
                    }
                    let start = start.clone();
                    ok_view.update(cx, |this, cx| {
                        this.run_op(
                            window,
                            cx,
                            move |root| create_branch(root, &name, &start, state),
                            |_, (), _, _| {},
                        )
                    });
                    true
                })
        });
    }
}

/// What the New Branch dialog's check boxes say; `refused`: Create was pressed on an invalid name.
#[derive(Clone, Copy)]
struct NewBranchForm {
    checkout: bool,
    overwrite: bool,
    refused: bool,
}

/// A command that may stop on conflicts: stopped there it is `Conflicts`, not an error.
fn stopped_on_conflicts(
    root: &std::path::Path,
    result: Result<Vec<u8>, spork_git::GitError>,
) -> Result<spork_git::Outcome, spork_git::GitError> {
    match result {
        Ok(_) => Ok(spork_git::Outcome::Done),
        Err(_) if spork_git::status(root).is_ok_and(|s| s.iter().any(|e| e.conflicted)) => {
            Ok(spork_git::Outcome::Conflicts)
        }
        Err(e) => Err(e),
    }
}

fn create_branch(
    root: &std::path::Path,
    name: &str,
    start: &str,
    form: NewBranchForm,
) -> Result<(), spork_git::GitError> {
    match (form.checkout, form.overwrite) {
        (switch, false) => spork_git::create_branch(root, name, Some(start), switch),
        (true, true) => spork_git::run(root, &["switch", "--force-create", name, start]).map(drop),
        (false, true) => spork_git::run(root, &["branch", "--force", name, start]).map(drop),
    }
}

impl RepoView {
    /// Branches section menu: delete every local branch already merged into HEAD (M04 §5).
    pub(crate) fn delete_merged_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run_op(
            window,
            cx,
            spork_git::branch::merged,
            |_, names, window, cx| {
                if names.is_empty() {
                    return window
                        .push_notification(Notification::info("No merged branches to delete"), cx);
                }
                let view = cx.entity();
                window.open_alert_dialog(cx, move |dialog, _, _| {
                    let (view, names) = (view.clone(), names.clone());
                    dialog
                    .title(format!("Delete {} merged branch(es)?", names.len()))
                    .description(format!(
                        "{}\n\nTheir commits are all in the current branch, so nothing is lost.",
                        names.join(", ")
                    ))
                    .confirm()
                    .ok_text("Delete")
                    .ok_variant(ButtonVariant::Danger)
                    .on_ok(move |_, window, cx| {
                        let names = names.clone();
                        view.update(cx, |this, cx| {
                            this.run_op(
                                window,
                                cx,
                                move |root| {
                                    for name in &names {
                                        spork_git::delete_branch(root, name)?;
                                    }
                                    Ok(format!("Deleted {} branch(es)", names.len()))
                                },
                                notify_success,
                            )
                        });
                        true
                    })
                });
            },
        );
    }

    /// Checkout was refused because local changes would be overwritten (M04 §0).
    fn offer_stash_checkout(
        &mut self,
        branch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, branch) = (view.clone(), branch.clone());
            dialog
                .title(format!("Your changes are in the way of {branch}"))
                .description("Spork can stash them, switch, and put them back. If they clash with the branch, the files are marked as conflicts and the stash is kept, so nothing is lost.")
                .confirm()
                .ok_text("Stash, switch and reapply")
                .on_ok(move |_, window, cx| {
                    let branch = branch.clone();
                    view.update(cx, |this, cx| {
                        this.run_outcome(window, cx, format!("Switched to {branch} with your changes"), move |root| {
                            spork_git::branch::checkout_with_stash(root, &branch)
                        })
                    });
                    true
                })
        });
    }

    /// Delete a local branch (M04 §5), optionally its remote branch too; unmerged work is only
    /// thrown away after a second, explicit confirmation.
    fn delete_dialog(&mut self, branch: String, window: &mut Window, cx: &mut Context<Self>) {
        // the remote branch it tracks, while it still exists
        let upstream = match &self.repo {
            Some(Ok(repo)) => repo
                .refs
                .iter()
                .find(|r| r.kind == RefKind::Local && r.name == branch && !r.upstream_gone)
                .and_then(|r| r.upstream.clone()),
            _ => None,
        };
        self.delete_remote_too = false;
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let remote_too = view.read(cx).delete_remote_too;
            let (check_view, ok_view, branch, upstream) =
                (view.clone(), view.clone(), branch.clone(), upstream.clone());
            dialog
                .title(format!("Delete branch {branch}?"))
                .description(
                    v_flex()
                        .gap_2()
                        .child("Git only deletes it if its commits are merged somewhere, so no work is lost.")
                        .when_some(upstream.clone(), |d, up| {
                            d.child(
                                Checkbox::new("delete-remote-too")
                                    .label(format!("Also delete {up} on the remote"))
                                    .checked(remote_too)
                                    .on_click(move |checked, _, cx| {
                                        check_view.update(cx, |this, cx| {
                                            this.delete_remote_too = *checked;
                                            cx.notify();
                                        })
                                    }),
                            )
                        }),
                )
                .confirm()
                .ok_text("Delete")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let (branch, upstream) = (branch.clone(), upstream.clone());
                    ok_view.update(cx, |this, cx| {
                        let upstream = upstream.filter(|_| this.delete_remote_too);
                        this.delete_branch(branch, upstream, false, window, cx)
                    });
                    true
                })
        });
    }

    fn delete_branch(
        &mut self,
        branch: String,
        upstream: Option<String>,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (retry_branch, retry_upstream) = (branch.clone(), upstream.clone());
        self.run_op(
            window,
            cx,
            move |root| {
                let deleted = if force {
                    spork_git::branch::force_delete(root, &branch)
                } else {
                    spork_git::delete_branch(root, &branch)
                };
                match deleted {
                    // asked about next instead of failing
                    Err(e) if spork_git::branch::is_unmerged_refusal(&e) => return Ok(None),
                    other => other?,
                }
                if let Some(up) = &upstream {
                    spork_git::branch::delete_remote(root, up)?;
                }
                Ok(Some(match upstream {
                    Some(up) => format!("Deleted {branch} and {up}"),
                    None => format!("Deleted branch {branch}"),
                }))
            },
            move |this, deleted, window, cx| match deleted {
                Some(msg) => notify_success(this, msg, window, cx),
                None => this.confirm_force_delete(retry_branch, retry_upstream, window, cx),
            },
        );
    }

    fn confirm_force_delete(
        &mut self,
        branch: String,
        upstream: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, branch, upstream) = (view.clone(), branch.clone(), upstream.clone());
            dialog
                .title(format!("{branch} isn't merged"))
                .description("It has commits that aren't on any other branch. Delete it anyway? Its commits stay in View ▸ Show Lost Commits for a while.")
                .confirm()
                .ok_text("Delete anyway")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let (branch, upstream) = (branch.clone(), upstream.clone());
                    view.update(cx, |this, cx| this.delete_branch(branch, upstream, true, window, cx));
                    true
                })
        });
    }

    /// Lean branching (M13 §4): a new branch from the main branch.
    pub(crate) fn start_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let main = spork_git::lean::main_branch(&repo.root).unwrap_or_else(|| "HEAD".into());
        self.new_branch_dialog(main, window, cx)
    }

    /// Lean branching (M13 §4): merge the current branch into main with a merge commit, or
    /// open a pull request for it instead when an account is connected.
    pub(crate) fn finish_branch_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let (Some(branch), Some(main)) =
            (repo.head.clone(), spork_git::lean::main_branch(&repo.root))
        else {
            return window.push_notification(
                Notification::info("No main branch found to finish into"),
                cx,
            );
        };
        if branch == main {
            return window.push_notification(
                Notification::info(format!(
                    "Check out the branch to finish; {main} is the main branch"
                )),
                cx,
            );
        }
        self.finish_delete = true;
        let hosted = self.hosted.is_some();
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let delete = view.read(cx).finish_delete;
            let (check_view, pr_view, ok_view) = (view.clone(), view.clone(), view.clone());
            let (branch, main) = (branch.clone(), main.clone());
            let pr_branch = branch.clone();
            dialog
                .title(format!("Finish {branch}"))
                .description(
                    v_flex()
                        .gap_2()
                        .child(format!(
                            "Switches to {main} and merges {branch} into it with a merge commit."
                        ))
                        .child(
                            Checkbox::new("finish-delete")
                                .label(format!("Delete {branch} afterwards"))
                                .checked(delete)
                                .on_click(move |checked, _, cx| {
                                    check_view.update(cx, |this, cx| {
                                        this.finish_delete = *checked;
                                        cx.notify();
                                    })
                                }),
                        )
                        .when(hosted, |d| {
                            d.child(
                                Button::new("finish-pr")
                                    .small()
                                    .ghost()
                                    .label("Open a pull request instead…")
                                    .on_click(move |_, window, cx| {
                                        window.close_dialog(cx);
                                        let branch = pr_branch.clone();
                                        pr_view.update(cx, |this, cx| {
                                            this.pull_request_dialog(branch, window, cx)
                                        })
                                    }),
                            )
                        }),
                )
                .confirm()
                .ok_text(format!("Merge into {main}"))
                .on_ok(move |_, window, cx| {
                    let (branch, main) = (branch.clone(), main.clone());
                    ok_view.update(cx, |this, cx| {
                        let delete = this.finish_delete;
                        let done = format!("Finished {branch} into {main}");
                        this.run_outcome(window, cx, done, move |root| {
                            spork_git::lean::finish(root, &branch, &main, delete)
                        })
                    });
                    true
                })
        });
    }

    /// Merge dialog (M11 §1): how `branch` comes in, and whether to commit it right away.
    /// Works out in the background whether bringing `other` in would conflict; dialogs show
    /// [`Self::conflict_line`] meanwhile.
    fn check_conflicts(&mut self, other: String, cx: &mut Context<Self>) {
        self.conflicts = None;
        let Some(Ok(repo)) = &self.repo else { return };
        let root = repo.root.clone();
        self._conflict_check = Some(cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move { spork_git::predict_conflicts(&root, &other) })
                .await;
            this.update(cx, |this, cx| {
                this.conflicts = Some(found);
                cx.notify();
            })
            .ok();
        }));
    }

    /// "Checking…", "No conflicts expected", or which files will conflict.
    pub(crate) fn conflict_line(&self, cx: &App) -> Div {
        let theme = cx.theme();
        let (text, color) = match &self.conflicts {
            None => ("Checking for conflicts…".to_owned(), theme.muted_foreground),
            Some(None) => (String::new(), theme.muted_foreground),
            Some(Some(files)) if files.is_empty() => {
                ("No conflicts expected.".to_owned(), theme.green)
            }
            Some(Some(files)) => {
                let shown: Vec<&str> = files.iter().take(5).map(String::as_str).collect();
                let more = files.len().saturating_sub(shown.len());
                let more = if more > 0 {
                    format!(" and {more} more")
                } else {
                    String::new()
                };
                (
                    format!(
                        "Will conflict in {} file{}: {}{more}",
                        files.len(),
                        if files.len() == 1 { "" } else { "s" },
                        shown.join(", ")
                    ),
                    theme.warning,
                )
            }
        };
        div().text_sm().text_color(color).child(text)
    }

    /// Rebase dialog: the current branch's commits replayed on `onto` (a branch, tag or commit).
    pub(crate) fn rebase_dialog(
        &mut self,
        onto: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = match &self.repo {
            Some(Ok(repo)) => repo
                .refs
                .iter()
                .find(|r| r.is_head && r.kind == RefKind::Local)
                .map_or("the current branch".to_owned(), |r| r.name.clone()),
            _ => return,
        };
        self.check_conflicts(onto.clone(), cx);
        let view = cx.entity();
        let muted = cx.theme().muted_foreground;
        let shown: String = if onto.len() == 40 {
            onto.chars().take(7).collect()
        } else {
            onto.clone()
        };
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let (view, onto) = (view.clone(), onto.clone());
            dialog
                .title(format!("Rebase {current} onto {shown}"))
                .description(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().text_color(muted).child(
                            "Replays the branch's own commits on top of it. Local changes are stashed and put back.",
                        ))
                        .child(view.read(cx).conflict_line(cx)),
                )
                .confirm()
                .ok_text("Rebase")
                .on_ok(move |_, window, cx| {
                    let onto = onto.clone();
                    view.update(cx, |this, cx| {
                        this.run_outcome(window, cx, "Rebased".into(), move |root| {
                            spork_git::rebase(root, &onto)
                        })
                    });
                    true
                })
        });
    }

    fn merge_dialog(&mut self, branch: String, window: &mut Window, cx: &mut Context<Self>) {
        self.merge_options = (MergeKind::Auto, true);
        self.check_conflicts(branch.clone(), cx);
        let into = match &self.repo {
            Some(Ok(repo)) => repo
                .refs
                .iter()
                .find(|r| r.is_head && r.kind == RefKind::Local)
                .map_or("the current branch".to_owned(), |r| r.name.clone()),
            _ => return,
        };
        let view = cx.entity();
        let muted = cx.theme().muted_foreground;
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let (kind, commit) = view.read(cx).merge_options;
            let pick = |id: &'static str, label: &'static str, value: MergeKind| {
                let view = view.clone();
                Button::new(id)
                    .small()
                    .label(label)
                    .selected(kind == value)
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.merge_options.0 = value;
                            cx.notify();
                        })
                    })
            };
            let hint = match kind {
                MergeKind::Auto => "Moves the branch forward when it has no commits of its own; otherwise creates a merge commit.",
                MergeKind::Commit => "Always records the merge as its own commit, keeping the branch visible in the graph.",
                MergeKind::Squash => "Stages all of its changes as one; you commit them yourself in Local Changes.",
            };
            let (check_view, ok_view, branch) = (view.clone(), view.clone(), branch.clone());
            dialog
                .title(format!("Merge {branch} into {into}"))
                .description(
                    v_flex()
                        .gap_2()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(pick("merge-auto", "Fast-forward if possible", MergeKind::Auto))
                                .child(pick("merge-commit", "Always a merge commit", MergeKind::Commit))
                                .child(pick("merge-squash", "Squash", MergeKind::Squash)),
                        )
                        .child(div().text_sm().text_color(muted).child(hint))
                        .child(view.read(cx).conflict_line(cx))
                        .child(
                            Checkbox::new("merge-commit-now")
                                .label("Commit the merge (untick to review it in Local Changes first)")
                                .checked(commit && kind != MergeKind::Squash)
                                .disabled(kind == MergeKind::Squash)
                                .on_click(move |checked, _, cx| {
                                    check_view.update(cx, |this, cx| {
                                        this.merge_options.1 = *checked;
                                        cx.notify();
                                    })
                                }),
                        ),
                )
                .confirm()
                .ok_text("Merge")
                .on_ok(move |_, window, cx| {
                    let branch = branch.clone();
                    ok_view.update(cx, |this, cx| {
                        let (kind, commit) = this.merge_options;
                        let done = if kind == MergeKind::Squash || !commit {
                            format!("Merged {branch}: review and commit it in Local Changes")
                        } else {
                            format!("Merged {branch}")
                        };
                        this.run_outcome(window, cx, done, move |root| {
                            spork_git::merge_with(root, &branch, kind, commit)
                        })
                    });
                    true
                })
        });
    }

    /// A history label dropped on another (M03 §3): merge one branch into the other or rebase it,
    /// checking the target out first when it isn't current; onto a remote branch, push there.
    pub(crate) fn ref_dropped(
        &mut self,
        dragged: Label,
        target: Label,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (a, b) = (dragged.name, target.name);
        if a == b || dragged.kind == RefKind::Tag {
            return;
        }
        if target.kind == RefKind::Remote {
            if let (RefKind::Local, Some(to)) = (dragged.kind, self.split_remote_branch(&b)) {
                self.push_dialog(a, Some(to), window, cx);
            }
            return;
        }
        let head = target.is_head;
        let choices = if head {
            [
                format!("Merge {a} into {b}"),
                format!("Rebase {b} onto {a}"),
            ]
        } else {
            [
                format!("Check out {b}, merge {a} into it"),
                format!("Check out {b}, rebase it onto {a}"),
            ]
        };
        let items = choices
            .iter()
            .map(|c| CommandItem::new().label(c.clone()))
            .collect();
        let (view, state) = (cx.entity(), cx.new(|cx| CommandState::new(window, cx)));
        crate::shell::palette(
            window,
            cx,
            state,
            items,
            "Drop: choose what to do…",
            move |ix, window, cx| {
                window.close_dialog(cx);
                let (a, b, rebase) = (a.clone(), b.clone(), ix == 1);
                let done = if rebase {
                    format!("Rebased {b} onto {a}")
                } else {
                    format!("Merged {a} into {b}")
                };
                view.update(cx, |this, cx| {
                    this.run_outcome(window, cx, done, move |root| {
                        if !head {
                            spork_git::checkout(root, &b)?;
                        }
                        if rebase {
                            spork_git::rebase(root, &a)
                        } else {
                            spork_git::merge(root, &a)
                        }
                    })
                });
            },
        );
    }

    pub(crate) fn upstream_of(&self, branch: &str) -> Option<String> {
        let Some(Ok(repo)) = &self.repo else {
            return None;
        };
        repo.refs
            .iter()
            .find(|r| r.kind == RefKind::Local && r.name == branch)?
            .upstream
            .clone()
    }

    /// "Rename" dialog; offers to rename the remote branch too when there is an upstream (M04 §4).
    fn rename_dialog(&mut self, old: String, window: &mut Window, cx: &mut Context<Self>) {
        let upstream = self.upstream_of(&old);
        self.branch_name
            .update(cx, |s, cx| s.set_value(old.clone(), window, cx));
        self.rename_on_remote = false;
        let (view, input) = (cx.entity(), self.branch_name.clone());
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let (view, input, old) = (view.clone(), input.clone(), old.clone());
            let on_remote = view.read(cx).rename_on_remote;
            let toggle = view.clone();
            dialog
                .title(format!("Rename {old}"))
                .description(
                    v_flex()
                        .gap_3()
                        .child(spork_workbench::TextField::new(&input))
                        .children(upstream.as_ref().map(|up| {
                            Checkbox::new("rename-remote")
                                .label(format!("Also rename {up} on the remote"))
                                .checked(on_remote)
                                .on_click(move |checked, _, cx| {
                                    toggle.update(cx, |this, cx| {
                                        this.rename_on_remote = *checked;
                                        cx.notify();
                                    })
                                })
                        })),
                )
                .confirm()
                .ok_text("Rename")
                .on_ok(move |_, window, cx| {
                    let (new, old) = (input.read(cx).value().trim().to_owned(), old.clone());
                    view.update(cx, |this, cx| {
                        let on_remote = this.rename_on_remote;
                        this.run_op(
                            window,
                            cx,
                            move |root| {
                                spork_git::branch::rename(root, &old, &new, on_remote)
                                    .map(|()| format!("Renamed {old} to {new}"))
                            },
                            notify_success,
                        )
                    });
                    true
                })
        });
    }

    /// "Set upstream" dialog: the remote branch `branch` pulls from and pushes to (M03 §4).
    fn upstream_dialog(&mut self, branch: String, window: &mut Window, cx: &mut Context<Self>) {
        let value = self
            .upstream_of(&branch)
            .unwrap_or_else(|| format!("origin/{branch}"));
        self.input_dialog(
            format!("Set upstream of {branch}"),
            "The remote branch it pulls from and pushes to, e.g. origin/main.".into(),
            value,
            move |root, up| {
                spork_git::branch::set_upstream(root, &branch, &up)
                    .map(|()| format!("{branch} now tracks {up}"))
            },
            window,
            cx,
        )
    }
}

fn notify_success(
    _: &mut RepoView,
    message: String,
    window: &mut Window,
    cx: &mut Context<RepoView>,
) {
    window.push_notification(Notification::success(message), cx);
}

/// What the reference app offers while git has paused an operation (`Git.Ongoing.Rebase.Actions`): at the
/// top of the branches popup, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ongoing {
    AbortRebase,
    AbortMerge,
    AbortCherryPick,
    ContinueCherryPick,
    AbortRevert,
    ContinueRebase,
    SkipCommit,
}

impl Ongoing {
    const ALL: [Ongoing; 7] = [
        Ongoing::AbortRebase,
        Ongoing::AbortMerge,
        Ongoing::AbortCherryPick,
        Ongoing::ContinueCherryPick,
        Ongoing::AbortRevert,
        Ongoing::ContinueRebase,
        Ongoing::SkipCommit,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Ongoing::AbortRebase => "Abort Rebase",
            Ongoing::AbortMerge => "Abort Merge",
            Ongoing::AbortCherryPick => "Abort Cherry-Pick",
            Ongoing::ContinueCherryPick => "Continue Cherry-Pick",
            Ongoing::AbortRevert => "Abort Revert",
            Ongoing::ContinueRebase => "Continue Rebase",
            Ongoing::SkipCommit => "Skip Commit",
        }
    }

    /// Those that apply while `op` is paused; a `git am` has none.
    pub(crate) fn for_state(op: OpState) -> Vec<Ongoing> {
        Self::ALL
            .into_iter()
            .filter(|action| match action {
                Ongoing::AbortRebase | Ongoing::ContinueRebase | Ongoing::SkipCommit => {
                    matches!(op, OpState::Rebasing { .. })
                }
                Ongoing::AbortMerge => op == OpState::Merging,
                Ongoing::AbortCherryPick | Ongoing::ContinueCherryPick => {
                    op == OpState::CherryPicking
                }
                Ongoing::AbortRevert => op == OpState::Reverting,
            })
            .collect()
    }
}

impl RepoView {
    pub(crate) fn ongoing_action(
        &mut self,
        action: Ongoing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            Ongoing::AbortRebase => self.abort_dialog(action, "rebase", window, cx),
            Ongoing::AbortMerge => self.abort_dialog(action, "merge", window, cx),
            Ongoing::AbortCherryPick => self.abort_dialog(action, "cherry-pick", window, cx),
            Ongoing::AbortRevert => self.abort_dialog(action, "revert", window, cx),
            Ongoing::ContinueRebase => self.run_outcome(
                window,
                cx,
                "Rebase successful".into(),
                spork_git::continue_op,
            ),
            Ongoing::ContinueCherryPick => self.run_outcome(
                window,
                cx,
                "Cherry-pick successful".into(),
                spork_git::continue_op,
            ),
            Ongoing::SkipCommit => {
                self.run_outcome(window, cx, "Rebase successful".into(), spork_git::skip_op)
            }
        }
    }

    /// The reference app's confirmation: "Abort Merge" / "Abort merge?", a rebase naming its repository.
    fn abort_dialog(
        &mut self,
        action: Ongoing,
        operation: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let repo_name = match &self.repo {
            Some(Ok(repo)) => repo
                .root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            _ => return,
        };
        let (message, done) = match action {
            Ongoing::AbortRebase => (
                format!("Abort rebase in {repo_name}?"),
                "Abort rebase succeeded".to_owned(),
            ),
            _ => {
                let mut name = operation.to_owned();
                name[..1].make_ascii_uppercase();
                (
                    format!("Abort {operation}?"),
                    format!("{name} abort succeeded"),
                )
            }
        };
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, done) = (view.clone(), done.clone());
            dialog
                .title(action.label())
                .description(message.clone())
                .confirm()
                .ok_text("Abort")
                .on_ok(move |_, window, cx| {
                    let done = done.clone();
                    view.update(cx, |this, cx| {
                        this.run_op(window, cx, spork_git::abort_op, move |_, (), window, cx| {
                            window.push_notification(Notification::success(done), cx)
                        })
                    });
                    true
                })
        });
    }

    /// "Checkout Tag or Revision…": a branch is checked out, anything else as a detached HEAD.
    pub(crate) fn checkout_revision_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx));
        input.update(cx, |state, cx| state.focus(window, cx));
        let (view, prompt) = (
            cx.entity(),
            "Enter reference (branch, tag) name or commit hash:",
        );
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, input) = (view.clone(), input.clone());
            dialog
                .title("Checkout")
                .description(
                    v_flex()
                        .gap_2()
                        .child(prompt)
                        .child(spork_workbench::TextField::new(&input).aria_label(prompt)),
                )
                .confirm()
                .ok_text("OK")
                .on_ok(move |_, window, cx| {
                    let reference = input.read(cx).value().trim().to_owned();
                    if reference.is_empty() {
                        return false;
                    }
                    view.update(cx, |this, cx| {
                        this.run_op(
                            window,
                            cx,
                            move |root| {
                                spork_git::run(root, &["checkout", &reference])
                                    .map(|_| format!("Checked out {reference}"))
                            },
                            notify_success,
                        )
                    });
                    true
                })
        });
    }

    /// "Compare with 'main'": the commits each side has that the other lacks.
    fn compare_with_current(&mut self, other: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self.current_branch() else {
            return;
        };
        let (a, b) = (other.clone(), current.clone());
        self.run_op(
            window,
            cx,
            move |root| {
                let log = |range: String| {
                    spork_git::run(root, &["log", "--format=%h %s", &range])
                        .map(|out| String::from_utf8_lossy(&out).trim().to_owned())
                };
                Ok((log(format!("{b}..{a}"))?, log(format!("{a}..{b}"))?))
            },
            move |_, (theirs, ours), window, cx| {
                window.open_alert_dialog(cx, move |dialog, _, _| {
                    let section = |id: &'static str, title: String, commits: &str| {
                        let commits = match commits.is_empty() {
                            true => "No commits".to_owned(),
                            false => commits.to_owned(),
                        };
                        v_flex()
                            .id(id)
                            .role(Role::Group)
                            .aria_label(title.clone())
                            .aria_value(commits.clone())
                            .gap_1()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                            .child(commits)
                    };
                    let title = format!("Comparing {other} with {current}");
                    dialog.title(title.clone()).description(
                        v_flex()
                            .id("compare-branches")
                            .role(Role::Group)
                            .aria_label(title)
                            .gap_3()
                            .text_sm()
                            .child(section(
                                "compare-theirs",
                                format!(
                                    "Commits that exist in {other} but don't exist in {current}:"
                                ),
                                &theirs,
                            ))
                            .child(section(
                                "compare-ours",
                                format!(
                                    "Commits that exist in {current} but don't exist in {other}:"
                                ),
                                &ours,
                            )),
                    )
                })
            },
        );
    }

    /// "Show Diff with Working Tree": the files that differ between `rev` and the working tree.
    fn diff_with_working_tree(&mut self, rev: String, window: &mut Window, cx: &mut Context<Self>) {
        let target = rev.clone();
        self.run_op(
            window,
            cx,
            move |root| {
                spork_git::run(root, &["diff", "--name-status", &target])
                    .map(|out| String::from_utf8_lossy(&out).trim().to_owned())
            },
            move |_, files, window, cx| {
                window.open_alert_dialog(cx, move |dialog, _, _| {
                    let title = format!("Changes Between {rev} and Current Working Tree");
                    let files = match files.is_empty() {
                        true => "No differences".to_owned(),
                        false => files.clone(),
                    };
                    dialog.title(title.clone()).description(
                        div()
                            .id("diff-with-working-tree")
                            .role(Role::Group)
                            .aria_label(title)
                            .aria_value(files.clone())
                            .text_sm()
                            .child(files),
                    )
                })
            },
        );
    }
}

/// Spork's branch and tag commands that the reference app's branch menu doesn't have: in Find Action, on
/// the selected commit's branch or tag (the current branch when the commit has none).
mod palette {
    use gpui_kit::*;

    actions!(
        spork,
        [
            ToggleFavoriteBranch,
            ShowOnlyThisBranch,
            HideBranchInGraph,
            MergeSelectedBranch,
            CreatePullRequestForBranch,
            SetTrackedBranch,
            CopyTagName,
            ShowTagDetails,
            PushAllTags,
            DeleteTagOnRemote,
            WriteReleaseNotes,
            PushSelectedTag,
        ]
    );
}

/// `(label, action)` of the commands above, for Find Action.
pub(crate) fn commands() -> Vec<(SharedString, Box<dyn gpui_kit::Action>)> {
    use palette::*;
    vec![
        (
            "Mark or Unmark as Favorite".into(),
            Box::new(ToggleFavoriteBranch),
        ),
        ("Show Only This Branch".into(), Box::new(ShowOnlyThisBranch)),
        ("Hide in Graph".into(), Box::new(HideBranchInGraph)),
        ("Merge into Current…".into(), Box::new(MergeSelectedBranch)),
        (
            "Create Pull Request for Selected Branch…".into(),
            Box::new(CreatePullRequestForBranch),
        ),
        ("Set Tracked Branch…".into(), Box::new(SetTrackedBranch)),
        ("Push Tag".into(), Box::new(PushSelectedTag)),
        ("Copy Tag Name".into(), Box::new(CopyTagName)),
        ("Tag Details…".into(), Box::new(ShowTagDetails)),
        ("Push All Tags".into(), Box::new(PushAllTags)),
        ("Delete Tag on Remote…".into(), Box::new(DeleteTagOnRemote)),
        (
            "Write Release Notes with AI…".into(),
            Box::new(WriteReleaseNotes),
        ),
        // The reference app's classic toolbar's VCS segment (`SegmentedVcsActionsBarGroup`), which only
        // Find Action reaches in its new UI: the branches popup and the VCS Operations popup
        (
            "Show Branches".into(),
            Box::new(crate::shell::QuickCheckout),
        ),
        (
            "Version Control".into(),
            Box::new(crate::shell::VcsOperationsPopup),
        ),
    ]
}

impl RepoView {
    /// Hooks [`commands`] to this view.
    pub(crate) fn branch_commands(&self, el: Div, cx: &mut Context<Self>) -> Div {
        use palette::*;
        el.on_action(cx.listener(|this, _: &ToggleFavoriteBranch, w, cx| {
            this.selected_ref_action(RefKind::Local, Action::Star, w, cx)
        }))
        .on_action(cx.listener(|this, _: &ShowOnlyThisBranch, w, cx| {
            this.selected_ref_action(RefKind::Local, Action::Only, w, cx)
        }))
        .on_action(cx.listener(|this, _: &HideBranchInGraph, w, cx| {
            this.selected_ref_action(RefKind::Local, Action::Hide, w, cx)
        }))
        .on_action(cx.listener(|this, _: &MergeSelectedBranch, w, cx| {
            this.selected_ref_action(RefKind::Local, Action::MergeDialog, w, cx)
        }))
        .on_action(cx.listener(|this, _: &CreatePullRequestForBranch, w, cx| {
            this.selected_ref_action(RefKind::Local, Action::PullRequest, w, cx)
        }))
        .on_action(cx.listener(|this, _: &SetTrackedBranch, w, cx| {
            this.selected_ref_action(RefKind::Local, Action::SetUpstream, w, cx)
        }))
        .on_action(cx.listener(|this, _: &CopyTagName, w, cx| {
            this.selected_ref_action(RefKind::Tag, Action::CopyName, w, cx)
        }))
        .on_action(cx.listener(|this, _: &ShowTagDetails, w, cx| {
            this.selected_ref_action(RefKind::Tag, Action::TagDetails, w, cx)
        }))
        .on_action(cx.listener(|this, _: &PushAllTags, w, cx| {
            this.ref_action(Action::PushAllTags, String::new(), w, cx)
        }))
        .on_action(cx.listener(|this, _: &DeleteTagOnRemote, w, cx| {
            this.selected_ref_action(RefKind::Tag, Action::DeleteTagRemote, w, cx)
        }))
        .on_action(cx.listener(|this, _: &WriteReleaseNotes, w, cx| {
            this.selected_ref_action(RefKind::Tag, Action::ReleaseNotes, w, cx)
        }))
        .on_action(cx.listener(|this, _: &PushSelectedTag, w, cx| {
            this.selected_ref_action(RefKind::Tag, Action::PushTag, w, cx)
        }))
    }

    /// `action` on the selected commit's branch or tag of `kind`, else on the current branch.
    fn selected_ref_action(
        &mut self,
        kind: RefKind,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(repo)) = &self.repo else { return };
        let id = self.selected_commit_id();
        let at = repo
            .refs
            .iter()
            .filter(|r| r.kind == kind && Some(&r.commit) == id.as_ref())
            .min_by_key(|r| !r.is_head)
            .map(|r| r.name.clone());
        match at.or_else(|| repo.head.clone().filter(|_| kind == RefKind::Local)) {
            Some(name) => self.ref_action(action, name, window, cx),
            None => window.push_notification(
                Notification::info(match kind {
                    RefKind::Tag => "Select a commit with a tag first",
                    _ => "Select a commit with a branch first",
                }),
                cx,
            ),
        }
    }
}

impl RepoView {
    /// The reference app's New Tag… dialog (Git ▸ New Tag…): the Git root, the current branch, the tag's
    /// name (Force once it names an existing tag), the commit (HEAD when empty, Validate checks
    /// it) and a message, which makes it an annotated tag.
    pub(crate) fn new_tag_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let root = repo.root.to_string_lossy().into_owned();
        let branch = repo.head.clone().unwrap_or_else(|| "HEAD".to_owned());
        let tags: Vec<String> = repo
            .refs
            .iter()
            .filter(|r| r.kind == RefKind::Tag)
            .map(|r| r.name.clone())
            .collect();
        let name = cx.new(|cx| InputState::new(window, cx));
        let commit = cx.new(|cx| InputState::new(window, cx));
        let message = cx.new(|cx| TextareaState::new(window, cx));
        name.update(cx, |state, cx| state.focus(window, cx));
        let force = std::rc::Rc::new(std::cell::Cell::new(false));
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let existing = tags.contains(&name.read(cx).value().trim().to_owned());
            let has_commit = !commit.read(cx).value().trim().is_empty();
            let label = |text: &'static str| div().w(px(LABEL_WIDTH)).flex_none().child(text);
            let row = |text: &'static str| h_flex().gap_2().child(label(text));
            let (force_flag, force_view) = (force.clone(), view.clone());
            let (validate_view, validate_commit) = (view.clone(), commit.clone());
            let root_choice = root.clone();
            let (ok_view, ok_name, ok_commit, ok_message, ok_force) = (
                view.clone(),
                name.clone(),
                commit.clone(),
                message.clone(),
                force.clone(),
            );
            dialog
                .title("Tag")
                .width(px(640.))
                .description(
                    v_flex()
                        .gap_2()
                        .child(
                            row("Git Root:").child(
                                // a long path is cut, not pushed out of the dialog
                                div().flex_1().min_w_0().child(
                                    spork_workbench::ComboBox::new("tag-git-root", root.clone())
                                        .w_full()
                                        .dropdown_menu(move |menu, _, _| {
                                            menu.item(
                                                PopupMenuItem::new(root_choice.clone())
                                                    .checked(true),
                                            )
                                        }),
                                ),
                            ),
                        )
                        .child(row("Current Branch:").child(branch.clone()))
                        .child(
                            row("Tag Name:").child(
                                spork_workbench::TextField::new(&name)
                                    .aria_label("Tag Name:")
                                    .flex_1(),
                            ),
                        )
                        .child(
                            h_flex()
                                .child(div().w(px(LABEL_WIDTH + 8.)).flex_none())
                                .child(
                                    Checkbox::new("tag-force")
                                        .label("Force")
                                        .checked(force.get() && existing)
                                        .disabled(!existing)
                                        .on_click(move |checked, _, cx| {
                                            force_flag.set(*checked);
                                            force_view.update(cx, |_, cx| cx.notify());
                                        }),
                                ),
                        )
                        .child(
                            row("Commit:")
                                .child(
                                    spork_workbench::TextField::new(&commit)
                                        .aria_label("Commit:")
                                        .flex_1(),
                                )
                                .child(
                                    spork_workbench::Button::new("tag-validate", "Validate")
                                        .disabled(!has_commit)
                                        .on_click(move |_, window, cx| {
                                            let rev =
                                                validate_commit.read(cx).value().trim().to_owned();
                                            validate_view.update(cx, |this, cx| {
                                                this.validate_commit(rev, window, cx)
                                            })
                                        }),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .items_start()
                                .child(label("Message:"))
                                .child(
                                    Textarea::new(&message)
                                        .aria_label("Message:")
                                        .h(px(80.))
                                        .flex_1(),
                                ),
                        ),
                )
                .confirm()
                .ok_text("Create Tag")
                .on_ok(move |_, window, cx| {
                    let tag = ok_name.read(cx).value().trim().to_owned();
                    if tag.is_empty() {
                        return false;
                    }
                    let commit = ok_commit.read(cx).value().trim().to_owned();
                    let message = ok_message.read(cx).value().to_string();
                    let force = ok_force.get() && existing;
                    ok_view.update(cx, |this, cx| {
                        this.run_op(
                            window,
                            cx,
                            move |root| {
                                let mut args = vec!["tag"];
                                if force {
                                    args.push("--force");
                                }
                                if !message.trim().is_empty() {
                                    args.extend(["--annotate", "--message", &message]);
                                }
                                args.push(&tag);
                                if !commit.is_empty() {
                                    args.push(&commit);
                                }
                                spork_git::run(root, &args).map(|_| format!("Created tag {tag}"))
                            },
                            notify_success,
                        )
                    });
                    true
                })
        });
    }

    /// The Tag dialog's Validate: the commit the text names, or git's error.
    fn validate_commit(&mut self, rev: String, window: &mut Window, cx: &mut Context<Self>) {
        self.run_op(
            window,
            cx,
            move |root| {
                let spec = format!("{rev}^{{commit}}");
                spork_git::run(root, &["log", "-1", "--format=%h %s", &spec, "--"])
                    .map(|out| String::from_utf8_lossy(&out).trim().to_owned())
            },
            |_, commit, window, cx| window.push_notification(Notification::info(commit), cx),
        )
    }
}

/// The Tag dialog's label column, as wide as its longest label.
const LABEL_WIDTH: f32 = 120.;
