//! IntelliJ's Settings dialog (ROADMAP, phase 9) over the IDE's one settings store: the search
//! field over the tree of groups, the page under its breadcrumbs, and Cancel / Apply / OK with
//! IntelliJ's semantics (changes wait for Apply or OK, Cancel drops them, Apply is enabled only
//! while something changed, a changed page is marked in the tree). Its pages write Zed's
//! settings (`settings.json`), the settings Spork IDE's own crates keep ([`OWNED`]), the user
//! keymap (`keymap.json`) and the extensions (shown as plugins); Version Control's are Spork's
//! own, as its tool windows read them. Built from Spork's workbench, after Spork's Settings
//! window. Which of IntelliJ's pages and options it has, and why the others are left out:
//! `docs/spork-settings-pages.md`.

mod export_import;
mod file_types;
mod keymap;
mod pages;
mod plugins;

pub use export_import::{ExportSettings, ImportSettings};
pub use keymap::add_shortcut;
pub use plugins::CheckSuggestedPlugins;

use anyhow::Context as _;
use collections::HashSet;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Root, Selectable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;
use settings::{RootUserSettings as _, SettingsContent, SettingsStore};
use spork_workbench::{Tokens, Typography as _};
use util::ResultExt as _;
use workspace::Workspace;

use pages::Row;

