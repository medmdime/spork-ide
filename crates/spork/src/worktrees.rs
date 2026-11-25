//! Worktrees and submodules in the sidebar (docs/specs/M12 §3–4), and the Git tool window's
//! Worktrees tab.

use std::path::PathBuf;

use crate::notices::Notification;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_git::{Submodule, Worktree};
use spork_workbench::tree::TreeItem;

use crate::RepoView;

/// The Worktrees tab's Help and Context Help (⇧⌘F1).
pub(crate) const HELP: &str = "https://git-scm.com/docs/git-worktree";

impl RepoView {
    pub(crate) fn worktree_items(
        &self,
        worktrees: &[Worktree],
        cx: &mut Context<Self>,
    ) -> Vec<TreeItem> {
        let view = cx.entity();
        worktrees
            .iter()
            .filter(|w| !w.is_main)
            .map(|w| {
                let folder = w.path.file_name().map_or_else(
                    || w.path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                let label = w.branch.clone().unwrap_or(folder);
                let (path, view) = (w.path.clone(), view.clone());
                TreeItem::new(label)
                    .icon(IconName::FolderGit2)
                    .context_menu(move |menu, _, _| {
                        let (open, remove, open_view, remove_view) =
                            (path.clone(), path.clone(), view.clone(), view.clone());
                        menu.item(PopupMenuItem::new("Open in this window").on_click(
                            move |_, _, cx| {
                                open_view.update(cx, |this, cx| this.switch_repo(open.clone(), cx))
                            },
                        ))
                        .item(PopupMenuItem::separator())
                        .item(
                            PopupMenuItem::new("Remove worktree…").on_click(
                                move |_, window, cx| {
                                    remove_view.update(cx, |this, cx| {
                                        this.confirm_remove_worktree(remove.clone(), window, cx)
                                    })
                                },
                            ),
                        )
                    })
            })
            .collect()
    }

    /// IntelliJ's Worktrees tab: every working tree with its branch and folder, a vertical
    /// toolbar (New Worktree…, Refresh, Delete…, Prune) and a context menu.
    pub(crate) fn worktrees_tab(&mut self, cx: &mut Context<Self>) -> Div {
        let Some(Ok(repo)) = &self.repo else {
            return v_flex();
        };
        let worktrees = repo.worktrees.clone();
        let tokens = spork_workbench::Tokens::get(cx);
        let muted = cx.theme().muted_foreground;
        let selected = self
            .selected_worktree
            .clone()
            .and_then(|path| worktrees.iter().find(|w| w.path == path).cloned());
        let deletable = selected
            .as_ref()
            .filter(|w| !w.is_main)
            .map(|w| w.path.clone());
        let prunable: Vec<PathBuf> = worktrees
            .iter()
            .filter(|w| w.prunable)
            .map(|w| w.path.clone())
            .collect();
        let toolbar = spork_workbench::shell::side_toolbar()
            .id("worktrees-toolbar")
            .role(Role::Toolbar)
            .aria_label("Worktrees")
            .child(
                spork_workbench::ActionButton::new("worktree-new", "general/add")
                    .tooltip("New Worktree…")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.new_branch_worktree_dialog(window, cx)
                    })),
            )
            .child(
                spork_workbench::ActionButton::new("worktree-refresh", "general/refresh")
                    .tooltip("Refresh")
                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
            )
            .child(
                spork_workbench::ActionButton::new("worktree-delete", "general/delete")
                    .tooltip("Delete…")
                    .disabled(deletable.is_none())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(path) = deletable.clone() {
                            this.confirm_remove_worktree(path, window, cx)
                        }
                    })),
            )
            .child(
                spork_workbench::ActionButton::new("worktree-prune", "actions/clearCash")
                    .tooltip("Prune")
                    .disabled(prunable.is_empty())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.confirm_prune_worktrees(&prunable, window, cx)
                    })),
            )
            .child(spork_workbench::shell::stripe_separator(
                spork_workbench::Tokens::get(cx),
            ))
            .child(
                spork_workbench::ActionButton::new("worktree-help", "general/questionMark")
                    .tooltip("Help")
                    .on_click(|_, _, cx| cx.open_url(HELP)),
            );
        // IntelliJ's empty state: the main worktree alone isn't worth a list
        if worktrees.iter().all(|w| w.is_main) {
            let create = spork_workbench::controls::link("worktree-create", "Create worktree…", cx)
                .on_click(
                    cx.listener(|this, _, window, cx| this.new_branch_worktree_dialog(window, cx)),
                );
            return h_flex().size_full().child(toolbar).child(
                v_flex()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .child(div().text_color(muted).child("No linked worktrees"))
                    .child(create),
            );
        }
        let view = cx.entity();
        let current = self.tab_root();
        let rows = worktrees.into_iter().enumerate().map(|(ix, worktree)| {
            let is_selected = selected.as_ref().is_some_and(|s| s.path == worktree.path);
            let folder = worktree.path.file_name().map_or_else(
                || worktree.path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
            let hint = match (worktree.is_main, worktree.locked, worktree.prunable) {
                (true, _, _) => Some("Main worktree"),
                (_, _, true) => Some("Prunable"),
                (_, true, _) => Some("Locked"),
                _ => None,
            };
            let path = worktree.path.clone();
            let view = view.clone();
            let name = worktree.branch.clone().unwrap_or(folder);
            // the window's own worktree is open already
            let current = current.as_ref() == Some(&worktree.path);
            h_flex()
                .id(("worktree", ix))
                .role(Role::Row)
                .aria_label(name.clone())
                .aria_selected(is_selected)
                .h(px(24.))
                .px_2()
                .gap_1p5()
                .rounded(px(4.))
                .cursor_pointer()
                .when(is_selected, |row| row.bg(tokens.selection_inactive))
                .when(!is_selected, |row| {
                    row.hover(|row| row.bg(tokens.selection_hover))
                })
                .child(spork_workbench::icon("vcs/worktree").size(px(16.)))
                .child(div().flex_none().child(name))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(muted)
                        .child(crate::repos::tilde(&worktree.path)),
                )
                .children(hint.map(|hint| div().flex_none().text_color(muted).child(hint)))
                .on_click(cx.listener({
                    let path = path.clone();
                    move |this, event: &ClickEvent, _, cx| {
                        if event.click_count() == 2 {
                            this.open_worktree(path.clone(), cx);
                        }
                        this.selected_worktree = Some(path.clone());
                        cx.notify();
                    }
                }))
                .context_menu(move |menu, _, _| {
                    let (open, lock, remove) = (path.clone(), path.clone(), path.clone());
                    let (open_view, lock_view, remove_view) =
                        (view.clone(), view.clone(), view.clone());
                    let locked = worktree.locked;
                    menu.item(
                        PopupMenuItem::new("Open Worktree")
                            .disabled(current)
                            .on_click(move |_, _, cx| {
                                open_view
                                    .update(cx, |this, cx| this.open_worktree(open.clone(), cx));
                            }),
                    )
                    .item(
                        // The reference app's toggle: ticked while locked
                        PopupMenuItem::new("Lock")
                            .checked(locked)
                            .disabled(worktree.is_main)
                            .on_click(move |_, window, cx| {
                                let path = lock.clone();
                                lock_view.update(cx, |this, cx| {
                                    this.run_op(
                                        window,
                                        cx,
                                        move |root| spork_git::lock_worktree(root, &path, !locked),
                                        |_, _, _, _| {},
                                    )
                                })
                            }),
                    )
                    .item(PopupMenuItem::separator())
                    .item(
                        PopupMenuItem::new("Delete…")
                            .disabled(worktree.is_main)
                            .on_click(move |_, window, cx| {
                                remove_view.update(cx, |this, cx| {
                                    this.confirm_remove_worktree(remove.clone(), window, cx)
                                })
                            }),
                    )
                })
        });
        h_flex().size_full().child(toolbar).child(
            div()
                .id("worktrees")
                .role(Role::Table)
                .aria_label("Worktree List")
                .size_full()
                .p_1()
                .overflow_y_scrollbar()
                .children(rows),
        )
    }

    fn confirm_remove_worktree(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = format!("Delete worktree at `{}`?", path.display());
        self.confirm_dialog(
            "Delete Worktree",
            message,
            "Delete Worktree",
            window,
            cx,
            move |this, window, cx| {
                let path = path.clone();
                let done = format!("Deleted worktree `{}`", path.display());
                this.run_op(
                    window,
                    cx,
                    move |root| spork_git::remove_worktree(root, &path),
                    move |_, _, window, cx| {
                        window.push_notification(Notification::success(done), cx)
                    },
                )
            },
        );
    }

    /// The reference app's Prune Worktrees question, naming the worktrees whose folders are gone.
    fn confirm_prune_worktrees(
        &mut self,
        prunable: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths: Vec<String> = prunable.iter().map(|p| p.display().to_string()).collect();
        let message = format!(
            "Remove Git records for missing worktrees: `{}`?",
            paths.join("`, `")
        );
        self.confirm_dialog(
            "Prune Worktrees",
            message,
            "Prune Worktrees",
            window,
            cx,
            |this, window, cx| {
                this.run_op(
                    window,
                    cx,
                    spork_git::prune_worktrees,
                    |_, _, window, cx| {
                        window.push_notification(
                            Notification::success("Successfully pruned worktrees"),
                            cx,
                        )
                    },
                )
            },
        );
    }

    /// A branch's New Worktree from 'x'…: the reference app's New Worktree dialog from `branch`.
    pub(crate) fn new_worktree_dialog(
        &mut self,
        branch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.worktree_dialog(Some(branch), window, cx)
    }

    pub(crate) fn submodule_items(
        &self,
        submodules: &[Submodule],
        cx: &mut Context<Self>,
    ) -> Vec<TreeItem> {
        let view = cx.entity();
        let root = match &self.repo {
            Some(Ok(repo)) => repo.root.clone(),
            _ => return Vec::new(),
        };
        submodules
            .iter()
            .map(|s| {
                let note = match s.state {
                    '-' => " (not initialized)",
                    '+' => " (different commit)",
                    'U' => " (conflict)",
                    _ => "",
                };
                let (path, view, sub_path) = (root.join(&s.path), view.clone(), s.path.clone());
                TreeItem::new(format!("{}{note}", s.path))
                    .icon(IconName::Package)
                    .context_menu(move |menu, _, _| {
                        let (path, open_view, update_view) =
                            (path.clone(), view.clone(), view.clone());
                        let (sync_view, remove_view, sub_path) =
                            (view.clone(), view.clone(), sub_path.clone());
                        menu.item(PopupMenuItem::new("Open in this window").on_click(
                            move |_, _, cx| {
                                open_view.update(cx, |this, cx| this.switch_repo(path.clone(), cx))
                            },
                        ))
                        .item(PopupMenuItem::new("Update all submodules").on_click(
                            move |_, window, cx| {
                                update_view.update(cx, |this, cx| {
                                    this.run_op(
                                        window,
                                        cx,
                                        spork_git::update_submodules,
                                        |_, _, window, cx| {
                                            window.push_notification(
                                                Notification::success("Submodules updated"),
                                                cx,
                                            )
                                        },
                                    )
                                })
                            },
                        ))
                        .item(PopupMenuItem::new("Sync URLs from .gitmodules").on_click(
                            move |_, window, cx| {
                                sync_view.update(cx, |this, cx| {
                                    this.run_op(
                                        window,
                                        cx,
                                        spork_git::submodule::sync,
                                        |_, _, window, cx| {
                                            window.push_notification(
                                                Notification::success("Submodule URLs synced"),
                                                cx,
                                            )
                                        },
                                    )
                                })
                            },
                        ))
                        .item(PopupMenuItem::separator())
                        .item(
                            PopupMenuItem::new("Remove submodule…").on_click(
                                move |_, window, cx| {
                                    let path = sub_path.clone();
                                    remove_view.update(cx, |this, cx| {
                                        this.confirm_remove_submodule(path, window, cx)
                                    })
                                },
                            ),
                        )
                    })
            })
            .collect()
    }

    fn confirm_remove_submodule(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, path) = (view.clone(), path.clone());
            dialog
                .title(format!("Remove submodule {path}?"))
                .description("Deletes its folder and its data under .git/modules, and stages the removal for your next commit.")
                .confirm()
                .ok_text("Remove")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let path = path.clone();
                    view.update(cx, |this, cx| {
                        this.run_op(
                            window,
                            cx,
                            move |root| spork_git::submodule::remove(root, &path).map(|()| path),
                            |_, path, window, cx| {
                                window.push_notification(Notification::success(format!("Removed {path}")), cx)
                            },
                        )
                    });
                    true
                })
        });
    }

    /// Right-click on the Submodules section header (M12 §3).
    pub(crate) fn add_submodule_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clone_url
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.field.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.set_placeholder("vendor/library", window, cx);
        });
        let (view, url, path) = (cx.entity(), self.clone_url.clone(), self.field.clone());
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, url, path) = (view.clone(), url.clone(), path.clone());
            dialog
                .title("Add submodule")
                .description(
                    v_flex()
                        .gap_2()
                        .child(spork_workbench::TextField::new(&url))
                        .child(spork_workbench::TextField::new(&path)),
                )
                .confirm()
                .ok_text("Add submodule")
                .on_ok(move |_, window, cx| {
                    let (url, path) = (
                        url.read(cx).value().to_string(),
                        path.read(cx).value().trim().to_owned(),
                    );
                    view.update(cx, |this, cx| {
                        this.run_op(
                            window,
                            cx,
                            move |root| spork_git::submodule::add(root, &url, &path).map(|()| path),
                            |_, path, window, cx| {
                                window.push_notification(
                                    Notification::success(format!(
                                        "Added {path}; commit to record it"
                                    )),
                                    cx,
                                )
                            },
                        )
                    });
                    true
                })
        });
    }

    /// New Worktree… (the Git menu, the branches popup, Worktrees): the reference app's New Worktree
    /// dialog from the current branch.
    pub(crate) fn new_branch_worktree_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.worktree_dialog(None, window, cx)
    }
}
