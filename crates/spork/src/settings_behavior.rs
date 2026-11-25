//! Settings ▸ Appearance & Behavior and the top-level pages beside it (Keymap aside): IntelliJ's
//! pages, their values kept by name (settings_window::Store).

use std::cell::RefCell;
use std::collections::HashMap;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_workbench::tokens::metrics;
use spork_workbench::{ActionButton, Tokens, Typography as _};

use crate::notices::Notification;
use crate::settings_window::{Node, SettingsWindow, comment, labeled, titled};

/// The pages under System Settings, as the reference app orders them (by name). Its Data Sharing page is
/// commented out of IntelliJ Community, so the reference app has none.
pub(crate) const SYSTEM_SETTINGS: &[Node] = &[
    Node::page("Date Formats").words("date time format 24-hour pretty"),
    Node::page("HTTP Proxy").words("proxy pac socks"),
    Node::page("Language and Region").words("language locale region"),
    Node::page("Passwords").words("password keychain keepass"),
    Node::page("Server Certificates").words("ssl certificates"),
    Node::page("Updates").words("updates channel check"),
];

/// A group's content, indented under its title as IntelliJ's `group`.
const INDENT: f32 = 20.;

const ZOOMS: &[&str] = &[
    "70%", "80%", "90%", "100%", "110%", "125%", "150%", "175%", "200%",
];

const COLOR_BLINDNESS_HELP: &str = "https://www.jetbrains.com/help/idea/?Colorblind_Settings";

/// A sound signal's key and label.
type Signal = (&'static str, &'static str);

/// The reference app's sound signals in their order: a group's title and its signals, or a signal alone.
/// Events lists only the signals a notification group plays.
const SOUND_SIGNALS: &[(Option<&str>, &[Signal])] = &[
    (
        Some("Events"),
        &[("accessibility.sound.breakpointHit", "Breakpoint hit")],
    ),
    (
        None,
        &[("accessibility.sound.progress", "Status bar progress")],
    ),
    (
        Some("Code highlighting"),
        &[
            ("accessibility.sound.errorLine", "Error on current line"),
            ("accessibility.sound.errorCaret", "Error under caret"),
            ("accessibility.sound.warningLine", "Warning on current line"),
            ("accessibility.sound.warningCaret", "Warning under caret"),
        ],
    ),
    (
        Some("Diff view"),
        &[
            ("accessibility.sound.diffInserted", "Line added"),
            ("accessibility.sound.diffDeleted", "Line removed above"),
            ("accessibility.sound.diffModified", "Line changed"),
            ("accessibility.sound.diffConflict", "Conflict"),
        ],
    ),
    (
        Some("Editor gutter"),
        &[
            (
                "accessibility.sound.breakpointLine",
                "Breakpoint on current line",
            ),
            ("accessibility.sound.gutterInserted", "Line added"),
            ("accessibility.sound.gutterDeleted", "Line removed above"),
            ("accessibility.sound.gutterModified", "Line changed"),
        ],
    ),
    (
        Some("Editor code folding"),
        &[
            (
                "accessibility.sound.foldedLine",
                "Folded code on current line",
            ),
            ("accessibility.sound.foldedCaret", "Folded code under caret"),
        ],
    ),
];

/// The action groups Menus and Toolbars customizes, in the reference app's order.
const ACTION_GROUPS: &[&str] = &[
    "Main Menu",
    "Main Toolbar",
    "Editor Popup Menu",
    "Editor Gutter Popup Menu",
    "Editor Tab Popup Menu",
    "Project View Popup Menu",
    "Scope View Popup Menu",
    "Navigation Bar Popup Menu",
    "Navigation Bar Toolbar",
    "Debug Header More Popup",
    "Debug Header Toolbar",
    "Debug Watches Toolbar",
    "File History Toolbar",
    "Floating Code Toolbar",
    "Run Tool Window Header More Popup",
    "Run Tool Window Header Toolbar",
    "VCS Local Changes Toolbar",
    "VCS Log Changes Browser Toolbar",
    "VCS Log Toolbar",
    "VCS Operations Popup",
];

/// The group the reference app keeps empty: its row has no chevron.
const EMPTY_ACTION_GROUP: &str = "Run Tool Window Header More Popup";

/// File Colors' rows by default: the reference app's predefined scopes that have a color, kept locally
/// (scope, color key, color, shared key); without the Java plugin, no Tests scope.
const FILE_COLORS: &[(&str, &str, &str, &str)] = &[(
    "Non-Project Files",
    "fileColors.nonProjectFiles.color",
    "Yellow",
    "fileColors.nonProjectFiles.shared",
)];

const COLORS: &[&str] = &[
    "Blue", "Green", "Orange", "Rose", "Violet", "Yellow", "Gray",
];

/// A notification group of the reference app's: its name, its settings' keys, its popup by default and
/// whether its notifications can show as a tool window balloon.
struct NotificationGroup {
    name: &'static str,
    popup: &'static str,
    log: &'static str,
    sound: &'static str,
    read_aloud: &'static str,
    default_popup: &'static str,
    tool_window: bool,
}

macro_rules! group {
    ($name:literal, $slug:literal, $popup:literal) => {
        group!(@ $name, $slug, $popup, false)
    };
    ($name:literal, $slug:literal, $popup:literal, tool_window) => {
        group!(@ $name, $slug, $popup, true)
    };
    (@ $name:literal, $slug:literal, $popup:literal, $tool_window:literal) => {
        NotificationGroup {
            name: $name,
            popup: concat!("notifications.", $slug, ".popup"),
            log: concat!("notifications.", $slug, ".log"),
            sound: concat!("notifications.", $slug, ".sound"),
            read_aloud: concat!("notifications.", $slug, ".readAloud"),
            default_popup: $popup,
            tool_window: $tool_window,
        }
    };
}

/// The reference app's notification groups (its plugins' `notificationGroup`s shown in Settings), sorted by
/// name as IntelliJ lists them.
const NOTIFICATION_GROUPS: &[NotificationGroup] = &[
    group!("Analysis ignore", "analysis-ignore", "Balloon"),
    group!(
        "Automatic indent detection disabled",
        "automatic-indent-detection",
        "Sticky balloon"
    ),
    group!(
        "Backup and Sync messages",
        "settingssync-errors",
        "Sticky balloon"
    ),
    group!("Batch quick fix", "batch-quick-fix", "Balloon"),
    group!("Branch context switched", "branch-context-group", "Balloon"),
    group!("Breakpoint hit", "breakpoint-hit", "No popup", tool_window),
    group!(
        "Browser configuration problems",
        "browsercfgproblems",
        "Balloon"
    ),
    group!(
        "Built-in HTTP server could not start",
        "built-in-server",
        "Sticky balloon"
    ),
    group!(
        "Cache recovery step performed",
        "cache-recovery",
        "Sticky balloon"
    ),
    group!(
        "Cannot update plugin",
        "missing-plugin-update-source",
        "Sticky balloon"
    ),
    group!(
        "Code inspection completed",
        "inspection-results",
        "Tool window balloon",
        tool_window
    ),
    group!(
        "Color Scheme deprecation",
        "colorschemedeprecation",
        "Sticky balloon"
    ),
    group!(
        "Conflict with system shortcuts detected",
        "system-shortcuts-conflicts",
        "Sticky balloon"
    ),
    group!("Could not load project", "project-loading-error", "Balloon"),
    group!("Could not load settings", "load-error", "Balloon"),
    group!("Could not save project", "project-settings", "Balloon"),
    group!("Could not save settings", "settings-error", "Balloon"),
    group!(
        "Debugger messages",
        "debugger-messages",
        "Tool window balloon",
        tool_window
    ),
    group!(
        "Duplicate content roots detected",
        "content-root-duplicates",
        "Balloon"
    ),
    group!(
        "Essential highlighting mode enabled",
        "essential-highlighting-mode",
        "Balloon"
    ),
    group!(
        "External process execution failed",
        "execution",
        "Tool window balloon"
    ),
    group!(
        "External system build scripts found",
        "external-system-auto-link-notification-group",
        "Sticky balloon"
    ),
    group!(
        "Failed to attach debugger to process",
        "attach-to-process-action",
        "Balloon"
    ),
    group!(
        "Failed to generate module descriptors",
        "failed-to-generate-module-descriptors",
        "Balloon"
    ),
    group!("Failed to remove BOM", "failed-to-remove-bom", "Balloon"),
    group!("Feedback in IDE", "feedback-in-ide", "Balloon"),
    group!("File System", "vfs", "Balloon"),
    group!(
        "File system synchronization issues detected",
        "file-watcher-messages",
        "Sticky balloon"
    ),
    group!(
        "File type conflict detected",
        "file-type-conflict",
        "Balloon"
    ),
    group!(
        "File type pattern reassigned",
        "pattern-reassigned",
        "Balloon"
    ),
    group!(
        "Find in Files operation failed",
        "find-in-path",
        "Tool window balloon",
        tool_window
    ),
    group!("Frameworks detected", "framework-detection", "Balloon"),
    group!(
        "Gradle wrapper update failed",
        "gradle-wrapper-update",
        "Balloon"
    ),
    group!(
        "Heap dump analysis performed",
        "heap-dump-analysis",
        "Sticky balloon"
    ),
    group!("HotSwap failed", "hotswap-messages", "Balloon"),
    group!(
        "IDE and plugin updates available",
        "ide-and-plugin-updates",
        "Sticky balloon"
    ),
    group!("IDE caches corruption detected", "ide-caches", "Balloon"),
    group!("IDE errors", "ide-errors", "Balloon"),
    group!(
        "IDE updates installed",
        "ide-update-results",
        "Sticky balloon"
    ),
    group!(
        "Important feedback in IDE",
        "sticky-feedback-in-ide",
        "Sticky balloon"
    ),
    group!(
        "Incorrect formatting inspection",
        "incorrect-formatting",
        "Sticky balloon"
    ),
    group!("Installation is complete", "ultimate-installed", "Balloon"),
    group!("Internal error report submitted", "error-report", "Balloon"),
    group!("JDK update failed", "jdk-update-error", "Sticky balloon"),
    group!(
        "JVM performance issues detected",
        "performancewatcher",
        "Sticky balloon"
    ),
    group!("Keymap installed", "keymapinstalled", "Balloon"),
    group!("Keymap missing", "keymapmissing", "Sticky balloon"),
    group!(
        "Language service stopped or restarted",
        "language-service-stopped-or-restarted",
        "Balloon"
    ),
    group!(
        "Leftover IDE directories found",
        "leftover-ide-directories",
        "Balloon"
    ),
    group!("Local history messages", "localhistory-general", "Balloon"),
    group!("Low memory detected", "low-memory", "Sticky balloon"),
    group!("LSP messages", "lsp-window-showmessage", "Balloon"),
    group!(
        "Maximum number of editor carets reached",
        "too-many-carets",
        "Balloon"
    ),
    group!(
        "Microsoft Defender may affect IDE",
        "windowsdefender",
        "Balloon"
    ),
    group!(
        "Module dependencies analysis completed",
        "dependencies",
        "Tool window balloon",
        tool_window
    ),
    group!(
        "Modules automatically loaded or unloaded",
        "automatic-module-unloading",
        "Balloon"
    ),
    group!(
        "Modules no longer referenced by external system",
        "build-sync-orphan-modules",
        "Tool window balloon",
        tool_window
    ),
    group!("Paid plugins not loaded", "paid-plugins", "Sticky balloon"),
    group!(
        "Parameter name hints settings changed",
        "parameter-name-hints",
        "Balloon"
    ),
    group!(
        "Password storage not available",
        "password-safe",
        "Sticky balloon"
    ),
    group!(
        "Plugin suggestions available",
        "plugins-suggestion",
        "Sticky balloon"
    ),
    group!(
        "Plugin update sources",
        "plugin-update-sources-reset",
        "Balloon"
    ),
    group!("Plugins failed to load", "plugin-error", "Balloon"),
    group!(
        "Plugins installed or updated",
        "plugin-update-results",
        "Balloon"
    ),
    group!("Power save mode enabled", "power-save-mode", "Balloon"),
    group!(
        "Presentation assistant",
        "presentation-assistant",
        "Balloon"
    ),
    group!(
        "Project startup task execution failed",
        "project-startup-tasks-messages",
        "No popup"
    ),
    group!("Project template created", "project-template", "Balloon"),
    group!(
        "Proxy authentication failed",
        "proxy-auth-failed",
        "Sticky balloon"
    ),
    group!(
        "Recommended language plugin available",
        "language-plugins-notifications",
        "Balloon"
    ),
    group!(
        "Required plugins not loaded",
        "required-plugins",
        "Sticky balloon"
    ),
    group!("Run Anything command executed", "run-anything", "Balloon"),
    group!("SDK automatically configured", "missing-sdks", "Balloon"),
    group!(
        "Spellchecker dictionary loading failed",
        "spellchecker",
        "Balloon"
    ),
    group!("Sticky lines disabled", "sticky-lines", "Balloon"),
    group!(
        "System clipboard unavailable",
        "system-clipboard",
        "Balloon"
    ),
    group!(
        "System health issue detected",
        "system-health",
        "Sticky balloon"
    ),
    group!("System messages", "system-messages", "Balloon"),
    group!("Task context data corrupted", "tasks", "Balloon"),
    group!("Terminal", "terminal", "Balloon"),
    group!(
        "TextMate bundle loading failed",
        "textmate-bundles",
        "Balloon"
    ),
    group!(
        "Timeout opening page in browser",
        "url-does-not-respond-notification",
        "Balloon"
    ),
    group!(
        "Unindexed Maven repositories found",
        "unindexed-maven-repositories-gradle-detection",
        "Balloon"
    ),
    group!(
        "VCS external executable validation failed",
        "external-executable-critical-failures",
        "Sticky balloon"
    ),
    group!(
        "VCS hosting integrations",
        "vcs-hosting-integrations",
        "Balloon"
    ),
    group!(
        "VCS important messages",
        "vcs-important-notifications",
        "Sticky balloon"
    ),
    group!(
        "VCS messages",
        "vcs-messages",
        "Tool window balloon",
        tool_window
    ),
    group!("VCS notifications", "vcs-notifications", "Balloon"),
    group!(
        "VCS silent notifications",
        "vcs-silent-notifications",
        "No popup"
    ),
];

