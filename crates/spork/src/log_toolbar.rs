//! The log's toolbar, as in IntelliJ / the reference app (docs/reference-app/toolbars.md): the "Text or hash"
//! field with its Text Filter Settings (Regex, Match Case), the Branch, User, Date and Paths
//! filters; at the right the VCS Log
//! Toolbar: Enable Git Log Indexing while the log isn't indexed, Refresh, Cherry-Pick, View
//! Options and Go To Hash/Branch/Tag. Matching commits stay as they are, the others are dimmed:
//! the graph keeps its shape. Spork's Graph Options (sort, first parent, reflog), which the reference app
//! lacks, stay in Find Action and the View menu.

use std::collections::HashSet;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, WindowExt as _,
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_git::RefKind;
use spork_git::log::SearchIn;

use crate::RepoView;
use crate::notices::Notification;
use crate::prefs::Prefs;
use crate::util::ResultExt as _;
use crate::view_filters::Toggle;

type Menu = std::rc::Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

/// The log toolbar's width that holds the text field, the four filter chips and the actions.
const LOG_TOOLBAR_FULL_WIDTH: f32 = 760.;

actions!(
    spork,
    [
        /// Vcs.Log.ResumeIndexing, "Enable Git Log Indexing" while the log isn't indexed.
        EnableLogIndexing,
        FocusTextFilter,
        ToggleRegex,
        ToggleMatchCase,
        CompactReferencesView,
        ShowTagNames,
        ShowLongEdges,
        PreferCommitDate,
        AlignLabels,
        ShowDetails,
        ShowDiffPreview,
        MoveDiffPreviewToBottom,
        MoveDiffPreviewToRight,
        ShowChangesFromParents,
        ShowOnlyAffectedChanges,
        GroupByDirectory,
        GroupByRepository,
        /// Spork's Graph Options ▸ Sort, which the reference app lacks.
        ToggleTopoOrder,
    ]
);

/// The log's toggles in the View Options menus, each kept in the preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LogOption {
    CompactRefs,
    TagNames,
    LongEdges,
    CommitTime,
    RefsLeft,
    MyCommits,
    MergeCommits,
    CurrentBranch,
    NotCherryPicked,
    Details,
    DiffPreview,
    ByDirectory,
    ByRepository,
    OnlyAffected,
    ChangesToParents,
}

impl LogOption {
    pub(crate) fn get(self, p: &Prefs) -> bool {
        let v = &p.log_view;
        match self {
            LogOption::CompactRefs => p.log_compact_refs,
            LogOption::TagNames => p.log_tag_names,
            LogOption::LongEdges => v.long_edges,
            LogOption::CommitTime => p.log_commit_time,
            LogOption::RefsLeft => p.log_refs_left,
            LogOption::MyCommits => p.log_mark_mine,
            LogOption::MergeCommits => p.log_mark_merges,
            LogOption::CurrentBranch => p.log_mark_branch,
            LogOption::NotCherryPicked => v.not_cherry_picked,
            LogOption::Details => p.log_details,
            LogOption::DiffPreview => v.diff_preview,
            LogOption::ByDirectory => p.log_changes_by_directory,
            LogOption::ByRepository => v.by_repository,
            LogOption::OnlyAffected => v.only_affected,
            LogOption::ChangesToParents => v.changes_to_parents,
        }
    }

    fn set(self, p: &mut Prefs, on: bool) {
        let v = &mut p.log_view;
        let field = match self {
            LogOption::CompactRefs => &mut p.log_compact_refs,
            LogOption::TagNames => &mut p.log_tag_names,
            LogOption::LongEdges => &mut v.long_edges,
            LogOption::CommitTime => &mut p.log_commit_time,
            LogOption::RefsLeft => &mut p.log_refs_left,
            LogOption::MyCommits => &mut p.log_mark_mine,
            LogOption::MergeCommits => &mut p.log_mark_merges,
            LogOption::CurrentBranch => &mut p.log_mark_branch,
            LogOption::NotCherryPicked => &mut v.not_cherry_picked,
            LogOption::Details => &mut p.log_details,
            LogOption::DiffPreview => &mut v.diff_preview,
            LogOption::ByDirectory => &mut p.log_changes_by_directory,
            LogOption::ByRepository => &mut v.by_repository,
            LogOption::OnlyAffected => &mut v.only_affected,
            LogOption::ChangesToParents => &mut v.changes_to_parents,
        };
        *field = on;
    }

    /// The e2e state's name for it.
    fn key(self) -> &'static str {
        match self {
            LogOption::CompactRefs => "compact_refs",
            LogOption::TagNames => "tag_names",
            LogOption::LongEdges => "long_edges",
            LogOption::CommitTime => "commit_time",
            LogOption::RefsLeft => "refs_left",
            LogOption::MyCommits => "my_commits",
            LogOption::MergeCommits => "merge_commits",
            LogOption::CurrentBranch => "current_branch",
            LogOption::NotCherryPicked => "not_cherry_picked",
            LogOption::Details => "details",
            LogOption::DiffPreview => "diff_preview",
            LogOption::ByDirectory => "by_directory",
            LogOption::ByRepository => "by_repository",
            LogOption::OnlyAffected => "only_affected",
            LogOption::ChangesToParents => "changes_to_parents",
        }
    }

    const ALL: [LogOption; 15] = [
        LogOption::CompactRefs,
        LogOption::TagNames,
        LogOption::LongEdges,
        LogOption::CommitTime,
        LogOption::RefsLeft,
        LogOption::MyCommits,
        LogOption::MergeCommits,
        LogOption::CurrentBranch,
        LogOption::NotCherryPicked,
        LogOption::Details,
        LogOption::DiffPreview,
        LogOption::ByDirectory,
        LogOption::ByRepository,
        LogOption::OnlyAffected,
        LogOption::ChangesToParents,
    ];
}

