//! Search Everywhere (IntelliJ's `SearchEverywhereUI`, in the reference app's look): a popup over the
//! window's middle with its tabs (All, Classes, Files, Symbols, Actions, Text), the search field
//! and its options, the results under their group's name with their icons and locations, "Include
//! non-project items", and the preview under the results. The standalone app's Search Everywhere
//! (crates/spork/src/search_everywhere.rs) is its model: the same sizes, header and rows.
//!
//! Classes are the language servers' types (classes, structs, enums, interfaces and traits),
//! Symbols all their workspace symbols, Actions the actions the focused element can run (the
//! command palette's), each with its menu path, shortcut and, for a toggle, its state.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use command_palette_hooks::CommandPaletteFilter;
use editor::{Editor, SelectionEffects, scroll::Autoscroll};
use fuzzy_nucleo::{MaxResults, PathMatchOptions, StringMatchCandidate, StringMatchOptions};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, AnyElement, App, AppContext as _, Context, DismissEvent, Entity, EventEmitter,
    FocusHandle, Focusable, InteractiveElement as _, IntoElement, KeyDownEvent, OwnedMenuItem,
    ParentElement as _, Render, Role, ScrollStrategy, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, TaskExt as _,
    UniformListScrollHandle, WeakEntity, Window, div, px, uniform_list,
};
use gpui_kit::component::input::{
    Escape, InputEvent, InputState, MoveDown, MovePageDown, MovePageUp, MoveToEnd, MoveToStart,
    MoveUp,
};
use gpui_kit::component::{Selectable as _, h_flex, v_flex};
use language::{Bias, SymbolKind, ToOffset as _};
use picker::{MatchLocation, PreviewBackend, PreviewLayout, PreviewUpdate};
use project::lsp_store::SymbolLocation;
use project::search::SearchQuery;
use project::{Project, ProjectPath, Symbol, WorktreeId};
use spork_workbench::{ActionButton, Tokens, Typography as _, popup};
use util::ResultExt as _;
use util::paths::PathMatcher;
use workspace::{ModalView, Workspace};

use crate::navigate;

/// IntelliJ's size for it, and how many results a group shows in All.
const WIDTH: f32 = 696.;
const HEIGHT: f32 = 596.;
const PREVIEW_HEIGHT: f32 = 260.;
const PER_GROUP: usize = 8;
const MAX_RESULTS: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tab {
    All,
    Classes,
    Files,
    Symbols,
    Actions,
    /// The project's HTTP endpoints (IntelliJ's `UrlSearchEverywhereContributor`), in projects
    /// that have them.
    Endpoints,
    Text,
}

impl Tab {
    const ALL: [Tab; 7] = [
        Tab::All,
        Tab::Classes,
        Tab::Files,
        Tab::Symbols,
        Tab::Actions,
        Tab::Endpoints,
        Tab::Text,
    ];

    fn title(self) -> &'static str {
        match self {
            Tab::All => "All",
            Tab::Classes => "Classes",
            Tab::Files => "Files",
            Tab::Symbols => "Symbols",
            Tab::Actions => "Actions",
            Tab::Endpoints => "Endpoints",
            Tab::Text => "Text",
        }
    }

    /// Whether IntelliJ shows its preview button: the tabs whose results are places in files.
    fn previews(self) -> bool {
        !matches!(self, Tab::Actions)
    }
}

/// An HTTP endpoint as the Endpoints tab lists it: its URL, methods and handler, and how to go
/// to it.
pub struct EndpointItem {
    pub url: SharedString,
    pub methods: SharedString,
    pub location: SharedString,
    pub open: Rc<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>,
}

/// The endpoints of a workspace's project, `None` in a project without HTTP endpoints (no tab).
type EndpointsProvider = Rc<dyn Fn(&Workspace, &App) -> Option<Vec<EndpointItem>>>;

struct Endpoints(EndpointsProvider);

impl gpui::Global for Endpoints {}

/// Gives Search Everywhere its Endpoints tab, the project's endpoints `provider` lists.
pub fn register_endpoints(
    provider: impl Fn(&Workspace, &App) -> Option<Vec<EndpointItem>> + 'static,
    cx: &mut App,
) {
    cx.set_global(Endpoints(Rc::new(provider)));
}

fn endpoints(workspace: &Workspace, cx: &App) -> Option<Vec<EndpointItem>> {
    let provider = cx.try_global::<Endpoints>()?.0.clone();
    provider(workspace, cx)
}

pub(crate) fn register(workspace: &mut Workspace) {
    // Go to Endpoint… is there in a project with endpoints, as IntelliJ's
    workspace.register_action_renderer(|div, workspace, _, cx| {
        div.when(endpoints(workspace, cx).is_some(), |div| {
            div.on_action(
                cx.listener(|workspace, _: &navigate::GoToEndpoint, window, cx| {
                    toggle(workspace, Tab::Endpoints, window, cx)
                }),
            )
        })
    });
    workspace.register_action(|workspace, _: &navigate::SearchEverywhere, window, cx| {
        toggle(workspace, Tab::All, window, cx)
    });
    workspace.register_action(|workspace, _: &navigate::GoToClass, window, cx| {
        toggle(workspace, Tab::Classes, window, cx)
    });
    workspace.register_action(|workspace, _: &navigate::GoToFile, window, cx| {
        toggle(workspace, Tab::Files, window, cx)
    });
    workspace.register_action(|workspace, _: &navigate::GoToSymbol, window, cx| {
        toggle(workspace, Tab::Symbols, window, cx)
    });
    workspace.register_action(|workspace, _: &navigate::GoToText, window, cx| {
        toggle(workspace, Tab::Text, window, cx)
    });
    workspace.register_action(|workspace, _: &navigate::FindAction, window, cx| {
        toggle(workspace, Tab::Actions, window, cx)
    });
}