/// A notification group whose Play sound is a sound signal's (IntelliJ's `notificationSound`).
const BOUND_SOUNDS: &[(&str, &str)] = &[("Breakpoint hit", "accessibility.sound.breakpointHit")];

const POPUPS: &[&str] = &["No popup", "Balloon", "Sticky balloon"];
const POPUPS_WITH_TOOL_WINDOW: &[&str] = &[
    "No popup",
    "Balloon",
    "Sticky balloon",
    "Tool window balloon",
];

const LOCATIONS: &[&str] = &["Top Right", "Bottom Right", "Top Left", "Bottom Left"];

const POSITIONS: &[&str] = &[
    "Top Left",
    "Top Center",
    "Top Right",
    "Bottom Left",
    "Bottom Center",
    "Bottom Right",
];

/// The keymaps the reference app offers on macOS, by their presentable names.
const KEYMAPS: &[&str] = &[
    "macOS",
    "Classic", // IntelliJ's "IntelliJ IDEA Classic", without the product's name
    "macOS System Shortcuts",
    "Sublime Text (macOS)",
    "Emacs",
    "Windows",
    "Sublime Text",
];

const DATE_PATTERNS: &str =
    "https://docs.oracle.com/javase/7/docs/api/java/text/SimpleDateFormat.html";

const AUTO_DETECT: &str = "Auto-detect proxy settings";
const NO_PROXY: &str = "No proxy";
const MANUAL: &str = "Manual proxy configuration";
const SYSTEM_PROXY: &str = "x-apple.systempreferences:com.apple.Network-Settings.extension";

const REGIONS: &[&str] = &[
    "Africa",
    "Americas",
    "Asia (except China Mainland)",
    "China Mainland",
    "Europe",
    "Middle East",
    "Oceania",
    "Not specified",
];
const REGION_HELP: &str = "https://www.jetbrains.com/help/idea/?region-settings";

const KEYCHAIN: &str = "In native Keychain";
const KEEPASS: &str = "In KeePass";

/// A plugin the reference app bundles: its name, its setting's key and its description.
struct Plugin {
    name: &'static str,
    key: &'static str,
    description: &'static str,
}

/// The plugins the reference app bundles that the Installed tab lists, by category in its order (by name,
/// Other Tools last); its implementation details aren't listed.
const PLUGINS: &[(&str, &[Plugin])] = &[
    (
        "Languages",
        &[
            Plugin {
                name: "TextMate Bundles",
                key: "plugins.org.jetbrains.plugins.textmate",
                description: "Supports syntax highlighting for arbitrary file types, controlled by TextMate bundles",
            },
            Plugin {
                name: "XML and HTML",
                key: "plugins.com.intellij.xml",
                description: "Provides XML, HTML, DTD and XSD editing support: highlighting, code completion, Emmet, structure view, and language injection.",
            },
        ],
    ),
    (
        "Libraries",
        &[
            Plugin {
                name: "Java Native Access (JNA)",
                key: "plugins.com.intellij.modules.jna",
                description: "Holds the one JNA copy and its jnidispatch native that other plugins reuse to call native code.",
            },
            Plugin {
                name: "Pseudo Terminal (PTY4J)",
                key: "plugins.com.intellij.modules.pty4j",
                description: "Runs a local process in a pseudo terminal with pty4j.",
            },
        ],
    ),
    (
        "Version Controls",
        &[
            Plugin {
                name: "Git",
                key: "plugins.Git4Idea",
                description: "Provides integration with Git.\n\
                    • Check out your source code from a Git repository\n\
                    • Browse, commit, push, pull, merge, view log, review pull requests, and much more\n\
                    • Dedicated item in the VCS menu and in the context menu\n\
                    To configure, open Settings / Preferences and go to Version Control | Git.",
            },
            Plugin {
                name: "GitHub",
                key: "plugins.org.jetbrains.plugins.github",
                description: "Provides integration with GitHub.\n\
                    • Specify your GitHub account to access all of your repositories from the IDE.\n\
                    • Clone repositories, create and review pull requests, create gists, and much more.\n\
                    To configure, open Settings / Preferences and go to Version Control | GitHub.\n\
                    Requires the Git plugin.",
            },
            Plugin {
                name: "GitLab",
                key: "plugins.org.jetbrains.plugins.gitlab",
                description: "Provides integration with GitLab.\n\
                    Access your GitLab projects without leaving the IDE\n\
                    Log in to your GitLab account to access all your projects from the IDE. Switch between multiple GitLab accounts to do your main job and contribute to your private projects.\n\
                    Clone projects\n\
                    Clone a project that you want to contribute to directly from the IDE and create a new project based on it.\n\
                    Work with merge requests\n\
                    Manage incoming merge requests:\n\
                    • Filter requests by state, author, assignee, reviewer, and label.\n\
                    • Keep up with the changes by checking the timeline.\n\
                    • Jump to a merge request on GitLab if necessary.\n\
                    Review merge requests:\n\
                    • View the diff between the suggested changes and the base revision.\n\
                    • Leave your comments and suggestions.\n\
                    • Merge or close merge requests.\n\
                    View and apply suggested changes:\n\
                    • Answer the comments to your merge request.\n\
                    • Close or save your merge request as a draft.\n\
                    To configure, open Settings / Preferences and go to Version Control | GitLab.\n\
                    Requires the Git plugin.",
            },
            Plugin {
                name: "Modal Commit Interface",
                key: "plugins.intellij.git.commit.modal",
                description: "This plugin restores the modal commit option for Git and Mercurial in the IDE, which was the default before the introduction of the non-modal commit window.\n\
                    If you prefer the traditional commit workflow, this plugin brings that functionality back (available in \"Settings | Advanced Settings | Version Control\").",
            },
        ],
    ),
    (
        "Other Tools",
        &[
            Plugin {
                name: "Bookmarks",
                key: "plugins.com.intellij.bookmarks",
                description: "Marks lines of code, files, folders, classes, and editor tabs for quick navigation, supports anonymous and mnemonic bookmarks accessible via keyboard shortcuts, and organizes them into named lists in a dedicated tool window.",
            },
            Plugin {
                name: "Images",
                key: "plugins.com.intellij.platform.images",
                description: "Provides Image file type preview, SVG file type contents language.",
            },
            Plugin {
                name: "Navigation Bar",
                key: "plugins.com.intellij.navbar",
                description: "The navigation bar is a quick alternative to the Project view, where you can go through the structure of your project, open specific files, and jump to specific code elements in the current file.",
            },
            Plugin {
                name: "Recent Files",
                key: "plugins.com.intellij.recentFiles",
                description: "Provides Recent Files platform modules for the IDE.",
            },
            Plugin {
                name: "Structure View",
                key: "plugins.com.intellij.structureView",
                description: "Provides Structure View tool window and actions",
            },
            Plugin {
                name: "Tasks and Contexts",
                key: "plugins.com.intellij.platform.tasks",
                description: "Automatically saves and restores relevant context information on VCS branch switches, such as a set of bookmarks, breakpoints, and tabs opened in the editor.",
            },
            Plugin {
                name: "Terminal",
                key: "plugins.org.jetbrains.plugins.terminal",
                description: "The Terminal (View | Tool Windows | Terminal) is an integrated command-line emulator that allows Developers and DevOps/SRE to run shell commands directly within the IDE interface, minimizing the need to switch between windows. It supports various shell types (e.g., Bash, Zsh, PowerShell) and offers a customizable experience, allowing users to set prompt styles, environment variables, and more. With multiple session support, users can manage tabs, split windows, and use IDE-specific commands, such as executing Git operations that automatically open relevant IDE windows like the Git log.\n\
                    To access the latest features of the terminal and AI capabilities, click \"Enable New Terminal\" in the terminal tab’s context menu (experimental).\n\
                    For more details, refer to the documentation.",
            },
            Plugin {
                name: "Version Control",
                key: "plugins.com.intellij.platform.vcs",
                description: "Provides the core version control support shared by all VCS integrations: the Commit and Local Changes views, the VCS Log, file history, shelving, annotations, and comparison of revisions.\n\
                    Individual integrations, such as Git, Subversion, and Mercurial, build on top of this plugin.",
            },
        ],
    ),
];

