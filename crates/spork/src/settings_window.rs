//! Settings, as IntelliJ's (docs/reference-spec.md §7): a window of its own, the categories down
//! the left under a search field, the page on the right under its breadcrumbs, then Cancel /
//! Apply / OK. Only settings Spork has are shown; IntelliJ hides empty groups the same way.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::button::Button;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Root, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::RepoView;
use crate::notices::Notification;
use crate::prefs::{Appearance, Prefs};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Appearance,
    System,
    Font,
    Commit,
    Git,
    GitHub,
    Terminal,
    ExternalTools,
    DiffMerge,
    Ai,
}

/// The pages Spork had before the whole tree, by path; they keep their own builders.
const SPORK_PAGES: &[(&str, Page)] = &[
    ("Appearance & Behavior/Appearance", Page::Appearance),
    ("Appearance & Behavior/System Settings", Page::System),
    ("Editor/Font", Page::Font),
    ("Version Control/Commit", Page::Commit),
    ("Version Control/Git", Page::Git),
    ("Version Control/GitHub", Page::GitHub),
    ("Tools/Terminal", Page::Terminal),
    ("Tools/External Tools", Page::ExternalTools),
    ("Tools/Diff & Merge", Page::DiffMerge),
    ("Tools/AI Assistant", Page::Ai),
];

/// A page of IntelliJ's tree: its name, words a search also finds it by, IntelliJ's mark for
/// settings kept per project, and the pages under it.
pub(crate) struct Node {
    pub(crate) name: &'static str,
    pub(crate) words: &'static str,
    pub(crate) project: bool,
    pub(crate) children: &'static [Node],
}

impl Node {
    pub(crate) const fn page(name: &'static str) -> Self {
        Node {
            name,
            words: "",
            project: false,
            children: &[],
        }
    }

    pub(crate) const fn group(name: &'static str, children: &'static [Node]) -> Self {
        Node {
            name,
            words: "",
            project: false,
            children,
        }
    }

    pub(crate) const fn words(mut self, words: &'static str) -> Self {
        self.words = words;
        self
    }

    pub(crate) const fn per_project(mut self) -> Self {
        self.project = true;
        self
    }
}

/// The reference app's Settings tree, in its order (docs/reference-spec.md §7); AI Assistant is Spork's.
const TREE: &[Node] = &[
    Node::group(
        "Appearance & Behavior",
        &[
            Node::page("Appearance")
                .words("theme dark light sync os font size scroll bars compact"),
            Node::page("Accessibility"),
            Node::page("Menus and Toolbars"),
            Node::group("System Settings", crate::settings_behavior::SYSTEM_SETTINGS)
                .per_project()
                .words("project folders repositories search everywhere startup"),
            Node::page("File Colors").per_project(),
            Node::page("Scopes").per_project(),
            Node::page("Notifications"),
            Node::page("Quick Lists"),
            Node::page("Required Plugins").per_project(),
            Node::page("Trusted Locations"),
            Node::page("Path Variables").per_project(),
            Node::page("Presentation Assistant"),
        ],
    ),
    Node::page("Keymap").words("shortcuts keys bindings"),
    Node::group(
        "Editor",
        &[
            Node::group("General", crate::settings_editor::GENERAL)
                .words("soft wrap whitespaces tab width diff"),
            Node::page("Code Editing"),
            Node::page("Font").words("editor font size code mono"),
            Node::group("Color Scheme", crate::settings_editor::COLOR_SCHEME),
            Node::group("Code Style", crate::settings_code::CODE_STYLE),
            Node::page("Inspections").per_project(),
            Node::page("File and Code Templates"),
            Node::page("File Encodings").per_project(),
            Node::page("Live Templates"),
            Node::page("File Types"),
            Node::page("Inlay Hints"),
            Node::page("Emmet"),
            Node::page("Intentions"),
            Node::page("Reader Mode").per_project(),
            Node::page("TextMate Bundles"),
        ],
    ),
    Node::page("Plugins"),
    Node::group("Version Control", crate::settings_vcs::VERSION_CONTROL).per_project(),
    Node::page("Build Tools").per_project(),
    Node::group(
        "Languages & Frameworks",
        &[
            Node::group("Schemas and DTDs", crate::settings_tools::SCHEMAS).per_project(),
            Node::page("Template Data Languages").per_project(),
        ],
    )
    .per_project(),
    Node::group(
        "Tools",
        &[
            Node::page("Actions on Save").per_project(),
            Node::group("Diff & Merge", crate::settings_tools::DIFF_MERGE)
                .words("ignore whitespace side by side split"),
            Node::page("External Tools").words("editor open custom commands"),
            Node::page("Startup Tasks").per_project(),
            Node::page("Terminal")
                .per_project()
                .words("external terminal app open"),
            Node::page("Web Browsers and Preview"),
            Node::page("AI Assistant").words("ai model key anthropic"),
        ],
    ),
    Node::page("Backup and Sync"),
    Node::page("Advanced Settings"),
];

/// The editor font's size Spork uses when none is set.
const DEFAULT_CODE_SIZE: u32 = 13;

/// IntelliJ's tab size for text files: Spork's diffs leave tabs as they are at it.
const DEFAULT_TAB_SIZE: u32 = 4;

/// The groups the tree starts with open, as the reference app's.
const OPEN_AT_FIRST: [&str; 4] = [
    "Appearance & Behavior",
    "Editor",
    "Languages & Frameworks",
    "Tools",
];

/// Settings Spork keeps by name, in `<config dir>/settings.conf` as `key=value` lines: the
/// pages' values Spork has no field of its own for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Store(std::collections::BTreeMap<String, String>);

impl Store {
    fn file() -> Option<std::path::PathBuf> {
        Some(crate::repos::config_dir()?.join("settings.conf"))
    }

    pub(crate) fn load() -> Self {
        let text = Self::file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .unwrap_or_default();
        Self::parse(&text)
    }

    fn parse(text: &str) -> Self {
        Store(
            text.lines()
                .filter_map(|l| l.split_once('='))
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
        )
    }

    fn render(&self) -> String {
        self.0.iter().map(|(k, v)| format!("{k}={v}\n")).collect()
    }

    fn save(&self) {
        use crate::util::ResultExt as _;
        if let Some(file) = Self::file() {
            std::fs::create_dir_all(file.parent().unwrap_or(&file)).log_err();
            std::fs::write(file, self.render()).log_err();
        }
    }

    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    pub(crate) fn set(&mut self, key: &str, value: &str) {
        self.0.insert(key.to_owned(), value.replace('\n', " "));
    }

    /// Changes one setting from outside the Settings window (the Commit tool window's Display
    /// as), keeping the others.
    pub(crate) fn save_one(key: &str, value: &str) {
        let mut store = Self::load();
        store.set(key, value);
        store.save();
    }

    fn remove(&mut self, key: &str) {
        self.0.remove(key);
    }
}

thread_local! {
    /// The open Settings window: asking again brings it to the front.
    static OPEN: RefCell<Option<AnyWindowHandle>> = const { RefCell::new(None) };
}