/// Opens Search Everywhere on `tab`. While it shows, another tab's action switches to that tab,
/// and the shown tab's own action includes the non-project items, as in IntelliJ.
fn toggle(workspace: &mut Workspace, tab: Tab, window: &mut Window, cx: &mut Context<Workspace>) {
    if let Some(popup) = workspace.active_modal::<SearchEverywhere>(cx) {
        popup.update(cx, |popup, cx| match popup.tab == tab {
            true => popup.toggle_non_project(window, cx),
            false => popup.set_tab(tab, window, cx),
        });
        return;
    }
    let previous_focus = window.focused(cx);
    crate::after_update(window, cx, move |workspace, window, cx| {
        let actions = actions(&workspace, window, cx);
        workspace.update(cx, |workspace, cx| {
            let recent = crate::recent_files(workspace, cx);
            let endpoints = endpoints(workspace, cx);
            let project = workspace.project().clone();
            let handle = cx.entity().downgrade();
            workspace.toggle_modal(window, cx, move |window, cx| {
                SearchEverywhere::new(
                    handle,
                    project,
                    tab,
                    recent,
                    actions,
                    endpoints,
                    previous_focus,
                    window,
                    cx,
                )
            });
        });
    });
}

/// An action as the Actions tab lists it.
struct ActionItem {
    /// Its menu item's text, or its name in words.
    text: SharedString,
    /// Where IntelliJ shows it: its menu path (`View | Tool Windows`), or its group's name.
    location: SharedString,
    shortcut: Option<SharedString>,
    action: Box<dyn Action>,
    enabled: bool,
    /// A toggle's state, for the toggles whose state shows somewhere.
    toggled: Option<bool>,
}