/// What a page of the tree shows.
#[derive(Clone, Copy)]
enum Content {
    /// A group's own page: what it holds, then its pages as links.
    Group(&'static str),
    /// Options over the IDE's settings.
    Options(&'static [Row]),
    Keymap,
    Plugins,
    /// Editor › File Types' two tabs.
    FileTypes,
    /// One of Spork's pages, at its path in Spork's tree.
    Spork(&'static str),
    /// Build, Execution, Deployment › Docker: `spork_docker`'s connections.
    Docker,
    /// A page another crate draws ([`register_page`]), by its path in the tree.
    External(&'static str),
}

/// A page another crate draws in the tree (spork_java's Kotlin Compiler), made for the project
/// the dialog opens on, and applied with the dialog.
pub trait ExternalPage {
    fn view(&self) -> AnyView;
    fn modified(&self, cx: &App) -> bool;
    fn apply(&self, cx: &mut App);
    fn e2e_state(&self, cx: &App) -> Value;
}

/// How an [`ExternalPage`] is made: for the project's first folder; `changed` tells the dialog
/// of an edit.
pub type MakePage = fn(
    Option<std::path::PathBuf>,
    std::rc::Rc<dyn Fn(&mut App)>,
    &mut Window,
    &mut App,
) -> std::rc::Rc<dyn ExternalPage>;

#[derive(Default)]
struct ExternalPages(Vec<(&'static str, MakePage)>);

impl Global for ExternalPages {}

/// Lets another crate draw the page at `path` (`KOTLIN_COMPILER_PAGE`).
pub fn register_page(path: &'static str, make: MakePage, cx: &mut App) {
    cx.default_global::<ExternalPages>().0.push((path, make));
}

/// Build, Execution, Deployment › Compiler › Kotlin Compiler, which spork_java draws.
pub const KOTLIN_COMPILER_PAGE: &str = "Build, Execution, Deployment/Compiler/Kotlin Compiler";

/// A page of IntelliJ's tree, with the pages under it.
struct Node {
    name: &'static str,
    content: Content,
    /// Words a search also finds a custom page by (its options' labels).
    words: &'static str,
    children: &'static [Node],
    /// The project's settings, not the IDE's: marked in the tree and the breadcrumbs.
    project: bool,
}

impl Node {
    const fn new(name: &'static str, content: Content) -> Self {
        Node {
            name,
            content,
            words: "",
            children: &[],
            project: false,
        }
    }

    const fn project(mut self) -> Self {
        self.project = true;
        self
    }

    const fn children(mut self, children: &'static [Node]) -> Self {
        self.children = children;
        self
    }

    const fn words(mut self, words: &'static str) -> Self {
        self.words = words;
        self
    }
}

/// IntelliJ's tree, in its order, with the groups and pages Spork IDE backs with a setting.
const TREE: &[Node] = &[
    Node::new(
        "Appearance & Behavior",
        Content::Group(
            "Customize IDE appearance and behavior: change themes and font size, tune the keymap, configure plugins and system settings, such as password policies, HTTP proxy, and updates.",
        ),
    )
    .children(&[
        Node::new("Appearance", Content::Options(pages::APPEARANCE)),
        Node::new("Accessibility", Content::Options(pages::ACCESSIBILITY)),
        Node::new("System Settings", Content::Options(pages::SYSTEM_SETTINGS)).project(),
    ]),
    Node::new("Keymap", Content::Keymap).words(keymap::WORDS),
    Node::new(
        "Editor",
        Content::Group(
            "Personalize source code appearance by changing fonts, highlighting styles, indents, etc. Customize the Editor from line numbers, caret placement and tabs to source code inspections, setting up templates and file encodings.",
        ),
    )
    .children(&[
        Node::new("General", Content::Options(pages::EDITOR_GENERAL)).children(&[
            Node::new("Appearance", Content::Options(pages::EDITOR_APPEARANCE)),
            Node::new("Breadcrumbs", Content::Options(pages::BREADCRUMBS)),
            Node::new("Code Completion", Content::Options(pages::CODE_COMPLETION)),
            Node::new("Code Folding", Content::Options(pages::CODE_FOLDING)),
            Node::new("Editor Tabs", Content::Options(pages::EDITOR_TABS)),
            Node::new("Smart Keys", Content::Options(pages::SMART_KEYS)),
            Node::new("Sticky Lines", Content::Options(pages::STICKY_LINES)),
        ]),
        Node::new("Code Editing", Content::Options(pages::CODE_EDITING)),
        Node::new("Font", Content::Options(pages::EDITOR_FONT)),
        Node::new("Color Scheme", Content::Options(pages::COLOR_SCHEME)),
        Node::new("Code Style", Content::Options(pages::CODE_STYLE)).children(&[Node::new(
            "Other File Types",
            Content::Options(pages::OTHER_FILE_TYPES),
        )]),
        Node::new("File Types", Content::FileTypes).words(file_types::WORDS),
        Node::new("Intentions", Content::Options(pages::INTENTIONS)),
    ]),
    Node::new("Plugins", Content::Plugins).words(plugins::WORDS),
    Node::new(
        "Version Control",
        Content::Group("Configure the settings related to version control used in your project"),
    )
    .project()
    .children(&[
        Node::new("Changelists", Content::Spork("Version Control/Changelists"))
            .project().words(
            "create changelists automatically allow putting changes within one file into different \
             changelists inactive changelist highlight files from inactive changelists show dialog \
             on attempt to edit file when an empty changelist becomes inactive conflicts highlight \
             files with changelist conflicts files with ignored conflicts clear",
        ),
        Node::new("Commit", Content::Spork("Version Control/Commit"))
            .project()
            .words("clear initial commit message commit message inspections severity"),
        Node::new("Confirmation", Content::Spork("Version Control/Confirmation"))
            .project().words(
            "confirmation when files are created when files are deleted show options before \
             update ask for confirmation to drop commits changes check for conflicts with the \
             server highlight files changed in the last highlight directories that contain \
             modified files when patch is created restore workspace when switching branches limit \
             history gutter highlight modified lines",
        ),
        Node::new(
            "Directory Mappings",
            Content::Spork("Version Control/Directory Mappings"),
        )
        .project()
        .words("directory vcs enable automatic mapping detection"),
        Node::new(
            "File Status Colors",
            Content::Spork("Version Control/File Status Colors"),
        )
        .words("added deleted modified ignored unknown merged conflicts customized"),
        Node::new("Log", Content::Spork("Version Control/Log"))
            .project().words(
            "location show the log in the editor window view options show only the first reference \
             show tag names show the time when the change was committed references on the left \
             display changes to each merged commit separately issue preview diff preview panel \
             visible columns author hash date gpg signature indexing",
        ),
        Node::new("Shelf", Content::Spork("Version Control/Shelf"))
            .project().words(
            "remove successfully applied files from the shelf shelve base revisions change shelves \
             location",
        ),
        Node::new("Git", Content::Spork("Version Control/Git"))
            .project().words(GIT_WORDS),
        Node::new("GitHub", Content::Spork("Version Control/GitHub"))
            .project().words(
            "clone git repositories using ssh automatically mark opened files as viewed unread \
             markers pull requests connection timeout",
        ),
        Node::new("GitLab", Content::Spork("Version Control/GitLab"))
            .project()
            .words("automatically mark opened files as viewed clone using ssh"),
    ]),
    Node::new(
        "Build, Execution, Deployment",
        Content::Group(
            "Configure the project Execution settings, set up Deployment options, and customize the Debugger behavior",
        ),
    )
    .children(&[
        Node::new("Compiler", Content::Group(""))
            .project()
            .children(&[Node::new(
                "Kotlin Compiler",
                Content::External(KOTLIN_COMPILER_PAGE),
            )
            .project()
            .words(
                "report compiler warnings kotlin compiler version language version api version \
                 additional command line parameters keep compiler process alive between \
                 invocations kotlin to jvm enable incremental compilation target jvm version \
                 kotlin to javascript generate source maps embed source code into source map \
                 copy library runtime files destination directory module kind",
            )]),
        Node::new("Debugger", Content::Group("")).children(&[Node::new(
            "Data Views",
            Content::Options(pages::DATA_VIEWS),
        )]),
        Node::new("Docker", Content::Docker).words(
            "docker connection connect to docker daemon with docker for mac colima unix socket \
             tcp socket engine api url connection successful",
        ),
    ]),
    Node::new(
        "Languages & Frameworks",
        Content::Group(
            "Configure the settings related to specific frameworks and technologies used in your project",
        ),
    )
    .children(&[
        Node::new("Markdown", Content::Options(pages::MARKDOWN)),
        Node::new("Spring", Content::Options(pages::SPRING)),
    ]),
    Node::new(
        "Tools",
        Content::Group(
            "Configure integration with third-party applications, specify the SSH Terminal connection settings, manage server certificates and tasks, configure diagrams layout, etc.",
        ),
    )
    .children(&[
        Node::new("Actions on Save", Content::Options(pages::ACTIONS_ON_SAVE)).project(),
        Node::new("Terminal", Content::Options(pages::TERMINAL)).project(),
    ]),
    Node::new("Advanced Settings", Content::Options(pages::ADVANCED)),
];

/// Settings Spork IDE's own crates keep, outside `settings.json`: their path in the dialog's
/// draft, how to read them and how to apply them.
pub(crate) struct Owned {
    path: &'static str,
    get: fn(&App) -> Value,
    set: fn(&Value, &mut App) -> anyhow::Result<()>,
}

/// Advanced Settings › IDE › Duration of storing changes in Local History.
pub(crate) const LOCAL_HISTORY_DAYS: &str = "/spork/local_history/days_to_keep";

/// Languages & Frameworks › Spring › Spring Boot › Project Import › "Create run configuration
/// automatically", which `spork_java` reads on each import.
pub(crate) const SPRING_BOOT_RUN_CONFIGURATIONS: &str =
    "/spork/spring_boot/create_run_configuration";

/// The key "Create run configuration automatically" is kept under.
const SPRING_BOOT_RUN_CONFIGURATIONS_KEY: &str = "spork_spring_boot_create_run_configuration";

/// Whether an import makes a Spring Boot run configuration for each application without one
/// (on unless turned off).
pub fn spring_boot_creates_run_configurations(cx: &App) -> bool {
    db::kvp::KeyValueStore::global(cx)
        .read_kvp(SPRING_BOOT_RUN_CONFIGURATIONS_KEY)
        .log_err()
        .flatten()
        .is_none_or(|value| value != "false")
}

pub(crate) const OWNED: &[Owned] = &[
    Owned {
        path: LOCAL_HISTORY_DAYS,
        get: |cx| serde_json::json!(spork_local_history::days_to_keep(cx)),
        set: |days, cx| {
            let days = days.as_i64().context("a number of days")?;
            spork_local_history::set_days_to_keep(days, cx)
        },
    },
    Owned {
        path: SPRING_BOOT_RUN_CONFIGURATIONS,
        get: |cx| serde_json::json!(spring_boot_creates_run_configurations(cx)),
        set: |on, cx| {
            let on = on.as_bool().context("on or off")?;
            let store = db::kvp::KeyValueStore::global(cx);
            cx.background_spawn(async move {
                store
                    .write_kvp(SPRING_BOOT_RUN_CONFIGURATIONS_KEY.into(), on.to_string())
                    .await
            })
            .detach_and_log_err(cx);
            Ok(())
        },
    },
];

/// The labels of Spork's Git page, for the search.
const GIT_WORDS: &str = "path to git executable exclude ignored directories commit staging area \
    crlf line separators detached head large files names cherry-picked push auto-update rejected \
    push dialog protected branches update method merge rebase clean working tree shelve stash fetch \
    author name email";

/// The groups the tree starts with open, as IntelliJ's.
const OPEN_AT_FIRST: [&str; 2] = ["Appearance & Behavior", "Editor"];

/// The Docker page's path in the tree.
const DOCKER_PAGE: &str = "Build, Execution, Deployment/Docker";

/// The page the dialog opens on when none is asked for.
const FIRST_PAGE: &str = "Appearance & Behavior/Appearance";

gpui::actions!(
    settings_dialog,
    [
        /// Opens Settings on Editor › Editor Tabs (the editor tab row's ⋮).
        ConfigureEditorTabs,
        /// Opens Settings on Editor › General › Code Completion (the completion popup's ⋮).
        ConfigureCodeCompletion,
        /// Opens Settings on Editor › Intentions (an intention's Edit intention settings).
        ConfigureIntentions
    ]
);

pub fn init(cx: &mut App) {
    follow_ui_options(cx);
    plugins::apply_themes_of_installed_plugins(cx);
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        export_import::register(workspace);
        plugins::register(workspace);
        workspace.register_action(|workspace, _: &zed_actions::OpenSettings, window, cx| {
            open(Some(workspace), None, window, cx)
        });
        workspace.register_action(|workspace, _: &ConfigureEditorTabs, window, cx| {
            open(
                Some(workspace),
                Some(Target::Page("Editor/General/Editor Tabs".into())),
                window,
                cx,
            )
        });
        workspace.register_action(|workspace, _: &ConfigureCodeCompletion, window, cx| {
            open(
                Some(workspace),
                Some(Target::Page("Editor/General/Code Completion".into())),
                window,
                cx,
            )
        });
        workspace.register_action(|workspace, _: &ConfigureIntentions, window, cx| {
            open(
                Some(workspace),
                Some(Target::Page("Editor/Intentions".into())),
                window,
                cx,
            )
        });
        workspace.register_action(|workspace, action: &zed_actions::Extensions, window, cx| {
            let query = action.id.clone().unwrap_or_default();
            open(Some(workspace), Some(Target::Plugins(query)), window, cx)
        });
    })
    .detach();
}

/// Hands IntelliJ's UI Options from the settings to Spork's shared components (the menus, the
/// project's glow), now and whenever the settings change.
fn follow_ui_options(cx: &mut App) {
    fn apply(cx: &mut App) {
        let options = SettingsStore::global(cx)
            .merged_settings()
            .ui_options
            .clone()
            .unwrap_or_default();
        gpui_base::menu_options::set_show_icons(options.show_icons_in_menus.unwrap_or(true));
        let colors = options.use_project_colors_in_toolbar.unwrap_or(true);
        if cx
            .try_global::<spork_workbench::shell::ProjectColors>()
            .is_none_or(|current| current.0 != colors)
        {
            cx.set_global(spork_workbench::shell::ProjectColors(colors));
            cx.refresh_windows();
        }
    }
    apply(cx);
    cx.observe_global::<SettingsStore>(apply).detach();
}

/// What Spork's shared components follow of the UI Options, by the setting's path: what the app
/// does, for the end-to-end tests.
fn applied_ui_option(path: &str, cx: &App) -> Option<Value> {
    let on = match path {
        "/ui_options/show_icons_in_menus" => gpui_base::menu_options::show_icons(),
        "/ui_options/use_project_colors_in_toolbar" => cx
            .try_global::<spork_workbench::shell::ProjectColors>()
            .is_none_or(|on| on.0),
        _ => return None,
    };
    Some(Value::Bool(on))
}

/// What to show once the dialog opens.
#[derive(Clone)]
pub enum Target {
    Page(String),
    /// The Plugins page, searching its Marketplace for this.
    Plugins(String),
}

struct OpenDialog(AnyWindowHandle);

impl Global for OpenDialog {}

/// The page shown last: the dialog opens on it again, as IntelliJ's.
struct LastPage(String);

impl Global for LastPage {}

/// Opens Settings on `target` (the page shown last otherwise), or brings the open one to the front.
pub fn open(
    workspace: Option<&Workspace>,
    target: Option<Target>,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(OpenDialog(handle)) = cx.try_global::<OpenDialog>().map(|d| OpenDialog(d.0))
        && handle
            .update(cx, |root, window, cx| {
                window.activate_window();
                if let (Some(target), Some(dialog)) = (&target, dialog_of(root, cx)) {
                    dialog.update(cx, |this, cx| this.go_to(target, window, cx));
                }
            })
            .is_ok()
    {
        return;
    }
    let repo = workspace
        .and_then(|workspace| spork_vcs::repo_view(workspace, cx))
        .map(|repo| (repo, window.window_handle()));
    // IntelliJ's "Settings – project"
    let title: SharedString = workspace
        .and_then(|workspace| workspace.project().read(cx).visible_worktrees(cx).next())
        .map_or_else(
            || TITLE.into(),
            |worktree| format!("{TITLE} – {}", worktree.read(cx).root_name_str()).into(),
        );
    let root = workspace
        .and_then(|workspace| workspace.project().read(cx).visible_worktrees(cx).next())
        .map(|worktree| worktree.read(cx).abs_path().to_path_buf());
    open_window(repo, root, title, target, cx);
}

/// The project's repository view and window, for Version Control's pages.
type Repo = Option<(Entity<spork::RepoView>, AnyWindowHandle)>;

/// Opens the Settings window, pinned or not as the pin was left.
fn open_window(
    repo: Repo,
    root: Option<std::path::PathBuf>,
    title: SharedString,
    target: Option<Target>,
    cx: &mut App,
) {
    let float = pinned(cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(
            None,
            size(px(DIALOG_WIDTH), px(DIALOG_HEIGHT)),
            cx,
        )),
        window_min_size: Some(size(px(DIALOG_WIDTH), px(DIALOG_HEIGHT))),
        titlebar: Some(TitlebarOptions {
            title: Some(title.clone()),
            appears_transparent: true,
            traffic_light_position: Some(spork_workbench::controls::WINDOW_HEADER_TRAFFIC_LIGHTS),
        }),
        // IntelliJ's Float mode (pinned) stays above the IDE and can't be minimized; its Window
        // mode is a window of its own
        kind: match float {
            true => WindowKind::Floating,
            false => WindowKind::Normal,
        },
        is_minimizable: !float,
        ..Default::default()
    };
    // the first frame draws now: wait for the workspace's own update to end
    cx.defer(move |cx| {
        match gpui_kit::open_window(options, cx, move |window, cx| {
            follow_ide_theme(window, cx);
            cx.new(|cx| {
                let mut dialog = SettingsDialog::new(repo, root, title, window, cx);
                if let Some(target) = &target {
                    dialog.go_to(target, window, cx);
                }
                dialog
            })
        }) {
            Ok((handle, _)) => cx.set_global(OpenDialog(handle)),
            Err(e) => log::error!("failed to open Settings: {e}"),
        }
    });
}

/// IntelliJ's Settings window size (`SettingsNonModalDialog`'s default and least, 900×700).
const DIALOG_WIDTH: f32 = 900.;
const DIALOG_HEIGHT: f32 = 700.;
const TITLE: &str = "Settings";

/// Whether the Settings window opens pinned (IntelliJ's Float mode), as the pin of its banner
/// last left it. IntelliJ opens it pinned; GPUI's floating window is a panel that hides while
/// the IDE is in the background, which IntelliJ's doesn't, so it opens unpinned (Window mode).
// shortcut: kept while the IDE runs (IntelliJ keeps `ide.settings.window.float`), until Spork IDE
// has a store for such UI state; unpinned at first until GPUI has a window kept above its owner.
struct Pinned(bool);

impl Global for Pinned {}

fn pinned(cx: &App) -> bool {
    cx.try_global::<Pinned>().is_some_and(|pinned| pinned.0)
}

/// The dialog in a window, if it is the Settings window.
fn dialog_of(root: AnyView, cx: &App) -> Option<Entity<SettingsDialog>> {
    root.downcast::<Root>()
        .ok()?
        .read(cx)
        .view()
        .clone()
        .downcast::<SettingsDialog>()
        .ok()
}

/// The workbench's colors follow the IDE's theme, light or dark, in the dialog.
fn follow_ide_theme(window: &mut Window, cx: &mut App) {
    use theme::ActiveTheme as _;
    let mode = match cx.theme().appearance {
        theme::Appearance::Light => gpui_kit::component::ThemeMode::Light,
        theme::Appearance::Dark => gpui_kit::component::ThemeMode::Dark,
    };
    if gpui_kit::component::ActiveTheme::theme(cx).mode != mode {
        gpui_kit::component::Theme::change(mode, Some(window), cx);
    }
}

/// The settings as the dialog edits them: the user's `settings.json` as last applied and as
/// edited, as JSON, and the values a setting not in it takes.
struct Draft {
    saved: Value,
    edited: Value,
    /// The settings in force for what the user's file doesn't set (the defaults and the OS's).
    fallback: Value,
}

impl Draft {
    fn load(cx: &App) -> Self {
        let store = SettingsStore::global(cx);
        let user = store
            .raw_user_settings()
            .and_then(|user| serde_json::to_value(&*user.content).log_err())
            .unwrap_or_else(|| Value::Object(Default::default()));
        let mut fallback = serde_json::to_value(store.merged_settings())
            .log_err()
            .unwrap_or_default();
        let mut user = user;
        for owned in OWNED {
            let value = (owned.get)(cx);
            set_pointer(&mut user, owned.path, Some(value.clone()));
            set_pointer(&mut fallback, owned.path, Some(value));
        }
        Draft {
            saved: user.clone(),
            edited: user,
            fallback,
        }
    }

    /// A setting's value: as edited, else as in force.
    fn get(&self, path: &str) -> Option<&Value> {
        match self.edited.pointer(path).filter(|value| !value.is_null()) {
            Some(value) => Some(value),
            None if self.saved.pointer(path).is_some() => None,
            None => self.fallback.pointer(path).filter(|value| !value.is_null()),
        }
    }

    /// Sets a setting; set back to what it is without the user's file, it leaves the file again.
    fn set(&mut self, path: &str, value: Option<Value>) {
        let in_file = self
            .saved
            .pointer(path)
            .is_some_and(|saved| !saved.is_null());
        let value = value.filter(|value| in_file || self.fallback.pointer(path) != Some(value));
        set_pointer(&mut self.edited, path, value);
    }

    /// The theme and icon theme as set outside the dialog (a plugin's, applied as it is
    /// installed), where the dialog hasn't changed them, as IntelliJ's Appearance follows a theme
    /// plugin's.
    fn follow_themes(&mut self, cx: &App) {
        let now = Draft::load(cx);
        for path in ["/theme", "/icon_theme"] {
            if !self.changed(path) {
                let value = now.saved.pointer(path).cloned();
                set_pointer(&mut self.saved, path, value.clone());
                set_pointer(&mut self.edited, path, value);
            }
        }
    }

    fn changed(&self, path: &str) -> bool {
        let value = |json: &Value| json.pointer(path).filter(|v| !v.is_null()).cloned();
        value(&self.edited) != value(&self.saved)
    }
}

/// Sets the value at a JSON pointer, making the objects on the way; `None` removes it.
fn set_pointer(json: &mut Value, path: &str, value: Option<Value>) {
    let mut keys: Vec<&str> = path.split('/').skip(1).collect();
    let Some(last) = keys.pop() else { return };
    let mut node = json;
    for key in keys {
        let Some(object) = node.as_object_mut() else {
            return;
        };
        node = object
            .entry(key.to_owned())
            .or_insert_with(|| Value::Object(Default::default()));
        if !node.is_object() {
            *node = Value::Object(Default::default());
        }
    }
    let Some(object) = node.as_object_mut() else {
        return;
    };
    match value {
        Some(value) => {
            object.insert(last.to_owned(), value);
        }
        None => {
            object.remove(last);
        }
    }
}

/// Every page's options, by path in the tree.
fn option_pages() -> Vec<(String, &'static [Row])> {
    fn walk(nodes: &'static [Node], parent: &str, pages: &mut Vec<(String, &'static [Row])>) {
        for node in nodes {
            let path = join(parent, node.name);
            if let Content::Options(rows) = node.content {
                pages.push((path.clone(), rows));
            }
            walk(node.children, &path, pages);
        }
    }
    let mut pages = Vec::new();
    walk(TREE, "", &mut pages);
    pages
}

/// The paths of Spork's pages at and under `node`.
fn spork_pages(node: &'static Node, parent: &str) -> Vec<String> {
    let path = join(parent, node.name);
    let mut found: Vec<String> = node
        .children
        .iter()
        .flat_map(|child| spork_pages(child, &path))
        .collect();
    if matches!(node.content, Content::Spork(_)) {
        found.insert(0, path);
    }
    found
}

fn join(parent: &str, name: &str) -> String {
    match parent {
        "" => name.to_owned(),
        parent => format!("{parent}/{name}"),
    }
}

/// The nodes from the tree's root to the page at `path`.
fn trail(path: &str) -> Vec<&'static Node> {
    let mut nodes = TREE;
    let mut found = Vec::new();
    for name in path.split('/') {
        let Some(node) = nodes.iter().find(|node| node.name == name) else {
            return Vec::new();
        };
        found.push(node);
        nodes = node.children;
    }
    found
}

/// IntelliJ's Copy Settings Path (`CopySettingsPathAction`): `Settings | Editor | General`, and
/// on an option its label after the page's names.
fn settings_path(page: &str, label: Option<&str>) -> String {
    std::iter::once("Settings")
        .chain(trail(page).iter().map(|node| node.name))
        .chain(label)
        .collect::<Vec<_>>()
        .join(" | ")
}

/// The menu of a page's row in the tree, a breadcrumb or an option.
fn copy_path_menu(
    menu: gpui_kit::component::menu::PopupMenu,
    path: String,
) -> gpui_kit::component::menu::PopupMenu {
    menu.item(
        gpui_kit::component::menu::PopupMenuItem::new("Copy Settings Path").on_click(
            move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(path.clone())),
        ),
    )
}