/// The log's options beside the eye's in `Prefs` (IntelliJ's VcsLogUiProperties): long edges,
/// the Not Cherry-Picked Commits highlight, the changes grouped by repository, only those under
/// the Paths filter, a merge's changes to each parent, and the diff preview and where it sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LogViewPrefs {
    pub(crate) long_edges: bool,
    pub(crate) not_cherry_picked: bool,
    pub(crate) by_repository: bool,
    pub(crate) only_affected: bool,
    pub(crate) changes_to_parents: bool,
    pub(crate) diff_preview: bool,
    /// The diff preview right of the changes rather than under the log (IntelliJ's default).
    pub(crate) diff_preview_right: bool,
}

impl Default for LogViewPrefs {
    fn default() -> Self {
        Self::parse("")
    }
}

impl LogViewPrefs {
    pub(crate) fn parse(text: &str) -> Self {
        let on = |key: &str| {
            text.lines()
                .any(|l| l.strip_prefix(key).and_then(|v| v.strip_prefix('=')) == Some("true"))
        };
        LogViewPrefs {
            long_edges: on("log_long_edges"),
            not_cherry_picked: on("log_not_cherry_picked"),
            by_repository: on("log_changes_by_repository"),
            only_affected: on("log_only_affected"),
            changes_to_parents: on("log_changes_to_parents"),
            diff_preview: on("log_diff_preview"),
            diff_preview_right: on("log_diff_preview_right"),
        }
    }

    pub(crate) fn render(&self) -> String {
        format!(
            "log_long_edges={}\nlog_not_cherry_picked={}\nlog_changes_by_repository={}\nlog_only_affected={}\nlog_changes_to_parents={}\nlog_diff_preview={}\nlog_diff_preview_right={}\n",
            self.long_edges,
            self.not_cherry_picked,
            self.by_repository,
            self.only_affected,
            self.changes_to_parents,
            self.diff_preview,
            self.diff_preview_right,
        )
    }
}

/// What the log's panes keep between frames.
#[derive(Default)]
pub(crate) struct LogUi {
    /// Whether git keeps a commit graph here (Enable Git Log Indexing hides once it does);
    /// `None` until looked up.
    pub(crate) indexed: Option<bool>,
    _index_check: Option<Task<()>>,
    /// Not Cherry-Picked Commits: the branch compared with the current one and its commits the
    /// current branch has no equivalent of.
    pub(crate) not_picked: Option<(String, HashSet<String>)>,
    _compare: Option<Task<()>>,
    /// The selected merge's changes (its resolutions, and to each parent), by its id.
    pub(crate) merge: Option<(String, Result<spork_git::MergeChanges, String>)>,
    pub(crate) _merge_load: Option<Task<()>>,
    /// The branches pane's picked row, by kind and name ("HEAD" for its HEAD row), what a single
    /// click on one does besides, and the pane's focus, made at its first frame
    /// (branches_pane.rs).
    pub(crate) branch_pick: Option<(RefKind, String)>,
    pub(crate) branch_click: Option<crate::branches_pane::BranchClick>,
    pub(crate) branches_focus: Option<FocusHandle>,
}

/// The log's shortcuts (macOS keymap): ⌘L focuses the text filter, ⌃P groups the changes by
/// directory.
pub(crate) fn install(cx: &mut App) {
    let in_view = || {
        KeyBindingContextPredicate::parse("Spork")
            .ok()
            .map(std::rc::Rc::new)
    };
    let mut keys: Vec<(&str, Box<dyn Action>)> = vec![
        ("secondary-l", Box::new(FocusTextFilter)),
        ("secondary-down", Box::new(crate::commit_side::EditSource)),
        ("f4", Box::new(crate::commit_side::EditSource)),
    ];
    // shortcut: IntelliJ binds ⌃P on every OS, where Ctrl is Spork's Quick Launch off macOS;
    // bind it there once the changes tree takes the focus of its own
    if cfg!(target_os = "macos") {
        keys.push(("ctrl-p", Box::new(GroupByDirectory)));
    }
    let bindings: Vec<KeyBinding> = keys
        .into_iter()
        .filter_map(|(keys, action)| {
            KeyBinding::load(keys, action, in_view(), false, None, cx).log_err()
        })
        .collect();
    cx.bind_keys(bindings);
    crate::branches_pane::install(cx);
}

/// Find Action's entries for the log's own actions, as the reference app names them.
pub(crate) fn commands() -> Vec<(SharedString, Box<dyn Action>)> {
    vec![
        (
            "Enable Git Log Indexing".into(),
            Box::new(EnableLogIndexing),
        ),
        ("Focus Text Filter".into(), Box::new(FocusTextFilter)),
        ("Regex".into(), Box::new(ToggleRegex)),
        ("Match Case".into(), Box::new(ToggleMatchCase)),
        (
            "Compact References View".into(),
            Box::new(CompactReferencesView),
        ),
        ("Tag Names".into(), Box::new(ShowTagNames)),
        ("Long Edges".into(), Box::new(ShowLongEdges)),
        ("Commit Timestamp".into(), Box::new(PreferCommitDate)),
        ("References on the Left".into(), Box::new(AlignLabels)),
        ("Show Details".into(), Box::new(ShowDetails)),
        ("Show Diff Preview".into(), Box::new(ShowDiffPreview)),
        ("Bottom".into(), Box::new(MoveDiffPreviewToBottom)),
        ("Right".into(), Box::new(MoveDiffPreviewToRight)),
        (
            "Show Changes to Parents".into(),
            Box::new(ShowChangesFromParents),
        ),
        (
            "Show Only Affected Changes".into(),
            Box::new(ShowOnlyAffectedChanges),
        ),
        ("Directory".into(), Box::new(GroupByDirectory)),
        ("Repository".into(), Box::new(GroupByRepository)),
        ("Sort Topologically".into(), Box::new(ToggleTopoOrder)),
    ]
}