#[derive(Clone)]
enum Found {
    Group(&'static str),
    Command(Tab),
    File(ProjectPath),
    Symbol(usize),
    Action(usize),
    Endpoint(usize),
    Text(usize),
}

pub(crate) struct SearchEverywhere {
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    search: Entity<InputState>,
    tab: Tab,
    /// IntelliJ's "Include non-project items": ignored files, symbols outside the project.
    non_project: bool,
    disabled_actions: bool,
    show_preview: bool,
    match_case: bool,
    words: bool,
    regex: bool,
    recent: Vec<ProjectPath>,
    files: Vec<ProjectPath>,
    symbols: Vec<Symbol>,
    actions: Vec<ActionItem>,
    action_matches: Vec<usize>,
    /// The project's endpoints, `None` without the Endpoints tab.
    endpoints: Option<Vec<EndpointItem>>,
    endpoint_matches: Vec<usize>,
    text: Vec<crate::find_in_files::Occurrence>,
    found: Vec<Found>,
    selected: usize,
    scroll: UniformListScrollHandle,
    preview: Arc<dyn PreviewBackend>,
    previous_focus: Option<FocusHandle>,
    file_cancel: Arc<AtomicBool>,
    _file_search: Task<()>,
    _symbol_search: Task<()>,
    _text_search: Task<()>,
    _subscription: Subscription,
}

impl EventEmitter<DismissEvent> for SearchEverywhere {}

impl ModalView for SearchEverywhere {}

impl Focusable for SearchEverywhere {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
}

impl SearchEverywhere {
    #[allow(clippy::too_many_arguments)]
    fn new(
        workspace: WeakEntity<Workspace>,
        project: Entity<Project>,
        tab: Tab,
        recent: Vec<ProjectPath>,
        actions: Vec<ActionItem>,
        endpoints: Option<Vec<EndpointItem>>,
        previous_focus: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx));
        let subscription =
            cx.subscribe_in(&search, window, |this, _, event, window, cx| match event {
                InputEvent::Change => this.refresh(window, cx),
                InputEvent::PressEnter { .. } => this.confirm(this.selected, window, cx),
                _ => {}
            });
        let preview = picker_preview::editor_preview(project.clone(), window, cx);
        let mut this = Self {
            workspace,
            project,
            search,
            tab,
            non_project: false,
            disabled_actions: false,
            show_preview: false,
            match_case: false,
            words: false,
            regex: false,
            recent,
            files: Vec::new(),
            symbols: Vec::new(),
            actions,
            action_matches: Vec::new(),
            endpoints,
            endpoint_matches: Vec::new(),
            text: Vec::new(),
            found: Vec::new(),
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            preview,
            previous_focus,
            file_cancel: Arc::new(AtomicBool::new(false)),
            _file_search: Task::ready(()),
            _symbol_search: Task::ready(()),
            _text_search: Task::ready(()),
            _subscription: subscription,
        };
        this.refresh(window, cx);
        this
    }

    /// The tabs shown: Endpoints in a project with endpoints only.
    fn tabs(&self) -> Vec<Tab> {
        Tab::ALL
            .into_iter()
            .filter(|tab| *tab != Tab::Endpoints || self.endpoints.is_some())
            .collect()
    }

    fn query(&self, cx: &App) -> String {
        self.search.read(cx).value().trim().to_owned()
    }

    /// The tab searched and what for: a command's (`/files main` searches Files for `main`, as
    /// IntelliJ's command keeps its contributor's results) or the shown tab's and the query.
    fn scope(&self, cx: &App) -> (Tab, String) {
        let value = self.search.read(cx).value();
        let command = value
            .trim_start()
            .strip_prefix('/')
            .and_then(|command| command.split_once(' '))
            .and_then(|(name, rest)| {
                let tab = self
                    .tabs()
                    .into_iter()
                    .find(|tab| tab.title().eq_ignore_ascii_case(name))?;
                Some((tab, rest.trim().to_owned()))
            });
        command.unwrap_or_else(|| (self.tab, self.query(cx)))
    }

    /// The commands `/typed` (no space yet) offers: the tabs whose name starts with it.
    fn commands(&self, query: &str) -> Option<Vec<Tab>> {
        let typed = query.strip_prefix('/')?;
        if typed.contains(' ') {
            return None;
        }
        let typed = typed.to_lowercase();
        Some(
            self.tabs()
                .into_iter()
                .filter(|tab| tab.title().to_lowercase().starts_with(&typed))
                .collect(),
        )
    }

    /// IntelliJ's Complete Command (⇥): the selected command, or the first one offered, written
    /// out with a space after it. With nothing to complete the key goes on (to Next Tab).
    fn complete_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().trim_start().to_owned();
        let Some(commands) = self.commands(&query) else {
            cx.propagate();
            return;
        };
        let selected = match self.found.get(self.selected) {
            Some(Found::Command(tab)) => Some(*tab),
            _ => None,
        };
        let Some(tab) = selected.or_else(|| commands.first().copied()) else {
            cx.propagate();
            return;
        };
        let text = format!("/{} ", tab.title().to_lowercase());
        self.search
            .update(cx, |search, cx| search.replace_all(text, window, cx));
    }

    fn set_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = tab;
        self.refresh(window, cx);
    }

    fn step_tab(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let tabs = self.tabs();
        let ix = tabs.iter().position(|tab| *tab == self.tab).unwrap_or(0) as isize;
        let next = (ix + step).rem_euclid(tabs.len() as isize) as usize;
        if let Some(tab) = tabs.get(next) {
            self.set_tab(*tab, window, cx);
        }
    }

    fn toggle_non_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.non_project = !self.non_project;
        self.refresh(window, cx);
    }

    /// Searches anew for the query: actions at once, files, symbols and text in the background.
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (tab, query) = self.scope(cx);
        let searching = self.commands(&query).is_none();
        self.match_actions(&query);
        self.match_endpoints(&query);
        let files = searching && matches!(tab, Tab::All | Tab::Files);
        self.search_files(
            if files { query.clone() } else { String::new() },
            window,
            cx,
        );
        let symbols = searching && matches!(tab, Tab::All | Tab::Classes | Tab::Symbols);
        self.search_symbols(
            if symbols {
                query.clone()
            } else {
                String::new()
            },
            window,
            cx,
        );
        let text = searching && tab == Tab::Text;
        self.search_text(if text { query } else { String::new() }, window, cx);
        self.rebuild(window, cx);
    }

    fn match_actions(&mut self, query: &str) {
        let candidates: Vec<StringMatchCandidate> = self
            .actions
            .iter()
            .enumerate()
            .filter(|(_, item)| item.enabled || self.disabled_actions)
            .map(|(ix, item)| StringMatchCandidate::new(ix, item.text.clone()))
            .collect();
        if query.is_empty() {
            self.action_matches = candidates.iter().map(|candidate| candidate.id).collect();
            return;
        }
        let mut matches = fuzzy_nucleo::match_strings(
            &candidates,
            query,
            StringMatchOptions {
                max_results: MaxResults::limit::<MAX_RESULTS>(),
                ..Default::default()
            },
        );
        // the enabled ones first, as IntelliJ lists them
        matches.sort_by_key(|found| {
            !self
                .actions
                .get(found.candidate_id)
                .is_some_and(|item| item.enabled)
        });
        self.action_matches = matches
            .into_iter()
            .map(|found| found.candidate_id)
            .collect();
    }

    /// The endpoints whose URL matches the query, as IntelliJ's URL search matches its paths.
    fn match_endpoints(&mut self, query: &str) {
        let Some(endpoints) = &self.endpoints else {
            return;
        };
        let candidates: Vec<StringMatchCandidate> = endpoints
            .iter()
            .enumerate()
            .map(|(ix, item)| StringMatchCandidate::new(ix, item.url.clone()))
            .collect();
        if query.is_empty() {
            self.endpoint_matches = candidates.iter().map(|candidate| candidate.id).collect();
            return;
        }
        self.endpoint_matches = fuzzy_nucleo::match_strings(
            &candidates,
            query,
            StringMatchOptions {
                max_results: MaxResults::limit::<MAX_RESULTS>(),
                ..Default::default()
            },
        )
        .into_iter()
        .map(|found| found.candidate_id)
        .collect();
    }

    fn search_files(&mut self, query: String, window: &mut Window, cx: &mut Context<Self>) {
        self.file_cancel.store(true, Ordering::Release);
        if query.is_empty() {
            self.files.clear();
            self._file_search = Task::ready(());
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.file_cancel = cancel.clone();
        let worktrees: Vec<_> = self.project.read(cx).visible_worktrees(cx).collect();
        let include_root_name = worktrees.len() > 1;
        let sets: Vec<project::PathMatchCandidateSet> = worktrees
            .iter()
            .map(|worktree| project::PathMatchCandidateSet {
                snapshot: worktree.read(cx).snapshot(),
                include_ignored: self.non_project,
                include_root_name,
                candidates: project::Candidates::Files,
            })
            .collect();
        let executor = cx.background_executor().clone();
        self._file_search = cx.spawn_in(window, async move |this, cx| {
            let matches = fuzzy_nucleo::match_path_sets(
                sets.as_slice(),
                &query,
                &None,
                PathMatchOptions {
                    max_results: MaxResults::limit::<MAX_RESULTS>(),
                    cancel_flag: Some(&cancel),
                    ..Default::default()
                },
                executor,
            )
            .await;
            if cancel.load(Ordering::Acquire) {
                return;
            }
            this.update_in(cx, |this, window, cx| {
                this.files = matches
                    .into_iter()
                    .map(|found| ProjectPath {
                        worktree_id: WorktreeId::from_usize(found.worktree_id),
                        path: found.path,
                    })
                    .collect();
                this.rebuild(window, cx);
            })
            .ok();
        });
    }

    /// The language servers' workspace symbols, once the typing pauses.
    fn search_symbols(&mut self, query: String, window: &mut Window, cx: &mut Context<Self>) {
        if query.is_empty() {
            self.symbols.clear();
            self._symbol_search = Task::ready(());
            return;
        }
        let project = self.project.clone();
        self._symbol_search = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            // shortcut: while the language servers find nothing (they may be indexing still),
            // ask again every second, as IntelliJ shows the results once its index is ready;
            // listen to the servers' progress instead if this asks too often
            loop {
                let symbols = project.update(cx, |project, cx| project.symbols(&query, cx));
                let symbols = symbols.await.log_err().unwrap_or_default();
                let found = this
                    .update_in(cx, |this, window, cx| {
                        let candidates: Vec<StringMatchCandidate> = symbols
                            .iter()
                            .enumerate()
                            .map(|(ix, symbol)| StringMatchCandidate::new(ix, symbol.name.clone()))
                            .collect();
                        let matches = fuzzy_nucleo::match_strings(
                            &candidates,
                            &query,
                            StringMatchOptions {
                                max_results: MaxResults::limit::<MAX_RESULTS>(),
                                ..Default::default()
                            },
                        );
                        let non_project = this.non_project;
                        this.symbols = matches
                            .into_iter()
                            .filter_map(|found| symbols.get(found.candidate_id).cloned())
                            .filter(|symbol| {
                                non_project || matches!(symbol.path, SymbolLocation::InProject(_))
                            })
                            .collect();
                        this.rebuild(window, cx);
                        !symbols.is_empty()
                    })
                    .unwrap_or(true);
                if found {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(1)).await;
            }
        });
    }

    /// The Text tab: the project's text occurrences, once the typing pauses.
    fn search_text(&mut self, query: String, window: &mut Window, cx: &mut Context<Self>) {
        if query.is_empty() {
            self.text.clear();
            self._text_search = Task::ready(());
            return;
        }
        let query = match self.regex {
            true => SearchQuery::regex(
                &query,
                self.words,
                self.match_case,
                self.non_project,
                true,
                PathMatcher::default(),
                PathMatcher::default(),
                false,
                None,
            ),
            false => SearchQuery::text(
                &query,
                self.words,
                self.match_case,
                self.non_project,
                PathMatcher::default(),
                PathMatcher::default(),
                false,
                None,
            ),
        };
        let Some(query) = query.log_err() else {
            self.text.clear();
            return;
        };
        let project = self.project.clone();
        self._text_search = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let Ok(search) = cx.update(|_, cx| {
                crate::find_in_files::find_occurrences(project, query, MAX_RESULTS, cx)
            }) else {
                return;
            };
            let (found, _) = search.await;
            this.update_in(cx, |this, window, cx| {
                this.text = found;
                this.rebuild(window, cx);
            })
            .ok();
        });
    }

    /// The rows for what was found: in All, each tab's results under its name.
    fn rebuild(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (tab, query) = self.scope(cx);
        let mut found = Vec::new();
        if let Some(commands) = self.commands(&query) {
            // IntelliJ's commands: a tab by its name
            found.extend(commands.into_iter().map(Found::Command));
        } else {
            let all = tab == Tab::All;
            let limit = if all { PER_GROUP } else { usize::MAX };
            let mut group = |title: &'static str, items: Vec<Found>| {
                if items.is_empty() {
                    return;
                }
                if all && !query.is_empty() {
                    found.push(Found::Group(title));
                }
                found.extend(items.into_iter().take(limit));
            };
            let types = |symbol: &Symbol| is_type(symbol.kind);
            if all || tab == Tab::Classes {
                group("Classes", self.symbol_rows(types));
            }
            if all || tab == Tab::Files {
                let files = match query.is_empty() {
                    true => &self.recent,
                    false => &self.files,
                };
                group("Files", files.iter().cloned().map(Found::File).collect());
            }
            if tab == Tab::Symbols {
                group("Symbols", self.symbol_rows(|_| true));
            } else if all {
                group("Symbols", self.symbol_rows(|symbol| !types(symbol)));
            }
            if (all && !query.is_empty()) || tab == Tab::Actions {
                let actions = self.action_matches.iter().copied().map(Found::Action);
                group("Actions", actions.collect());
            }
            if (all && !query.is_empty()) || tab == Tab::Endpoints {
                let endpoints = self.endpoint_matches.iter().copied().map(Found::Endpoint);
                group("Endpoints", endpoints.collect());
            }
            if tab == Tab::Text {
                group("Text", (0..self.text.len()).map(Found::Text).collect());
            }
        }
        self.found = found;
        self.selected = self
            .found
            .iter()
            .position(|found| !matches!(found, Found::Group(_)))
            .unwrap_or(0);
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Top);
        self.update_preview(window, cx);
        cx.notify();
    }

    fn symbol_rows(&self, keep: impl Fn(&Symbol) -> bool) -> Vec<Found> {
        self.symbols
            .iter()
            .enumerate()
            .filter(|(_, symbol)| keep(symbol))
            .map(|(ix, _)| Found::Symbol(ix))
            .collect()
    }

    fn select(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected != ix {
            self.selected = ix;
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
            self.update_preview(window, cx);
            cx.notify();
        }
    }

    /// IntelliJ's Next and Prev Group Element: the last result, or the first, as its mixed
    /// results scroll (`MixedSearchListModel.getIndexToScroll`).
    fn select_end(&mut self, last: bool, window: &mut Window, cx: &mut Context<Self>) {
        let result = |found: &Found| !matches!(found, Found::Group(_));
        let ix = match last {
            true => self.found.iter().rposition(result),
            false => self.found.iter().position(result),
        };
        if let Some(ix) = ix {
            self.select(ix, window, cx);
        }
    }

    fn select_next(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.found.len() as isize;
        if count == 0 {
            return;
        }
        let mut ix = self.selected as isize;
        for _ in 0..count {
            ix = (ix + step).rem_euclid(count);
            if !matches!(self.found.get(ix as usize), Some(Found::Group(_))) {
                break;
            }
        }
        self.select(ix as usize, window, cx);
    }

    fn update_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.show_preview {
            return;
        }
        let update = match self.found.get(self.selected) {
            Some(Found::File(path)) => self
                .project
                .read(cx)
                .absolute_path(path, cx)
                .map(PreviewUpdate::from_path),
            Some(Found::Symbol(ix)) => self
                .symbols
                .get(*ix)
                .map(|symbol| PreviewUpdate::from_symbol(symbol.clone())),
            Some(Found::Text(ix)) => self.text.get(*ix).map(|found| {
                let snapshot = found.buffer.read(cx).snapshot();
                PreviewUpdate::from_buffer(
                    found.buffer.clone(),
                    MatchLocation {
                        anchor_range: found.range.clone(),
                        range: found.range.start.to_offset(&snapshot)
                            ..found.range.end.to_offset(&snapshot),
                    },
                )
            }),
            _ => None,
        };
        match update {
            Some(update) => self.preview.update(update, window, cx),
            None => self.preview.clear(cx),
        }
    }

    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(found) = self.found.get(ix).cloned() else {
            return;
        };
        let workspace = self.workspace.clone();
        match found {
            Found::Group(_) => {}
            Found::Command(tab) => {
                self.search
                    .update(cx, |search, cx| search.set_value("", window, cx));
                self.set_tab(tab, window, cx);
            }
            Found::Action(ix) => {
                let Some(item) = self.actions.get(ix).filter(|item| item.enabled) else {
                    return;
                };
                let action = item.action.boxed_clone();
                if let Some(focus) = &self.previous_focus {
                    window.focus(focus, cx);
                }
                cx.emit(DismissEvent);
                window.dispatch_action(action, cx);
            }
            Found::File(path) => {
                cx.emit(DismissEvent);
                workspace
                    .update(cx, |workspace, cx| {
                        crate::open_at(workspace, path, None, window, cx)
                    })
                    .log_err();
            }
            Found::Endpoint(ix) => {
                let Some(open) = self
                    .endpoints
                    .as_ref()
                    .and_then(|endpoints| endpoints.get(ix))
                    .map(|item| item.open.clone())
                else {
                    return;
                };
                cx.emit(DismissEvent);
                workspace
                    .update(cx, |workspace, cx| open(workspace, window, cx))
                    .log_err();
            }
            Found::Text(ix) => {
                let Some(found) = self.text.get(ix) else {
                    return;
                };
                let (path, point) = (found.path.clone(), found.point);
                cx.emit(DismissEvent);
                workspace
                    .update(cx, |workspace, cx| {
                        crate::open_at(workspace, path, Some(point), window, cx)
                    })
                    .log_err();
            }
            Found::Symbol(ix) => {
                let Some(symbol) = self.symbols.get(ix).cloned() else {
                    return;
                };
                cx.emit(DismissEvent);
                open_symbol(workspace, self.project.clone(), symbol, window, cx);
            }
        }
    }

    /// Find Action's ⌥⏎, IntelliJ's Add Shortcut…: the Keyboard Shortcut dialog for the selected
    /// action.
    fn assign_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Found::Action(ix)) = self.found.get(self.selected) else {
            return;
        };
        let Some(item) = self.actions.get(*ix) else {
            return;
        };
        let action = item.action.boxed_clone();
        if let Some(focus) = &self.previous_focus {
            window.focus(focus, cx);
        }
        cx.emit(DismissEvent);
        window.defer(cx, move |window, cx| {
            spork_settings::add_shortcut(action.as_ref(), window, cx)
        });
    }

    fn render_rows(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let tokens = Tokens::get(cx);
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
                // a location or shortcut, its own text to assistive technology
                let secondary = |text: SharedString| {
                    div()
                        .id(text.clone())
                        .role(Role::Label)
                        .aria_label(text.clone())
                        .flex_none()
                        .text_color(match selected {
                            true => tokens.text,
                            false => tokens.text_secondary,
                        })
                        .child(text)
                };
                let row = popup::row(("found", ix), selected, cx)
                    .role(Role::ListBoxOption)
                    .aria_selected(selected);
                let row = match found {
                    Found::Group(_) => row,
                    Found::Command(tab) => row
                        .aria_label(format!("/{}", tab.title().to_lowercase()))
                        .child(div().child(format!("/{}", tab.title().to_lowercase())))
                        .child(secondary(format!("Switch to {}", tab.title()).into())),
                    Found::File(path) => {
                        let name = path
                            .path
                            .file_name()
                            .unwrap_or(path.path.as_unix_str())
                            .to_owned();
                        let dir = path
                            .path
                            .parent()
                            .map(|dir| dir.as_unix_str().to_owned())
                            .unwrap_or_default();
                        let root = self.root_name(path, cx);
                        let color = crate::file_status_color(&self.project, path, cx);
                        row.aria_label(name.clone())
                            .child(spork_workbench::image(
                                spork_workbench::file_icon(&name),
                                cx,
                            ))
                            .child(
                                div()
                                    .flex_none()
                                    .when_some(color, |name, color| name.text_color(color))
                                    .child(name),
                            )
                            .child(secondary(dir.into()).min_w_0().truncate())
                            .child(div().flex_1())
                            .children(root.map(secondary))
                            .child(spork_workbench::image("nodes/ideaProject", cx).size(px(16.)))
                    }
                    Found::Symbol(ix) => {
                        let symbol = self.symbols.get(*ix)?;
                        let file = match &symbol.path {
                            SymbolLocation::InProject(path) => path
                                .path
                                .file_name()
                                .map(|name| name.to_owned())
                                .unwrap_or_default(),
                            SymbolLocation::OutsideProject { abs_path, .. } => abs_path
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                        };
                        // IntelliJ colours a symbol by its file's status
                        let color = match &symbol.path {
                            SymbolLocation::InProject(path) => {
                                crate::file_status_color(&self.project, path, cx)
                            }
                            SymbolLocation::OutsideProject { .. } => None,
                        };
                        row.aria_label(symbol.name.clone())
                            .child(spork_workbench::image(symbol_icon(symbol.kind), cx))
                            .child(
                                div()
                                    .flex_none()
                                    .when_some(color, |name, color| name.text_color(color))
                                    .child(symbol.name.clone()),
                            )
                            .children(
                                symbol
                                    .container_name
                                    .clone()
                                    .map(|container| secondary(container.into()).truncate()),
                            )
                            .child(div().flex_1())
                            .child(secondary(file.clone().into()))
                            .child(spork_workbench::image(
                                spork_workbench::file_icon(&file),
                                cx,
                            ))
                    }
                    Found::Action(ix) => {
                        let item = self.actions.get(*ix)?;
                        row.aria_label(item.text.clone())
                            .child(div().size(px(16.)).flex_none())
                            .child(
                                div()
                                    .flex_none()
                                    .when(!item.enabled, |text| {
                                        text.text_color(tokens.text_disabled)
                                    })
                                    .child(item.text.clone()),
                            )
                            .child(div().flex_1())
                            .children(item.toggled.map(|on| {
                                spork_workbench::controls::switch(("toggle", *ix), on, cx)
                            }))
                            .children(item.shortcut.clone().map(secondary))
                            .child(secondary(item.location.clone()).truncate())
                    }
                    Found::Endpoint(ix) => {
                        let item = self.endpoints.as_ref()?.get(*ix)?;
                        row.aria_label(item.url.clone())
                            .child(spork_workbench::image("gutter/web", cx))
                            .child(div().flex_none().child(item.url.clone()))
                            .child(secondary(item.methods.clone()))
                            .child(div().flex_1())
                            .child(secondary(item.location.clone()).truncate())
                    }
                    Found::Text(ix) => {
                        let found = self.text.get(*ix)?;
                        let name = found.path.path.file_name().unwrap_or_default().to_owned();
                        row.aria_label(found.line.clone())
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(found.line.clone()),
                            )
                            .child(secondary(format!("{name} {}", found.point.row + 1).into()))
                    }
                };
                Some(
                    div()
                        .w_full()
                        .child(
                            row.on_mouse_move(
                                cx.listener(move |this, _, window, cx| this.select(ix, window, cx)),
                            )
                            .on_click(
                                cx.listener(move |this, _, window, cx| {
                                    this.confirm(ix, window, cx)
                                }),
                            ),
                        )
                        .into_any_element(),
                )
            })
            .collect()
    }

    fn root_name(&self, path: &ProjectPath, cx: &App) -> Option<SharedString> {
        let worktree = self
            .project
            .read(cx)
            .worktree_for_id(path.worktree_id, cx)?;
        Some(worktree.read(cx).root_name_str().to_owned().into())
    }

    /// The header's right end: the tab's check box and buttons.
    fn header_options(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut options = Vec::new();
        match self.tab {
            Tab::Actions => options.push(
                spork_workbench::controls::labeled_check(
                    "disabled-actions",
                    "Include disabled actions",
                    self.disabled_actions,
                    cx,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.disabled_actions = !this.disabled_actions;
                    this.refresh(window, cx);
                }))
                .into_any_element(),
            ),
            _ => options.push(
                spork_workbench::controls::labeled_check(
                    "non-project",
                    "Include non-project items",
                    self.non_project,
                    cx,
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_non_project(window, cx)))
                .into_any_element(),
            ),
        }
        if self.tab.previews() {
            options.push(
                ActionButton::new("preview", "general/previewHorizontally")
                    .tooltip("Preview")
                    .selected(self.show_preview)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.show_preview = !this.show_preview;
                        this.update_preview(window, cx);
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        options
    }

    /// The Text tab's search options in its field: Match Case, Words, Regex.
    fn text_options(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let option = |id: &'static str, icon: &'static str, tooltip: &'static str, on: bool| {
            ActionButton::new(id, icon).tooltip(tooltip).selected(on)
        };
        h_flex()
            .gap(px(2.))
            .child(
                option(
                    "match-case",
                    "inline/matchCase",
                    "Match Case",
                    self.match_case,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.match_case = !this.match_case;
                    this.refresh(window, cx);
                })),
            )
            .child(
                option("words", "inline/exactWords", "Words", self.words).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.words = !this.words;
                        this.refresh(window, cx);
                    },
                )),
            )
            .child(
                option("regex", "inline/regex", "Regex", self.regex).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.regex = !this.regex;
                        this.refresh(window, cx);
                    },
                )),
            )
    }

    /// What the footer says of the selected row: a file's path, or the action's ⌥⏎.
    fn footer(&self, cx: &App) -> Option<String> {
        let project = self.project.read(cx);
        let path = |path: &ProjectPath| {
            project.absolute_path(path, cx).map(|path| {
                util::paths::PathExt::compact(&path)
                    .to_string_lossy()
                    .into_owned()
            })
        };
        match self.found.get(self.selected)? {
            Found::File(file) => path(file),
            Found::Text(ix) => {
                let found = self.text.get(*ix)?;
                Some(format!("{}:{}", path(&found.path)?, found.point.row + 1))
            }
            Found::Symbol(ix) => match &self.symbols.get(*ix)?.path {
                SymbolLocation::InProject(file) => path(file),
                SymbolLocation::OutsideProject { abs_path, .. } => Some(
                    util::paths::PathExt::compact(&abs_path.to_path_buf())
                        .to_string_lossy()
                        .into_owned(),
                ),
            },
            Found::Action(_) => Some("Press ⌥⏎ to assign a shortcut".to_owned()),
            Found::Group(_) | Found::Command(_) | Found::Endpoint(_) => None,
        }
    }
}

