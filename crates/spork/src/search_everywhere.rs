//! Search Everywhere, as in IntelliJ / the reference app: a popup centered in the window with its tabs
//! (All, Files, Actions, Text), "Include non-project items", the search field ("Type / to see
//! commands"), the results and, under them, the selected file's path.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::component::input::{Escape, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_git::file::Match;
use spork_workbench::{Tokens, Typography as _, popup};

use crate::RepoView;
use crate::repos::{display_name, tilde};

/// IntelliJ's size for it, and how many results a group shows in All.
const WIDTH: f32 = 696.;
const HEIGHT: f32 = 596.;
const PER_GROUP: usize = 8;
const MAX_RESULTS: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tab {
    All,
    Files,
    Actions,
    Text,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::All, Tab::Files, Tab::Actions, Tab::Text];

    fn title(self) -> &'static str {
        match self {
            Tab::All => "All",
            Tab::Files => "Files",
            Tab::Actions => "Actions",
            Tab::Text => "Text",
        }
    }
}

/// What the popup searches, gathered when it opens.
pub(crate) struct Sources {
    pub(crate) root: PathBuf,
    /// New files git doesn't ignore.
    pub(crate) untracked: Vec<String>,
    pub(crate) recent_files: Vec<String>,
    pub(crate) branches: Vec<(String, String)>,
    pub(crate) projects: Vec<PathBuf>,
}