/// A section's title in a popup ("Show", "Highlight"), as IntelliJ heads a group: greyed and,
/// to accessibility, not an item.
pub(crate) fn header(menu: PopupMenu, title: &'static str) -> PopupMenu {
    menu.item(PopupMenuItem::element(move |_, _| div().child(title)).disabled(true))
}

/// A menu item turning `option` on or off.
pub(crate) fn option_item(
    view: &Entity<RepoView>,
    label: &'static str,
    option: LogOption,
    prefs: &Prefs,
) -> PopupMenuItem {
    let view = view.clone();
    PopupMenuItem::new(label)
        .checked(option.get(prefs))
        .on_click(move |_, window, cx| {
            view.update(cx, |this, cx| this.toggle_log_option(option, window, cx))
        })
}

impl RepoView {
    /// Hooks the log's actions to this view (shell.rs `on_actions` does the app's).
    pub(crate) fn log_toolbar_actions(&self, el: Div, cx: &mut Context<Self>) -> Div {
        macro_rules! toggles {
            ($el:expr, $($action:ty => $option:expr),* $(,)?) => {
                $el$(.on_action(cx.listener(|this, _: &$action, window, cx| {
                    this.toggle_log_option($option, window, cx)
                })))*
            };
        }
        let el = toggles!(el,
            CompactReferencesView => LogOption::CompactRefs,
            ShowTagNames => LogOption::TagNames,
            ShowLongEdges => LogOption::LongEdges,
            PreferCommitDate => LogOption::CommitTime,
            AlignLabels => LogOption::RefsLeft,
            ShowDetails => LogOption::Details,
            ShowDiffPreview => LogOption::DiffPreview,
            ShowChangesFromParents => LogOption::ChangesToParents,
            ShowOnlyAffectedChanges => LogOption::OnlyAffected,
            GroupByRepository => LogOption::ByRepository,
        );
        el.on_action(cx.listener(|this, _: &EnableLogIndexing, w, cx| this.index_log(w, cx)))
            // ⌃P groups the changes tree that has the focus: a tool window's are its own
            .on_action(cx.listener(
                |this, _: &GroupByDirectory, window, cx| match this.active_tool {
                    None => this.toggle_log_option(LogOption::ByDirectory, window, cx),
                    Some(_) => cx.propagate(),
                },
            ))
            .on_action(
                cx.listener(|this, _: &crate::commit_side::EditSource, w, cx| {
                    this.edit_source(w, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &FocusTextFilter, w, cx| {
                    this.toggle_text_filter_focus(w, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ToggleRegex, _, cx| {
                this.log_filters.regex ^= true;
                this.refilter(cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleMatchCase, _, cx| {
                this.log_filters.match_case ^= true;
                this.refilter(cx)
            }))
            .on_action(cx.listener(|this, _: &MoveDiffPreviewToBottom, _, cx| {
                this.place_diff_preview(false, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveDiffPreviewToRight, _, cx| {
                this.place_diff_preview(true, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleTopoOrder, _, cx| {
                this.toggle_filter(Toggle::TopoOrder, cx)
            }))
    }

    /// Turns a View Options toggle on or off, saved at once.
    pub(crate) fn toggle_log_option(
        &mut self,
        option: LogOption,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let on = !option.get(&self.prefs);
        option.set(&mut self.prefs, on);
        self.prefs.save();
        match option {
            LogOption::LongEdges => self.relayout_graph(),
            LogOption::NotCherryPicked if on => self.compare_cherry_picks(window, cx),
            LogOption::NotCherryPicked => self.log_ui.not_picked = None,
            _ => {}
        }
        cx.notify();
    }

    /// Diff Preview Location ▸ Bottom / Right; the preview shows if it was hidden.
    pub(crate) fn place_diff_preview(&mut self, right: bool, cx: &mut Context<Self>) {
        self.prefs.log_view.diff_preview_right = right;
        self.prefs.log_view.diff_preview = true;
        self.prefs.save();
        cx.notify();
    }

    /// After a load: the graph's long lines whole when View Options ▸ Long Edges says so.
    pub(crate) fn relayout_graph(&mut self) {
        let long_edges = self.prefs.log_view.long_edges;
        if let Some(Ok(repo)) = &mut self.repo {
            repo.history.relayout(long_edges);
        }
    }

    /// The log's options for the end-to-end tests' state.
    pub(crate) fn log_state(&self) -> serde_json::Value {
        let mut options: serde_json::Map<String, serde_json::Value> = LogOption::ALL
            .iter()
            .map(|o| (o.key().to_owned(), o.get(&self.prefs).into()))
            .collect();
        options.insert(
            "diff_preview_right".into(),
            self.prefs.log_view.diff_preview_right.into(),
        );
        options.insert("regex".into(), self.log_filters.regex.into());
        options.insert("match_case".into(), self.log_filters.match_case.into());
        options.insert("indexed".into(), self.log_ui.indexed.into());
        options.insert("matches".into(), self.commit_matches.into());
        if let Some(Ok(repo)) = &self.repo {
            options.insert("uncommitted".into(), repo.status.len().into());
            // the Branch filter: the branches shown alone, or HEAD
            let branch_filter = match self.filters.current_branch {
                true => vec!["HEAD".to_owned()],
                false => repo.only.clone(),
            };
            options.insert("branch_filter".into(), branch_filter.into());
        }
        options.insert(
            "branch_pick".into(),
            self.log_ui
                .branch_pick
                .as_ref()
                .map(|(_, name)| name.clone())
                .into(),
        );
        // Rebasing Commits' plan as git's todo list, oldest first
        options.insert(
            "rebase".into(),
            self.rebase_plan.as_ref().map(|p| p.lines()).into(),
        );
        options.insert(
            "rebase_selected".into(),
            self.rebase_plan
                .as_ref()
                .map(|p| p.selected_subjects())
                .into(),
        );
        options.insert(
            "changes_selected".into(),
            self.commit_file_selected.clone().into(),
        );
        options.insert("paths".into(), self.log_filters.path.clone().into());
        options.insert("merge_parent".into(), self.merge_parent.into());
        options.insert(
            "file_history".into(),
            self.file.as_ref().and_then(|f| f.history_len()).into(),
        );
        options.insert(
            "not_picked".into(),
            self.log_ui
                .not_picked
                .as_ref()
                .map(|(_, ids)| ids.len())
                .into(),
        );
        options.into()
    }

    /// Dimmed by Not Cherry-Picked Commits: on the compared branch's side, `id` is in the
    /// current branch already, or has an equivalent there.
    pub(crate) fn picked_elsewhere(&self, id: &str) -> bool {
        let (Some((branch, left)), Some(Ok(repo))) = (&self.log_ui.not_picked, &self.repo) else {
            return false;
        };
        // the comparison holds while the Branch filter shows that branch alone
        repo.only.len() == 1 && repo.only[0] == *branch && !left.contains(id)
    }

    /// Enable Git Log Indexing: git writes its commit graph, with changed-path filters.
    fn index_log(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run_op(
            window,
            cx,
            spork_git::log::write_index,
            |this, (), window, cx| {
                this.log_ui.indexed = Some(true);
                window.push_notification(Notification::success("Git Log indexed"), cx);
            },
        );
    }

    /// Whether the log is indexed, looked up once in the background.
    fn check_index(&mut self, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        if self.log_ui.indexed.is_some() || self.log_ui._index_check.is_some() {
            return;
        }
        let root = repo.root.clone();
        self.log_ui._index_check = Some(cx.spawn(async move |this, cx| {
            let indexed = cx
                .background_spawn(async move { spork_git::log::is_indexed(&root) })
                .await
                .log_err()
                .unwrap_or(true);
            this.update(cx, |this, cx| {
                this.log_ui.indexed = Some(indexed);
                cx.notify();
            })
            .ok();
        }));
    }

    /// ⌘L: the text filter, or back to the commits from it.
    fn toggle_text_filter_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .commit_filter
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
        {
            window.focus(&self.focus, cx);
            return cx.notify();
        }
        self.focus_commit_filter(window, cx)
    }

    /// Not Cherry-Picked Commits: compares the branch the Branch filter shows alone with the
    /// current one; without one, asks which (IntelliJ's "Select Source Branch").
    fn compare_cherry_picks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let head = repo
            .refs
            .iter()
            .find(|r| r.is_head && r.kind == RefKind::Local)
            .map(|r| r.name.clone());
        match &repo.only[..] {
            [one] if Some(one) != head.as_ref() => {
                self.log_compare_with_branch(one.clone(), window, cx)
            }
            _ => {
                let branches: Vec<String> = repo
                    .refs
                    .iter()
                    .filter(|r| r.kind != RefKind::Tag && Some(&r.name) != head.as_ref())
                    .map(|r| r.name.clone())
                    .collect();
                self.select_source_branch(branches, window, cx)
            }
        }
    }

    /// Finds `branch`'s commits the current branch has no equivalent of.
    fn log_compare_with_branch(
        &mut self,
        branch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(repo)) = &self.repo else { return };
        let root = repo.root.clone();
        let compared = branch.clone();
        self.log_ui._compare = Some(cx.spawn_in(window, async move |this, cx| {
            let left = cx
                .background_spawn(
                    async move { spork_git::log::not_cherry_picked(&root, &compared) },
                )
                .await;
            this.update_in(cx, |this, window, cx| {
                match left {
                    Ok(left) => this.log_ui.not_picked = Some((branch, left)),
                    Err(e) => window.push_notification(
                        Notification::error(format!("Could not compare with branch {branch}: {e}")),
                        cx,
                    ),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// "Select Source Branch": the branch to show alone and compare with the current one.
    fn select_source_branch(
        &mut self,
        branches: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _, cx| {
            let hover = cx.theme().list_hover;
            dialog
                .title("Select Source Branch")
                .child(
                    v_flex().children(branches.iter().enumerate().map(|(ix, name)| {
                        let (view, name) = (view.clone(), name.clone());
                        div()
                            .id(("source-branch", ix))
                            .role(Role::ListItem)
                            .aria_label(name.clone())
                            .px_2()
                            .py_1()
                            .rounded(px(4.))
                            .hover(move |row| row.bg(hover))
                            .child(name.clone())
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                view.update(cx, |this, cx| {
                                    this.show_only_branch(Some(name.clone()), window, cx);
                                    this.log_compare_with_branch(name.clone(), window, cx);
                                })
                            })
                    })),
                )
        });
    }

    /// The Git tool window's ⌄ after its tabs (Vcs.Log.ToolWindow.TabActions.DropDown).
    pub(crate) fn log_tab_actions(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let view = cx.entity();
        spork_workbench::ActionButton::new("git-tab-actions", "general/chevronDown")
            .aria_label("Tab Actions")
            .dropdown_menu(move |menu, _, _| {
                let run = |label: &'static str,
                           f: fn(&mut RepoView, &mut Window, &mut Context<RepoView>)| {
                    let view = view.clone();
                    PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        view.update(cx, |this, cx| f(this, window, cx))
                    })
                };
                menu.item(
                    run("Show History", |this, _, cx| {
                        this.open_file(".".into(), false, cx)
                    })
                    .icon(spork_workbench::icon("general/history")),
                )
                .item(run("Show VCS Console Tab", |this, _, cx| {
                    this.show_console = true;
                    this.show_terminal = false;
                    this.worktrees_tab = false;
                    cx.notify();
                }))
                .item(
                    run("Worktrees…", |_, window, cx| {
                        window.dispatch_action(Box::new(crate::shell::ShowWorktrees), cx)
                    })
                    .icon(spork_workbench::icon("vcs/worktree")),
                )
                .item(run(
                    "Open External Git Repository Log…",
                    RepoView::open_external_log,
                ))
                .separator()
                .item(run("Settings", |this, window, cx| {
                    this.settings_page(Some("Version Control/Log".into()), window, cx)
                }))
            })
    }

    /// Open External Git Repository Log…: a repository's log in a window of its own, beside
    /// this one.
    fn open_external_log(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Log".into()),
        });
        cx.spawn(async move |_, cx| {
            let Ok(Ok(Some(mut picked))) = paths.await else {
                return;
            };
            let Some(folder) = picked.pop() else { return };
            cx.update(|cx| crate::open_repo_window(vec![folder], None, cx));
        })
        .detach();
    }

    /// The log's toolbar.
    pub(crate) fn log_toolbar(&mut self, cx: &mut Context<Self>) -> Div {
        self.check_index(cx);
        let (only, branches) = match &self.repo {
            Some(Ok(repo)) => (
                repo.only.clone(),
                repo.refs
                    .iter()
                    .filter(|r| r.kind != RefKind::Tag)
                    .map(|r| (r.name.clone(), r.kind))
                    .collect::<Vec<_>>(),
            ),
            _ => (Vec::new(), Vec::new()),
        };
        let f = self.filters;
        let lf = &self.log_filters;
        let view = cx.entity();
        let muted = cx.theme().muted_foreground;

        // the text field: what it searches in its magnifier menu, the toggles inside
        let mode = self.search_mode_button(cx);
        let toggle =
            |id: &'static str, icon: &str, on: bool, tip: &'static str, set: fn(&mut RepoView)| {
                Button::new(id)
                    .ghost()
                    .xsmall()
                    .icon(spork_workbench::icon(icon))
                    .selected(on)
                    .tooltip(tip)
                    .accessibility_label(tip)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        set(this);
                        this.refilter(cx)
                    }))
            };
        let text = spork_workbench::TextField::new(&self.commit_filter)
            .cleanable(true)
            .prefix(mode)
            .suffix(
                h_flex()
                    .id("text-filter-settings")
                    .role(Role::Toolbar)
                    .aria_label("Text Filter Settings")
                    .child(toggle("f-regex", "inline/regex", lf.regex, "Regex", |t| {
                        t.log_filters.regex = !t.log_filters.regex
                    }))
                    .child(toggle(
                        "f-case",
                        "inline/matchCase",
                        lf.match_case,
                        "Match Case",
                        |t| t.log_filters.match_case = !t.log_filters.match_case,
                    )),
            );

        // Branch: HEAD, or one branch shown alone
        let branch_value = match (f.current_branch, only.is_empty()) {
            (true, _) => Some("HEAD".to_owned()),
            (false, false) => Some(only.join(", ")),
            _ => None,
        };
        let branch_menu: Menu = {
            let view = view.clone();
            std::rc::Rc::new(move |menu, window, cx| {
                fn pick(view: &Entity<RepoView>, name: &str) -> PopupMenuItem {
                    let (view, name) = (view.clone(), name.to_owned());
                    PopupMenuItem::new(name.clone()).on_click(move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.show_only_branch(Some(name.clone()), window, cx)
                        })
                    })
                }
                let head = {
                    let view = view.clone();
                    PopupMenuItem::new("HEAD").on_click(move |_, window, cx| {
                        view.update(cx, |this, cx| this.show_only_branch(None, window, cx))
                    })
                };
                let locals: Vec<String> = branches
                    .iter()
                    .filter(|(_, k)| *k == RefKind::Local)
                    .map(|(n, _)| n.clone())
                    .collect();
                let remotes: Vec<String> = branches
                    .iter()
                    .filter(|(_, k)| *k == RefKind::Remote)
                    .map(|(n, _)| n.clone())
                    .collect();
                let (local_view, remote_view) = (view.clone(), view.clone());
                menu.item(head)
                    .separator()
                    .submenu("Local", window, cx, move |m, _, _| {
                        locals.iter().fold(m, |m, n| m.item(pick(&local_view, n)))
                    })
                    .submenu("Remote", window, cx, move |m, _, _| {
                        remotes.iter().fold(m, |m, n| m.item(pick(&remote_view, n)))
                    })
            })
        };
        let branch = self.filter_chip(
            "f-branch",
            "Branch",
            branch_value,
            branch_menu.clone(),
            cx,
            |this, window, cx| this.clear_branch_filter(window, cx),
        );

        // User: you, or anyone who committed here
        let authors = self.top_authors(15);
        let me = match &self.repo {
            Some(Ok(repo)) => spork_git::config::get(&repo.root, "user.name"),
            _ => None,
        };
        let user_menu: Menu = {
            let view = view.clone();
            std::rc::Rc::new(move |menu, _, _| {
                let pick = |label: String, user: String| {
                    let view = view.clone();
                    PopupMenuItem::new(label).on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.log_filters.user = Some(user.clone());
                            this.refilter(cx)
                        })
                    })
                };
                let menu = match &me {
                    Some(me) => menu.item(pick("me".into(), me.clone())).separator(),
                    None => menu,
                };
                authors
                    .iter()
                    .fold(menu, |m, a| m.item(pick(a.clone(), a.clone())))
            })
        };
        let user = self.filter_chip(
            "f-user",
            "User",
            lf.user.clone(),
            user_menu.clone(),
            cx,
            |this, _, cx| {
                this.log_filters.user = None;
                this.refilter(cx)
            },
        );

        // Date: the last day, week or month
        let date_menu: Menu = {
            let view = view.clone();
            std::rc::Rc::new(move |menu, _, _| {
                [
                    ("Last 24 hours", 1),
                    ("Last 7 days", 7),
                    ("Last 30 days", 30),
                ]
                .into_iter()
                .fold(menu, |m, (label, days)| {
                    let view = view.clone();
                    m.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |d| d.as_secs() as i64);
                        view.update(cx, |this, cx| {
                            this.log_filters.since = Some((now - days * 86_400, label));
                            this.refilter(cx)
                        })
                    }))
                })
            })
        };
        let date = self.filter_chip(
            "f-date",
            "Date",
            lf.since.map(|(_, label)| label.to_owned()),
            date_menu.clone(),
            cx,
            |this, _, cx| {
                this.log_filters.since = None;
                this.refilter(cx)
            },
        );

        // Paths: commits that touch a path
        let paths_menu: Menu = {
            let view = view.clone();
            std::rc::Rc::new(move |menu, _, _| {
                let view = view.clone();
                menu.item(
                    PopupMenuItem::new("Select…").on_click(move |_, window, cx| {
                        view.update(cx, |this, cx| this.path_filter_dialog(window, cx))
                    }),
                )
            })
        };
        let paths = self.filter_chip(
            "f-paths",
            "Paths",
            lf.path.clone(),
            paths_menu.clone(),
            cx,
            |this, _, cx| this.set_path_filter(None, cx),
        );

        // IntelliJ's eye (Vcs.Log.PresentationSettings), kept in the preferences
        let prefs = self.prefs.clone();
        let view_options = {
            spork_workbench::ActionButton::new("f-view", "general/show")
                .popup_marker()
                .tooltip("View Options")
                .aria_label("View Options")
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
                    let item = |label: &'static str, option: LogOption| {
                        option_item(&view, label, option, &prefs)
                    };
                    let p = &prefs;
                    let (author, date, hash) = (p.log_author, p.log_date, p.log_hash);
                    let columns_view = view.clone();
                    // Root Names shows with more than one repository: Spork's log has one
                    header(menu, "Show")
                        .item(item("Compact References View", LogOption::CompactRefs))
                        .item(item("Tag Names", LogOption::TagNames))
                        .item(item("Long Edges", LogOption::LongEdges))
                        .item(item("Commit Timestamp", LogOption::CommitTime))
                        .item(item("References on the Left", LogOption::RefsLeft))
                        .submenu("Columns", window, cx, move |menu, _, _| {
                            let view = &columns_view;
                            menu.item(pref_item(view, "Author", author, |p| p.log_author ^= true))
                                .item(pref_item(view, "Date", date, |p| p.log_date ^= true))
                                .item(pref_item(view, "Hash", hash, |p| p.log_hash ^= true))
                        })
                        .separator()
                        .map(|menu| header(menu, "Highlight"))
                        .item(item("My Commits", LogOption::MyCommits))
                        .item(item("Merge Commits", LogOption::MergeCommits))
                        .item(item("Current Branch", LogOption::CurrentBranch))
                        .item(item(
                            "Not Cherry-Picked Commits",
                            LogOption::NotCherryPicked,
                        ))
                })
        };
        let icon_button = |id: &'static str, icon: &str, label: &'static str, tip: &'static str| {
            Button::new(id)
                .ghost()
                .small()
                .icon(spork_workbench::icon(icon))
                .tooltip(tip)
                .accessibility_label(label)
        };
        let selected = self.selected_commit_id();
        // too narrow for the filter chips: IntelliJ's ">" holds them, the same menus inside
        let width = self.log_toolbar_width.clone();
        let narrow = width.get() < px(LOG_TOOLBAR_FULL_WIDTH);
        let more = spork_workbench::ActionButton::new("f-more", "general/chevronRight")
            .tooltip("Show More Filters")
            .aria_label("Show More Filters")
            .dropdown_menu(move |menu, window, cx| {
                let (branch, user, date, paths) = (
                    branch_menu.clone(),
                    user_menu.clone(),
                    date_menu.clone(),
                    paths_menu.clone(),
                );
                menu.submenu("Branch", window, cx, move |m, w, c| branch(m, w, c))
                    .submenu("User", window, cx, move |m, w, c| user(m, w, c))
                    .submenu("Date", window, cx, move |m, w, c| date(m, w, c))
                    .submenu("Paths", window, cx, move |m, w, c| paths(m, w, c))
            });
        h_flex()
            .relative()
            .flex_none()
            .h(px(34.))
            .px(px(7.))
            .gap_1()
            .overflow_hidden()
            // measures the bar, so the next frame knows whether the chips fit
            .child(
                canvas(
                    move |bounds, window, _| {
                        if width.get() != bounds.size.width {
                            width.set(bounds.size.width);
                            window.refresh();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .w(px(260.))
                    .min_w(px(140.))
                    .flex_shrink(1.)
                    .child(text),
            )
            .when(!narrow, |bar| {
                bar.child(branch).child(user).child(date).child(paths)
            })
            .when(narrow, |bar| bar.child(more))
            // shortcut: Vcs.Log.Toolbar.Internal's Open New Git Log Tab (⌥G) goes here once the
            // editor holds more than one log, each with its own filters
            .child(div().flex_1())
            .when(self.filtering(cx), |bar| {
                bar.child(div().text_xs().text_color(muted).px_1().child(
                    match self.commit_matches {
                        1 => "1 commit".to_owned(),
                        n => format!("{n} commits"),
                    },
                ))
            })
            .child(
                h_flex()
                    .id("log-right-corner")
                    .role(Role::Toolbar)
                    .aria_label("VCS Log Toolbar")
                    .gap_1()
                    .when(self.log_ui.indexed == Some(false), |bar| {
                        bar.child(
                            icon_button(
                                "f-index",
                                "progress/resume",
                                "Enable Git Log Indexing",
                                "Enable Git Log Indexing",
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| this.index_log(window, cx)),
                            ),
                        )
                    })
                    .child(
                        icon_button("f-refresh", "general/refresh", "Refresh", "Refresh (⌘R)")
                            .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                    )
                    .child(
                        icon_button(
                            "f-cherry-pick",
                            "dvcs/cherryPick",
                            "Cherry-Pick",
                            "Cherry-Pick",
                        )
                        .disabled(selected.is_none())
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                let Some(id) = selected.clone() else { return };
                                let short: String = id.chars().take(7).collect();
                                this.run_outcome(
                                    window,
                                    cx,
                                    format!("Cherry-picked {short}"),
                                    move |root| spork_git::cherry_pick(root, &id),
                                )
                            },
                        )),
                    )
                    .child(view_options)
                    .child(
                        icon_button(
                            "f-goto",
                            "general/search",
                            "Go To Hash/Branch/Tag",
                            "Go To Hash/Branch/Tag (⌘F)",
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.log_go_to_ref_popup(window, cx)),
                        ),
                    ),
            )
    }

    /// A filter chip: its name with ▾ while off; "Name: value ×" while on, × turning it off.
    #[allow(clippy::too_many_arguments)]
    fn filter_chip(
        &self,
        id: &'static str,
        name: &'static str,
        value: Option<String>,
        menu: Menu,
        cx: &mut Context<Self>,
        clear: fn(&mut RepoView, &mut Window, &mut Context<RepoView>),
    ) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let chip = Button::new(id).ghost().small().accessibility_label(name);
        let chip = match value {
            None => chip
                .child(div().text_color(muted).child(name))
                .child(Icon::new(IconName::ChevronDown).xsmall().text_color(muted)),
            Some(value) => chip
                .child(div().text_color(muted).child(format!("{name}:")))
                .child(div().max_w(px(180.)).truncate().child(value))
                .child(
                    Button::new(SharedString::from(format!("{id}-clear")))
                        .ghost()
                        .xsmall()
                        .icon(spork_workbench::icon("general/closeSmall"))
                        .tooltip("Clear the filter")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            clear(this, window, cx)
                        })),
                ),
        };
        chip.dropdown_menu(move |m, window, cx| menu(m, window, cx))
    }

    /// The `n` authors with the most commits here, most first.
    fn top_authors(&self, n: usize) -> Vec<String> {
        let Some(Ok(repo)) = &self.repo else {
            return Vec::new();
        };
        let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for c in &repo.history.commits {
            *counts.entry(c.author.as_str()).or_default() += 1;
        }
        let mut authors: Vec<(&str, usize)> = counts.into_iter().collect();
        authors.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        authors
            .into_iter()
            .take(n)
            .map(|(a, _)| a.to_owned())
            .collect()
    }

    /// The Branch filter: HEAD (`None`), or only `branch`.
    pub(crate) fn show_only_branch(
        &mut self,
        branch: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let only = match &self.repo {
            Some(Ok(repo)) => repo.only.clone(),
            _ => Vec::new(),
        };
        if self.filters.current_branch != branch.is_none() {
            self.toggle_filter(Toggle::CurrentBranch, cx);
        }
        self.run_op(
            window,
            cx,
            move |root| {
                only.iter().try_for_each(|name| {
                    spork_git::config::remove_local(root, "spork.only", name)
                })?;
                match &branch {
                    Some(name) => spork_git::config::add_local(root, "spork.only", name),
                    None => Ok(()),
                }
            },
            |_, (), _, _| {},
        );
    }

    fn clear_branch_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.filters.current_branch {
            self.toggle_filter(Toggle::CurrentBranch, cx);
        }
        let only = match &self.repo {
            Some(Ok(repo)) => repo.only.clone(),
            _ => Vec::new(),
        };
        if !only.is_empty() {
            self.run_op(
                window,
                cx,
                move |root| {
                    only.iter().try_for_each(|name| {
                        spork_git::config::remove_local(root, "spork.only", name)
                    })
                },
                |_, (), _, _| {},
            );
        }
    }

    /// Paths › Select…: the path whose commits to keep.
    fn path_filter_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("A folder or file, e.g. src/ or README.md")
        });
        let current = self.log_filters.path.clone().unwrap_or_default();
        input.update(cx, |s, cx| s.set_value(current, window, cx));
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let input = input.clone();
            let view = view.clone();
            dialog
                .title("Filter by Path")
                .child(
                    v_flex()
                        .gap_2()
                        .child(spork_workbench::TextField::new(&input)),
                )
                .confirm()
                .ok_text("Filter")
                .on_ok(move |_, _, cx| {
                    let path = input.read(cx).value().trim().to_owned();
                    view.update(cx, |this, cx| {
                        this.set_path_filter((!path.is_empty()).then_some(path), cx)
                    });
                    true
                })
        });
    }

    /// The text field's magnifier: what it searches (messages, text in changes, file paths).
    fn search_mode_button(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let view = cx.entity();
        Button::new("search-mode")
            .ghost()
            .xsmall()
            .child(
                spork_workbench::icon("general/searchWithHistory")
                    .w(px(20.))
                    .h(px(16.)),
            )
            .tooltip(match self.search_in {
                None => "Searching messages, authors and hashes",
                Some(SearchIn::Changes) => "Searching text in the changes",
                Some(SearchIn::Paths) => "Searching file paths",
            })
            .dropdown_menu(move |menu, _, _| {
                let pick = |label: &str, placeholder: &'static str, mode: Option<SearchIn>| {
                    let view = view.clone();
                    PopupMenuItem::new(label.to_owned()).on_click(move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.search_in = mode;
                            this.commit_filter
                                .update(cx, |s, cx| s.set_placeholder(placeholder, window, cx));
                            match mode {
                                Some(within) => this.deep_search(within, cx),
                                None => {
                                    this.search_hits = None;
                                    this.jump_to_match(0, cx)
                                }
                            }
                        })
                    })
                };
                menu.item(pick("Messages, Authors and Hashes", "Text or hash", None))
                    .item(pick(
                        "Text in Changes",
                        "Text in changes",
                        Some(SearchIn::Changes),
                    ))
                    .item(pick("File Paths", "File path", Some(SearchIn::Paths)))
            })
    }
}