impl Render for SearchEverywhere {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let tabs = self.tabs().into_iter().map(|tab| {
            let on = tab == self.tab;
            div()
                .id(tab.title())
                .role(Role::Tab)
                .aria_label(tab.title())
                .aria_selected(on)
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
                        .border_color(gpui::transparent_black())
                        .hover(move |t| t.bg(tokens.hover)),
                })
                .child(tab.title())
                .on_click(cx.listener(move |this, _, window, cx| this.set_tab(tab, window, cx)))
        });
        let header = h_flex()
            .h(px(40.))
            .flex_none()
            .pl(px(8.))
            .pr(px(12.))
            .gap(px(4.))
            .children(tabs)
            .child(div().flex_1())
            .children(self.header_options(cx));
        let field = div().px(px(8.)).pb(px(8.)).flex_none().child(
            spork_workbench::TextField::new(&self.search)
                .in_popup()
                .aria_label("Search Everywhere")
                .prefix(spork_workbench::icon("general/searchWithHistory").size(px(16.)))
                .map(|field| match self.tab {
                    Tab::Text => field.suffix(self.text_options(cx)),
                    _ => field.suffix(
                        div()
                            .text_color(tokens.text_secondary)
                            .child("Type / to see commands"),
                    ),
                }),
        );
        let empty = self.found.is_empty() && !self.query(cx).is_empty();
        let list = uniform_list(
            "search-everywhere",
            self.found.len(),
            cx.processor(|this, range, _, cx| this.render_rows(range, cx)),
        )
        .track_scroll(&self.scroll)
        .flex_1()
        .min_h_0();
        let preview = (self.show_preview && self.tab.previews()).then(|| {
            div()
                .flex_none()
                .h(px(PREVIEW_HEIGHT))
                .border_t_1()
                .border_color(tokens.popup_border)
                .child(self.preview.render(PreviewLayout::Below, cx))
        });
        let footer = self.footer(cx).map(|text| {
            h_flex()
                .flex_none()
                .px(px(12.))
                .py(px(8.))
                .border_t_1()
                .border_color(tokens.popup_border)
                .text_ui_small()
                .text_color(tokens.text_secondary)
                .child(div().truncate().child(text))
        });
        // the modal layer puts a popup 5 rems from the top: IntelliJ centers it
        let top = ((window.viewport_size().height - px(HEIGHT)) / 2. - window.rem_size() * 5.)
            .max(px(0.));
        spork_vcs::spork_look(cx).pt(top).child(
            popup::surface(cx)
                .id("search-everywhere")
                .role(Role::Dialog)
                .aria_label("Search Everywhere")
                .w(px(WIDTH))
                .h(px(HEIGHT))
                .key_context("SearchEverywhere")
                .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                    cx.stop_propagation();
                    this.select_next(-1, window, cx)
                }))
                .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                    cx.stop_propagation();
                    this.select_next(1, window, cx)
                }))
                // Page Down and ⌘↓, Page Up and ⌘↑: IntelliJ's Next and Prev Group Element,
                // ahead of the field's own
                .capture_action(cx.listener(|this, _: &MovePageDown, window, cx| {
                    cx.stop_propagation();
                    this.select_end(true, window, cx)
                }))
                .capture_action(cx.listener(|this, _: &MoveToEnd, window, cx| {
                    cx.stop_propagation();
                    this.select_end(true, window, cx)
                }))
                .capture_action(cx.listener(|this, _: &MovePageUp, window, cx| {
                    cx.stop_propagation();
                    this.select_end(false, window, cx)
                }))
                .capture_action(cx.listener(|this, _: &MoveToStart, window, cx| {
                    cx.stop_propagation();
                    this.select_end(false, window, cx)
                }))
                .capture_action(cx.listener(|_, _: &Escape, _, cx| {
                    cx.stop_propagation();
                    cx.emit(DismissEvent)
                }))
                // the IDE binds ↑↓⏎⎋ to the menu actions outside any context, ahead of the field's
                .on_action(cx.listener(|this, _: &menu::SelectPrevious, window, cx| {
                    this.select_next(-1, window, cx)
                }))
                .on_action(cx.listener(|this, _: &menu::SelectNext, window, cx| {
                    this.select_next(1, window, cx)
                }))
                .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| {
                    this.confirm(this.selected, window, cx)
                }))
                .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
                // Tab and ⇧Tab go through the tabs, as in IntelliJ
                .on_action(cx.listener(|this, _: &navigate::NextTab, window, cx| {
                    this.step_tab(1, window, cx)
                }))
                .on_action(cx.listener(|this, _: &navigate::PreviousTab, window, cx| {
                    this.step_tab(-1, window, cx)
                }))
                .on_action(
                    cx.listener(|this, _: &navigate::CompleteCommand, window, cx| {
                        this.complete_command(window, cx)
                    }),
                )
                .on_action(cx.listener(|this, _: &navigate::NextGroup, window, cx| {
                    this.select_end(true, window, cx)
                }))
                .on_action(
                    cx.listener(|this, _: &navigate::PreviousGroup, window, cx| {
                        this.select_end(false, window, cx)
                    }),
                )
                .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "enter" && event.keystroke.modifiers.alt {
                        cx.stop_propagation();
                        this.assign_shortcut(window, cx);
                    }
                }))
                .child(header)
                .child(field)
                .child(
                    v_flex()
                        .flex_1()
                        .min_h_0()
                        .when(empty, |list| {
                            list.child(
                                div()
                                    .id("nothing-found")
                                    .role(Role::Label)
                                    .aria_label("Nothing found")
                                    .pt(px(40.))
                                    .flex()
                                    .justify_center()
                                    .text_color(tokens.text_secondary)
                                    .child("Nothing found"),
                            )
                        })
                        .child(list),
                )
                .children(preview)
                .children(footer),
        )
    }
}