/// Opens Settings with `prefs`, or brings the open one to the front. `project`: the project's
/// view and name when opened from one (not from the Welcome window); called from the view's own
/// update, so it doesn't read the view.
pub(crate) fn open(
    project: Option<(Entity<RepoView>, String)>,
    prefs: Prefs,
    page: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let open = OPEN.with_borrow(|w| *w);
    if let Some(handle) = open
        && handle
            .update(cx, |root, window, cx| {
                window.activate_window();
                if let (Some(page), Ok(settings)) = (&page, root.downcast::<Root>()) {
                    let view = settings.read(cx).view().clone();
                    if let Ok(settings) = view.downcast::<SettingsWindow>() {
                        settings.update(cx, |this, cx| this.show(page, cx));
                    }
                }
            })
            .is_ok()
    {
        return;
    }
    let title: SharedString = match &project {
        Some((_, name)) => format!("Settings – {name}").into(),
        None => "Settings".into(),
    };
    let view = project.map(|(view, _)| (view.downgrade(), window.window_handle()));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(None, size(px(900.), px(700.)), cx)),
        window_min_size: Some(size(px(900.), px(700.))),
        titlebar: Some(TitlebarOptions {
            title: Some(title.clone()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.), px(8.))),
        }),
        ..Default::default()
    };
    // the first frame draws now, and a page showing the project's folder reads its view: wait
    // for the view's own update to end
    cx.defer(move |cx| {
        match gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| {
                let mut settings = SettingsWindow::new(view, title, prefs, window, cx);
                if let Some(page) = &page {
                    settings.show(page, cx);
                }
                settings
            })
        }) {
            Ok((handle, _)) => OPEN.with_borrow_mut(|w| *w = Some(handle)),
            Err(e) => eprintln!("failed to open Settings: {e}"),
        }
    });
}

/// The page the open Settings window shows, for the end-to-end tests.
pub(crate) fn shown_page(cx: &mut App) -> Option<String> {
    let handle = OPEN.with_borrow(|w| *w)?;
    handle
        .update(cx, |root, _, cx| {
            let view = root.downcast::<Root>().ok()?.read(cx).view().clone();
            Some(
                view.downcast::<SettingsWindow>()
                    .ok()?
                    .read(cx)
                    .page
                    .clone(),
            )
        })
        .ok()?
}

/// A part of the tree's page builder: the page at a path when it is one of its own.
type PageBuilder =
    fn(&mut SettingsWindow, &str, &mut Window, &mut Context<SettingsWindow>) -> Option<Div>;

/// A choice in a combo box: its label, whether it is the current one, and what picking it sets.
type Choice = (SharedString, bool, Box<dyn Fn(&mut Prefs)>);

pub(crate) struct SettingsWindow {
    /// The project Settings was opened from, and its window: Accounts…, External Tools… and
    /// the AI settings open there. None from the Welcome window.
    project: Option<(WeakEntity<RepoView>, AnyWindowHandle)>,
    title: SharedString,
    /// As last applied, and as being edited.
    saved: Prefs,
    draft: Prefs,
    /// The page shown, as its path in the tree ("Editor/Font"), and the groups open.
    page: String,
    open: std::collections::HashSet<String>,
    /// The by-name settings as last applied and as being edited, and the text fields that
    /// edit some of them.
    values_saved: Store,
    values: Store,
    /// With each, its default: a field left at it doesn't count as a change.
    inputs: std::collections::HashMap<&'static str, (Entity<InputState>, String)>,
    search: Entity<InputState>,
    ui_size: Entity<InputState>,
    code_size: Entity<InputState>,
    tab_width: Entity<InputState>,
    fetch: Entity<InputState>,
    folders: Entity<InputState>,
    terminal: Entity<InputState>,
    editor: Entity<InputState>,
    git_name: Entity<InputState>,
    git_email: Entity<InputState>,
    /// git's global user.name and user.email as last saved.
    identity: (String, String),
    fonts: Rc<Vec<SharedString>>,
    /// The tree's focus: IntelliJ greys its selection while the search has the focus.
    tree_focus: FocusHandle,
    /// The tree's scroll, and whether it is to bring the page shown into view.
    tree_scroll: ScrollHandle,
    reveal: bool,
    /// Drawn inside another app's Settings dialog (the IDE's): only the page shows, and applying
    /// leaves the look to that app.
    embedded: bool,
    _subscriptions: Vec<Subscription>,
}