/// The Go To Hash/Branch/Tag popup's label (VcsLogBundle `vcs.log.go.to.hash.popup.label`), its
/// field's name too.
const GO_TO_LABEL: &str = "Enter hash or branch/tag name:";

/// Go To Hash/Branch/Tag (⌘F): "Enter hash or branch/tag name:", the refs that start with what
/// is typed listed under it; ⏎ selects the commit in the log.
pub(crate) struct GoToRefPopup {
    view: WeakEntity<RepoView>,
    input: Entity<InputState>,
    /// The branch and tag names, local branches first.
    refs: Vec<String>,
    _subscription: Subscription,
}

impl GoToRefPopup {
    fn new(
        view: WeakEntity<RepoView>,
        refs: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx));
        // ⏎ is the dialog's Confirm (`log_go_to_ref_popup`)
        let subscription = cx.subscribe(&input, |_, _, event, cx| {
            if let InputEvent::Change = event {
                cx.notify()
            }
        });
        Self {
            view,
            input,
            refs,
            _subscription: subscription,
        }
    }

    fn go(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }
        window.close_dialog(cx);
        self.view
            .update(cx, |this, cx| this.log_go_to(&text, window, cx))
            .log_err();
    }
}

impl Render for GoToRefPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typed = self.input.read(cx).value().trim().to_owned();
        let hover = cx.theme().list_hover;
        let matches: Vec<String> = match typed.is_empty() {
            true => Vec::new(),
            false => self
                .refs
                .iter()
                .filter(|name| name.starts_with(&typed) && **name != typed)
                .take(10)
                .cloned()
                .collect(),
        };
        v_flex()
            .id("go-to-ref")
            .role(Role::Group)
            .aria_label("Go To Hash/Branch/Tag")
            .gap_1()
            .child(div().child(GO_TO_LABEL))
            .child(spork_workbench::TextField::new(&self.input).aria_label(GO_TO_LABEL))
            .children(matches.into_iter().enumerate().map(|(ix, name)| {
                div()
                    .id(("go-to-ref-match", ix))
                    .role(Role::ListItem)
                    .aria_label(name.clone())
                    .px_2()
                    .py_0p5()
                    .rounded(px(4.))
                    .hover(move |row| row.bg(hover))
                    .child(name.clone())
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.go(name.clone(), window, cx)),
                    )
            }))
    }
}