/// The types among the symbols: IntelliJ's Classes tab.
fn is_type(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class | SymbolKind::Struct | SymbolKind::Enum | SymbolKind::Interface
    )
}

/// IntelliJ's node icon for a symbol's kind.
fn symbol_icon(kind: SymbolKind) -> &'static str {
    match kind {
        // IntelliJ's class icon stands for structs too, as Rust's plugin draws them
        SymbolKind::Class | SymbolKind::Struct | SymbolKind::Object => "nodes/class",
        SymbolKind::Enum => "nodes/enum",
        SymbolKind::Interface => "nodes/interface",
        SymbolKind::Method | SymbolKind::Constructor => "nodes/method",
        SymbolKind::Function | SymbolKind::Operator => "nodes/function",
        SymbolKind::Field | SymbolKind::EnumMember | SymbolKind::Key => "nodes/field",
        SymbolKind::Property | SymbolKind::Event => "nodes/property",
        SymbolKind::Constant
        | SymbolKind::String
        | SymbolKind::Number
        | SymbolKind::Boolean
        | SymbolKind::Null
        | SymbolKind::Array => "nodes/constant",
        SymbolKind::Variable => "nodes/variable",
        SymbolKind::Module | SymbolKind::Namespace => "nodes/module",
        SymbolKind::Package => "nodes/package",
        SymbolKind::TypeParameter => "nodes/type",
        SymbolKind::File => "nodes/unknown",
    }
}