/// What the pages show that isn't a setting: the rows selected, the Plugins tab, the search
/// fields. It lasts as long as the Settings window, which Spork opens one at a time.
#[derive(Default)]
struct ViewState {
    window: Option<EntityId>,
    selected: HashMap<&'static str, usize>,
    inputs: HashMap<&'static str, Entity<InputState>>,
    /// The groups open in Keymap's tree, by path.
    keymap_open: std::collections::HashSet<String>,
}

thread_local! {
    static VIEW: RefCell<ViewState> = RefCell::new(ViewState::default());
}

/// The view state of the Settings window `owner`, started afresh for a new window.
fn with_view<R>(owner: EntityId, action: impl FnOnce(&mut ViewState) -> R) -> R {
    VIEW.with_borrow_mut(|view| {
        if view.window != Some(owner) {
            *view = ViewState {
                window: Some(owner),
                ..ViewState::default()
            };
        }
        action(view)
    })
}

impl SettingsWindow {
    /// The page at `path` when it is one of these, else None.
    pub(crate) fn behavior_settings(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        match path {
            "Plugins" => return Some(self.plugins_page(window, cx)),
            "Keymap" => return Some(self.keymap_page(window, cx)),
            _ => {}
        }
        let page = match path.strip_prefix("Appearance & Behavior/")? {
            "Accessibility" => self.accessibility_page(cx),
            "Menus and Toolbars" => self.menus_page(window, cx),
            "File Colors" => self.file_colors_page(cx),
            "Scopes" => self.scopes_page(cx),
            "Notifications" => self.notifications_page(cx),
            "Quick Lists" => self.quick_lists_page(cx),
            "Required Plugins" => self.required_plugins_page(cx),
            "Trusted Locations" => self.trusted_locations_page(cx),
            "Path Variables" => self.path_variables_page(window, cx),
            "Presentation Assistant" => self.presentation_page(window, cx),
            "System Settings/Date Formats" => self.date_formats_page(window, cx),
            "System Settings/HTTP Proxy" => self.proxy_page(window, cx),
            "System Settings/Language and Region" => self.language_page(cx),
            "System Settings/Passwords" => self.passwords_page(window, cx),
            "System Settings/Server Certificates" => self.certificates_page(cx),
            "System Settings/Updates" => self.updates_page(cx),
            _ => return None,
        };
        Some(page)
    }

    /// The row selected in `list`, if any.
    fn selected(&self, list: &'static str, cx: &Context<Self>) -> Option<usize> {
        with_view(cx.entity_id(), |view| view.selected.get(list).copied())
    }

    /// A row of `list`: IntelliJ's 24 px, the selection's fill; a click selects it.
    fn list_row(
        &self,
        list: &'static str,
        ix: usize,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let tokens = Tokens::get(cx);
        h_flex()
            .id(SharedString::from(format!("{list}-row-{ix}")))
            .flex_none()
            .h(px(metrics::ROW_HEIGHT))
            .px_2()
            .gap_2()
            .when(selected, |row| row.bg(tokens.selection))
            .on_click(cx.listener(move |_, _, _, cx| {
                with_view(cx.entity_id(), |view| view.selected.insert(list, ix));
                cx.notify();
            }))
    }