pub struct SettingsDialog {
    /// The page shown, by its path in the tree ("Editor/Font"), and the groups open.
    page: String,
    open: HashSet<String>,
    /// The pages shown, for the banner's back and forward arrows.
    history: Vec<String>,
    history_ix: usize,
    search: Entity<InputState>,
    draft: Draft,
    /// The text fields of the options, by their setting's path.
    inputs: collections::HashMap<&'static str, Entity<InputState>>,
    fonts: std::rc::Rc<Vec<SharedString>>,
    keymap: Entity<keymap::KeymapPage>,
    plugins: Entity<plugins::PluginsPage>,
    docker: Entity<spork_docker::SettingsPage>,
    vcs: spork::ide::SettingsPages,
    /// Spork's pages changed since the last Apply, by path.
    vcs_changed: HashSet<String>,
    /// The page shown when Spork's pages last told of a change: a change told on another page
    /// comes from showing it, not from an edit.
    vcs_seen: String,
    /// The collapsible groups open, by title.
    expanded: HashSet<&'static str>,
    file_types: file_types::FileTypesState,
    tree_focus: FocusHandle,
    tree_scroll: ScrollHandle,
    reveal: bool,
    /// What the window was opened with, to open it again when it's pinned or unpinned.
    repo: Repo,
    root: Option<std::path::PathBuf>,
    /// The pages other crates draw, by their path.
    external: Vec<(&'static str, std::rc::Rc<dyn ExternalPage>)>,
    title: SharedString,
    /// The plugins the registry has a newer version of: the count on Plugins' row.
    plugin_updates: usize,
    _subscriptions: Vec<Subscription>,
}

impl SettingsDialog {
    fn new(
        repo: Repo,
        root: Option<std::path::PathBuf>,
        title: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let external = {
            let makers = cx
                .try_global::<ExternalPages>()
                .map(|pages| pages.0.clone())
                .unwrap_or_default();
            let this = cx.entity().downgrade();
            let changed: std::rc::Rc<dyn Fn(&mut App)> = std::rc::Rc::new(move |cx| {
                this.update(cx, |_, cx| cx.notify()).ok();
            });
            makers
                .into_iter()
                .map(|(path, make)| (path, make(root.clone(), changed.clone(), window, cx)))
                .collect()
        };
        let search = cx.new(|cx| InputState::new(window, cx));
        let keymap = cx.new(|cx| keymap::KeymapPage::new(window, cx));
        let plugins = cx.new(|cx| plugins::PluginsPage::new(window, cx));
        let docker = cx.new(|cx| spork_docker::SettingsPage::new(window, cx));
        let vcs = spork::ide::SettingsPages::new(repo.clone(), window, cx);
        let mut fonts: Vec<SharedString> = cx
            .text_system()
            .all_font_names()
            .into_iter()
            .filter(|font| !font.starts_with('.'))
            .map(SharedString::from)
            .collect();
        fonts.sort();
        fonts.dedup();
        let _subscriptions = vec![
            cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.reveal = true;
                    cx.notify();
                }
            }),
            cx.observe(&keymap, |_, _, cx| cx.notify()),
            cx.observe(&plugins, |_, _, cx| cx.notify()),
            cx.observe(&docker, |_, _, cx| cx.notify()),
            // noted when the page changes, not when the dialog draws: macOS doesn't draw a
            // window behind others, and Apply and the page's mark would miss the change
            vcs.observe(cx, |this, cx| this.note_vcs_changes(cx)),
            cx.observe_global_in::<theme::GlobalTheme>(window, |this, window, cx| {
                follow_ide_theme(window, cx);
                this.draft.follow_themes(cx);
            }),
            // closed by its buttons or by the window's; a pinned or unpinned window's own
            // replaces it first
            cx.on_release({
                let handle = window.window_handle();
                move |_, cx| {
                    if cx
                        .try_global::<OpenDialog>()
                        .is_some_and(|open| open.0 == handle)
                    {
                        cx.remove_global::<OpenDialog>();
                    }
                }
            }),
        ];
        plugins::count_updates(cx);
        let mut dialog = Self {
            page: String::new(),
            open: OPEN_AT_FIRST
                .iter()
                .map(|group| (*group).to_owned())
                .collect(),
            history: Vec::new(),
            history_ix: 0,
            search,
            draft: Draft::load(cx),
            inputs: Default::default(),
            fonts: std::rc::Rc::new(fonts),
            keymap,
            plugins,
            docker,
            vcs,
            vcs_changed: HashSet::default(),
            vcs_seen: String::new(),
            expanded: HashSet::default(),
            file_types: Default::default(),
            tree_focus: cx.focus_handle(),
            tree_scroll: ScrollHandle::new(),
            reveal: true,
            repo,
            root,
            external,
            title,
            plugin_updates: 0,
            _subscriptions,
        };
        let first = cx
            .try_global::<LastPage>()
            .map_or_else(|| FIRST_PAGE.to_owned(), |last| last.0.clone());
        dialog.show(&first, cx);
        dialog
    }

    fn go_to(&mut self, target: &Target, window: &mut Window, cx: &mut Context<Self>) {
        match target {
            Target::Page(path) if !trail(path).is_empty() => self.show(path, cx),
            Target::Page(_) => {}
            Target::Plugins(query) => {
                self.show("Plugins", cx);
                self.plugins.update(cx, |plugins, cx| {
                    plugins.search_marketplace(query, window, cx)
                });
            }
        }
    }

    fn query(&self, cx: &App) -> String {
        self.search.read(cx).value().trim().to_lowercase()
    }

    /// Shows the page at `path`, the groups above it open.
    fn show(&mut self, path: &str, cx: &mut Context<Self>) {
        let mut parent = String::new();
        for node in trail(path) {
            if !parent.is_empty() {
                self.open.insert(parent.clone());
                parent.push('/');
            }
            parent.push_str(node.name);
        }
        if self.page != path {
            self.history.truncate(self.history_ix + 1);
            self.history.push(path.to_owned());
            self.history_ix = self.history.len() - 1;
        }
        self.set_page(path.to_owned(), cx);
    }

    fn set_page(&mut self, path: String, cx: &mut Context<Self>) {
        match trail(&path).last().map(|node| node.content) {
            Some(Content::Spork(spork_path)) => self.vcs.show(spork_path, cx),
            // the Marketplace loads once its page shows
            Some(Content::Plugins) => self.plugins.update(cx, |plugins, cx| plugins.shown(cx)),
            _ => {}
        }
        cx.set_global(LastPage(path.clone()));
        self.page = path;
        self.reveal = true;
        cx.notify();
    }

    fn step(&mut self, back: bool, cx: &mut Context<Self>) {
        let ix = match back {
            true => self.history_ix.checked_sub(1),
            false => Some(self.history_ix + 1).filter(|ix| *ix < self.history.len()),
        };
        if let Some(ix) = ix {
            self.history_ix = ix;
            self.set_page(self.history[ix].clone(), cx);
        }
    }

    /// The pages with changes not yet applied.
    fn modified_pages(&self, cx: &App) -> Vec<String> {
        let mut modified: Vec<String> = option_pages()
            .into_iter()
            .filter(|(_, rows)| pages::paths(rows).any(|path| self.draft.changed(path)))
            .map(|(path, _)| path)
            .collect();
        if self.keymap.read(cx).modified()
            || keymap::PATHS.iter().any(|path| self.draft.changed(path))
        {
            modified.push("Keymap".to_owned());
        }
        if file_types::PATHS
            .iter()
            .any(|path| self.draft.changed(path))
        {
            modified.push(file_types::PAGE.to_owned());
        }
        if self.docker.read(cx).modified() {
            modified.push(DOCKER_PAGE.to_owned());
        }
        modified.extend(
            self.external
                .iter()
                .filter(|(_, page)| page.modified(cx))
                .map(|(path, _)| (*path).to_owned()),
        );
        if self.vcs.modified(cx) {
            modified.extend(
                TREE.iter()
                    .flat_map(|node| spork_pages(node, ""))
                    .filter(|path| self.vcs_changed.contains(path)),
            );
        }
        modified
    }

    /// Spork's pages share one draft: the page shown is the one an edit came from. Showing one of
    /// them tells of a change too, which isn't an edit.
    fn note_vcs_changes(&mut self, cx: &App) {
        let shown = self.vcs_seen != self.page;
        self.vcs_seen = self.page.clone();
        if !self.vcs.modified(cx) {
            self.vcs_changed.clear();
        } else if !shown
            && matches!(
                trail(&self.page).last().map(|node| node.content),
                Some(Content::Spork(_))
            )
        {
            self.vcs_changed.insert(self.page.clone());
        }
    }

    fn modified(&self, cx: &App) -> bool {
        !self.modified_pages(cx).is_empty()
    }

    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (owned, changes): (Vec<_>, Vec<_>) = option_pages()
            .into_iter()
            .flat_map(|(_, rows)| pages::paths(rows))
            .chain(keymap::PATHS.iter().copied())
            .chain(file_types::PATHS.iter().copied())
            .filter(|path| self.draft.changed(path))
            .map(|path| (path, self.draft.edited.pointer(path).cloned()))
            .partition(|(path, _)| OWNED.iter().any(|owned| owned.path == *path));
        for (path, value) in owned {
            if let (Some(owned), Some(value)) =
                (OWNED.iter().find(|owned| owned.path == path), value)
            {
                (owned.set)(&value, cx).log_err();
            }
        }
        if !changes.is_empty() {
            settings::update_settings_file(<dyn fs::Fs>::global(cx), cx, move |content, _| {
                let Some(mut json) = serde_json::to_value(&*content).log_err() else {
                    return;
                };
                for (path, value) in &changes {
                    set_pointer(&mut json, path, value.clone());
                }
                if let Some(applied) =
                    SettingsContent::parse_json_with_comments(&json.to_string()).log_err()
                {
                    *content = applied;
                }
            });
        }
        self.draft.saved = self.draft.edited.clone();
        self.keymap
            .update(cx, |keymap, cx| keymap.apply(window, cx));
        self.docker.update(cx, |docker, cx| docker.apply(cx));
        for (_, page) in &self.external {
            page.apply(cx);
        }
        if self.vcs.modified(cx) {
            self.vcs.apply(window, cx);
        }
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, _: &mut Context<Self>) {
        window.remove_window();
    }

    /// The tree down the left: groups open and close; the search shows the pages it finds (by
    /// name or by an option's label) and the groups around them, open.
    fn tree(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let query = self.query(cx);
        let modified = self.modified_pages(cx);
        let mut rows = Vec::new();
        let selected = self.tree_rows(TREE, "", 0, &query, &modified, &mut rows, cx);
        // IntelliJ scrolls the page shown into view with the two rows after it
        if std::mem::take(&mut self.reveal)
            && let Some(ix) = selected
        {
            self.tree_scroll
                .scroll_to_item((ix + 2).min(rows.len().saturating_sub(1)));
        }
        v_flex()
            .id("settings-tree")
            .role(Role::Tree)
            .aria_label(SharedString::from("Settings categories"))
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
        modified: &[String],
        rows: &mut Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let mut shown = None;
        for node in nodes {
            if !query.is_empty() && !matches(node, query) {
                continue;
            }
            let path = join(parent, node.name);
            let has_children = !node.children.is_empty();
            let open = has_children && (!query.is_empty() || self.open.contains(&path));
            let selected = self.page == path;
            if selected {
                shown = Some(rows.len());
            }
            let target = path.clone();
            let badge = match node.content {
                Content::Plugins if self.plugin_updates > 0 => Some(self.plugin_updates),
                _ => None,
            };
            rows.push(
                spork_workbench::controls::settings_tree_row(
                    SharedString::from(format!("node-{path}")),
                    node.name,
                    depth,
                    has_children.then_some(open),
                    selected,
                    // IntelliJ IDEA 2026.3 paints the page's row as focused whatever has the
                    // focus (`ALWAYS_PAINT_SELECTION_AS_FOCUSED`)
                    true,
                    modified.contains(&path),
                    cx,
                )
                .when(node.project, |row| {
                    row.child(spork_workbench::controls::settings_project_mark(
                        SharedString::from(format!("project-{path}")),
                        cx,
                    ))
                })
                // IntelliJ's count sits 15 px further in than the project's mark
                .when_some(badge, |row, count| {
                    row.child(spork_workbench::controls::count_badge(count, cx).mr(px(BADGE_INSET)))
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
                    this.show(&target, cx);
                }))
                .context_menu({
                    let path = settings_path(&path, None);
                    move |menu, _, _| copy_path_menu(menu, path.clone())
                })
                .into_any_element(),
            );
            if open {
                shown = shown.or(self.tree_rows(
                    node.children,
                    &path,
                    depth + 1,
                    query,
                    modified,
                    rows,
                    cx,
                ));
            }
        }
        shown
    }

    fn page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let path = self.page.clone();
        let Some(node) = trail(&path).last().copied() else {
            return div().into_any_element();
        };
        match node.content {
            Content::Group(description) => self.group_page(&path, node, description, cx),
            // Editor › Font previews the font beside its options
            Content::Options(rows) if std::ptr::eq(rows, pages::EDITOR_FONT) => h_flex()
                .size_full()
                .items_start()
                .child(
                    div()
                        .id("settings-page")
                        .role(Role::Group)
                        .aria_label(SharedString::from(PAGE_LABEL))
                        .w(px(FONT_OPTIONS_WIDTH))
                        .flex_none()
                        .pl(px(PAGE_PADDING))
                        .child(self.options_page(rows, window, cx)),
                )
                .child(self.font_preview(cx))
                .into_any_element(),
            Content::Options(rows) => self.options_page(rows, window, cx).into_any_element(),
            Content::Keymap => v_flex()
                .size_full()
                .px(px(PAGE_PADDING))
                .pb(px(PAGE_PADDING))
                .gap(px(12.))
                .child(self.keymap_picker(cx))
                .child(div().h(px(1.)).bg(Tokens::get(cx).separator))
                .child(div().flex_1().min_h_0().child(self.keymap.clone()))
                .into_any_element(),
            Content::Plugins => self.plugins.clone().into_any_element(),
            Content::FileTypes => self.file_types_page(cx),
            Content::Spork(_) => self.vcs.view().into_any_element(),
            Content::Docker => self.docker.clone().into_any_element(),
            Content::External(path) => self
                .external
                .iter()
                .find(|(page, _)| *page == path)
                .map_or_else(
                    || div().into_any_element(),
                    |(_, page)| page.view().into_any_element(),
                ),
        }
    }

    /// The keymap picked, IntelliJ's macOS keymap first: Spork IDE's base keymap.
    fn keymap_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.draft.get("/base_keymap").cloned();
        let value = |keymap: settings::BaseKeymap| {
            serde_json::to_value(Into::<settings::BaseKeymapContent>::into(keymap)).ok()
        };
        let shown = settings::BaseKeymap::OPTIONS
            .iter()
            .find(|(_, keymap)| value(*keymap) == current)
            .map_or("", |(name, _)| *name);
        let this = cx.entity();
        div()
            .w(px(KEYMAP_COMBO_WIDTH))
            .child(spork_workbench::controls::combo(
                "combo-/base_keymap",
                shown,
                false,
                move |menu, _, _| {
                    settings::BaseKeymap::OPTIONS
                        .iter()
                        .fold(menu, |menu, (name, keymap)| {
                            let this = this.clone();
                            let picked = value(*keymap);
                            menu.item(
                                gpui_kit::component::menu::PopupMenuItem::new(*name)
                                    .checked(picked == current)
                                    .on_click(move |_, _, cx| {
                                        this.update(cx, |this, cx| {
                                            this.draft.set("/base_keymap", picked.clone());
                                            cx.notify();
                                        })
                                    }),
                            )
                        })
                },
            ))
            .into_any_element()
    }

    /// A group's page says what it holds and lists its pages as links, as IntelliJ's.
    fn group_page(
        &self,
        path: &str,
        node: &'static Node,
        description: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let links = node.children.iter().map(|child| {
            let target = join(path, child.name);
            spork_workbench::controls::link(SharedString::from(target.clone()), child.name, cx)
                .on_click(cx.listener(move |this, _, _, cx| this.show(&target, cx)))
        });
        v_flex()
            .gap(px(7.))
            .child(description)
            .child(v_flex().pl(px(24.)).gap(px(1.)).children(links))
            .into_any_element()
    }

    /// The banner over the page: its breadcrumbs (the project's mark after a project's page),
    /// the arrows back and forward through the pages shown, and the pin.
    fn banner(&self, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let trail = trail(&self.page);
        let last = trail.len().saturating_sub(1);
        let (back, forward) = (
            self.history_ix > 0,
            self.history_ix + 1 < self.history.len(),
        );
        let float = pinned(cx);
        h_flex()
            .flex_none()
            .items_start()
            .h(px(BANNER_HEIGHT))
            .pt(px(3.))
            .pl(px(PAGE_PADDING))
            .pr(px(9.))
            .child(
                h_flex()
                    .h(px(22.))
                    .min_w_0()
                    .children(trail.iter().enumerate().flat_map(|(ix, node)| {
                        let path = settings_path(&self.page, None);
                        let crumb = div()
                            .id(("crumb", ix))
                            .font_weight(FontWeight::BOLD)
                            .child(node.name)
                            .context_menu(move |menu, _, _| copy_path_menu(menu, path.clone()))
                            .into_any_element();
                        // IntelliJ's chevron, 7.5 px after a crumb and 2.5 px before the next
                        let arrow = (ix < last).then(|| {
                            spork_workbench::icon("general/chevronRight")
                                .size(px(16.))
                                .ml(px(7.5))
                                .mr(px(2.5))
                                .text_color(tokens.text)
                                .into_any_element()
                        });
                        std::iter::once(crumb).chain(arrow)
                    }))
                    .when(trail.last().is_some_and(|node| node.project), |crumbs| {
                        crumbs.child(
                            spork_workbench::controls::settings_project_mark("project-page", cx)
                                .ml(px(12.)),
                        )
                    }),
            )
            .when(
                matches!(
                    trail.last().map(|node| node.content),
                    Some(Content::Plugins)
                ),
                |banner| {
                    banner
                        .child(div().flex_1())
                        .child(plugins::PluginsPage::tabs(&self.plugins, cx))
                },
            )
            .child(div().flex_1())
            .child(
                h_flex()
                    .gap(px(4.))
                    .child(
                        spork_workbench::ActionButton::new("settings-back", "general/left")
                            .tooltip("Back")
                            .disabled(!back)
                            .on_click(cx.listener(|this, _, _, cx| this.step(true, cx))),
                    )
                    .child(
                        spork_workbench::ActionButton::new("settings-forward", "general/right")
                            .tooltip("Forward")
                            .disabled(!forward)
                            .on_click(cx.listener(|this, _, _, cx| this.step(false, cx))),
                    )
                    .child(
                        spork_workbench::ActionButton::new("settings-pin", "general/pin")
                            .tooltip("Pin Window")
                            .selected(float)
                            .on_click(cx.listener(|this, _, window, cx| this.pin(window, cx))),
                    ),
            )
    }

    /// IntelliJ's pin: Float mode (above the IDE) or Window mode. A window can't change its
    /// kind, so it opens again in the other, on the same page; with changes not applied yet the
    /// next one opens so.
    fn pin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.set_global(Pinned(!pinned(cx)));
        cx.notify();
        if self.modified(cx) {
            return;
        }
        let (repo, root, title, page) = (
            self.repo.clone(),
            self.root.clone(),
            self.title.clone(),
            self.page.clone(),
        );
        window.remove_window();
        open_window(repo, root, title, Some(Target::Page(page)), cx);
    }
}

