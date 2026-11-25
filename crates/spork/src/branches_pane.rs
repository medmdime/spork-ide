//! The branches pane on the left of the log, as in IntelliJ / the reference app (docs/reference-spec.md §4):
//! a vertical toolbar, the "Branch or tag" search and the tree (HEAD, Local, Remote, Tags…).
//! It folds away to a strip; its width is dragged on the split.
//!
//! A click picks a branch, which the toolbar, ⏎ (Update Branch Filter in Log), ⌥F1 (Navigate Log
//! to Selected Branch Head), F2 (Rename) and ⌦ act on; a double click updates the branch filter.
//! Branches Pane Settings ▸ On Single Click makes a click filter the log or navigate it as well.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, h_resizable, resizable_panel, v_flex,
};
use gpui_kit::*;
use spork_git::{Ref, RefKind};

use crate::RepoView;
use crate::branches::Action as RefAction;
use crate::util::ResultExt as _;

actions!(
    spork,
    [
        /// Git.Log.Branches.Change.Branch.Filter: the log shows the picked branch (⏎, double
        /// click).
        UpdateBranchFilterInLog,
        /// Git.Log.Branches.Navigate.Log.To.Selected.Branch (⌥F1).
        NavigateLogToSelectedBranch,
    ]
);

/// The name the pane's "HEAD (Current Branch)" row is picked under.
const HEAD: &str = "HEAD";

/// What a single click on a branch does besides picking it: Branches Pane Settings ▸ On Single
/// Click, nothing by default as in IntelliJ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BranchClick {
    /// Update Branch Filter.
    Filter,
    /// Navigate Log to Branch Head.
    Navigate,
}

/// The pane's keys (the reference app's macOS keymap), while it has the focus.
pub(crate) fn install(cx: &mut App) {
    let in_pane = || {
        KeyBindingContextPredicate::parse("BranchesPane")
            .ok()
            .map(std::rc::Rc::new)
    };
    let keys: Vec<KeyBinding> = [
        (
            "enter",
            Box::new(UpdateBranchFilterInLog) as Box<dyn Action>,
        ),
        ("alt-f1", Box::new(NavigateLogToSelectedBranch)),
        // Git.Rename.Local.Branch; F2 bound last, the one a menu shows
        ("shift-f6", Box::new(crate::shell::RenameSelectedBranch)),
        ("f2", Box::new(crate::shell::RenameSelectedBranch)),
    ]
    .into_iter()
    .filter_map(|(keys, action)| {
        KeyBinding::load(keys, action, in_pane(), false, None, cx).log_err()
    })
    .collect();
    cx.bind_keys(keys);
}

/// A click on a ref's row of the pane (`head`: the "HEAD (Current Branch)" row).
pub(crate) fn ref_click(
    r: &Ref,
    head: bool,
    view: Entity<RepoView>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let (kind, name) = match head {
        true => (RefKind::Local, HEAD.to_owned()),
        false => (r.kind, r.name.clone()),
    };
    move |event, window, cx| {
        let (name, count) = (name.clone(), event.click_count());
        view.update(cx, |this, cx| {
            this.branches_pane_click(kind, name, count, window, cx)
        })
    }
}

impl RepoView {
    /// Whether the pane's row of `kind` `name` is the one picked.
    pub(crate) fn branches_pane_picked(&self, kind: RefKind, name: &str) -> bool {
        self.log_ui
            .branch_pick
            .as_ref()
            .is_some_and(|(k, n)| *k == kind && n == name)
    }

    /// Whether the pane's HEAD row is the one picked.
    pub(crate) fn branches_pane_head_picked(&self) -> bool {
        self.branches_pane_picked(RefKind::Local, HEAD)
    }

    fn branches_pane_click(
        &mut self,
        kind: RefKind,
        name: String,
        count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.log_ui.branch_pick = Some((kind, name));
        if let Some(focus) = &self.log_ui.branches_focus {
            window.focus(focus, cx);
        }
        match (count, self.log_ui.branch_click) {
            (2.., _) | (_, Some(BranchClick::Filter)) => {
                self.branches_pane_update_filter(window, cx)
            }
            (_, Some(BranchClick::Navigate)) => self.branches_pane_navigate(window, cx),
            _ => {}
        }
        cx.notify();
    }

    /// The picked ref as it is now, HEAD's row as the current branch.
    fn branches_pane_ref(&self) -> Option<Ref> {
        let (Some(Ok(repo)), Some((kind, name))) = (&self.repo, &self.log_ui.branch_pick) else {
            return None;
        };
        repo.refs
            .iter()
            .find(|r| match name.as_str() {
                HEAD => r.is_head && r.kind == RefKind::Local,
                _ => r.kind == *kind && r.name == *name,
            })
            .cloned()
    }

