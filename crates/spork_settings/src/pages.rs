//! The pages over the IDE's settings: each option is IntelliJ's label and control over the
//! `settings.json` key Spork IDE honours it with (a JSON pointer), or over a setting one of Spork
//! IDE's crates keeps (`/spork/…`, [`crate::OWNED`]), so the dialog reads, edits, compares and
//! applies them all the same way. Each page has IntelliJ's options in IntelliJ's order, groups
//! and labels; those Spork IDE can't honour are left out (`docs/spork-settings-pages.md` lists
//! them, page by page).

use std::rc::Rc;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Value, json};
use spork_workbench::{Tokens, Typography as _};

use crate::SettingsDialog;

/// A value an option writes.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Lit {
    Bool(bool),
    Str(&'static str),
    Num(f64),
}

impl Lit {
    fn value(self) -> Value {
        match self {
            Lit::Bool(value) => Value::Bool(value),
            Lit::Str(value) => Value::String(value.to_owned()),
            Lit::Num(value) => json!(value),
        }
    }
}

pub(crate) enum Row {
    /// IntelliJ's titled separator over a group: the rows after it are indented, to the next
    /// title or [`Row::End`].
    Title(&'static str),
    /// IntelliJ's collapsible group, closed at first: its rows show once it is opened.
    Collapsible(&'static str),
    /// The end of a group: the rows after it aren't indented.
    End,
    /// The group's rows after it form its second column, as IntelliJ's `twoColumnsRow`: every
    /// second column of a page starts at the same place, 60 px after the widest first one.
    Right,
    Opt(Opt),
    /// Grey text under the option before it.
    Comment(&'static str),
    /// A link to another page of the dialog, by its path in the tree, on the row of the option
    /// before it.
    Link(&'static str, &'static str),
}

pub(crate) struct Opt {
    label: &'static str,
    path: &'static str,
    kind: Kind,
    /// Under the check box before it, as IntelliJ nests a dependent option: greyed while that
    /// check box is off.
    indent: bool,
    /// On the row of the option before it, as IntelliJ's rows of two cells.
    inline: bool,
    /// Text after the control: its unit.
    unit: &'static str,
    /// IntelliJ's context help after the option: a "?" whose tooltip says this.
    help: &'static str,
    /// The combo box's width, as IntelliJ sizes it to its longest choice; 0 for the least.
    width: f32,
}

pub(crate) enum Kind {
    /// A check box writing `on` or `off`.
    Check(Lit, Lit),
    /// A combo box of labelled values.
    Choice(&'static [(&'static str, Lit)]),
    /// Radio buttons under the label.
    Radios(&'static [(&'static str, Lit)]),
    /// A number field: the setting is its text over `scale`, with `decimals`.
    Number {
        min: f64,
        max: f64,
        scale: f64,
        decimals: usize,
        width: f32,
    },
    /// A font family combo box.
    Font,
    /// A fallback font combo box: the first of a list of families, or None.
    Fallback,
    /// A font weight combo box, by the weights' names.
    Weight,
    /// The theme combo box, with Sync with OS beside it when `sync`.
    Theme { sync: bool },
    /// The zoom combo box: the interface's text size against IntelliJ's 13 px.
    Zoom,
    /// Use custom font: the interface's font and size.
    CustomFont,
    /// A line height `{"custom": 1.2}` (Zed's "comfortable" and "standard" shown as numbers).
    LineHeight,
    /// Show line numbers: on or off, absolute or hybrid.
    LineNumbers,
    /// Visual guides: a list of columns.
    Guides,
    /// Autosave's two check boxes over one setting: when idle (with its seconds) or on leaving.
    Autosave { idle: bool },
    /// A text field writing `{key: text}`, `empty` when cleared.
    Wrapped {
        key: &'static str,
        empty: Option<Lit>,
        width: f32,
    },
    /// Environment variables as IntelliJ writes them, `NAME=value;NAME2=value2`.
    Env,
    /// The intentions turned off from their submenu in ⌥⏎'s list: a check box each, checked
    /// once turned back on.
    Intentions,
    /// Close button position: left or right (`tabs.close_position`), or none
    /// (`tabs.show_close_button`).
    CloseButton,
    /// A combo box of one choice, the one Spork IDE has of IntelliJ's; greyed while the check box
    /// it belongs to is off. It writes nothing.
    Fixed(&'static str),
}

const fn opt(label: &'static str, path: &'static str, kind: Kind) -> Row {
    Row::Opt(Opt {
        label,
        path,
        kind,
        indent: false,
        inline: false,
        unit: "",
        help: "",
        width: 0.,
    })
}

const fn check(label: &'static str, path: &'static str) -> Row {
    opt(label, path, Kind::Check(Lit::Bool(true), Lit::Bool(false)))
}

const fn check_as(label: &'static str, path: &'static str, on: Lit, off: Lit) -> Row {
    opt(label, path, Kind::Check(on, off))
}

const fn number(label: &'static str, path: &'static str, min: f64, max: f64) -> Row {
    opt(
        label,
        path,
        Kind::Number {
            min,
            max,
            scale: 1.,
            decimals: 0,
            width: 64.,
        },
    )
}

const fn indented(row: Row) -> Row {
    match row {
        Row::Opt(mut opt) => {
            opt.indent = true;
            Row::Opt(opt)
        }
        row => row,
    }
}

const fn unit(row: Row, unit: &'static str) -> Row {
    match row {
        Row::Opt(mut opt) => {
            opt.unit = unit;
            Row::Opt(opt)
        }
        row => row,
    }
}

const fn inline(row: Row) -> Row {
    match row {
        Row::Opt(mut opt) => {
            opt.inline = true;
            Row::Opt(opt)
        }
        row => row,
    }
}

const fn help(row: Row, text: &'static str) -> Row {
    match row {
        Row::Opt(mut opt) => {
            opt.help = text;
            Row::Opt(opt)
        }
        row => row,
    }
}

const fn width(row: Row, width: f32) -> Row {
    match row {
        Row::Opt(mut opt) => {
            opt.width = width;
            Row::Opt(opt)
        }
        row => row,
    }
}

const ALWAYS: Lit = Lit::Str("always");
const NEVER: Lit = Lit::Str("never");

/// IntelliJ's font size choices (`UIUtil.getStandardFontSizes`).
const FONT_SIZES: &[(&str, Lit)] = &[
    ("8", Lit::Num(8.)),
    ("9", Lit::Num(9.)),
    ("10", Lit::Num(10.)),
    ("11", Lit::Num(11.)),
    ("12", Lit::Num(12.)),
    ("13", Lit::Num(13.)),
    ("14", Lit::Num(14.)),
    ("16", Lit::Num(16.)),
    ("18", Lit::Num(18.)),
    ("20", Lit::Num(20.)),
    ("22", Lit::Num(22.)),
    ("24", Lit::Num(24.)),
    ("26", Lit::Num(26.)),
    ("28", Lit::Num(28.)),
    ("36", Lit::Num(36.)),
    ("48", Lit::Num(48.)),
    ("72", Lit::Num(72.)),
];

/// Appearance & Behavior › Appearance as IntelliJ IDEA 2026.3 has it (`AppearanceConfigurable`).
/// Left out: Editor color scheme (Zed's theme is its scheme too), Different tool window
/// background, Compact mode, Keep popups open for toggle items, Drag-and-drop with Alt pressed
/// only, Smooth scrolling, the mnemonics, Background Image…, the tool window bars' and names' options, the side-by-side
/// layouts, Remember size for each tool window, Presentation Mode and Antialiasing
/// (`docs/spork-settings-pages.md` says why).
pub(crate) const APPEARANCE: &[Row] = &[
    opt("Theme:", "/theme", Kind::Theme { sync: true }),
    opt("Use custom font:", "/ui_font_family", Kind::CustomFont),
    Row::Title("UI Options"),
    check_as(
        "Always show full path in window header",
        "/window_title_format",
        Lit::Str(FULL_PATH_TITLE),
        Lit::Str(TITLE),
    ),
    check(
        "Use project colors in main toolbar",
        "/ui_options/use_project_colors_in_toolbar",
    ),
    Row::Comment("Distinguish projects with different toolbar colors at a glance."),
    Row::Right,
    check(
        "Display icons in menu items",
        "/ui_options/show_icons_in_menus",
    ),
    Row::Title("Tree Views"),
    check_as(
        "Show indent guides",
        "/project_panel/indent_guides/show",
        ALWAYS,
        NEVER,
    ),
    Row::Right,
    // IntelliJ's compact tree indents against its usual ones (`Tree.leftChildIndent` and
    // `rightChildIndent`)
    check_as(
        "Use smaller indents",
        "/project_panel/indent_size",
        Lit::Num(12.),
        Lit::Num(20.),
    ),
    Row::Title("Tool Windows"),
    help(
        check_as(
            "Widescreen tool window layout",
            "/bottom_dock_layout",
            Lit::Str("contained"),
            Lit::Str("full"),
        ),
        "Maximize the height of vertical tool windows by limiting the width of horizontal tool windows",
    ),
];

/// The window's title (`window_title_format`): the project and the file's name, or its full
/// path.
const TITLE: &str = "${projectName}${separator}${fileName}";
const FULL_PATH_TITLE: &str = "${projectName}${separator}${filePath}";

/// Appearance & Behavior › Accessibility (`AccessibilityConfigurable`): its Vision group's Zoom.
/// Left out: contrast scrollbars, red-green vision deficiency, Screen Reader Mode, Sound Signals.
pub(crate) const ACCESSIBILITY: &[Row] = &[
    Row::Title("Vision"),
    opt("Zoom:", "/ui_font_size", Kind::Zoom),
];