    /// A text field that isn't a setting (a search, a password Spork doesn't keep): it lives with
    /// the view state.
    fn view_input(
        &self,
        id: &'static str,
        placeholder: &'static str,
        masked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let owner = cx.entity_id();
        if let Some(state) = with_view(owner, |view| view.inputs.get(id).cloned()) {
            return state;
        }
        let state = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .masked(masked)
        });
        // the field outlives no window: the subscription ends with it or with the window
        cx.subscribe(&state, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify()
            }
        })
        .detach();
        with_view(owner, |view| view.inputs.insert(id, state.clone()));
        state
    }

    /// `opt_check`, or its disabled look while `enabled` is false (IntelliJ's `enabledIf`).
    fn behavior_check_if(
        &self,
        enabled: bool,
        key: &'static str,
        label: &'static str,
        default: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match enabled {
            true => self.opt_check(key, label, default, cx).into_any_element(),
            false => disabled_check(key, label, self.flag(key, default), cx).into_any_element(),
        }
    }

    /// `opt_combo`, or a disabled combo box while `enabled` is false.
    fn behavior_combo_if(
        &self,
        enabled: bool,
        key: &'static str,
        choices: &'static [&'static str],
        default: &'static str,
        width: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        match enabled {
            true => self.opt_combo(key, choices, default, width, cx),
            false => disabled_combo(key, self.value(key, default), width),
        }
    }

    /// `opt_text`, or its text greyed and read-only while `enabled` is false.
    fn text_if(
        &mut self,
        enabled: bool,
        key: &'static str,
        default: &str,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        match enabled {
            true => self.opt_text(key, default, width, window, cx),
            false => disabled_field(self.value(key, default), width, cx),
        }
    }

    /// One radio button of `key` (`choices` holds one), so other rows can sit between a group's
    /// buttons; greyed while `enabled` is false.
    pub(crate) fn radio_if(
        &self,
        enabled: bool,
        key: &'static str,
        choices: &'static [&'static str],
        default: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        if enabled {
            return self.opt_radio(key, choices, default, cx);
        }
        let current = self.value(key, default);
        v_flex().children(choices.iter().map(|choice| {
            spork_workbench::controls::disabled_radio(
                SharedString::from(format!("{key}-{choice}")),
                *choice,
                *choice == current,
                cx,
            )
        }))
    }

    /// `opt_button`, or the button disabled while `enabled` is false.
    fn button_if(
        &self,
        enabled: bool,
        id: &'static str,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        match enabled {
            true => self.opt_button(id, label, cx),
            false => h_flex().child(spork_workbench::Button::new(id, label).disabled(true)),
        }
    }

    fn accessibility_page(&self, cx: &mut Context<Self>) -> Div {
        let screen_readers = self.flag("accessibility.screenReaders", false);
        let announce = screen_readers && self.flag("accessibility.announceNotifications", true);
        let vision = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(labeled(
                        "Zoom:",
                        self.opt_combo("accessibility.zoom", ZOOMS, "100%", 90., cx)
                            .into_any_element(),
                    ))
                    .child(hint("Change with ⌃⌥= or ⌃⌥-. Set to 100% with ⌃⌥0", cx)),
            )
            .child(self.opt_check(
                "accessibility.contrastScrollbars",
                "Use contrast scrollbars",
                false,
                cx,
            ))
            .child(
                h_flex()
                    .gap_3()
                    .child(self.opt_check(
                        "accessibility.colorBlindness",
                        "Adjust colors for red-green vision deficiency",
                        false,
                        cx,
                    ))
                    .child(web_link(
                        "color-blindness-help",
                        "How it works",
                        COLOR_BLINDNESS_HELP,
                        cx,
                    )),
            )
            .child(comment(
                "Requires restart. For protanopia and deuteranopia.",
                cx,
            ));
        let screen_reader = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(
                h_flex()
                    .gap_3()
                    .child(self.opt_check(
                        "accessibility.screenReaders",
                        "Support screen readers",
                        false,
                        cx,
                    ))
                    .child(hint("Requires restart", cx)),
            )
            .child(comment(
                "Disables tooltips on mouse hover. Enables sound signals.",
                cx,
            ))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_2()
                    .child(self.behavior_check_if(
                        screen_readers,
                        "accessibility.announceNotifications",
                        "Announce notifications with screen reader",
                        true,
                        cx,
                    ))
                    .child(
                        h_flex()
                            .pl(px(INDENT))
                            .gap_2()
                            .child(label_if(announce, "Announcement mode:", cx))
                            .child(self.behavior_combo_if(
                                announce,
                                "accessibility.announcementMode",
                                &["With medium priority", "With high priority"],
                                "With medium priority",
                                200.,
                                cx,
                            )),
                    )
                    .child(div().pl(px(INDENT + 130.)).child(hint(
                        "Notifications will interrupt previous output if it has lower priority",
                        cx,
                    ))),
            );
        v_flex()
            .gap_3()
            .child(titled("Vision", cx))
            .child(vision)
            .child(titled("Screen Reader Mode", cx))
            .child(screen_reader)
            .child(titled("Sound Signals", cx))
            .child(self.sound_signals(screen_readers, cx))
    }

    /// The sound signals: a signal nobody chose follows the screen reader support.
    fn sound_signals(&self, screen_readers: bool, cx: &mut Context<Self>) -> Div {
        let mut rows = Vec::new();
        for &(group, signals) in SOUND_SIGNALS {
            let mut signal_rows = Vec::new();
            for &(key, label) in signals {
                signal_rows.push(
                    h_flex()
                        .gap_2()
                        .child(self.opt_check(key, label, screen_readers, cx))
                        .child(preview_sound(SharedString::from(format!("{key}-preview"))))
                        .into_any_element(),
                );
            }
            let Some(title) = group else {
                rows.extend(signal_rows);
                continue;
            };
            let on = signals
                .iter()
                .filter(|(key, _)| self.flag(key, screen_readers))
                .count();
            let state = match on {
                0 => Some(false),
                on if on == signals.len() => Some(true),
                _ => None,
            };
            rows.push(
                v_flex()
                    .gap_2()
                    .child(
                        spork_workbench::controls::labeled_check_state(
                            SharedString::from(format!("sound-group-{title}")),
                            title,
                            state,
                            cx,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let value = if state == Some(true) { "false" } else { "true" };
                            for (key, _) in signals {
                                this.set_value(key, value);
                            }
                            cx.notify();
                        })),
                    )
                    .child(v_flex().pl(px(INDENT)).gap_2().children(signal_rows))
                    .into_any_element(),
            );
        }
        v_flex().pl(px(INDENT)).gap_3().children(rows)
    }

    fn menus_page(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let filter = self.view_input("menus.filter", "", false, window, cx);
        let query = filter.read(cx).value().trim().to_lowercase();
        // IntelliJ's filter selects the first group it finds rather than hiding the others
        let selected = match query.is_empty() {
            // The reference app opens the page on its first group
            true => self.selected("menus", cx).or(Some(0)),
            false => ACTION_GROUPS
                .iter()
                .position(|group| group.to_lowercase().contains(&query)),
        };
        let restore_label = selected
            .and_then(|ix| ACTION_GROUPS.get(ix))
            .map_or_else(|| "Restore".to_owned(), |group| format!("Restore {group}"));
        let restore = ActionButton::new("menus-restore", "general/reset")
            .tooltip("Restore Actions…")
            .popup_marker()
            .dropdown_menu(move |menu, _, _| {
                let label = restore_label.clone();
                menu.item(
                    PopupMenuItem::new(label.clone())
                        .disabled(selected.is_none())
                        .on_click(move |_, window, cx| not_yet(&label, window, cx)),
                )
                .item(
                    PopupMenuItem::new("Restore All Defaults")
                        .on_click(|_, window, cx| not_yet("Restore All Defaults", window, cx)),
                )
            });
        // the groups are roots: an icon, a move or a removal needs an action under one
        let toolbar = h_flex()
            .gap(px(2.))
            .child(
                spork_workbench::Button::new("menus-add", "Add…")
                    .disabled(selected.is_none())
                    .on_click(|_, window, cx| not_yet("Add…", window, cx)),
            )
            .child(div().w(px(6.)))
            .child(action(
                "menus-edit-icon",
                "general/edit",
                "Edit Icon…",
                false,
            ))
            .child(action("menus-up", "general/moveUp", "Move Up", false))
            .child(action("menus-down", "general/moveDown", "Move Down", false))
            .child(toolbar_separator(cx))
            .child(action("menus-remove", "general/delete", "Remove", false))
            .child(restore);
        let mut rows = Vec::new();
        for (ix, group) in ACTION_GROUPS.iter().enumerate() {
            rows.push(
                self.list_row("menus", ix, selected == Some(ix), cx)
                    .role(Role::TreeItem)
                    .aria_label(*group)
                    .gap(px(2.))
                    .child(
                        div()
                            .size(px(16.))
                            .when(*group != EMPTY_ACTION_GROUP, |chevron| {
                                chevron.child(
                                    spork_workbench::icon("general/chevronRight")
                                        .size(px(16.))
                                        .text_color(tokens.icon),
                                )
                            }),
                    )
                    .child(*group),
            );
        }
        v_flex()
            .gap_2()
            .child(
                h_flex().justify_between().child(toolbar).child(
                    div().w(px(220.)).child(
                        spork_workbench::TextField::new(&filter)
                            .prefix(spork_workbench::icon("general/search").size(px(16.)))
                            .cleanable(true),
                    ),
                ),
            )
            .child(
                div()
                    .id("menus-tree")
                    .h(px(480.))
                    .border_1()
                    .border_color(tokens.separator)
                    .overflow_y_scrollbar()
                    .child(v_flex().children(rows)),
            )
    }

    fn file_colors_page(&self, cx: &mut Context<Self>) -> Div {
        let enabled = self.flag("fileColors.enabled", true);
        // local colors first, then the shared ones, as IntelliJ's two lists
        let mut order: Vec<usize> = (0..FILE_COLORS.len()).collect();
        order.sort_by_key(|&ix| {
            FILE_COLORS
                .get(ix)
                .is_some_and(|row| self.flag(row.3, false))
        });
        let shared = |row: usize| {
            order
                .get(row)
                .and_then(|&ix| FILE_COLORS.get(ix))
                .map(|color| self.flag(color.3, false))
        };
        let selected = self.selected("file-colors", cx);
        // a row moves only within its list
        let can_move = |to: Option<usize>| match (selected, to) {
            (Some(from), Some(to)) => shared(to).is_some() && shared(from) == shared(to),
            _ => false,
        };
        let up = can_move(selected.and_then(|row| row.checked_sub(1)));
        let down = can_move(selected.map(|row| row + 1));
        let mut rows = vec![
            table_header(
                &[
                    ("Scope", Some(240.)),
                    ("Color", Some(160.)),
                    ("Share through VCS", None),
                ],
                cx,
            )
            .into_any_element(),
        ];
        for (row, ix) in order.iter().enumerate() {
            let Some(&(scope, color_key, color, shared_key)) = FILE_COLORS.get(*ix) else {
                continue;
            };
            rows.push(
                self.list_row("file-colors", row, selected == Some(row), cx)
                    .h(px(32.))
                    .gap_0()
                    .child(div().w(px(240.)).child(scope))
                    .child(
                        div()
                            .w(px(160.))
                            .child(self.opt_combo(color_key, COLORS, color, 140., cx)),
                    )
                    .child(self.opt_check(shared_key, "", false, cx))
                    .into_any_element(),
            );
        }
        let actions = h_flex()
            .child(action("file-colors-add", "general/add", "Add", true).popup_marker())
            .child(action(
                "file-colors-remove",
                "general/remove",
                "Remove",
                selected.is_some(),
            ))
            .child(action("file-colors-up", "general/moveUp", "Move Up", up))
            .child(action(
                "file-colors-down",
                "general/moveDown",
                "Move Down",
                down,
            ));
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_4()
                    .child(self.opt_check("fileColors.enabled", "Enable file colors", true, cx))
                    .child(self.behavior_check_if(
                        enabled,
                        "fileColors.editorTabs",
                        "Use in editor tabs",
                        true,
                        cx,
                    ))
                    .child(self.behavior_check_if(
                        enabled,
                        "fileColors.projectView",
                        "Use in project view",
                        true,
                        cx,
                    )),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(decorated(actions, v_flex().children(rows), cx).h(px(380.)))
                    .child(hint(
                        "Files can belong to several scopes. If there are two colors for one scope, the color of the first scope in the list is used.",
                        cx,
                    )),
            )
            .child(
                h_flex().child(
                    spork_workbench::controls::link("file-colors-scopes", "Manage scopes…", cx)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show("Appearance & Behavior/Scopes", cx)
                        })),
                ),
            )
    }

    fn scopes_page(&self, cx: &mut Context<Self>) -> Div {
        let add = ActionButton::new("scopes-add", "general/add")
            .tooltip("Add Scope")
            .popup_marker()
            .dropdown_menu(|menu, _, _| {
                menu.item(
                    PopupMenuItem::new("Local")
                        .icon(spork_workbench::icon("ide/localScope"))
                        .on_click(|_, window, cx| not_yet("Add Scope", window, cx)),
                )
                .item(
                    PopupMenuItem::new("Shared")
                        .icon(spork_workbench::icon("ide/sharedScope"))
                        .on_click(|_, window, cx| not_yet("Add Scope", window, cx)),
                )
            });
        let actions = h_flex()
            .child(add)
            .child(action("scopes-remove", "general/remove", "Delete", false))
            .child(action(
                "scopes-copy",
                "general/copy",
                "Copy Configuration",
                false,
            ))
            .child(action("scopes-save-as", "general/save", "Save As", false))
            .child(action("scopes-up", "general/moveUp", "Move Up", false))
            .child(action(
                "scopes-down",
                "general/moveDown",
                "Move Down",
                false,
            ));
        h_flex()
            .h(px(520.))
            .gap_2()
            .child(
                decorated(actions, empty_text("No scopes added.", cx), cx)
                    .w(px(240.))
                    .h_full(),
            )
            .child(div().flex_1().h_full().child(empty_text(
                "Select a scope to view or edit its details here",
                cx,
            )))
    }

    fn notifications_page(&self, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let balloons = self.flag("notifications.balloons", true);
        let selected = self.selected("notifications", cx).unwrap_or(0);
        let mut rows = Vec::new();
        for (ix, group) in NOTIFICATION_GROUPS.iter().enumerate() {
            rows.push(
                self.list_row("notifications", ix, ix == selected, cx)
                    .child(div().truncate().child(group.name)),
            );
        }
        let settings = match NOTIFICATION_GROUPS.get(selected) {
            Some(group) => self.notification_settings(group, balloons, cx),
            None => div(),
        };
        let do_not_ask = h_flex().child(action(
            "do-not-ask-remove",
            "general/remove",
            "Remove",
            false,
        ));
        v_flex()
            .gap_3()
            .child(self.opt_check(
                "notifications.balloons",
                "Display balloon notifications",
                true,
                cx,
            ))
            .child(self.opt_check(
                "notifications.system",
                "Enable system notifications",
                true,
                cx,
            ))
            .child(labeled(
                "Notification location:",
                self.opt_combo(
                    "notifications.location",
                    LOCATIONS,
                    "Bottom Right",
                    160.,
                    cx,
                )
                .into_any_element(),
            ))
            .child(
                h_flex()
                    .items_start()
                    .gap_3()
                    .child(
                        div()
                            .id("notification-groups")
                            .w(px(300.))
                            .h(px(260.))
                            .border_1()
                            .border_color(tokens.separator)
                            .overflow_y_scrollbar()
                            .child(v_flex().children(rows)),
                    )
                    .child(settings),
            )
            .child(
                v_flex()
                    .pt_1()
                    .gap_1()
                    .child("Don't ask again notifications:")
                    .child(
                        // The reference app's table shows no empty text
                        decorated(do_not_ask, div(), cx).h(px(120.)),
                    ),
            )
    }

    /// The selected notification group's settings, beside the list.
    fn notification_settings(
        &self,
        group: &'static NotificationGroup,
        balloons: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let choices = match group.tool_window {
            true => POPUPS_WITH_TOOL_WINDOW,
            false => POPUPS,
        };
        // with balloons off, every group shows no popup
        let popup = match balloons {
            true => self.opt_combo(group.popup, choices, group.default_popup, 180., cx),
            false => disabled_combo(group.popup, "No popup".to_owned(), 180.),
        };
        let screen_readers = self.flag("accessibility.screenReaders", false);
        let (sound, sound_default) = BOUND_SOUNDS
            .iter()
            .find(|(name, _)| *name == group.name)
            .map_or((group.sound, false), |(_, signal)| {
                (*signal, screen_readers)
            });
        // the screen reader announces notifications instead of reading them aloud
        let announcer = screen_readers && self.flag("accessibility.announceNotifications", true);
        let read_aloud = match announcer {
            true => disabled_check(group.read_aloud, "Read aloud", false, cx),
            false => self
                .opt_check(group.read_aloud, "Read aloud", false, cx)
                .into_any_element(),
        };
        v_flex()
            .gap_2()
            .p(px(2.))
            .child(labeled("Popup type:", popup.into_any_element()))
            .child(self.opt_check(group.log, "Show in tool window", true, cx))
            .child(self.opt_check(sound, "Play sound", sound_default, cx))
            .child(read_aloud)
    }

    fn quick_lists_page(&self, cx: &mut Context<Self>) -> Div {
        let actions = h_flex()
            .child(action("quick-lists-add", "general/add", "Add", true))
            .child(action(
                "quick-lists-remove",
                "general/remove",
                "Remove",
                false,
            ));
        h_flex()
            .h(px(520.))
            .gap_2()
            .child(
                decorated(actions, empty_text("No quick lists", cx), cx)
                    .w(relative(0.3))
                    .h_full(),
            )
            .child(div().flex_1().h_full().flex().items_center().px(px(25.)).child(
                "Quick Lists allow you to define commonly used groups of actions (for example, refactoring or VCS actions) and to assign keyboard shortcuts to such groups.",
            ))
    }

    fn required_plugins_page(&self, cx: &mut Context<Self>) -> Div {
        let actions = h_flex()
            .child(action("required-plugins-add", "general/add", "Add", true))
            .child(action(
                "required-plugins-remove",
                "general/remove",
                "Remove",
                false,
            ))
            .child(action(
                "required-plugins-edit",
                "general/edit",
                "Edit",
                false,
            ));
        v_flex()
            .gap(px(5.))
            .child(
                v_flex()
                    .child("Specify a list of plugins required for your project.")
                    .child(
                        "Spork will notify you if a required plugin is missing or needs an update.",
                    ),
            )
            .child(decorated(actions, empty_text("Nothing to show", cx), cx).h(px(460.)))
    }

    fn trusted_locations_page(&self, cx: &mut Context<Self>) -> Div {
        let actions = h_flex()
            .child(action("trusted-add", "general/add", "Add", true))
            .child(action("trusted-remove", "general/remove", "Remove", false))
            .child(action("trusted-up", "general/moveUp", "Move Up", false))
            .child(action(
                "trusted-down",
                "general/moveDown",
                "Move Down",
                false,
            ));
        v_flex()
            .gap_2()
            .child("Projects and files located under these local directories will be considered as trusted")
            .child(decorated(actions, empty_text("Nothing to show", cx), cx).h(px(460.)))
    }

    fn path_variables_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let actions = h_flex()
            .child(action("path-variables-add", "general/add", "Add", true))
            .child(action(
                "path-variables-remove",
                "general/remove",
                "Remove",
                false,
            ))
            .child(action("path-variables-edit", "general/edit", "Edit", false));
        let table = v_flex()
            .size_full()
            .child(table_header(&[("Name", Some(240.)), ("Value", None)], cx))
            .child(div().flex_1().child(empty_text("No path variables", cx)));
        v_flex()
            .gap_3()
            .child(decorated(actions, table, cx).h(px(420.)))
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .child("Ignored Variables:")
                            .child(self.opt_text("pathVariables.ignored", "", 460., window, cx)),
                    )
                    .child(
                        div()
                            .pl(px(120.))
                            .child(hint("Use ; to separate ignored variables", cx)),
                    ),
            )
    }

    fn presentation_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let additional = self.flag("presentationAssistant.additional", false);
        let column = 90.;
        let options = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(
                h_flex()
                    .child(row_label("Popup size:", column))
                    .child(self.opt_combo(
                        "presentationAssistant.popupSize",
                        &["Small", "Medium", "Large"],
                        "Medium",
                        160.,
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(row_label("Display for:", column - 8.))
                    .child(self.opt_text("presentationAssistant.duration", "4", 60., window, cx))
                    .child("seconds"),
            )
            .child(
                h_flex()
                    .child(row_label("Position:", column))
                    .child(self.opt_combo(
                        "presentationAssistant.position",
                        POSITIONS,
                        "Bottom Center",
                        160.,
                        cx,
                    )),
            );
        let main_keymap = h_flex()
            .gap_2()
            .child(row_label("Main:", 100.))
            .child(self.opt_combo(
                "presentationAssistant.mainKeymap",
                KEYMAPS,
                "macOS",
                200.,
                cx,
            ))
            .child("Label:")
            .child(self.opt_text("presentationAssistant.mainLabel", "macOS", 140., window, cx));
        let additional_keymap = h_flex()
            .gap_2()
            .child(div().w(px(100.)).child(self.opt_check(
                "presentationAssistant.additional",
                "Additional:",
                false,
                cx,
            )))
            .child(self.behavior_combo_if(
                additional,
                "presentationAssistant.additionalKeymap",
                KEYMAPS,
                "Windows",
                200.,
                cx,
            ))
            .child(label_if(additional, "Label:", cx))
            .child(self.text_if(
                additional,
                "presentationAssistant.additionalLabel",
                "Win/Linux",
                140.,
                window,
                cx,
            ));
        v_flex()
            .gap_3()
            .child(self.opt_check(
                "presentationAssistant.enabled",
                "Show action names and shortcuts in popup",
                false,
                cx,
            ))
            .child(options)
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_3()
                    .child(titled("Keymaps", cx))
                    .child(
                        v_flex()
                            .pl(px(INDENT))
                            .gap_2()
                            .child(main_keymap)
                            .child(additional_keymap),
                    ),
            )
    }

    fn date_formats_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let overriding = self.flag("dateFormats.override", false);
        // shortcut: the preview follows the pattern as applied, not as typed; the field's text
        // isn't readable here until Apply, which a live preview would need
        let preview = date_preview(
            &self.value("dateFormats.pattern", "dd MMM yyyy"),
            self.flag("dateFormats.use24Hours", true),
        );
        v_flex()
            .gap_3()
            .child(self.opt_check(
                "dateFormats.override",
                "Override system date and time format",
                false,
                cx,
            ))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(label_if(overriding, "Date format:", cx))
                            .child(self.text_if(
                                overriding,
                                "dateFormats.pattern",
                                "dd MMM yyyy",
                                140.,
                                window,
                                cx,
                            ))
                            // The reference app shows the patterns' help only while the format is editable
                            .when(overriding, |row| {
                                row.child(web_link(
                                    "date-patterns",
                                    "Date patterns",
                                    DATE_PATTERNS,
                                    cx,
                                ))
                            }),
                    )
                    .child(self.behavior_check_if(
                        overriding,
                        "dateFormats.use24Hours",
                        "Use 24-hour time",
                        true,
                        cx,
                    ))
                    .child(hint(preview, cx)),
            )
            .child(
                v_flex()
                    .pt_2()
                    .gap_1()
                    .child(self.opt_check("dateFormats.pretty", "Use pretty formatting", true, cx))
                    .child(comment(
                        "Replace numeric date with Today, Yesterday, and 10 minutes ago",
                        cx,
                    )),
            )
    }

    fn proxy_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let mode = self.value("httpProxy.mode", NO_PROXY);
        let auto = mode == AUTO_DETECT;
        let manual = mode == MANUAL;
        let pac = auto && self.flag("httpProxy.usePacUrl", false);
        let authentication = manual && self.flag("httpProxy.authentication", false);
        let column = 100.;
        let password = self.view_input("httpProxy.password", "", true, window, cx);
        let auto_options = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(self.behavior_check_if(
                        auto,
                        "httpProxy.usePacUrl",
                        "Automatic proxy configuration URL:",
                        false,
                        cx,
                    ))
                    .child(self.text_if(pac, "httpProxy.pacUrl", "", 300., window, cx)),
            )
            .child(
                div()
                    .pl(px(250.))
                    .child(hint("Example: https://example.com/wpad.pac", cx)),
            )
            .child(self.button_if(auto, "proxy-clear-passwords", "Clear Passwords", cx));
        let manual_field = |this: &mut Self,
                            label: &'static str,
                            key: &'static str,
                            default: &str,
                            width: f32,
                            window: &mut Window,
                            cx: &mut Context<Self>| {
            h_flex()
                .child(div().w(px(column)).child(label_if(manual, label, cx)))
                .child(this.text_if(manual, key, default, width, window, cx))
        };
        let host = manual_field(self, "Host name:", "httpProxy.host", "", 380., window, cx);
        let port = manual_field(
            self,
            "Port number:",
            "httpProxy.port",
            "80",
            80.,
            window,
            cx,
        );
        let exclude = manual_field(
            self,
            "No proxy for:",
            "httpProxy.exclude",
            "",
            380.,
            window,
            cx,
        )
        // shortcut: IntelliJ's expandable field opens a multi-line editor (its expandComponent icon,
        // expandAll here); Spork says it hasn't one yet
        .child(action(
            "proxy-exclude-expand",
            "general/expandAll",
            "Expand (⇧⏎)",
            manual,
        ));
        let login = h_flex()
            .child(
                div()
                    .w(px(column))
                    .child(label_if(authentication, "Login:", cx)),
            )
            .child(self.text_if(authentication, "httpProxy.login", "", 260., window, cx));
        // the password isn't kept in settings.conf: IntelliJ keeps it in the password store
        let password_field = match authentication {
            true => div()
                .w(px(260.))
                .child(spork_workbench::TextField::new(&password).mask_toggle()),
            false => disabled_field(String::new(), 260., cx),
        };
        let manual_options = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(
                h_flex()
                    .gap_4()
                    .child(self.radio_if(manual, "httpProxy.type", &["HTTP"], "HTTP", cx))
                    .child(self.radio_if(manual, "httpProxy.type", &["SOCKS"], "HTTP", cx)),
            )
            .child(host)
            .child(port)
            .child(exclude)
            .child(
                div()
                    .pl(px(column))
                    .child(hint("Example: *.example.com, 192.168.*", cx)),
            )
            .child(self.behavior_check_if(
                manual,
                "httpProxy.authentication",
                "Proxy authentication",
                false,
                cx,
            ))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_2()
                    .child(login)
                    .child(
                        h_flex()
                            .child(div().w(px(column)).child(label_if(
                                authentication,
                                "Password:",
                                cx,
                            )))
                            .child(password_field),
                    )
                    .child(div().pl(px(column)).child(self.behavior_check_if(
                        authentication,
                        "httpProxy.rememberPassword",
                        "Remember",
                        false,
                        cx,
                    ))),
            );
        v_flex()
            .gap_2()
            .child(self.opt_radio("httpProxy.mode", &[NO_PROXY], NO_PROXY, cx))
            .child(
                h_flex()
                    .justify_between()
                    .child(self.opt_radio("httpProxy.mode", &[AUTO_DETECT], NO_PROXY, cx))
                    .child(web_link(
                        "system-proxy-settings",
                        "System proxy settings…",
                        SYSTEM_PROXY,
                        cx,
                    )),
            )
            .child(auto_options)
            .child(self.opt_radio("httpProxy.mode", &[MANUAL], NO_PROXY, cx))
            .child(manual_options)
            .child(div().pt_2().child(self.opt_button(
                "proxy-check-connection",
                "Check Connection",
                cx,
            )))
    }

    fn language_page(&self, cx: &mut Context<Self>) -> Div {
        let column = 70.;
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .child(row_label("Language:", column))
                    .child(self.opt_combo(
                        "languageAndRegion.language",
                        &["English"],
                        "English",
                        240.,
                        cx,
                    ))
                    .child(hint("Requires restart", cx)),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(row_label("Region:", column))
                            .child(self.opt_combo(
                                "languageAndRegion.region",
                                REGIONS,
                                "Not specified",
                                240.,
                                cx,
                            )),
                    )
                    .child(
                        v_flex()
                            .pl(px(column + 8.))
                            .child(hint(
                                // The reference app names its vendor's marketplace here
                                "Select a region to ensure that licensing, the plugin marketplace, and",
                                cx,
                            ))
                            .child(hint(
                                "other region-specific features and links work correctly. Requires",
                                cx,
                            ))
                            .child(
                                h_flex()
                                    .text_xs()
                                    .child(hint("restart. See the ", cx))
                                    .child(web_link(
                                        "region-help",
                                        "documentation",
                                        REGION_HELP,
                                        cx,
                                    ))
                                    .child(hint(" for details.", cx)),
                            ),
                    ),
            )
    }

    fn passwords_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let keepass = self.value("passwords.store", KEYCHAIN) == KEEPASS;
        let pgp = keepass && self.flag("passwords.usePgpKey", false);
        let database = crate::repos::config_dir()
            .map(|dir| dir.join("c.kdbx").display().to_string())
            .unwrap_or_default();
        let database_actions = match keepass {
            true => ActionButton::new("passwords-database-actions", "general/settings")
                .dropdown_menu(|menu, _, _| {
                    ["Clear", "Import", "Set Master Password"].into_iter().fold(
                        menu,
                        |menu, label| {
                            menu.item(
                                PopupMenuItem::new(label)
                                    .on_click(move |_, window, cx| not_yet(label, window, cx)),
                            )
                        },
                    )
                })
                .into_any_element(),
            false => ActionButton::new("passwords-database-actions", "general/settings")
                .disabled(true)
                .into_any_element(),
        };
        let keepass_options = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(label_if(keepass, "Database:", cx))
                    .child(self.opt_path(
                        "passwords.database",
                        &database,
                        400.,
                        keepass,
                        true,
                        window,
                        cx,
                    ))
                    .child(database_actions),
            )
            .child(div().pl(px(70.)).child(
                hint(
                    "Stored using weak encryption. It is recommended to store on encrypted volume for additional security.",
                    cx,
                )
                // greyed with the database it's about
                .when(!keepass, |hint| hint.text_color(Tokens::get(cx).text_disabled)),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(self.behavior_check_if(
                        keepass,
                        "passwords.usePgpKey",
                        "Protect master password using PGP key (No keys configured)",
                        false,
                        cx,
                    ))
                    .child(self.behavior_combo_if(pgp, "passwords.pgpKey", &[], "", 200., cx)),
            );
        v_flex().gap_2().child("Save passwords:").child(
            v_flex()
                .pl(px(INDENT))
                .gap_2()
                .child(self.opt_radio("passwords.store", &[KEYCHAIN], KEYCHAIN, cx))
                .child(self.opt_radio("passwords.store", &[KEEPASS], KEYCHAIN, cx))
                .child(keepass_options)
                .child(self.opt_radio(
                    "passwords.store",
                    &["Do not save, forget passwords after restart"],
                    KEYCHAIN,
                    cx,
                )),
        )
    }

    fn certificates_page(&self, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let actions = h_flex()
            .child(action("certificates-add", "general/add", "Add", true))
            .child(action(
                "certificates-remove",
                "general/remove",
                "Remove",
                false,
            ));
        v_flex()
            .gap_3()
            .child(self.opt_check(
                "certificates.acceptAutomatically",
                "Accept non-trusted certificates automatically",
                false,
                cx,
            ))
            .child(
                v_flex()
                    .h(px(500.))
                    .child(
                        v_flex()
                            .h(relative(0.3))
                            .gap_1()
                            .pb_2()
                            .child("Accepted certificates:")
                            .child(
                                decorated(actions, empty_text("No certificates", cx), cx)
                                    .flex_1()
                                    .min_h_0(),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .border_t_1()
                            .border_color(tokens.separator)
                            .child(empty_text("No certificate selected", cx)),
                    ),
            )
    }

    fn updates_page(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .gap_3()
            .child(
                v_flex()
                    .pb_1()
                    .child(format!("Current version: Spork {}", env!("CARGO_PKG_VERSION"))),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(self.opt_check("updates.check", "Check for Spork updates", true, cx))
                    .child(comment("Spork does not currently do EAP releases.", cx)),
            )
            .child(self.opt_check("updates.plugins", "Check for plugin updates", true, cx))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_1()
                    .child(self.opt_check(
                        "updates.pluginsAutomatically",
                        "Update plugins automatically",
                        false,
                        cx,
                    ))
                    .child(comment(
                        "Updates will be downloaded in the background and applied after restart automatically",
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .pt_2()
                    .gap_3()
                    .child(self.opt_button("updates-check-now", "Check for Updates…", cx))
                    .child(hint("Last checked: Never", cx)),
            )
            .child(self.opt_check(
                "updates.whatsNew",
                "Show What's New in the editor after a Spork update",
                true,
                cx,
            ))
    }

    fn plugins_page(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        // shortcut: opens on Installed, where IntelliJ remembers the last tab (Marketplace at
        // first); Spork can't load the Marketplace
        let installed = self.selected("plugins.tab", cx).unwrap_or(1) == 1;
        let mut tabs = h_flex().gap_1();
        for (ix, label) in ["Marketplace", "Installed"].into_iter().enumerate() {
            tabs = tabs.child(
                spork_workbench::shell::header_tab(
                    SharedString::from(format!("plugins-tab-{ix}")),
                    label,
                    installed == (ix == 1),
                    cx,
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    with_view(cx.entity_id(), |view| {
                        view.selected.insert("plugins.tab", ix)
                    });
                    cx.notify();
                })),
            );
        }
        let gear = ActionButton::new("plugins-gear", "general/settings")
            .tooltip("Manage Repositories, Configure Proxy or Install Plugin from Disk")
            .dropdown_menu(|menu, _, _| {
                let item = |label: &'static str| {
                    PopupMenuItem::new(label)
                        .on_click(move |_, window, cx| not_yet(label, window, cx))
                };
                menu.item(item("Manage Plugin Repositories…"))
                    .item(item("HTTP Proxy Settings…"))
                    .item(item("Manage Plugin Certificates…"))
                    .separator()
                    .item(item("Install Plugin from Disk…"))
                    .separator()
                    .item(item("Disable All Downloaded Plugins").disabled(true))
                    .item(item("Enable All Downloaded Plugins").disabled(true))
            });
        let search_id = match installed {
            true => "plugins.installed.search",
            false => "plugins.marketplace.search",
        };
        let search = self.view_input(search_id, "Type / to see options", false, window, cx);
        let query = search.read(cx).value().trim().to_lowercase();
        let search_field = spork_workbench::TextField::new(&search)
            .prefix(spork_workbench::icon("general/search").size(px(16.)))
            .suffix(
                ActionButton::new("plugins-search-options", "general/filter")
                    .tooltip("Search Options"),
            )
            .cleanable(true);
        let (list, details) = match installed {
            true => self.installed_plugins(&query, cx),
            false => (
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .text_color(tokens.text_muted)
                    .child("Marketplace plugins are not loaded.")
                    .child("Check the internet connection.")
                    .child(
                        spork_workbench::controls::link("plugins-refresh", "Refresh", cx)
                            .on_click(|_, window, cx| not_yet("Refresh", window, cx)),
                    ),
                empty_text("Select plugin to preview details", cx),
            ),
        };
        v_flex()
            .h(px(560.))
            .gap_2()
            .child(h_flex().justify_between().child(tabs).child(gear))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .border_1()
                    .border_color(tokens.separator)
                    .child(
                        v_flex()
                            .w(relative(0.45))
                            .h_full()
                            .border_r_1()
                            .border_color(tokens.separator)
                            .child(div().p_2().child(search_field))
                            .child(list),
                    )
                    .child(div().flex_1().h_full().child(details)),
            )
    }

    /// The Installed tab's list, by category, and the selected plugin's details.
    fn installed_plugins(&self, query: &str, cx: &mut Context<Self>) -> (Div, Div) {
        let tokens = Tokens::get(cx);
        let selected = self.selected("plugins.installed", cx);
        let mut rows = Vec::new();
        let mut shown: Vec<&'static Plugin> = Vec::new();
        for &(category, plugins) in PLUGINS {
            let matching: Vec<&'static Plugin> = plugins
                .iter()
                .filter(|plugin| plugin.name.to_lowercase().contains(query))
                .collect();
            if matching.is_empty() {
                continue;
            }
            let enabled = matching
                .iter()
                .filter(|plugin| self.flag(plugin.key, true))
                .count();
            let toggle_all = match enabled {
                0 => "Enable all",
                _ => "Disable all",
            };
            let keys: Vec<&'static str> = matching.iter().map(|plugin| plugin.key).collect();
            let value = if enabled == 0 { "true" } else { "false" };
            rows.push(
                h_flex()
                    .h(px(28.))
                    .px_3()
                    .justify_between()
                    .bg(tokens.island)
                    .child(div().font_weight(FontWeight::BOLD).child(format!(
                        "{category} ({enabled} of {} enabled)",
                        matching.len()
                    )))
                    .child(
                        spork_workbench::controls::link(
                            SharedString::from(format!("plugins-toggle-{category}")),
                            toggle_all,
                            cx,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            for key in &keys {
                                this.set_value(key, value);
                            }
                            cx.notify();
                        })),
                    )
                    .into_any_element(),
            );
            for plugin in matching {
                let ix = shown.len();
                shown.push(plugin);
                rows.push(
                    self.list_row("plugins.installed", ix, selected == Some(ix), cx)
                        .h(px(48.))
                        .px_3()
                        .gap_3()
                        .child(spork_workbench::image("nodes/pluginLogo", cx).size(px(32.)))
                        .child(div().flex_1().min_w_0().truncate().child(plugin.name))
                        .child(self.opt_check(plugin.key, "", true, cx))
                        .into_any_element(),
                );
            }
        }
        let list = match rows.is_empty() {
            true => v_flex().flex_1().child(empty_text("Nothing found.", cx)),
            false => v_flex().flex_1().min_h_0().child(
                div()
                    .id("plugins-installed-list")
                    .size_full()
                    .overflow_y_scrollbar()
                    .child(v_flex().children(rows)),
            ),
        };
        let details = match selected.and_then(|ix| shown.get(ix)) {
            Some(plugin) => v_flex()
                .id("plugin-details")
                .size_full()
                .p_4()
                .gap_3()
                .overflow_y_scrollbar()
                .child(
                    h_flex()
                        .gap_3()
                        .child(spork_workbench::image("nodes/pluginLogo", cx).size(px(80.)))
                        .child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_h3()
                                        .font_weight(FontWeight::BOLD)
                                        .child(plugin.name),
                                )
                                .child(div().text_color(tokens.text_muted).child("Bundled")),
                        ),
                )
                .child(div().child(plugin.description))
                .into_any_element(),
            None => empty_text("Select plugin to preview details", cx).into_any_element(),
        };
        (list, div().size_full().child(details))
    }
}