impl SettingsWindow {
    fn new(
        project: Option<(WeakEntity<RepoView>, AnyWindowHandle)>,
        title: SharedString,
        prefs: Prefs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut field = |value: String, placeholder: &str| {
            let placeholder = placeholder.to_owned();
            cx.new(|cx| {
                let mut state = InputState::new(window, cx).placeholder(placeholder);
                state.set_value(value, window, cx);
                state
            })
        };
        let identity = (
            spork_git::config::get_global("user.name").unwrap_or_default(),
            spork_git::config::get_global("user.email").unwrap_or_default(),
        );
        let search = field(String::new(), "");
        let ui_size = field(prefs.text_size().to_string(), "13");
        // IntelliJ shows the size with its decimal; 0 is Spork's default
        let code_size = field(
            format!(
                "{:.1}",
                match prefs.code_font_size {
                    0 => DEFAULT_CODE_SIZE,
                    n => n,
                } as f32
            ),
            "",
        );
        let tab_width = field(
            match prefs.tab_width {
                0 => DEFAULT_TAB_SIZE.to_string(),
                n => n.to_string(),
            },
            "",
        );
        let fetch = field(prefs.fetch_minutes.to_string(), "0");
        let folders = field(prefs.source_folders.clone(), "~/Projects; ~/Work");
        let terminal = field(prefs.terminal.clone(), "Terminal, iTerm, Warp, Ghostty");
        let editor = field(
            prefs.editor.clone(),
            "Visual Studio Code, Zed, Sublime Text",
        );
        let git_name = field(identity.0.clone(), "Your name");
        let git_email = field(identity.1.clone(), "you@example.com");
        // any edit can enable Apply; the search narrows the tree
        let _subscriptions = [
            &search, &ui_size, &code_size, &tab_width, &fetch, &folders, &terminal, &editor,
            &git_name, &git_email,
        ]
        .into_iter()
        .map(|state| {
            cx.subscribe(state, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify()
                }
            })
        })
        .collect();
        let mut fonts: Vec<SharedString> = cx
            .text_system()
            .all_font_names()
            .into_iter()
            .filter(|f| !f.starts_with('.'))
            .map(SharedString::from)
            .collect();
        fonts.sort();
        fonts.dedup();
        Self {
            project,
            title,
            saved: prefs.clone(),
            draft: prefs,
            page: "Appearance & Behavior/Appearance".to_owned(),
            open: OPEN_AT_FIRST.iter().map(|g| (*g).to_owned()).collect(),
            values_saved: Store::load(),
            values: Store::load(),
            inputs: Default::default(),
            search,
            ui_size,
            code_size,
            tab_width,
            fetch,
            folders,
            terminal,
            editor,
            git_name,
            git_email,
            identity,
            fonts: Rc::new(fonts),
            tree_focus: cx.focus_handle(),
            tree_scroll: ScrollHandle::new(),
            reveal: true,
            embedded: false,
            _subscriptions,
        }
    }

    /// The pages without the window around them, for another app's Settings dialog.
    pub(crate) fn embedded(
        project: Option<(WeakEntity<RepoView>, AnyWindowHandle)>,
        prefs: Prefs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut settings = Self::new(project, SharedString::default(), prefs, window, cx);
        settings.embedded = true;
        settings
    }

    fn text(&self, state: &Entity<InputState>, cx: &App) -> String {
        state.read(cx).value().trim().to_owned()
    }

    /// The preferences as the page shows them now.
    fn collect(&self, cx: &App) -> Prefs {
        let mut p = self.draft.clone();
        let number = |state: &Entity<InputState>| self.text(state, cx).parse::<u32>().unwrap_or(0);
        p.ui_font_size = match number(&self.ui_size) {
            13 => 0,
            n if (9..=24).contains(&n) => n,
            _ => p.ui_font_size,
        };
        p.code_font_size = match self.text(&self.code_size, cx).parse::<f32>() {
            Ok(size) if size.round() as u32 == DEFAULT_CODE_SIZE => 0,
            Ok(size) if (6.0..=72.0).contains(&size) => size.round() as u32,
            _ => p.code_font_size,
        };
        p.tab_width = match number(&self.tab_width) {
            DEFAULT_TAB_SIZE => 0,
            n => n,
        };
        p.fetch_minutes = number(&self.fetch);
        p.source_folders = self.text(&self.folders, cx);
        p.terminal = self.text(&self.terminal, cx);
        p.editor = self.text(&self.editor, cx);
        p
    }

    /// The by-name settings as the pages show them now.
    fn collected_values(&self, cx: &App) -> Store {
        let mut values = self.values.clone();
        for (key, (state, default)) in &self.inputs {
            let text = self.text(state, cx);
            if values.get(key).is_some() || text != *default {
                values.set(key, &text);
            }
        }
        values
    }

    pub(crate) fn modified(&self, cx: &App) -> bool {
        self.collect(cx) != self.saved
            || self.collected_values(cx) != self.values_saved
            || self.text(&self.git_name, cx) != self.identity.0
            || self.text(&self.git_email, cx) != self.identity.1
    }

    pub(crate) fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prefs = self.collect(cx);
        prefs.save();
        if let Some(view) = self.project.as_ref().and_then(|(view, _)| view.upgrade()) {
            let applied = prefs.clone();
            view.update(cx, |this, cx| {
                this.prefs = applied;
                this.redisplay_diffs(cx);
                this.scan_source_folders(cx);
                cx.notify();
            });
        }
        if !self.embedded {
            prefs.apply_appearance(window, cx);
            cx.refresh_windows();
        }
        // git's global identity, written only when changed
        let identity = (
            self.text(&self.git_name, cx),
            self.text(&self.git_email, cx),
        );
        for (key, new, old) in [
            ("user.name", &identity.0, &self.identity.0),
            ("user.email", &identity.1, &self.identity.1),
        ] {
            if !new.is_empty()
                && new != old
                && let Err(e) = spork_git::config::set_global(key, new)
            {
                window.push_notification(Notification::error(e.to_string()), cx);
            }
        }
        self.identity = identity;
        self.saved = prefs.clone();
        self.draft = prefs;
        let values = self.collected_values(cx);
        values.save();
        self.values_saved = values.clone();
        self.values = values;
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, _: &mut Context<Self>) {
        OPEN.with_borrow_mut(|w| *w = None);
        window.remove_window();
    }

    /// Runs `action` in the project's window, brought to the front.
    pub(crate) fn in_project(&self, action: Box<dyn Action>, cx: &mut App) {
        if let Some((_, window)) = &self.project {
            window
                .update(cx, |_, window, cx| {
                    window.activate_window();
                    window.dispatch_action(action, cx)
                })
                .ok();
        }
    }

    /// A button that runs `action` in the project's window; off without a project.
    fn project_button(
        &self,
        id: &'static str,
        label: &'static str,
        action: fn() -> Box<dyn Action>,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .outline()
            .label(label)
            .disabled(self.project.is_none())
            .on_click(cx.listener(move |this, _, _, cx| this.in_project(action(), cx)))
    }

    /// The category tree, narrowed to what the search finds.
    /// The tree down the left: groups open and close; the search shows what matches, and the
    /// groups around it, open.
    fn tree(&mut self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let query = self.text(&self.search, cx).to_lowercase();
        let focused = self.tree_focus.is_focused(window);
        let mut rows = Vec::new();
        let selected = self.tree_rows(TREE, "", 0, &query, focused, &mut rows, cx);
        // IntelliJ scrolls the page shown into view with the two rows after it
        if std::mem::take(&mut self.reveal)
            && let Some(ix) = selected
        {
            self.tree_scroll
                .scroll_to_item((ix + 2).min(rows.len().saturating_sub(1)));
        }
        v_flex()
            .id("settings-tree")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.tree_scroll)
            .track_focus(&self.tree_focus)
            .children(rows)
    }

    #[allow(clippy::too_many_arguments)]
    fn tree_rows(
        &self,
        nodes: &'static [Node],
        parent: &str,
        depth: usize,
        query: &str,
        focused: bool,
        rows: &mut Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let mut shown = None;
        for node in nodes {
            if !query.is_empty() && !matches(node, query) {
                continue;
            }
            let path = match parent {
                "" => node.name.to_owned(),
                parent => format!("{parent}/{}", node.name),
            };
            let has_children = !node.children.is_empty();
            let open = has_children && (!query.is_empty() || self.open.contains(&path));
            let selected = self.page == path;
            if selected {
                shown = Some(rows.len());
            }
            let target = path.clone();
            rows.push(
                spork_workbench::controls::settings_tree_row(
                    format!("node-{path}"),
                    node.name,
                    depth,
                    has_children.then_some(open),
                    selected,
                    focused,
                    false,
                    cx,
                )
                .when(node.project, |r| {
                    r.child(spork_workbench::controls::settings_project_mark(
                        SharedString::from(format!("project-{path}")),
                        cx,
                    ))
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&this.tree_focus, cx);
                    // a click shows the page and opens its group; again, closes the group
                    if has_children {
                        match this.page == target && this.open.contains(&target) {
                            true => this.open.remove(&target),
                            false => this.open.insert(target.clone()),
                        };
                    }
                    this.page = target.clone();
                    cx.notify();
                }))
                .into_any_element(),
            );
            if open {
                shown = shown.or(self.tree_rows(
                    node.children,
                    &path,
                    depth + 1,
                    query,
                    focused,
                    rows,
                    cx,
                ));
            }
        }
        shown
    }

    fn breadcrumbs(&self) -> String {
        trail(&self.page)
            .iter()
            .map(|node| node.name)
            .collect::<Vec<_>>()
            .join(" › ")
    }

    /// Shows the page at `path`, its groups open (a link to another page).
    pub(crate) fn show(&mut self, path: &str, cx: &mut Context<Self>) {
        let mut parent = String::new();
        for node in trail(path) {
            if !parent.is_empty() {
                self.open.insert(parent.clone());
                parent.push('/');
            }
            parent.push_str(node.name);
        }
        // IntelliJ opens the page's own children too
        self.open.insert(path.to_owned());
        self.page = path.to_owned();
        self.reveal = true;
        cx.notify();
    }

    /// The node at `path`.
    fn node(path: &str) -> Option<&'static Node> {
        trail(path).last().copied()
    }

    /// A checkbox and its label; a click flips what `get` reads with `set`.
    fn check(
        &self,
        id: &'static str,
        label: &'static str,
        get: fn(&Prefs) -> bool,
        set: fn(&mut Prefs, bool),
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let on = get(&self.draft);
        spork_workbench::controls::labeled_check(id, label, on, cx).on_click(cx.listener(
            move |this, _, _, cx| {
                set(&mut this.draft, !on);
                cx.notify();
            },
        ))
    }

    /// A combo box of `choices`; picking one changes the draft.
    fn combo(
        &self,
        id: &'static str,
        current: SharedString,
        disabled: bool,
        choices: impl Fn() -> Vec<Choice> + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let this = cx.entity();
        spork_workbench::controls::combo(id, current, disabled, move |menu, _, _| {
            choices()
                .into_iter()
                .fold(menu, |menu, (label, checked, pick)| {
                    let this = this.clone();
                    menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                        move |_, _, cx| {
                            this.update(cx, |this, cx| {
                                pick(&mut this.draft);
                                cx.notify();
                            })
                        },
                    ))
                })
        })
    }

    fn page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let path = self.page.clone();
        let builders: [PageBuilder; 5] = [
            Self::behavior_settings,
            Self::editor_settings,
            Self::code_settings,
            Self::vcs_settings,
            Self::tools_settings,
        ];
        if let Some(page) = SPORK_PAGES
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, page)| *page)
        {
            return match page {
                Page::Appearance => self.appearance_page(cx),
                Page::System => self.system_page(window, cx),
                Page::Font => self.font_page(window, cx),
                Page::Commit => self.commit_page(cx),
                Page::Git => self.git_page(window, cx),
                Page::GitHub => self.github_page(window, cx),
                Page::Terminal => self.terminal_page(cx),
                Page::ExternalTools => self.tools_page(cx),
                Page::DiffMerge => self.diff_page(cx),
                Page::Ai => self.ai_page(cx),
            };
        }
        for build in builders {
            if let Some(page) = build(self, &path, window, cx) {
                return page;
            }
        }
        self.children_page(&path, cx)
    }

    /// A group without settings of its own says what it holds and lists its pages as links, as
    /// IntelliJ's.
    fn children_page(&self, path: &str, cx: &mut Context<Self>) -> Div {
        let description = match path {
            "Appearance & Behavior" => Some(
                "Customize IDE appearance and behavior: change themes and font size, tune the keymap, configure plugins and system settings, such as password policies, HTTP proxy, and updates.",
            ),
            "Editor" => Some(
                "Personalize source code appearance by changing fonts, highlighting styles, indents, etc. Customize the Editor from line numbers, caret placement and tabs to source code inspections, setting up templates and file encodings.",
            ),
            "Languages & Frameworks" => Some(
                "Configure the settings related to specific frameworks and technologies used in your project",
            ),
            "Tools" => Some(
                "Configure integration with third-party applications, specify the SSH Terminal connection settings, manage server certificates and tasks, configure diagrams layout, etc.",
            ),
            _ => None,
        };
        let children = Self::node(path).map_or(&[][..], |n| n.children);
        let links = children.iter().map(|child| {
            let target = format!("{path}/{}", child.name);
            spork_workbench::controls::link(SharedString::from(target.clone()), child.name, cx)
                .on_click(cx.listener(move |this, _, _, cx| this.show(&target, cx)))
        });
        v_flex()
            .gap(px(7.))
            .children(description)
            .child(v_flex().pl(px(24.)).gap(px(1.)).children(links))
    }

    /// The repository of the project Settings was opened from; None from the Welcome window.
    pub(crate) fn project_root(&self, cx: &App) -> Option<std::path::PathBuf> {
        let (view, _) = self.project.as_ref()?;
        view.upgrade()?.read(cx).tab_root()
    }

    /// Sets a by-name setting, for controls that change several at once.
    pub(crate) fn set_value(&mut self, key: &str, value: &str) {
        self.values.set(key, value);
    }

    /// Puts a by-name setting back to its default.
    pub(crate) fn reset_value(&mut self, key: &str) {
        self.values.remove(key);
        self.inputs.remove(key);
    }

    /// A by-name setting's value, else `default`.
    pub(crate) fn value(&self, key: &str, default: &str) -> String {
        self.values.get(key).unwrap_or(default).to_owned()
    }

    pub(crate) fn flag(&self, key: &str, default: bool) -> bool {
        match self.values.get(key) {
            Some(value) => value == "true",
            None => default,
        }
    }

    /// A checkbox for the by-name setting `key`.
    pub(crate) fn opt_check(
        &self,
        key: &'static str,
        label: &'static str,
        default: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let on = self.flag(key, default);
        spork_workbench::controls::labeled_check(key, label, on, cx).on_click(cx.listener(
            move |this, _, _, cx| {
                this.values.set(key, if on { "false" } else { "true" });
                cx.notify();
            },
        ))
    }

    /// Editor ▸ General ▸ Appearance's Show whitespaces, which is Spork's diffs' own.
    pub(crate) fn whitespace_check(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        self.check(
            "whitespace",
            "Show whitespaces",
            |prefs| prefs.show_whitespace,
            |prefs, on| prefs.show_whitespace = on,
            cx,
        )
    }

    pub(crate) fn shows_whitespace(&self) -> bool {
        self.draft.show_whitespace
    }

    /// Code Style ▸ Other File Types' Tab size, which Spork's diffs show tabs at.
    pub(crate) fn tab_size_field(&self) -> Div {
        field(&self.tab_width, 48.)
    }

    /// [`Self::opt_check`] greyed: the reference app shows it, but it can't be changed here.
    pub(crate) fn opt_check_disabled(
        &self,
        key: &'static str,
        label: &'static str,
        default: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        spork_workbench::controls::disabled_check(key, label, self.flag(key, default), cx)
    }

    /// A combo box of `choices` for `key`.
    pub(crate) fn opt_combo(
        &self,
        key: &'static str,
        choices: &'static [&'static str],
        default: &'static str,
        width: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let current = self.value(key, default);
        let this = cx.entity();
        div().w(px(width)).child(spork_workbench::controls::combo(
            key,
            SharedString::from(current.clone()),
            false,
            move |menu, _, _| {
                choices.iter().fold(menu, |menu, choice| {
                    let this = this.clone();
                    menu.item(
                        PopupMenuItem::new(*choice)
                            .checked(*choice == current)
                            .on_click(move |_, _, cx| {
                                this.update(cx, |this, cx| {
                                    this.values.set(key, choice);
                                    cx.notify();
                                })
                            }),
                    )
                })
            },
        ))
    }

    /// Radio buttons, one a line, for `key`.
    pub(crate) fn opt_radio(
        &self,
        key: &'static str,
        choices: &'static [&'static str],
        default: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let current = self.value(key, default);
        v_flex().gap_1().children(choices.iter().map(|choice| {
            spork_workbench::controls::labeled_radio(
                SharedString::from(format!("{key}-{choice}")),
                *choice,
                *choice == current,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.values.set(key, choice);
                cx.notify();
            }))
        }))
    }

    /// A text field `width` px wide for `key`.
    pub(crate) fn opt_text(
        &mut self,
        key: &'static str,
        default: &str,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        if !self.inputs.contains_key(key) {
            let value = self.value(key, default);
            let state = cx.new(|cx| {
                let mut state = InputState::new(window, cx);
                state.set_value(value, window, cx);
                state
            });
            self._subscriptions
                .push(cx.subscribe(&state, |_, _, e: &InputEvent, cx| {
                    if matches!(e, InputEvent::Change) {
                        cx.notify()
                    }
                }));
            self.inputs.insert(key, (state, default.to_owned()));
        }
        match self.inputs.get(key) {
            Some((state, _)) => field(state, width),
            None => div(),
        }
    }

    /// [`Self::opt_text`] greyed and read-only while `disabled`, as IntelliJ's fields that
    /// follow a checkbox.
    pub(crate) fn opt_text_enabled(
        &mut self,
        key: &'static str,
        default: &str,
        width: f32,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let field = self.opt_text(key, default, width, window, cx);
        match self.inputs.get(key) {
            Some((state, _)) if !enabled => div()
                .w(px(width))
                .child(spork_workbench::TextField::new(state).disabled(true)),
            _ => field,
        }
    }

    /// [`Self::opt_radio`] on one line, as IntelliJ's `buttonsGroup` in a row.
    pub(crate) fn inline_radios(
        &self,
        key: &'static str,
        choices: &'static [&'static str],
        default: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        h_flex().gap_4().children(
            choices
                .iter()
                .map(|choice| self.opt_radio(key, std::slice::from_ref(choice), default, cx)),
        )
    }

    /// [`Self::opt_text_enabled`] with IntelliJ's browse button inside it, which picks a folder
    /// (or a file with `files`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn opt_path(
        &mut self,
        key: &'static str,
        default: &str,
        width: f32,
        enabled: bool,
        files: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        self.opt_text(key, default, width, window, cx);
        let Some((state, _)) = self.inputs.get(key) else {
            return div();
        };
        let browse = spork_workbench::ActionButton::new(
            SharedString::from(format!("{key}-browse")),
            "general/open",
        )
        .tooltip("Browse… (⇧⏎)")
        .disabled(!enabled)
        .on_click({
            let state = state.clone();
            cx.listener(move |_, _, window, cx| {
                let picked = cx.prompt_for_paths(PathPromptOptions {
                    files,
                    directories: !files,
                    multiple: false,
                    prompt: None,
                });
                let state = state.clone();
                cx.spawn_in(window, async move |_, cx| {
                    if let Ok(Ok(Some(mut paths))) = picked.await
                        && let Some(folder) = paths.pop()
                    {
                        state
                            .update_in(cx, |state, window, cx| {
                                state.set_value(folder.to_string_lossy().into_owned(), window, cx)
                            })
                            .ok();
                    }
                })
                .detach();
            })
        });
        div().w(px(width)).child(
            spork_workbench::TextField::new(state)
                .disabled(!enabled)
                .suffix(browse),
        )
    }

    /// A button for a dialog Spork doesn't have yet: a click says so.
    pub(crate) fn opt_button(
        &self,
        id: &'static str,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        h_flex().child(
            spork_workbench::Button::new(id, label).on_click(cx.listener(
                move |_, _, window, cx| {
                    window.push_notification(
                        Notification::info(format!(
                            "{} isn't in Spork yet.",
                            label.trim_end_matches('…')
                        )),
                        cx,
                    )
                },
            )),
        )
    }

    fn appearance_page(&self, cx: &mut Context<Self>) -> Div {
        let content = v_flex().gap_3();
        let sync = self.draft.appearance == Appearance::System;
        let dark = match self.draft.appearance {
            Appearance::Dark => true,
            Appearance::Light => false,
            Appearance::System => cx.theme().mode.is_dark(),
        };
        let theme_combo = self.combo(
            "theme",
            if dark {
                "Islands Dark"
            } else {
                "Islands Light"
            }
            .into(),
            sync,
            move || {
                vec![
                    (
                        "Islands Dark".into(),
                        dark,
                        Box::new(|prefs: &mut Prefs| prefs.appearance = Appearance::Dark)
                            as Box<dyn Fn(&mut Prefs)>,
                    ),
                    (
                        "Islands Light".into(),
                        !dark,
                        Box::new(|prefs: &mut Prefs| prefs.appearance = Appearance::Light),
                    ),
                ]
            },
            cx,
        );
        let custom = !self.draft.ui_font.is_empty();
        let fonts = self.fonts.clone();
        let current = self.draft.ui_font.clone();
        let font_combo = self.combo(
            "ui-font",
            if custom {
                current.clone().into()
            } else {
                "System font".into()
            },
            !custom,
            move || {
                fonts
                    .iter()
                    .map(|f| {
                        let name = f.to_string();
                        (
                            f.clone(),
                            *f == current,
                            Box::new(move |prefs: &mut Prefs| prefs.ui_font = name.clone())
                                as Box<dyn Fn(&mut Prefs)>,
                        )
                    })
                    .collect()
            },
            cx,
        );
        let (indent, column) = (px(20.), px(290.));
        let theme_gear = spork_workbench::ActionButton::new("theme-gear", "general/settings")
            .tooltip("Preferred Themes")
            .dropdown_menu(|menu, _, _| {
                menu.label("Preferred light theme")
                    .item(PopupMenuItem::new("Islands Light").checked(true))
                    .separator()
                    .label("Preferred dark theme")
                    .item(PopupMenuItem::new("Islands Dark").checked(true))
            });
        let left_options = v_flex()
            .w(column)
            .gap_2()
            .child(self.opt_check("appearance.compact", "Compact mode", false, cx))
            .child(comment("UI elements take up less screen space", cx))
            .child(self.opt_check(
                "appearance.fullPathInTitle",
                "Always show full path in window header",
                false,
                cx,
            ))
            .child(self.opt_check(
                "appearance.projectColors",
                "Use project colors in main toolbar",
                true,
                cx,
            ))
            .child(comment(
                "Distinguish projects with different toolbar colors at a glance.",
                cx,
            ))
            .child(self.opt_check(
                "appearance.keepPopupsOpen",
                "Keep popups open for toggle items",
                true,
                cx,
            ))
            .child(self.check(
                "scrollbars",
                "Always show scroll bars",
                |prefs| prefs.scrollbars_always,
                |prefs, on| prefs.scrollbars_always = on,
                cx,
            ))
            .child(self.opt_button("background-image", "Background Image…", cx));
        let right_options = v_flex()
            .gap_2()
            .child(self.opt_check(
                "appearance.dndWithAlt",
                "Drag-and-drop with Alt pressed only",
                false,
                cx,
            ))
            .child(self.opt_check("appearance.smoothScrolling", "Smooth scrolling", true, cx))
            .child(self.opt_check(
                "appearance.mnemonicsInControls",
                "Enable mnemonics in controls",
                true,
                cx,
            ))
            .child(self.opt_check(
                "appearance.mnemonicsInMenu",
                "Enable mnemonics in menu",
                false,
                cx,
            ))
            .child(self.opt_check(
                "appearance.iconsInMenus",
                "Display icons in menu items",
                true,
                cx,
            ));
        let tool_windows = v_flex()
            .w(column)
            .gap_2()
            .child(self.opt_check(
                "appearance.toolWindowBars",
                "Show tool window bars",
                true,
                cx,
            ))
            .child(self.opt_check(
                "appearance.toolWindowNames",
                "Show tool window names",
                false,
                cx,
            ))
            .child(self.opt_check(
                "appearance.sideBySideLeft",
                "Side-by-side layout on the left",
                false,
                cx,
            ))
            .child(self.opt_check(
                "appearance.sideBySideRight",
                "Side-by-side layout on the right",
                false,
                cx,
            ))
            .child(self.opt_check(
                "appearance.widescreen",
                "Widescreen tool window layout",
                false,
                cx,
            ))
            .child(self.opt_check(
                "appearance.rememberSizes",
                "Remember size for each tool window",
                false,
                cx,
            ));
        let tokens = spork_workbench::Tokens::get(cx);
        // IntelliJ's picture of the layout: the toolbar, the side tool windows, the editor,
        // the bottom tool window
        let preview = v_flex()
            .flex_1()
            .h(px(150.))
            .border_1()
            .border_color(tokens.control_border)
            .bg(tokens.frame)
            .child(div().h(px(8.)).bg(tokens.selection))
            .child(
                h_flex()
                    .flex_1()
                    .p(px(6.))
                    .gap(px(4.))
                    .child(div().w(relative(0.2)).h_full().bg(tokens.island))
                    .child(div().flex_1().h_full().bg(tokens.control_bg_raised))
                    .child(div().w(relative(0.2)).h_full().bg(tokens.island)),
            )
            .child(div().h(px(36.)).mx(px(6.)).mb(px(6.)).bg(tokens.island));
        content
            .child(
                h_flex()
                    .gap_3()
                    .child(labeled("Theme:", theme_combo))
                    .child(self.check(
                        "sync-os",
                        "Sync with OS",
                        |prefs| prefs.appearance == Appearance::System,
                        |prefs, on| {
                            prefs.appearance = match on {
                                true => Appearance::System,
                                false => Appearance::Dark,
                            }
                        },
                        cx,
                    ))
                    .child(theme_gear),
            )
            .child(
                h_flex()
                    .pl(indent)
                    .gap_3()
                    .child(labeled(
                        "Editor color scheme:",
                        self.opt_combo(
                            "appearance.editorScheme",
                            crate::settings_editor::SCHEMES,
                            if dark { "Islands Dark" } else { "Light" },
                            228.,
                            cx,
                        )
                        .into_any_element(),
                    ))
                    .child(
                        spork_workbench::ActionButton::new("scheme-gear", "general/settings")
                            .tooltip("Editor ▸ Color Scheme")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.page = "Editor/Color Scheme".to_owned();
                                this.open.insert("Editor".to_owned());
                                cx.notify();
                            })),
                    ),
            )
            .child(
                v_flex()
                    .pl(indent)
                    .gap_1()
                    .child(self.opt_check(
                        "appearance.differentToolWindowBackground",
                        "Different tool window background",
                        false,
                        cx,
                    ))
                    .child(comment(
                        "Use lighter color in the dark theme and darker color in the light theme as a background",
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(self.check(
                        "custom-font",
                        "Use custom font:",
                        |prefs| !prefs.ui_font.is_empty(),
                        |prefs, on| prefs.ui_font = if on { "Inter".into() } else { String::new() },
                        cx,
                    ))
                    .child(font_combo)
                    .child(labeled(
                        "Size:",
                        field(&self.ui_size, 64.).into_any_element(),
                    )),
            )
            .child(titled("UI Options", cx))
            .child(h_flex().pl(indent).items_start().child(left_options).child(right_options))
            .child(titled("Tree Views", cx))
            .child(
                h_flex()
                    .pl(indent)
                    .child(div().w(column).child(self.opt_check(
                        "appearance.indentGuides",
                        "Show indent guides",
                        false,
                        cx,
                    )))
                    .child(self.opt_check(
                        "appearance.smallerIndents",
                        "Use smaller indents",
                        false,
                        cx,
                    )),
            )
            .child(titled("Tool Windows", cx))
            .child(h_flex().pl(indent).items_start().gap_4().child(tool_windows).child(preview))
            .child(titled("Presentation Mode", cx))
            .child(h_flex().pl(indent).child(labeled(
                "Zoom:",
                self.opt_combo(
                    "appearance.presentationZoom",
                    &["100%", "125%", "150%", "175%", "200%", "250%", "300%", "400%"],
                    "175%",
                    100.,
                    cx,
                )
                .into_any_element(),
            )))
            .child(titled("Antialiasing", cx))
            .child(
                h_flex()
                    .pl(indent)
                    .child(div().w(column).child(labeled(
                        "IDE:",
                        self.opt_combo(
                            "appearance.antialiasingIde",
                            &["Subpixel", "Greyscale", "No antialiasing"],
                            "Greyscale",
                            160.,
                            cx,
                        )
                        .into_any_element(),
                    )))
                    .child(labeled(
                        "Editor:",
                        self.opt_combo(
                            "appearance.antialiasingEditor",
                            &["Subpixel", "Greyscale", "No antialiasing"],
                            "Greyscale",
                            160.,
                            cx,
                        )
                        .into_any_element(),
                    )),
            )
    }

    /// The reference app's System Settings, then Spork's own folders to find repositories in.
    fn system_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let indent = px(20.);
        let idle = self.flag("system.saveOnIdle", false);
        let process = self.inline_radios(
            "system.processCloseConfirmation",
            &["Terminate process", "Disconnect", "Ask"],
            "Ask",
            cx,
        );
        let open_in = self.inline_radios(
            "system.openProjectIn",
            &["New window", "Current window", "Ask"],
            "Ask",
            cx,
        );
        let project = v_flex()
            .pl(indent)
            .gap_3()
            .child(self.opt_check(
                "system.reopenProjects",
                "Reopen projects on startup",
                true,
                cx,
            ))
            .child(h_flex().gap_4().child("Open project in").child(open_in))
            .child(labeled(
                "Default project directory:",
                self.opt_path(
                    "system.defaultProjectDirectory",
                    "",
                    360.,
                    true,
                    false,
                    window,
                    cx,
                )
                .into_any_element(),
            ))
            .child(div().pl(px(200.)).child(comment(
                "This directory is preselected in \"Open...\" and \"New | Project...\" dialogs.",
                cx,
            )))
            .child(
                h_flex()
                    .gap_2()
                    .child(self.opt_check(
                        "system.storeInProjectRoot",
                        "Store project settings in the project root directory",
                        true,
                        cx,
                    ))
                    .child(help_mark(
                        "system-store-in-root-help",
                        "Disable this if you don't want Spork to generate or respect .idea directories in the root of your projects.",
                        cx,
                    ))
                    .child(app_badge(cx)),
            )
            .child(comment("Requires restart", cx))
            .child(
                h_flex()
                    .gap_2()
                    .child(self.opt_check(
                        "system.indexing",
                        "Enable indexing files for search",
                        true,
                        cx,
                    ))
                    .child(help_mark(
                        "system-indexing-help",
                        "Disable this if Spork is causing performance issues when indexing. Useful if you use a separate IDE and don't need Spork's search features.",
                        cx,
                    ))
                    .child(app_badge(cx)),
            );
        let files = v_flex()
            .pl(indent)
            .gap_3()
            .child(self.opt_check(
                "system.moveToBin",
                "Move files to the bin instead of deleting permanently",
                true,
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(self.opt_check(
                        "system.saveOnIdle",
                        "Save files if Spork is idle for",
                        false,
                        cx,
                    ))
                    .child(self.opt_text_enabled("system.idleSeconds", "15", 56., idle, window, cx))
                    .child("seconds"),
            )
            .child(self.opt_check(
                "system.saveOnDeactivation",
                "Save files when switching to a different application or a built-in terminal",
                true,
                cx,
            ))
            .child(self.opt_check("system.backup", "Back up files before saving", true, cx))
            .child("Sync external changes:")
            .child(
                v_flex()
                    .pl(indent)
                    .gap_3()
                    .child(self.opt_check(
                        "system.syncOnActivation",
                        "When switching to the Spork window or opening an editor tab",
                        true,
                        cx,
                    ))
                    .child(self.opt_check(
                        "system.syncPeriodically",
                        "Periodically when Spork is inactive (experimental)",
                        true,
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(comment("Autosave cannot be disabled completely.", cx))
                    .child(
                        spork_workbench::controls::link("system-autosave-help", "How it works", cx)
                            .text_xs()
                            .on_click(|_, _, cx| {
                                cx.open_url("https://www.jetbrains.com/help/idea/?autosave")
                            }),
                    ),
            );
        v_flex()
            .gap_3()
            .child(self.opt_check(
                "system.confirmExit",
                "Confirm before exiting Spork",
                true,
                cx,
            ))
            .child(
                h_flex()
                    .gap_4()
                    .child("When closing a tool window with a running process:")
                    .child(process),
            )
            .child(titled("Project", cx))
            .child(project)
            .child(titled("Files", cx))
            .child(files)
            .child(titled("Repositories", cx))
            .child(
                v_flex()
                    .pl(indent)
                    .gap_2()
                    .child(labeled(
                        "Search for repositories in:",
                        field(&self.folders, 360.).into_any_element(),
                    ))
                    .child(comment("Search Everywhere (⌘P) also finds the repositories in these folders; separate them with ;", cx)),
            )
    }

    /// The reference app's Editor ▸ Font: Spork's code font and size, then the line height, ligatures and
    /// the preview.
    fn font_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let fonts = self.fonts.clone();
        let current = self.draft.code_font.clone();
        let shown: SharedString = match current.is_empty() {
            true => "JetBrains Mono".into(),
            false => current.clone().into(),
        };
        let font_combo = self.combo(
            "code-font",
            shown,
            false,
            move || {
                fonts
                    .iter()
                    .map(|f| {
                        let name = f.to_string();
                        (
                            f.clone(),
                            *f == current,
                            Box::new(move |prefs: &mut Prefs| prefs.code_font = name.clone())
                                as Box<dyn Fn(&mut Prefs)>,
                        )
                    })
                    .collect()
            },
            cx,
        );
        let tokens = spork_workbench::Tokens::get(cx);
        let line_height = self.opt_text("editorFont.lineHeight", "1.2", 64., window, cx);
        let typography_open = self.flag("editorFont.typographyOpen", false);
        let options = v_flex()
            .w(px(300.))
            .flex_none()
            .gap_3()
            .child(labeled("Font:", div().w(px(230.)).child(font_combo).into_any_element()))
            .child(
                h_flex()
                    .gap_2()
                    .child("Size:")
                    .child(field(&self.code_size, 64.))
                    .child(div().pl_2().child("Line height:"))
                    .child(line_height),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(self.opt_check("editorFont.ligatures", "Enable ligatures", false, cx))
                    .child(help_mark(
                        "editor-font-ligatures-help",
                        "Use several characters as a single ligature. Ligatures enhance code readability. Use the section below to preview the changes",
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .gap_1()
                    .text_color(tokens.text_muted)
                    .child("See line height and ligatures also in")
                    .child(
                        spork_workbench::controls::link("editor-font-reader-mode", "Reader mode", cx)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show("Editor/Reader Mode", cx)
                            })),
                    ),
            )
            .child(
                h_flex()
                    .id("editor-font-typography")
                    .pt_4()
                    .gap_1()
                    .cursor_pointer()
                    .child(
                        spork_workbench::icon(match typography_open {
                            true => "general/chevronDown",
                            false => "general/chevronRight",
                        })
                        .size(px(16.))
                        .text_color(tokens.icon),
                    )
                    .child("Typography Settings")
                    .child(div().flex_1().h(px(1.)).bg(tokens.separator))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_value(
                            "editorFont.typographyOpen",
                            if typography_open { "false" } else { "true" },
                        );
                        cx.notify();
                    })),
            )
            .when(typography_open, |options| {
                options.child(
                    v_flex()
                        .pl(px(20.))
                        .gap_2()
                        .child(labeled(
                            "Main font weight:",
                            self.opt_combo("editorFont.mainWeight", &["Regular"], "Regular", 140., cx)
                                .into_any_element(),
                        ))
                        .child(labeled(
                            "Bold font weight:",
                            self.opt_combo("editorFont.boldWeight", &["Bold"], "Bold", 140., cx)
                                .into_any_element(),
                        ))
                        .child(labeled(
                            "Fallback font:",
                            self.opt_combo("editorFont.fallback", &["<None>"], "<None>", 140., cx)
                                .into_any_element(),
                        )),
                )
            });
        let dark = self.dark_scheme(cx);
        h_flex().items_start().gap_4().child(options).child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(crate::settings_editor::preview(
                    crate::settings_editor::FONT_PREVIEW,
                    dark,
                    self.flag("editorFont.ligatures", false),
                    cx,
                ))
                .child(comment("Enter any text to preview", cx)),
        )
    }

    fn commit_page(&self, cx: &mut Context<Self>) -> Div {
        self.vcs_commit_page(cx)
    }

    /// The reference app's Git page, then Spork's own: how often it fetches, and git's identity.
    fn git_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let content = self.git_settings(window, cx);
        content
            .child(titled("Fetch", cx))
            .child(
                h_flex()
                    .gap_2()
                    .child("Fetch remote changes automatically every")
                    .child(field(&self.fetch, 56.))
                    .child("minutes"),
            )
            .child(comment("0: never.", cx))
            .child(titled("Author", cx))
            .child(labeled(
                "Name:",
                field(&self.git_name, 260.).into_any_element(),
            ))
            .child(labeled(
                "Email:",
                field(&self.git_email, 260.).into_any_element(),
            ))
            .child(comment(
                "git's global user.name and user.email: every commit records them.",
                cx,
            ))
    }

    fn terminal_page(&self, cx: &mut Context<Self>) -> Div {
        let content = v_flex().gap_3();
        content
            .child(labeled(
                "Open In › Terminal uses:",
                field(&self.terminal, 320.).into_any_element(),
            ))
            .child(comment("An app name; empty: the system's Terminal.", cx))
    }

    fn tools_page(&self, cx: &mut Context<Self>) -> Div {
        let content = v_flex().gap_3();
        content
            .child(labeled(
                "Open In › Editor uses:",
                field(&self.editor, 320.).into_any_element(),
            ))
            .child(comment("An app name, as in /Applications.", cx))
            .child(titled("Commands", cx))
            .child(self.project_button(
                "custom-commands",
                "External Tools…",
                || Box::new(crate::shell::CustomCommands),
                cx,
            ))
    }

    fn diff_page(&self, cx: &mut Context<Self>) -> Div {
        let content = v_flex().gap_3();
        content
            .child(self.check(
                "ignore-ws",
                "Ignore whitespace changes",
                |prefs| prefs.ignore_whitespace,
                |prefs, on| prefs.ignore_whitespace = on,
                cx,
            ))
            .child(self.check(
                "split",
                "Side-by-side viewer",
                |prefs| prefs.split_diffs,
                |prefs, on| prefs.split_diffs = on,
                cx,
            ))
    }

    fn ai_page(&self, cx: &mut Context<Self>) -> Div {
        let content = v_flex().gap_3();
        let muted = cx.theme().muted_foreground;
        content
            .child(div().text_color(muted).child(
                "Commit messages, reviews and explanations written by a model you choose; Spork shows what it sends first.",
            ))
            .child(self.project_button("ai-settings", "Configure…", || Box::new(crate::shell::AiSettings), cx))
    }
}