pub(crate) const SYSTEM_SETTINGS: &[Row] = &[
    check("Confirm before exiting the IDE", "/confirm_quit"),
    Row::Title("Project"),
    check_as(
        "Reopen projects on startup",
        "/restore_on_startup",
        Lit::Str("last_session"),
        Lit::Str("none"),
    ),
    Row::Title("Autosave"),
    unit(
        opt(
            "Save files if the IDE is idle for",
            "/autosave",
            Kind::Autosave { idle: true },
        ),
        "seconds",
    ),
    opt(
        "Save files when switching to a different application or a built-in terminal",
        "/autosave",
        Kind::Autosave { idle: false },
    ),
];

pub(crate) const EDITOR_GENERAL: &[Row] = &[
    Row::Title("Mouse Control"),
    check(
        "Change font size with Command+Mouse Wheel in:",
        "/mouse_wheel_zoom",
    ),
    check(
        "Move code fragments with drag-and-drop",
        "/drag_and_drop_selection/enabled",
    ),
    Row::Title("Scroll Offset"),
    number(
        "Vertical scroll offset:",
        "/vertical_scroll_margin",
        0.,
        100.,
    ),
    number(
        "Horizontal scroll offset:",
        "/horizontal_scroll_margin",
        0.,
        100.,
    ),
    Row::Title("On Save"),
    check(
        "Remove trailing spaces on:",
        "/remove_trailing_whitespace_on_save",
    ),
    // Zed removes them from every line: IntelliJ's "All lines"
    inline(indented(width(
        opt(
            "Remove trailing spaces on:",
            "/remove_trailing_whitespace_on_save",
            Kind::Fixed("All lines"),
        ),
        128.,
    ))),
    check(
        "Ensure every saved file ends with a line break",
        "/ensure_final_newline_on_save",
    ),
];

pub(crate) const EDITOR_APPEARANCE: &[Row] = &[
    check("Caret blinking (ms):", "/cursor_blink"),
    check_as(
        "Use block caret",
        "/cursor_shape",
        Lit::Str("block"),
        Lit::Str("bar"),
    ),
    check(
        "Highlight occurrences of selected text",
        "/selection_highlight",
    ),
    check(
        "Show hard wrap and visual guides (configured in Code Style options)",
        "/show_wrap_guides",
    ),
    opt(
        "Show line numbers:",
        "/gutter/line_numbers",
        Kind::LineNumbers,
    ),
    check_as(
        "Show whitespaces",
        "/show_whitespaces",
        Lit::Str("all"),
        Lit::Str("none"),
    ),
    check("Show indent guides", "/indent_guides/enabled"),
];

pub(crate) const BREADCRUMBS: &[Row] = &[check("Show breadcrumbs", "/toolbar/breadcrumbs")];

/// Editor › General › Code Completion: the options Spork IDE honours. Match case and the
/// automatic insertion of single suggestions are left out (the editor has neither), so is
/// Machine Learning-Assisted Completion.
pub(crate) const CODE_COMPLETION: &[Row] = &[
    check(
        "Sort suggestions alphabetically",
        "/completion_sort_alphabetically",
    ),
    check("Show suggestions as you type", "/show_completions_on_input"),
    check(
        "Insert selected suggestion by pressing space, dot, or other context-dependent keys",
        "/completion_insert_on_characters",
    ),
    check(
        "Show the documentation popup in",
        "/show_completion_documentation",
    ),
    indented(unit(
        number("", "/completion_documentation_delay", 0., 10000.),
        "ms",
    )),
    Row::Title("Parameter Info"),
    check("Show the parameter info popup in", "/auto_signature_help"),
];

pub(crate) const CODE_FOLDING: &[Row] = &[check("Show code folding arrows", "/gutter/folds")];

/// Editor › Intentions: the language servers' intentions Spork IDE knows are those turned off
/// (IntelliJ's tree lists every intention of every language).
pub(crate) const INTENTIONS: &[Row] = &[opt("", "/disabled_code_actions", Kind::Intentions)];

pub(crate) const EDITOR_TABS: &[Row] = &[
    Row::Title("Appearance"),
    width(
        opt(
            "Tab placement:",
            "/tab_bar/show",
            Kind::Choice(&[("Top", Lit::Bool(true)), ("None", Lit::Bool(false))]),
        ),
        92.,
    ),
    check(
        "Show pinned tabs in a separate row",
        "/tab_bar/show_pinned_tabs_in_separate_row",
    ),
    check("Show file icon", "/tabs/file_icons"),
    width(
        opt(
            "Close button position:",
            "/tabs/close_position",
            Kind::CloseButton,
        ),
        80.,
    ),
    Row::Title("Opening Policy"),
    check("Enable preview tab", "/preview_tabs/enabled"),
    Row::Comment(
        "The preview tab is reused to show files selected with a single click in the Project tool window, and files opened during debugging.",
    ),
    Row::Title("Closing Policy"),
    number("Tab limit:", "/max_tabs", 1., 1000.),
    opt(
        "When the current tab is closed, activate:",
        "/tabs/activate_on_close",
        Kind::Radios(&[
            ("The tab on the left", Lit::Str("left_neighbour")),
            ("The tab on the right", Lit::Str("neighbour")),
            ("Most recently opened tab", Lit::Str("history")),
        ]),
    ),
];

pub(crate) const SMART_KEYS: &[Row] = &[
    check("Insert paired brackets (), [], {}, <>", "/use_autoclose"),
    check(
        "Surround selection on typing quote or brace",
        "/use_auto_surround",
    ),
    Row::Title("Enter"),
    check_as(
        "Smart indent",
        "/auto_indent",
        Lit::Str("syntax_aware"),
        Lit::Str("preserve_indent"),
    ),
    Row::End,
    width(
        opt(
            "Reformat on paste:",
            "/auto_indent_on_paste",
            Kind::Choice(&[
                ("None", Lit::Bool(false)),
                ("Indent block", Lit::Bool(true)),
            ]),
        ),
        146.,
    ),
];

pub(crate) const STICKY_LINES: &[Row] = &[check(
    "Show sticky lines while scrolling",
    "/sticky_scroll/enabled",
)];

pub(crate) const CODE_EDITING: &[Row] = &[
    Row::Title("Quick Documentation"),
    check(
        "Show quick documentation on hover",
        "/hover_popover_enabled",
    ),
    Row::Title("Editor Tooltips"),
    unit(
        number("Tooltip delay:", "/hover_popover_delay", 0., 5000.),
        "milliseconds",
    ),
];

/// The tooltip of Enable ligatures' context help (`ligatures.tooltip`).
const LIGATURES_HELP: &str = "Use several characters as a single ligature. Ligatures enhance code readability. Use the section below to preview the changes";

pub(crate) const EDITOR_FONT: &[Row] = &[
    width(opt("Font:", "/buffer_font_family", Kind::Font), 279.),
    opt(
        "Size:",
        "/buffer_font_size",
        Kind::Number {
            min: 4.,
            max: 72.,
            scale: 1.,
            decimals: 1,
            width: 44.,
        },
    ),
    inline(opt("Line height:", "/buffer_line_height", Kind::LineHeight)),
    help(
        check("Enable ligatures", "/buffer_font_features/calt"),
        LIGATURES_HELP,
    ),
    Row::Collapsible("Typography Settings"),
    width(
        opt("Main weight:", "/buffer_font_weight", Kind::Weight),
        160.,
    ),
    width(
        opt("Fallback font:", "/buffer_font_fallbacks", Kind::Fallback),
        160.,
    ),
    Row::Comment("Used for symbols not supported by the main font"),
];

pub(crate) const COLOR_SCHEME: &[Row] = &[
    opt("Scheme:", "/theme", Kind::Theme { sync: false }),
    Row::Link("Change IDE Theme…", "Appearance & Behavior/Appearance"),
];

pub(crate) const CODE_STYLE: &[Row] = &[
    width(
        opt(
            "Line separator:",
            "/line_ending",
            Kind::Choice(&[
                ("System-Dependent", Lit::Str("detect")),
                ("Unix and macOS (\\n)", Lit::Str("prefer_lf")),
                ("Windows (\\r\\n)", Lit::Str("prefer_crlf")),
            ]),
        ),
        176.,
    ),
    Row::Comment("Applied to new files"),
    unit(
        width(
            number("Hard wrap at:", "/preferred_line_length", 1., 1000.),
            200.,
        ),
        "columns",
    ),
    unit(
        width(opt("Visual guides:", "/wrap_guides", Kind::Guides), 200.),
        "columns",
    ),
    Row::Comment("Specify one guide (80) or several (80, 120)"),
];

pub(crate) const OTHER_FILE_TYPES: &[Row] = &[
    check("Use tab character", "/hard_tabs"),
    number("Tab size:", "/tab_size", 1., 16.),
];

pub(crate) const DATA_VIEWS: &[Row] = &[
    Row::Title("Editor"),
    check("Show values inline", "/inlay_hints/show_value_hints"),
];

pub(crate) const MARKDOWN: &[Row] = &[
    width(
        opt(
            "Default layout:",
            "/markdown_preview/open_markdown_files_in_preview",
            Kind::Choice(&[("Editor", Lit::Bool(false)), ("Preview", Lit::Bool(true))]),
        ),
        160.,
    ),
    width(
        opt(
            "Preview font size:",
            "/markdown_preview/font_size",
            Kind::Choice(FONT_SIZES),
        ),
        64.,
    ),
];

