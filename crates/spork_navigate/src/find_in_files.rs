//! Find in Files (⇧⌘F) and Replace in Files (⇧⌘R), IntelliJ's `FindPopupPanel` over Zed's project
//! search: the title with the matches found and the file mask, the search field with its history
//! and its Cc, W and .* options, the replace field with Preserve case, the scope tabs (In Project,
//! Module, Directory, Scope), the occurrences with their file and line, the preview under them,
//! ⏎ to go to one and Open in Find Window (⌘⏎) to keep them in the Find tool window; Replace and
//! Replace All in the replace variant.
//!
//! Left out, as Zed's search can't serve them: the filter popup (In Comments, In String
//! Literals…), the scopes IntelliJ derives from its project model (production and test files,
//! scratches, changelists); and the popup's pin.

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use collections::HashSet;
use editor::Editor;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, Context, DismissEvent, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, PromptLevel,
    Render, Role, ScrollStrategy, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, TaskExt as _, UniformListScrollHandle, WeakEntity, Window, div, px,
    uniform_list,
};
use gpui_kit::component::input::{Escape, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::{h_flex, v_flex};
use language::{Buffer, Point, ToOffset as _, ToPoint as _};

use gpui_kit::component::Selectable as _;
use picker::{MatchLocation, PreviewBackend, PreviewLayout, PreviewUpdate};
use project::search::{SearchQuery, SearchResult};
use project::{Project, ProjectPath, WorktreeId};
use spork_workbench::{ActionButton, Tokens, popup};
use theme::ActiveTheme as _;
use util::ResultExt as _;
use util::paths::PathMatcher;
use workspace::{ModalView, Workspace};

use crate::navigate;

/// IntelliJ's first size for it, its height kept within the IDE's window.
const WIDTH: f32 = 650.;
const HEIGHT: f32 = 640.;
/// The results' share of the space the preview splits with them (`myPreviewSplitter`, .33).
const RESULTS_SHARE: f32 = 0.33;
/// How many occurrences a search keeps: IntelliJ's popup shows the first ones too.
pub(crate) const MAX_OCCURRENCES: usize = 1000;
/// Where IntelliJ shows the popup: this far from the window's top (`FindPopupPanel.showUI`).
const TOP: f32 = 115.;

/// Where a search looks.
#[derive(Clone)]
pub(crate) enum Scope {
    Project,
    /// A root folder: Zed's worktree is IntelliJ's module.
    Module(WorktreeId),
    /// A folder of the project, from its root ("" the root), with its subfolders or not.
    Directory {
        path: String,
        recursive: bool,
    },
    /// Some files: the open ones, the current one.
    Buffers {
        title: SharedString,
        buffers: Vec<Entity<Buffer>>,
    },
}

/// What a search looks for and where: IntelliJ's `FindModel`.
#[derive(Clone)]
pub(crate) struct FindModel {
    pub(crate) text: String,
    pub(crate) match_case: bool,
    pub(crate) words: bool,
    pub(crate) regex: bool,
    pub(crate) scope: Scope,
    /// File masks, comma-separated (`*.rs, *.toml`).
    pub(crate) mask: Option<String>,
}

impl FindModel {
    pub(crate) fn query(&self, project: &Project, cx: &App) -> anyhow::Result<SearchQuery> {
        let style = project.path_style(cx);
        let masks: Vec<String> = self
            .mask
            .iter()
            .flat_map(|mask| mask.split(','))
            .map(|mask| mask.trim().to_owned())
            .filter(|mask| !mask.is_empty())
            .collect();
        let within = |folder: &str, recursive: bool| -> Vec<String> {
            let folder = folder.trim_matches('/');
            let prefix = match (folder.is_empty(), recursive) {
                (true, true) => "**/".to_owned(),
                (true, false) => String::new(),
                (false, true) => format!("{folder}/**/"),
                (false, false) => format!("{folder}/"),
            };
            match masks.is_empty() {
                true => vec![format!("{prefix}*")],
                false => masks.iter().map(|mask| format!("{prefix}{mask}")).collect(),
            }
        };
        let (include, full_paths, buffers) = match &self.scope {
            Scope::Project => (masks.clone(), false, None),
            Scope::Module(id) => {
                let root = project
                    .worktree_for_id(*id, cx)
                    .map(|worktree| worktree.read(cx).root_name_str().to_owned())
                    .unwrap_or_default();
                (within(&root, true), true, None)
            }
            Scope::Directory { path, recursive } => (within(path, *recursive), false, None),
            Scope::Buffers { buffers, .. } => (masks.clone(), false, Some(buffers.clone())),
        };
        let include = PathMatcher::new(include, style)?;
        match self.regex {
            true => SearchQuery::regex(
                &self.text,
                self.words,
                self.match_case,
                false,
                false,
                include,
                PathMatcher::default(),
                full_paths,
                buffers,
            ),
            false => SearchQuery::text(
                &self.text,
                self.words,
                self.match_case,
                false,
                include,
                PathMatcher::default(),
                full_paths,
                buffers,
            ),
        }
    }

    /// The scope as IntelliJ names it in the Find tool window: "Project", "Module 'name'",
    /// "Directory src", with " with mask '*.rs'".
    pub(crate) fn scope_title(&self, project: &Project, cx: &App) -> String {
        let scope = match &self.scope {
            Scope::Project => "Project".to_owned(),
            Scope::Module(id) => {
                let name = project
                    .worktree_for_id(*id, cx)
                    .map(|worktree| worktree.read(cx).root_name_str().to_owned())
                    .unwrap_or_default();
                format!("Module '{name}'")
            }
            Scope::Directory { path, .. } => format!("Directory {path}"),
            Scope::Buffers { title, .. } => title.to_string(),
        };
        match &self.mask {
            Some(mask) if !mask.trim().is_empty() => format!("{scope} with mask '{mask}'"),
            _ => scope,
        }
    }
}

/// A text occurrence: its file, where it is, and its line as the results show it.
#[derive(Clone)]
pub struct Occurrence {
    pub path: ProjectPath,
    pub buffer: Entity<Buffer>,
    pub range: Range<language::Anchor>,
    pub point: Point,
    /// The line, its indentation and its end trimmed.
    pub line: SharedString,
    /// The occurrence within `line`.
    pub found: Range<usize>,
}

impl Occurrence {
    /// The occurrence of `range` in `buffer`, a file of the project; `None` for a buffer with no
    /// file.
    pub fn new(buffer: &Entity<Buffer>, range: Range<language::Anchor>, cx: &App) -> Option<Self> {
        let file = buffer.read(cx).file()?;
        let path = ProjectPath {
            worktree_id: file.worktree_id(cx),
            path: file.path().clone(),
        };
        let snapshot = buffer.read(cx).snapshot();
        let point = range.start.to_point(&snapshot);
        let end = range.end.to_point(&snapshot);
        let line_end = Point::new(point.row, snapshot.line_len(point.row));
        let line: String = snapshot
            .text_for_range(Point::new(point.row, 0)..line_end)
            .collect();
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        let start = (point.column as usize).saturating_sub(indent);
        let end = match end.row == point.row {
            true => (end.column as usize).saturating_sub(indent),
            false => trimmed.len(),
        };
        Some(Occurrence {
            path,
            buffer: buffer.clone(),
            range,
            point,
            line: trimmed.to_owned().into(),
            found: start.min(trimmed.len())..end.min(trimmed.len()),
        })
    }

    pub fn file_name(&self) -> SharedString {
        self.path
            .path
            .file_name()
            .unwrap_or_default()
            .to_owned()
            .into()
    }

    pub fn preview(&self, cx: &App) -> PreviewUpdate {
        let snapshot = self.buffer.read(cx).snapshot();
        PreviewUpdate::from_buffer(
            self.buffer.clone(),
            MatchLocation {
                anchor_range: self.range.clone(),
                range: self.range.start.to_offset(&snapshot)..self.range.end.to_offset(&snapshot),
            },
        )
    }
}

/// Searches the project, keeping the first `limit` occurrences; answers whether there were more.
pub fn find_occurrences(
    project: Entity<Project>,
    query: SearchQuery,
    limit: usize,
    cx: &mut App,
) -> Task<(Vec<Occurrence>, bool)> {
    let results = project.update(cx, |project, cx| project.search(query, cx));
    cx.spawn(async move |cx| {
        let _search = results.task_handle;
        let mut found = Vec::new();
        let mut limited = false;
        while let Ok(result) = results.rx.recv().await {
            let SearchResult::Buffer { buffer, ranges } = result else {
                if let SearchResult::LimitReached = result {
                    limited = true;
                    break;
                }
                continue;
            };
            let occurrences = cx.update(|cx| {
                ranges
                    .into_iter()
                    .filter_map(|range| Occurrence::new(&buffer, range, cx))
                    .collect::<Vec<_>>()
            });
            found.extend(occurrences);
            if found.len() >= limit {
                found.truncate(limit);
                limited = true;
                break;
            }
        }
        (found, limited)
    })
}

/// Replaces `occurrences` with `replacement` (in each one's case with `preserve_case`), then
/// saves their files, as IntelliJ's Replace in Files leaves them.
pub(crate) fn replace_occurrences(
    project: Entity<Project>,
    query: SearchQuery,
    occurrences: Vec<Occurrence>,
    replacement: String,
    preserve_case: bool,
    cx: &mut App,
) -> Task<anyhow::Result<()>> {
    let query = query.with_replacement(replacement.clone());
    let mut buffers: Vec<Entity<Buffer>> = Vec::new();
    for occurrence in &occurrences {
        if !buffers.contains(&occurrence.buffer) {
            buffers.push(occurrence.buffer.clone());
        }
    }
    for buffer in &buffers {
        let snapshot = buffer.read(cx).snapshot();
        let edits: Vec<(Range<language::Anchor>, String)> = occurrences
            .iter()
            .filter(|occurrence| occurrence.buffer == *buffer)
            .filter_map(|occurrence| {
                let range = occurrence.range.start.to_offset(&snapshot)
                    ..occurrence.range.end.to_offset(&snapshot);
                let row = occurrence.point.row;
                let line_start = snapshot.point_to_offset(Point::new(row, 0));
                let line_end = snapshot.point_to_offset(Point::new(row, snapshot.line_len(row)));
                let line: String = snapshot.text_for_range(line_start..line_end).collect();
                let found: String = snapshot.text_for_range(range.clone()).collect();
                let hit = range.start.saturating_sub(line_start)
                    ..range.end.min(line_end).saturating_sub(line_start);
                let text = match query.replacement_requires_context() {
                    true => query.replacement_for(&line, hit)?.into_owned(),
                    false if preserve_case => {
                        search::buffer_search::preserve_case(&replacement, &found)
                    }
                    false => replacement.clone(),
                };
                Some((occurrence.range.clone(), text))
            })
            .collect();
        buffer.update(cx, |buffer, cx| buffer.edit(edits, None, cx));
    }
    let saves: Vec<_> = buffers
        .into_iter()
        .map(|buffer| project.update(cx, |project, cx| project.save_buffer(buffer, cx)))
        .collect();
    cx.background_spawn(async move {
        for save in saves {
            save.await?;
        }
        anyhow::Ok(())
    })
}

pub(crate) fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &navigate::FindInFiles, window, cx| {
        open(workspace, false, window, cx)
    });
    workspace.register_action(|workspace, _: &navigate::ReplaceInFiles, window, cx| {
        open(workspace, true, window, cx)
    });
}