/// The banner's height (the page's first row starts under it) and the page's padding, measured
/// on IntelliJ IDEA 2026.3's Settings.
const BANNER_HEIGHT: f32 = 42.;
/// The keymap combo box's width, `KeymapSelector`'s.
const KEYMAP_COMBO_WIDTH: f32 = 200.;
const PAGE_PADDING: f32 = 16.;
/// Editor › Font, and the width of its options beside the preview (IntelliJ IDEA 2026.3's
/// splitter starts the preview at about half the page).
const FONT_PAGE: &str = "Editor/Font";
const FONT_OPTIONS_WIDTH: f32 = 350.;
/// How far in from a tree row's end Plugins' count sits, past the project mark's place.
const BADGE_INSET: f32 = 15.;

/// Whether the search finds `node`: by its name, an option's label on its page, or a page under it.
fn matches(node: &Node, query: &str) -> bool {
    node.name.to_lowercase().contains(query)
        || node.words.contains(query)
        || match node.content {
            Content::Options(rows) => pages::labels(rows).any(|label| pages::finds(label, query)),
            _ => false,
        }
        || node.children.iter().any(|child| matches(child, query))
}

impl Render for SettingsDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let modified = self.modified(cx);
        let query = self.query(cx);
        let nothing_found = !query.is_empty() && !TREE.iter().any(|node| matches(node, &query));
        let tree = self.tree(cx);
        let page = self.page(window, cx);
        let banner = self.banner(cx);
        let scrolls = !matches!(
            trail(&self.page).last().map(|node| node.content),
            Some(Content::Keymap | Content::Plugins | Content::FileTypes)
        ) && self.page != FONT_PAGE;
        v_flex()
            .id("settings-dialog")
            .key_context("SettingsDialog")
            .size_full()
            .bg(tokens.panel)
            .text_color(tokens.text)
            .text_ui()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                // Esc empties the search first, then closes the dialog as Cancel
                if event.keystroke.key == "escape"
                    && event.keystroke.modifiers.number_of_modifiers() == 0
                {
                    match this.query(cx).is_empty() {
                        true => this.close(window, cx),
                        false => this
                            .search
                            .update(cx, |search, cx| search.set_value("", window, cx)),
                    }
                    cx.stop_propagation();
                }
            }))
            .child(spork_workbench::controls::window_header(
                self.title.clone(),
                cx,
            ))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        v_flex()
                            .w(relative(TREE_SHARE))
                            .min_w(px(TREE_MIN_WIDTH))
                            .flex_none()
                            .h_full()
                            .border_r_1()
                            .border_color(tokens.separator)
                            .child(
                                div().pt(px(7.)).px(px(8.)).pb(px(9.)).child(
                                    spork_workbench::TextField::new(&self.search)
                                        .when(nothing_found, |field| field.bg(tokens.search_error))
                                        .aria_label("Search settings")
                                        .prefix(
                                            spork_workbench::icon("general/searchWithHistory")
                                                .w(px(20.))
                                                .h(px(16.)),
                                        )
                                        .cleanable(true),
                                ),
                            )
                            .map(|column| match nothing_found {
                                true => column.child(
                                    div()
                                        .id("settings-nothing")
                                        .role(Role::Label)
                                        .aria_label(SharedString::from("Nothing to show"))
                                        .p(px(10.))
                                        .flex()
                                        .justify_center()
                                        .text_color(tokens.text_secondary)
                                        .child("Nothing to show"),
                                ),
                                false => column.child(tree.vertical_scrollbar(&self.tree_scroll)),
                            }),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(banner)
                            .map(|column| match scrolls {
                                true => column.child(
                                    div()
                                        .id("settings-page")
                                        .role(Role::Group)
                                        .aria_label(SharedString::from(PAGE_LABEL))
                                        .flex_1()
                                        .min_h_0()
                                        .overflow_y_scrollbar()
                                        .px(px(PAGE_PADDING))
                                        .pb(px(PAGE_PADDING))
                                        .child(page),
                                ),
                                false => column.child(div().flex_1().min_h_0().child(page)),
                            }),
                    ),
            )
            .child(
                // IntelliJ IDEA 2026.3's: 11 px above and under the 28 px buttons, 14.5 and 15 px
                // from the window's sides, the buttons 12 px apart
                h_flex()
                    .flex_none()
                    .pl(px(14.5))
                    .pr(px(15.))
                    .py(px(11.))
                    .gap(px(12.))
                    .border_t_1()
                    .border_color(tokens.separator)
                    .child(
                        spork_workbench::controls::help_button("settings-help", cx)
                            .on_click(|_, _, cx| cx.open_url(HELP_URL)),
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
    }
}