/// Tools › Actions on Save (`ActionsOnSaveConfigurable`): Reformat code is Zed's formatting on
/// save; IntelliJ's other actions (optimize imports, rearrange, cleanup, copyright, build) have no
/// engine behind them.
pub(crate) const ACTIONS_ON_SAVE: &[Row] = &[
    check_as(
        "Reformat code",
        "/format_on_save",
        Lit::Str("on"),
        Lit::Str("off"),
    ),
    Row::Link(
        "Configure autosave options…",
        "Appearance & Behavior/System Settings",
    ),
];

pub(crate) const TERMINAL: &[Row] = &[
    Row::Title("Project Settings"),
    opt(
        "Start directory:",
        "/terminal/working_directory",
        Kind::Wrapped {
            key: "always",
            empty: Some(Lit::Str("current_project_directory")),
            width: 420.,
        },
    ),
    opt("Environment variables:", "/terminal/env", Kind::Env),
    Row::Title("Font Settings"),
    width(opt("Font:", "/terminal/font_family", Kind::Font), 260.),
    inline(width(
        opt("Fallback:", "/terminal/font_fallbacks", Kind::Fallback),
        260.,
    )),
    opt(
        "Size:",
        "/terminal/font_size",
        Kind::Number {
            min: 4.,
            max: 72.,
            scale: 1.,
            decimals: 1,
            width: 44.,
        },
    ),
    inline(opt(
        "Line height:",
        "/terminal/line_height",
        Kind::LineHeight,
    )),
    Row::Title("Application Settings"),
    opt(
        "Shell path:",
        "/terminal/shell",
        Kind::Wrapped {
            key: "program",
            empty: Some(Lit::Str("system")),
            width: 420.,
        },
    ),
    check("Copy to clipboard on selection", "/terminal/copy_on_select"),
    check("Use Option as Meta key", "/terminal/option_as_meta"),
    width(
        opt(
            "Cursor shape:",
            "/terminal/cursor_shape",
            Kind::Choice(&[
                ("Block", Lit::Str("block")),
                ("Underline", Lit::Str("underline")),
                ("Vertical", Lit::Str("bar")),
            ]),
        ),
        120.,
    ),
];

pub(crate) const ADVANCED: &[Row] = &[
    Row::Title("IDE"),
    unit(
        number(
            "Duration of storing changes in Local History:",
            crate::LOCAL_HISTORY_DAYS,
            1.,
            365.,
        ),
        "days",
    ),
    Row::Comment(
        "Do not use Local History as the primary version control system since it may become corrupted if the IDE hangs",
    ),
    Row::Title("Terminal"),
    unit(
        number(
            "Terminal scrollback buffer size:",
            "/terminal/max_scroll_history_lines",
            0.,
            100_000.,
        ),
        "lines",
    ),
    Row::Title("User Interface"),
    check(
        "Use native file chooser dialog on Windows/macOS",
        "/use_system_path_prompts",
    ),
    Row::Comment(
        "When enabled, the IDE will use an OS-specific file chooser dialog instead of the custom one for opening files/directories.",
    ),
];

/// The settings a page's options write, for its changes.
pub(crate) fn paths(rows: &'static [Row]) -> impl Iterator<Item = &'static str> {
    rows.iter()
        .filter_map(|row| match row {
            Row::Opt(opt) if !matches!(opt.kind, Kind::Fixed(_)) => Some(opt),
            _ => None,
        })
        .flat_map(|opt| {
            let extra: &[&'static str] = match opt.kind {
                Kind::CustomFont => &["/ui_font_size"],
                Kind::LineNumbers => &["/relative_line_numbers"],
                Kind::CloseButton => &["/tabs/show_close_button"],
                _ => &[],
            };
            std::iter::once(opt.path).chain(extra.iter().copied())
        })
}

/// What the search finds a page by: its options' labels, titles, comments and links.
pub(crate) fn labels(rows: &'static [Row]) -> impl Iterator<Item = &'static str> {
    rows.iter()
        .flat_map(|row| -> Box<dyn Iterator<Item = &'static str>> {
            match row {
                Row::Title(text)
                | Row::Collapsible(text)
                | Row::Comment(text)
                | Row::Link(text, _) => Box::new(std::iter::once(*text)),
                Row::End | Row::Right => Box::new(std::iter::empty()),
                Row::Opt(opt) => Box::new(opt_labels(opt)),
            }
        })
}

/// An option's label and the texts beside it.
fn opt_labels(opt: &'static Opt) -> impl Iterator<Item = &'static str> {
    let extra: &'static [&'static str] = match opt.kind {
        Kind::Theme { sync: true } => &["Sync with OS"],
        Kind::CustomFont => &["Size:"],
        _ => &[],
    };
    let radios: &'static [(&'static str, Lit)] = match opt.kind {
        Kind::Radios(choices) => choices,
        _ => &[],
    };
    std::iter::once(opt.label)
        .chain(extra.iter().copied())
        .chain(radios.iter().map(|(text, _)| *text))
}

/// Whether the search's `query` (lowercase) is in `text`.
pub(crate) fn finds(text: &str, query: &str) -> bool {
    !query.is_empty() && text.to_lowercase().contains(query)
}

/// The spotlight's outline around an option, `SpotlightPainter`'s: 8 px out sideways, 6 px up
/// and down, rounded; and how much it dims the rest.
const SPOTLIGHT_X: f32 = 8.;
const SPOTLIGHT_Y: f32 = 6.;
const SPOTLIGHT_RADIUS: f32 = 8.;
const SPOTLIGHT_DIM: f32 = 0.6;

/// IntelliJ's text size, which Spork IDE's interface draws at 0.875 of its font size.
const UI_SCALE: f64 = 0.875;
/// IntelliJ's 13 px interface text, as Spork IDE's interface font size.
const UI_DEFAULT: f64 = 13. / UI_SCALE;
const ZOOMS: [u32; 9] = [70, 80, 90, 100, 110, 125, 150, 175, 200];

/// The font weights by the names IntelliJ's Main weight lists a font's faces by.
const WEIGHTS: [(&str, u32); 9] = [
    ("Thin", 100),
    ("ExtraLight", 200),
    ("Light", 300),
    ("Regular", 400),
    ("Medium", 500),
    ("SemiBold", 600),
    ("Bold", 700),
    ("ExtraBold", 800),
    ("Black", 900),
];

/// What picking a combo box's choice does to the draft.
type Pick = Rc<dyn Fn(&mut SettingsDialog)>;

/// Picking sets `path` to `value` (`None` removes it).
fn pick(path: &'static str, value: Option<Value>) -> Pick {
    Rc::new(move |this: &mut SettingsDialog| this.draft.set(path, value.clone()))
}

/// A row of a page as it is built: its content, whether the search found it, and its indent.
struct Line {
    content: Div,
    found: bool,
    full_width: bool,
    indent: f32,
    /// Space over it besides the rows' gap: after a group, IntelliJ's medium gap.
    top: f32,
    /// A group's two columns, each line in them drawn as itself: not outlined or dimmed whole.
    columns: bool,
}

impl Line {
    fn new(content: Div, found: bool, indent: f32, top: f32) -> Self {
        Line {
            content,
            found,
            full_width: false,
            indent,
            top,
            columns: false,
        }
    }

    /// As the page draws it: its indent and the space over it; outlined when the search found
    /// it, dimmed when the search found something else.
    fn render(self, spotlight: bool, tokens: &Tokens) -> Div {
        div()
            .when(self.full_width, |row| row.w_full())
            .ml(px(self.indent))
            .mt(px(self.top))
            .when(spotlight && !self.columns && self.found, |row| {
                row.mx(px(-SPOTLIGHT_X))
                    .my(px(-SPOTLIGHT_Y))
                    .px(px(SPOTLIGHT_X - 2.))
                    .py(px(SPOTLIGHT_Y - 2.))
                    .border_2()
                    .border_color(tokens.spotlight)
                    .rounded(px(SPOTLIGHT_RADIUS))
            })
            .when(spotlight && !self.columns && !self.found, |row| {
                row.opacity(SPOTLIGHT_DIM)
            })
            .child(self.content)
    }
}

/// Puts a group's lines from `right` on into a second column beside the ones from `start`.
fn into_columns(
    lines: &mut Vec<Line>,
    start: usize,
    right: Option<usize>,
    column: f32,
    spotlight: bool,
    tokens: &Tokens,
) {
    let Some(right) = right.filter(|right| *right >= start && *right <= lines.len()) else {
        return;
    };
    let second: Vec<Line> = lines.drain(right..).collect();
    let first: Vec<Line> = lines.drain(start..).collect();
    let indent = first
        .iter()
        .chain(&second)
        .map(|line| line.indent)
        .fold(f32::MAX, f32::min);
    let side = |lines: Vec<Line>| {
        v_flex()
            .items_start()
            .gap(px(ROW_GAP))
            .children(lines.into_iter().map(|mut line| {
                line.indent -= indent;
                line.render(spotlight, tokens)
            }))
    };
    let mut both = Line::new(
        h_flex()
            .items_start()
            .gap(px(COLUMNS_GAP))
            .child(side(first).w(px(column)))
            .child(side(second)),
        false,
        indent.min(INDENT),
        0.,
    );
    both.columns = true;
    lines.push(both);
}