/// A row of Keymap's tree: a group of actions, or an action and its shortcut.
enum KeymapNode {
    Group(SharedString, Vec<KeymapNode>),
    Action(SharedString, Box<dyn Action>),
}

impl KeymapNode {
    fn name(&self) -> &SharedString {
        match self {
            KeymapNode::Group(name, _) | KeymapNode::Action(name, _) => name,
        }
    }

    /// Whether the action, or one under the group, has `query` in its name.
    fn finds(&self, query: &str) -> bool {
        match self {
            KeymapNode::Action(name, _) => name.to_lowercase().contains(query),
            KeymapNode::Group(_, children) => children.iter().any(|child| child.finds(query)),
        }
    }
}

/// A menu's items as Keymap's rows: separators left out, a submenu a group.
fn keymap_menu(items: Vec<MenuItem>) -> Vec<KeymapNode> {
    items
        .into_iter()
        .filter_map(|item| match item {
            MenuItem::Action { name, action, .. } => Some(KeymapNode::Action(name, action)),
            MenuItem::Submenu(menu) => Some(KeymapNode::Group(menu.name, keymap_menu(menu.items))),
            _ => None,
        })
        .collect()
}

/// The names of a menu's actions, its submenus' too.
fn menu_names(items: &[MenuItem], out: &mut std::collections::HashSet<SharedString>) {
    for item in items {
        match item {
            MenuItem::Action { name, .. } => {
                out.insert(name.clone());
            }
            MenuItem::Submenu(menu) => menu_names(&menu.items, out),
            _ => {}
        }
    }
}