/// Opens a symbol's file at the symbol, as Zed's project symbols picker does.
fn open_symbol(
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    symbol: Symbol,
    window: &mut Window,
    cx: &mut App,
) {
    let buffer = project.update(cx, |project, cx| {
        project.open_buffer_for_symbol(&symbol, cx)
    });
    window
        .spawn(cx, async move |cx| {
            let buffer = buffer.await?;
            workspace.update_in(cx, |workspace, window, cx| {
                let position = buffer
                    .read(cx)
                    .clip_point_utf16(symbol.range.start, Bias::Left);
                let editor = workspace
                    .open_project_item::<Editor>(None, buffer, true, true, true, true, window, cx);
                editor.update(cx, |editor, cx| {
                    let snapshot = editor.buffer().read(cx).snapshot(cx);
                    let Some(buffer) = snapshot.as_singleton() else {
                        return;
                    };
                    let Some(anchor) = snapshot.anchor_in_buffer(buffer.anchor_before(position))
                    else {
                        return;
                    };
                    editor.change_selections(
                        SelectionEffects::scroll(Autoscroll::center()).unfold(),
                        window,
                        cx,
                        |selections| selections.select_ranges([anchor..anchor]),
                    );
                });
            })?;
            anyhow::Ok(())
        })
        .detach_and_log_err(cx);
}