impl SettingsDialog {
    pub(crate) fn options_page(
        &mut self,
        rows: &'static [Row],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let query = self.query(cx);
        let tokens = Tokens::get(cx);
        // IntelliJ's spotlight: the options the search finds outlined, the rest of the page dimmed
        let spotlight = !query.is_empty() && labels(rows).any(|label| finds(label, &query));
        let columns = label_columns(rows, window, cx);
        let mut lines: Vec<Line> = Vec::new();
        // the indent of a group's rows, and whether a collapsible group's rows are hidden
        let mut group = 0.;
        let mut hidden = false;
        // whether the last check box not indented is on: the options under it follow it
        let mut parent_on = true;
        let mut comment_offset = LABEL_OFFSET;
        let mut after_group = false;
        // where the group's lines start, and its second column's
        let mut group_start = 0;
        let mut right = None;
        let first_column = first_column(rows, window, cx);
        for (ix, row) in rows.iter().enumerate() {
            let found = match row {
                Row::Opt(opt) => opt_labels(opt).any(|label| finds(label, &query)),
                Row::Title(text)
                | Row::Collapsible(text)
                | Row::Comment(text)
                | Row::Link(text, _) => finds(text, &query),
                Row::End | Row::Right => false,
            };
            if matches!(row, Row::Title(_) | Row::Collapsible(_) | Row::End) {
                into_columns(
                    &mut lines,
                    group_start,
                    right.take(),
                    first_column,
                    spotlight,
                    tokens,
                );
            }
            let content = match row {
                Row::Right => {
                    right = Some(lines.len());
                    continue;
                }
                Row::Title(title) => {
                    group = INDENT;
                    hidden = false;
                    // the page's first title sits right under the banner
                    let top = if lines.is_empty() { -TITLE_TOP } else { 0. };
                    let mut line = Line::new(titled(title, &query, cx), found, 0., top);
                    line.full_width = true;
                    lines.push(line);
                    group_start = lines.len();
                    continue;
                }
                Row::Collapsible(title) => {
                    group = INDENT;
                    // the search opens a group around what it finds
                    let open = self.expanded.contains(title)
                        || (spotlight
                            && section_finds(rows.get(ix + 1..).unwrap_or_default(), &query));
                    hidden = !open;
                    let mut line =
                        Line::new(self.collapsible(title, open, &query, cx), found, 0., 0.);
                    line.full_width = true;
                    lines.push(line);
                    group_start = lines.len();
                    continue;
                }
                Row::End => {
                    group = 0.;
                    hidden = false;
                    after_group = true;
                    continue;
                }
                _ if hidden => continue,
                // under a check box's label, or under the control of a label and a control
                Row::Comment(text) => comment(text, comment_offset, cx),
                Row::Link(text, target) => h_flex().child(
                    spork_workbench::controls::link(SharedString::from(*text), *text, cx)
                        .on_click(cx.listener(move |this, _, _, cx| this.show(target, cx))),
                ),
                Row::Opt(opt) => {
                    let column = columns.get(ix).copied().unwrap_or_default();
                    if !opt.inline {
                        comment_offset = match in_column(row) {
                            true => column + SMALL_GAP,
                            false => LABEL_OFFSET,
                        };
                    }
                    let enabled = !opt.indent || parent_on;
                    let control = self.option(opt, enabled, column, &query, window, cx);
                    if !opt.indent
                        && let Kind::Check(on, _) = opt.kind
                    {
                        parent_on = self.checked(opt.path, on);
                    }
                    let path = crate::settings_path(&self.page, Some(opt.label));
                    // its right-click menu: Copy Settings Path, as IntelliJ's on a control;
                    // the gap again between it and an inline cell after it
                    h_flex().gap(px(SMALL_GAP)).child(
                        h_flex()
                            .id(("option", ix))
                            .gap(px(SMALL_GAP))
                            .context_menu(move |menu, _, _| {
                                crate::copy_path_menu(menu, path.clone())
                            })
                            .child(control)
                            .when(!opt.unit.is_empty(), |row| row.child(opt.unit))
                            .when(!opt.help.is_empty(), |row| {
                                row.child(spork_workbench::controls::context_help(
                                    SharedString::from(format!("help-{}", opt.path)),
                                    opt.help,
                                    cx,
                                ))
                            }),
                    )
                }
            };
            match (row, lines.last_mut()) {
                // the cell after another on IntelliJ's row
                (Row::Opt(opt), Some(last)) if opt.inline => {
                    let before = std::mem::replace(&mut last.content, div());
                    let spacer = match opt.indent {
                        true => 0.,
                        false => DEFAULT_GAP - 2. * SMALL_GAP,
                    };
                    last.content = before.child(div().w(px(spacer))).child(content);
                    last.found |= found;
                }
                (Row::Link(..), Some(last)) => {
                    let before = std::mem::replace(&mut last.content, div());
                    last.content = before
                        .child(div().w(px(DEFAULT_GAP - 2. * SMALL_GAP)))
                        .child(content);
                    last.found |= found;
                }
                _ => lines.push(Line {
                    content,
                    found,
                    // a comment wraps at the page's or its column's width
                    full_width: matches!(row, Row::Comment(_)),
                    indent: group + if is_indented(row) { INDENT } else { 0. },
                    top: match (std::mem::take(&mut after_group), row) {
                        (true, _) => MEDIUM_GAP - ROW_GAP,
                        // a comment sits right under its option
                        (false, Row::Comment(_)) => COMMENT_GAP - ROW_GAP,
                        (false, _) => 0.,
                    },
                    columns: false,
                }),
            }
        }
        into_columns(
            &mut lines,
            group_start,
            right,
            first_column,
            spotlight,
            tokens,
        );
        // rows as wide as their controls, so the spotlight outlines just them
        v_flex()
            .items_start()
            .gap(px(ROW_GAP))
            .line_height(relative(LINE_HEIGHT))
            .children(lines.into_iter().map(|line| line.render(spotlight, tokens)))
    }

    /// Whether the check box over `path` shows checked.
    fn checked(&self, path: &str, on: Lit) -> bool {
        match (self.draft.get(path), on) {
            // a font feature is kept as a number: 1 on, 0 off
            (Some(Value::Number(number)), Lit::Bool(on)) => (number.as_f64() != Some(0.)) == on,
            (value, on) => value == Some(&on.value()),
        }
    }