/// Shows the page at `path` in the Settings window a click came from: for links built where
/// only the window is at hand.
pub(crate) fn show_page(path: &str, window: &mut Window, cx: &mut App) {
    let Some(Some(root)) = window.root::<Root>() else {
        return;
    };
    let view = root.read(cx).view().clone();
    if let Ok(settings) = view.downcast::<SettingsWindow>() {
        settings.update(cx, |this, cx| this.show(path, cx));
    }
}

/// The nodes from the top of the tree down to `path`; a page's own name may hold a "/"
/// ("HTML/CSS"), so each step takes the child whose whole name comes next.
fn trail(path: &str) -> Vec<&'static Node> {
    let mut nodes = TREE;
    let mut rest = path;
    let mut found = Vec::new();
    while !rest.is_empty() {
        let Some(node) = nodes.iter().find(|n| {
            rest == n.name
                || rest
                    .strip_prefix(n.name)
                    .is_some_and(|r| r.starts_with('/'))
        }) else {
            break;
        };
        found.push(node);
        rest = rest[node.name.len()..].trim_start_matches('/');
        nodes = node.children;
    }
    found
}

/// Whether `node` or a page under it is what the search looks for.
fn matches(node: &Node, query: &str) -> bool {
    node.name.to_lowercase().contains(query)
        || node.words.contains(query)
        || node.children.iter().any(|child| matches(child, query))
}