/// The scope tabs, IntelliJ's `find.popup.scope.*`.
#[derive(Clone, Copy, PartialEq)]
enum ScopeTab {
    Project,
    Module,
    Directory,
    Scope,
}

impl ScopeTab {
    const ALL: [ScopeTab; 4] = [
        ScopeTab::Project,
        ScopeTab::Module,
        ScopeTab::Directory,
        ScopeTab::Scope,
    ];

    fn title(self) -> &'static str {
        match self {
            ScopeTab::Project => "In Project",
            ScopeTab::Module => "Module",
            ScopeTab::Directory => "Directory",
            ScopeTab::Scope => "Scope",
        }
    }
}

/// The named scopes Zed's search can serve.
#[derive(Clone, Copy, PartialEq)]
enum NamedScope {
    ProjectFiles,
    OpenFiles,
    CurrentFile,
}

impl NamedScope {
    const ALL: [NamedScope; 3] = [
        NamedScope::ProjectFiles,
        NamedScope::OpenFiles,
        NamedScope::CurrentFile,
    ];

    fn title(self) -> &'static str {
        match self {
            NamedScope::ProjectFiles => "Project Files",
            NamedScope::OpenFiles => "Open Files",
            NamedScope::CurrentFile => "Current File",
        }
    }
}

/// The search options and scope a popup was left with, for the next one: IntelliJ's
/// `FindSettings`.
#[derive(Clone)]
struct Remembered {
    match_case: bool,
    words: bool,
    regex: bool,
    preserve_case: bool,
    tab: ScopeTab,
    named: NamedScope,
    recursive: bool,
    mask: Option<String>,
    new_tab: bool,
    history: Vec<String>,
    replace_history: Vec<String>,
}