/// The reference app's Keymap tree, its top groups in its order, holding Spork's actions: the menu bar
/// under Main Menu, its text editing under Editor Actions, the Git menu under Version Control
/// Systems, every other command under Other. The groups Spork has nothing for stay empty.
fn keymap_tree() -> Vec<KeymapNode> {
    let mut in_menus = std::collections::HashSet::new();
    let mut editing = Vec::new();
    for menu in crate::shell::all_menus() {
        menu_names(&menu.items, &mut in_menus);
        for item in menu.items {
            if let MenuItem::Action {
                name,
                action,
                os_action: Some(_),
                ..
            } = item
            {
                editing.push(KeymapNode::Action(name, action));
            }
        }
    }
    let git = crate::shell::all_menus()
        .into_iter()
        .find(|menu| menu.name == "Git")
        .map_or_else(Vec::new, |menu| keymap_menu(menu.items));
    let main_menu = crate::shell::all_menus()
        .into_iter()
        .map(|menu| KeymapNode::Group(menu.name, keymap_menu(menu.items)))
        .collect();
    let other = crate::shell::commands()
        .into_iter()
        .filter(|(name, _)| !in_menus.contains(name))
        .map(|(name, action)| KeymapNode::Action(name, action))
        .collect();
    let group = |name: &'static str, children| KeymapNode::Group(name.into(), children);
    vec![
        group("Editor Actions", editing),
        group("Main Menu", main_menu),
        group("Tool Windows", Vec::new()),
        group("External Tools", Vec::new()),
        group("Version Control Systems", vec![group("Git", git)]),
        group("External Build Systems", Vec::new()),
        group("Macros", Vec::new()),
        group("Intentions", Vec::new()),
        group("Quick Lists", Vec::new()),
        group("Plugins", Vec::new()),
        group("Other", other),
    ]
}