    /// IntelliJ's collapsible group's title: its chevron, its name and a line; a click opens or
    /// closes it.
    fn collapsible(
        &self,
        title: &'static str,
        open: bool,
        query: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let tokens = Tokens::get(cx);
        h_flex()
            .w_full()
            .pt(px(TITLE_TOP))
            .pb(px(TITLE_BOTTOM))
            .gap(px(4.))
            .child(
                h_flex()
                    .id(SharedString::from(format!("group-{title}")))
                    .role(Role::Button)
                    .aria_label(title)
                    .aria_expanded(open)
                    .gap(px(4.))
                    .cursor_pointer()
                    .child(
                        spork_workbench::icon(match open {
                            true => "general/chevronDown",
                            false => "general/chevronRight",
                        })
                        .size(px(16.))
                        .text_color(tokens.icon),
                    )
                    .child(label(title, query, cx))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.expanded.remove(title) {
                            this.expanded.insert(title);
                        }
                        cx.notify();
                    })),
            )
            .child(div().flex_1().h(px(1.)).bg(tokens.separator))
    }

    #[allow(clippy::too_many_arguments)]
    fn option(
        &mut self,
        opt: &'static Opt,
        enabled: bool,
        column: f32,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let path = opt.path;
        let name = opt.label.trim_end_matches(':');
        match opt.kind {
            Kind::Check(on, off) => {
                let checked = self.checked(path, on);
                self.check(
                    path,
                    opt.label,
                    query,
                    checked,
                    enabled,
                    cx,
                    move |this, checked| {
                        let value = if checked { on } else { off };
                        this.draft.set(path, Some(value.value()));
                    },
                )
            }
            Kind::Choice(choices) => {
                let current = self
                    .draft
                    .get(path)
                    .or_else(|| inherited(path).and_then(|from| self.draft.get(from)))
                    .cloned();
                // a number setting matches its choice whether whole or not
                let same = |value: &Value, current: &Option<Value>| match (value, current) {
                    (Value::Number(a), Some(Value::Number(b))) => a.as_f64() == b.as_f64(),
                    (value, current) => Some(value) == current.as_ref(),
                };
                // a size between the choices shows as it is
                let between = current
                    .as_ref()
                    .and_then(Value::as_f64)
                    .map(|size| format_number(size, 0))
                    .unwrap_or_default();
                let shown = choices
                    .iter()
                    .find(|(_, value)| same(&value.value(), &current))
                    .map_or(between.as_str(), |(label, _)| *label);
                let combo = self.combo(path, name, shown, opt.width, !enabled, cx, move |_| {
                    choices
                        .iter()
                        .map(|(label, value)| {
                            (
                                SharedString::from(*label),
                                same(&value.value(), &current),
                                pick(path, Some(value.value())),
                            )
                        })
                        .collect()
                });
                labeled(label(opt.label, query, cx), column, combo)
            }
            Kind::Fixed(choice) => {
                let combo = self.combo(path, name, choice, opt.width, !enabled, cx, move |_| {
                    vec![(
                        SharedString::from(choice),
                        true,
                        Rc::new(|_: &mut Self| {}) as Pick,
                    )]
                });
                combo
            }
            Kind::Radios(choices) => {
                let current = self.draft.get(path).cloned();
                let radios = choices.iter().map(|(text, value)| {
                    let on = Some(value.value()) == current;
                    spork_workbench::controls::labeled_radio(
                        format!("{path}-{text}"),
                        *text,
                        on,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.draft.set(path, Some(value.value()));
                        cx.notify();
                    }))
                });
                v_flex()
                    .gap(px(6.))
                    .child(label(opt.label, query, cx))
                    .child(v_flex().pl(px(INDENT)).gap(px(6.)).children(radios))
                    .into_any_element()
            }
            Kind::Number {
                min,
                max,
                scale,
                decimals,
                width,
            } => {
                let shown = self
                    .draft
                    .get(path)
                    .or_else(|| inherited(path).and_then(|from| self.draft.get(from)))
                    .and_then(Value::as_f64)
                    .map(|value| format_number(value * scale, decimals))
                    .unwrap_or_default();
                let input = self.input(path, shown, window, cx, move |text| {
                    let text = text.trim();
                    if text.is_empty() {
                        return Some(None);
                    }
                    let shown = text.parse::<f64>().ok()?;
                    (min..=max)
                        .contains(&shown)
                        .then(|| Some(number_value(shown / scale, decimals)))
                });
                let width = if opt.width > 0. { opt.width } else { width };
                labeled(
                    label(opt.label, query, cx),
                    column,
                    field(&input, opt.label, width),
                )
            }
            Kind::Font => {
                let current: SharedString = self
                    .draft
                    .get(path)
                    .or_else(|| inherited(path).and_then(|from| self.draft.get(from)))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
                    .into();
                let fonts = self.fonts.clone();
                let combo = self.combo(
                    path,
                    name,
                    &current.clone(),
                    opt.width,
                    false,
                    cx,
                    move |_| {
                        fonts
                            .iter()
                            .map(|font| {
                                (
                                    font.clone(),
                                    *font == current,
                                    pick(path, Some(Value::String(font.to_string()))),
                                )
                            })
                            .collect()
                    },
                );
                labeled(label(opt.label, query, cx), column, combo)
            }
            Kind::Fallback => {
                let current: Option<SharedString> = self
                    .draft
                    .get(path)
                    .and_then(|fallbacks| fallbacks.get(0))
                    .and_then(Value::as_str)
                    .map(|font| SharedString::from(font.to_owned()));
                let fonts = self.fonts.clone();
                let shown = current.clone().unwrap_or_else(|| "None".into());
                let combo = self.combo(path, name, &shown, opt.width, false, cx, move |_| {
                    let none = (
                        SharedString::from("None"),
                        current.is_none(),
                        Rc::new(move |this: &mut Self| {
                            this.draft.set(path, None);
                            // the defaults' fallback goes too
                            if this
                                .draft
                                .get(path)
                                .is_some_and(|value| value != &json!([]))
                            {
                                this.draft.set(path, Some(json!([])));
                            }
                        }) as Pick,
                    );
                    std::iter::once(none)
                        .chain(fonts.iter().map(|font| {
                            (
                                font.clone(),
                                current.as_ref() == Some(font),
                                pick(path, Some(json!([font.as_ref()]))),
                            )
                        }))
                        .collect()
                });
                labeled(label(opt.label, query, cx), column, combo)
            }
            Kind::Weight => {
                let weight = self.draft.get(path).and_then(Value::as_f64).unwrap_or(400.);
                let shown = WEIGHTS
                    .iter()
                    .min_by_key(|(_, value)| (*value as f64 - weight).abs() as u32)
                    .map_or("Regular", |(name, _)| *name);
                let combo = self.combo(path, name, shown, opt.width, false, cx, move |_| {
                    WEIGHTS
                        .iter()
                        .map(|(name, value)| {
                            (
                                SharedString::from(*name),
                                *name == shown,
                                pick(path, Some(json!(*value as f64))),
                            )
                        })
                        .collect()
                });
                labeled(label(opt.label, query, cx), column, combo)
            }
            Kind::CloseButton => {
                let hidden = self.draft.get("/tabs/show_close_button") == Some(&json!("hidden"));
                let left = self.draft.get(path) == Some(&json!("left"));
                let shown = match (hidden, left) {
                    (true, _) => "None",
                    (false, true) => "Left",
                    (false, false) => "Right",
                };
                let combo = self.combo(path, name, shown, opt.width, false, cx, move |_| {
                    let place = |side: &'static str| -> Pick {
                        Rc::new(move |this: &mut Self| {
                            this.draft.set(path, Some(json!(side)));
                            if this.draft.get("/tabs/show_close_button") == Some(&json!("hidden")) {
                                this.draft
                                    .set("/tabs/show_close_button", Some(json!("hover")));
                            }
                        })
                    };
                    vec![
                        ("Left".into(), shown == "Left", place("left")),
                        ("Right".into(), shown == "Right", place("right")),
                        (
                            "None".into(),
                            shown == "None",
                            pick("/tabs/show_close_button", Some(json!("hidden"))),
                        ),
                    ]
                });
                labeled(label(opt.label, query, cx), column, combo)
            }
            Kind::Theme { sync } => self.theme_row(opt, sync, query, cx),
            Kind::Zoom => {
                let size = self
                    .draft
                    .get(path)
                    .and_then(Value::as_f64)
                    .unwrap_or(UI_DEFAULT);
                let zoom = (size / UI_DEFAULT * 100.).round() as u32;
                let combo = self.combo(
                    path,
                    name,
                    &format!("{zoom}%"),
                    ZOOM_WIDTH,
                    false,
                    cx,
                    move |_| {
                        ZOOMS
                            .iter()
                            .map(|choice| {
                                (
                                    SharedString::from(format!("{choice}%")),
                                    *choice == zoom,
                                    pick(
                                        path,
                                        Some(number_value(UI_DEFAULT * *choice as f64 / 100., 3)),
                                    ),
                                )
                            })
                            .collect()
                    },
                );
                h_flex()
                    .gap(px(SMALL_GAP))
                    .child(label(opt.label, query, cx))
                    .child(combo)
                    .child(
                        div()
                            .text_color(Tokens::get(cx).text_secondary)
                            .child("Change with ⌃⌥= or ⌃⌥-. Set to 100% with ⌃⌥0"),
                    )
                    .into_any_element()
            }
            Kind::CustomFont => self.custom_font_row(opt, query, window, cx),
            Kind::LineHeight => {
                let shown = match self.draft.get(path) {
                    Some(Value::String(name)) if name == "comfortable" => "1.618".to_owned(),
                    Some(Value::String(name)) if name == "standard" => "1.3".to_owned(),
                    Some(value) => value
                        .pointer("/custom")
                        .and_then(Value::as_f64)
                        .map(|height| format_number(height, 1))
                        .unwrap_or_default(),
                    None => "1.0".to_owned(),
                };
                let input = self.input(path, shown, window, cx, |text| {
                    let height = text.trim().parse::<f64>().ok()?;
                    (0.5..=4.0)
                        .contains(&height)
                        .then(|| Some(json!({ "custom": height })))
                });
                labeled(
                    label(opt.label, query, cx),
                    column,
                    field(&input, opt.label, 44.),
                )
            }
            Kind::LineNumbers => {
                let on = self.draft.get(path) != Some(&Value::Bool(false));
                let relative = self.draft.get("/relative_line_numbers").cloned();
                let hybrid = relative.as_ref().is_some_and(|value| value != "disabled");
                let check = self.check(
                    path,
                    opt.label,
                    query,
                    on,
                    true,
                    cx,
                    move |this, checked| {
                        this.draft.set(path, Some(Value::Bool(checked)));
                    },
                );
                let combo = self.combo(
                    "/relative_line_numbers",
                    name,
                    if hybrid { "Hybrid" } else { "Absolute" },
                    LINE_NUMBERS_WIDTH,
                    !on,
                    cx,
                    move |_| {
                        vec![
                            (
                                "Absolute".into(),
                                !hybrid,
                                pick("/relative_line_numbers", Some(json!("disabled"))),
                            ),
                            (
                                "Hybrid".into(),
                                hybrid,
                                pick("/relative_line_numbers", Some(json!("enabled"))),
                            ),
                        ]
                    },
                );
                h_flex()
                    .gap(px(SMALL_GAP))
                    .child(check)
                    .child(combo)
                    .into_any_element()
            }
            Kind::Guides => {
                let shown = self
                    .draft
                    .get(path)
                    .and_then(Value::as_array)
                    .map(|columns| {
                        columns
                            .iter()
                            .filter_map(Value::as_u64)
                            .map(|column| column.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                let input = self.input(path, shown, window, cx, |text| {
                    let columns: Option<Vec<u64>> = text
                        .split(',')
                        .map(str::trim)
                        .filter(|column| !column.is_empty())
                        .map(|column| column.parse().ok())
                        .collect();
                    Some(Some(json!(columns?)))
                });
                let width = if opt.width > 0. { opt.width } else { 160. };
                labeled(
                    label(opt.label, query, cx),
                    column,
                    field(&input, opt.label, width),
                )
            }
            Kind::Autosave { idle } => self.autosave_row(opt, idle, query, window, cx),
            Kind::Wrapped { key, empty, width } => {
                let shown = self
                    .draft
                    .get(path)
                    .and_then(|value| value.pointer(&format!("/{key}")))
                    .map(|inner| match inner {
                        Value::String(text) => text.clone(),
                        // {"always": {"directory": "…"}}
                        other => other
                            .pointer("/directory")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    })
                    .unwrap_or_default();
                let input = self.input(path, shown, window, cx, move |text| {
                    let text = text.trim();
                    Some(match text.is_empty() {
                        true => empty.map(Lit::value),
                        false if key == "always" => Some(json!({ key: { "directory": text } })),
                        false => Some(json!({ key: text })),
                    })
                });
                labeled(
                    label(opt.label, query, cx),
                    column,
                    field(&input, opt.label, width),
                )
            }
            Kind::Intentions => self.intentions(path, cx),
            Kind::Env => {
                let shown = self
                    .draft
                    .get(path)
                    .and_then(Value::as_object)
                    .map(|vars| {
                        vars.iter()
                            .map(|(name, value)| format!("{name}={}", value.as_str().unwrap_or("")))
                            .collect::<Vec<_>>()
                            .join(";")
                    })
                    .unwrap_or_default();
                let input = self.input(path, shown, window, cx, |text| {
                    let vars: serde_json::Map<String, Value> = text
                        .split(';')
                        .filter_map(|pair| pair.split_once('='))
                        .map(|(name, value)| (name.trim().to_owned(), json!(value)))
                        .filter(|(name, _)| !name.is_empty())
                        .collect();
                    Some((!vars.is_empty()).then_some(Value::Object(vars)))
                });
                labeled(
                    label(opt.label, query, cx),
                    column,
                    field(&input, opt.label, 420.),
                )
            }
        }
    }

    /// Theme: the themes by name; with Sync with OS, the light or dark one the OS's appearance
    /// picks, as IntelliJ's.
    fn theme_row(
        &mut self,
        opt: &'static Opt,
        sync: bool,
        query: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selection = self.draft.get("/theme").cloned();
        let synced = selection
            .as_ref()
            .and_then(|value| value.pointer("/mode"))
            .is_some_and(|mode| mode == "system");
        let dark_os = matches!(
            theme::SystemAppearance::global(cx).0,
            theme::Appearance::Dark
        );
        let current = theme_name(selection.as_ref(), dark_os);
        let mut themes = theme::ThemeRegistry::global(cx).list();
        themes.sort_by(|a, b| a.name.cmp(&b.name));
        let choices: Vec<(SharedString, bool)> = themes
            .iter()
            .map(|meta| (meta.name.clone(), meta.appearance.is_light()))
            .collect();
        let light = choices
            .iter()
            .any(|(name, light)| *light && *name == current);
        let combo = {
            let current = current.clone();
            self.combo(
                "/theme",
                opt.label.trim_end_matches(':'),
                &current.clone(),
                THEME_WIDTH,
                false,
                cx,
                move |_| {
                    choices
                        .iter()
                        .map(|(name, light)| {
                            let value = match &selection {
                                Some(selection) if synced => {
                                    let mut selection = selection.clone();
                                    let slot = if *light { "light" } else { "dark" };
                                    selection[slot] = json!(name.as_ref());
                                    selection
                                }
                                _ => json!(name.as_ref()),
                            };
                            (name.clone(), *name == current, pick("/theme", Some(value)))
                        })
                        .collect()
                },
            )
        };
        let row = h_flex()
            .gap(px(SMALL_GAP))
            .child(label(opt.label, query, cx))
            .child(combo);
        if !sync {
            return row.into_any_element();
        }
        let sync_check = self.check(
            "/theme-sync",
            "Sync with OS",
            query,
            synced,
            true,
            cx,
            move |this, checked| {
                let value = match checked {
                    true => {
                        json!({
                            "mode": "system",
                            "light": if light { current.as_ref() } else { DEFAULT_LIGHT },
                            "dark": if light { DEFAULT_DARK } else { current.as_ref() },
                        })
                    }
                    false => json!(current.as_ref()),
                };
                this.draft.set("/theme", Some(value));
            },
        );
        // IntelliJ's gear for the themes Sync with OS picks (`PreferredLafAndSchemeAction`)
        let this = cx.entity();
        let gear = spork_workbench::ActionButton::new("theme-preferred", "general/settings")
            .tooltip("Preferred Theme")
            .popup_marker()
            .disabled(!synced)
            .dropdown_menu(move |menu, _, cx| {
                let selection = this.read(cx).draft.get("/theme").cloned();
                let mut themes = theme::ThemeRegistry::global(cx).list();
                themes.sort_by(|a, b| a.name.cmp(&b.name));
                [
                    ("For Light OS", "light", true),
                    ("For Dark OS", "dark", false),
                ]
                .into_iter()
                .enumerate()
                .fold(menu, |menu, (ix, (header, slot, light))| {
                    let menu = match ix {
                        0 => menu.label(header),
                        _ => menu.separator().label(header),
                    };
                    let current = selection
                        .as_ref()
                        .and_then(|theme| theme.pointer(&format!("/{slot}")))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    themes
                        .iter()
                        .filter(|theme| theme.appearance.is_light() == light)
                        .fold(menu, |menu, theme| {
                            let (this, name) = (this.clone(), theme.name.clone());
                            menu.item(
                                PopupMenuItem::new(name.clone())
                                    .checked(name.as_ref() == current)
                                    .on_click(move |_, _, cx| {
                                        this.update(cx, |this, cx| {
                                            if let Some(mut theme) =
                                                this.draft.get("/theme").cloned()
                                            {
                                                theme[slot] = json!(name.as_ref());
                                                this.draft.set("/theme", Some(theme));
                                            }
                                            cx.notify();
                                        })
                                    }),
                            )
                        })
                })
            });
        row.child(div().w(px(DEFAULT_GAP - 2. * SMALL_GAP)))
            .child(sync_check)
            .child(gear)
            .into_any_element()
    }

    /// Use custom font: the interface's font and its size, greyed until checked.
    fn custom_font_row(
        &mut self,
        opt: &'static Opt,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let custom = self
            .draft
            .edited
            .pointer(opt.path)
            .is_some_and(|v| !v.is_null());
        let family: SharedString = match self.draft.get(opt.path).and_then(Value::as_str) {
            Some(".SystemUIFont") | None => "System font".into(),
            Some(family) => family.to_owned().into(),
        };
        let check = self.check(
            opt.path,
            opt.label,
            query,
            custom,
            true,
            cx,
            move |this, checked| {
                // IntelliJ's interface font, bundled with the workbench
                let family = checked.then(|| json!("Inter"));
                this.draft.set("/ui_font_family", family);
            },
        );
        let fonts = self.fonts.clone();
        let current = family.clone();
        let path = opt.path;
        let combo = self.combo(
            path,
            "Font",
            &family,
            CUSTOM_FONT_WIDTH,
            !custom,
            cx,
            move |_| {
                fonts
                    .iter()
                    .map(|font| {
                        (
                            font.clone(),
                            *font == current,
                            pick(path, Some(Value::String(font.to_string()))),
                        )
                    })
                    .collect()
            },
        );
        let size = self
            .draft
            .get("/ui_font_size")
            .and_then(Value::as_f64)
            .unwrap_or(UI_DEFAULT);
        let input = self.input(
            "/ui_font_size",
            format_number(size * UI_SCALE, 0),
            window,
            cx,
            |text| {
                let size = text.trim().parse::<f64>().ok()?;
                (6.0..=48.0)
                    .contains(&size)
                    .then(|| Some(number_value(size / UI_SCALE, 3)))
            },
        );
        let tokens = Tokens::get(cx);
        h_flex()
            .gap(px(SMALL_GAP))
            .child(check)
            .child(combo)
            .child(div().w(px(DEFAULT_GAP - 2. * SMALL_GAP)))
            .child(
                div()
                    .when(!custom, |size| size.text_color(tokens.text_disabled))
                    .child(label("Size:", query, cx)),
            )
            // IntelliJ's editable combo box is 85 px
            .child(field(&input, "Size:", 85.).when(!custom, |field| field.opacity(0.5)))
            .into_any_element()
    }

    /// Autosave: idle with its seconds, or on leaving; Spork IDE saves one way at a time.
    fn autosave_row(
        &mut self,
        opt: &'static Opt,
        idle: bool,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let path = opt.path;
        let current = self.draft.get(path).cloned();
        let delay = current
            .as_ref()
            .and_then(|value| value.pointer("/after_delay/milliseconds"))
            .and_then(Value::as_u64);
        if !idle {
            let on = current
                .as_ref()
                .is_some_and(|value| value == "on_focus_change" || value == "on_window_change");
            return self.check(path, opt.label, query, on, true, cx, |this, checked| {
                let value = if checked { "on_focus_change" } else { "off" };
                this.draft.set("/autosave", Some(json!(value)));
            });
        }
        let check = self.check(
            "/autosave-idle",
            opt.label,
            query,
            delay.is_some(),
            true,
            cx,
            |this, checked| {
                let value = match checked {
                    true => json!({ "after_delay": { "milliseconds": 15000 } }),
                    false => json!("off"),
                };
                this.draft.set("/autosave", Some(value));
            },
        );
        let seconds = delay.map_or(15, |ms| ms / 1000).to_string();
        let input = self.input("/autosave-seconds", seconds, window, cx, |text| {
            let seconds = text.trim().parse::<u64>().ok().filter(|s| *s > 0)?;
            Some(Some(
                json!({ "after_delay": { "milliseconds": seconds * 1000 } }),
            ))
        });
        h_flex()
            .gap(px(SMALL_GAP))
            .child(check)
            .child(field(&input, opt.label, 56.).when(delay.is_none(), |f| f.opacity(0.5)))
            .into_any_element()
    }

    /// A check box and its label; a click flips it with `set`, greyed and still while not
    /// `enabled`.
    #[allow(clippy::too_many_arguments)]
    fn check(
        &self,
        id: &'static str,
        text: &'static str,
        query: &str,
        checked: bool,
        enabled: bool,
        cx: &mut Context<Self>,
        set: impl Fn(&mut Self, bool) + 'static,
    ) -> AnyElement {
        let tokens = Tokens::get(cx);
        let check = h_flex()
            .min_h(px(CHECK_ROW))
            .id(SharedString::from(format!("check-{id}-{text}")))
            .role(Role::CheckBox)
            .aria_label(SharedString::from(text))
            .aria_toggled(checked.into())
            .gap(px(4.))
            .child(spork_workbench::controls::check_box(
                SharedString::from(format!("box-{id}-{text}")),
                Some(checked),
                cx,
            ))
            .child(label(text, query, cx));
        match enabled {
            true => check
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    set(this, !checked);
                    this.sync_inputs(cx);
                    cx.notify();
                }))
                .into_any_element(),
            false => spork_workbench::controls::A11yDisabled::new(
                check.opacity(0.5).text_color(tokens.text_disabled),
                true,
            )
            .into_any_element(),
        }
    }

    /// The intentions turned off, as saved and as edited: unchecking one turns it off, checking
    /// it turns it back on.
    fn intentions(&self, path: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let titles = |json: Option<&Value>| -> Vec<String> {
            json.and_then(Value::as_array)
                .map(|titles| {
                    titles
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        let off = titles(self.draft.get(path));
        let mut shown = titles(self.draft.saved.pointer(path));
        shown.extend(
            off.iter()
                .filter(|title| !shown.contains(title))
                .cloned()
                .collect::<Vec<_>>(),
        );
        if shown.is_empty() {
            return comment("Nothing to show", 0., cx).into_any_element();
        }
        v_flex()
            .gap(px(6.))
            .children(shown.into_iter().map(|title| {
                let checked = !off.contains(&title);
                let label = SharedString::from(title.clone());
                h_flex()
                    .id(SharedString::from(format!("intention-{title}")))
                    .role(Role::CheckBox)
                    .aria_label(label.clone())
                    .aria_toggled(checked.into())
                    .gap(px(4.))
                    .cursor_pointer()
                    .child(spork_workbench::controls::check_box(
                        SharedString::from(format!("intention-box-{title}")),
                        Some(checked),
                        cx,
                    ))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mut off = titles(this.draft.get(path));
                        match checked {
                            true => off.push(title.clone()),
                            false => off.retain(|other| other != &title),
                        }
                        this.draft.set(path, Some(json!(off)));
                        cx.notify();
                    }))
            }))
            .into_any_element()
    }

    /// A combo box of `choices`, named `name`: their labels, which is current, and what picking
    /// one does.
    #[allow(clippy::too_many_arguments)]
    fn combo(
        &self,
        path: &'static str,
        name: &'static str,
        current: &str,
        width: f32,
        disabled: bool,
        cx: &mut Context<Self>,
        choices: impl Fn(&App) -> Vec<(SharedString, bool, Pick)> + 'static,
    ) -> AnyElement {
        let this = cx.entity();
        spork_workbench::controls::labeled_combo(
            SharedString::from(format!("combo-{path}")),
            name,
            SharedString::from(current.to_owned()),
            width,
            disabled,
            move |menu, _, cx| {
                choices(cx)
                    .into_iter()
                    .fold(menu, |menu, (label, checked, picked)| {
                        let this = this.clone();
                        menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                            move |_, _, cx| {
                                this.update(cx, |this, cx| {
                                    picked(this);
                                    this.sync_inputs(cx);
                                    cx.notify();
                                })
                            },
                        ))
                    })
            },
        )
    }

    /// The text field of an option, made the first time it shows with `text`; an edit `parse`s
    /// to the value to set (`Some(None)` removes it), or to `None` while not a valid one.
    fn input(
        &mut self,
        path: &'static str,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
        parse: impl Fn(&str) -> Option<Option<Value>> + 'static,
    ) -> Entity<InputState> {
        if let Some(input) = self.inputs.get(path) {
            return input.clone();
        }
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(text, window, cx);
            state
        });
        let setting = match path {
            "/autosave-seconds" => "/autosave",
            path => path,
        };
        self._subscriptions.push(cx.subscribe(
            &input,
            move |this, input, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let text = input.read(cx).value().to_string();
                if let Some(value) = parse(&text) {
                    this.draft.set(setting, value);
                    cx.notify();
                }
            },
        ));
        self.inputs.insert(path, input.clone());
        input
    }

    /// Fields show a setting another control changed (Zoom and Size are one setting): they're
    /// made again with its value.
    fn sync_inputs(&mut self, _: &mut Context<Self>) {
        self.inputs.clear();
    }
}