impl gpui::Global for Remembered {}

impl Default for Remembered {
    fn default() -> Self {
        Self {
            match_case: false,
            words: false,
            regex: false,
            preserve_case: false,
            tab: ScopeTab::Project,
            named: NamedScope::ProjectFiles,
            recursive: true,
            mask: None,
            new_tab: false,
            history: Vec::new(),
            replace_history: Vec::new(),
        }
    }
}

fn open(
    workspace: &mut Workspace,
    replace: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if let Some(popup) = workspace.active_modal::<FindInFiles>(cx) {
        popup.update(cx, |popup, cx| popup.set_replace(replace, window, cx));
        return;
    }
    let project = workspace.project().clone();
    // the selection, else the word at the caret, seeds the search as in IntelliJ
    let editor = workspace.active_item_as::<Editor>(cx);
    let seed = editor.as_ref().and_then(|editor| {
        editor.update(cx, |editor, cx| {
            let snapshot = editor.buffer().read(cx).snapshot(cx);
            let selection = editor
                .selections
                .newest::<editor::MultiBufferOffset>(&editor.display_snapshot(cx));
            let range = selection.range();
            (!range.is_empty() && range.end.0 - range.start.0 < 200)
                .then(|| snapshot.text_for_range(range).collect::<String>())
                .filter(|text| !text.contains('\n'))
        })
    });
    let current = editor.as_ref().and_then(|editor| {
        let buffer = editor.read(cx).buffer().read(cx).as_singleton()?;
        let name = buffer.read(cx).file()?.file_name(cx).to_owned();
        Some((buffer, name))
    });
    let directory = current
        .as_ref()
        .and_then(|(buffer, _)| buffer.read(cx).file())
        .and_then(|file| file.path().parent())
        .map(|dir| dir.as_unix_str().to_owned())
        .unwrap_or_default();
    let open_buffers: Vec<Entity<Buffer>> = workspace
        .items_of_type::<Editor>(cx)
        .filter_map(|editor| editor.read(cx).buffer().read(cx).as_singleton())
        .collect();
    let handle = cx.entity().downgrade();
    workspace.toggle_modal(window, cx, move |window, cx| {
        FindInFiles::new(
            handle,
            project,
            replace,
            seed,
            directory,
            current,
            open_buffers,
            window,
            cx,
        )
    });
}