/// A setting's comment: small grey text under it, lined up with its label.
pub(crate) fn comment(text: &'static str, cx: &App) -> Div {
    div()
        .pl(px(22.))
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

/// A text field `width` px wide.
pub(crate) fn field(state: &Entity<InputState>, width: f32) -> Div {
    div()
        .w(px(width))
        .child(spork_workbench::TextField::new(state))
}

/// A label and its control on one line.
pub(crate) fn labeled(label: &'static str, control: AnyElement) -> Div {
    h_flex().gap_2().child(label).child(control)
}

/// IntelliJ's context help: a "?" whose tooltip says more about the setting beside it.
pub(crate) fn help_mark(id: &'static str, text: &'static str, cx: &App) -> impl IntoElement {
    div()
        .id(id)
        .role(Role::Image)
        .aria_label(text)
        // IntelliJ's contextHelp, the New UI's question mark in a circle
        .child(spork_workbench::image("general/questionMark", cx).size(px(16.)))
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(text).build(window, cx)
        })
}

/// IntelliJ's badge on a project page's setting that is the app's.
fn app_badge(cx: &App) -> Div {
    let tokens = spork_workbench::Tokens::get(cx);
    div()
        .px(px(6.))
        .rounded(px(8.))
        .bg(tokens.control_bg_raised)
        .text_xs()
        .text_color(tokens.text_secondary)
        .child("Spork setting")
}