#[derive(Clone)]
enum Found {
    Group(&'static str),
    Command(Tab),
    File(String),
    Action(usize),
    Branch(String, String),
    Project(PathBuf),
    Text(Match),
}

pub(crate) struct SearchEverywhere {
    view: WeakEntity<RepoView>,
    search: Entity<InputState>,
    tab: Tab,
    /// Ignored files too, as IntelliJ's non-project items.
    non_project: bool,
    sources: Sources,
    files: Vec<String>,
    ignored: Vec<String>,
    actions: Vec<(SharedString, Box<dyn Action>)>,
    text: Vec<Match>,
    found: Vec<Found>,
    selected: usize,
    scroll: UniformListScrollHandle,
    _load: Task<()>,
    _grep: Option<Task<()>>,
    _subscription: Subscription,
}

impl EventEmitter<DismissEvent> for SearchEverywhere {}

impl SearchEverywhere {
    pub(crate) fn new(
        view: WeakEntity<RepoView>,
        sources: Sources,
        tab: Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx));
        search.update(cx, |search, cx| search.focus(window, cx));
        let subscription =
            cx.subscribe_in(&search, window, |this, _, event, window, cx| match event {
                InputEvent::Change => this.refresh(cx),
                InputEvent::PressEnter { .. } => this.open(this.selected, window, cx),
                _ => {}
            });
        // the work tree's files: committed, then new ones git doesn't ignore
        let (root, untracked) = (sources.root.clone(), sources.untracked.clone());
        let load = cx.spawn(async move |this, cx| {
            let (files, ignored) = cx
                .background_spawn(async move {
                    let mut files = spork_git::file::tree(&root, "HEAD").unwrap_or_default();
                    files.extend(untracked);
                    (files, spork_git::ignored_paths(&root))
                })
                .await;
            this.update(cx, |this, cx| {
                this.files = files;
                this.ignored = ignored;
                this.refresh(cx);
            })
            .ok();
        });
        let mut this = Self {
            view,
            search,
            tab,
            non_project: false,
            sources,
            files: Vec::new(),
            ignored: Vec::new(),
            actions: crate::shell::commands(),
            text: Vec::new(),
            found: Vec::new(),
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            _load: load,
            _grep: None,
            _subscription: subscription,
        };
        this.refresh(cx);
        this
    }

    fn query(&self, cx: &App) -> String {
        self.search.read(cx).value().trim().to_lowercase()
    }

    fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.refresh(cx);
    }

    /// The results for the query in this tab; Text searches in the background.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let query = self.query(cx);
        let mut found = Vec::new();
        if let Some(command) = query.strip_prefix('/') {
            // IntelliJ's commands: a tab by its name
            found.extend(
                Tab::ALL
                    .into_iter()
                    .filter(|t| t.title().to_lowercase().starts_with(command))
                    .map(Found::Command),
            );
        } else {
            let all = self.tab == Tab::All;
            let limit = if all { PER_GROUP } else { MAX_RESULTS };
            if all || self.tab == Tab::Files {
                let files = self.files_matching(&query, limit);
                if !files.is_empty() && all && !query.is_empty() {
                    found.push(Found::Group("Files"));
                }
                found.extend(files.into_iter().map(Found::File));
            }
            if (all && !query.is_empty()) || self.tab == Tab::Actions {
                let mut actions: Vec<usize> = (0..self.actions.len())
                    .filter(|ix| self.actions[*ix].0.to_lowercase().contains(&query))
                    .collect();
                // as IntelliJ ranks them: the action named so first, then those it starts
                actions.sort_by_cached_key(|ix| {
                    let name = self.actions[*ix].0.to_lowercase();
                    (name != query, !name.starts_with(&query))
                });
                actions.truncate(limit);
                if !actions.is_empty() && all {
                    found.push(Found::Group("Actions"));
                }
                found.extend(actions.into_iter().map(Found::Action));
            }
            if all && !query.is_empty() {
                // Spork's own: the branches (IntelliJ's Git results) and the repositories
                let branches: Vec<Found> = self
                    .sources
                    .branches
                    .iter()
                    .filter(|(name, _)| name.to_lowercase().contains(&query))
                    .take(limit)
                    .map(|(name, commit)| Found::Branch(name.clone(), commit.clone()))
                    .collect();
                if !branches.is_empty() {
                    found.push(Found::Group("Git"));
                    found.extend(branches);
                }
                let projects: Vec<Found> = self
                    .sources
                    .projects
                    .iter()
                    .filter(|p| tilde(p).to_lowercase().contains(&query))
                    .take(limit)
                    .map(|p| Found::Project(p.clone()))
                    .collect();
                if !projects.is_empty() {
                    found.push(Found::Group("Projects"));
                    found.extend(projects);
                }
            }
            if self.tab == Tab::Text {
                self.grep(query, cx);
                found.extend(self.text.iter().cloned().map(Found::Text));
            }
        }
        self.found = found;
        self.selected = self
            .found
            .iter()
            .position(|f| !matches!(f, Found::Group(_)))
            .unwrap_or(0);
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Top);
        cx.notify();
    }

    /// Files whose name has the query, then those whose path has it; with no query the files
    /// opened lately.
    fn files_matching(&self, query: &str, limit: usize) -> Vec<String> {
        if query.is_empty() {
            return self
                .sources
                .recent_files
                .iter()
                .take(limit)
                .cloned()
                .collect();
        }
        let ignored = self.ignored.iter().filter(|_| self.non_project);
        let all: Vec<&String> = self.files.iter().chain(ignored).collect();
        let name = |p: &str| p.rsplit('/').next().unwrap_or(p).to_lowercase();
        let mut by_name: Vec<&String> = all
            .iter()
            .copied()
            .filter(|p| name(p).contains(query))
            .collect();
        // a name that starts with the query first, then the shorter paths
        by_name.sort_by_key(|p| (!name(p).starts_with(query), p.len()));
        let by_path = all
            .iter()
            .copied()
            .filter(|p| !name(p).contains(query) && p.to_lowercase().contains(query));
        by_name
            .into_iter()
            .chain(by_path)
            .take(limit)
            .cloned()
            .collect()
    }

    /// Text: `git grep` once the typing pauses.
    fn grep(&mut self, query: String, cx: &mut Context<Self>) {
        if query.chars().count() < 2 {
            self.text.clear();
            self._grep = None;
            return;
        }
        let root = self.sources.root.clone();
        self._grep = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let found = cx
                .background_spawn(async move { spork_git::file::grep(&root, &query, MAX_RESULTS) })
                .await
                .unwrap_or_default();
            this.update(cx, |this, cx| {
                this.text = found;
                this.found = this.text.iter().cloned().map(Found::Text).collect();
                this.selected = 0;
                cx.notify();
            })
            .ok();
        }));
    }

    fn select_next(&mut self, step: isize, cx: &mut Context<Self>) {
        let count = self.found.len() as isize;
        if count == 0 {
            return;
        }
        let mut ix = self.selected as isize;
        for _ in 0..count {
            ix = (ix + step).rem_euclid(count);
            if !matches!(self.found[ix as usize], Found::Group(_)) {
                break;
            }
        }
        self.selected = ix as usize;
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn open(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(found) = self.found.get(ix).cloned() else {
            return;
        };
        if let Found::Command(tab) = found {
            self.search
                .update(cx, |search, cx| search.set_value("", window, cx));
            return self.set_tab(tab, cx);
        }
        if matches!(found, Found::Group(_)) {
            return;
        }
        cx.emit(DismissEvent);
        if let Found::Action(ix) = found {
            if let Some((_, action)) = self.actions.get(ix) {
                window.dispatch_action(action.boxed_clone(), cx);
            }
            return;
        }
        let Some(view) = self.view.upgrade() else {
            return;
        };
        view.update(cx, |this, cx| match found {
            Found::File(path) | Found::Text(Match { path, .. }) => this.open_file(path, false, cx),
            Found::Branch(_, commit) => this.reveal_commit(&commit, window, cx),
            Found::Project(path) => {
                crate::open_repo_window(vec![path], None, cx);
            }
            Found::Group(_) | Found::Command(_) | Found::Action(_) => {}
        });
    }

    fn render_rows(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let tokens = Tokens::get(cx);
        let project = display_name(&self.sources.root);
        range
            .filter_map(|ix| {
                let found = self.found.get(ix)?;
                if let Found::Group(title) = found {
                    return Some(
                        div()
                            .w_full()
                            .h(px(24.))
                            .flex()
                            .items_center()
                            .px(px(16.))
                            .text_ui_small()
                            .text_color(tokens.text_secondary)
                            .child(*title)
                            .into_any_element(),
                    );
                }
                let selected = ix == self.selected;
                let secondary = |text: String| {
                    div()
                        .flex_none()
                        .text_color(match selected {
                            true => tokens.text,
                            false => tokens.text_secondary,
                        })
                        .child(text)
                };
                let row = popup::row(("found", ix), selected, cx);
                let row = match found {
                    Found::Group(_) => row,
                    Found::Command(tab) => row
                        .child(div().child(format!("/{}", tab.title().to_lowercase())))
                        .child(secondary(format!("Switch to {}", tab.title()))),
                    Found::File(path) => {
                        let (dir, name) = path.rsplit_once('/').unwrap_or((".", path));
                        row.child(spork_workbench::image(spork_workbench::file_icon(name), cx))
                            .child(div().flex_none().child(name.to_owned()))
                            .child(secondary(dir.to_owned()).min_w_0().truncate())
                            .child(div().flex_1())
                            .child(secondary(project.clone()))
                            .child(spork_workbench::image("nodes/ideaProject", cx).size(px(16.)))
                    }
                    Found::Action(action) => row.child(div().size(px(16.)).flex_none()).child(
                        div()
                            .flex_1()
                            .truncate()
                            .child(self.actions[*action].0.clone()),
                    ),
                    Found::Branch(name, _) => row
                        .child(
                            spork_workbench::icon("general/vcs")
                                .size(px(16.))
                                .text_color(tokens.icon),
                        )
                        .child(div().flex_1().truncate().child(name.clone())),
                    Found::Project(path) => row
                        .child(spork_workbench::image("nodes/ideaProject", cx).size(px(16.)))
                        .child(div().flex_none().child(display_name(path)))
                        .child(secondary(tilde(path)).min_w_0().truncate()),
                    Found::Text(found) => row
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(found.text.trim().to_owned()),
                        )
                        .child(secondary(format!(
                            "{} {}",
                            found.path.rsplit('/').next().unwrap_or(&found.path),
                            found.line
                        ))),
                };
                Some(
                    div()
                        .w_full()
                        .child(
                            row.on_mouse_move(cx.listener(move |this, _, _, cx| {
                                if this.selected != ix {
                                    this.selected = ix;
                                    cx.notify();
                                }
                            }))
                            .on_click(
                                cx.listener(move |this, _, window, cx| this.open(ix, window, cx)),
                            ),
                        )
                        .into_any_element(),
                )
            })
            .collect()
    }
}