impl SettingsDialog {
    /// Editor › Font's preview (IntelliJ's `FontEditorPreview`): its text in the font, size,
    /// line height and ligatures as edited.
    pub(crate) fn font_preview(&self, cx: &App) -> AnyElement {
        let tokens = Tokens::get(cx);
        let family = self
            .draft
            .get("/buffer_font_family")
            .and_then(Value::as_str)
            .unwrap_or("JetBrains Mono")
            .to_owned();
        let size = self
            .draft
            .get("/buffer_font_size")
            .and_then(Value::as_f64)
            .unwrap_or(13.) as f32;
        let height = match self.draft.get("/buffer_line_height") {
            Some(Value::String(name)) if name == "comfortable" => 1.618,
            Some(Value::String(name)) if name == "standard" => 1.3,
            Some(value) => value
                .pointer("/custom")
                .and_then(Value::as_f64)
                .unwrap_or(1.2) as f32,
            None => 1.2,
        };
        let ligatures = self.checked("/buffer_font_features/calt", Lit::Bool(true));
        let font = Font {
            family: family.into(),
            features: FontFeatures(std::sync::Arc::new(vec![(
                "calt".into(),
                u32::from(ligatures),
            )])),
            ..font("")
        };
        let block = |lines: &'static str, bold: bool| {
            v_flex().children(lines.lines().map(move |line| {
                div()
                    .when(bold, |line| line.font_weight(FontWeight::BOLD))
                    .child(line)
            }))
        };
        v_flex()
            .id("font-preview")
            .flex_1()
            .min_w_0()
            .h_full()
            .border_l_1()
            .border_color(tokens.separator)
            .child(
                v_flex()
                    .id("font-preview-text")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(12.))
                    .gap(px(size * height))
                    .font(font)
                    .text_size(px(size))
                    .line_height(relative(height))
                    .child(block(PREVIEW_TEXT, false))
                    .child(block(PREVIEW_DEFAULT, false))
                    .child(block(PREVIEW_DEFAULT, true))
                    .child(block(PREVIEW_LIGATURES, false)),
            )
            .child(
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .text_color(tokens.text_secondary)
                    .child("Enter any text to preview"),
            )
            .into_any_element()
    }
}