    /// Update Branch Filter in Log: the log shows the picked branch, HEAD's row the current one.
    fn branches_pane_update_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, name)) = self.log_ui.branch_pick.clone() else {
            return;
        };
        let branch = (name != HEAD).then_some(name);
        self.show_only_branch(branch, window, cx);
    }

    /// Navigate Log to Selected Branch Head.
    fn branches_pane_navigate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(r) = self.branches_pane_ref() {
            self.reveal_commit(&r.commit, window, cx)
        }
    }

    /// The toolbar's New Branch… and ⌥⌘N (Git.New.Branch.In.Log): from the picked branch, else
    /// from HEAD.
    fn branches_pane_new_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let start = self
            .branches_pane_ref()
            .filter(|_| !self.branches_pane_head_picked())
            .map_or_else(|| HEAD.to_owned(), |r| r.name);
        self.new_branch_dialog(start, window, cx)
    }

    /// The toolbar's Delete and ⌦ on the picked branch, remote branch or tag.
    fn branches_pane_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.branches_pane_ref().filter(|r| !r.is_head) else {
            return;
        };
        let action = match r.kind {
            RefKind::Local => RefAction::Delete,
            RefKind::Remote => RefAction::DeleteRemote,
            RefKind::Tag => RefAction::DeleteTag,
        };
        self.ref_action(action, r.name, window, cx)
    }

    /// The log with the branches pane on its left (or its folded strip).
    pub(crate) fn with_branches(&mut self, tree: Div, log: Div, cx: &mut Context<Self>) -> Div {
        let border = cx.theme().border;
        let toolbar = self.branches_toolbar(cx);
        if !self.branches_open {
            return gpui_kit::component::h_flex()
                .size_full()
                .child(toolbar.border_r_1().border_color(border))
                .child(log.flex_1().min_w_0());
        }
        let focus = self
            .log_ui
            .branches_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let pane =
            gpui_kit::component::h_flex()
                .id("branches-pane")
                .role(Role::Group)
                .aria_label("Git Branches")
                .size_full()
                .track_focus(&focus)
                .key_context("BranchesPane")
                .on_action(
                    cx.listener(|this, _: &UpdateBranchFilterInLog, window, cx| {
                        this.branches_pane_update_filter(window, cx)
                    }),
                )
                .on_action(
                    cx.listener(|this, _: &NavigateLogToSelectedBranch, window, cx| {
                        this.branches_pane_navigate(window, cx)
                    }),
                )
                .on_action(cx.listener(
                    |this, _: &crate::shell::RenameSelectedBranch, window, cx| {
                        match this.branches_pane_ref() {
                            Some(r) if r.kind == RefKind::Local => {
                                this.ref_action(RefAction::Rename, r.name, window, cx)
                            }
                            _ => cx.propagate(),
                        }
                    },
                ))
                .on_action(cx.listener(
                    |this, _: &crate::shell::DeleteSelectedBranch, window, cx| {
                        this.branches_pane_delete(window, cx)
                    },
                ))
                .on_action(
                    cx.listener(|this, _: &crate::shell::NewBranch, window, cx| {
                        this.branches_pane_new_branch(window, cx)
                    }),
                )
                .child(toolbar.border_r_1().border_color(border))
                .child(tree.flex_1().min_w_0());
        v_flex().size_full().child(
            h_resizable("branches-split")
                .child(resizable_panel().size(px(280.)).child(pane))
                .child(resizable_panel().child(log)),
        )
    }

    /// The pane's vertical toolbar, top to bottom as in IntelliJ.
    fn branches_toolbar(&self, cx: &mut Context<Self>) -> Div {
        // shortcut: Fetch, Expand All and Collapse All stay unnamed to accessibility, as other
        // scenarios click those names without saying where; name them once those say "in"
        let icon = |id: &'static str, name: &str, tip: &'static str| {
            Button::new(id)
                .ghost()
                .small()
                .icon(spork_workbench::icon(name))
                .tooltip(tip)
        };
        let named = |id: &'static str, name: &str, tip: &'static str| {
            icon(id, name, tip).accessibility_label(tip.split(" (").next().unwrap_or(tip))
        };
        let separator = || div().w(px(16.)).h(px(1.)).my_1().bg(cx.theme().border);
        if !self.branches_open {
            return v_flex()
                .w(px(36.))
                .h_full()
                .flex_none()
                .items_center()
                .pt_1()
                .child(
                    named("branches-show", "general/chevronRight", "Show Git Branches").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.branches_open = true;
                            cx.notify();
                        }),
                    ),
                );
        }
        let picked = self.branches_pane_ref();
        let has_pick = picked.is_some();
        let deletable = picked.as_ref().is_some_and(|r| !r.is_head);
        let view = cx.entity();
        let (show_tags, group_dirs, on_click) = (
            self.branches_show_tags,
            self.branches_group_dirs,
            self.log_ui.branch_click,
        );
        // The reference app's Git.Log.Branches.Settings as it shows on a single repository: Group by
        // Repository only shows with several
        let settings = named(
            "branches-settings",
            "general/settings",
            "Branches Pane Settings",
        )
        .dropdown_menu(move |menu, _, _| {
            let toggle = |label: &str, on: bool, flip: fn(&mut RepoView)| {
                let view = view.clone();
                PopupMenuItem::new(label.to_owned())
                    .checked(on)
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            flip(this);
                            cx.notify();
                        })
                    })
            };
            // one of the two at most, as IntelliJ's selection handling mode
            let mode = |label: &str, mode: BranchClick| {
                let view = view.clone();
                PopupMenuItem::new(label.to_owned())
                    .checked(on_click == Some(mode))
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            let current = this.log_ui.branch_click;
                            this.log_ui.branch_click = (current != Some(mode)).then_some(mode);
                            cx.notify();
                        })
                    })
            };
            crate::log_toolbar::header(menu, "On Single Click")
                .item(mode("Update Branch Filter", BranchClick::Filter))
                .item(mode("Navigate Log to Branch Head", BranchClick::Navigate))
                .separator()
                .item(toggle("Group by Directory", group_dirs, |this| {
                    this.branches_group_dirs = !this.branches_group_dirs
                }))
                .item(toggle("Show Tags", show_tags, |this| {
                    this.branches_show_tags = !this.branches_show_tags
                }))
        });
        v_flex()
            .w(px(36.))
            .h_full()
            .flex_none()
            .items_center()
            .pt_1()
            .gap_0p5()
            .child(
                named("branches-hide", "general/chevronLeft", "Hide Git Branches").on_click(
                    cx.listener(|this, _, _, cx| {
                        this.branches_open = false;
                        cx.notify();
                    }),
                ),
            )
            .child(separator())
            .child(
                named("branches-new", "general/add", "New Branch… (⌥⌘N)").on_click(
                    cx.listener(|this, _, window, cx| this.branches_pane_new_branch(window, cx)),
                ),
            )
            .child(
                named("branches-update", "vcs/update", "Update Selected")
                    .on_click(cx.listener(|this, _, window, cx| this.pull_dialog(window, cx))),
            )
            .child(
                named("branches-delete", "general/delete", "Delete Branch")
                    .disabled(!deletable)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.branches_pane_delete(window, cx)),
                    ),
            )
            .child(named("branches-compare", "vcs/diff", "Compare with Current").disabled(true))
            .child(
                named("branches-search", "general/search", "Search Branches").on_click(
                    cx.listener(|this, _, window, cx| {
                        this.sidebar_filter.update(cx, |s, cx| s.focus(window, cx))
                    }),
                ),
            )
            .child(
                icon("branches-fetch", "vcs/fetch", "Fetch").on_click(cx.listener(
                    |this, _, window, cx| this.remote_op(crate::RemoteOp::Fetch, window, cx),
                )),
            )
            .child(
                named("branches-favorite", "nodes/star", "Mark/Unmark As Favorite")
                    .disabled(!has_pick)
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(r) = this.branches_pane_ref() {
                            this.ref_action(RefAction::Star, r.name, window, cx)
                        }
                    })),
            )
            .child(
                named(
                    "branches-locate",
                    "general/locate",
                    "Navigate Log to Selected Branch Head (⌥F1)",
                )
                .disabled(!has_pick)
                .on_click(
                    cx.listener(|this, _, window, cx| this.branches_pane_navigate(window, cx)),
                ),
            )
            .child(separator())
            .child(settings)
            .child(
                icon("branches-expand", "general/expandAll", "Expand All").on_click(cx.listener(
                    |this, _, _, cx| {
                        // a new tree id starts every node over, open
                        this.branches_tree_gen += 1;
                        this.branches_expanded = true;
                        cx.notify();
                    },
                )),
            )
            .child(
                icon("branches-collapse", "general/collapseAll", "Collapse All").on_click(
                    cx.listener(|this, _, _, cx| {
                        this.branches_tree_gen += 1;
                        this.branches_expanded = false;
                        cx.notify();
                    }),
                ),
            )
    }
}