/// IntelliJ's mark beside a page's breadcrumbs when its settings are the project's, its
/// explanation in a tooltip.
fn project_mark(cx: &App) -> impl IntoElement {
    const TEXT: &str = "Settings marked with this icon are only applied to the current project. Non-marked settings are applied to all projects.";
    div()
        .id("project-settings-mark")
        .role(Role::Image)
        .aria_label(TEXT)
        .child(
            spork_workbench::icon("general/projectConfigurable")
                .size(px(16.))
                .text_color(spork_workbench::Tokens::get(cx).icon),
        )
        .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new(TEXT).build(window, cx))
}

/// A group's title with a line after it, as IntelliJ's titled separator.
pub(crate) fn titled(title: &'static str, cx: &App) -> Div {
    h_flex()
        .pt_2()
        .gap_2()
        .child(title)
        .child(div().flex_1().h(px(1.)).bg(cx.theme().border))
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.embedded {
            return self.page(window, cx).into_any_element();
        }
        let theme = cx.theme();
        let (bg, fg, border) = (theme.background, theme.foreground, theme.border);
        let left = match theme.mode.is_dark() {
            true => rgb(0x191A1C),
            false => rgb(0xF7F8F9),
        };
        let modified = self.modified(cx);
        let tree = self.tree(window, cx);
        let page = self.page(window, cx);
        v_flex()
            .size_full()
            .bg(bg)
            .text_color(fg)
            .text_sm()
            .child(spork_workbench::shell::window_title(self.title.clone(), cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        v_flex()
                            .w(relative(0.2))
                            .min_w(px(200.))
                            .h_full()
                            .bg(left)
                            .border_r_1()
                            .border_color(border)
                            .child(
                                div().pt(px(7.)).px(px(5.)).pb(px(6.)).child(
                                    spork_workbench::TextField::new(&self.search).prefix(
                                        spork_workbench::icon("general/search").size(px(16.)),
                                    ),
                                ),
                            )
                            .child(tree.vertical_scrollbar(&self.tree_scroll)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(
                                h_flex()
                                    .pt(px(11.))
                                    .pl(px(16.))
                                    .pr(px(10.))
                                    .pb_2()
                                    .gap_2()
                                    .font_weight(FontWeight::BOLD)
                                    .child(self.breadcrumbs())
                                    .when(
                                        Self::node(&self.page).is_some_and(|node| node.project),
                                        |row| row.child(project_mark(cx)),
                                    ),
                            )
                            .child(
                                div()
                                    .id("settings-page")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scrollbar()
                                    .px(px(16.))
                                    .py_2()
                                    .child(page),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .px(px(12.))
                    .py(px(8.))
                    .gap_2()
                    .border_t_1()
                    .border_color(border)
                    .child(
                        // IntelliJ's round "?" help button
                        div()
                            .id("settings-help")
                            .size(px(24.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .border_1()
                            .border_color(spork_workbench::Tokens::get(cx).control_border)
                            .cursor_pointer()
                            .child("?")
                            .on_click(|_, _, cx| cx.open_url("https://github.com/medmdime/Spork")),
                    )
                    .child(div().flex_1())
                    .child(
                        spork_workbench::Button::new("settings-cancel", "Cancel")
                            .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    )
                    .child(
                        spork_workbench::Button::new("settings-apply", "Apply")
                            .disabled(!modified)
                            .on_click(cx.listener(|this, _, window, cx| this.apply(window, cx))),
                    )
                    .child(
                        spork_workbench::Button::new("settings-ok", "OK")
                            .primary(true)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.apply(window, cx);
                                this.close(window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::trail;

    #[test]
    fn a_name_with_a_slash_is_one_step() {
        let names: Vec<&str> = trail("Editor/General/Smart Keys/HTML/CSS")
            .iter()
            .map(|node| node.name)
            .collect();
        assert_eq!(names, ["Editor", "General", "Smart Keys", "HTML/CSS"]);
        assert!(trail("Nowhere").is_empty());
    }
}