/// The actions the focused element can run, as the command palette lists them, with IntelliJ's
/// text and menu path for those in the menu bar; then, greyed, the others that take no argument.
fn actions(workspace: &Entity<Workspace>, window: &mut Window, cx: &mut App) -> Vec<ActionItem> {
    let mut menus = Vec::new();
    if let Some(bar) = cx.get_menus() {
        for menu in bar {
            walk_menu(&menu.items, &mut vec![menu.name.to_string()], &mut menus);
        }
    }
    let filter = CommandPaletteFilter::try_global(cx);
    let available = window.available_actions(cx);
    let tool_windows = crate::tool_windows(workspace, window, cx);
    let mut names = collections::HashSet::default();
    let mut items: Vec<ActionItem> = available
        .into_iter()
        .filter(|action| !filter.is_some_and(|filter| filter.is_hidden(action.as_ref())))
        .filter(|action| names.insert(action.name()))
        .map(|action| {
            let menu = menus
                .iter()
                .find(|(_, _, menu_action)| menu_action.partial_eq(action.as_ref()));
            let (text, location) = match menu {
                Some((path, name, _)) => (name.clone().into(), path.join(" | ").into()),
                None => in_words(action.name()),
            };
            ActionItem {
                text,
                location,
                shortcut: crate::shortcut_text(action.as_ref(), window),
                toggled: toggle_state(
                    action.as_ref(),
                    &tool_windows,
                    workspace.read(cx),
                    window,
                    cx,
                ),
                action,
                enabled: true,
            }
        })
        .collect();
    let mut disabled: Vec<ActionItem> = cx
        .all_action_names()
        .iter()
        .filter(|name| !names.contains(*name))
        .filter_map(|name| cx.build_action(name, None).ok())
        .filter(|action| !filter.is_some_and(|filter| filter.is_hidden(action.as_ref())))
        .map(|action| {
            let (text, location) = in_words(action.name());
            ActionItem {
                text,
                location,
                shortcut: None,
                toggled: None,
                action,
                enabled: false,
            }
        })
        .collect();
    items.sort_by(|a, b| a.text.cmp(&b.text));
    disabled.sort_by(|a, b| a.text.cmp(&b.text));
    items.extend(disabled);
    items
}