/// The paths of every group under `nodes`.
fn keymap_groups(nodes: &[KeymapNode], parent: &str, out: &mut Vec<String>) {
    for node in nodes {
        if let KeymapNode::Group(name, children) = node {
            let path = format!("{parent}/{name}");
            keymap_groups(children, &path, out);
            out.push(path);
        }
    }
}

impl SettingsWindow {
    /// The reference app's Keymap: the keymap, then its tree of actions with their shortcuts.
    fn keymap_page(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let filter = self.view_input("keymap.filter", "", false, window, cx);
        let query = filter.read(cx).value().trim().to_lowercase();
        let open = with_view(cx.entity_id(), |view| view.keymap_open.clone());
        let tree = keymap_tree();
        let mut rows = Vec::new();
        self.keymap_rows(&tree, "", 0, &query, &open, &mut rows, window, cx);
        let mut all = Vec::new();
        keymap_groups(&tree, "", &mut all);
        let gear = ActionButton::new("keymap-actions", "general/settings")
            .popup_marker()
            .dropdown_menu(|menu, _, _| {
                menu.item(
                    PopupMenuItem::new("Duplicate…")
                        .on_click(|_, window, cx| not_yet("Duplicate…", window, cx)),
                )
                .item(PopupMenuItem::new("Rename…").disabled(true))
                .item(PopupMenuItem::new("Restore Defaults").disabled(true))
                .item(PopupMenuItem::new("Delete…").disabled(true))
            });
        let toolbar = h_flex()
            .gap(px(2.))
            .child(
                ActionButton::new("keymap-expand", "general/expandAll")
                    .tooltip("Expand All (⌘=)")
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let all = all.clone();
                        with_view(cx.entity_id(), |view| view.keymap_open.extend(all));
                        cx.notify();
                    })),
            )
            .child(
                ActionButton::new("keymap-collapse", "general/collapseAll")
                    .tooltip("Collapse All (⌘-)")
                    .on_click(cx.listener(|_, _, _, cx| {
                        with_view(cx.entity_id(), |view| view.keymap_open.clear());
                        cx.notify();
                    })),
            )
            .child(action("keymap-edit", "general/edit", "Edit Shortcut", false).popup_marker())
            .child(action(
                "keymap-conflicts",
                "status/warning",
                "Show conflicts with system shortcuts",
                true,
            ));
        let search = h_flex()
            .gap_2()
            .child(
                div().w(px(220.)).child(
                    spork_workbench::TextField::new(&filter)
                        .prefix(spork_workbench::icon("general/search").size(px(16.)))
                        .cleanable(true),
                ),
            )
            .child(action(
                "keymap-find-shortcut",
                "actions/shortcutFilter",
                "Find Actions by Shortcut",
                true,
            ));
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().w(px(220.)).child(spork_workbench::controls::combo(
                        "keymap",
                        "macOS",
                        false,
                        |menu, _, _| menu.item(PopupMenuItem::new("macOS").checked(true)),
                    )))
                    .child(gear),
            )
            .child(
                h_flex().child(
                    spork_workbench::controls::link(
                        "keymap-more",
                        "Get more keymaps in Settings | Plugins",
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.show("Plugins", cx))),
                ),
            )
            .child(
                v_flex()
                    .pt_2()
                    .gap_1()
                    .border_t_1()
                    .border_color(tokens.separator)
                    .child(h_flex().justify_between().child(toolbar).child(search))
                    .child(
                        div()
                            .id("keymap-tree")
                            .h(px(440.))
                            .border_1()
                            .border_color(tokens.separator)
                            .overflow_y_scrollbar()
                            .child(v_flex().children(rows)),
                    ),
            )
    }

    /// The rows of `nodes` and of the groups open under them; the filter opens every group
    /// that holds an action it finds.
    #[allow(clippy::too_many_arguments)]
    fn keymap_rows(
        &self,
        nodes: &[KeymapNode],
        parent: &str,
        depth: usize,
        query: &str,
        open: &std::collections::HashSet<String>,
        rows: &mut Vec<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tokens = Tokens::get(cx);
        for node in nodes {
            if !query.is_empty() && !node.finds(query) {
                continue;
            }
            let path = format!("{parent}/{}", node.name());
            let children = match node {
                KeymapNode::Group(_, children) => Some(children),
                KeymapNode::Action(..) => None,
            };
            let expanded = children.is_some() && (!query.is_empty() || open.contains(&path));
            let chevron = children.is_some_and(|children| !children.is_empty());
            let row = h_flex()
                .id(SharedString::from(format!("keymap-{path}")))
                .role(Role::TreeItem)
                .aria_label(node.name().clone())
                .h(px(metrics::ROW_HEIGHT))
                .pl(px(4. + depth as f32 * 18.))
                .pr_2()
                .gap_1()
                .flex_none()
                .hover(move |row| row.bg(tokens.selection_hover))
                .child(div().size(px(16.)).flex_none().when(chevron, |slot| {
                    slot.child(
                        spork_workbench::icon(match expanded {
                            true => "general/chevronDown",
                            false => "general/chevronRight",
                        })
                        .size(px(16.))
                        .text_color(tokens.icon),
                    )
                }))
                .when(children.is_some(), |row| {
                    row.child(
                        spork_workbench::icon("nodes/folder")
                            .size(px(16.))
                            .text_color(tokens.icon),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(node.name().clone()),
                );
            let row = match node {
                KeymapNode::Action(_, action) => row
                    .children(gpui_kit::component::kbd::Kbd::binding_for_action(
                        &**action, None, window,
                    ))
                    .into_any_element(),
                KeymapNode::Group(..) => row
                    .cursor_pointer()
                    .on_click(cx.listener({
                        let path = path.clone();
                        move |_, _, _, cx| {
                            with_view(cx.entity_id(), |view| {
                                if !view.keymap_open.remove(&path) {
                                    view.keymap_open.insert(path.clone());
                                }
                            });
                            cx.notify();
                        }
                    }))
                    .into_any_element(),
            };
            rows.push(row);
            if let (true, Some(children)) = (expanded, children) {
                self.keymap_rows(children, &path, depth + 1, query, open, rows, window, cx);
            }
        }
    }
}

/// What a click on an action Spork doesn't have yet says, as `opt_button`'s.
fn not_yet(label: &str, window: &mut Window, cx: &mut App) {
    window.push_notification(
        Notification::info(format!(
            "{} isn't in Spork yet.",
            label.trim_end_matches('…')
        )),
        cx,
    )
}

/// A toolbar button for an action Spork doesn't have yet: off as IntelliJ has it when `enabled`
/// is false, else a click says so.
fn action(
    id: impl Into<ElementId>,
    icon: &'static str,
    tooltip: &'static str,
    enabled: bool,
) -> ActionButton {
    ActionButton::new(id, icon)
        .tooltip(tooltip)
        .disabled(!enabled)
        .on_click(move |_, window, cx| not_yet(tooltip, window, cx))
}

/// IntelliJ's sound preview button.
fn preview_sound(id: SharedString) -> ActionButton {
    action(id, "actions/playSound", "Preview Sound", true)
}

/// IntelliJ's ToolbarDecorator: the actions above a list or table, one border around both.
fn decorated(actions: Div, content: impl IntoElement, cx: &App) -> Div {
    let tokens = Tokens::get(cx);
    v_flex()
        .border_1()
        .border_color(tokens.separator)
        .child(
            actions
                .flex_none()
                .p(px(4.))
                .gap(px(2.))
                .border_b_1()
                .border_color(tokens.separator),
        )
        .child(v_flex().flex_1().min_h_0().child(content))
}

/// A table's header: its columns' names, each `width` px wide, or the rest.
fn table_header(columns: &[(&'static str, Option<f32>)], cx: &App) -> Div {
    let tokens = Tokens::get(cx);
    h_flex()
        .flex_none()
        .h(px(metrics::ROW_HEIGHT))
        .px_2()
        .border_b_1()
        .border_color(tokens.separator)
        .text_color(tokens.text_secondary)
        .children(columns.iter().map(|(name, width)| match width {
            Some(width) => div().w(px(*width)).child(*name),
            None => div().flex_1().child(*name),
        }))
}

fn toolbar_separator(cx: &App) -> Div {
    div()
        .w(px(1.))
        .h(px(16.))
        .mx(px(3.))
        .bg(Tokens::get(cx).separator)
}

/// A list's empty text: grey, in the middle, as IntelliJ's StatusText.
fn empty_text(text: &'static str, cx: &App) -> Div {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(Tokens::get(cx).text_muted)
        .child(text)
}

/// IntelliJ's comment beside a control (`commentRight`) or under a row.
fn hint(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_xs()
        .text_color(Tokens::get(cx).text_muted)
        .child(text.into())
}

/// A row's label in a column `width` px wide, as IntelliJ aligns a panel's row labels.
fn row_label(text: &'static str, width: f32) -> Div {
    div().flex_none().w(px(width)).child(text)
}

/// A label, grey while its row is off (IntelliJ's `enabledIf` greys the row).
fn label_if(enabled: bool, text: &'static str, cx: &App) -> Div {
    div()
        .flex_none()
        .when(!enabled, |label| {
            label.text_color(Tokens::get(cx).text_disabled)
        })
        .child(text)
}

fn web_link(id: &'static str, label: &'static str, url: &'static str, cx: &App) -> Stateful<Div> {
    spork_workbench::controls::link(id, label, cx)
        .flex()
        .items_center()
        .gap(px(2.))
        .child(spork_workbench::icon("ide/externalLink").size(px(12.)))
        .on_click(move |_, _, cx| cx.open_url(url))
}

/// A checkbox IntelliJ shows disabled: greyed, not clickable.
fn disabled_check(key: &'static str, label: &'static str, on: bool, cx: &App) -> AnyElement {
    spork_workbench::controls::disabled_check(key, label, on, cx).into_any_element()
}

fn disabled_combo(key: &'static str, current: String, width: f32) -> Div {
    div().w(px(width)).child(spork_workbench::controls::combo(
        key,
        current,
        true,
        |menu, _, _| menu,
    ))
}

/// A text field IntelliJ shows disabled: its text greyed, not editable.
fn disabled_field(text: String, width: f32, cx: &App) -> Div {
    let tokens = Tokens::get(cx);
    h_flex()
        .w(px(width))
        .h(px(metrics::CONTROL_HEIGHT))
        .px(px(6.))
        .rounded(px(metrics::RADIUS))
        .border_1()
        .border_color(tokens.control_border_disabled)
        .text_color(tokens.text_disabled)
        .child(div().truncate().child(text))
}

/// IntelliJ's preview of a date pattern: its sample date, Friday 31 December 2100 at 23:59, in
/// `pattern` (SimpleDateFormat's day, month, year and weekday letters; 'quoted' text kept), then
/// the short time.
fn date_preview(pattern: &str, hours24: bool) -> String {
    let mut preview = String::new();
    let mut letters = pattern.chars().peekable();
    while let Some(letter) = letters.next() {
        if letter == '\'' {
            if letters.next_if_eq(&'\'').is_some() {
                preview.push('\'');
                continue;
            }
            while let Some(quoted) = letters.next() {
                if quoted != '\'' {
                    preview.push(quoted);
                } else if letters.next_if_eq(&'\'').is_some() {
                    preview.push('\'');
                } else {
                    break;
                }
            }
            continue;
        }
        let mut run = 1;
        while letters.next_if_eq(&letter).is_some() {
            run += 1;
        }
        match letter {
            'd' => preview.push_str(&format!("{:0run$}", 31)),
            'M' => preview.push_str(match run {
                1 | 2 => "12",
                3 => "Dec",
                _ => "December",
            }),
            'y' => preview.push_str(if run == 2 { "00" } else { "2100" }),
            'E' => preview.push_str(if run >= 4 { "Friday" } else { "Fri" }),
            other => (0..run).for_each(|_| preview.push(other)),
        }
    }
    let time = match hours24 {
        true => "23:59",
        false => "11:59\u{202F}PM",
    };
    format!("{preview} {time}")
}

#[cfg(test)]
mod tests {
    use super::date_preview;

    #[test]
    fn previews_intellij_sample_date() {
        assert_eq!(date_preview("dd MMM yyyy", true), "31 Dec 2100 23:59");
        assert_eq!(
            date_preview("EEEE, d MMMM yy 'at' ''", false),
            "Friday, 31 December 00 at ' 11:59\u{202F}PM"
        );
    }
}