/// `FontEditorPreview`'s text, with the IDE's name.
const PREVIEW_TEXT: &str = "Spork IDE is an Integrated\nDevelopment Environment (IDE) designed\nto maximize productivity. It provides\nclever code completion, static code\nanalysis, and refactorings, and lets\nyou focus on the bright side of\nsoftware development making\nit an enjoyable experience.";
const PREVIEW_DEFAULT: &str = "abcdefghijklmnopqrstuvwxyz\nABCDEFGHIJKLMNOPQRSTUVWXYZ\n0123456789 (){}[]\n+ - * / = .,;:!? #&$%@|^";
const PREVIEW_LIGATURES: &str = "<!-- -- != := === >= >- >=> |-> -> <$>\n</> #[ |||> |= ~@";

/// Whether the search finds an option of the group starting at `rows`.
fn section_finds(rows: &'static [Row], query: &str) -> bool {
    rows.iter()
        .take_while(|row| !matches!(row, Row::Title(_) | Row::Collapsible(_) | Row::End))
        .filter(|row| !matches!(row, Row::Right))
        .any(|row| match row {
            Row::Opt(opt) => opt_labels(opt).any(|label| finds(label, query)),
            Row::Comment(text) | Row::Link(text, _) => finds(text, query),
            _ => false,
        })
}

/// The setting one takes while unset: the terminal's font is the editor's, the Markdown
/// preview's size the interface's.
fn inherited(path: &str) -> Option<&'static str> {
    match path {
        "/terminal/font_family" => Some("/buffer_font_family"),
        "/terminal/font_size" => Some("/buffer_font_size"),
        "/markdown_preview/font_size" => Some("/ui_font_size"),
        _ => None,
    }
}

fn is_indented(row: &Row) -> bool {
    matches!(row, Row::Opt(opt) if opt.indent && !opt.inline)
}

const DEFAULT_LIGHT: &str = settings::DEFAULT_LIGHT_THEME;
const DEFAULT_DARK: &str = settings::DEFAULT_DARK_THEME;