/// What assistive technology (and the end-to-end tests) names the page shown by.
const PAGE_LABEL: &str = "Settings page";

/// The tree's share of the width, as IntelliJ's splitter starts, and its narrowest: 244 px and
/// its line in IntelliJ IDEA 2026.3, as wide as its longest row.
const TREE_SHARE: f32 = 0.2;
const TREE_MIN_WIDTH: f32 = 245.;

/// Where the "?" help button leads: Spork's documentation.
const HELP_URL: &str = "https://github.com/medmdime/Spork";

/// Text assistive technology reads as a label.
pub(crate) fn text_label(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Stateful<Div> {
    let text = text.into();
    div()
        .id(id)
        .role(Role::Label)
        .aria_label(text.clone())
        .child(text)
}

/// The value of one of the settings Spork IDE's crates keep, by its path in the dialog, or of a
/// UI Option as the shared components follow it.
pub fn owned_setting(path: &str, cx: &App) -> Option<Value> {
    OWNED
        .iter()
        .find(|owned| owned.path == path)
        .map(|owned| (owned.get)(cx))
        .or_else(|| applied_ui_option(path, cx))
}

/// What the open Settings dialog shows, for the end-to-end tests: the `page`, whether `apply`
/// is enabled, the `modified` pages and the `search` text.
pub fn e2e_state(cx: &mut App) -> Option<Value> {
    let handle = cx.try_global::<OpenDialog>()?.0;
    handle
        .update(cx, |root, _, cx| {
            let dialog = dialog_of(root, cx)?;
            let dialog = dialog.read(cx);
            Some(serde_json::json!({
                "page": dialog.page,
                "apply": dialog.modified(cx),
                "modified": dialog.modified_pages(cx),
                "search": dialog.query(cx),
                "keymap": dialog.keymap.read(cx).e2e_state(cx),
                "plugins": dialog.plugins.read(cx).e2e_state(cx),
                "docker": dialog.docker.read(cx).e2e_state(cx),
                "external": dialog
                    .external
                    .iter()
                    .map(|(path, page)| ((*path).to_owned(), page.e2e_state(cx)))
                    .collect::<serde_json::Map<_, _>>(),
            }))
        })
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use settings::{RootUserSettings as _, SettingsContent};

    use super::{OWNED, option_pages, pages, set_pointer};

    #[test]
    fn a_pointer_makes_its_objects_and_removes_its_value() {
        let mut json = json!({"tabs": {"file_icons": true}});
        set_pointer(&mut json, "/gutter/line_numbers", Some(json!(false)));
        set_pointer(&mut json, "/tabs/file_icons", None);
        assert_eq!(json, json!({"gutter": {"line_numbers": false}, "tabs": {}}));
    }

    /// Every option writes a setting the IDE has: what its control writes reads back the same,
    /// none is dropped as unknown or invalid.
    #[test]
    fn every_option_is_a_setting() {
        for (page, rows) in option_pages() {
            // two options over one setting (Autosave's): the last one written stays
            // the settings Spork IDE's crates keep aren't `settings.json`'s
            let samples: collections::IndexMap<&str, Value> = pages::samples(rows)
                .into_iter()
                .filter(|(path, _)| !OWNED.iter().any(|owned| owned.path == *path))
                .collect();
            let mut json = json!({});
            for (path, value) in &samples {
                set_pointer(&mut json, path, Some(value.clone()));
            }
            let content = SettingsContent::parse_json_with_comments(&json.to_string()).unwrap();
            let read = serde_json::to_value(&content).unwrap();
            for (path, value) in &samples {
                let read = read.pointer(path);
                // a font feature reads back as a number
                let as_number = value.as_bool().map(u64::from);
                let same = read == Some(value)
                    || (as_number.is_some() && read.and_then(Value::as_u64) == as_number)
                    || (value.is_number() && read.and_then(Value::as_f64) == value.as_f64());
                assert!(same, "{page}: {path} reads back as {read:?}, not {value}");
            }
        }
    }

    /// What the controls write reads back as the same setting: none is dropped as invalid.
    #[test]
    fn the_values_written_are_valid_settings() {
        let written = [
            (
                "/theme",
                json!({"mode": "system", "light": "Islands Light", "dark": "Darcula"}),
            ),
            ("/gutter/line_numbers", json!(false)),
            ("/relative_line_numbers", json!("enabled")),
            ("/autosave", json!({"after_delay": {"milliseconds": 15000}})),
            ("/buffer_line_height", json!({"custom": 1.5})),
            ("/wrap_guides", json!([80, 120])),
            (
                "/terminal/working_directory",
                json!({"always": {"directory": "/tmp"}}),
            ),
            ("/terminal/shell", json!({"program": "zsh"})),
            ("/terminal/env", json!({"A": "1"})),
            ("/tab_bar/show", json!(false)),
            ("/tabs/activate_on_close", json!("left_neighbour")),
            ("/line_ending", json!("prefer_lf")),
            ("/base_keymap", json!("VSCode")),
            ("/ui_font_size", json!(16.25)),
            ("/tabs/show_close_button", json!("hidden")),
            ("/buffer_font_fallbacks", json!([])),
            ("/terminal/font_fallbacks", json!(["Menlo"])),
            ("/buffer_font_weight", json!(300.0)),
            ("/auto_indent", json!("preserve_indent")),
            ("/markdown_preview/font_size", json!(16.0)),
            ("/bottom_dock_layout", json!("contained")),
            ("/terminal/max_scroll_history_lines", json!(5000)),
        ];
        let mut json = json!({});
        for (path, value) in &written {
            set_pointer(&mut json, path, Some(value.clone()));
        }
        let content = SettingsContent::parse_json_with_comments(&json.to_string()).unwrap();
        let read = serde_json::to_value(&content).unwrap();
        for (path, value) in &written {
            assert_eq!(read.pointer(path), Some(value), "{path}");
        }
    }
}
