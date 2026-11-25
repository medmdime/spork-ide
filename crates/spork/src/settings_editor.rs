//! Settings ▸ Editor: General and its pages, Code Editing, Color Scheme and its pages, as the reference app
//! shows them (IntelliJ's `EditorOptionsPanel` children, `EditorCodeEditingConfigurable` and
//! `ColorAndFontOptions`). Labels and order are the reference app's (its bundles and searchable options),
//! defaults IntelliJ's (`EditorSettingsExternalizable`, `CodeInsightSettings`, `UISettingsState`).

use std::cell::RefCell;
use std::collections::HashMap;

use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_workbench::Tokens;
use spork_workbench::tokens::metrics;

use crate::notices::Notification;
use crate::settings_window::{Node, SettingsWindow, labeled, titled};

/// A group's indent and the first of two columns, as on the Appearance page.
const INDENT: f32 = 20.;
const COLUMN: f32 = 290.;

/// The pages under Editor ▸ General: Auto Import (weight 1) first, then by name, as IntelliJ's
/// `Weighted.COMPARATOR` sorts `preferences.editor`'s children.
pub(crate) const GENERAL: &[Node] = &[
    Node::page("Auto Import"),
    Node::page("Appearance").words("caret line numbers whitespaces indent guides"),
    Node::page("Breadcrumbs"),
    Node::group(
        "Code Completion",
        &[Node::page("Popup"), Node::page("Inline")],
    ),
    Node::page("Code Folding"),
    Node::page("Console"),
    Node::page("Editor Tabs").words("tab placement limit"),
    Node::page("Gutter Icons"),
    Node::page("Postfix Completion"),
    Node::group("Smart Keys", &[Node::page("HTML/CSS")]),
    Node::page("Sticky Lines"),
];

/// The pages under Editor ▸ Color Scheme, in `ColorAndFontOptions`' order: by display priority
/// (general, font, common, language, other), then by name.
pub(crate) const COLOR_SCHEME: &[Node] = &[
    Node::page("General"),
    Node::page("Language Defaults"),
    Node::page("Color Scheme Font"),
    Node::page("Console Font"),
    Node::page("Code Review"),
    Node::page("Console Colors"),
    Node::page("Debugger"),
    Node::page("Diff & Merge"),
    Node::page("User-Defined File Types"),
    Node::page("VCS"),
    Node::page("HTML"),
    Node::page("RegExp"),
    Node::page("XML"),
    Node::page("By Scope"),
    Node::page("Images"),
];

/// The reference app's bundled editor color schemes, as its scheme combo lists them, and the dark ones.
pub(crate) const SCHEMES: &[&str] = &[
    "Classic Light",
    "Darcula",
    "Darcula Contrast",
    "Dark",
    "High contrast",
    "IntelliJ Light",
    "Islands Dark",
    "Light",
];
const DARK_SCHEMES: &[&str] = &[
    "Darcula",
    "Darcula Contrast",
    "Dark",
    "High contrast",
    "Islands Dark",
];

/// The scheme setting, shared with Appearance's "Editor color scheme" combo as in IntelliJ.
const SCHEME_KEY: &str = "appearance.editorScheme";

/// The color pages: their name, their attributes and their preview text.
const COLOR_PAGES: &[(&str, &str, &str)] = &[
    ("General", GENERAL_COLORS, GENERAL_COLORS_PREVIEW),
    (
        "Language Defaults",
        LANGUAGE_DEFAULTS,
        LANGUAGE_DEFAULTS_PREVIEW,
    ),
    ("Code Review", CODE_REVIEW, ""),
    ("Console Colors", CONSOLE_COLORS, CONSOLE_COLORS_PREVIEW),
    ("Debugger", DEBUGGER, ""),
    ("Diff & Merge", DIFF_MERGE, ""),
    (
        "User-Defined File Types",
        USER_DEFINED,
        USER_DEFINED_PREVIEW,
    ),
    ("VCS", VCS, ""),
    ("HTML", HTML, HTML_PREVIEW),
    ("RegExp", REGEXP, REGEXP_PREVIEW),
    ("XML", XML, XML_PREVIEW),
    ("By Scope", "", ""),
    ("Images", IMAGES, ""),
];

/// IntelliJ's effect types by their number in scheme files (`AttributesFlyweightSerDe`), and the
/// effects combo's order (`ColorAndFontDescriptionPanel`).
const EFFECTS: &[&str] = &[
    "Bordered",
    "Underscored",
    "Underwaved",
    "Strikeout",
    "Bold Underscored",
    "Dotted Line",
];
const EFFECT_CHOICES: &[&str] = &[
    "Underscored",
    "Bold Underscored",
    "Underwaved",
    "Bordered",
    "Strikeout",
    "Dotted Line",
    "Faded",
];

thread_local! {
    /// The attribute selected on each color page (none at first), and the groups open in its
    /// tree, by path: view state, not settings.
    static SELECTED: RefCell<HashMap<&'static str, Option<usize>>> = RefCell::new(HashMap::new());
    static OPEN_GROUPS: RefCell<HashMap<&'static str, std::collections::HashSet<String>>> =
        RefCell::new(HashMap::new());
}

/// The color pages the reference app opens with their options greyed while nothing is selected; the
/// others show none.
const GREYED_OPTIONS: &[&str] = &["Code Review", "VCS"];

/// Diff & Merge opens on its first color, its group open, as the reference app's `DiffColorsPage`.
const DIFF_PAGE: &str = "Diff & Merge";

/// The by-name keys of Color Scheme Font or Console Font.
struct FontKeys {
    overwrite_label: &'static str,
    overwrite: &'static str,
    font: &'static str,
    monospaced: &'static str,
    size: &'static str,
    line_height: &'static str,
    fallback: &'static str,
    ligatures: &'static str,
}

const SCHEME_FONT: FontKeys = FontKeys {
    overwrite_label: "Use color scheme font instead of the",
    overwrite: "schemeFont.overwrite",
    font: "schemeFont.font",
    monospaced: "schemeFont.monospacedOnly",
    size: "schemeFont.size",
    line_height: "schemeFont.lineHeight",
    fallback: "schemeFont.fallback",
    ligatures: "schemeFont.ligatures",
};

const CONSOLE_FONT: FontKeys = FontKeys {
    overwrite_label: "Use console font instead of the",
    overwrite: "consoleFont.overwrite",
    font: "consoleFont.font",
    monospaced: "consoleFont.monospacedOnly",
    size: "consoleFont.size",
    line_height: "consoleFont.lineHeight",
    fallback: "consoleFont.fallback",
    ligatures: "consoleFont.ligatures",
};