/// The theme a selection shows: its name, or the light or dark one the OS's appearance picks.
fn theme_name(selection: Option<&Value>, dark_os: bool) -> SharedString {
    match selection {
        Some(Value::String(name)) => name.clone().into(),
        Some(selection) => {
            let slot = match selection.pointer("/mode").and_then(Value::as_str) {
                Some("light") => "light",
                Some("dark") => "dark",
                _ if dark_os => "dark",
                _ => "light",
            };
            selection
                .pointer(&format!("/{slot}"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
                .into()
        }
        None => DEFAULT_DARK.into(),
    }
}

/// A number as the setting keeps it: whole when it has no `decimals`.
fn number_value(value: f64, decimals: usize) -> Value {
    match decimals {
        0 => json!(value.round() as i64),
        decimals => {
            let factor = 10f64.powi(decimals as i32);
            json!((value * factor).round() / factor)
        }
    }
}

fn format_number(value: f64, decimals: usize) -> String {
    format!("{value:.decimals$}")
}

/// IntelliJ's indent of a group's rows and of a dependent option (`horizontalIndent`).
const INDENT: f32 = 24.;
/// The gap between a label and its control, and between cells of a row (IntelliJ's
/// `horizontalSmallGap` and `horizontalDefaultGap`).
const SMALL_GAP: f32 = 6.;
const DEFAULT_GAP: f32 = 16.;
/// The gap between rows, and before and after a group (IntelliJ's `verticalMediumGap`).
const ROW_GAP: f32 = 9.;
const MEDIUM_GAP: f32 = 20.;
/// The gap between an option and its comment under it, and the room a comment keeps under it.
const COMMENT_GAP: f32 = 5.;
const COMMENT_BOTTOM: f32 = 3.;
/// A check box's row (`JBCheckBox`, 2 px over and under its box), and the room over a group's
/// title and under it: measured on IntelliJ IDEA 2026.3's Appearance page (a check box row's
/// middle 29 px under the one before, a title's 40 px under the row before and 28.5 px over the
/// row after).
const CHECK_ROW: f32 = 20.;
const TITLE_TOP: f32 = 23.;
const TITLE_BOTTOM: f32 = 1.5;
/// IntelliJ's text lines: 16 px for its 13 px text, 15 px for a comment's 12 px.
const LINE_HEIGHT: f32 = 1.25;
/// A check box's label starts this far in: a comment under it lines up with it.
const LABEL_OFFSET: f32 = 20.;
/// IntelliJ's Appearance page's combo boxes, as IntelliJ IDEA 2026.3 draws them: Theme, Zoom,
/// Use custom font.
const THEME_WIDTH: f32 = 190.;
const ZOOM_WIDTH: f32 = 78.;
const CUSTOM_FONT_WIDTH: f32 = 266.;
/// Show line numbers' combo box, as wide as "Absolute".
const LINE_NUMBERS_WIDTH: f32 = 102.;

/// An option's label, the search's matches highlighted as IntelliJ's.
pub(crate) fn label(text: &'static str, query: &str, cx: &App) -> AnyElement {
    let lower = text.to_lowercase();
    let mut highlights = Vec::new();
    if !query.is_empty() {
        let mut start = 0;
        while let Some(found) = lower[start..].find(query) {
            let at = start + found;
            highlights.push((
                at..at + query.len(),
                HighlightStyle {
                    background_color: Some(Tokens::get(cx).search_match),
                    ..Default::default()
                },
            ));
            start = at + query.len();
        }
    }
    match highlights.is_empty() {
        true => div().child(text).into_any_element(),
        false => div()
            .child(StyledText::new(text).with_highlights(highlights))
            .into_any_element(),
    }
}

/// A label and its control, the label `column` wide at least so that the controls of the rows
/// around it line up, as IntelliJ's rows of a panel.
fn labeled(label: AnyElement, column: f32, control: impl IntoElement) -> AnyElement {
    h_flex()
        .gap(px(SMALL_GAP))
        .child(div().min_w(px(column)).child(label))
        .child(control)
        .into_any_element()
}

/// Whether the option is a label before its control, in IntelliJ's column of labels.
fn in_column(row: &Row) -> bool {
    matches!(
        row,
        Row::Opt(Opt {
            inline: false,
            kind: Kind::Choice(_)
                | Kind::Number { .. }
                | Kind::Font
                | Kind::Fallback
                | Kind::Weight
                | Kind::CloseButton
                | Kind::LineHeight
                | Kind::Guides
                | Kind::Wrapped { .. }
                | Kind::Env,
            ..
        })
    )
}

/// Each row's label column: rows of a label and its control next to each other share the width
/// of their widest label (a comment between them doesn't part them).
fn label_columns(rows: &'static [Row], window: &Window, cx: &App) -> Vec<f32> {
    let mut columns = vec![0.; rows.len()];
    let mut run: Vec<usize> = Vec::new();
    let close = |run: &mut Vec<usize>, columns: &mut Vec<f32>| {
        let widest = run
            .iter()
            .filter_map(|ix| match &rows[*ix] {
                Row::Opt(opt) => Some(text_width(opt.label, window, cx)),
                _ => None,
            })
            .fold(0., f32::max);
        for ix in run.drain(..) {
            if let Some(column) = columns.get_mut(ix) {
                *column = widest;
            }
        }
    };
    for (ix, row) in rows.iter().enumerate() {
        match row {
            row if in_column(row) => run.push(ix),
            Row::Comment(_) | Row::Opt(Opt { inline: true, .. }) => {}
            _ => close(&mut run, &mut columns),
        }
    }
    close(&mut run, &mut columns);
    columns
}

/// The first column of a page's two-column groups: as wide as its widest option, check box
/// included, so that every second column starts at the same place.
fn first_column(rows: &'static [Row], window: &Window, cx: &App) -> f32 {
    let mut widest: f32 = 0.;
    let mut first = false;
    for (ix, row) in rows.iter().enumerate() {
        match row {
            Row::Title(_) | Row::Collapsible(_) | Row::End => {
                // a group has two columns when a `Right` follows before its end
                first = rows
                    .get(ix + 1..)
                    .unwrap_or_default()
                    .iter()
                    .take_while(|row| {
                        !matches!(row, Row::Title(_) | Row::Collapsible(_) | Row::End)
                    })
                    .any(|row| matches!(row, Row::Right));
            }
            Row::Right => first = false,
            Row::Opt(opt) if first => {
                let check = matches!(opt.kind, Kind::Check(..));
                let width = text_width(opt.label, window, cx)
                    + if check { LABEL_OFFSET } else { 0. }
                    + if opt.indent { INDENT } else { 0. };
                widest = widest.max(width);
            }
            _ => {}
        }
    }
    widest
}

/// Between the two columns of a group (the Kotlin UI DSL's columns gap).
const COLUMNS_GAP: f32 = 60.;

/// How wide `text` draws in the dialog's 13 px interface text.
fn text_width(text: &'static str, window: &Window, cx: &App) -> f32 {
    let family = gpui_kit::component::ActiveTheme::theme(cx)
        .font_family
        .clone();
    let run = TextRun {
        len: text.len(),
        font: font(family),
        color: Hsla::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let size = rems(UI_SCALE as f32).to_pixels(window.rem_size());
    let line = window
        .text_system()
        .shape_line(SharedString::from(text), size, &[run], None);
    f32::from(line.width).ceil()
}

fn field(input: &Entity<InputState>, label: &'static str, width: f32) -> Div {
    div()
        .w(px(width))
        .child(spork_workbench::TextField::new(input).aria_label(label.trim_end_matches(':')))
}

/// IntelliJ's titled separator: the group's name, then a line.
fn titled(title: &'static str, query: &str, cx: &App) -> Div {
    h_flex()
        .w_full()
        .pt(px(TITLE_TOP))
        .pb(px(TITLE_BOTTOM))
        .gap(px(8.))
        .child(label(title, query, cx))
        .child(div().flex_1().h(px(1.)).bg(Tokens::get(cx).separator))
}

/// A comment under an option: small, grey, `offset` in, where the option's label or control
/// starts.
fn comment(text: &'static str, offset: f32, cx: &App) -> Div {
    div()
        .pl(px(offset))
        .pb(px(COMMENT_BOTTOM))
        .text_ui_small()
        .text_color(Tokens::get(cx).text_secondary)
        .child(text)
}

/// A value each option's control can write, for the tests: every option is a real setting.
#[cfg(test)]
pub(crate) fn samples(rows: &'static [Row]) -> Vec<(&'static str, Value)> {
    let mut samples = Vec::new();
    for row in rows {
        let Row::Opt(opt) = row else { continue };
        let value = match opt.kind {
            Kind::Check(on, _) => on.value(),
            Kind::Choice(choices) | Kind::Radios(choices) => choices[0].1.value(),
            Kind::Number { min, decimals, .. } => number_value(min, decimals),
            Kind::Font => json!("Menlo"),
            Kind::Fallback => json!(["Menlo"]),
            Kind::Weight => json!(600.0),
            Kind::Theme { .. } => json!("Darcula"),
            Kind::Zoom => json!(16.25),
            Kind::CustomFont => {
                samples.push(("/ui_font_size", json!(16.25)));
                json!("Menlo")
            }
            Kind::LineHeight => json!({ "custom": 1.5 }),
            Kind::LineNumbers => {
                samples.push(("/relative_line_numbers", json!("enabled")));
                json!(false)
            }
            Kind::Guides => json!([80, 120]),
            Kind::Autosave { idle: true } => json!({ "after_delay": { "milliseconds": 15000 } }),
            Kind::Autosave { idle: false } => json!("on_focus_change"),
            Kind::Wrapped { key: "always", .. } => json!({ "always": { "directory": "/tmp" } }),
            Kind::Wrapped { key, .. } => json!({ key: "zsh" }),
            Kind::Env => json!({ "A": "1" }),
            Kind::Intentions => json!(["Rewrite as raw string"]),
            Kind::CloseButton => {
                samples.push(("/tabs/show_close_button", json!("hidden")));
                json!("left")
            }
            Kind::Fixed(_) => continue,
        };
        samples.push((opt.path, value));
    }
    samples
}