pub(crate) struct FindInFiles {
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    replace: bool,
    search: Entity<InputState>,
    replacement: Entity<InputState>,
    mask: Entity<InputState>,
    directory: Entity<InputState>,
    options: Remembered,
    module: Option<WorktreeId>,
    current: Option<(Entity<Buffer>, String)>,
    open_buffers: Vec<Entity<Buffer>>,
    occurrences: Vec<Occurrence>,
    limited: bool,
    searching: bool,
    error: Option<String>,
    selected: usize,
    scroll: UniformListScrollHandle,
    preview: Arc<dyn PreviewBackend>,
    _search: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DismissEvent> for FindInFiles {}

impl ModalView for FindInFiles {
    fn on_before_dismiss(
        &mut self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> workspace::DismissDecision {
        self.remember(cx);
        workspace::DismissDecision::Dismiss(true)
    }
}

impl Focusable for FindInFiles {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
}

impl FindInFiles {
    #[allow(clippy::too_many_arguments)]
    fn new(
        workspace: WeakEntity<Workspace>,
        project: Entity<Project>,
        replace: bool,
        seed: Option<String>,
        directory: String,
        current: Option<(Entity<Buffer>, String)>,
        open_buffers: Vec<Entity<Buffer>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let options = cx.try_global::<Remembered>().cloned().unwrap_or_default();
        let text = seed
            .or_else(|| options.history.last().cloned())
            .unwrap_or_default();
        let search = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("Search");
            state.set_value(text, window, cx);
            state
        });
        let replacement = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("Replace");
            if let Some(last) = options.replace_history.last() {
                state.set_value(last.clone(), window, cx);
            }
            state
        });
        let mask = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("*.java");
            if let Some(mask) = &options.mask {
                state.set_value(mask.clone(), window, cx);
            }
            state
        });
        let directory = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(directory, window, cx);
            state
        });
        let refresh_on =
            |this: &mut Self, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>| {
                if let InputEvent::Change = event {
                    this.refresh(window, cx);
                }
            };
        let mut subscriptions = vec![
            cx.subscribe_in(
                &search,
                window,
                move |this, _, event, window, cx| match event {
                    InputEvent::PressEnter { secondary, .. } => match secondary {
                        true => this.open_in_find_window(window, cx),
                        false => this.go_to(this.selected, window, cx),
                    },
                    event => refresh_on(this, event, window, cx),
                },
            ),
            cx.subscribe_in(&mask, window, move |this, _, event, window, cx| {
                refresh_on(this, event, window, cx)
            }),
            cx.subscribe_in(&directory, window, move |this, _, event, window, cx| {
                refresh_on(this, event, window, cx)
            }),
        ];
        subscriptions.push(
            cx.subscribe_in(&replacement, window, |this, _, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.replace_selected(window, cx);
                }
            }),
        );
        let module = project
            .read(cx)
            .visible_worktrees(cx)
            .next()
            .map(|worktree| worktree.read(cx).id());
        let preview = picker_preview::editor_preview(project.clone(), window, cx);
        let mut this = Self {
            workspace,
            project,
            replace,
            search,
            replacement,
            mask,
            directory,
            options,
            module,
            current,
            open_buffers,
            occurrences: Vec::new(),
            limited: false,
            searching: false,
            error: None,
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            preview,
            _search: Task::ready(()),
            _subscriptions: subscriptions,
        };
        this.search
            .update(cx, |search, cx| search.select_all(window, cx));
        this.refresh(window, cx);
        this
    }

    fn text(&self, cx: &App) -> String {
        self.search.read(cx).value().to_string()
    }

    fn set_replace(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.replace = replace;
        window.focus(&self.search.focus_handle(cx), cx);
        cx.notify();
    }

    /// The search the popup's fields and options describe.
    pub(crate) fn model(&self, cx: &App) -> FindModel {
        let scope = match self.options.tab {
            ScopeTab::Project => Scope::Project,
            ScopeTab::Module => self.module.map_or(Scope::Project, Scope::Module),
            ScopeTab::Directory => Scope::Directory {
                path: self.directory.read(cx).value().to_string(),
                recursive: self.options.recursive,
            },
            ScopeTab::Scope => match self.options.named {
                NamedScope::ProjectFiles => Scope::Project,
                NamedScope::OpenFiles => Scope::Buffers {
                    title: "Open Files".into(),
                    buffers: self.open_buffers.clone(),
                },
                NamedScope::CurrentFile => Scope::Buffers {
                    title: self
                        .current
                        .as_ref()
                        .map(|(_, name)| format!("File '{name}'"))
                        .unwrap_or_else(|| "Current File".to_owned())
                        .into(),
                    buffers: self
                        .current
                        .iter()
                        .map(|(buffer, _)| buffer.clone())
                        .collect(),
                },
            },
        };
        let mask = self.mask.read(cx).value().to_string();
        FindModel {
            text: self.text(cx),
            match_case: self.options.match_case,
            words: self.options.words,
            regex: self.options.regex,
            scope,
            mask: (self.options.mask.is_some() && !mask.trim().is_empty()).then_some(mask),
        }
    }

    /// Searches anew once the typing pauses.
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model(cx);
        self.error = None;
        if model.text.is_empty() {
            self.occurrences.clear();
            self.searching = false;
            self._search = Task::ready(());
            self.selected = 0;
            self.update_preview(window, cx);
            cx.notify();
            return;
        }
        let query = match model.query(self.project.read(cx), cx) {
            Ok(query) => query,
            Err(error) => {
                self.error = Some(error.to_string());
                self.occurrences.clear();
                cx.notify();
                return;
            }
        };
        self.searching = true;
        let project = self.project.clone();
        self._search = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let search = cx.update(|_, cx| find_occurrences(project, query, MAX_OCCURRENCES, cx));
            let Ok(search) = search else {
                return;
            };
            let (occurrences, limited) = search.await;
            this.update_in(cx, |this, window, cx| {
                this.occurrences = occurrences;
                this.limited = limited;
                this.searching = false;
                this.selected = 0;
                this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                this.update_preview(window, cx);
                cx.notify();
            })
            .ok();
        });
        cx.notify();
    }

    fn update_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.occurrences.get(self.selected) {
            Some(occurrence) => {
                let update = occurrence.preview(cx);
                self.preview.update(update, window, cx)
            }
            None => self.preview.clear(cx),
        }
    }

    fn select(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix < self.occurrences.len() && ix != self.selected {
            self.selected = ix;
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
            self.update_preview(window, cx);
            cx.notify();
        }
    }

    fn step(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.occurrences.len() as isize;
        if count > 0 {
            let ix = (self.selected as isize + step).rem_euclid(count) as usize;
            self.select(ix, window, cx);
        }
    }

    /// Keeps the options and the search for the next popup, as IntelliJ's `FindSettings`.
    fn remember(&mut self, cx: &mut Context<Self>) {
        let text = self.text(cx);
        let mut options = self.options.clone();
        let mask = self.mask.read(cx).value().to_string();
        options.mask = options.mask.is_some().then_some(mask);
        for (history, entry) in [
            (&mut options.history, text),
            (
                &mut options.replace_history,
                self.replacement.read(cx).value().to_string(),
            ),
        ] {
            if !entry.is_empty() {
                history.retain(|old| *old != entry);
                history.push(entry);
            }
        }
        cx.set_global(options);
    }

    /// ⏎: opens the occurrence and closes the popup.
    fn go_to(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(occurrence) = self.occurrences.get(ix).cloned() else {
            return;
        };
        cx.emit(DismissEvent);
        self.workspace
            .update(cx, |workspace, cx| {
                crate::open_at(
                    workspace,
                    occurrence.path,
                    Some(occurrence.point),
                    window,
                    cx,
                )
            })
            .log_err();
    }

    /// Open in Find Window (⌘⏎): the occurrences go to the Find tool window, in a tab of their own
    /// with Open results in new tab.
    fn open_in_find_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model(cx);
        if model.text.is_empty() {
            return;
        }
        let occurrences = self.occurrences.clone();
        let new_tab = self.options.new_tab;
        cx.emit(DismissEvent);
        self.workspace
            .update(cx, |workspace, cx| {
                crate::find_panel::show_occurrences(
                    workspace,
                    model,
                    Some(occurrences),
                    new_tab,
                    window,
                    cx,
                )
            })
            .log_err();
    }

    fn replace_with(
        &mut self,
        occurrences: Vec<Occurrence>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let model = self.model(cx);
        let Some(query) = model.query(self.project.read(cx), cx).log_err() else {
            return;
        };
        let replacement = self.replacement.read(cx).value().to_string();
        let preserve_case = self.options.preserve_case && !model.regex;
        let replaced = replace_occurrences(
            self.project.clone(),
            query,
            occurrences,
            replacement,
            preserve_case,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            replaced.await?;
            this.update_in(cx, |this, window, cx| this.refresh(window, cx))
        })
        .detach_and_log_err(cx);
    }

    /// Replace: the selected occurrence, then on to the next.
    fn replace_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.replace {
            return;
        }
        let Some(occurrence) = self.occurrences.get(self.selected).cloned() else {
            return;
        };
        self.replace_with(vec![occurrence], window, cx);
    }

    /// Replace All, after IntelliJ's confirmation.
    fn replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.occurrences.is_empty() {
            return;
        }
        let files: HashSet<&ProjectPath> = self.occurrences.iter().map(|o| &o.path).collect();
        let message = format!(
            "Replace {} occurrences of '{}' across {} files with '{}'?",
            self.occurrences.len(),
            self.text(cx),
            files.len(),
            self.replacement.read(cx).value()
        );
        let answer = window.prompt(
            PromptLevel::Info,
            "Replace All",
            Some(&message),
            &["Replace", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await? != 0 {
                return Ok(());
            }
            this.update_in(cx, |this, window, cx| {
                let occurrences = this.occurrences.clone();
                this.replace_with(occurrences, window, cx)
            })
        })
        .detach_and_log_err(cx);
    }

    fn set_option(
        &mut self,
        set: impl FnOnce(&mut Remembered),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        set(&mut self.options);
        self.refresh(window, cx);
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.key == "enter" && keystroke.modifiers.platform {
            cx.stop_propagation();
            self.open_in_find_window(window, cx);
        }
    }

    fn render_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let tokens = Tokens::get(cx);
        let found_bg = cx.theme().colors().search_match_background;
        range
            .filter_map(|ix| {
                let occurrence = self.occurrences.get(ix)?;
                let selected = ix == self.selected;
                let line = &occurrence.line;
                let found = occurrence.found.clone();
                let (before, at, after) = match (
                    line.get(..found.start),
                    line.get(found.clone()),
                    line.get(found.end..),
                ) {
                    (Some(before), Some(at), Some(after)) => (before, at, after),
                    _ => (line.as_ref(), "", ""),
                };
                let location = format!("{} {}", occurrence.file_name(), occurrence.point.row + 1);
                Some(
                    div()
                        .w_full()
                        .child(
                            popup::row(("occurrence", ix), selected, cx)
                                .role(Role::ListBoxOption)
                                .aria_label(line.clone())
                                .aria_selected(selected)
                                .child(
                                    h_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .child(before.to_owned())
                                        .child(div().bg(found_bg).child(at.to_owned()))
                                        .child(after.to_owned()),
                                )
                                .child(
                                    div()
                                        .id(("location", ix))
                                        .role(Role::Label)
                                        .aria_label(location.clone())
                                        .flex_none()
                                        .text_color(match selected {
                                            true => tokens.text,
                                            false => tokens.text_secondary,
                                        })
                                        .child(location),
                                )
                                .on_click(cx.listener(
                                    move |this, event: &gpui::ClickEvent, window, cx| {
                                        this.select(ix, window, cx);
                                        if event.click_count() >= 2 {
                                            this.go_to(ix, window, cx);
                                        }
                                    },
                                )),
                        )
                        .into_any_element(),
                )
            })
            .collect()
    }

    fn option(
        &self,
        id: &'static str,
        icon: &'static str,
        tooltip: &'static str,
        on: bool,
        set: fn(&mut Remembered),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        ActionButton::new(id, icon)
            .tooltip(tooltip)
            .selected(on)
            .on_click(cx.listener(move |this, _, window, cx| this.set_option(set, window, cx)))
            .into_any_element()
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let title = match self.replace {
            true => "Replace in Files",
            false => "Find in Files",
        };
        let files: HashSet<&ProjectPath> = self.occurrences.iter().map(|o| &o.path).collect();
        let info = (!self.occurrences.is_empty()).then(|| {
            let plus = if self.limited { "+" } else { "" };
            let matches = self.occurrences.len();
            let (match_word, file_word) = (
                if matches == 1 { "match" } else { "matches" },
                if files.len() == 1 { "file" } else { "files" },
            );
            format!(
                "{matches}{plus} {match_word} in {}{plus} {file_word}",
                files.len()
            )
        });
        let mask_on = self.options.mask.is_some();
        h_flex()
            .flex_none()
            .h(px(40.))
            .pl(px(16.))
            .pr(px(12.))
            .gap(px(8.))
            .border_b_1()
            .border_color(tokens.popup_border)
            .child(
                div()
                    .id("title")
                    .role(Role::Label)
                    .aria_label(title)
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(title),
            )
            .children(info.map(|info| {
                div()
                    .id("info")
                    .role(Role::Label)
                    .aria_label(info.clone())
                    .text_color(tokens.text_secondary)
                    .child(info)
            }))
            .child(div().flex_1())
            .child(
                spork_workbench::controls::labeled_check("file-mask", "File mask:", mask_on, cx)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let mask = this.mask.read(cx).value().to_string();
                        this.set_option(
                            move |options| {
                                options.mask = match options.mask {
                                    Some(_) => None,
                                    None => Some(mask),
                                }
                            },
                            window,
                            cx,
                        )
                    })),
            )
            .child(
                div().w(px(110.)).child(
                    spork_workbench::TextField::new(&self.mask)
                        .aria_label("File mask")
                        .disabled(!mask_on),
                ),
            )
            .into_any_element()
    }

    fn field_row(
        &self,
        state: Entity<InputState>,
        label: &'static str,
        suffix: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = Tokens::get(cx);
        div()
            .flex_none()
            .px(px(8.))
            .py(px(4.))
            .border_b_1()
            .border_color(tokens.popup_border)
            .child(
                spork_workbench::TextField::new(&state)
                    .in_popup()
                    .aria_label(label)
                    .prefix(spork_workbench::icon("inline/searchHistory").size(px(16.)))
                    .suffix(suffix),
            )
            .into_any_element()
    }

    fn scope_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let tabs = ScopeTab::ALL.into_iter().map(|tab| {
            let on = tab == self.options.tab;
            div()
                .id(tab.title())
                .role(Role::RadioButton)
                .aria_label(tab.title())
                .aria_selected(on)
                .h(px(24.))
                .px(px(6.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .cursor_pointer()
                .map(|t| match on {
                    true => t.bg(tokens.pressed),
                    false => t.hover(move |t| t.bg(tokens.hover)),
                })
                .child(tab.title())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.set_option(move |options| options.tab = tab, window, cx)
                }))
        });
        let control: Option<AnyElement> = match self.options.tab {
            ScopeTab::Project => None,
            ScopeTab::Module => {
                let project = self.project.read(cx);
                let modules: Vec<(WorktreeId, String)> = project
                    .visible_worktrees(cx)
                    .map(|worktree| {
                        let worktree = worktree.read(cx);
                        (worktree.id(), worktree.root_name_str().to_owned())
                    })
                    .collect();
                let current = modules
                    .iter()
                    .find(|(id, _)| Some(*id) == self.module)
                    .map(|(_, name)| name.clone())
                    .unwrap_or_default();
                let this = cx.entity().downgrade();
                Some(spork_workbench::controls::combo(
                    "module",
                    current,
                    false,
                    move |menu, _, _| {
                        modules.iter().fold(menu, |menu, (id, name)| {
                            let (this, id) = (this.clone(), *id);
                            menu.item(PopupMenuItem::new(name.clone()).on_click(
                                move |_, window, cx| {
                                    this.update(cx, |this, cx| {
                                        this.module = Some(id);
                                        this.refresh(window, cx);
                                    })
                                    .ok();
                                },
                            ))
                        })
                    },
                ))
            }
            ScopeTab::Directory => Some(
                h_flex()
                    .flex_1()
                    .gap(px(4.))
                    .child(div().flex_1().child(
                        spork_workbench::TextField::new(&self.directory).aria_label("Directory"),
                    ))
                    .child(
                        ActionButton::new("recursive", "actions/showAsTree")
                            .tooltip("Recursively")
                            .selected(self.options.recursive)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.set_option(
                                    |options| options.recursive = !options.recursive,
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .into_any_element(),
            ),
            ScopeTab::Scope => {
                let this = cx.entity().downgrade();
                Some(spork_workbench::controls::combo(
                    "scope",
                    self.options.named.title(),
                    false,
                    move |menu, _, _| {
                        NamedScope::ALL.into_iter().fold(menu, |menu, named| {
                            let this = this.clone();
                            menu.item(PopupMenuItem::new(named.title()).on_click(
                                move |_, window, cx| {
                                    this.update(cx, |this, cx| {
                                        this.set_option(
                                            move |options| options.named = named,
                                            window,
                                            cx,
                                        )
                                    })
                                    .ok();
                                },
                            ))
                        })
                    },
                ))
            }
        };
        h_flex()
            .flex_none()
            .h(px(40.))
            .px(px(12.))
            .gap(px(6.))
            .child(h_flex().gap(px(4.)).children(tabs))
            .children(control.map(|control| div().flex_1().ml(px(8.)).child(control)))
            .into_any_element()
    }

    fn footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let none = self.occurrences.is_empty();
        let hint =
            spork_workbench::popup::keystrokes_text(&[gpui::KeybindingKeystroke::from_keystroke(
                gpui::Keystroke::parse("cmd-enter").unwrap_or_default(),
            )]);
        h_flex()
            .flex_none()
            .h(px(44.))
            .px(px(16.))
            .gap(px(12.))
            .border_t_1()
            .border_color(tokens.popup_border)
            .child(
                spork_workbench::controls::labeled_check(
                    "new-tab",
                    "Open results in new tab",
                    self.options.new_tab,
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.options.new_tab = !this.options.new_tab;
                    cx.notify();
                })),
            )
            .child(div().flex_1())
            .child(div().text_color(tokens.text_secondary).child(hint))
            .child(
                spork_workbench::Button::new("open-in-find-window", "Open in Find Window")
                    .disabled(self.text(cx).is_empty())
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_in_find_window(window, cx)),
                    ),
            )
            .when(self.replace, |footer| {
                footer
                    .child(
                        spork_workbench::Button::new("replace-all", "Replace All")
                            .disabled(none)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.replace_all(window, cx)),
                            ),
                    )
                    .child(
                        spork_workbench::Button::new("replace", "Replace")
                            .disabled(none)
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.replace_selected(window, cx)
                                }),
                            ),
                    )
            })
            .into_any_element()
    }
}