impl Render for SearchEverywhere {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let tabs = Tab::ALL.into_iter().map(|tab| {
            let on = tab == self.tab;
            div()
                .id(tab.title())
                .h(px(28.))
                .px(px(8.))
                .flex()
                .items_center()
                .rounded(px(6.))
                .border_1()
                .cursor_pointer()
                .map(|t| match on {
                    true => t
                        .bg(tokens.tab_selected)
                        .border_color(tokens.tab_selected_border),
                    false => t
                        .border_color(transparent_black())
                        .hover(move |t| t.bg(tokens.hover)),
                })
                .child(tab.title())
                .on_click(cx.listener(move |this, _, _, cx| this.set_tab(tab, cx)))
        });
        let non_project = self.non_project;
        let header = h_flex()
            .h(px(40.))
            .flex_none()
            .pl(px(8.))
            .pr(px(12.))
            .gap(px(4.))
            .children(tabs)
            .child(div().flex_1())
            .when(matches!(self.tab, Tab::All | Tab::Files), |header| {
                header.child(
                    spork_workbench::controls::labeled_check(
                        "non-project",
                        "Include non-project items",
                        non_project,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.non_project = !this.non_project;
                        this.refresh(cx);
                    })),
                )
            });
        let field = div().px(px(8.)).pb(px(8.)).flex_none().child(
            spork_workbench::TextField::new(&self.search)
                .prefix(spork_workbench::icon("general/searchWithHistory").size(px(16.)))
                .suffix(
                    div()
                        .text_color(tokens.text_secondary)
                        .child("Type / to see commands"),
                ),
        );
        let selected_path = match self.found.get(self.selected) {
            Some(Found::File(path)) => Some(tilde(&self.sources.root.join(path))),
            Some(Found::Text(found)) => Some(format!(
                "{}:{}",
                tilde(&self.sources.root.join(&found.path)),
                found.line
            )),
            _ => None,
        };
        let empty = self.found.is_empty() && !self.query(cx).is_empty();
        let list = uniform_list(
            "search-everywhere",
            self.found.len(),
            cx.processor(|this, range, _, cx| this.render_rows(range, cx)),
        )
        .track_scroll(&self.scroll)
        .flex_1()
        .min_h_0();
        popup::surface(cx)
            .id("search-everywhere")
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .key_context("SearchEverywhere")
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                cx.stop_propagation();
                this.select_next(-1, cx)
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                cx.stop_propagation();
                this.select_next(1, cx)
            }))
            .capture_action(cx.listener(|_, _: &Escape, _, cx| {
                cx.stop_propagation();
                cx.emit(DismissEvent)
            }))
            // Tab and ⇧Tab go through the tabs, as in IntelliJ
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key != "tab" {
                    return;
                }
                cx.stop_propagation();
                let ix = Tab::ALL.iter().position(|t| *t == this.tab).unwrap_or(0) as isize;
                let step = if event.keystroke.modifiers.shift {
                    -1
                } else {
                    1
                };
                let next = Tab::ALL[(ix + step).rem_euclid(Tab::ALL.len() as isize) as usize];
                this.set_tab(next, cx);
            }))
            .on_mouse_down_out(cx.listener(|_, _, _, cx| cx.emit(DismissEvent)))
            .child(header)
            .child(field)
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .when(empty, |list| {
                        list.child(
                            div()
                                .pt(px(40.))
                                .flex()
                                .justify_center()
                                .text_color(tokens.text_secondary)
                                .child("Nothing found"),
                        )
                    })
                    .child(list),
            )
            .children(selected_path.map(|path| {
                h_flex()
                    .flex_none()
                    .px(px(12.))
                    .py(px(8.))
                    .border_t_1()
                    .border_color(tokens.popup_border)
                    .text_ui_small()
                    .text_color(tokens.text_secondary)
                    .child(div().truncate().child(path))
            }))
    }
}