fn walk_menu(
    items: &[OwnedMenuItem],
    path: &mut Vec<String>,
    out: &mut Vec<(Vec<String>, String, Box<dyn Action>)>,
) {
    for item in items {
        match item {
            OwnedMenuItem::Action { name, action, .. } => {
                out.push((path.clone(), name.clone(), action.boxed_clone()))
            }
            OwnedMenuItem::Submenu(menu) => {
                path.push(menu.name.to_string());
                walk_menu(&menu.items, path, out);
                path.pop();
            }
            OwnedMenuItem::Separator | OwnedMenuItem::SystemMenu(_) => {}
        }
    }
}

/// An action not in the menu bar, as IntelliJ would name it: `workspace::ToggleLeftDock` is
/// "Toggle Left Dock", in "Workspace".
fn in_words(name: &str) -> (SharedString, SharedString) {
    let (namespace, name) = name.rsplit_once("::").unwrap_or(("", name));
    let mut words = String::new();
    for (ix, ch) in name.char_indices() {
        let previous = name[..ix].chars().next_back();
        if ch.is_uppercase() && previous.is_some_and(|previous| !previous.is_uppercase()) {
            words.push(' ');
        }
        words.push(ch);
    }
    let group = namespace
        .rsplit("::")
        .next()
        .unwrap_or_default()
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    // a one-word name says too little on its own: "Toggle" is "Toggle Debug Panel"
    if !words.contains(' ') && !group.is_empty() {
        words = format!("{words} {group}");
    }
    (words.into(), group.into())
}

/// A toggle's state where it shows: a tool window open, full screen, the active editor's line
/// numbers, soft wrap and breadcrumbs.
// shortcut: Zed's actions don't say whether they toggle, so only these show a state; others
// join as Zed exposes their state
fn toggle_state(
    action: &dyn Action,
    tool_windows: &[crate::ToolWindow],
    workspace: &Workspace,
    window: &Window,
    cx: &App,
) -> Option<bool> {
    for tool_window in tool_windows {
        if tool_window.toggle_action.partial_eq(action) {
            return Some(tool_window.open);
        }
    }
    let editor = || workspace.active_item_as::<Editor>(cx);
    match action.name() {
        "zed::ToggleFullScreen" => Some(window.is_fullscreen()),
        "editor::ToggleLineNumbers" => Some(editor()?.read(cx).line_numbers_enabled(cx)),
        "editor::ToggleSoftWrap" => Some(!matches!(
            editor()?.read(cx).soft_wrap_mode(cx),
            editor::SoftWrap::None
        )),
        "editor::ToggleBreadcrumb" => Some(editor()?.read(cx).breadcrumbs_visible()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::in_words;

    #[test]
    fn names_actions_in_words() {
        assert_eq!(
            in_words("workspace::ToggleLeftDock"),
            ("Toggle Left Dock".into(), "Workspace".into())
        );
        assert_eq!(
            in_words("project_panel::ToggleFocus"),
            ("Toggle Focus".into(), "Project Panel".into())
        );
        assert_eq!(in_words("zed::OpenLog"), ("Open Log".into(), "Zed".into()));
        assert_eq!(
            in_words("debug_panel::Toggle"),
            ("Toggle Debug Panel".into(), "Debug Panel".into())
        );
    }
}