impl RepoView {
    /// Go To Hash/Branch/Tag (⌘F, the log toolbar's magnifier).
    pub(crate) fn log_go_to_ref_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let mut refs: Vec<&spork_git::Ref> = repo.refs.iter().collect();
        refs.sort_by_key(|r| (r.kind != RefKind::Local, r.kind == RefKind::Tag));
        let mut names: Vec<String> = refs.into_iter().map(|r| r.name.clone()).collect();
        names.insert(0, "HEAD".to_owned());
        self.view = crate::View::History;
        let view = cx.entity().downgrade();
        let popup = cx.new(|cx| GoToRefPopup::new(view.clone(), names, window, cx));
        let input = popup.read(cx).input.clone();
        window.open_dialog(cx, {
            let input = input.clone();
            move |dialog, _, _| {
                let (input, view) = (input.clone(), view.clone());
                dialog
                    .w(px(420.))
                    .close_button(false)
                    .child(popup.clone())
                    .on_ok(move |_, window, cx| {
                        let text = input.read(cx).value().trim().to_owned();
                        if !text.is_empty() {
                            view.update(cx, |this, cx| this.log_go_to(&text, window, cx))
                                .log_err();
                        }
                        true
                    })
            }
        });
        input.update(cx, |s, cx| s.focus(window, cx));
    }

    /// Selects the commit `text` names: a branch, a tag, HEAD, or a hash (or its start).
    fn log_go_to(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let by_ref = match text {
            "HEAD" => repo
                .refs
                .iter()
                .find(|r| r.is_head && r.kind == RefKind::Local),
            _ => repo.refs.iter().find(|r| r.name == text),
        }
        .map(|r| r.commit.clone());
        let hash_like = text.len() >= 4 && text.chars().all(|c| c.is_ascii_hexdigit());
        let by_hash = hash_like
            .then(|| {
                let text = text.to_ascii_lowercase();
                repo.history
                    .commits
                    .iter()
                    .find(|c| c.id.starts_with(&text))
                    .map(|c| c.id.clone())
            })
            .flatten();
        match by_ref.or(by_hash) {
            Some(commit) => self.reveal_commit(&commit, window, cx),
            None => {
                // IntelliJ's VcsLogBundle: vcs.log.commit.prefix / commit.or.reference.prefix
                let what = match hash_like {
                    true => format!("Commit '{text}'"),
                    false => format!("Commit or reference '{text}'"),
                };
                window.push_notification(Notification::warning(format!("{what} not found")), cx)
            }
        }
    }
}

/// A menu item that turns a preference on or off, saved at once.
pub(crate) fn pref_item(
    view: &Entity<RepoView>,
    label: &str,
    on: bool,
    flip: fn(&mut crate::prefs::Prefs),
) -> PopupMenuItem {
    let view = view.clone();
    PopupMenuItem::new(label.to_owned())
        .checked(on)
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                flip(&mut this.prefs);
                this.prefs.save();
                cx.notify();
            })
        })
}