/// A line marker on Gutter Icons: its key, its icon, its name.
type Marker = (&'static str, &'static str, &'static str);

/// The line markers the reference app registers, by plugin ("Common" is the platform's), then by name.
const GUTTER_MARKERS: &[(&str, &[Marker])] = &[
    (
        "Common",
        &[
            ("gutterIcons.colorPreview", "gutter/colors", "Color preview"),
            (
                "gutterIcons.docRendering",
                "gutter/javadocRead",
                "Documentation comments in-place rendering",
            ),
            (
                "gutterIcons.lspInheritance",
                "gutter/implementedMethod",
                "LSP Overridden and Implemented Declarations",
            ),
            ("gutterIcons.runLineMarker", "gutter/run", "Run line marker"),
        ],
    ),
    (
        "Git",
        &[(
            "gutterIcons.vcsIgnoredDirectories",
            "nodes/folder",
            "Version control ignored directories",
        )],
    ),
];

// shortcut: a fixed list of macOS monospaced fonts, since the store's combos take static choices;
// list the system's fonts once the window shares its font list with these pages.
const MONOSPACED_FONTS: &[&str] = &[
    "JetBrains Mono",
    "Menlo",
    "Monaco",
    "SF Mono",
    "Courier New",
];

impl SettingsWindow {
    /// The page at `path` when it is one of these, else None.
    pub(crate) fn editor_settings(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let page = match path {
            "Editor/General" => self.editor_general_page(window, cx),
            "Editor/General/Auto Import" => self.auto_import_page(cx),
            "Editor/General/Appearance" => self.editor_appearance_page(window, cx),
            "Editor/General/Breadcrumbs" => self.breadcrumbs_page(cx),
            "Editor/General/Code Completion" => code_completion_page(),
            "Editor/General/Code Completion/Popup" => self.completion_popup_page(window, cx),
            "Editor/General/Code Completion/Inline" => inline_completion_page(cx),
            "Editor/General/Code Folding" => self.code_folding_page(cx),
            "Editor/General/Console" => self.console_page(window, cx),
            "Editor/General/Editor Tabs" => self.editor_tabs_page(window, cx),
            "Editor/General/Gutter Icons" => self.gutter_icons_page(cx),
            "Editor/General/Postfix Completion" => self.postfix_page(cx),
            "Editor/General/Smart Keys" => self.smart_keys_page(cx),
            "Editor/General/Smart Keys/HTML/CSS" => self.web_smart_keys_page(cx),
            "Editor/General/Sticky Lines" => self.sticky_lines_page(window, cx),
            "Editor/Code Editing" => self.code_editing_page(window, cx),
            "Editor/Color Scheme" => self.color_scheme_page(cx),
            "Editor/Color Scheme/Color Scheme Font" => {
                self.scheme_font_page(&SCHEME_FONT, window, cx)
            }
            "Editor/Color Scheme/Console Font" => self.scheme_font_page(&CONSOLE_FONT, window, cx),
            other => {
                let name = other.strip_prefix("Editor/Color Scheme/")?;
                let &(name, data, sample) = COLOR_PAGES.iter().find(|(page, ..)| *page == name)?;
                self.color_page(name, data, sample, cx)
            }
        };
        Some(page)
    }

    /// A checkbox that IntelliJ greys out while `enabled` is false.
    fn check_if(
        &self,
        enabled: bool,
        key: &'static str,
        label: &'static str,
        default: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if enabled {
            return self.opt_check(key, label, default, cx).into_any_element();
        }
        spork_workbench::controls::disabled_check(key, label, self.flag(key, default), cx)
            .into_any_element()
    }

    /// A combo box that IntelliJ greys out while `enabled` is false.
    pub(crate) fn combo_if(
        &self,
        enabled: bool,
        key: &'static str,
        choices: &'static [&'static str],
        default: &'static str,
        width: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        if enabled {
            return self.opt_combo(key, choices, default, width, cx);
        }
        let current = self.value(key, default);
        div().w(px(width)).child(spork_workbench::controls::combo(
            key,
            current,
            true,
            |menu, _, _| menu,
        ))
    }

    /// Editor ▸ General itself, as IntelliJ's `EditorOptionsPanel`.
    fn editor_general_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let wheel = self.flag("editor.wheelFontChange", false);
        let wrap = self.flag("editor.softWrapFiles", true);
        let rich_text = self.flag("editor.richCopy", true);
        let trailing = self.flag("editor.stripTrailingSpaces", true);
        let tokens = Tokens::get(cx);
        let hint = |text: &'static str| div().text_xs().text_color(tokens.text_muted).child(text);
        let number = |this: &mut Self,
                      label: &'static str,
                      key: &'static str,
                      default: &'static str,
                      window: &mut Window,
                      cx: &mut Context<Self>| {
            h_flex()
                .gap_2()
                .child(div().w(px(170.)).child(label))
                .child(this.opt_text(key, default, 56., window, cx))
        };
        let wheel_radios = |this: &Self, cx: &mut Context<Self>| {
            let key = "editor.wheelFontChangeIn";
            h_flex()
                .gap_4()
                .child(this.radio_if(wheel, key, &["Active editor"], "Active editor", cx))
                .child(this.radio_if(wheel, key, &["All editors"], "Active editor", cx))
        };
        let mouse = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(self.opt_check(
                "editor.wheelFontChange",
                "Change font size with Command+Mouse Wheel in:",
                false,
                cx,
            ))
            .child(div().pl(px(INDENT)).child(wheel_radios(self, cx)))
            .child(
                h_flex()
                    .gap_3()
                    .child(self.opt_check(
                        "editor.dndEnabled",
                        "Move code fragments with drag-and-drop",
                        true,
                        cx,
                    ))
                    .child(hint("To copy, hold Command while dragging")),
            );
        let wrap_field = self.opt_text_enabled(
            "editor.softWrapFileMasks",
            "*.md; *.txt; *.rst; *.adoc",
            360.,
            wrap,
            window,
            cx,
        );
        let indent_field = self.opt_text("editor.softWrapIndent", "0", 56., window, cx);
        let soft_wraps = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(self.opt_check(
                        "editor.softWrapFiles",
                        "Soft-wrap these files:",
                        true,
                        cx,
                    ))
                    .child(wrap_field),
            )
            .child(
                div()
                    .pl(px(170.))
                    .child(hint("Use * and ? as wildcards and ; to separate patterns")),
            )
            .child(self.opt_check(
                "editor.softWrapOriginalIndent",
                "Use the original line's indent for wrapped fragments",
                true,
                cx,
            ))
            .child(
                h_flex()
                    .pl(px(INDENT))
                    .gap_2()
                    .child("Add additional indent:")
                    .child(indent_field)
                    .child("symbols"),
            )
            .child(self.opt_check(
                "editor.softWrapCurrentLineIndicator",
                "Only show soft-wrap indicators for the current line",
                true,
                cx,
            ));
        let virtual_space = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(
                h_flex()
                    .gap_3()
                    .child("Allow caret placement:")
                    .child(self.opt_check(
                        "editor.virtualSpaceAfterLineEnd",
                        "After the end of line",
                        false,
                        cx,
                    ))
                    .child(self.opt_check(
                        "editor.virtualSpaceInsideTabs",
                        "Inside tabs",
                        false,
                        cx,
                    )),
            )
            .child(self.opt_check(
                "editor.virtualSpaceAtBottom",
                "Show virtual space at the bottom of the file",
                false,
                cx,
            ));
        let scroll_offset = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(number(
                self,
                "Vertical scroll offset:",
                "editor.verticalScrollOffset",
                "1",
                window,
                cx,
            ))
            .child(number(
                self,
                "Vertical scroll jump:",
                "editor.verticalScrollJump",
                "0",
                window,
                cx,
            ))
            .child(number(
                self,
                "Horizontal scroll offset:",
                "editor.horizontalScrollOffset",
                "3",
                window,
                cx,
            ))
            .child(number(
                self,
                "Horizontal scroll jump:",
                "editor.horizontalScrollJump",
                "0",
                window,
                cx,
            ));
        let caret = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(labeled(
                "When moving by words:",
                self.opt_combo(
                    "editor.caretWordMove",
                    &[
                        "Jump to the current word boundaries",
                        "Always jump to the word start",
                        "Always jump to the word end",
                        "Jump to the next/previous word boundaries",
                        "Stop at both word boundaries",
                    ],
                    "Jump to the current word boundaries",
                    300.,
                    cx,
                )
                .into_any_element(),
            ))
            .child(labeled(
                "Upon line break:",
                self.opt_combo(
                    "editor.caretLineBreak",
                    &[
                        "Jump to the next/previous line boundaries",
                        "Ignore line breaks",
                        "Stop at both line boundaries",
                        "Jump to the current line boundaries",
                        "Always jump to the line start",
                        "Always jump to the line end",
                    ],
                    "Jump to the next/previous line boundaries",
                    300.,
                    cx,
                )
                .into_any_element(),
            ));
        let scrolling = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(self.opt_check(
                "editor.smoothScrolling",
                "Enable smooth scrolling",
                true,
                cx,
            ))
            .child("Caret behavior:")
            .child(div().pl(px(INDENT)).child(self.opt_radio(
                "editor.caretBehavior",
                &[
                    "Keep the caret in place, scroll editor canvas",
                    "Move caret, minimize editor scrolling",
                ],
                "Keep the caret in place, scroll editor canvas",
                cx,
            )));
        let rich_copy = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(
                h_flex()
                    .gap_3()
                    .child(self.opt_check("editor.richCopy", "Copy (⌘C) as rich text", true, cx))
                    .child(hint(
                        "All formatting will be copied, including font, colors and so on",
                    )),
            )
            .child(
                div().pl(px(INDENT)).child(labeled(
                    "Color scheme for copied fragment:",
                    self.combo_if(
                        rich_text,
                        "editor.richCopyScheme",
                        &[
                            "Active scheme",
                            "Light",
                            "Dark",
                            "High contrast",
                            "Classic Light",
                            "Darcula",
                            "Darcula Contrast",
                            "IntelliJ Light",
                            "Islands Dark",
                        ],
                        "Active scheme",
                        200.,
                        cx,
                    )
                    .into_any_element(),
                )),
            );
        let on_save = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(self.opt_check(
                        "editor.stripTrailingSpaces",
                        "Remove trailing spaces on:",
                        true,
                        cx,
                    ))
                    .child(self.combo_if(
                        trailing,
                        "editor.stripTrailingSpacesOn",
                        &["Modified lines", "All lines"],
                        "Modified lines",
                        160.,
                        cx,
                    )),
            )
            .child(div().pl(px(INDENT)).child(self.check_if(
                trailing,
                "editor.keepTrailingSpacesOnCaretLine",
                "Keep trailing spaces on caret line",
                true,
                cx,
            )))
            .child(self.opt_check(
                "editor.removeTrailingBlankLines",
                "Remove trailing blank lines at the end of saved files",
                false,
                cx,
            ))
            .child(self.opt_check(
                "editor.ensureNewlineAtEof",
                "Ensure every saved file ends with a line break",
                false,
                cx,
            ));
        v_flex()
            .gap_3()
            .child(titled("Mouse Control", cx))
            .child(mouse)
            .child(titled("Soft Wraps", cx))
            .child(soft_wraps)
            .child(titled("Virtual Space", cx))
            .child(virtual_space)
            .child(titled("Scroll Offset", cx))
            .child(scroll_offset)
            .child(titled("Caret Movement", cx))
            .child(caret)
            .child(titled("Scrolling", cx))
            .child(scrolling)
            .child(titled("Rich-Text Copy", cx))
            .child(rich_copy)
            .child(titled("On Save", cx))
            .child(on_save)
    }

    fn auto_import_page(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .gap_3()
            .child(titled("XML", cx))
            .child(div().pl(px(INDENT)).child(self.opt_check(
                "autoImport.xmlTooltip",
                "Show auto-import tooltip",
                true,
                cx,
            )))
    }

    fn editor_appearance_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let blinking = self.flag("editorAppearance.caretBlinking", true);
        let whitespaces = self.shows_whitespace();
        let tag_tree = self.flag("editorAppearance.tagTreeHighlighting", true);
        let caret = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(self.opt_check(
                        "editorAppearance.caretBlinking",
                        "Caret blinking (ms):",
                        true,
                        cx,
                    ))
                    .child(self.opt_text(
                        "editorAppearance.caretBlinkingMs",
                        "500",
                        64.,
                        window,
                        cx,
                    )),
            )
            .child(self.check_if(
                blinking,
                "editorAppearance.smoothCaretBlinking",
                "Use smooth caret blinking",
                false,
                cx,
            ))
            .child(self.opt_check("editorAppearance.blockCaret", "Use block caret", false, cx))
            .child(self.opt_check(
                "editorAppearance.fullLineHeightCaret",
                "Use full line height caret",
                false,
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(self.opt_check(
                        "editorAppearance.smoothCaretMovement",
                        "Use smooth caret movement",
                        false,
                        cx,
                    ))
                    .child(self.opt_combo(
                        "editorAppearance.caretEasing",
                        &["Snappy", "Gliding"],
                        "Snappy",
                        120.,
                        cx,
                    )),
            );
        let code_structure = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(self.opt_check(
                "editorAppearance.rightMargin",
                "Show hard wrap and visual guides (configured in Code Style options)",
                true,
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(self.opt_check(
                        "editorAppearance.lineNumbers",
                        "Show line numbers:",
                        true,
                        cx,
                    ))
                    .child(self.opt_combo(
                        "editorAppearance.lineNumeration",
                        &["Absolute", "Relative", "Hybrid"],
                        "Absolute",
                        120.,
                        cx,
                    )),
            )
            .child(self.opt_check(
                "editorAppearance.methodSeparators",
                "Show method separators",
                false,
                cx,
            ))
            .child(self.opt_check(
                "editorAppearance.indentGuides",
                "Show indent guides",
                true,
                cx,
            ));
        let presentation = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(self.opt_check(
                "editorAppearance.highlightSelectionOccurrences",
                "Highlight occurrences of selected text",
                true,
                cx,
            ))
            // Spork's diffs show their whitespace with it
            .child(self.whitespace_check(cx))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_2()
                    .child(self.check_if(
                        whitespaces,
                        "editorAppearance.leadingWhitespaces",
                        "Leading",
                        true,
                        cx,
                    ))
                    .child(self.check_if(
                        whitespaces,
                        "editorAppearance.innerWhitespaces",
                        "Inner",
                        true,
                        cx,
                    ))
                    .child(self.check_if(
                        whitespaces,
                        "editorAppearance.trailingWhitespaces",
                        "Trailing",
                        true,
                        cx,
                    ))
                    .child(self.check_if(
                        whitespaces,
                        "editorAppearance.selectionWhitespaces",
                        "Selection",
                        true,
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(self.opt_check(
                        "editorAppearance.renderedDocs",
                        "Render documentation comments",
                        false,
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_1()
                            .text_color(Tokens::get(cx).text_muted)
                            .child("Also in")
                            .child(nav_link(
                                "reader-mode-link",
                                "Reader mode",
                                "Editor/Reader Mode",
                                cx,
                            )),
                    ),
            )
            .child(self.opt_check(
                "editorAppearance.editorFontInInlays",
                "Use editor font for inlay hints",
                false,
                cx,
            ))
            .child(self.opt_check(
                "editorAppearance.tagTreeHighlighting",
                "Enable HTML/XML tag tree highlighting",
                true,
                cx,
            ))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_2()
                    .when(!tag_tree, |levels| {
                        levels.text_color(Tokens::get(cx).text_disabled)
                    })
                    .child(labeled(
                        "Levels to highlight:",
                        self.opt_text("editorAppearance.tagTreeLevels", "6", 64., window, cx)
                            .into_any_element(),
                    ))
                    .child(labeled(
                        "Opacity:",
                        self.opt_text("editorAppearance.tagTreeOpacity", "0.1", 64., window, cx)
                            .into_any_element(),
                    )),
            );
        let code_assistance = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(self.opt_check(
                "editorAppearance.intentionBulb",
                "Show intention bulb",
                true,
                cx,
            ))
            .child(self.opt_check(
                "editorAppearance.intentionPreview",
                "Show preview for intention actions when available",
                true,
                cx,
            ))
            .child(self.opt_check(
                "editorAppearance.codeLensOnScrollbar",
                "Show code lens on scrollbar hover",
                true,
                cx,
            ));
        v_flex()
            .gap_3()
            .child(titled("Caret", cx))
            .child(caret)
            .child(titled("Code Structure", cx))
            .child(code_structure)
            .child(titled("Presentation", cx))
            .child(presentation)
            .child(titled("Code Assistance", cx))
            .child(code_assistance)
    }

    fn breadcrumbs_page(&self, cx: &mut Context<Self>) -> Div {
        let shown = self.flag("breadcrumbs.show", true);
        v_flex()
            .gap_3()
            .child(self.opt_check("breadcrumbs.show", "Show breadcrumbs", true, cx))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_2()
                    .when(!shown, |options| {
                        options.text_color(Tokens::get(cx).text_disabled)
                    })
                    .child(labeled(
                        "Placement:",
                        self.opt_radio("breadcrumbs.placement", &["Top", "Bottom"], "Bottom", cx)
                            .flex_row()
                            .gap_4()
                            .into_any_element(),
                    ))
                    .child("Languages:")
                    .child(
                        // The reference app shows them off
                        h_flex()
                            .gap_4()
                            .child(self.check_if(shown, "breadcrumbs.html", "HTML", false, cx))
                            .child(self.check_if(shown, "breadcrumbs.xhtml", "XHTML", false, cx))
                            .child(self.check_if(shown, "breadcrumbs.xml", "XML", false, cx)),
                    ),
            )
            .child(nav_link(
                "breadcrumbs-colors",
                "Manage colors",
                "Editor/Color Scheme/General",
                cx,
            ))
    }

    fn completion_popup_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let match_case = self.flag("completion.matchCase", true);
        let as_you_type = self.flag("completion.autoPopup", true);
        let documentation = self.flag("completion.documentationPopup", false);
        let radios = self
            .opt_radio(
                "completion.matchCaseLetters",
                &["First letter only", "All letters"],
                "First letter only",
                cx,
            )
            .flex_row()
            .gap_4()
            .when(!match_case, |radios| {
                radios.text_color(Tokens::get(cx).text_disabled)
            });
        v_flex()
            .gap_3()
            .child("Displays suggestions in a popup while typing, based on static analysis of the code")
            .child(
                h_flex()
                    .gap_4()
                    .child(self.opt_check("completion.matchCase", "Match case:", true, cx))
                    .child(radios),
            )
            .child(self.opt_check(
                "completion.sortAlphabetically",
                "Sort suggestions alphabetically",
                false,
                cx,
            ))
            .child(self.opt_check(
                "completion.autoPopup",
                "Show suggestions as you type",
                true,
                cx,
            ))
            .child(div().pl(px(INDENT)).child(self.check_if(
                as_you_type,
                "completion.selectByChars",
                "Insert selected suggestion by pressing space, dot, or other context-dependent keys",
                false,
                cx,
            )))
            .child(
                h_flex()
                    .gap_2()
                    .child(self.opt_check(
                        "completion.documentationPopup",
                        "Show the documentation popup in",
                        false,
                        cx,
                    ))
                    .child(self.opt_text_enabled(
                        "completion.documentationDelay",
                        "500",
                        64.,
                        documentation,
                        window,
                        cx,
                    ))
                    .child("ms"),
            )
            .child(self.opt_check(
                "completion.insertParentheses",
                "Insert parentheses automatically when applicable",
                true,
                cx,
            ))
            .child(titled("HTML", cx))
            .child(div().pl(px(INDENT)).child(self.opt_check(
                "completion.htmlTagAutoPopup",
                "Enable auto-popup of tag name code completion when typing in HTML text",
                true,
                cx,
            )))
            .child(titled("Parameter Info", cx))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(self.opt_check(
                                "completion.parameterInfoPopup",
                                "Show the parameter info popup in",
                                true,
                                cx,
                            ))
                            .child(self.opt_text(
                                "completion.parameterInfoDelay",
                                "1000",
                                64.,
                                window,
                                cx,
                            ))
                            .child("ms"),
                    )
                    .child(self.opt_check(
                        "completion.fullSignatures",
                        "Show full method signatures",
                        false,
                        cx,
                    )),
            )
    }

    fn code_folding_page(&self, cx: &mut Context<Self>) -> Div {
        let arrows = self.flag("codeFolding.arrows", true);
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .child(self.opt_check(
                        "codeFolding.arrows",
                        "Show code folding arrows",
                        true,
                        cx,
                    ))
                    .child(self.opt_combo(
                        "codeFolding.arrowsShown",
                        &["Always", "On mouse hover"],
                        "On mouse hover",
                        140.,
                        cx,
                    )),
            )
            .child(div().pl(px(INDENT)).child(self.check_if(
                arrows,
                "codeFolding.bottomArrows",
                "Show bottom arrows",
                false,
                cx,
            )))
            .child(div().pt_2().child("Fold by default:"))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_3()
                    .child(titled("General", cx))
                    .child(
                        v_flex()
                            .pl(px(INDENT))
                            .gap_2()
                            .child(self.opt_check(
                                "codeFolding.fileHeader",
                                "File header",
                                true,
                                cx,
                            ))
                            .child(self.opt_check("codeFolding.imports", "Imports", true, cx))
                            .child(self.opt_check(
                                "codeFolding.docComments",
                                "Documentation comments",
                                false,
                                cx,
                            ))
                            .child(self.opt_check(
                                "codeFolding.methodBodies",
                                "Method bodies",
                                false,
                                cx,
                            ))
                            .child(self.opt_check(
                                "codeFolding.customRegions",
                                "Custom folding regions",
                                false,
                                cx,
                            )),
                    )
                    .child(titled("XML", cx))
                    .child(
                        v_flex()
                            .pl(px(INDENT))
                            .gap_2()
                            .child(self.opt_check("codeFolding.xmlTags", "XML tags", false, cx))
                            .child(self.opt_check(
                                "codeFolding.htmlStyle",
                                "HTML 'style' attribute",
                                true,
                                cx,
                            ))
                            .child(self.opt_check(
                                "codeFolding.xmlEntities",
                                "XML entities",
                                true,
                                cx,
                            ))
                            .child(self.opt_check("codeFolding.dataUris", "Data URIs", true, cx)),
                    ),
            )
    }

    fn console_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let override_buffer = self.flag("editorConsole.overrideCycleBuffer", false);
        v_flex()
            .gap_3()
            .child(self.opt_check(
                "editorConsole.softWraps",
                "Use soft wraps in console",
                false,
                cx,
            ))
            .child(labeled(
                "Console commands history size:",
                self.opt_text("editorConsole.historySize", "300", 64., window, cx)
                    .into_any_element(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(self.opt_check(
                        "editorConsole.overrideCycleBuffer",
                        "Override console cycle buffer size (1024 KB)",
                        false,
                        cx,
                    ))
                    .child(self.opt_text_enabled(
                        "editorConsole.cycleBufferKb",
                        "1024",
                        64.,
                        override_buffer,
                        window,
                        cx,
                    ))
                    .child("KB"),
            )
            .child(labeled(
                "Default Encoding:",
                self.opt_combo(
                    "editorConsole.encoding",
                    &[
                        "<System Default: UTF-8>",
                        "UTF-8",
                        "UTF-16",
                        "ISO-8859-1",
                        "US-ASCII",
                        "windows-1252",
                    ],
                    "<System Default: UTF-8>",
                    220.,
                    cx,
                )
                .into_any_element(),
            ))
            .child(rule_list(
                ("console-fold-add", "console-fold-remove"),
                "Fold console lines that contain:",
                "Fold nothing",
                cx,
            ))
            .child(rule_list(
                ("console-exceptions-add", "console-exceptions-remove"),
                "Exceptions:",
                "No exceptions",
                cx,
            ))
    }

    fn editor_tabs_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let placement = self.value("editorTabs.placement", "Top");
        let horizontal = placement == "Top" || placement == "Bottom";
        let visible = placement != "None";
        let one_row = self.value("editorTabs.rows", "One row, and if tabs don't fit:")
            == "One row, and if tabs don't fit:";
        let appearance = v_flex()
            .pl(px(INDENT))
            .gap_2()
            .child(labeled(
                "Tab placement:",
                self.opt_combo(
                    "editorTabs.placement",
                    &["Top", "Left", "Bottom", "Right", "None"],
                    "Top",
                    120.,
                    cx,
                )
                .into_any_element(),
            ))
            .child(
                v_flex()
                    .gap_2()
                    .when(!horizontal, |rows| {
                        rows.text_color(Tokens::get(cx).text_disabled)
                    })
                    .child("Show tabs in:")
                    .child(self.opt_radio(
                        "editorTabs.rows",
                        &["One row, and if tabs don't fit:"],
                        "One row, and if tabs don't fit:",
                        cx,
                    ))
                    .child(
                        div()
                            .pl(px(INDENT))
                            .when(!one_row, |overflow| {
                                overflow.text_color(Tokens::get(cx).text_disabled)
                            })
                            .child(self.opt_radio(
                                "editorTabs.overflow",
                                &["Scroll the tabs panel", "Squeeze tabs"],
                                "Scroll the tabs panel",
                                cx,
                            )),
                    )
                    .child(self.opt_radio(
                        "editorTabs.rows",
                        &["Multiple rows"],
                        "One row, and if tabs don't fit:",
                        cx,
                    )),
            )
            .child(self.check_if(
                horizontal,
                "editorTabs.pinnedSeparateRow",
                "Show pinned tabs in a separate row",
                false,
                cx,
            ))
            .child(self.check_if(visible, "editorTabs.fileIcon", "Show file icon", true, cx))
            .child(self.check_if(
                visible,
                "editorTabs.fileExtension",
                "Show file extension",
                true,
                cx,
            ))
            .child(self.check_if(
                visible,
                "editorTabs.directoryForNonUnique",
                "Show directory for non-unique file names",
                true,
                cx,
            ))
            .child(self.check_if(
                visible,
                "editorTabs.markModified",
                "Mark modified",
                false,
                cx,
            ))
            .child(self.check_if(
                visible,
                "editorTabs.fullPathOnHover",
                "Show full path on mouse hover",
                true,
                cx,
            ))
            .child(labeled(
                "Close button position:",
                self.combo_if(
                    visible,
                    "editorTabs.closeButton",
                    &["Left", "Right", "None"],
                    "Right",
                    120.,
                    cx,
                )
                .into_any_element(),
            ));
        v_flex()
            .gap_3()
            .child(titled("Appearance", cx))
            .child(appearance)
            .child(titled("Tab Order", cx))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_2()
                    .child(self.opt_check(
                        "editorTabs.sortAlphabetically",
                        "Sort tabs alphabetically",
                        false,
                        cx,
                    ))
                    .child(self.opt_check(
                        "editorTabs.openAtEnd",
                        "Open new tabs at the end",
                        false,
                        cx,
                    )),
            )
            .child(titled("Opening Policy", cx))
            .child(div().pl(px(INDENT)).child(self.opt_check(
                "editorTabs.previewTab",
                "Enable preview tab",
                false,
                cx,
            )))
            .child(titled("Closing Policy", cx))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_2()
                    .child(labeled(
                        "Tab limit:",
                        self.opt_text("editorTabs.limit", "30", 64., window, cx)
                            .into_any_element(),
                    ))
                    .child("When tabs exceed the limit:")
                    .child(self.opt_radio(
                        "editorTabs.whenExceeded",
                        &["Close unchanged", "Close unused"],
                        "Close unused",
                        cx,
                    ))
                    .child("When the current tab is closed, activate:")
                    .child(self.opt_radio(
                        "editorTabs.activateOnClose",
                        &[
                            "The tab on the left",
                            "The tab on the right",
                            "Most recently opened tab",
                        ],
                        "The tab on the left",
                        cx,
                    )),
            )
    }

    fn gutter_icons_page(&self, cx: &mut Context<Self>) -> Div {
        let shown = self.flag("gutterIcons.show", true);
        let tokens = Tokens::get(cx);
        let mut list = v_flex()
            .p_1()
            .border_1()
            .border_color(tokens.control_border)
            .bg(tokens.control_bg);
        for &(plugin, items) in GUTTER_MARKERS {
            // IntelliJ centers a plugin's title between two lines
            list = list.child(
                h_flex()
                    .gap_2()
                    .text_color(tokens.text_muted)
                    .child(div().flex_1().h(px(1.)).bg(tokens.separator))
                    .child(plugin)
                    .child(div().flex_1().h(px(1.)).bg(tokens.separator)),
            );
            for &(key, icon, label) in items {
                list = list.child(
                    h_flex()
                        .h(px(metrics::ROW_HEIGHT))
                        .gap_1()
                        .child(div().size(px(metrics::TOOLBAR_ICON)).when(
                            !icon.is_empty(),
                            |slot| {
                                slot.child(
                                    spork_workbench::image(icon, cx)
                                        .size(px(metrics::TOOLBAR_ICON)),
                                )
                            },
                        ))
                        .child(self.check_if(shown, key, label, true, cx)),
                );
            }
        }
        v_flex()
            .gap_3()
            .child(self.opt_check("gutterIcons.show", "Show gutter icons", true, cx))
            .child(list)
    }

    fn postfix_page(&self, cx: &mut Context<Self>) -> Div {
        let enabled = self.flag("postfix.enabled", true);
        let tokens = Tokens::get(cx);
        let templates = v_flex()
            .w(px(COLUMN))
            .h(px(260.))
            .border_1()
            .border_color(tokens.control_border)
            .child(
                h_flex()
                    .p_1()
                    .gap_1()
                    .border_b_1()
                    .border_color(tokens.control_border)
                    // nothing is selected in the empty tree
                    .child(todo_action("postfix-add", "general/add", "Add").popup_marker())
                    .child(todo_action("postfix-remove", "general/remove", "Remove").disabled(true))
                    .child(todo_action("postfix-edit", "general/edit", "Edit").disabled(true))
                    .child(
                        todo_action("postfix-duplicate", "general/copy", "Duplicate")
                            .disabled(true),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(tokens.control_bg)
                    .text_color(tokens.text_muted)
                    .child("Nothing to show"),
            );
        // what the reference app shows while a language group is selected
        let description = v_flex()
            .flex_1()
            .min_w_0()
            .gap_3()
            .child(
                "You have selected a postfix completion language group.\n\
                 By clicking the checkbox, you can enable or disable all postfix templates for this language.\n\
                 To show the details of a particular postfix template select it inside the group.",
            )
            .child(
                v_flex().gap_1().child("Before:").child(numbered_sample(
                    &[
                        &["Sample code for the selected"],
                        &["template will be shown here."],
                        &["A ", "flashing rectangle", " shows where"],
                        &["the template can be used."],
                    ],
                    cx,
                )),
            )
            .child(v_flex().gap_1().child("After:").child(numbered_sample(
                &[
                    &["The result of the Postfix completion"],
                    &["will be shown here."],
                ],
                cx,
            )));
        v_flex()
            .gap_3()
            .child(self.opt_check("postfix.enabled", "Enable postfix completion", true, cx))
            .child(labeled(
                "Expand templates with",
                self.combo_if(
                    enabled,
                    "postfix.expandKey",
                    &["Tab", "Space", "Enter"],
                    "Tab",
                    100.,
                    cx,
                )
                .into_any_element(),
            ))
            .child(
                h_flex()
                    .items_start()
                    .gap_4()
                    .child(templates.h(px(400.)))
                    .child(description),
            )
    }

    fn smart_keys_page(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .gap_3()
            .child(
                v_flex()
                    .gap_2()
                    .child(self.opt_check(
                        "smartKeys.smartHome",
                        "Home moves caret to first non-whitespace character",
                        true,
                        cx,
                    ))
                    .child(self.opt_check(
                        "smartKeys.smartEnd",
                        "End on blank line moves caret to indent position",
                        true,
                        cx,
                    ))
                    .child(self.opt_check(
                        "smartKeys.pairBracket",
                        "Insert paired brackets (), [], {}, <>",
                        true,
                        cx,
                    ))
                    .child(self.opt_check("smartKeys.pairQuote", "Insert pair quote", true, cx))
                    .child(self.opt_check(
                        "smartKeys.reformatOnRBrace",
                        "Reformat block on typing '}'",
                        true,
                        cx,
                    ))
                    .child(self.opt_check(
                        "smartKeys.camelWords",
                        "Use \"CamelHumps\" words",
                        false,
                        cx,
                    ))
                    .child(self.opt_check(
                        "smartKeys.camelWordsOnDoubleClick",
                        "Honor \"CamelHumps\" words settings when selecting on double click",
                        true,
                        cx,
                    ))
                    .child(self.opt_check(
                        "smartKeys.surroundSelection",
                        "Surround selection on typing quote or brace",
                        true,
                        cx,
                    ))
                    .child(self.opt_check(
                        "smartKeys.caretsOnDoubleModifier",
                        "Add multiple carets on double ⌥ with arrow keys",
                        true,
                        cx,
                    ))
                    .child(self.opt_check(
                        "smartKeys.tabExitsBrackets",
                        "Jump outside closing bracket/quote with Tab when typing",
                        true,
                        cx,
                    )),
            )
            .child(titled("Enter", cx))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_2()
                    .child(self.opt_check("smartKeys.smartIndent", "Smart indent", true, cx))
                    .child(self.opt_check("smartKeys.pairCurlyBrace", "Insert pair '}'", true, cx))
                    .child(self.opt_check(
                        "smartKeys.closeBlockComment",
                        "Close block comment",
                        true,
                        cx,
                    )),
            )
            .child(labeled(
                "Unindent on Backspace:",
                self.opt_combo(
                    "smartKeys.backspace",
                    &[
                        "Disabled",
                        "To nearest indent position",
                        "To proper indent position",
                    ],
                    "To proper indent position",
                    220.,
                    cx,
                )
                .into_any_element(),
            ))
            .child(labeled(
                "Reformat on paste:",
                self.opt_combo(
                    "smartKeys.reformatOnPaste",
                    &["None", "Indent block", "Indent each line", "Reformat block"],
                    "Indent each line",
                    180.,
                    cx,
                )
                .into_any_element(),
            ))
            .child(self.opt_check(
                "smartKeys.secondReformat",
                "Reformat again to remove custom line breaks",
                false,
                cx,
            ))
    }

    fn web_smart_keys_page(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .gap_3()
            .child(titled("XML/HTML", cx))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_2()
                    .child(self.opt_check(
                        "htmlCss.closingTag",
                        "Insert closing tag on tag completion",
                        true,
                        cx,
                    ))
                    .child(self.opt_check(
                        "htmlCss.requiredAttributes",
                        "Insert required attributes on tag completion",
                        true,
                        cx,
                    ))
                    .child(self.opt_check(
                        "htmlCss.requiredSubtags",
                        "Insert required subtags on tag completion",
                        true,
                        cx,
                    ))
                    .child(self.opt_check(
                        "htmlCss.startAttribute",
                        "Start attribute on tag completion",
                        true,
                        cx,
                    ))
                    .child(self.opt_check(
                        "htmlCss.attributeQuotes",
                        "Add quotes for attribute value on typing '=' and attribute completion",
                        true,
                        cx,
                    ))
                    .child(self.opt_check(
                        "htmlCss.autoCloseTag",
                        "Auto-close tag on typing '</'",
                        true,
                        cx,
                    ))
                    .child(self.opt_check(
                        "htmlCss.syncTagEditing",
                        "Simultaneous '<tag></tag>' editing",
                        true,
                        cx,
                    )),
            )
            .child(titled("CSS", cx))
            .child(div().pl(px(INDENT)).child(self.opt_check(
                "htmlCss.wholeCssIdentifiers",
                "Select whole CSS identifiers on double click",
                true,
                cx,
            )))
    }

    fn sticky_lines_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let shown = self.flag("stickyLines.show", true);
        v_flex()
            .gap_3()
            .child(self.opt_check(
                "stickyLines.show",
                "Show sticky lines while scrolling",
                true,
                cx,
            ))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_2()
                    .when(!shown, |options| {
                        options.text_color(Tokens::get(cx).text_disabled)
                    })
                    .child(labeled(
                        "Maximum number of lines:",
                        self.opt_text("stickyLines.limit", "5", 48., window, cx)
                            .into_any_element(),
                    ))
                    .child("Languages:")
                    .child(
                        h_flex()
                            .gap_4()
                            .child(self.check_if(shown, "stickyLines.html", "HTML", true, cx))
                            .child(self.check_if(shown, "stickyLines.xhtml", "XHTML", true, cx))
                            .child(self.check_if(shown, "stickyLines.xml", "XML", true, cx)),
                    ),
            )
            .child(nav_link(
                "sticky-lines-colors",
                "Manage colors",
                "Editor/Color Scheme/General",
                cx,
            ))
    }

    fn code_editing_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        v_flex()
            .gap_3()
            .child(titled("Highlight on Caret Movement", cx))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_2()
                    .child(self.opt_check("codeEditing.matchedBrace", "Matched brace", true, cx))
                    .child(self.opt_check("codeEditing.currentScope", "Current scope", false, cx))
                    .child(self.opt_check(
                        "codeEditing.usagesAtCaret",
                        "Usages of element at caret",
                        true,
                        cx,
                    )),
            )
            .child(titled("Quick Documentation", cx))
            .child(div().pl(px(INDENT)).child(self.opt_check(
                "codeEditing.quickDocOnHover",
                "Show quick documentation on hover",
                true,
                cx,
            )))
            .child(titled("Refactorings", cx))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_2()
                    .child("Specify refactoring options:")
                    .child(self.opt_radio(
                        "codeEditing.refactoringOptions",
                        &["In the editor", "In modal dialogs"],
                        "In the editor",
                        cx,
                    ))
                    .child(self.opt_check(
                        "codeEditing.preselectRename",
                        "Preselect current symbol name for Rename refactoring",
                        true,
                        cx,
                    ))
                    .child(self.opt_check(
                        "codeEditing.inlineDialog",
                        "Show inline dialog for local variables",
                        true,
                        cx,
                    )),
            )
            .child(titled("Error Highlighting", cx))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .child("Error stripe mark min height:")
                            .child(self.opt_text(
                                "codeEditing.stripeMinHeight",
                                "2",
                                64.,
                                window,
                                cx,
                            ))
                            .child("pixels"),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child("Autoreparse delay:")
                            .child(self.opt_text(
                                "codeEditing.reparseDelay",
                                "300",
                                64.,
                                window,
                                cx,
                            ))
                            .child("milliseconds"),
                    )
                    .child(labeled(
                        "The 'Next Error' action goes through:",
                        self.opt_combo(
                            "codeEditing.nextError",
                            &["The problems with the highest priority", "All problems"],
                            "The problems with the highest priority",
                            280.,
                            cx,
                        )
                        .into_any_element(),
                    )),
            )
            .child(titled("Editor Tooltips", cx))
            .child(
                h_flex()
                    .pl(px(INDENT))
                    .gap_2()
                    .child("Tooltip delay:")
                    .child(self.opt_text("codeEditing.tooltipDelay", "500", 64., window, cx))
                    .child("milliseconds"),
            )
    }

    /// Whether the scheme in use is a dark one.
    pub(crate) fn dark_scheme(&self, cx: &App) -> bool {
        let scheme = self.value(SCHEME_KEY, default_scheme(cx));
        DARK_SCHEMES
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&scheme))
    }

    /// IntelliJ's `SchemesPanel`: "Scheme:", the combo, the gear, "Change IDE Theme…" and its help.
    fn scheme_row(&self, cx: &mut Context<Self>) -> Div {
        let default = default_scheme(cx);
        let gear = spork_workbench::ActionButton::new("scheme-actions", "general/settings")
            .tooltip("Show Scheme Actions")
            .dropdown_menu(|menu, window, cx| {
                menu.item(todo_item("Duplicate…"))
                    .item(PopupMenuItem::new("Rename…").disabled(true))
                    .item(PopupMenuItem::new("Restore Defaults").disabled(true))
                    .item(PopupMenuItem::new("Delete…").disabled(true))
                    .submenu("Export", window, cx, |menu, _, _| {
                        // The reference app names its vendor's product before these
                        menu.item(todo_item("Color scheme (.icls)"))
                            .item(todo_item("Color scheme plugin .jar"))
                    })
                    .separator()
                    .submenu("Import Scheme", window, cx, |menu, _, _| {
                        menu.item(todo_item("Color scheme (.icls) or settings (.jar)"))
                    })
            });
        h_flex()
            .gap_2()
            .child("Scheme:")
            .child(self.opt_combo(SCHEME_KEY, SCHEMES, default, 200., cx))
            .child(gear)
            .child(nav_link("change-ide-theme", "Change IDE Theme…", "Appearance & Behavior/Appearance", cx))
            .child(
                div()
                    .id("scheme-help")
                    .text_color(Tokens::get(cx).icon)
                    .child(
                        spork_workbench::image("general/questionMark", cx)
                            .size(px(metrics::TOOLBAR_ICON)),
                    )
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(
                            "The IDE theme defines the appearance of windows, dialogs, buttons, and all visual elements of the user interface",
                        )
                        .build(window, cx)
                    }),
            )
    }

    /// Color Scheme itself: the scheme, then its pages as links, as `ColorAndFontOptions`.
    fn color_scheme_page(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .gap_3()
            .child(self.scheme_row(cx))
            .child(
                div()
                    .pt_2()
                    .child("Configure colors and the font for source code and console output:"),
            )
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap_2()
                    .children(COLOR_SCHEME.iter().map(|page| {
                        nav_link(
                            SharedString::from(format!("scheme-page-{}", page.name)),
                            page.name,
                            format!("Editor/Color Scheme/{}", page.name),
                            cx,
                        )
                    })),
            )
    }

    /// Color Scheme Font and Console Font: `FontOptions` / `ConsoleFontOptions` and their preview.
    fn scheme_font_page(
        &mut self,
        keys: &FontKeys,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let overwrite = self.flag(keys.overwrite, false);
        let tokens = Tokens::get(cx);
        // The reference app's two columns: the font, its size and ligatures; the fallback font beside
        let font = v_flex()
            .gap_2()
            .child(labeled(
                "Font:",
                self.combo_if(
                    overwrite,
                    keys.font,
                    MONOSPACED_FONTS,
                    "JetBrains Mono",
                    200.,
                    cx,
                )
                .into_any_element(),
            ))
            .child(div().pl(px(40.)).child(self.check_if(
                overwrite,
                keys.monospaced,
                "Show only monospaced fonts",
                true,
                cx,
            )))
            .child(
                h_flex()
                    .gap_2()
                    .child("Size:")
                    .child(self.opt_text_enabled(keys.size, "13.0", 64., overwrite, window, cx))
                    .child(div().pl_4().child("Line height:"))
                    .child(self.opt_text_enabled(
                        keys.line_height,
                        "1.2",
                        64.,
                        overwrite,
                        window,
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .pt_2()
                    .gap_1()
                    .child(self.check_if(overwrite, keys.ligatures, "Enable ligatures", false, cx))
                    .child(crate::settings_window::help_mark(
                        "scheme-font-ligatures-help",
                        "Use several characters as a single ligature. Ligatures enhance code readability. Use the section below to preview the changes",
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .pt_2()
                    .gap_1()
                    .text_color(tokens.text_muted)
                    .child("See line height and ligatures also in")
                    .child(nav_link("scheme-font-reader-mode", "Reader mode", "Editor/Reader Mode", cx)),
            );
        let fallback = v_flex()
            .gap_1()
            .child(labeled(
                "Fallback font:",
                self.combo_if(overwrite, keys.fallback, &["<None>"], "<None>", 200., cx)
                    .into_any_element(),
            ))
            .child(
                div()
                    .pl(px(100.))
                    .text_xs()
                    .text_color(tokens.text_muted)
                    .child("Used for symbols not supported\nby the main font"),
            );
        let options = h_flex()
            .pl(px(INDENT))
            .items_start()
            .gap_6()
            .when(!overwrite, |options| {
                options.text_color(tokens.text_disabled)
            })
            .child(font)
            .child(fallback);
        let dark = self.dark_scheme(cx);
        v_flex()
            .gap_3()
            .child(self.scheme_row(cx))
            .child(
                h_flex()
                    .gap_1()
                    .child(self.opt_check(keys.overwrite, keys.overwrite_label, false, cx))
                    .child(nav_link("font-default", "default", "Editor/Font", cx))
                    .child(
                        div()
                            .text_color(tokens.text_muted)
                            .child("(JetBrains Mono, 13)"),
                    ),
            )
            .child(options)
            .child(preview(
                FONT_PREVIEW,
                dark,
                overwrite && self.flag(keys.ligatures, false),
                cx,
            ))
            .child(crate::settings_window::comment(
                "Enter any text to preview",
                cx,
            ))
    }

    /// A color page of `ColorAndFontOptions`: the attributes down the left, the selected one's
    /// options on the right, the page's sample text below, in the scheme's colors.
    fn color_page(
        &self,
        page: &'static str,
        data: &'static str,
        sample: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let dark = self.dark_scheme(cx);
        let tokens = Tokens::get(cx);
        let items = attributes(data);
        let first = (page == DIFF_PAGE).then_some(0);
        let selected = SELECTED.with_borrow(|selected| *selected.get(page).unwrap_or(&first));
        let open = OPEN_GROUPS.with_borrow(|open| match open.get(page) {
            Some(open) => open.clone(),
            None => tree_rows(&items)
                .into_iter()
                .filter(|row| page == DIFF_PAGE && row.0 == 0 && row.2.is_none())
                .take(1)
                .map(|row| row.3)
                .collect(),
        });
        let rows = tree_rows(&items)
            .into_iter()
            // a row shows while every group above it is open
            .filter(|(depth, _, _, path)| {
                let parts: Vec<&str> = path.split("//").collect();
                (1..=*depth).all(|up| {
                    parts
                        .get(..up)
                        .is_some_and(|above| open.contains(&above.join("//")))
                })
            })
            .map(|(depth, label, attribute, path)| {
                let chosen = attribute.is_some() && attribute == selected;
                let expanded = open.contains(&path);
                h_flex()
                    .id(SharedString::from(format!(
                        "attribute-{page}-{depth}-{label}"
                    )))
                    .role(Role::TreeItem)
                    .aria_label(label)
                    .h(px(metrics::ROW_HEIGHT))
                    .pl(px(4. + depth as f32 * 16.))
                    .pr_2()
                    .gap_1()
                    .flex_none()
                    .cursor_pointer()
                    .child(div().size(px(metrics::TOOLBAR_ICON)).flex_none().when(
                        attribute.is_none(),
                        |chevron| {
                            chevron.child(
                                spork_workbench::icon(match expanded {
                                    true => "general/chevronDown",
                                    false => "general/chevronRight",
                                })
                                .size(px(metrics::TOOLBAR_ICON))
                                .text_color(tokens.icon),
                            )
                        },
                    ))
                    .when(chosen, |row| row.bg(tokens.selection))
                    .when(!chosen, |row| {
                        row.hover(move |row| row.bg(tokens.selection_hover))
                    })
                    .child(div().truncate().child(label))
                    .on_click(cx.listener({
                        let open = open.clone();
                        move |_, _, _, cx| {
                            match attribute {
                                Some(ix) => SELECTED.with_borrow_mut(|selected| {
                                    selected.insert(page, Some(ix));
                                }),
                                None => OPEN_GROUPS.with_borrow_mut(|groups| {
                                    let mut open = open.clone();
                                    if !open.remove(&path) {
                                        open.insert(path.clone());
                                    }
                                    groups.insert(page, open);
                                }),
                            }
                            cx.notify();
                        }
                    }))
            });
        let tree = div()
            .id(SharedString::from(format!("attributes-{page}")))
            .w(px(COLUMN))
            .h(px(300.))
            .flex_none()
            .border_1()
            .border_color(tokens.control_border)
            .bg(tokens.control_bg)
            .overflow_y_scrollbar()
            .child(v_flex().children(rows));
        let options = match selected.and_then(|ix| items.get(ix)) {
            Some(attribute) if page == DIFF_PAGE => diff_options(attribute, dark, cx),
            Some(attribute) => attribute_options(attribute, dark, cx),
            None if GREYED_OPTIONS.contains(&page) => attribute_options(
                &Attribute {
                    name: "",
                    kind: Kind::Attributes,
                    dark: "inherit",
                    light: "inherit",
                    parent: None,
                },
                dark,
                cx,
            ),
            None => v_flex(),
        };
        v_flex()
            .gap_3()
            .child(self.scheme_row(cx))
            .child(
                h_flex()
                    .items_start()
                    .gap_4()
                    .child(tree)
                    .child(options.flex_1()),
            )
            .when(page == "By Scope", |content| {
                content.child(self.opt_button("manage-scopes", "Manage Scopes…", cx))
            })
            .when(!sample.is_empty(), |content| {
                content.child(preview(sample, dark, false, cx))
            })
    }
}

/// Code Completion itself: what it is, and its two pages as links.
fn code_completion_page() -> Div {
    v_flex().child(
        "Provides code suggestions while typing, displayed either in a popup or inline in the editor",
    )
}

/// Inline completion without a plugin that provides it, as in the reference app, whose list names its
/// vendor's plugins: Spork leaves them out.
fn inline_completion_page(cx: &App) -> Div {
    let tokens = Tokens::get(cx);
    h_flex()
        .items_start()
        .gap_2()
        .child(
            spork_workbench::icon("general/balloon")
                .size(px(metrics::TOOLBAR_ICON))
                .text_color(tokens.icon),
        )
        .child(
            v_flex()
                .gap_1()
                .child("You do not have any inline code completion plugins enabled.")
                .child(nav_link("inline-plugins", "Plugins", "Plugins", cx)),
        )
}

/// A link to the Settings page at `target`.
fn nav_link(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    target: impl Into<String>,
    cx: &App,
) -> Stateful<Div> {
    let target = target.into();
    spork_workbench::controls::link(id, label, cx)
        .on_click(move |_, window, cx| crate::settings_window::show_page(&target, window, cx))
}

/// The scheme IntelliJ uses until one is picked: the Islands theme's own.
fn default_scheme(cx: &App) -> &'static str {
    match cx.theme().mode.is_dark() {
        true => "Islands Dark",
        false => "Light",
    }
}

/// The editor color scheme in use, which Quick Switch Scheme's Editor Color Scheme ticks.
pub(crate) fn current_scheme(cx: &App) -> String {
    crate::settings_window::Store::load()
        .get(SCHEME_KEY)
        .map_or_else(|| default_scheme(cx).to_owned(), str::to_owned)
}

/// Quick Switch Scheme's pick of an editor color scheme.
pub(crate) fn set_scheme(name: &str) {
    crate::settings_window::Store::save_one(SCHEME_KEY, name);
}

/// An icon button for an action Spork doesn't have yet: a click says so, as `opt_button`.
fn todo_action(
    id: &'static str,
    icon: &'static str,
    tooltip: &'static str,
) -> spork_workbench::ActionButton {
    spork_workbench::ActionButton::new(id, icon)
        .tooltip(tooltip)
        .on_click(move |_, window, cx| {
            window.push_notification(
                Notification::info(format!("{tooltip} isn't in Spork yet.")),
                cx,
            )
        })
}

/// A menu item for an action Spork doesn't have yet: a click says so.
fn todo_item(label: &'static str) -> PopupMenuItem {
    PopupMenuItem::new(label).on_click(move |_, window, cx| {
        window.push_notification(
            Notification::info(format!(
                "{} isn't in Spork yet.",
                label.trim_end_matches('…')
            )),
            cx,
        )
    })
}

/// IntelliJ's `AddDeleteListPanel`: a title, an empty list saying so, add and remove buttons.
fn rule_list(
    (add, remove): (&'static str, &'static str),
    title: &'static str,
    empty: &'static str,
    cx: &App,
) -> Div {
    let tokens = Tokens::get(cx);
    v_flex().gap_1().child(title).child(
        v_flex()
            .border_1()
            .border_color(tokens.control_border)
            .child(
                h_flex()
                    .p_1()
                    .gap_1()
                    .border_b_1()
                    .border_color(tokens.control_border)
                    // nothing to remove or edit while the list is empty
                    .child(todo_action(add, "general/add", "Add"))
                    .child(todo_action(remove, "general/remove", "Remove").disabled(true))
                    .child(
                        spork_workbench::ActionButton::new(
                            SharedString::from(format!("{add}-edit")),
                            "general/edit",
                        )
                        .tooltip("Edit")
                        .disabled(true),
                    ),
            )
            .child(
                div()
                    .h(px(96.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(tokens.control_bg)
                    .text_color(tokens.text_muted)
                    .child(empty),
            ),
    )
}

/// The selected attribute's options, as `ColorAndFontDescriptionPanel`: font style, colors,
/// effects and what it inherits. They show the scheme's values; Spork doesn't edit schemes.
fn attribute_options(attribute: &Attribute, dark: bool, cx: &App) -> Div {
    let tokens = Tokens::get(cx);
    let look = look(
        &attribute.kind,
        if dark {
            attribute.dark
        } else {
            attribute.light
        },
    );
    let enabled = !look.inherited;
    let row = |id: &'static str, label: &'static str, color: Option<Hsla>| {
        h_flex()
            .gap_2()
            .child(
                div()
                    .w(px(150.))
                    .child(shown_check(id, label, color.is_some(), enabled, cx)),
            )
            .child(swatch(color, cx))
    };
    match attribute.kind {
        Kind::Semantic => {
            return v_flex().child(shown_check(
                "semantic-highlighting",
                "Semantic highlighting",
                false,
                true,
                cx,
            ));
        }
        Kind::Foreground => {
            return v_flex().child(row("color-foreground", "Foreground", look.foreground));
        }
        Kind::Background => {
            return v_flex().child(row("color-background", "Background", look.background));
        }
        Kind::Attributes => {}
    }
    let effect = look.effect.map(|(name, _)| name);
    let effects = effect.filter(|_| enabled).map(|current| {
        move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
            EFFECT_CHOICES.iter().fold(menu, |menu, choice| {
                menu.item(PopupMenuItem::new(*choice).checked(*choice == current))
            })
        }
    });
    v_flex()
        .gap_2()
        .child(
            h_flex()
                .gap_4()
                .child(
                    div()
                        .w(px(40.))
                        .text_color(match enabled {
                            true => tokens.text,
                            false => tokens.text_disabled,
                        })
                        .child("Font"),
                )
                .child(shown_check("font-bold", "Bold", look.bold, enabled, cx))
                .child(shown_check(
                    "font-italic",
                    "Italic",
                    look.italic,
                    enabled,
                    cx,
                )),
        )
        .child(row("attribute-foreground", "Foreground", look.foreground))
        .child(row("attribute-background", "Background", look.background))
        .child(row("attribute-stripe", "Error stripe mark", look.stripe))
        .child(row(
            "attribute-effects",
            "Effects",
            look.effect.map(|(_, color)| color),
        ))
        .child(div().pl(px(INDENT)).child(match effects {
            Some(menu) => spork_workbench::controls::combo(
                "attribute-effect",
                effect.unwrap_or("Underscored"),
                false,
                menu,
            ),
            None => spork_workbench::controls::combo(
                "attribute-effect",
                effect.unwrap_or("Underscored"),
                true,
                |menu, _, _| menu,
            ),
        }))
        .when_some(attribute.parent, |options, (page, name)| {
            options
                .child(shown_check(
                    "attribute-inherit",
                    "Inherit values from:",
                    look.inherited,
                    true,
                    cx,
                ))
                .child(
                    v_flex()
                        .pl(px(INDENT))
                        .child(nav_link(
                            "attribute-parent",
                            name.replace("//", " → "),
                            format!("Editor/Color Scheme/{page}"),
                            cx,
                        ))
                        .child(
                            div()
                                .text_color(tokens.text_muted)
                                .child(format!("({page})")),
                        ),
                )
        })
}

/// A checkbox that shows a value without changing it.
fn shown_check(
    id: &'static str,
    label: &'static str,
    on: bool,
    enabled: bool,
    cx: &App,
) -> AnyElement {
    match enabled {
        true => spork_workbench::controls::labeled_check(id, label, on, cx)
            .cursor_default()
            .into_any_element(),
        false => spork_workbench::controls::disabled_check(id, label, on, cx).into_any_element(),
    }
}

/// Diff & Merge's options for a color, as `DiffColorsPage`'s: the important and ignored
/// backgrounds and the stripe, the ignored one inherited.
fn diff_options(attribute: &Attribute, dark: bool, cx: &App) -> Div {
    let look = look(
        &attribute.kind,
        match dark {
            true => attribute.dark,
            false => attribute.light,
        },
    );
    let row = |label: &'static str, color: Option<Hsla>| {
        h_flex()
            .gap_2()
            .child(div().w(px(150.)).child(label))
            .child(swatch(color, cx))
    };
    v_flex()
        .gap_2()
        .child(row("Important", look.background))
        .child(row("Ignored", None))
        .child(row("Error stripe mark", look.stripe))
        .child(shown_check(
            "diff-inherit-ignored",
            "Inherit ignored color",
            true,
            true,
            cx,
        ))
}

/// IntelliJ's `ColorPanel`: the color, or an empty box without one.
fn swatch(color: Option<Hsla>, cx: &App) -> Div {
    let tokens = Tokens::get(cx);
    div()
        .w(px(metrics::BUTTON_MIN_WIDTH))
        .h(px(metrics::CONTROL_HEIGHT))
        .rounded(px(metrics::RADIUS))
        .border_1()
        .border_color(tokens.control_border)
        .when_some(color, |swatch, color| swatch.bg(color))
}

/// A page's sample text in a monospace block, in the scheme's default text colors.
/// A sample in an editor with line numbers, as IntelliJ's template previews: each line its
/// parts, the second of three the spot a template applies at, boxed.
fn numbered_sample(lines: &[&[&'static str]], cx: &App) -> Div {
    let tokens = Tokens::get(cx);
    let rows = lines.iter().enumerate().map(|(ix, parts)| {
        h_flex()
            .child(
                div()
                    .w(px(24.))
                    .flex_none()
                    .text_color(tokens.text_muted)
                    .child((ix + 1).to_string()),
            )
            .children(parts.iter().enumerate().map(|(part, text)| {
                div()
                    .when(parts.len() == 3 && part == 1, |spot| {
                        spot.border_1().border_color(tokens.text)
                    })
                    .child(*text)
            }))
    });
    v_flex()
        .p_1()
        .h(px(120.))
        .border_1()
        .border_color(tokens.control_border)
        .bg(tokens.control_bg)
        .overflow_hidden()
        .font_family(cx.theme().mono_font_family.clone())
        .children(rows)
}

/// A sample's text without its `<b>` and `</b>` marks, and the ranges they made bold.
fn bold_ranges(marked: &str) -> (String, Vec<std::ops::Range<usize>>) {
    let (mut text, mut bold, mut start) = (String::new(), Vec::new(), None);
    let mut rest = marked;
    while let Some(at) = rest.find('<') {
        text.push_str(&rest[..at]);
        rest = &rest[at..];
        if let Some(after) = rest.strip_prefix("<b>") {
            start = Some(text.len());
            rest = after;
        } else if let Some(after) = rest.strip_prefix("</b>") {
            bold.extend(start.take().map(|start| start..text.len()));
            rest = after;
        } else {
            text.push('<');
            rest = &rest[1..];
        }
    }
    text.push_str(rest);
    (text, bold)
}

/// A page's sample text in a monospace block, in the scheme's default text colors; `<b>` marks
/// its bold parts, and its ligatures show only with `ligatures`.
pub(crate) fn preview(text: &'static str, dark: bool, ligatures: bool, cx: &App) -> Div {
    let tokens = Tokens::get(cx);
    let text_look = attributes(GENERAL_COLORS)
        .into_iter()
        .find(|attribute| attribute.name == "Text//Default text")
        .map(|attribute| {
            look(
                &attribute.kind,
                if dark {
                    attribute.dark
                } else {
                    attribute.light
                },
            )
        })
        .unwrap_or_default();
    div()
        .p_2()
        .border_1()
        .border_color(tokens.control_border)
        .font_family(cx.theme().mono_font_family.clone())
        .when_some(text_look.background, |block, color| block.bg(color))
        .when_some(text_look.foreground, |block, color| block.text_color(color))
        .when(!ligatures, |block| {
            block.font_features(FontFeatures::disable_ligatures())
        })
        .child({
            let (text, bold) = bold_ranges(text.trim_matches('\n'));
            let style = HighlightStyle {
                font_weight: Some(FontWeight::BOLD),
                ..Default::default()
            };
            StyledText::new(text).with_highlights(bold.into_iter().map(|range| (range, style)))
        })
}

/// What a color page row edits: text attributes, one color, or the rainbow switch.
#[derive(Debug)]
enum Kind {
    Attributes,
    Foreground,
    Background,
    Semantic,
}

/// A color page row: its name with its groups ("Text//Default text"), its values in Islands Dark
/// and in Light, and the attribute it inherits from (page, name) when it has one.
#[derive(Debug)]
struct Attribute {
    name: &'static str,
    kind: Kind,
    dark: &'static str,
    light: &'static str,
    parent: Option<(&'static str, &'static str)>,
}

/// The rows of a page's data: name, kind, dark values, light values, then the inherited
/// attribute's page and name, separated by tabs.
fn attributes(data: &'static str) -> Vec<Attribute> {
    data.lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next().filter(|name| !name.is_empty())?;
            let kind = match fields.next()? {
                "A" => Kind::Attributes,
                "F" => Kind::Foreground,
                "B" => Kind::Background,
                "S" => Kind::Semantic,
                _ => return None,
            };
            let dark = fields.next().unwrap_or_default();
            let light = fields.next().unwrap_or_default();
            let parent = fields.next().zip(fields.next());
            Some(Attribute {
                name,
                kind,
                dark,
                light,
                parent,
            })
        })
        .collect()
}

/// An attribute's values in one scheme.
#[derive(Debug, Default)]
struct Look {
    foreground: Option<Hsla>,
    background: Option<Hsla>,
    stripe: Option<Hsla>,
    effect: Option<(&'static str, Hsla)>,
    bold: bool,
    italic: bool,
    inherited: bool,
}

/// Reads "bold italic fg:RRGGBB bg:RRGGBB stripe:RRGGBB fxN:RRGGBB inherit", or a lone color
/// for a color row.
fn look(kind: &Kind, values: &str) -> Look {
    let mut look = Look::default();
    for token in values.split(' ') {
        match token.split_once(':') {
            Some(("fg", hex)) => look.foreground = color(hex),
            Some(("bg", hex)) => look.background = color(hex),
            Some(("stripe", hex)) => look.stripe = color(hex),
            Some((effect, hex)) => {
                let name = effect
                    .strip_prefix("fx")
                    .and_then(|number| number.parse::<usize>().ok())
                    .and_then(|number| EFFECTS.get(number));
                look.effect = name.copied().zip(color(hex));
            }
            None => match token {
                "bold" => look.bold = true,
                "italic" => look.italic = true,
                "inherit" => look.inherited = true,
                "" => {}
                hex => match kind {
                    Kind::Foreground => look.foreground = color(hex),
                    Kind::Background => look.background = color(hex),
                    Kind::Attributes | Kind::Semantic => {}
                },
            },
        }
    }
    look
}

/// RRGGBB, or RRGGBBAA as some scheme colors are.
fn color(hex: &str) -> Option<Hsla> {
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(match hex.len() {
        8 => rgba(value).into(),
        6 => rgb(value).into(),
        _ => return None,
    })
}

/// The attribute tree's rows, as IntelliJ's `ColorOptionsTree` groups the sorted names: each
/// group once, before its first attribute; leaves carry their attribute's index.
/// A color page's tree: each row's depth, label, attribute (none for a group) and path.
fn tree_rows(attributes: &[Attribute]) -> Vec<(usize, &'static str, Option<usize>, String)> {
    let mut rows = Vec::new();
    let mut previous: Vec<&'static str> = Vec::new();
    for (ix, attribute) in attributes.iter().enumerate() {
        let parts: Vec<&'static str> = attribute.name.split("//").collect();
        let Some((leaf, groups)) = parts.split_last() else {
            continue;
        };
        let shared = previous
            .iter()
            .zip(groups)
            .take_while(|(old, new)| old == new)
            .count();
        for (depth, group) in groups.iter().enumerate().skip(shared) {
            rows.push((depth, *group, None, groups[..=depth].join("//")));
        }
        rows.push((groups.len(), *leaf, Some(ix), attribute.name.to_owned()));
        previous = groups.to_vec();
    }
    rows
}

/// The font pages' sample text: `FontPreviewService`'s, with the reference app's name.
pub(crate) const FONT_PREVIEW: &str = r##"
Spork is an <b>Integrated
Development Environment (IDE)</b> designed
to maximize productivity. It provides
<b>clever code completion, static code
analysis, and refactorings,</b> and lets
you focus on the bright side of
software development making
it an enjoyable experience.

Default:
abcdefghijklmnopqrstuvwxyz
ABCDEFGHIJKLMNOPQRSTUVWXYZ
0123456789 (){}[]
+ - * / = .,;:!? #&$%@|^

<b>Bold:
abcdefghijklmnopqrstuvwxyz
ABCDEFGHIJKLMNOPQRSTUVWXYZ
0123456789 (){}[]
+ - * / = .,;:!? #&$%@|^</b>

<!-- -- != := === >= >- >=> |-> -> <$>
</> #[ |||> |= ~@
"##;

// The color pages' rows, generated from the reference app's jars: names from its searchable options and
// bundles, keys from IntelliJ's page sources and the reference app's bytecode, values from its schemes
// (Islands Dark: themes/islands/IslandSchemeDark.xml over Darcula and Default; Light:
// themes/expUI/expUI_lightScheme.xml over Default, both in intellij.platform.resources.jar).
// Fields: name, kind (A attributes, F foreground, B background, S semantic switch), Islands Dark
// values, Light values, the inherited attribute's page and name.

const GENERAL_COLORS: &str = r#"
Code//Identifier under caret	A	bg:373B39 stripe:5B786A	bg:EDEBFC stripe:D0A1FF
Code//Identifier under caret (write)	A	bg:402F33 stripe:BA6387	bg:FCE8F4 stripe:F0A8D2
Code//Injected language fragment	A	bg:293C40	bg:EDFCED
Code//Line number	F	4B5059	AEB3C2
Code//Line number on caret row	F	A1A3AB	767A8A
Code//Matched brace	A	bold bg:43454A	bg:93D9D9
Code//Method separator color	F	43454A	DFE1E5
Code//TODO defaults	A	italic fg:8BB33D stripe:73AD2B	italic fg:008DDE stripe:54AAE3
Code//Unmatched brace	A	fg:F75464	bg:FFDCDC
Editor//Bookmarks	A	stripe:F7E9C6	stripe:000000
Editor//Breadcrumbs//Border	B
Editor//Breadcrumbs//Current	A	fg:DFE1E5 bg:2B2D30	fg:000000 bg:EBECF0
Editor//Breadcrumbs//Default	A	fg:9DA0A8	fg:6C707E
Editor//Breadcrumbs//Hovered	A	fg:DFE1E5 bg:2B2D30	fg:000000 bg:EBECF0
Editor//Breadcrumbs//Inactive	A	fg:6F737A	fg:A8ADBD
Editor//Caret	F	CED0D6	000000
Editor//Caret row	B	1F2024	F5F8FE
Editor//Guides//Hard wrap guide	F	323438	EBECF0
Editor//Guides//Indent guide	B	323438	EBECF0
Editor//Guides//Indent guide selected	B	4E5157	AEB3C2
Editor//Guides//Matched brace guide	B	4E5157	62B3B3
Editor//Guides//Visual guides	F	2B2D30	DAE6FD
Editor//Gutter background	B	313335	FFFFFF
Editor//Notification background	B	25324D	FFF8E3
Editor//Selection background	B	214283	A6D2FF
Editor//Selection background inactive	B	4C4F56	B5B7BD
Editor//Selection foreground	F
Editor//Sticky Lines//Background	B
Editor//Sticky Lines//Border	B
Editor//Sticky Lines//Hovered	B
Editor//Tabs//Modified icon color	F	4083C9	4083C9
Editor//Tabs//Selected Tab	A
Editor//Tabs//Selected Tab inactive	A
Editor//Tabs//Underline	B
Editor//Tabs//Underline inactive	B
Editor//Tear line	F	555555	D4D4D4
Editor//Tear line selection	F	787878	9C9C9C
Editor//Vertical Scrollbar//Thumb	B	FFFFFF26	00000033
Editor//Vertical Scrollbar//Thumb while scrolling	B	FFFFFF4D	00000080
Errors and Warnings//Deprecated symbol	A	fx3:BCBEC4	fx3:404040
Errors and Warnings//Deprecated symbol marked for removal	A	fx3:F75464	fx3:FF0000
Errors and Warnings//Duplicate from server	A	bg:5E5339	bg:F5F7F0
Errors and Warnings//Error	A	stripe:D64D5B fx2:FA6675	stripe:CF5B56 fx2:FF0000
Errors and Warnings//Problem from server	A	stripe:B06100 fx1:F49810	stripe:E69317 fx2:F49810
Errors and Warnings//Runtime problem	A	stripe:D64D5B fx5:F2C55C	stripe:CF5B56 fx5:F49810
Errors and Warnings//Unknown symbol	A	fg:F75464	fg:F50000
Errors and Warnings//Unused code	A	fg:6F737A	fg:808080 stripe:EBC700
Errors and Warnings//Warning	A	stripe:C29E4A fx2:F2C55C	bg:FCF4D4 stripe:F2BF57
Errors and Warnings//Weak Warning	A	fx2:857042	stripe:D9CFAD fx2:CCCCCC
Hyperlinks//Followed	A	fg:B189F5 fx1:B189F5	fg:8552C6 fx1:8552C6
Hyperlinks//Inactive	A	fx1:6B6C73	fx1:808080
Hyperlinks//Reference	A	fg:548AF7 fx1:548AF7	fg:006DCC fx1:006DCC
Hyperlinks//Unfollowed	A	fg:548AF7 fx1:548AF7	fg:006DCC fx1:006DCC
Identifiers//Reassigned local variable	A	fg:BCBEC4 fx1:84868C	fx1:909090
Line Coverage//Full	A	bold fg:375239	bold fg:CCFFCC
Line Coverage//Partial	A	bold fg:5E4D33	bold fg:FFFFCC
Line Coverage//Uncovered	A	bold fg:5E3838	bold fg:FFCCCC
Live Templates//Active Segment	A	fx0:467FF2	fx0:0055FF
Live Templates//Inactive Segment	A	fx0:9DA0A8	fx0:BFBFBF
Live Templates//Template Variable	A	fg:B189F5	italic fg:7F0000
Popups and Hints//Code lens	A	fx0:43454A	fx0:818594
Popups and Hints//Completion	B	27282B	FFFFFF
Popups and Hints//Documentation	B	27282B	FFFFFF
Popups and Hints//Error hint	B	402929	FFE8E8
Popups and Hints//Hint border	B	393B40
Popups and Hints//Information hint	B	2B2D30	FFFFFF
Popups and Hints//Promotion pane	B	25324D	E7EFFD
Popups and Hints//Question hint	B	25324D	F5F8FE
Popups and Hints//Recent locations selection	B	2B2D30	E7EFFD
Popups and Hints//Tooltip	B
Popups and Hints//Warning hint	B
Preview//Background	B
Preview//Border	B
Search Results//Search result	A	bg:2D543F stripe:42BD77	bg:CCCCFF stripe:8E8EB2
Search Results//Search result (write access)	A	bg:66313F stripe:FA7DB1	bg:FFCDFF stripe:B28FB2
Search Results//Text search result	A	bg:114957 stripe:72D6D6 fx0:165E70	fg:000000 bg:FCD47E stripe:C47233
Text//Background in read-only files	B
Text//Default text	A	fg:BCBEC4 bg:191A1C	fg:080808 bg:FFFFFF
Text//Deleted text	A	fg:BBBBBB bg:450505 fx3:C3C3C3	fg:C3C3C3 bg:F0F0F0
Text//Folded text	A	fg:868991 bg:393B40	fg:414D41 bg:E9F5E6
Text//Folded text with highlighting	B	2B2D30	ADD9C7
Text//Read-only fragment background	B		CFE7FF
Text//Soft wrap sign	F	C0C0C0	C0C0C0
Text//Tabs	F
Text//Whitespaces	F	6F737A	ADADAD
"#;

// The reference app's sample links to its vendor's sites: Spork's go to example.com
const GENERAL_COLORS_PREVIEW: &str = r##"
//TODO: Visit Web resources:
Home Page: http://www.example.com
Developer Community: https://www.example.com/devnet
ReferenceHyperlink
Inactive hyperlink in code: "http://example.com"

Search:
  result = "text, text, text";
  i = result
  return i;

Folded text
Folded text with highlighting
Deleted text
Live template: active inactive $VARIABLE$
Injected language: .(gif|jpg|png)$

Code Inspections:
  Error
  Warning
  Weak warning
  Deprecated symbol
  Deprecated symbol marked for removal
  Unused symbol
  Unknown symbol
  Runtime problem
  Problem from server
  Duplicate from server
"##;

const LANGUAGE_DEFAULTS: &str = r#"
Bad character	A	fg:F75464	bg:FFCCCC
Braces and Operators//Braces	A	fg:BCBEC4
Braces and Operators//Brackets	A	fg:BCBEC4
Braces and Operators//Comma	A	fg:BCBEC4
Braces and Operators//Dot	A	fg:BCBEC4
Braces and Operators//Operation sign	A	fg:BCBEC4
Braces and Operators//Parentheses	A	fg:BCBEC4
Braces and Operators//Semicolon	A	fg:BCBEC4
Classes//Class name	A
Classes//Class reference	A	fg:BCBEC4
Classes//Instance field	A	fg:C77DBB	fg:871094
Classes//Instance method	A	fg:57AAF7
Classes//Interface name	A
Classes//Static field	A	italic fg:C77DBB	italic fg:871094
Classes//Static method	A	italic fg:57AAF7	italic fg:00627A
Comments//Block comment	A	fg:7A7E85	italic fg:8C8C8C
Comments//Doc comment//Code block	A	fg:CED0D6 bg:26292C fx0:393B40	fg:000000 bg:F8F8F9 fx0:EBECF0
Comments//Doc comment//Inline code fragment	A	fg:CED0D6 bg:343539	fg:000000 bg:F3F3F4
Comments//Doc comment//Link in rendered view	F	3887A1	4585BE
Comments//Doc comment//Markup	A	fg:68A67E	bg:E2FFE2
Comments//Doc comment//Shortcut	A	fg:CED0D6 fx0:6F737A	fg:000000 fx0:A8ADBD
Comments//Doc comment//Tag	A	fg:67A37C	fx1:999999
Comments//Doc comment//Tag value	A	fg:ABADB3	italic fg:3D3D3D
Comments//Doc comment//Text	A	italic fg:5F826B	italic fg:8C8C8C
Comments//Doc comment//Vertical guide for rendered view	F	394D3F	E8E8E8
Comments//Line comment	A	fg:7A7E85	italic fg:8C8C8C
Identifiers//Constant	A	italic fg:C77DBB	italic fg:871094
Identifiers//Default	A	fg:BCBEC4	fg:000000	General	Text//Default text
Identifiers//Function call	A	inherit fg:BCBEC4
Identifiers//Function declaration	A	fg:56A8F5	fg:00627A
Identifiers//Global variable	A
Identifiers//Label	A	bold fx1:808080	fx1:808080
Identifiers//Local variable	A
Identifiers//Parameter	A
Identifiers//Predefined symbol	A	italic	italic
Identifiers//Reassigned local variable	A	fg:BCBEC4 fx1:84868C	fx1:909090
Identifiers//Reassigned parameter	A	fg:BCBEC4 fx1:84868C	fx1:909090
Inline hints//Default	A	fg:868A91 bg:393B40	fg:7A7A7A bg:EDEDED
Inline hints//Inlay Button//Default	A	fg:9DA0A8 fx0:FFFFFF33	fg:494B57 fx0:00000033
Inline hints//Inlay Button//Focused	A	fg:9DA0A8 fx0:FFFFFF52	fg:494B57 fx0:00000047
Inline hints//Inlay Button//Hovered	A	fg:9DA0A8 fx0:FFFFFF52	fg:494B57 fx0:00000047
Inline hints//Parameters//Current	A	fg:83ACFC bg:35538F	fg:5B5B5B bg:BCDAF7
Inline hints//Parameters//Default	A	fg:858A94 bg:393B40	fg:7A7A7A bg:EDEDED
Inline hints//Parameters//Highlighted	A	fg:C9CBD6 bg:43454A	fg:5B5B5B bg:CCCCCC
Inline hints//Refactoring options//Default	B	393B40	D8D8D88C
Inline hints//Refactoring options//Focused	B	393B40	B3B3B38C
Inline hints//Refactoring options//Hovered	B	393B40	B3B3B38C
Inline hints//Text without background	A	fg:727782	fg:7A7A7A
Keyword	A	fg:CF8E6D	fg:0033B3
Markup//Attribute	A	bold fg:0000FF	fg:174AD4
Markup//Entity	A	bold fg:0000FF	fg:174BE6
Markup//Tag	A	fg:BCBEC4		General	Text//Default text
Metadata	A	fg:B3AE60	fg:9E880D	General	Text//Default text
Number	A	fg:2AACB8	fg:1750EB
Semantic highlighting	S
String//Escape sequence//Invalid	A	fg:CF8E6D fx2:FA6675	fg:067D17 bg:FFCCCC
String//Escape sequence//Valid	A	fg:CF8E6D	fg:0037A6
String//Highlighted reference	A	fx1:6B6C73	fx1:808080
String//String text	A	fg:6AAB73	fg:067D17
Template language	A	bg:2B2D30	bg:F7FAFF	General	Text//Default text
"#;

const LANGUAGE_DEFAULTS_PREVIEW: &str = r##"
Bad characters: ????
Keyword
Identifier
'String \n\?'
12345
Operator
Dot: . comma: , semicolon: ;
{ Braces }
( Parentheses )
[ Brackets ]
// Line comment
/* Block comment */
:Label
predefined_symbol()
CONSTANT
Global variable
/** RENDERED DOC */
/** 
 * Doc comment
 * @tag Markup<
 * @param parameter1 documentation
 * @param parameter2 documentation
 * @param parameter3 documentation
 * @param parameter4 documentation
 */
Function declaration (parameter1 parameter2 parameter3 parameter4)
    Local variable1 variable2 variable3 variable4
    Reassigned local variable
Function call(p: 0, param: 1, parameterName: 2)
Current function call(param: 0, currentParam: 1)
Interface Name
@Metadata
Class Name
    instance method
    instance field
    static method
    static field

function("/highlighted/reference/{param}")

@TAG attribute=Value
    Entity: &amp;
    {% Template language %}
"##;

const CODE_REVIEW: &str = r#"
Editor Gutter//Lines changed in code review	B	8A4175	F8A0DF
"#;

const CONSOLE_COLORS: &str = r#"
ANSI colors//Black	A	fg:000000 bg:000000	fg:000000 bg:000000	Console Colors	Console//Standard output
ANSI colors//Blue	A	fg:3993D4 bg:245980	fg:063FDB bg:C4DBFA	Console Colors	Console//Standard output
ANSI colors//Bright Black	A	fg:595959 bg:424242	fg:656565 bg:B7B7B7
ANSI colors//Bright Blue	A	fg:1FB0FF bg:1778BD	fg:2D61F0 bg:95C2FF
ANSI colors//Bright Cyan	A	fg:00E5E5 bg:006E6E	fg:15C1C1 bg:8CF1F1
ANSI colors//Bright Green	A	fg:4FC414 bg:458500	fg:16B42C bg:85F399
ANSI colors//Bright Magenta	A	fg:ED7EED bg:B247B2	fg:E617E6 bg:FBACFB
ANSI colors//Bright Red	A	fg:FF4050 bg:B82421	fg:FF1616 bg:FE7277
ANSI colors//Bright White	A	fg:FFFFFF bg:FFFFFF	fg:C9C9C9 bg:FFFFFF	Console Colors	Console//Standard output
ANSI colors//Bright Yellow	A	fg:E5BF00 bg:A87B00	fg:ECC32C bg:FFE055
ANSI colors//Cyan	A	fg:00A3A3 bg:154F4F	fg:028E8E bg:BFEEEE	Console Colors	Console//Standard output
ANSI colors//Green	A	fg:5C962C bg:39511F	fg:067D17 bg:B5E7BE	Console Colors	Console//Standard output
ANSI colors//Magenta	A	fg:A771BF bg:5C4069	fg:B309B3 bg:F3D2F3	Console Colors	Console//Standard output
ANSI colors//Red	A	fg:F0524F bg:772E2C	fg:CE0505 bg:FCA4A7	Console Colors	Console//Standard output
ANSI colors//White (Gray)	A	fg:808080 bg:616161	fg:929292 bg:E6E6E6	Console Colors	Console//Standard output
ANSI colors//Yellow	A	fg:A68A0D bg:5C4F17	fg:B28C00 bg:F7E79F	Console Colors	Console//Standard output
Console//Background	B	191A1C	FFFFFF
Console//Error output	A	fg:F75464	fg:7F0000
Console//Standard output	A	fg:BCBEC4	fg:000000
Console//System output	A	fg:BCBEC4	fg:00007F
Console//User input	A	italic fg:6AAB73	italic fg:007F00
Log console//Debug	A	fg:299999	fg:00CCCC
Log console//Error	A	fg:F75464	fg:CD0000
Log console//Expired entry	A	fg:555555	fg:555555
Log console//Info	A	fg:E0BB65	fg:00CD00
Log console//Verbose	A	fg:56A8F5	fg:0000EE
Log console//Warning	A		fg:A66F00
Reworked terminal//Black	A	fg:000000 bg:1E1F22	fg:000000 bg:000000
Reworked terminal//Block background gradient end	B	212329	FAFAFA
Reworked terminal//Block background gradient start	B	2B2D30	F0F2F5
Reworked terminal//Block hovered background gradient end	B	FFFFFF03	00000003
Reworked terminal//Block hovered background gradient start	B	FFFFFF0A	0000000A
Reworked terminal//Blue	A	fg:5594FA bg:134EBF	fg:225CD6 bg:0F5ED4
Reworked terminal//Bright Black	A	fg:4E5157 bg:4E5157	fg:6C707E bg:A8ADBD
Reworked terminal//Bright Blue	A	fg:3399FF bg:1AAFFF	fg:009DFF bg:4DACFF
Reworked terminal//Bright Cyan	A	fg:40FFE9 bg:22E5CF	fg:31C4B3 bg:2EDCE5
Reworked terminal//Bright Green	A	fg:67FF59 bg:48E539	fg:3DCC3D bg:4FE34F
Reworked terminal//Bright Magenta	A	fg:D970FF bg:B459FF	fg:D659FF bg:EC73FF
Reworked terminal//Bright Red	A	fg:FF6B7A bg:FF475A	fg:FF5462 bg:FA4352
Reworked terminal//Bright White	A	fg:FFFFFF bg:FFFFFF	fg:D2D4D9 bg:FFFFFF
Reworked terminal//Bright Yellow	A	fg:FFEC1A bg:FFBF00	fg:D4BB00 bg:FFCD1A
Reworked terminal//Command	A	bold fg:FFFFFF	bold fg:000000
Reworked terminal//Current search entry	A	fg:000000 bg:FF941A	fg:000000 bg:FF941A
Reworked terminal//Cyan	A	fg:47CCBD bg:008576	fg:007A80 bg:0AA5AB
Reworked terminal//Default background	B
Reworked terminal//Default foreground	F
Reworked terminal//Error block border	F	DB5C5C	DB3B4B
Reworked terminal//Generate command caret color	F	955AE0	834DF0
Reworked terminal//Generate command placeholder foreground	F	6C469C	BFA1F8
Reworked terminal//Generate command prompt text	A	fg:B589EC	fg:7444D4
Reworked terminal//Green	A	fg:6BCC62 bg:288720	fg:357D1F bg:3DA154
Reworked terminal//Inactive selected block background	B	393B40	EBECF0
Reworked terminal//Inactive selected block border	F	5A5D63	C9CCD6
Reworked terminal//Magenta	A	fg:C092FA bg:843AE0	fg:6E32E5 bg:A95EFF
Reworked terminal//Prompt separator color	F	393B40	EBECF0
Reworked terminal//Red	A	fg:F27481 bg:BA3F4B	fg:C93645 bg:D43B48
Reworked terminal//Search entry	A	fg:000000 bg:FFEA00	fg:000000 bg:FFEA00
Reworked terminal//Selected block background	B	25324D	EDF3FF
Reworked terminal//Selected block border	F	3574F0	4682FA
Reworked terminal//White	A	fg:CED0D6 bg:CED0D6	fg:BEC0C5 bg:DCDEE3
Reworked terminal//Yellow	A	fg:E0CE70 bg:A66900	fg:B2A400 bg:BD8317
Terminal//Command to run using IDE	A	bg:40503C	bg:CFEFC6
"#;

const CONSOLE_COLORS_PREVIEW: &str = r##"
C:\command.com
- C:>
- help
Bad command or file name

Log error
Log warning
Log info
Log verbose
Log debug
An expired log entry

# Process output highlighted using ANSI colors codes
ANSI: black
ANSI: red
ANSI: green
ANSI: yellow
ANSI: blue
ANSI: magenta
ANSI: cyan
ANSI: gray
ANSI: dark gray
ANSI: bright red
ANSI: bright green
ANSI: bright yellow
ANSI: bright blue
ANSI: bright magenta
ANSI: bright cyan
ANSI: white

git log
Process finished with exit code 1
"##;

const DEBUGGER: &str = r#"
Breakpoint line	A	bg:40252B stripe:8C5B65	bg:FAEAE6 stripe:FFC8C8
Evaluated expression text	A	bg:424242	bg:E5E5E5
Evaluated expression text for execution line	A	bg:474747	bg:BFBFBF
Execution point	A	bg:2A5091	fg:FFFFFF bg:2154A6 stripe:2154A6
Inline stack frames	A	bg:534A57	bg:E6E0F1
Inlined modified values	A	italic fg:B2AE60	italic fg:CA8021
Inlined values	A	italic fg:666D75	italic fg:868686
Inlined values for execution line	A	italic fg:849FBF	italic fg:A9B7D6
Not top frame	A	bg:273552	bg:C0D0F0
Smart step into selection	A	bg:824991 fx0:C76DC7	fg:000000 bg:F2B3FF fx0:CC29CC	General	Live Templates//Active Segment
Smart step into target	A	bg:53385E	fg:000000 bg:E2D3F0	General	Search Results//Search result
"#;

const DIFF_MERGE: &str = r#"
Changed lines//Changed	A	bg:385570 stripe:43698D	bg:C2D8F2 stripe:B6D2F2
Changed lines//Conflict	A	bg:45302B stripe:8F5247	bg:FFD5CC stripe:FFC8BD
Changed lines//Deleted	A	bg:484A4A stripe:656E76	bg:D6D6D6 stripe:C8C8C8
Changed lines//Inserted	A	bg:294436 stripe:447152	bg:BEE6BE stripe:AADEAA
Folded unchanged fragments//Wave	F	555555	D0D0D0
"#;

const USER_DEFINED: &str = r#"
Block comment	A	italic fg:808080	italic fg:808080	Language Defaults	Comments//Block comment
Invalid string escape	A	fg:008000 bg:FFCCCC	fg:008000 bg:FFCCCC	Language Defaults	String//Escape sequence//Invalid
Keyword1	A	fg:CF8E6D	inherit fg:0033B3	Language Defaults	Keyword
Keyword2	A	fg:C77DBB	fg:74118C
Keyword3	A	fg:0DA19E	fg:008080
Keyword4	A	fg:B3AE60	fg:800000
Line comment	A	italic fg:808080	italic fg:808080	Language Defaults	Comments//Line comment
Number	A	fg:0000FF	inherit fg:1750EB	Language Defaults	Number
String	A	inherit fg:6AAB73	inherit fg:067D17	Language Defaults	String//String text
Valid string escape	A	inherit fg:CF8E6D	bold fg:000080	Language Defaults	String//Escape sequence//Valid
"#;

const USER_DEFINED_PREVIEW: &str = r##"
# Line comment
aKeyword1 variable = 123;
anotherKeyword1 someString = "SomeString";
aKeyword2 variable = 123;
anotherKeyword2 someString = "SomeString";
aKeyword3 variable = 123;
anotherKeyword3 someString = "SomeString";
aKeyword4 variable = 123;
anotherKeyword4 someString = "SomeString \n\x  \& \g ";
/* 
 * Block comment
 */
"##;

const VCS: &str = r#"
Editor Gutter//Added ignored lines border	B	549159	7FC784
Editor Gutter//Added lines	B	549159	7FC784
Editor Gutter//Border	B
Editor Gutter//Changed lines popup	B
Editor Gutter//Deleted ignored lines border	B	868A91	767A8A
Editor Gutter//Deleted lines	B	868A91	767A8A
Editor Gutter//Modified ignored lines border	B	375FAD	88ADF7
Editor Gutter//Modified lines	B	375FAD	88ADF7
Editor Gutter//Whitespace-modified lines	B	52433D	F7E2CB
VCS Annotations//Background color #1	B	2F5194	A1BEF7
VCS Annotations//Background color #2	B	2D416B	C9DAFB
VCS Annotations//Background color #3	B	283754	DEE8FC
VCS Annotations//Background color #4	B	242D42	EDF2FA
VCS Annotations//Background color #5	B	22252E	F7F8FA
VCS Annotations//Foreground	F	8D9199	5A5D6B
VCS Annotations//Foreground for last commit	F	CED0D6	000000
"#;

const HTML: &str = r#"
Attribute name	A	fg:BABABA	inherit fg:174AD4	Language Defaults	Markup//Attribute
Attribute value	A	inherit fg:6AAB73	inherit fg:067D17	Language Defaults	String//String text
Comment	A	inherit fg:7A7E85	inherit italic fg:8C8C8C	Language Defaults	Comments//Line comment
Custom Tag Name	A	fg:2FBAA3
Entity reference	A	fg:56A8F5	inherit fg:174BE6	Language Defaults	Markup//Entity
HTML code	A	inherit fg:BCBEC4 bg:191A1C	inherit fg:080808 bg:FFFFFF	General	Text//Default text
Injected Language Fragment	A	inherit bg:293C40	inherit bg:EDFCED	General	Code//Injected language fragment
Tag	A	fg:D5B778	inherit	Language Defaults	Markup//Tag
Tag name	A	fg:D5B778	inherit fg:0033B3	Language Defaults	Keyword
Tag tree (level 1)	B
Tag tree (level 2)	B
Tag tree (level 3)	B
Tag tree (level 4)	B
Tag tree (level 5)	B
Tag tree (level 6)	B
"#;

const HTML_PREVIEW: &str = r##"
<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 3.2//EN">
<!--
*        Sample comment
-->
<HTML>
<head>
<title>Spork</title>
</head>
<body>
<h1>Spork</h1>
<p><br><b><IMG border=0 height=12 src="images/hg.gif" width=18 >
What is Spork? &#x00B7; &Alpha; </b><br><br>
<custom-tag>hello</custom_tag>
</body>
</html>
"##;

const REGEXP: &str = r#"
Bad character	A	inherit fg:F75464	inherit bg:FFCCCC	Language Defaults	Bad character
Brace	A	inherit fg:BCBEC4	inherit	Language Defaults	Braces and Operators//Braces
Bracket	A	inherit fg:BCBEC4	inherit	Language Defaults	Braces and Operators//Brackets
Character class	A	bold fg:B2AE60	inherit fg:174BE6	Language Defaults	Markup//Entity
Comma	A	inherit fg:BCBEC4	inherit	Language Defaults	Braces and Operators//Comma
Comment	A	inherit fg:7A7E85	inherit italic fg:8C8C8C	Language Defaults	Comments//Line comment
Dot	A	inherit fg:BCBEC4	inherit	Language Defaults	Braces and Operators//Dot
Escaped character	A	inherit fg:CF8E6D	inherit fg:0037A6	Language Defaults	String//Escape sequence//Valid
Inline option	A	inherit italic	inherit italic	Language Defaults	Identifiers//Predefined symbol
Invalid escape sequence	A	inherit fg:CF8E6D fx2:FA6675	inherit fg:067D17 bg:FFCCCC	Language Defaults	String//Escape sequence//Invalid
Matched groups	A	bg:6B6B6B	bg:D8D8D8
Name	A	inherit fg:BCBEC4	inherit fg:000000	Language Defaults	Identifiers//Default
Operator character	A	inherit fg:CF8E6D	inherit fg:0033B3	Language Defaults	Keyword
Parenthesis	A	inherit fg:BCBEC4	inherit	Language Defaults	Braces and Operators//Parentheses
Plain character	A	inherit fg:6AAB73	inherit fg:067D17	Language Defaults	String//String text
Quantifier	A	inherit fg:2AACB8	inherit fg:1750EB	Language Defaults	Number
Quote escape	A	inherit fg:CF8E6D	inherit fg:0037A6	Language Defaults	String//Escape sequence//Valid
Redundant escape sequence	A	bold fg:6F7587	inherit fg:0037A6	Language Defaults	String//Escape sequence//Valid
"#;

const REGEXP_PREVIEW: &str = r##"
^[\w\.-]+@([\w\-]+|\.)+[A-Z0-9]{2,4}(?x)
\x0g\#\p{Alpha}\1(?#comment)
.*\Q...\E$# end-of-line comment
"##;

const XML: &str = r#"
Attribute Name	A	fg:BCBEC4	inherit fg:174AD4	Language Defaults	Markup//Attribute
Attribute Value	A	inherit fg:6AAB73	inherit fg:067D17	Language Defaults	String//String text
Comment	A	inherit fg:7A7E85	inherit italic fg:8C8C8C	Language Defaults	Comments//Line comment
Custom Tag Name	A	fg:2FBAA3	fg:008077
Entity Reference	A	fg:56A8F5	inherit fg:174BE6	Language Defaults	Markup//Entity
Injected Language Fragment	A	inherit bg:293C40	inherit bg:EDFCED	General	Code//Injected language fragment
Matched Tag	A	bg:3F4045	bg:C9ECEC	General	Code//Matched brace
Namespace Prefix	A	inherit fg:C77DBB	inherit fg:871094	Language Defaults	Classes//Instance field
Prologue	A	fg:D5B778	italic	General	Text//Default text
Tag	A	fg:D5B778	inherit	Language Defaults	Markup//Tag
Tag Data	A		inherit fg:080808 bg:FFFFFF	General	Text//Default text
Tag Name	A	fg:D5B778	inherit fg:0033B3	Language Defaults	Keyword
"#;

const XML_PREVIEW: &str = r##"
<?xml version='1.0' encoding='ISO-8859-1'  ?>
<!DOCTYPE index>
<!-- Some xml example -->
<index version="1.0" xmlns:pf="http://test">
   <name>Main Index</name>
   <indexitem text="rename" target="refactoring.rename"/>
   <indexitem text="move" target="refactoring.move"/>
   <indexitem text="migrate" target="refactoring.migrate"/>
   <indexitem text="usage search" target="find.findUsages"/>
   <<matched>indexitem</matched>>Matched tag name</<matched>indexitem</matched>>
   <someTextWithEntityRefs>&amp; &#x00B7;</someTextWithEntityRefs>
   <withCData><![CDATA[
          <object class="MyClass" key="constant">
          </object>
        ]]>
   </withCData>
   <indexitem text="project" target="project.management"/>
   <custom-tag>hello</custom-tag>
   <pf:foo pf:bar="bar"/>
</index>
"##;

const IMAGES: &str = r#"
'Black' cell	B
'White' cell	B
Background	B
Grid line	B
"#;

#[cfg(test)]
mod tests {
    use super::{COLOR_PAGES, Kind, attributes, bold_ranges, color, look, tree_rows};

    #[test]
    fn bold_marks_become_ranges() {
        let (text, bold) = bold_ranges("a <b>bc</b> d <x> <b>e</b>");
        assert_eq!(text, "a bc d <x> e");
        assert_eq!(bold, vec![2..4, 11..12]);
    }

    #[test]
    fn every_row_reads() {
        for (page, data, _) in COLOR_PAGES {
            let lines = data.lines().filter(|line| !line.is_empty()).count();
            assert_eq!(attributes(data).len(), lines, "{page}");
            for attribute in attributes(data) {
                for values in [attribute.dark, attribute.light] {
                    for token in values.split(' ').filter(|token| token.contains(':')) {
                        let hex = token.split_once(':').map_or("", |(_, hex)| hex);
                        assert!(color(hex).is_some(), "{page}: {} {token}", attribute.name);
                    }
                }
            }
        }
    }

    #[test]
    fn looks_and_groups() {
        let look = look(&Kind::Attributes, "bold fg:BCBEC4 fx2:FA6675 inherit");
        assert!(look.bold && look.inherited && look.foreground.is_some());
        assert_eq!(look.effect.map(|(name, _)| name), Some("Underwaved"));
        let rows = tree_rows(&attributes("A//B//x\tA\nA//B//y\tA\nA//z\tA\nw\tA"));
        let labels: Vec<_> = rows
            .iter()
            .map(|(depth, label, ..)| (*depth, *label))
            .collect();
        assert_eq!(
            labels,
            [(0, "A"), (1, "B"), (2, "x"), (2, "y"), (1, "z"), (0, "w")]
        );
    }
}