impl RepoView {
    /// Opens Search Everywhere on `tab`, or closes it.
    pub(crate) fn toggle_search_everywhere(
        &mut self,
        tab: Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.search_everywhere.take().is_some() {
            return cx.notify();
        }
        let Some(Ok(repo)) = &self.repo else { return };
        let mut projects = self.recent.clone();
        projects.extend(
            self.scanned
                .iter()
                .filter(|p| !self.recent.contains(p))
                .cloned(),
        );
        let sources = Sources {
            root: repo.root.clone(),
            untracked: repo
                .status
                .iter()
                .filter(|e| e.untracked())
                .map(|e| e.path.clone())
                .collect(),
            recent_files: self.recent_files.clone(),
            branches: repo
                .refs
                .iter()
                .filter(|r| r.kind != spork_git::RefKind::Tag)
                .map(|r| (r.name.clone(), r.commit.clone()))
                .collect(),
            projects,
        };
        let view = cx.entity().downgrade();
        let popup = cx.new(|cx| SearchEverywhere::new(view, sources, tab, window, cx));
        let subscription = cx.subscribe(&popup, |this, _, _: &DismissEvent, cx| {
            this.search_everywhere = None;
            cx.notify();
        });
        self.search_everywhere = Some((popup, subscription));
        cx.notify();
    }

    /// Search Everywhere over the window's middle, as IntelliJ first places it.
    pub(crate) fn search_everywhere_overlay(
        &self,
        window: &Window,
    ) -> Option<impl IntoElement + use<>> {
        let (popup, _) = self.search_everywhere.as_ref()?;
        let viewport = window.viewport_size();
        let at = point(
            ((viewport.width - px(WIDTH)) / 2.).max(px(8.)),
            ((viewport.height - px(HEIGHT)) / 2.).max(px(8.)),
        );
        Some(
            deferred(
                anchored()
                    .position(at)
                    .snap_to_window_with_margin(px(8.))
                    .child(popup.clone()),
            )
            .with_priority(1),
        )
    }
}