impl Render for FindInFiles {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let options = h_flex()
            .gap(px(2.))
            .child(
                ActionButton::new("new-line", "inline/newLine")
                    .tooltip("New Line")
                    .on_click(cx.listener(|this, _, window, cx| {
                        let text = format!("{}\n", this.text(cx));
                        this.search
                            .update(cx, |search, cx| search.set_value(text, window, cx));
                        this.refresh(window, cx);
                    })),
            )
            .child(self.option(
                "match-case",
                "inline/matchCase",
                "Match Case",
                self.options.match_case,
                |options| options.match_case = !options.match_case,
                cx,
            ))
            .child(self.option(
                "words",
                "inline/exactWords",
                "Words",
                self.options.words,
                |options| options.words = !options.words,
                cx,
            ))
            .child(self.option(
                "regex",
                "inline/regex",
                "Regex",
                self.options.regex,
                |options| options.regex = !options.regex,
                cx,
            ))
            .into_any_element();
        let search_row = self.field_row(self.search.clone(), "Search", options, cx);
        let replace_row = self.replace.then(|| {
            let preserve = h_flex()
                .gap(px(2.))
                .child(self.option(
                    "preserve-case",
                    "inline/preserveCase",
                    "Preserve case",
                    self.options.preserve_case,
                    |options| options.preserve_case = !options.preserve_case,
                    cx,
                ))
                .into_any_element();
            self.field_row(self.replacement.clone(), "Replace", preserve, cx)
        });
        let empty_text = match (&self.error, self.text(cx).is_empty(), self.searching) {
            (Some(_), _, _) => Some("Bad pattern"),
            (None, true, _) => Some("Type search query to find in files"),
            (None, false, true) if self.occurrences.is_empty() => Some("Searching…"),
            (None, false, false) if self.occurrences.is_empty() => Some("Nothing found"),
            _ => None,
        };
        let body = match empty_text {
            Some(text) => div()
                .id("find-empty")
                .role(Role::Label)
                .aria_label(text)
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(tokens.text_secondary)
                .child(text)
                .into_any_element(),
            None => {
                let list = uniform_list(
                    "occurrences",
                    self.occurrences.len(),
                    cx.processor(|this, range, _, cx| this.render_rows(range, cx)),
                )
                .track_scroll(&self.scroll)
                .size_full();
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("results")
                            .role(Role::ListBox)
                            .aria_label("Results")
                            .flex_none()
                            .h(gpui::relative(RESULTS_SHARE))
                            .py(px(4.))
                            .child(list),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .border_t_1()
                            .border_color(tokens.popup_border)
                            .child(self.preview.render(PreviewLayout::Below, cx)),
                    )
                    .into_any_element()
            }
        };
        let title = match self.replace {
            true => "Replace in Files",
            false => "Find in Files",
        };
        let height = px(HEIGHT).min(window.viewport_size().height - px(TOP + 40.));
        // the modal layer puts a popup 5 rems from the top; IntelliJ, 115 px
        let top = (px(TOP) - window.rem_size() * 5.).max(px(0.));
        spork_vcs::spork_look(cx).pt(top).child(
            popup::surface(cx)
                .id("find-in-files")
                .role(Role::Dialog)
                .aria_label(title)
                .w(px(WIDTH))
                .h(height)
                .key_context("FindInFiles")
                .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                    cx.stop_propagation();
                    this.step(-1, window, cx)
                }))
                .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                    cx.stop_propagation();
                    this.step(1, window, cx)
                }))
                .capture_action(cx.listener(|_, _: &Escape, _, cx| {
                    cx.stop_propagation();
                    cx.emit(DismissEvent)
                }))
                .capture_key_down(cx.listener(Self::key_down))
                .on_action(cx.listener(|this, _: &menu::SelectPrevious, window, cx| {
                    this.step(-1, window, cx)
                }))
                .on_action(
                    cx.listener(|this, _: &menu::SelectNext, window, cx| this.step(1, window, cx)),
                )
                .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| {
                    this.go_to(this.selected, window, cx)
                }))
                .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
                .child(self.header(cx))
                .child(search_row)
                .children(replace_row)
                .child(self.scope_row(cx))
                .child(body)
                .child(self.footer(cx)),
        )
    }
}
