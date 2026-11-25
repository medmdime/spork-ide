//! Settings ▸ Editor: Code Style and its pages, Inspections, templates, encodings, file types,
//! inlay hints, Emmet, intentions, Reader Mode, TextMate bundles.

use std::cell::RefCell;
use std::collections::HashMap;

use gpui_kit::component::input::InputState;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_workbench::tokens::metrics;
use spork_workbench::{ActionButton, Tokens};

use crate::notices::Notification;
use crate::settings_window::{Node, SettingsWindow, comment, labeled, titled};

/// The pages under Editor ▸ Code Style: the reference app's code style providers (`langCodeStyleSettingsProvider`
/// HTML and XML, `codeStyleSettingsProvider` Other File Types), ordered as IntelliJ's
/// `DisplayPriority`, then by name; the General provider is Code Style's own page.
pub(crate) const CODE_STYLE: &[Node] = &[
    Node::page("HTML").words("indent tab wrap attributes quote arrangement comment"),
    Node::page("XML").words("indent tab wrap attributes cdata arrangement comment"),
    Node::page("Other File Types").words("text indent tab size"),
];

impl SettingsWindow {
    /// The page at `path` when it is one of these, else None.
    pub(crate) fn code_settings(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        Some(match path {
            "Editor/Code Style" => self.code_style_page(window, cx),
            "Editor/Code Style/HTML" => self.html_style_page(window, cx),
            "Editor/Code Style/XML" => self.xml_style_page(window, cx),
            "Editor/Code Style/Other File Types" => self.other_types_style_page(window, cx),
            "Editor/Inspections" => self.inspections_page(window, cx),
            "Editor/File and Code Templates" => self.file_templates_page(cx),
            "Editor/File Encodings" => self.file_encodings_page(cx),
            "Editor/Live Templates" => self.live_templates_page(cx),
            "Editor/File Types" => self.file_types_page(cx),
            "Editor/Inlay Hints" => self.inlay_hints_page(cx),
            "Editor/Emmet" => self.emmet_page(window, cx),
            "Editor/Intentions" => self.intentions_page(window, cx),
            "Editor/Reader Mode" => self.reader_mode_page(cx),
            "Editor/TextMate Bundles" => self.textmate_page(cx),
            _ => return None,
        })
    }

    /// IntelliJ's schemes panel: `label`, the combo of the IDE's and the project's schemes,
    /// and the gear with the scheme actions.
    fn code_scheme_row(
        &self,
        label: &'static str,
        key: &'static str,
        choices: &'static [&'static str],
        default: &'static str,
        actions: &'static [&'static str],
        cx: &mut Context<Self>,
    ) -> Div {
        h_flex()
            .gap_2()
            .child(labeled(
                label,
                self.opt_combo(key, choices, default, 220., cx)
                    .into_any_element(),
            ))
            .child(gear(SharedString::from(format!("{key}-gear")), actions))
    }

    // ---- Code Style ----------------------------------------------------------------------

    fn code_style_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let scheme = self.code_scheme_row(
            "Scheme:",
            "codeStyle.scheme",
            CODE_STYLE_SCHEMES,
            "Default",
            SCHEME_ACTIONS,
            cx,
        );
        let (tab_row, tab) = tabs("codeStyle.tab", &["General", "Formatter"], &[], cx);
        let content = match tab {
            "Formatter" => self.formatter_tab(window, cx),
            _ => self.general_style_tab(window, cx),
        };
        v_flex()
            .gap_3()
            .child(scheme)
            .children(self.indents_banner(cx))
            .child(tab_row)
            .child(content.pl(px(10.)))
    }

    /// IntelliJ's warning over a code style page while indents are detected from the file.
    fn indents_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        self.flag("codeStyle.detectIndents", true).then(|| {
            h_flex()
                .gap_1()
                .child(spork_workbench::image("status/warning", cx).size(px(16.)))
                .child("Settings may be overridden by Indents Detection")
                .child(
                    spork_workbench::controls::link("code-style-indents-disable", "Disable", cx)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_value("codeStyle.detectIndents", "false");
                            cx.notify();
                        })),
                )
        })
    }

    fn general_style_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let hard_wrap = self.opt_text("codeStyle.hardWrapAt", "120", 64., window, cx);
        let guides = self.opt_text("codeStyle.visualGuides", "", 120., window, cx);
        v_flex()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(labeled(
                        "Line separator:",
                        self.opt_combo(
                            "codeStyle.lineSeparator",
                            LINE_SEPARATORS,
                            "System-Dependent",
                            220.,
                            cx,
                        )
                        .into_any_element(),
                    ))
                    .child(comment("Applied to new files", cx)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child("Hard wrap at:")
                    .child(hard_wrap)
                    .child("columns")
                    .child(self.opt_check("codeStyle.wrapOnTyping", "Wrap on typing", false, cx)),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .child("Visual guides:")
                            .child(guides)
                            .child("columns"),
                    )
                    .child(comment("Specify one guide (80) or several (80, 120)", cx)),
            )
            .child(self.opt_check(
                "codeStyle.detectIndents",
                "Detect and use existing file indents for editing",
                true,
                cx,
            ))
    }

    fn formatter_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let patterns = self.opt_text("codeStyle.doNotFormat", "", 360., window, cx);
        let off = self.opt_text("codeStyle.formatterOff", "@formatter:off", 200., window, cx);
        let on = self.opt_text("codeStyle.formatterOn", "@formatter:on", 200., window, cx);
        v_flex()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(h_flex().gap_2().child("Do not format:").child(patterns))
                    .child(comment(
                        "Specify file name patterns and directories with a glob pattern",
                        cx,
                    )),
            )
            .child(self.opt_check(
                "codeStyle.formatterMarkers",
                "Turn formatter on/off with markers in code comments",
                false,
                cx,
            ))
            .child(
                v_flex()
                    .pl(px(20.))
                    .gap_2()
                    .child(form_row("Off:", 40., off))
                    .child(form_row("On:", 40., on))
                    .child(self.opt_check(
                        "codeStyle.formatterRegexp",
                        "Enable regular expressions in formatter markers",
                        false,
                        cx,
                    )),
            )
    }

    /// A language page's top: IntelliJ's "Set from…" link over the schemes panel.
    fn language_style_header(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .gap_1()
            .child(self.code_scheme_row(
                "Scheme:",
                "codeStyle.scheme",
                CODE_STYLE_SCHEMES,
                "Default",
                SCHEME_ACTIONS,
                cx,
            ))
            .child(h_flex().justify_end().child(not_yet_link(
                "code-style-set-from",
                "Set from…",
                cx,
            )))
            .children(self.indents_banner(cx))
    }

    fn html_style_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let header = self.language_style_header(cx);
        let (tab_row, tab) = tabs("codeStyle.html.tab", LANGUAGE_TABS, &[], cx);
        let content = match tab {
            "Other" => self.html_other_tab(window, cx),
            "Arrangement" => arrangement_tab(&[], "attribute", "order by name", cx),
            "Code Generation" => self.code_generation_tab(HTML_COMMENTS, cx),
            _ => {
                let indents = self.indent_options(&HTML_INDENTS, window, cx);
                indents.child(self.opt_check(
                    "codeStyle.html.uniformIndent",
                    "Use HTML indents within <style> and <script> tags",
                    false,
                    cx,
                ))
            }
        };
        v_flex()
            .gap_3()
            .child(header)
            .child(tab_row)
            .child(content.pl(px(10.)))
    }

    fn xml_style_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let header = self.language_style_header(cx);
        let (tab_row, tab) = tabs("codeStyle.xml.tab", LANGUAGE_TABS, &[], cx);
        let content = match tab {
            "Other" => self.xml_other_tab(window, cx),
            "Arrangement" => arrangement_tab(&["xmlns:.*"], "", "", cx),
            "Code Generation" => self.code_generation_tab(XML_COMMENTS, cx),
            _ => self.indent_options(&XML_INDENTS, window, cx),
        };
        v_flex()
            .gap_3()
            .child(header)
            .child(tab_row)
            .child(content.pl(px(10.)))
    }

    fn other_types_style_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let indents = self.indent_options(&OTHER_INDENTS, window, cx);
        v_flex()
            .gap_3()
            .child(self.code_scheme_row(
                "Scheme:",
                "codeStyle.scheme",
                CODE_STYLE_SCHEMES,
                "Default",
                SCHEME_ACTIONS,
                cx,
            ))
            .children(self.indents_banner(cx))
            .child(
                v_flex()
                    .pl(px(20.))
                    .gap_2()
                    .child("Text files and unsupported file types:")
                    .child(indents.pl(px(20.))),
            )
    }

    /// IntelliJ's IndentOptionsEditor: the tab options, the sizes, and SmartIndentOptionsEditor's
    /// continuation indent and empty-line indents where the language has them.
    fn indent_options(
        &mut self,
        keys: &IndentKeys,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        // text files' tab size is the one Spork's diffs show tabs at
        let tab_size = match keys.tab_size == OTHER_INDENTS.tab_size {
            true => self.tab_size_field(),
            false => self.opt_text(keys.tab_size, "4", 48., window, cx),
        };
        let indent = self.opt_text(keys.indent, "4", 48., window, cx);
        let continuation = keys
            .continuation
            .map(|key| self.opt_text(key, "8", 48., window, cx));
        v_flex()
            .gap_2()
            .child(self.opt_check(keys.use_tab, "Use tab character", false, cx))
            .child(div().pl(px(20.)).child(self.opt_check(
                keys.smart_tabs,
                "Smart tabs",
                false,
                cx,
            )))
            .child(form_row("Tab size:", 150., tab_size))
            .child(form_row("Indent:", 150., indent))
            .children(continuation.map(|field| form_row("Continuation indent:", 150., field)))
            .children(
                keys.keep_indents
                    .map(|key| self.opt_check(key, "Keep indents on empty lines", false, cx)),
            )
    }

    /// IntelliJ's RightMarginForm, atop a language's Other tab; empty fields take the General
    /// tab's values.
    fn margin_options(
        &mut self,
        [hard_wrap, wrap_on_typing, guides]: [&'static str; 3],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let hard_wrap = self.opt_text(hard_wrap, "", 64., window, cx);
        let guides = self.opt_text(guides, "", 120., window, cx);
        v_flex()
            .gap_2()
            .child(form_row(
                "Hard wrap at:",
                150.,
                h_flex().gap_2().child(hard_wrap).child("columns"),
            ))
            .child(form_row(
                "Wrap on typing",
                150.,
                self.opt_combo(
                    wrap_on_typing,
                    &["Default: No", "No", "Yes"],
                    "Default: No",
                    140.,
                    cx,
                ),
            ))
            .child(form_row(
                "Visual guides:",
                150.,
                h_flex().gap_2().child(guides).child("columns"),
            ))
            .child(
                div()
                    .pl(px(158.))
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Specify one guide (80) or several (80, 120)"),
            )
    }

    fn html_other_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let margins = self.margin_options(
            [
                "codeStyle.html.hardWrapAt",
                "codeStyle.html.wrapOnTyping",
                "codeStyle.html.visualGuides",
            ],
            window,
            cx,
        );
        let blank_lines = self.opt_text("codeStyle.html.keepBlankLines", "2", 48., window, cx);
        let insert_before = self.opt_text(
            "codeStyle.html.insertNewLineBefore",
            "body,div,p,form,h1,h2,h3",
            300.,
            window,
            cx,
        );
        let remove_before =
            self.opt_text("codeStyle.html.removeNewLineBefore", "br", 300., window, cx);
        let no_indent = self.opt_text(
            "codeStyle.html.doNotIndentChildrenOf",
            "html,body,thead,tbody,tfoot",
            300.,
            window,
            cx,
        );
        let min_lines = self.opt_text(
            "codeStyle.html.doNotIndentChildrenMinLines",
            "",
            48.,
            window,
            cx,
        );
        let inline = self.opt_text(
            "codeStyle.html.inlineElements",
            HTML_INLINE_ELEMENTS,
            300.,
            window,
            cx,
        );
        let keep_inside = self.opt_text(
            "codeStyle.html.keepWhiteSpacesInside",
            "span,pre,textarea",
            300.,
            window,
            cx,
        );
        let dont_break = self.opt_text(
            "codeStyle.html.dontBreakIfInlineContent",
            "title,h1,h2,h3,h4,h5,h6,p",
            300.,
            window,
            cx,
        );
        let column = 210.;
        v_flex()
            .gap_3()
            .child(margins)
            .child(
                v_flex()
                    .gap_2()
                    .child(self.opt_check(
                        "codeStyle.html.keepLineBreaks",
                        "Keep line breaks",
                        true,
                        cx,
                    ))
                    .child(self.opt_check(
                        "codeStyle.html.keepLineBreaksInText",
                        "Keep line breaks in text",
                        true,
                        cx,
                    ))
                    .child(form_row("Keep blank lines:", 150., blank_lines))
                    .child(form_row(
                        "Wrap attributes:",
                        150.,
                        self.opt_combo(
                            "codeStyle.html.wrapAttributes",
                            WRAP_STYLES,
                            "Wrap if long",
                            160.,
                            cx,
                        ),
                    )),
            )
            .child(
                h_flex()
                    .items_start()
                    .gap_6()
                    .child(
                        v_flex()
                            .w(px(200.))
                            .gap_2()
                            .child(self.opt_check("codeStyle.html.wrapText", "Wrap text", true, cx))
                            .child(self.opt_check(
                                "codeStyle.html.alignAttributes",
                                "Align attributes",
                                true,
                                cx,
                            ))
                            .child(self.opt_check(
                                "codeStyle.html.alignText",
                                "Align text",
                                false,
                                cx,
                            ))
                            .child(self.opt_check(
                                "codeStyle.html.keepWhiteSpaces",
                                "Keep white spaces",
                                false,
                                cx,
                            )),
                    )
                    .child(self.spaces_group(HTML_SPACES, cx)),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(form_row("Insert new line before:", column, insert_before))
                    .child(form_row("Remove new line before:", column, remove_before))
                    .child(form_row("Do not indent children of:", column, no_indent))
                    .child(form_row(
                        "or if tag size more than",
                        column,
                        h_flex().gap_2().child(min_lines).child("lines"),
                    ))
                    .child(form_row("Inline elements:", column, inline))
                    .child(form_row("Keep white spaces inside:", column, keep_inside))
                    .child(form_row(
                        "Don't break if inline content:",
                        column,
                        dont_break,
                    ))
                    .child(form_row(
                        "New line before first attribute:",
                        column,
                        self.opt_combo(
                            "codeStyle.html.newLineBeforeFirstAttribute",
                            NEW_LINE_STYLES,
                            "Never",
                            160.,
                            cx,
                        ),
                    ))
                    .child(form_row(
                        "New line after last attribute:",
                        column,
                        self.opt_combo(
                            "codeStyle.html.newLineAfterLastAttribute",
                            NEW_LINE_STYLES,
                            "Never",
                            160.,
                            cx,
                        ),
                    ))
                    .child(form_row(
                        "Generated quote marks:",
                        column,
                        self.opt_combo(
                            "codeStyle.html.quoteMarks",
                            &["None", "Single", "Double"],
                            "Double",
                            120.,
                            cx,
                        ),
                    ))
                    .child(div().pl(px(column + 8.)).child(self.opt_check(
                        "codeStyle.html.enforceQuotes",
                        "Enforce on format",
                        false,
                        cx,
                    ))),
            )
    }

    fn xml_other_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let margins = self.margin_options(
            [
                "codeStyle.xml.hardWrapAt",
                "codeStyle.xml.wrapOnTyping",
                "codeStyle.xml.visualGuides",
            ],
            window,
            cx,
        );
        let blank_lines = self.opt_text("codeStyle.xml.keepBlankLines", "2", 48., window, cx);
        v_flex()
            .gap_3()
            .child(margins)
            .child(
                v_flex()
                    .gap_2()
                    .child(self.opt_check(
                        "codeStyle.xml.keepLineBreaks",
                        "Keep line breaks",
                        true,
                        cx,
                    ))
                    .child(self.opt_check(
                        "codeStyle.xml.keepLineBreaksInText",
                        "Keep line breaks in text",
                        true,
                        cx,
                    ))
                    .child(form_row("Keep blank lines:", 150., blank_lines))
                    .child(form_row(
                        "Wrap attributes:",
                        150.,
                        self.opt_combo(
                            "codeStyle.xml.wrapAttributes",
                            WRAP_STYLES,
                            "Wrap if long",
                            160.,
                            cx,
                        ),
                    )),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(self.opt_check("codeStyle.xml.wrapText", "Wrap text", true, cx))
                    .child(self.opt_check(
                        "codeStyle.xml.alignAttributes",
                        "Align attributes",
                        true,
                        cx,
                    ))
                    .child(self.opt_check(
                        "codeStyle.xml.keepWhiteSpaces",
                        "Keep white spaces",
                        false,
                        cx,
                    )),
            )
            .child(self.spaces_group(XML_SPACES, cx))
            .child(
                v_flex().gap_2().child(titled("CDATA", cx)).child(
                    v_flex()
                        .pl(px(20.))
                        .gap_2()
                        .child(labeled(
                            "Whitespace around:",
                            self.opt_combo(
                                "codeStyle.xml.whitespaceAroundCdata",
                                &["Preserve", "Remove (keep with tags)", "Add new lines"],
                                "Preserve",
                                200.,
                                cx,
                            )
                            .into_any_element(),
                        ))
                        .child(self.opt_check(
                            "codeStyle.xml.keepWhitespaceInsideCdata",
                            "Keep whitespace inside",
                            false,
                            cx,
                        )),
                ),
            )
    }

    /// The Other tab's "Spaces" group.
    fn spaces_group(
        &self,
        [around_equality, after_tag_name, in_empty_tag]: [&'static str; 3],
        cx: &mut Context<Self>,
    ) -> Div {
        v_flex().flex_1().gap_2().child(titled("Spaces", cx)).child(
            v_flex()
                .pl(px(20.))
                .gap_2()
                .child(self.opt_check(around_equality, "Around \"=\" in attribute", false, cx))
                .child(self.opt_check(after_tag_name, "After tag name", false, cx))
                .child(self.opt_check(in_empty_tag, "In empty tag", false, cx)),
        )
    }

    /// GenerationCodeStylePanel: the commenter options XML and HTML show.
    fn code_generation_tab(
        &self,
        [line_first_column, block_first_column, block_add_space]: [&'static str; 3],
        cx: &mut Context<Self>,
    ) -> Div {
        v_flex().gap_2().child(titled("Comments", cx)).child(
            v_flex()
                .pl(px(20.))
                .gap_2()
                .child(self.opt_check(line_first_column, "Line comment at first column", true, cx))
                .child(self.opt_check(
                    block_first_column,
                    "Block comment at first column",
                    true,
                    cx,
                ))
                .child(self.opt_check(
                    block_add_space,
                    "Add spaces around block comments",
                    false,
                    cx,
                )),
        )
    }

    // ---- Inspections ---------------------------------------------------------------------

    fn inspections_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let filter = window.use_keyed_state("inspections-filter", cx, |window, cx| {
            InputState::new(window, cx)
        });
        let query = filter.read(cx).value().trim().to_lowercase();
        let selected = view("inspections.selected");
        let mut rows = Vec::new();
        for (group, inspections) in INSPECTIONS {
            let group: &'static str = group;
            let shown: Vec<&'static Inspection> = inspections
                .iter()
                .filter(|inspection| {
                    query.is_empty()
                        || inspection.name.to_lowercase().contains(&query)
                        || group.to_lowercase().contains(&query)
                })
                .collect();
            if shown.is_empty() {
                continue;
            }
            let on = inspections
                .iter()
                .filter(|inspection| self.flag(inspection.enabled_key, inspection.enabled))
                .count();
            let open = !query.is_empty() || is_open("inspections", group);
            rows.push(
                group_row(
                    "inspections",
                    group,
                    group_state(on, inspections.len()),
                    inspections.iter().map(|i| i.enabled_key).collect(),
                    open,
                    cx,
                )
                .into_any_element(),
            );
            if !open {
                continue;
            }
            for inspection in shown {
                let key = inspection.enabled_key;
                rows.push(
                    tree_row(key, 1, selected.as_deref() == Some(key), cx)
                        .child(self.opt_check(key, "", inspection.enabled, cx))
                        .child(inspection.name)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            set_view("inspections.selected", key);
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
        }
        if rows.is_empty() {
            rows.push(empty_text("No inspections found", cx).into_any_element());
        }
        let inspection = selected.as_deref().and_then(|key| {
            INSPECTIONS
                .iter()
                .flat_map(|(_, inspections)| inspections.iter())
                .find(|inspection| inspection.enabled_key == key)
        });
        let details = match inspection {
            Some(inspection) => {
                let severity = self.value(inspection.severity_key, inspection.severity);
                v_flex()
                    .size_full()
                    .child(
                        div()
                            .h(px(200.))
                            .p_2()
                            .overflow_hidden()
                            .child(inspection.description),
                    )
                    .child(div().h(px(1.)).bg(tokens.separator))
                    .child(
                        h_flex()
                            .p_2()
                            .gap_3()
                            .flex_wrap()
                            .child(spork_workbench::controls::combo(
                                "inspection-scope",
                                "In All Scopes",
                                false,
                                |menu, _, _| {
                                    menu.item(PopupMenuItem::new("In All Scopes").checked(true))
                                },
                            ))
                            .child(labeled(
                                "Severity:",
                                self.opt_combo(
                                    inspection.severity_key,
                                    SEVERITIES,
                                    inspection.severity,
                                    230.,
                                    cx,
                                )
                                .into_any_element(),
                            ))
                            // shortcut: shows the severity's own attributes and can't be changed;
                            // keep a key per inspection once Spork's editor highlights them
                            .child(labeled(
                                "Highlighting in editor:",
                                div()
                                    .w(px(160.))
                                    .child(spork_workbench::controls::combo(
                                        "inspection-highlighting",
                                        severity,
                                        true,
                                        |menu, _, _| menu,
                                    ))
                                    .into_any_element(),
                            )),
                    )
            }
            None => v_flex().size_full(),
        };
        let expand = ActionButton::new("inspections-expand", "general/expandAll")
            .tooltip("Expand All")
            .on_click(cx.listener(|_, _, _, cx| {
                for (group, _) in INSPECTIONS {
                    set_open("inspections", group, true);
                }
                cx.notify();
            }));
        let collapse = ActionButton::new("inspections-collapse", "general/collapseAll")
            .tooltip("Collapse All")
            .on_click(cx.listener(|_, _, _, cx| {
                for (group, _) in INSPECTIONS {
                    set_open("inspections", group, false);
                }
                cx.notify();
            }));
        v_flex()
            .gap_3()
            .child(self.code_scheme_row(
                "Profile:",
                "inspections.profile",
                &["Default", "Project Default"],
                "Project Default",
                PROFILE_ACTIONS,
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div().w(px(240.)).child(
                            spork_workbench::TextField::new(&filter)
                                .prefix(spork_workbench::icon("general/search").size(px(16.))),
                        ),
                    )
                    .child(toolbar(
                        "inspections",
                        &[("general/filter", "Filter Inspections")],
                    ))
                    .child(div().w(px(1.)).h(px(16.)).bg(tokens.separator))
                    .child(expand)
                    .child(collapse)
                    .child(toolbar(
                        "inspections",
                        &[("actions/unselectAll", "Reset to Empty")],
                    )),
            )
            .child(
                h_flex()
                    .items_start()
                    .h(px(400.))
                    .gap_2()
                    .child(
                        tree_box("inspections-tree", rows, cx)
                            .w(px(300.))
                            .flex_none(),
                    )
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .border_1()
                            .border_color(tokens.control_border)
                            .child(details),
                    ),
            )
            .child(self.opt_check(
                "inspections.disableNewByDefault",
                "Disable new inspections by default",
                false,
                cx,
            ))
    }

    // ---- File and Code Templates ---------------------------------------------------------

    fn file_templates_page(&mut self, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let (tab_row, tab) = tabs(
            "fileTemplates.tab",
            &["Files", "Includes", "Code"],
            &["Code"],
            cx,
        );
        // The reference app has one template, the internal Generic Empty File; Includes is empty
        let templates: &[&str] = match tab {
            "Files" => &["Generic Empty File"],
            _ => &[],
        };
        let list = list_box(cx)
            .w(px(220.))
            .h_full()
            .children(templates.iter().map(|name| {
                tree_row(
                    SharedString::from(format!("file-template-{name}")),
                    0,
                    true,
                    cx,
                )
                .child(spork_workbench::image("fileTypes/text", cx))
                .child(*name)
            }));
        let editor = match templates.is_empty() {
            true => v_flex().flex_1(),
            false => v_flex()
                .flex_1()
                .h_full()
                .gap_2()
                .child(text_box("", 220., cx))
                .child(
                    h_flex()
                        .gap_4()
                        .child(self.opt_check(
                            "fileTemplates.genericEmptyFile.reformat",
                            "Reformat according to style",
                            true,
                            cx,
                        ))
                        .child(self.opt_check(
                            "fileTemplates.genericEmptyFile.liveTemplates",
                            "Enable Live Templates",
                            false,
                            cx,
                        )),
                )
                .child("Description")
                .child(
                    div()
                        .flex_1()
                        .border_1()
                        .border_color(tokens.control_border),
                ),
        };
        v_flex()
            .gap_3()
            .child(labeled(
                "Scheme:",
                self.opt_combo(
                    "fileTemplates.scheme",
                    &["Default", "Project"],
                    "Default",
                    160.,
                    cx,
                )
                .into_any_element(),
            ))
            .child(tab_row)
            .child(toolbar(
                "file-templates",
                &[
                    ("general/add", "Create Template"),
                    ("general/remove", "Remove Template"),
                    ("general/copy", "Copy Template"),
                    ("general/reset", "Revert to Original Template"),
                ],
            ))
            .child(
                h_flex()
                    .items_start()
                    .h(px(400.))
                    .gap_2()
                    .child(list)
                    .child(editor),
            )
    }

    // ---- File Encodings ------------------------------------------------------------------

    fn file_encodings_page(&mut self, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let bom_comment = match self
            .value("fileEncodings.createUtf8Files", "with NO BOM")
            .as_str()
        {
            "with BOM" => "Spork will add UTF-8 BOM to every created file in UTF-8 encoding",
            "with BOM under Windows, with no BOM otherwise" => {
                "Spork will add UTF-8 BOM to every created UTF-8 file only when it's running under Windows"
            }
            _ => "Spork will NOT add UTF-8 BOM to every created file in UTF-8 encoding",
        };
        let header = h_flex()
            .h(px(metrics::ROW_HEIGHT))
            .px(px(6.))
            .border_b_1()
            .border_color(tokens.control_border)
            .child(div().flex_1().child("Path"))
            .child(div().w(px(160.)).child("Encoding"));
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_4()
                    .child(labeled(
                        "Global Encoding:",
                        self.opt_combo("fileEncodings.global", ENCODINGS, "UTF-8", 140., cx)
                            .into_any_element(),
                    ))
                    .child(labeled(
                        "Project Encoding:",
                        self.opt_combo("fileEncodings.project", ENCODINGS, "UTF-8", 140., cx)
                            .into_any_element(),
                    )),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "Add the path to a file or directory and select the encoding Spork should use. \
                         Files and directories inherit the encoding from the parent directory or from \
                         the Project Encoding. Built-in file encodings in JSP, HTML, and XML files \
                         override these settings.",
                    ),
            )
            .child(
                v_flex()
                    .child(toolbar(
                        "file-encodings",
                        &[("general/add", "Add"), ("general/remove", "Remove")],
                    ))
                    .child(
                        list_box(cx)
                            .h(px(220.))
                            .child(header)
                            .child(empty_text("Encodings are not configured", cx)),
                    ),
            )
            .child(labeled(
                "Default encoding for properties files:",
                self.opt_combo(
                    "fileEncodings.properties",
                    PROPERTIES_ENCODINGS,
                    "<Properties Default: UTF-8>",
                    220.,
                    cx,
                )
                .into_any_element(),
            ))
            .child(div().pl(px(20.)).child(self.opt_check(
                "fileEncodings.nativeToAscii",
                "Transparent native-to-ascii conversion",
                false,
                cx,
            )))
            .child(
                v_flex()
                    .gap_1()
                    .child(labeled(
                        "Create UTF-8 files:",
                        self.opt_combo(
                            "fileEncodings.createUtf8Files",
                            &[
                                "with BOM",
                                "with NO BOM",
                                "with BOM under Windows, with no BOM otherwise",
                            ],
                            "with NO BOM",
                            320.,
                            cx,
                        )
                        .into_any_element(),
                    ))
                    .child(comment(bom_comment, cx)),
            )
    }

    // ---- Live Templates ------------------------------------------------------------------

    fn live_templates_page(&mut self, cx: &mut Context<Self>) -> Div {
        let selected = view("liveTemplates.selected");
        let muted = cx.theme().muted_foreground;
        let mut rows = Vec::new();
        for (group, templates) in LIVE_TEMPLATES {
            let group: &'static str = group;
            let on = templates
                .iter()
                .filter(|template| self.flag(template.key, true))
                .count();
            let open = is_open("liveTemplates", group);
            rows.push(
                group_row(
                    "liveTemplates",
                    group,
                    group_state(on, templates.len()),
                    templates.iter().map(|t| t.key).collect(),
                    open,
                    cx,
                )
                .into_any_element(),
            );
            if !open {
                continue;
            }
            for template in templates.iter() {
                let key = template.key;
                rows.push(
                    tree_row(key, 1, selected.as_deref() == Some(key), cx)
                        .child(self.opt_check(key, "", true, cx))
                        .child(template.abbreviation)
                        .when(!template.description.is_empty(), |row| {
                            row.child(
                                div().truncate().text_color(muted).child(SharedString::from(
                                    format!("({})", template.description),
                                )),
                            )
                        })
                        .on_click(cx.listener(move |_, _, _, cx| {
                            set_view("liveTemplates.selected", key);
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
        }
        let template = selected.as_deref().and_then(|key| {
            LIVE_TEMPLATES
                .iter()
                .flat_map(|(_, templates)| templates.iter())
                .find(|template| template.key == key)
        });
        let details = match template {
            Some(template) => self.live_template_details(template, cx),
            None => empty_text("No live templates are selected", cx),
        };
        v_flex()
            .gap_3()
            .child(labeled(
                "By default expand with",
                self.opt_combo(
                    "liveTemplates.expandWith",
                    &["Space", "Tab", "Enter", "Custom"],
                    "Tab",
                    120.,
                    cx,
                )
                .into_any_element(),
            ))
            .child(
                v_flex()
                    .child(toolbar(
                        "live-templates",
                        &[
                            ("general/add", "Add"),
                            ("general/remove", "Remove"),
                            ("general/copy", "Duplicate"),
                            ("general/reset", "Restore Deleted Defaults"),
                        ],
                    ))
                    .child(tree_box("live-templates-tree", rows, cx).h(px(280.))),
            )
            .child(details)
    }

    /// LiveTemplateSettingsEditor: the selected template's fields, text and options.
    fn live_template_details(
        &self,
        template: &'static LiveTemplate,
        cx: &mut Context<Self>,
    ) -> Div {
        let (context, link) = match template.applies {
            Applies::HtmlXml => ("Applicable in HTML; XML: XML text, XML tag", "Change"),
            Applies::XmlText => ("Applicable in XML: XML text", "Change"),
            Applies::HtmlText => ("Applicable in HTML: HTML Text", "Change"),
            Applies::XslText => ("Applicable in XML: XSL Text", "Change"),
            Applies::None => ("No applicable contexts", "Define"),
        };
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_4()
                    .child(labeled(
                        "Abbreviation:",
                        read_only_field(template.abbreviation, 120., cx).into_any_element(),
                    ))
                    .child(labeled(
                        "Description:",
                        read_only_field(template.description, 300., cx).into_any_element(),
                    )),
            )
            .child("Template text:")
            .child(
                h_flex()
                    .items_start()
                    .gap_3()
                    .child(text_box(template.text, 140., cx).flex_1())
                    .child(
                        v_flex()
                            .w(px(220.))
                            .gap_2()
                            .child(self.opt_button(
                                "live-template-variables",
                                "Edit Variables…",
                                cx,
                            ))
                            .child(titled("Options", cx))
                            .child(labeled(
                                "Expand with",
                                self.opt_combo(
                                    template.expand_key,
                                    &["Default (Tab)", "Space", "Tab", "Enter", "None"],
                                    template.expand,
                                    130.,
                                    cx,
                                )
                                .into_any_element(),
                            ))
                            .child(self.opt_check(
                                template.reformat_key,
                                "Reformat according to style",
                                template.reformat,
                                cx,
                            )),
                    ),
            )
            .child(h_flex().gap_2().child(context).child(not_yet_link(
                "live-template-context",
                link,
                cx,
            )))
    }

    // ---- File Types ----------------------------------------------------------------------

    fn file_types_page(&mut self, cx: &mut Context<Self>) -> Div {
        let (tab_row, tab) = tabs(
            "fileTypes.tab",
            &["Recognized File Types", "Ignored Files and Folders"],
            &[],
            cx,
        );
        let content =
            match tab {
                "Ignored Files and Folders" => v_flex()
                    .gap_2()
                    .child("Ignored files and folders are not visible in the IDE and not indexed")
                    .child(toolbar(
                        "ignored-files",
                        &[("general/add", "Add"), ("general/remove", "Remove")],
                    ))
                    .child(list_box(cx).h(px(380.)).children(IGNORED_FILES.iter().map(
                        |pattern| {
                            tree_row(
                                SharedString::from(format!("ignored-{pattern}")),
                                0,
                                false,
                                cx,
                            )
                            .child(*pattern)
                        },
                    ))),
                _ => self.recognized_file_types(cx),
            };
        v_flex().gap_3().child(tab_row).child(content)
    }

    fn recognized_file_types(&self, cx: &mut Context<Self>) -> Div {
        let selected = view("fileTypes.selected");
        let file_type = FILE_TYPES
            .iter()
            .find(|(name, _, _)| selected.as_deref() == Some(*name));
        let rows: Vec<AnyElement> = FILE_TYPES
            .iter()
            .map(|(name, icon, _)| {
                let name: &'static str = name;
                tree_row(
                    SharedString::from(format!("file-type-{name}")),
                    0,
                    selected.as_deref() == Some(name),
                    cx,
                )
                .child(spork_workbench::image(icon, cx))
                .child(name)
                .on_click(cx.listener(move |_, _, _, cx| {
                    set_view("fileTypes.selected", name);
                    cx.notify();
                }))
                .into_any_element()
            })
            .collect();
        let patterns: Vec<AnyElement> = match file_type {
            Some((_, _, patterns)) if !patterns.is_empty() => patterns
                .iter()
                .map(|pattern| {
                    tree_row(
                        SharedString::from(format!("pattern-{pattern}")),
                        0,
                        false,
                        cx,
                    )
                    .child(*pattern)
                    .into_any_element()
                })
                .collect(),
            _ => vec![empty_text("No registered file patterns", cx).into_any_element()],
        };
        let edit = &[
            ("general/add", "Add"),
            ("general/remove", "Remove"),
            ("general/edit", "Edit"),
        ];
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .items_start()
                    .h(px(400.))
                    .gap_3()
                    .child(
                        v_flex()
                            .w(px(260.))
                            .h_full()
                            .child(toolbar("file-types", edit))
                            .child(tree_box("file-types-list", rows, cx).flex_1()),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .h_full()
                            .gap_2()
                            .child("File name patterns:")
                            .child(toolbar("file-patterns", edit))
                            .child(tree_box("file-patterns-list", patterns, cx).flex_1())
                            .child("HashBang patterns:")
                            .child(toolbar(
                                "hashbang-patterns",
                                &[("general/add", "Add"), ("general/remove", "Remove")],
                            ))
                            .child(
                                list_box(cx)
                                    .h(px(90.))
                                    .child(empty_text("No registered file patterns", cx)),
                            ),
                    ),
            )
            .child(self.opt_button(
                "file-types-associate",
                "Associate File Types with Spork…",
                cx,
            ))
    }

    // ---- Inlay Hints ---------------------------------------------------------------------

    fn inlay_hints_page(&mut self, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let selected = view("inlayHints.selected");
        let on = CODE_VISION
            .iter()
            .filter(|(_, key, _)| self.flag(key, true))
            .count();
        let open = view("inlayHints/open/Code vision").is_none_or(|open| open == "true");
        let mut rows = vec![
            group_row(
                "inlayHints",
                "Code vision",
                group_state(on, CODE_VISION.len()),
                CODE_VISION.iter().map(|(_, key, _)| *key).collect(),
                open,
                cx,
            )
            .into_any_element(),
        ];
        if open {
            for (name, key, _) in CODE_VISION {
                let key: &'static str = key;
                rows.push(
                    tree_row(key, 1, selected.as_deref() == Some(key), cx)
                        .child(self.opt_check(key, "", true, cx))
                        .child(*name)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            set_view("inlayHints.selected", key);
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
        }
        let description = CODE_VISION
            .iter()
            .find(|(_, key, _)| selected.as_deref() == Some(*key))
            .map(|(name, _, description)| {
                v_flex()
                    .gap_2()
                    .p_2()
                    .child(div().font_weight(FontWeight::BOLD).child(*name))
                    .child(*description)
            });
        h_flex()
            .items_start()
            .h(px(480.))
            .gap_2()
            .child(
                tree_box("inlay-hints-tree", rows, cx)
                    .w(px(260.))
                    .flex_none(),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .border_1()
                    .border_color(tokens.control_border)
                    .children(description),
            )
    }

    // ---- Emmet ---------------------------------------------------------------------------

    /// EmmetCompositeConfigurable with the reference app's only inner page, XmlEmmetConfigurable.
    fn emmet_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let element = self.opt_text("emmet.bemElementSeparator", "__", 48., window, cx);
        let modifier = self.opt_text("emmet.bemModifierSeparator", "_", 48., window, cx);
        let prefix = self.opt_text("emmet.bemShortElementPrefix", "-", 48., window, cx);
        v_flex()
            .gap_3()
            .child(labeled(
                "Expand abbreviation with",
                self.opt_combo(
                    "emmet.expandWith",
                    &["Space", "Tab", "Enter", "Custom"],
                    "Tab",
                    120.,
                    cx,
                )
                .into_any_element(),
            ))
            .child(self.opt_check("emmet.enabled", "Enable XML/HTML Emmet", true, cx))
            .child(
                v_flex()
                    .gap_2()
                    .child(self.opt_check(
                        "emmet.preview",
                        "Enable abbreviation preview",
                        false,
                        cx,
                    ))
                    .child(self.opt_check(
                        "emmet.hrefAutoDetect",
                        "Enable automatic URL recognition while wrapping text with <a> tag",
                        true,
                        cx,
                    ))
                    .child(self.opt_check(
                        "emmet.editPointAtEnd",
                        "Add edit point at the end of template",
                        false,
                        cx,
                    )),
            )
            .child(titled("BEM", cx))
            .child(
                v_flex()
                    .pl(px(20.))
                    .gap_2()
                    .child(form_row("Element separator in class names:", 240., element))
                    .child(form_row(
                        "Modifier separator in class names:",
                        240.,
                        modifier,
                    ))
                    .child(form_row("Short element prefix:", 240., prefix)),
            )
            .child(titled("Filters enabled by default", cx))
            .child(
                v_flex().pl(px(20.)).gap_2().children(
                    EMMET_FILTERS
                        .iter()
                        .map(|(key, label)| self.opt_check(key, label, false, cx)),
                ),
            )
    }

    // ---- Intentions ----------------------------------------------------------------------

    fn intentions_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let filter = window.use_keyed_state("intentions-filter", cx, |window, cx| {
            InputState::new(window, cx)
        });
        let query = filter.read(cx).value().trim().to_lowercase();
        let selected = view("intentions.selected");
        let mut rows = Vec::new();
        for (category, intentions) in INTENTIONS {
            let category: &'static str = category;
            let shown: Vec<&'static Intention> = intentions
                .iter()
                .filter(|intention| {
                    query.is_empty()
                        || intention.name.to_lowercase().contains(&query)
                        || category.to_lowercase().contains(&query)
                })
                .collect();
            if shown.is_empty() {
                continue;
            }
            let on = intentions
                .iter()
                .filter(|intention| self.flag(intention.key, true))
                .count();
            let open = !query.is_empty() || is_open("intentions", category);
            rows.push(
                group_row(
                    "intentions",
                    category,
                    group_state(on, intentions.len()),
                    intentions.iter().map(|i| i.key).collect(),
                    open,
                    cx,
                )
                .into_any_element(),
            );
            if !open {
                continue;
            }
            for intention in shown {
                let key = intention.key;
                rows.push(
                    tree_row(key, 1, selected.as_deref() == Some(key), cx)
                        .child(self.opt_check(key, "", true, cx))
                        .child(intention.name)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            set_view("intentions.selected", key);
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
        }
        let description = selected.as_deref().and_then(|key| {
            INTENTIONS
                .iter()
                .flat_map(|(_, intentions)| intentions.iter())
                .find(|intention| intention.key == key)
                .map(|intention| intention.description)
        });
        v_flex()
            .gap_3()
            .child(
                div().w(px(240.)).child(
                    spork_workbench::TextField::new(&filter)
                        .prefix(spork_workbench::icon("general/search").size(px(16.))),
                ),
            )
            .child(
                h_flex()
                    .items_start()
                    .h(px(460.))
                    .gap_2()
                    .child(
                        tree_box("intentions-tree", rows, cx)
                            .w(px(300.))
                            .flex_none(),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .h_full()
                            .gap_1()
                            .child("Description")
                            .child(
                                div()
                                    .flex_1()
                                    .p_2()
                                    .border_1()
                                    .border_color(tokens.control_border)
                                    .children(description),
                            ),
                    ),
            )
    }

    // ---- Reader Mode ---------------------------------------------------------------------

    fn reader_mode_page(&mut self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let chosen = self.value(
            "readerMode.visualFormattingScheme",
            "Use active scheme: Default",
        ) == "Choose scheme:";
        v_flex()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(self.opt_check("readerMode.enabled", "Enable Reader mode", true, cx))
                    .child(comment(
                        "Available for library and read-only files. Makes the code convenient to read using the options below.",
                        cx,
                    )),
            )
            .child(
                v_flex()
                    .pl(px(20.))
                    .gap_2()
                    .child("Show in Reader mode:")
                    .child(
                        v_flex()
                            .pl(px(20.))
                            .gap_2()
                            .child(self.opt_check(
                                "readerMode.renderedDocs",
                                "Rendered documentation comments",
                                true,
                                cx,
                            ))
                            .child(self.opt_check(
                                "readerMode.warnings",
                                "Error and warning highlighting, inspection widget",
                                false,
                                cx,
                            ))
                            .child(self.opt_check("readerMode.ligatures", "Font ligatures", false, cx))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(self.opt_check(
                                        "readerMode.lineSpacing",
                                        "Increased line height",
                                        false,
                                        cx,
                                    ))
                                    .child(div().text_xs().text_color(muted).child("Increased by 1.2")),
                            ),
                    )
                    .child(self.opt_check(
                        "readerMode.visualFormatting",
                        "Format code according to preferred style",
                        true,
                        cx,
                    ))
                    .child(
                        v_flex()
                            .pl(px(20.))
                            .gap_2()
                            .child(self.opt_radio(
                                "readerMode.visualFormattingScheme",
                                &["Use active scheme: Default"],
                                "Use active scheme: Default",
                                cx,
                            ))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(self.opt_radio(
                                        "readerMode.visualFormattingScheme",
                                        &["Choose scheme:"],
                                        "Use active scheme: Default",
                                        cx,
                                    ))
                                    .child(self.combo_if(
                                        chosen,
                                        "readerMode.chosenScheme",
                                        &["Project", "Default"],
                                        "Default",
                                        120.,
                                        cx,
                                    )),
                            ),
                    ),
            )
    }

    // ---- TextMate Bundles ----------------------------------------------------------------

    fn textmate_page(&mut self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let rows: Vec<AnyElement> = TEXTMATE_BUNDLES
            .iter()
            .map(|(name, key)| {
                tree_row(SharedString::from(format!("bundle-{name}")), 0, false, cx)
                    .child(self.opt_check(key, "", true, cx))
                    .child(div().flex_1().child(*name))
                    .child(div().text_color(muted).child("Built-in"))
                    .into_any_element()
            })
            .collect();
        v_flex()
            .gap_3()
            .child(
                "Use this page to add TextMate bundles for languages that do not have first-class support in Spork.",
            )
            .child(
                v_flex()
                    .child(toolbar(
                        "textmate",
                        &[("general/add", "Add"), ("general/remove", "Remove")],
                    ))
                    .child(tree_box("textmate-bundles", rows, cx).h(px(440.))),
            )
    }
}

thread_local! {
    /// UI state of these pages that isn't a setting: the tab shown, the selected row, the open
    /// groups. Kept for the session, as IntelliJ keeps the Code Style tab in a static.
    static VIEW: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

fn view(key: &str) -> Option<String> {
    VIEW.with_borrow(|view| view.get(key).cloned())
}

fn set_view(key: &str, value: &str) {
    VIEW.with_borrow_mut(|view| {
        view.insert(key.to_owned(), value.to_owned());
    });
}

fn is_open(tree: &str, group: &str) -> bool {
    view(&format!("{tree}/open/{group}")).is_some_and(|open| open == "true")
}

fn set_open(tree: &str, group: &str, open: bool) {
    set_view(
        &format!("{tree}/open/{group}"),
        if open { "true" } else { "false" },
    );
}

/// A group's checkbox from its items: checked, unchecked, or partly (None).
fn group_state(on: usize, total: usize) -> Option<bool> {
    match on {
        0 => Some(false),
        on if on == total => Some(true),
        _ => None,
    }
}

/// IntelliJ's tabbed pane header; the tab picked is kept under `key`, the first one at first.
/// `disabled`: tabs shown greyed, as File and Code Templates' empty Code tab.
fn tabs(
    key: &'static str,
    labels: &'static [&'static str],
    disabled: &'static [&'static str],
    cx: &mut Context<SettingsWindow>,
) -> (Div, &'static str) {
    let tokens = Tokens::get(cx);
    let selected = view(key)
        .and_then(|picked| labels.iter().copied().find(|label| *label == picked))
        .or_else(|| labels.first().copied())
        .unwrap_or_default();
    let row = h_flex().gap_1().children(labels.iter().map(|label| {
        let label: &'static str = label;
        let tab = spork_workbench::shell::header_tab(
            SharedString::from(format!("{key}-{label}")),
            label,
            label == selected,
            cx,
        );
        match disabled.contains(&label) {
            true => tab.text_color(tokens.text_disabled),
            false => tab.on_click(cx.listener(move |_, _, _, cx| {
                set_view(key, label);
                cx.notify();
            })),
        }
    }));
    (row, selected)
}

/// The schemes panel's gear; its actions are dialogs Spork doesn't have, a click says so.
fn gear(id: SharedString, actions: &'static [&'static str]) -> impl IntoElement {
    ActionButton::new(id, "general/settings").dropdown_menu(move |menu, _, _| {
        actions.iter().fold(menu, |menu, action| match *action {
            "" => menu.separator(),
            action => menu.item(
                PopupMenuItem::new(action)
                    .on_click(move |_, window, cx| not_in_spork(action, window, cx)),
            ),
        })
    })
}

/// IntelliJ's ToolbarDecorator buttons for dialogs Spork doesn't have: a click says so.
fn toolbar(id: &'static str, actions: &'static [(&'static str, &'static str)]) -> Div {
    h_flex()
        .gap(px(2.))
        .children(actions.iter().map(|(icon, tooltip)| {
            let tooltip: &'static str = tooltip;
            ActionButton::new(SharedString::from(format!("{id}-{icon}")), *icon)
                .tooltip(tooltip)
                .on_click(move |_, window, cx| not_in_spork(tooltip, window, cx))
        }))
}

fn not_in_spork(label: &str, window: &mut Window, cx: &mut App) {
    window.push_notification(
        Notification::info(format!(
            "{} isn't in Spork yet.",
            label.trim_end_matches('…')
        )),
        cx,
    );
}

/// A link to a dialog Spork doesn't have: a click says so.
fn not_yet_link(id: &'static str, label: &'static str, cx: &App) -> Stateful<Div> {
    spork_workbench::controls::link(id, label, cx)
        .on_click(move |_, window, cx| not_in_spork(label, window, cx))
}

/// A label `width` px wide and its control, as IntelliJ's form columns.
fn form_row(label: &'static str, width: f32, control: impl IntoElement) -> Div {
    h_flex()
        .gap_2()
        .child(div().w(px(width)).flex_none().child(label))
        .child(control)
}

/// A list's or a tree's box: IntelliJ's 1 px border.
fn list_box(cx: &App) -> Div {
    v_flex()
        .border_1()
        .border_color(Tokens::get(cx).control_border)
}

/// A scrolling tree or list in IntelliJ's 1 px border, its rows down from the top.
fn tree_box(id: &'static str, rows: Vec<AnyElement>, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .h_full()
        .border_1()
        .border_color(Tokens::get(cx).control_border)
        .child(
            div()
                .size_full()
                .overflow_y_scrollbar()
                .id(SharedString::from(format!("{id}-scroll")))
                .child(v_flex().children(rows)),
        )
}

/// A tree or list row at `depth`, highlighted when `selected`.
fn tree_row(id: impl Into<ElementId>, depth: usize, selected: bool, cx: &App) -> Stateful<Div> {
    let tokens = Tokens::get(cx);
    h_flex()
        .id(id)
        .h(px(metrics::ROW_HEIGHT))
        .flex_none()
        .pl(px(4. + depth as f32 * 20.))
        .pr(px(6.))
        .gap(px(4.))
        .cursor_pointer()
        .when(selected, |row| row.bg(tokens.selection))
        .when(!selected, |row| {
            row.hover(move |row| row.bg(tokens.selection_hover))
        })
}

/// A tree's group: its chevron, its checkbox from its items, its name; a click opens or closes
/// it.
fn group_row(
    tree: &'static str,
    group: &'static str,
    state: Option<bool>,
    keys: Vec<&'static str>,
    open: bool,
    cx: &mut Context<SettingsWindow>,
) -> Stateful<Div> {
    let tokens = Tokens::get(cx);
    tree_row(
        SharedString::from(format!("{tree}-group-{group}")),
        0,
        false,
        cx,
    )
    .child(
        spork_workbench::icon(match open {
            true => "general/chevronDown",
            false => "general/chevronRight",
        })
        .size(px(16.))
        .text_color(tokens.icon),
    )
    .child(
        spork_workbench::controls::check_box(
            SharedString::from(format!("{tree}-group-{group}-box")),
            state,
            cx,
        )
        // all of the group's items on, or all off when they all are
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            let value = if state == Some(true) { "false" } else { "true" };
            for key in &keys {
                this.set_value(key, value);
            }
            cx.notify();
        })),
    )
    // IntelliJ's checkbox trees set their groups in bold
    .child(div().font_weight(FontWeight::BOLD).child(group))
    .on_click(cx.listener(move |_, _, _, cx| {
        set_open(tree, group, !open);
        cx.notify();
    }))
}

/// An empty list's or pane's grey text.
/// An empty list's or panel's text, centered in it as IntelliJ's.
fn empty_text(text: &'static str, cx: &App) -> Div {
    div()
        .flex_1()
        .min_h(px(120.))
        .p_2()
        .flex()
        .items_center()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

/// A text field's look for a value Spork shows but doesn't edit.
fn read_only_field(text: &'static str, width: f32, cx: &App) -> Div {
    let tokens = Tokens::get(cx);
    div()
        .w(px(width))
        .h(px(metrics::CONTROL_HEIGHT))
        .px_2()
        .flex()
        .items_center()
        .rounded(px(metrics::RADIUS))
        .border_1()
        .border_color(tokens.control_border)
        .truncate()
        .child(text)
}

/// A template's text, in the editor font.
fn text_box(text: &'static str, height: f32, cx: &App) -> Div {
    let tokens = Tokens::get(cx);
    div()
        .h(px(height))
        .p_2()
        .overflow_hidden()
        .border_1()
        .border_color(tokens.control_border)
        .font_family(cx.theme().mono_font_family.clone())
        .whitespace_normal()
        .child(text)
}

/// The Arrangement tab: its matching rules, XmlRearranger's or HtmlRearranger's defaults.
fn arrangement_tab(
    names: &'static [&'static str],
    entry: &'static str,
    order: &'static str,
    cx: &App,
) -> Div {
    let tokens = Tokens::get(cx);
    let token = |text: &'static str| {
        div()
            .px_1()
            .rounded(px(metrics::RADIUS))
            .border_1()
            .border_color(tokens.control_border)
            .child(text)
    };
    let rule = h_flex()
        .h(px(metrics::ROW_HEIGHT))
        .px(px(6.))
        .gap_2()
        .child(
            div()
                .w(px(16.))
                .text_color(cx.theme().muted_foreground)
                .child("1"),
        )
        .when(!entry.is_empty(), |rule| rule.child(token(entry)))
        .children(names.iter().map(|name| token(name)))
        .child(div().flex_1())
        .when(!order.is_empty(), |rule| rule.child(order));
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .justify_between()
                .child("Matching rules:")
                .child(toolbar(
                    "arrangement",
                    &[
                        ("general/add", "Add Rule"),
                        ("general/remove", "Remove Rule"),
                        ("general/edit", "Edit Rule"),
                        ("general/moveUp", "Move Up"),
                        ("general/moveDown", "Move Down"),
                    ],
                )),
        )
        .child(list_box(cx).h(px(300.)).child(rule))
}

const CODE_STYLE_SCHEMES: &[&str] = &["Default", "Project"];

/// AbstractSchemeActions' menu for a code style scheme.
const SCHEME_ACTIONS: &[&str] = &[
    "Copy to Project…",
    "Duplicate…",
    "Rename…",
    "Restore Defaults",
    "Delete…",
    "",
    "Export",
    "Import Scheme",
];

/// AbstractSchemeActions' menu for the project's inspection profile.
const PROFILE_ACTIONS: &[&str] = &[
    "Copy to IDE…",
    "Duplicate…",
    "Rename…",
    "Restore Defaults",
    "Delete…",
    "",
    "Export",
    "Import Profile",
];

const LINE_SEPARATORS: &[&str] = &[
    "System-Dependent",
    "Unix and macOS (\\n)",
    "Windows (\\r\\n)",
    "Classic Mac OS (\\r)",
];

/// TabbedLanguageCodeStylePanel's tabs for HTML and XML.
const LANGUAGE_TABS: &[&str] = &[
    "Tabs and Indents",
    "Other",
    "Arrangement",
    "Code Generation",
];

const WRAP_STYLES: &[&str] = &[
    "Do not wrap",
    "Wrap if long",
    "Chop down if long",
    "Wrap always",
];

const NEW_LINE_STYLES: &[&str] = &["Never", "When multiline"];

const HTML_INLINE_ELEMENTS: &str = "a,abbr,acronym,b,basefont,bdo,big,br,cite,code,dfn,em,font,i,img,input,kbd,label,q,s,samp,select,small,span,strike,strong,sub,sup,textarea,tt,u,var";

/// A language's Tabs and Indents settings, by key.
struct IndentKeys {
    use_tab: &'static str,
    smart_tabs: &'static str,
    tab_size: &'static str,
    indent: &'static str,
    /// SmartIndentOptionsEditor's: Other File Types has neither.
    continuation: Option<&'static str>,
    keep_indents: Option<&'static str>,
}

const HTML_INDENTS: IndentKeys = IndentKeys {
    use_tab: "codeStyle.html.useTab",
    smart_tabs: "codeStyle.html.smartTabs",
    tab_size: "codeStyle.html.tabSize",
    indent: "codeStyle.html.indent",
    continuation: Some("codeStyle.html.continuationIndent"),
    keep_indents: Some("codeStyle.html.keepIndentsOnEmptyLines"),
};

const XML_INDENTS: IndentKeys = IndentKeys {
    use_tab: "codeStyle.xml.useTab",
    smart_tabs: "codeStyle.xml.smartTabs",
    tab_size: "codeStyle.xml.tabSize",
    indent: "codeStyle.xml.indent",
    continuation: Some("codeStyle.xml.continuationIndent"),
    keep_indents: Some("codeStyle.xml.keepIndentsOnEmptyLines"),
};

const OTHER_INDENTS: IndentKeys = IndentKeys {
    use_tab: "codeStyle.other.useTab",
    smart_tabs: "codeStyle.other.smartTabs",
    tab_size: "codeStyle.other.tabSize",
    indent: "codeStyle.other.indent",
    continuation: None,
    keep_indents: None,
};

const HTML_SPACES: [&str; 3] = [
    "codeStyle.html.spaceAroundEquality",
    "codeStyle.html.spaceAfterTagName",
    "codeStyle.html.spaceInEmptyTag",
];

const XML_SPACES: [&str; 3] = [
    "codeStyle.xml.spaceAroundEquality",
    "codeStyle.xml.spaceAfterTagName",
    "codeStyle.xml.spaceInEmptyTag",
];

const HTML_COMMENTS: [&str; 3] = [
    "codeStyle.html.lineCommentAtFirstColumn",
    "codeStyle.html.blockCommentAtFirstColumn",
    "codeStyle.html.blockCommentAddSpace",
];

const XML_COMMENTS: [&str; 3] = [
    "codeStyle.xml.lineCommentAtFirstColumn",
    "codeStyle.xml.blockCommentAtFirstColumn",
    "codeStyle.xml.blockCommentAddSpace",
];

/// LevelChooserAction's severities, from the highest.
const SEVERITIES: &[&str] = &[
    "Error",
    "Warning",
    "Weak Warning",
    "Server Problem",
    "Typo",
    "Consideration",
    "No highlighting (fix available)",
];

// shortcut: the charsets IntelliJ's chooser lists first; its "More" submenu has every charset
const ENCODINGS: &[&str] = &[
    "UTF-8",
    "ISO-8859-1",
    "US-ASCII",
    "UTF-16",
    "UTF-16BE",
    "UTF-16LE",
    "windows-1252",
];

const PROPERTIES_ENCODINGS: &[&str] = &[
    "<Properties Default: UTF-8>",
    "UTF-8",
    "ISO-8859-1",
    "US-ASCII",
    "UTF-16",
    "UTF-16BE",
    "UTF-16LE",
    "windows-1252",
];

/// FileTypeManagerImpl.DEFAULT_IGNORED.
const IGNORED_FILES: &[&str] = &[
    "*.pyc",
    "*.pyo",
    "*.rbc",
    "*.yarb",
    "*~",
    ".DS_Store",
    ".git",
    ".hg",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".svn",
    "CVS",
    "__pycache__",
    "_svn",
    "vssver.scc",
    "vssver2.scc",
];

/// The reference app's file types (its `fileType` registrations but the internal tool window tab),
/// sorted by description as FileTypeConfigurable lists them: description, icon, patterns.
const FILE_TYPES: &[(&str, &str, &[&str])] = &[
    (
        ".gitignore (GitIgnore)",
        "fileTypes/gitignore",
        &["*.gitignore"],
    ),
    (".ignore (IgnoreLang)", "fileTypes/ignored", &["*.ignore"]),
    ("Analysis ignore", "fileTypes/ignored", &[".analysisignore"]),
    (
        "Archive",
        "fileTypes/archive",
        &[
            "*.ane", "*.apk", "*.ear", "*.egg", "*.jar", "*.swc", "*.war", "*.zip",
        ],
    ),
    ("exclude (GitExclude)", "fileTypes/gitignore", &["exclude"]),
    (
        "File type auto-detected by file content",
        "fileTypes/text",
        &[],
    ),
    (
        "Files opened in associated applications",
        "fileTypes/anyType",
        &[
            "*.doc", "*.docx", "*.hlp", "*.mdb", "*.odt", "*.pdf", "*.ppt", "*.pptx", "*.vsd",
        ],
    ),
    (
        "Files supported via TextMate bundles",
        "fileTypes/text",
        &[],
    ),
    (
        "HTML",
        "fileTypes/html",
        &["*.htm", "*.html", "*.sht", "*.shtm", "*.shtml"],
    ),
    ("IDEA module", "fileTypes/ideaModule", &["*.iml"]),
    ("IDEA project", "nodes/ideaProject", &["*.ipr"]),
    ("IDEA workspace", "nodes/ideaProject", &["*.iws"]),
    (
        "Image",
        "fileTypes/image",
        &[
            "*.bigtiff",
            "*.bmp",
            "*.dcx",
            "*.gif",
            "*.icns",
            "*.ico",
            "*.jbig2",
            "*.jpeg",
            "*.jpg",
            "*.pam",
            "*.pbm",
            "*.pcx",
            "*.pgm",
            "*.png",
            "*.pnm",
            "*.ppm",
            "*.psd",
            "*.rgbe",
            "*.tga",
            "*.tif",
            "*.tiff",
            "*.wbmp",
            "*.xbm",
            "*.xpm",
        ],
    ),
    ("Patch", "fileTypes/patch", &["*.diff", "*.patch"]),
    ("Regular expression", "fileTypes/regexp", &["*.regexp"]),
    ("RELAX NG compact syntax", "fileTypes/text", &["*.rnc"]),
    ("Scalable Vector Graphics", "fileTypes/image", &["*.svg"]),
    ("Spell checker dictionary", "fileTypes/text", &["*.dic"]),
    ("Text", "fileTypes/text", &["*.log", "*.txt"]),
    ("Unknown", "fileTypes/unknown", &[]),
    ("XHTML", "fileTypes/xhtml", &["*.xhtml"]),
    (
        "XML",
        "fileTypes/xml",
        &[
            "*.ant", "*.fxml", "*.jhm", "*.jnlp", "*.jrxml", "*.rng", "*.tld", "*.wsdl", "*.xml",
            "*.xsd", "*.xsl", "*.xslt", "*.xul",
        ],
    ),
    (
        "XML Document Type Definition",
        "fileTypes/xml",
        &["*.dtd", "*.elt", "*.ent", "*.mod"],
    ),
];

/// Inlay Hints ▸ Code vision: the reference app's code vision providers and group settings, by name:
/// name, key, description.
const CODE_VISION: &[(&str, &str, &str)] = &[
    (
        "Change signature",
        "inlayHints.codeVision.changeSignature",
        "Shown after changing the signature of a method. Click the hint to correct all references.",
    ),
    (
        "Code author",
        "inlayHints.codeVision.codeAuthor",
        "The author of the code according to the records in the VCS. The author is whoever last edited the class, method, or function. If there are multiple commits from different authors, the one who owns the most lines is considered the author.",
    ),
    (
        "Inheritors",
        "inlayHints.codeVision.inheritors",
        "The number of descendants the class or interface has. Click the hint to navigate to the code that inherits from this class or interface.",
    ),
    (
        "LSP Code Lens",
        "inlayHints.codeVision.lsp",
        "Displays code lens information from LSP servers",
    ),
    (
        "Related problems",
        "inlayHints.codeVision.relatedProblems",
        "The number of problems in the project related to the class or member signature. For example: missing parameters, wrong return type, and so on. The hint does not appear for problems within the same file.",
    ),
    (
        "Rename",
        "inlayHints.codeVision.rename",
        "Shown after renaming a symbol. Click the hint to rename usages of the symbol in the project.",
    ),
    (
        "Usages",
        "inlayHints.codeVision.usages",
        "The number of usages of the class or method in the project. Click the hint to navigate to the usages.",
    ),
];

/// XmlEmmetConfigurable's filters: the reference app's `xml.zenCodingFilter`s but the system XSL one.
const EMMET_FILTERS: &[(&str, &str)] = &[
    ("emmet.filter.comment", "Comment tags"),
    ("emmet.filter.escape", "Escape"),
    ("emmet.filter.singleLine", "Single line"),
    ("emmet.filter.bem", "BEM"),
    ("emmet.filter.trim", "Trim line markers"),
];

/// One of the reference app's intentions (`intentionAction` with a category).
struct Intention {
    name: &'static str,
    key: &'static str,
    description: &'static str,
}

/// The reference app's intentions by category, sorted as IntelliJ's tree.
const INTENTIONS: &[(&str, &[Intention])] = &[
    (
        "Language injection",
        &[Intention {
            name: "Edit injected fragment",
            key: "intentions.QuickEditAction",
            description: "Opens the injected fragment in a designated editor without outer language escaping and concatenations.",
        }],
    ),
    (
        "LightEdit mode",
        &[Intention {
            name: "Open file in project…",
            key: "intentions.LightEditOpenInProjectIntention",
            description: "Opens the current file in a project.",
        }],
    ),
    (
        "Microservices",
        &[Intention {
            name: "Open in web browser",
            key: "intentions.OpenInWebBrowserIntention",
            description: "Opens full URL in Web browser.",
        }],
    ),
    (
        "Other",
        &[
            Intention {
                name: "Choose template data language",
                key: "intentions.ChooseTemplateDataLanguageIntention",
                description: "Configure the preferred template data language for the current file.",
            },
            Intention {
                name: "Convert absolute path to relative",
                key: "intentions.ConvertAbsolutePathToRelativeIntentionAction",
                description: "Converts absolute paths to relative ones if applicable.",
            },
            Intention {
                name: "Convert relative path to absolute",
                key: "intentions.ConvertRelativePathToAbsoluteIntentionAction",
                description: "Converts relative paths to absolute ones if applicable.",
            },
            Intention {
                name: "Create new scratch file from selection",
                key: "intentions.ScratchFromSelectionIntention",
                description: "Creates a new scratch file from the selected fragment.",
            },
            Intention {
                name: "Flip comma",
                key: "intentions.FlipCommaIntention",
                description: "Swaps two elements separated by a comma.",
            },
            Intention {
                name: "Put elements on multiple lines",
                key: "intentions.SplitLineIntention",
                description: "Puts elements on multiple lines.",
            },
            Intention {
                name: "Put elements on one line",
                key: "intentions.JoinLinesIntention",
                description: "Puts elements on one line.",
            },
        ],
    ),
    (
        "RegExp",
        &[
            Intention {
                name: "Check RegExp",
                key: "intentions.CheckRegExpIntentionAction",
                description: "Allows you to check validity of regular expressions in a dedicated dialog.",
            },
            Intention {
                name: "Explain regular expression",
                key: "intentions.ExplainRegExpIntention",
                description: "Shows an explanation tree of a regular expression or the selected part of a regular expression.",
            },
            Intention {
                name: "Flip elements",
                key: "intentions.FlipElementsIntention",
                description: "Swaps two branches of an alternation or two elements of a character class.",
            },
        ],
    ),
    (
        "XML",
        &[
            Intention {
                name: "Add 'xsi:schemaLocation' attribute",
                key: "intentions.AddXsiSchemaLocationForExtResourceAction",
                description: "Adds an xsi:schemaLocation attribute for an external resource.",
            },
            Intention {
                name: "Choose color",
                key: "intentions.XmlChooseColorIntentionAction",
                description: "Opens the color picker for the value of an attribute.",
            },
            Intention {
                name: "Collapse empty tag",
                key: "intentions.CollapseTagIntention",
                description: "Replaces an opening tag and the corresponding closing tag with an equivalent self-closing tag.",
            },
            Intention {
                name: "Convert CDATA to text",
                key: "intentions.CDataToTextIntention",
                description: "Unwraps a text fragment from the CDATA section and escapes characters if necessary.",
            },
            Intention {
                name: "Convert text to CDATA",
                key: "intentions.TextToCDataIntention",
                description: "Wraps a text fragment in a CDATA section and removes escaping from characters if necessary.",
            },
            Intention {
                name: "Fetch external resource",
                key: "intentions.FetchExtResourceAction",
                description: "Downloads a resource with the specified URL from the internet.",
            },
            Intention {
                name: "Insert namespace prefix",
                key: "intentions.AddSchemaPrefixIntention",
                description: "Inserts a namespace prefix.",
            },
            Intention {
                name: "Manually set up external resource",
                key: "intentions.ManuallySetupExtResourceAction",
                description: "Maps a namespace to an XML schema file (DTD or XSD).",
            },
            Intention {
                name: "Rearrange tag attributes",
                key: "intentions.RearrangeAttributesIntention",
                description: "Rearranges the attributes of a tag. The order can be specified in Settings | Editor | Code Style | XML | Arrangement.",
            },
            Intention {
                name: "Remove attribute",
                key: "intentions.RemoveAttributeIntentionFix",
                description: "Removes an attribute from a tag.",
            },
            Intention {
                name: "Remove tag (preserves children)",
                key: "intentions.RemoveTagAndPromoteChildrenIntentionAction",
                description: "Removes an enclosing tag and promotes its children.",
            },
            Intention {
                name: "Reset to default namespace",
                key: "intentions.ConvertSchemaPrefixToDefaultIntention",
                description: "Removes the selected namespace prefix.",
            },
            Intention {
                name: "Split current tag",
                key: "intentions.XmlSplitTagAction",
                description: "Splits the tag at the caret.",
            },
        ],
    ),
];

/// One of the reference app's inspections.
struct Inspection {
    name: &'static str,
    enabled_key: &'static str,
    severity_key: &'static str,
    /// Its default severity, as the severity combo names it.
    severity: &'static str,
    enabled: bool,
    description: &'static str,
}

/// Where a live template applies, among the reference app's template contexts.
#[derive(Clone, Copy)]
enum Applies {
    HtmlXml,
    XmlText,
    HtmlText,
    XslText,
    /// Only in contexts the reference app lacks (JSX, TSX).
    None,
}

/// One of the reference app's live templates.
struct LiveTemplate {
    abbreviation: &'static str,
    key: &'static str,
    description: &'static str,
    text: &'static str,
    applies: Applies,
    expand_key: &'static str,
    expand: &'static str,
    reformat_key: &'static str,
    reformat: bool,
}

/// The reference app's inspections by group, as its tree sorts them: name, short name, default
/// severity, enabled by default, the description's first paragraph.
const INSPECTIONS: &[(&str, &[Inspection])] = &[
    (
        "General",
        &[
            Inspection {
                name: "Annotator",
                enabled_key: "inspections.Annotator",
                severity_key: "inspections.Annotator.severity",
                severity: "Error",
                enabled: true,
                description: "Reports issues essential to this file (e.g., syntax errors) in the result of a batch code inspection run. These issues are usually always highlighted in the editor and can't be configured, unlike inspections. These options control the scope of checks performed by this inspection: Option \"Report syntax errors\": report parser-related issues.; Option \"Report issues from language-specific annotators\": report issues found by annotators configured for the relevant language. See Custom Language Support: Annotators for details.; Option \"Report other highlighting problems\": report issues specific to the language of the current file (e.g., type mismatches or unreported exceptions). See Custom Language Support: Highlighting for details.",
            },
            Inspection {
                name: "Empty directory",
                enabled_key: "inspections.EmptyDirectory",
                severity_key: "inspections.EmptyDirectory.severity",
                severity: "Warning",
                enabled: false,
                description: "Reports empty directories.",
            },
            Inspection {
                name: "Inconsistent line separators",
                enabled_key: "inspections.InconsistentLineSeparators",
                severity_key: "inspections.InconsistentLineSeparators.severity",
                severity: "Warning",
                enabled: false,
                description: "Reports files with line separators different from the ones that are specified in the project's settings.",
            },
            Inspection {
                name: "Incorrect formatting",
                enabled_key: "inspections.IncorrectFormatting",
                severity_key: "inspections.IncorrectFormatting.severity",
                severity: "Weak Warning",
                enabled: false,
                description: "Reports formatting issues that appear if your code doesn't follow your project's code style settings.",
            },
            Inspection {
                name: "Line is longer than allowed by code style",
                enabled_key: "inspections.LongLine",
                severity_key: "inspections.LongLine.severity",
                severity: "Warning",
                enabled: false,
                description: "Reports lines that are longer than the Hard wrap at parameter specified in Settings | Editor | Code Style | General.",
            },
            Inspection {
                name: "Problematic whitespace",
                enabled_key: "inspections.ProblematicWhitespace",
                severity_key: "inspections.ProblematicWhitespace.severity",
                severity: "Warning",
                enabled: false,
                description: "Reports the following problems: Tabs used for indentation when the code style is configured to use only spaces.; Spaces used for indentation when the code style is configured to use only tabs.; Spaces used for indentation and tabs used for alignment when the code style is configured to use smart tabs.",
            },
            Inspection {
                name: "Reassigned to plain text",
                enabled_key: "inspections.ReassignedToPlainText",
                severity_key: "inspections.ReassignedToPlainText.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports files that were explicitly re-assigned to Plain Text File Type. This association is unnecessary because the platform auto-detects text files by content automatically.",
            },
            Inspection {
                name: "Redundant suppression",
                enabled_key: "inspections.RedundantSuppression",
                severity_key: "inspections.RedundantSuppression.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports usages of the following elements that can be safely removed because the inspection they affect is no longer applicable in this context: @SuppressWarning annotation, or; // noinspection line comment, or; /** noinspection */ JavaDoc comment",
            },
            Inspection {
                name: "TODO comment",
                enabled_key: "inspections.TodoComment",
                severity_key: "inspections.TodoComment.severity",
                severity: "Warning",
                enabled: false,
                description: "Reports TODO comments in your code.",
            },
        ],
    ),
    (
        "HTML",
        &[
            Inspection {
                name: "Empty tag",
                enabled_key: "inspections.CheckEmptyScriptTag",
                severity_key: "inspections.CheckEmptyScriptTag.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports empty tags that do not work in some browsers.",
            },
            Inspection {
                name: "Incorrect boolean attribute",
                enabled_key: "inspections.HtmlUnknownBooleanAttribute",
                severity_key: "inspections.HtmlUnknownBooleanAttribute.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports an HTML non-boolean attribute without a value. Suggests configuring attributes that should not be reported.",
            },
            Inspection {
                name: "Malformed content of 'script' tag",
                enabled_key: "inspections.CheckValidXmlInScriptTagBody",
                severity_key: "inspections.CheckValidXmlInScriptTagBody.severity",
                severity: "Error",
                enabled: true,
                description: "Reports contents of script tags that are invalid XML.",
            },
            Inspection {
                name: "Missing closing tag",
                enabled_key: "inspections.HtmlMissingClosingTag",
                severity_key: "inspections.HtmlMissingClosingTag.severity",
                severity: "No highlighting (fix available)",
                enabled: true,
                description: "Reports an HTML element without a closing tag. Some coding styles require that HTML elements have closing tags even where this is optional.",
            },
            Inspection {
                name: "Missing required attribute",
                enabled_key: "inspections.RequiredAttributes",
                severity_key: "inspections.RequiredAttributes.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports a missing mandatory attribute in an XML/HTML tag. Suggests configuring attributes that should not be reported.",
            },
            Inspection {
                name: "Redundant closing tag",
                enabled_key: "inspections.HtmlExtraClosingTag",
                severity_key: "inspections.HtmlExtraClosingTag.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports redundant closing tags on empty elements, for example, img or br.",
            },
            Inspection {
                name: "Unknown attribute",
                enabled_key: "inspections.HtmlUnknownAttribute",
                severity_key: "inspections.HtmlUnknownAttribute.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports an unknown HTML attribute. Suggests configuring attributes that should not be reported.",
            },
            Inspection {
                name: "Unknown tag",
                enabled_key: "inspections.HtmlUnknownTag",
                severity_key: "inspections.HtmlUnknownTag.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports an unknown HTML tag. Suggests configuring tags that should not be reported.",
            },
            Inspection {
                name: "Unresolved file in a link",
                enabled_key: "inspections.HtmlUnknownTarget",
                severity_key: "inspections.HtmlUnknownTarget.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports an unresolved file in a link.",
            },
            Inspection {
                name: "Unresolved fragment in a link",
                enabled_key: "inspections.HtmlUnknownAnchorTarget",
                severity_key: "inspections.HtmlUnknownAnchorTarget.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports an unresolved last part of an URL after the # sign.",
            },
            Inspection {
                name: "Wrong attribute value",
                enabled_key: "inspections.HtmlWrongAttributeValue",
                severity_key: "inspections.HtmlWrongAttributeValue.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports an incorrect HTML attribute value.",
            },
        ],
    ),
    (
        "Internationalization",
        &[
            Inspection {
                name: "Lossy encoding",
                enabled_key: "inspections.LossyEncoding",
                severity_key: "inspections.LossyEncoding.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports characters that cannot be displayed because of the current document encoding.",
            },
            Inspection {
                name: "Non-ASCII characters",
                enabled_key: "inspections.NonAsciiCharacters",
                severity_key: "inspections.NonAsciiCharacters.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports code elements that use non-ASCII symbols in an unusual context.",
            },
        ],
    ),
    (
        "RegExp",
        &[
            Inspection {
                name: "Anonymous capturing group or numeric back reference",
                enabled_key: "inspections.RegExpAnonymousGroup",
                severity_key: "inspections.RegExpAnonymousGroup.severity",
                severity: "Warning",
                enabled: false,
                description: "Reports anonymous capturing groups and numeric back references in a RegExp. These are only reported when the RegExp dialect supports named group and named group references. Named groups and named back references improve code readability and are recommended to use instead. When a capture is not needed, matching can be more performant and use less memory by using a non-capturing group, i.e. (?:xxx) instead of (xxx).",
            },
            Inspection {
                name: "Begin or end anchor in unexpected position",
                enabled_key: "inspections.RegExpUnexpectedAnchor",
                severity_key: "inspections.RegExpUnexpectedAnchor.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports ^ or \\A anchors not at the beginning of the pattern and $, \\Z or \\z anchors not at the end of the pattern. In the wrong position these RegExp anchors prevent the pattern from matching anything. In case of the ^ and $ anchors, most likely the literal character was meant and the escape forgotten.",
            },
            Inspection {
                name: "Consecutive spaces",
                enabled_key: "inspections.RegExpRepeatedSpace",
                severity_key: "inspections.RegExpRepeatedSpace.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports multiple consecutive spaces in a RegExp. Because spaces are not visible by default, it can be hard to see how many spaces are required. The RegExp can be made more clear by replacing the consecutive spaces with a single space and a counted quantifier.",
            },
            Inspection {
                name: "Duplicate branch in alternation",
                enabled_key: "inspections.RegExpDuplicateAlternationBranch",
                severity_key: "inspections.RegExpDuplicateAlternationBranch.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports duplicate branches in a RegExp alternation. Duplicate branches slow down matching and obscure the intent of the expression.",
            },
            Inspection {
                name: "Duplicate character in character class",
                enabled_key: "inspections.RegExpDuplicateCharacterInClass",
                severity_key: "inspections.RegExpDuplicateCharacterInClass.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports duplicate characters inside a RegExp character class. Duplicate characters are unnecessary and can be removed without changing the semantics of the regex.",
            },
            Inspection {
                name: "Empty branch in alternation",
                enabled_key: "inspections.RegExpEmptyAlternationBranch",
                severity_key: "inspections.RegExpEmptyAlternationBranch.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports empty branches in a RegExp alternation. An empty branch will only match the empty string, and in most cases that is not what is desired. This inspection will not report a single empty branch at the start or the end of an alternation.",
            },
            Inspection {
                name: "Escaped meta character",
                enabled_key: "inspections.RegExpEscapedMetaCharacter",
                severity_key: "inspections.RegExpEscapedMetaCharacter.severity",
                severity: "No highlighting (fix available)",
                enabled: true,
                description: "Reports escaped meta characters. Some RegExp coding styles specify that meta characters should be placed inside a character class, to make the regular expression easier to understand. This inspection does not warn about the meta character [, ] and ^, because those would need additional escaping inside a character class.",
            },
            Inspection {
                name: "Octal escape",
                enabled_key: "inspections.RegExpOctalEscape",
                severity_key: "inspections.RegExpOctalEscape.severity",
                severity: "No highlighting (fix available)",
                enabled: true,
                description: "Reports octal escapes, which are easily confused with back references. Use hexadecimal escapes to avoid confusion.",
            },
            Inspection {
                name: "Redundant '\\\\d', '[:digit:]', or '\\\\D' class elements",
                enabled_key: "inspections.RegExpRedundantClassElement",
                severity_key: "inspections.RegExpRedundantClassElement.severity",
                severity: "Weak Warning",
                enabled: true,
                description: "Reports redundant \\d or [:digit:] that are used in one class with \\w or [:word:] (\\D with \\W) and can be removed.",
            },
            Inspection {
                name: "Redundant character escape",
                enabled_key: "inspections.RegExpRedundantEscape",
                severity_key: "inspections.RegExpRedundantEscape.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports redundant character escape sequences that can be replaced with unescaped characters preserving the meaning. Many escape sequences that are necessary outside of a character class are redundant inside square brackets [] of a character class.",
            },
            Inspection {
                name: "Redundant nested character class",
                enabled_key: "inspections.RegExpRedundantNestedCharacterClass",
                severity_key: "inspections.RegExpRedundantNestedCharacterClass.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports unnecessary nested character classes.",
            },
            Inspection {
                name: "Regular expression can be simplified",
                enabled_key: "inspections.RegExpSimplifiable",
                severity_key: "inspections.RegExpSimplifiable.severity",
                severity: "Weak Warning",
                enabled: true,
                description: "Reports regular expressions that can be simplified.",
            },
            Inspection {
                name: "Single character alternation",
                enabled_key: "inspections.RegExpSingleCharAlternation",
                severity_key: "inspections.RegExpSingleCharAlternation.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports single char alternation in a RegExp. It is simpler to use a character class instead. This may also provide better matching performance.",
            },
            Inspection {
                name: "Suspicious back reference",
                enabled_key: "inspections.RegExpSuspiciousBackref",
                severity_key: "inspections.RegExpSuspiciousBackref.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports back references that will not be resolvable at runtime. This means that the back reference can never match anything. A back reference will not be resolvable when the group is defined after the back reference, or if the group is defined in a different branch of an alternation.",
            },
            Inspection {
                name: "Unnecessary non-capturing group",
                enabled_key: "inspections.RegExpUnnecessaryNonCapturingGroup",
                severity_key: "inspections.RegExpUnnecessaryNonCapturingGroup.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports unnecessary non-capturing groups, which have no influence on the match result.",
            },
        ],
    ),
    (
        "RELAX NG",
        &[
            Inspection {
                name: "Unresolved reference",
                enabled_key: "inspections.UnresolvedReference",
                severity_key: "inspections.UnresolvedReference.severity",
                severity: "Error",
                enabled: true,
                description: "Reports an unresolved reference to a named pattern (define) in RELAX-NG files that use XML syntax. Suggests creating the referenced define element.",
            },
            Inspection {
                name: "Unused define",
                enabled_key: "inspections.UnusedDefine",
                severity_key: "inspections.UnusedDefine.severity",
                severity: "Warning",
                enabled: false,
                description: "Reports an unused named pattern (define) in a RELAX-NG file (XML or Compact Syntax). define elements that are used through an include in another file are ignored.",
            },
        ],
    ),
    (
        "Version control",
        &[Inspection {
            name: "Ignore file duplicates",
            enabled_key: "inspections.IgnoreFileDuplicateEntry",
            severity_key: "inspections.IgnoreFileDuplicateEntry.severity",
            severity: "Warning",
            enabled: true,
            description: "Reports duplicate entries (patterns) in the ignore file (e.g. .gitignore, .hgignore). Duplicate entries in these files are redundant and can be removed.",
        }],
    ),
    (
        "XML",
        &[
            Inspection {
                name: "Deprecated symbol",
                enabled_key: "inspections.XmlDeprecatedElement",
                severity_key: "inspections.XmlDeprecatedElement.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports a deprecated XML element or attribute.",
            },
            Inspection {
                name: "Duplicate 'id' attribute",
                enabled_key: "inspections.XmlDuplicatedId",
                severity_key: "inspections.XmlDuplicatedId.severity",
                severity: "Error",
                enabled: true,
                description: "Reports a duplicate values of the id attribute in XML and HTML.",
            },
            Inspection {
                name: "Empty element content",
                enabled_key: "inspections.CheckTagEmptyBody",
                severity_key: "inspections.CheckTagEmptyBody.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports XML elements without contents.",
            },
            Inspection {
                name: "Failed external validation",
                enabled_key: "inspections.CheckXmlFileWithXercesValidator",
                severity_key: "inspections.CheckXmlFileWithXercesValidator.severity",
                severity: "Error",
                enabled: true,
                description: "Reports a discrepancy in an XML file with the specified DTD or schema detected by the Xerces validator.",
            },
            Inspection {
                name: "Redundant attribute with default value",
                enabled_key: "inspections.XmlDefaultAttributeValue",
                severity_key: "inspections.XmlDefaultAttributeValue.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports a redundant assignment of the default value to an XML attribute.",
            },
            Inspection {
                name: "Unbound namespace prefix",
                enabled_key: "inspections.XmlUnboundNsPrefix",
                severity_key: "inspections.XmlUnboundNsPrefix.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports an unbound namespace prefix in XML.",
            },
            Inspection {
                name: "Unresolved 'id' reference",
                enabled_key: "inspections.XmlInvalidId",
                severity_key: "inspections.XmlInvalidId.severity",
                severity: "Error",
                enabled: true,
                description: "Reports the use of the id that is not defined anywhere in XML and HTML.",
            },
            Inspection {
                name: "Unresolved DTD reference",
                enabled_key: "inspections.CheckDtdRefs",
                severity_key: "inspections.CheckDtdRefs.severity",
                severity: "Error",
                enabled: true,
                description: "Reports inconsistency in a DTD-specific reference, for example, in a reference to an XML entity or to a DTD element declaration. Works in DTD an XML files.",
            },
            Inspection {
                name: "Unresolved file reference",
                enabled_key: "inspections.XmlPathReference",
                severity_key: "inspections.XmlPathReference.severity",
                severity: "Error",
                enabled: true,
                description: "Reports an unresolved file reference in XML.",
            },
            Inspection {
                name: "Unresolved references",
                enabled_key: "inspections.XmlUnresolvedReference",
                severity_key: "inspections.XmlUnresolvedReference.severity",
                severity: "Error",
                enabled: true,
                description: "Reports an unresolved references in XML.",
            },
            Inspection {
                name: "Unused schema declaration",
                enabled_key: "inspections.XmlUnusedNamespaceDeclaration",
                severity_key: "inspections.XmlUnusedNamespaceDeclaration.severity",
                severity: "Warning",
                enabled: true,
                description: "Reports an unused namespace declaration or location hint in XML.",
            },
            Inspection {
                name: "Wrong root element",
                enabled_key: "inspections.XmlWrongRootElement",
                severity_key: "inspections.XmlWrongRootElement.severity",
                severity: "Error",
                enabled: true,
                description: "Reports a root tag name different from the name specified in the <doctype> tag.",
            },
            Inspection {
                name: "XML highlighting",
                enabled_key: "inspections.XmlHighlighting",
                severity_key: "inspections.XmlHighlighting.severity",
                severity: "Error",
                enabled: true,
                description: "Reports XML validation problems in the results of a batch code inspection.",
            },
        ],
    ),
];

/// The reference app's live templates (`liveTemplates/*.xml`) by group: abbreviation, description,
/// template text, where it applies.
const LIVE_TEMPLATES: &[(&str, &[LiveTemplate])] = &[
    (
        "HTML/XML",
        &[
            LiveTemplate {
                abbreviation: "CD",
                key: "liveTemplates.HTML/XML.CD",
                description: "Surround with CDATA section",
                text: "<![CDATA[\n$SELECTION$\n]]>",
                applies: Applies::XmlText,
                expand_key: "liveTemplates.HTML/XML.CD.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.HTML/XML.CD.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "T",
                key: "liveTemplates.HTML/XML.T",
                description: "Surround with <tag></tag>",
                text: "<$TAG$>$SELECTION$</$TAGNAME$>",
                applies: Applies::HtmlXml,
                expand_key: "liveTemplates.HTML/XML.T.expand",
                expand: "Tab",
                reformat_key: "liveTemplates.HTML/XML.T.reformat",
                reformat: true,
            },
        ],
    ),
    (
        "Zen HTML",
        &[
            LiveTemplate {
                abbreviation: "!",
                key: "liveTemplates.Zen HTML.!",
                description: "",
                text: "<!doctype html>\n<html lang=\"$ENV_LOCALE$\">\n<head>\n<meta charset=\"$CHARSET$\">\n             <meta name=\"viewport\" content=\"width=$WIDTH$, user-scalable=$SCALABLE$, initial-scale=$INITIAL_SCALE$, maximum-scale=$MAX_SCALE$, minimum-scale=$MIN_SCALE$\">\n                         <meta http-equiv=\"X-UA-Compatible\" content=\"ie=$IE$\">\n             <title>$TITLE$</title>\n</head>\n<body>\n  $END$\n</body>\n</html>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.!.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.!.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "!!!",
                key: "liveTemplates.Zen HTML.!!!",
                description: "<!doctype html>",
                text: "<!doctype html>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.!!!.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.!!!.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "!!!4s",
                key: "liveTemplates.Zen HTML.!!!4s",
                description: "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01//EN\" \"http://www.w3.org/TR/html4/strict.dtd\">",
                text: "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01//EN\" \"http://www.w3.org/TR/html4/strict.dtd\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.!!!4s.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.!!!4s.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "!!!4t",
                key: "liveTemplates.Zen HTML.!!!4t",
                description: "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\" \"http://www.w3.org/TR/html4/loose.dtd\">",
                text: "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\" \"http://www.w3.org/TR/html4/loose.dtd\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.!!!4t.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.!!!4t.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "!!!xs",
                key: "liveTemplates.Zen HTML.!!!xs",
                description: "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Strict//EN\" \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd\">",
                text: "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Strict//EN\" \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.!!!xs.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.!!!xs.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "!!!xt",
                key: "liveTemplates.Zen HTML.!!!xt",
                description: "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\" \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd\">",
                text: "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\" \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.!!!xt.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.!!!xt.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "!!!xxs",
                key: "liveTemplates.Zen HTML.!!!xxs",
                description: "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.1//EN\" \"http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd\">",
                text: "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.1//EN\" \"http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.!!!xxs.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.!!!xxs.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "a",
                key: "liveTemplates.Zen HTML.a",
                description: "<a href=\"...\">...</a>",
                text: "<a href=\"$VAR0$\">$END$</a>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.a.expand",
                expand: "None",
                reformat_key: "liveTemplates.Zen HTML.a.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "a:link",
                key: "liveTemplates.Zen HTML.a:link",
                description: "<a href=\"http://...\">...</a>",
                text: "<a href=\"http://$VAR0$\">$END$</a>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.a:link.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.a:link.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "a:mail",
                key: "liveTemplates.Zen HTML.a:mail",
                description: "<a href=\"mailto:...\">...</a>",
                text: "<a href=\"mailto:$VAR0$\">$END$</a>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.a:mail.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.a:mail.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "a:tel",
                key: "liveTemplates.Zen HTML.a:tel",
                description: "<a href=\"tel:+...\">...</a>",
                text: "<a href=\"tel:+$VAR0$\">$END$</a>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.a:tel.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.a:tel.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "abbr",
                key: "liveTemplates.Zen HTML.abbr",
                description: "<abbr title=\"...\">...</abbr>",
                text: "<abbr title=\"$VAR0$\">$END$</abbr>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.abbr.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.abbr.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "acr",
                key: "liveTemplates.Zen HTML.acr",
                description: "<acronym title=\"...\">...</acronym>",
                text: "<acronym title=\"$VAR0$\">$END$</acronym>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.acr.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.acr.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "acronym",
                key: "liveTemplates.Zen HTML.acronym",
                description: "<acronym title=\"...\">...</acronym>",
                text: "<acronym title=\"$VAR0$\">$END$</acronym>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.acronym.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.acronym.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "adr",
                key: "liveTemplates.Zen HTML.adr",
                description: "<address>...</address>",
                text: "<address>$END$</address>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.adr.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.adr.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "area",
                key: "liveTemplates.Zen HTML.area",
                description: "<area shape=\"...\" coords=\"...\" href=\"...\" alt=\"...\">",
                text: "<area shape=\"$VAR0$\" coords=\"$VAR1$\" href=\"$VAR2$\" alt=\"$VAR3$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.area.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.area.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "area:c",
                key: "liveTemplates.Zen HTML.area:c",
                description: "<area shape=\"circle\" coords=\"...\" href=\"...\" alt=\"...\">",
                text: "<area shape=\"circle\" coords=\"$VAR0$\" href=\"$VAR1$\" alt=\"$VAR2$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.area:c.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.area:c.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "area:d",
                key: "liveTemplates.Zen HTML.area:d",
                description: "<area shape=\"default\" coords=\"...\" href=\"...\" alt=\"...\">",
                text: "<area shape=\"default\" coords=\"$COORDS$\" href=\"$VAR0$\" alt=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.area:d.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.area:d.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "area:p",
                key: "liveTemplates.Zen HTML.area:p",
                description: "<area shape=\"poly\" coords=\"...\" href=\"...\" alt=\"...\">",
                text: "<area shape=\"poly\" coords=\"$VAR0$\" href=\"$VAR1$\" alt=\"$VAR2$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.area:p.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.area:p.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "area:r",
                key: "liveTemplates.Zen HTML.area:r",
                description: "<area shape=\"rect\" coords=\"...\" href=\"...\" alt=\"...\">",
                text: "<area shape=\"rect\" coords=\"$VAR0$\" href=\"$VAR1$\" alt=\"$VAR2$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.area:r.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.area:r.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "art",
                key: "liveTemplates.Zen HTML.art",
                description: "<article>...</article>",
                text: "<article>$END$</article>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.art.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.art.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "audio",
                key: "liveTemplates.Zen HTML.audio",
                description: "<audio src=\"...\">...</audio>",
                text: "<audio src=\"$VAR0$\">$END$</audio>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.audio.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.audio.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "b",
                key: "liveTemplates.Zen HTML.b",
                description: "<b>...</b>",
                text: "<b>$END$</b>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.b.expand",
                expand: "None",
                reformat_key: "liveTemplates.Zen HTML.b.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "base",
                key: "liveTemplates.Zen HTML.base",
                description: "<base href=\"...\">",
                text: "<base href=\"$VAR0$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.base.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.base.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "basefont",
                key: "liveTemplates.Zen HTML.basefont",
                description: "<basefont>",
                text: "<basefont>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.basefont.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.basefont.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "bdo",
                key: "liveTemplates.Zen HTML.bdo",
                description: "<bdo dir=\"...\">...</bdo>",
                text: "<bdo dir=\"$VAR0$\">$END$</bdo>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.bdo.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.bdo.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "bdo:l",
                key: "liveTemplates.Zen HTML.bdo:l",
                description: "<bdo dir=\"ltr\">...</bdo>",
                text: "<bdo dir=\"ltr\">$END$</bdo>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.bdo:l.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.bdo:l.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "bdo:r",
                key: "liveTemplates.Zen HTML.bdo:r",
                description: "<bdo dir=\"rtl\">...</bdo>",
                text: "<bdo dir=\"rtl\">$END$</bdo>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.bdo:r.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.bdo:r.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "bq",
                key: "liveTemplates.Zen HTML.bq",
                description: "<blockquote>...</blockquote>",
                text: "<blockquote>$END$</blockquote>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.bq.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.bq.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "br",
                key: "liveTemplates.Zen HTML.br",
                description: "<br>",
                text: "<br>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.br.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.br.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "btn",
                key: "liveTemplates.Zen HTML.btn",
                description: "<button>...</button>",
                text: "<button>$END$</button>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.btn.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.btn.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "btn:b",
                key: "liveTemplates.Zen HTML.btn:b",
                description: "<button type=\"button\">...</button>",
                text: "<button type=\"button\">$END$</button>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.btn:b.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.btn:b.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "btn:d",
                key: "liveTemplates.Zen HTML.btn:d",
                description: "<button disabled>...</button>",
                text: "<button disabled>$END$</button>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.btn:d.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.btn:d.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "btn:r",
                key: "liveTemplates.Zen HTML.btn:r",
                description: "<button type=\"reset\">...</button>",
                text: "<button type=\"reset\">$END$</button>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.btn:r.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.btn:r.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "btn:s",
                key: "liveTemplates.Zen HTML.btn:s",
                description: "<button type=\"submit\">...</button>",
                text: "<button type=\"submit\">$END$</button>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.btn:s.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.btn:s.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "button:d",
                key: "liveTemplates.Zen HTML.button:d",
                description: "<button disabled>...</button>",
                text: "<button disabled>$END$</button>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.button:d.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.button:d.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "button:disabled",
                key: "liveTemplates.Zen HTML.button:disabled",
                description: "<button disabled>...</button>",
                text: "<button disabled>$END$</button>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.button:disabled.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.button:disabled.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "button:r",
                key: "liveTemplates.Zen HTML.button:r",
                description: "<button type=\"reset\"></button>",
                text: "<button type=\"reset\">$END$</button>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.button:r.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.button:r.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "button:reset",
                key: "liveTemplates.Zen HTML.button:reset",
                description: "<button type=\"reset\"></button>",
                text: "<button type=\"reset\">$END$</button>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.button:reset.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.button:reset.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "button:s",
                key: "liveTemplates.Zen HTML.button:s",
                description: "<button type=\"submit\"></button>",
                text: "<button type=\"submit\">$END$</button>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.button:s.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.button:s.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "button:submit",
                key: "liveTemplates.Zen HTML.button:submit",
                description: "<button type=\"submit\"></button>",
                text: "<button type=\"submit\">$END$</button>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.button:submit.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.button:submit.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "c",
                key: "liveTemplates.Zen HTML.c",
                description: "<!-- -->",
                text: "<!-- $END$ -->",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.c.expand",
                expand: "None",
                reformat_key: "liveTemplates.Zen HTML.c.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "cap",
                key: "liveTemplates.Zen HTML.cap",
                description: "<caption>...</caption>",
                text: "<caption>$END$</caption>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.cap.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.cap.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "cc:ie",
                key: "liveTemplates.Zen HTML.cc:ie",
                description: "",
                text: "<!--[if IE]>\n  $END$\n<![endif]-->",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.cc:ie.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.cc:ie.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "cc:ie6",
                key: "liveTemplates.Zen HTML.cc:ie6",
                description: "",
                text: "<!--[if lte IE 6]>\n  $END$\n<![endif]-->",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.cc:ie6.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.cc:ie6.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "cc:noie",
                key: "liveTemplates.Zen HTML.cc:noie",
                description: "",
                text: "<!--[if !IE]><!-->\n  $END$\n<!--<![endif]-->",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.cc:noie.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.cc:noie.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "cmd",
                key: "liveTemplates.Zen HTML.cmd",
                description: "<command>",
                text: "<command>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.cmd.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.cmd.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "col",
                key: "liveTemplates.Zen HTML.col",
                description: "<col>",
                text: "<col>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.col.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.col.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "colg",
                key: "liveTemplates.Zen HTML.colg",
                description: "<colgroup>...</colgroup>",
                text: "<colgroup>$END$</colgroup>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.colg.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.colg.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "colg+",
                key: "liveTemplates.Zen HTML.colg+",
                description: "",
                text: "<colgroup>\n<col>\n</colgroup>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.colg+.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.colg+.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "colgroup+",
                key: "liveTemplates.Zen HTML.colgroup+",
                description: "",
                text: "<colgroup>\n<col>\n</colgroup>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.colgroup+.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.colgroup+.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "command",
                key: "liveTemplates.Zen HTML.command",
                description: "<command>",
                text: "<command>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.command.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.command.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "datag",
                key: "liveTemplates.Zen HTML.datag",
                description: "<datagrid>...</datagrid>",
                text: "<datagrid>$END$</datagrid>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.datag.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.datag.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "datal",
                key: "liveTemplates.Zen HTML.datal",
                description: "<datalist>...</datalist>",
                text: "<datalist>$END$</datalist>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.datal.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.datal.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "det",
                key: "liveTemplates.Zen HTML.det",
                description: "<details>...</details>",
                text: "<details>$END$</details>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.det.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.det.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "div",
                key: "liveTemplates.Zen HTML.div",
                description: "<div>...</div>",
                text: "<div>$END$</div>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.div.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.div.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "dl+",
                key: "liveTemplates.Zen HTML.dl+",
                description: "",
                text: "<dl>\n<dt>$VAR0$</dt>\n<dd>$END$</dd>\n</dl>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.dl+.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.dl+.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "dlg",
                key: "liveTemplates.Zen HTML.dlg",
                description: "<dialog>...</dialog>",
                text: "<dialog>$END$</dialog>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.dlg.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.dlg.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "doc",
                key: "liveTemplates.Zen HTML.doc",
                description: "",
                text: "<html lang=\"en\">\n<head>\n<meta charset=\"$CHARSET$\">\n             <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n             <title>$TITLE$</title>\n</head>\n<body>\n  $END$\n</body>\n</html>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.doc.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.doc.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "doc4",
                key: "liveTemplates.Zen HTML.doc4",
                description: "",
                text: "<html>\n<head>\n<meta http-equiv=\"Content-Type\" content=\"text/html;charset=UTF-8\">\n<title>$TITLE$</title>\n</head>\n<body>\n  $END$\n</body>\n</html>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.doc4.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.doc4.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "emb",
                key: "liveTemplates.Zen HTML.emb",
                description: "<embed src=\"...\" type=\"...\">",
                text: "<embed src=\"$VAR0$\" type=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.emb.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.emb.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "embed",
                key: "liveTemplates.Zen HTML.embed",
                description: "<embed src=\"...\" type=\"...\">",
                text: "<embed src=\"$VAR0$\" type=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.embed.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.embed.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "fieldset:d",
                key: "liveTemplates.Zen HTML.fieldset:d",
                description: "<fieldset disabled>...</fieldset>",
                text: "<fieldset disabled>$END$</fieldset>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.fieldset:d.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.fieldset:d.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "fieldset:disabled",
                key: "liveTemplates.Zen HTML.fieldset:disabled",
                description: "<fieldset disabled>...</fieldset>",
                text: "<fieldset disabled>$END$</fieldset>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.fieldset:disabled.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.fieldset:disabled.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "fig",
                key: "liveTemplates.Zen HTML.fig",
                description: "<figure>...</figure>",
                text: "<figure>$END$</figure>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.fig.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.fig.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "figc",
                key: "liveTemplates.Zen HTML.figc",
                description: "<figcaption>...</figcaption>",
                text: "<figcaption>$END$</figcaption>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.figc.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.figc.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "form",
                key: "liveTemplates.Zen HTML.form",
                description: "<form action=\"...\">...</form>",
                text: "<form action=\"$VAR0$\">$END$</form>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.form.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.form.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "form:get",
                key: "liveTemplates.Zen HTML.form:get",
                description: "<form action=\"...\" method=\"get\">...</form>",
                text: "<form action=\"$VAR0$\" method=\"get\">$END$</form>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.form:get.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.form:get.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "form:post",
                key: "liveTemplates.Zen HTML.form:post",
                description: "<form action=\"...\" method=\"post\">...</form>",
                text: "<form action=\"$VAR0$\" method=\"post\">$END$</form>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.form:post.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.form:post.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "frame",
                key: "liveTemplates.Zen HTML.frame",
                description: "<frame>",
                text: "<frame>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.frame.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.frame.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "fset",
                key: "liveTemplates.Zen HTML.fset",
                description: "<fieldset>...</fieldset>",
                text: "<fieldset>$END$</fieldset>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.fset.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.fset.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "fset:d",
                key: "liveTemplates.Zen HTML.fset:d",
                description: "<fieldset disabled>...</fieldset>",
                text: "<fieldset disabled>$END$</fieldset>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.fset:d.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.fset:d.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "fst",
                key: "liveTemplates.Zen HTML.fst",
                description: "<fieldset>...</fieldset>",
                text: "<fieldset>$END$</fieldset>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.fst.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.fst.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "fst:d",
                key: "liveTemplates.Zen HTML.fst:d",
                description: "<fieldset disabled>...</fieldset>",
                text: "<fieldset disabled>$END$</fieldset>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.fst:d.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.fst:d.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "ftr",
                key: "liveTemplates.Zen HTML.ftr",
                description: "<footer>...</footer>",
                text: "<footer>$END$</footer>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.ftr.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.ftr.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "h1",
                key: "liveTemplates.Zen HTML.h1",
                description: "<h1>...</h1>",
                text: "<h1>$END$</h1>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.h1.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.h1.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "h2",
                key: "liveTemplates.Zen HTML.h2",
                description: "<h2>...</h2>",
                text: "<h2>$END$</h2>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.h2.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.h2.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "h3",
                key: "liveTemplates.Zen HTML.h3",
                description: "<h3>...</h3>",
                text: "<h3>$END$</h3>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.h3.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.h3.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "h4",
                key: "liveTemplates.Zen HTML.h4",
                description: "<h4>...</h4>",
                text: "<h4>$END$</h4>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.h4.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.h4.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "h5",
                key: "liveTemplates.Zen HTML.h5",
                description: "<h5>...</h5>",
                text: "<h5>$END$</h5>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.h5.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.h5.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "h6",
                key: "liveTemplates.Zen HTML.h6",
                description: "<h6>...</h6>",
                text: "<h6>$END$</h6>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.h6.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.h6.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "hdr",
                key: "liveTemplates.Zen HTML.hdr",
                description: "<header>...</header>",
                text: "<header>$END$</header>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.hdr.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.hdr.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "head",
                key: "liveTemplates.Zen HTML.head",
                description: "<head>...</head>",
                text: "<head>$END$</head>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.head.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.head.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "header",
                key: "liveTemplates.Zen HTML.header",
                description: "<header>...</header>",
                text: "<header>$END$</header>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.header.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.header.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "hr",
                key: "liveTemplates.Zen HTML.hr",
                description: "<hr>",
                text: "<hr>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.hr.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.hr.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "html:5",
                key: "liveTemplates.Zen HTML.html:5",
                description: "",
                text: "<!doctype html>\n<html lang=\"$ENV_LOCALE$\">\n<head>\n<meta charset=\"$CHARSET$\">\n             <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n             <title>$TITLE$</title>\n</head>\n<body>\n  $END$\n</body>\n</html>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.html:5.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.html:5.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "i",
                key: "liveTemplates.Zen HTML.i",
                description: "<i>...</i>",
                text: "<i>$END$</i>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.i.expand",
                expand: "None",
                reformat_key: "liveTemplates.Zen HTML.i.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "ifr",
                key: "liveTemplates.Zen HTML.ifr",
                description: "<iframe src=\"...\" frameborder=\"0\">...</iframe>",
                text: "<iframe src=\"$VAR0$\" frameborder=\"0\">$END$</iframe>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.ifr.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.ifr.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "iframe",
                key: "liveTemplates.Zen HTML.iframe",
                description: "<iframe src=\"...\" frameborder=\"0\">...</iframe>",
                text: "<iframe src=\"$VAR0$\" frameborder=\"0\">$END$</iframe>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.iframe.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.iframe.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "img",
                key: "liveTemplates.Zen HTML.img",
                description: "<img src=\"...\" alt=\"...\">",
                text: "<img src=\"$VAR0$\" alt=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.img.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.img.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "img:s",
                key: "liveTemplates.Zen HTML.img:s",
                description: "<img src=\"...\" alt=\"...\" srcset=\"...\">",
                text: "<img src=\"$VAR1$\" alt=\"$VAR2$\" srcset=\"$VAR0$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.img:s.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.img:s.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "img:sizes",
                key: "liveTemplates.Zen HTML.img:sizes",
                description: "<img src=\"...\" alt=\"...\" sizes=\"...\" srcset=\"...\">",
                text: "<img src=\"$VAR2$\" alt=\"$VAR3$\" sizes=\"$VAR0$\" srcset=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.img:sizes.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.img:sizes.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "img:srcset",
                key: "liveTemplates.Zen HTML.img:srcset",
                description: "<img src=\"...\" alt=\"...\" srcset=\"...\">",
                text: "<img src=\"$VAR1$\" alt=\"$VAR2$\" srcset=\"$VAR0$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.img:srcset.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.img:srcset.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "img:z",
                key: "liveTemplates.Zen HTML.img:z",
                description: "<img src=\"...\" alt=\"...\" sizes=\"...\" srcset=\"...\">",
                text: "<img src=\"$VAR2$\" alt=\"$VAR3$\" sizes=\"$VAR0$\" srcset=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.img:z.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.img:z.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "inp",
                key: "liveTemplates.Zen HTML.inp",
                description: "<input type=\"...\" name=\"...\" id=\"...\">",
                text: "<input type=\"$VAR0$\" name=\"$NAME$\" id=\"$ID$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.inp.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.inp.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input",
                key: "liveTemplates.Zen HTML.input",
                description: "<input type=\"...\">",
                text: "<input type=\"$VAR0$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:b",
                key: "liveTemplates.Zen HTML.input:b",
                description: "<input type=\"button\" value=\"...\">",
                text: "<input type=\"button\" value=\"$VAR0$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:b.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:b.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:button",
                key: "liveTemplates.Zen HTML.input:button",
                description: "<input type=\"button\" value=\"...\">",
                text: "<input type=\"button\" value=\"$VAR0$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:button.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:button.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:c",
                key: "liveTemplates.Zen HTML.input:c",
                description: "<input type=\"checkbox\" name=\"...\" id=\"...\">",
                text: "<input type=\"checkbox\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:c.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:c.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:checkbox",
                key: "liveTemplates.Zen HTML.input:checkbox",
                description: "<input type=\"checkbox\" name=\"...\" id=\"...\">",
                text: "<input type=\"checkbox\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:checkbox.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:checkbox.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:color",
                key: "liveTemplates.Zen HTML.input:color",
                description: "<input type=\"color\" name=\"...\" id=\"...\">",
                text: "<input type=\"color\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:color.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:color.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:date",
                key: "liveTemplates.Zen HTML.input:date",
                description: "<input type=\"date\" name=\"...\" id=\"...\">",
                text: "<input type=\"date\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:date.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:date.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:datetime",
                key: "liveTemplates.Zen HTML.input:datetime",
                description: "<input type=\"datetime\" name=\"...\" id=\"...\">",
                text: "<input type=\"datetime\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:datetime.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:datetime.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:datetime-local",
                key: "liveTemplates.Zen HTML.input:datetime-local",
                description: "<input type=\"datetime-local\" name=\"...\" id=\"...\">",
                text: "<input type=\"datetime-local\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:datetime-local.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:datetime-local.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:email",
                key: "liveTemplates.Zen HTML.input:email",
                description: "<input type=\"email\" name=\"...\" id=\"...\">",
                text: "<input type=\"email\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:email.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:email.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:f",
                key: "liveTemplates.Zen HTML.input:f",
                description: "<input type=\"file\" name=\"...\" id=\"...\">",
                text: "<input type=\"file\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:f.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:f.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:file",
                key: "liveTemplates.Zen HTML.input:file",
                description: "<input type=\"file\" name=\"...\" id=\"...\">",
                text: "<input type=\"file\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:file.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:file.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:h",
                key: "liveTemplates.Zen HTML.input:h",
                description: "<input type=\"hidden\" name=\"...\">",
                text: "<input type=\"hidden\" name=\"$VAR0$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:h.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:h.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:hidden",
                key: "liveTemplates.Zen HTML.input:hidden",
                description: "<input type=\"hidden\" name=\"...\">",
                text: "<input type=\"hidden\" name=\"$VAR0$\" >",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:hidden.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:hidden.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:i",
                key: "liveTemplates.Zen HTML.input:i",
                description: "<input type=\"image\" src=\"...\" alt=\"...\">",
                text: "<input type=\"image\" src=\"$VAR0$\" alt=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:i.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:i.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:image",
                key: "liveTemplates.Zen HTML.input:image",
                description: "<input type=\"image\" src=\"...\" alt=\"...\">",
                text: "<input type=\"image\" src=\"$VAR0$\" alt=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:image.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:image.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:month",
                key: "liveTemplates.Zen HTML.input:month",
                description: "<input type=\"month\" name=\"...\" id=\"...\">",
                text: "<input type=\"month\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:month.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:month.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:number",
                key: "liveTemplates.Zen HTML.input:number",
                description: "<input type=\"number\" name=\"...\" id=\"...\">",
                text: "<input type=\"number\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:number.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:number.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:p",
                key: "liveTemplates.Zen HTML.input:p",
                description: "<input type=\"password\" name=\"...\" id=\"...\">",
                text: "<input type=\"password\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:p.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:p.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:password",
                key: "liveTemplates.Zen HTML.input:password",
                description: "<input type=\"password\" name=\"...\" id=\"...\">",
                text: "<input type=\"password\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:password.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:password.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:r",
                key: "liveTemplates.Zen HTML.input:r",
                description: "<input type=\"radio\" name=\"...\" id=\"...\">",
                text: "<input type=\"radio\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:r.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:r.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:radio",
                key: "liveTemplates.Zen HTML.input:radio",
                description: "<input type=\"radio\" name=\"...\" id=\"...\">",
                text: "<input type=\"radio\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:radio.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:radio.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:range",
                key: "liveTemplates.Zen HTML.input:range",
                description: "<input type=\"range\" name=\"...\" id=\"...\">",
                text: "<input type=\"range\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:range.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:range.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:reset",
                key: "liveTemplates.Zen HTML.input:reset",
                description: "<input type=\"reset\" value=\"...\">",
                text: "<input type=\"reset\" value=\"$VAR0$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:reset.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:reset.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:s",
                key: "liveTemplates.Zen HTML.input:s",
                description: "<input type=\"submit\" value=\"...\">",
                text: "<input type=\"submit\" value=\"$VAR0$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:s.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:s.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:search",
                key: "liveTemplates.Zen HTML.input:search",
                description: "<input type=\"search\" name=\"...\" id=\"...\">",
                text: "<input type=\"search\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:search.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:search.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:submit",
                key: "liveTemplates.Zen HTML.input:submit",
                description: "<input type=\"submit\" value=\"...\">",
                text: "<input type=\"submit\" value=\"$VAR0$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:submit.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:submit.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:t",
                key: "liveTemplates.Zen HTML.input:t",
                description: "<input type=\"text\" name=\"...\" id=\"...\">",
                text: "<input type=\"text\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:t.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:t.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:tel",
                key: "liveTemplates.Zen HTML.input:tel",
                description: "<input type=\"tel\" name=\"...\" id=\"...\">",
                text: "<input type=\"tel\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:tel.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:tel.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:text",
                key: "liveTemplates.Zen HTML.input:text",
                description: "<input type=\"text\" name=\"...\" id=\"...\">",
                text: "<input type=\"text\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:text.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:text.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:time",
                key: "liveTemplates.Zen HTML.input:time",
                description: "<input type=\"time\" name=\"...\" id=\"...\">",
                text: "<input type=\"time\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:time.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:time.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:url",
                key: "liveTemplates.Zen HTML.input:url",
                description: "<input type=\"url\" name=\"...\" id=\"...\">",
                text: "<input type=\"url\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:url.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:url.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "input:week",
                key: "liveTemplates.Zen HTML.input:week",
                description: "<input type=\"week\" name=\"...\" id=\"...\">",
                text: "<input type=\"week\" name=\"$VAR0$\" id=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.input:week.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.input:week.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "isindex",
                key: "liveTemplates.Zen HTML.isindex",
                description: "<isindex>",
                text: "<isindex>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.isindex.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.isindex.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "keygen",
                key: "liveTemplates.Zen HTML.keygen",
                description: "<keygen>",
                text: "<keygen>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.keygen.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.keygen.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "kg",
                key: "liveTemplates.Zen HTML.kg",
                description: "<keygen>",
                text: "<keygen>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.kg.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.kg.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "label",
                key: "liveTemplates.Zen HTML.label",
                description: "<label for=\"...\">...</label>",
                text: "<label for=\"$VAR0$\">$END$</label>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.label.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.label.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "leg",
                key: "liveTemplates.Zen HTML.leg",
                description: "<legend>...</legend>",
                text: "<legend>$END$</legend>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.leg.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.leg.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "li",
                key: "liveTemplates.Zen HTML.li",
                description: "<li>...</li>",
                text: "<li>$END$</li>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.li.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.li.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "link",
                key: "liveTemplates.Zen HTML.link",
                description: "<link rel=\"stylesheet\" href=\"...\">",
                text: "<link rel=\"stylesheet\" href=\"$END$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.link.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.link.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "link:atom",
                key: "liveTemplates.Zen HTML.link:atom",
                description: "<link rel=\"alternate\" href=\"atom.xml\" type=\"application/atom+xml\" title=\"Atom\">",
                text: "<link rel=\"alternate\" href=\"$ATOM$\" type=\"application/atom+xml\" title=\"Atom\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.link:atom.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.link:atom.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "link:css",
                key: "liveTemplates.Zen HTML.link:css",
                description: "<link rel=\"stylesheet\" href=\"style.css\" >",
                text: "<link rel=\"stylesheet\" href=\"$STYLE$.css\" >",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.link:css.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.link:css.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "link:favicon",
                key: "liveTemplates.Zen HTML.link:favicon",
                description: "<link rel=\"shortcut icon\" href=\"favicon.ico\" type=\"image/x-icon\">",
                text: "<link rel=\"shortcut icon\" href=\"$FAVICON$\" type=\"image/x-icon\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.link:favicon.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.link:favicon.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "link:im",
                key: "liveTemplates.Zen HTML.link:im",
                description: "<link rel=\"import\" href=\"\">",
                text: "<link rel=\"import\" href=\"$HTML$.html\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.link:im.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.link:im.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "link:import",
                key: "liveTemplates.Zen HTML.link:import",
                description: "<link rel=\"import\" href=\"\">",
                text: "<link rel=\"import\" href=\"$HTML$.html\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.link:import.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.link:import.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "link:print",
                key: "liveTemplates.Zen HTML.link:print",
                description: "<link rel=\"stylesheet\" href=\"print.css\" media=\"print\">",
                text: "<link rel=\"stylesheet\" href=\"$STYLE$.css\" media=\"print\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.link:print.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.link:print.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "link:rss",
                key: "liveTemplates.Zen HTML.link:rss",
                description: "<link rel=\"alternate\" href=\"rss.xml\" type=\"application/rss+xml\" title=\"RSS\">",
                text: "<link rel=\"alternate\" href=\"$RSS$\" type=\"application/rss+xml\" title=\"RSS\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.link:rss.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.link:rss.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "link:touch",
                key: "liveTemplates.Zen HTML.link:touch",
                description: "<link rel=\"apple-touch-icon\" href=\"favicon.png\">",
                text: "<link rel=\"apple-touch-icon\" href=\"$FAVICON$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.link:touch.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.link:touch.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "map",
                key: "liveTemplates.Zen HTML.map",
                description: "<map name=\"...\">...</map>",
                text: "<map name=\"$VAR0$\">$END$</map>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.map.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.map.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "map+",
                key: "liveTemplates.Zen HTML.map+",
                description: "",
                text: "<map name=\"$VAR0$\">\n<area shape=\"$VAR1$\" coords=\"$VAR2$\" href=\"$VAR3$\" alt=\"$VAR4$\">$END$\n</map>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.map+.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.map+.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "marquee",
                key: "liveTemplates.Zen HTML.marquee",
                description: "<marquee behavior=\"...\" direction=\"...\">...</marquee>",
                text: "<marquee behavior=\"$BEHAVIOR$\" direction=\"$DIRECTION$\">$END$</marquee>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.marquee.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.marquee.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "menu:c",
                key: "liveTemplates.Zen HTML.menu:c",
                description: "<menu type=\"context\">...</menu>",
                text: "<menu type=\"context\">$END$</menu>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.menu:c.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.menu:c.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "menu:context",
                key: "liveTemplates.Zen HTML.menu:context",
                description: "<menu type=\"context\">...</menu>",
                text: "<menu type=\"context\">$END$</menu>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.menu:context.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.menu:context.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "menu:t",
                key: "liveTemplates.Zen HTML.menu:t",
                description: "<menu type=\"toolbar\">...</menu>",
                text: "<menu type=\"toolbar\">$END$</menu>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.menu:t.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.menu:t.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "menu:toolbar",
                key: "liveTemplates.Zen HTML.menu:toolbar",
                description: "<menu type=\"toolbar\">...</menu>",
                text: "<menu type=\"toolbar\">$END$</menu>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.menu:toolbar.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.menu:toolbar.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "meta",
                key: "liveTemplates.Zen HTML.meta",
                description: "<meta>",
                text: "<meta>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.meta.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.meta.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "meta:compat",
                key: "liveTemplates.Zen HTML.meta:compat",
                description: "<meta http-equiv=\"X-UA-Compatible\" content=\"IE=7\">",
                text: "<meta http-equiv=\"X-UA-Compatible\" content=\"$VAR0$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.meta:compat.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.meta:compat.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "meta:edge",
                key: "liveTemplates.Zen HTML.meta:edge",
                description: "<meta http-equiv=\"X-UA-Compatible\" content=\"ie=edge\">",
                text: "<meta http-equiv=\"X-UA-Compatible\" content=\"ie=$IE$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.meta:edge.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.meta:edge.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "meta:redirect",
                key: "liveTemplates.Zen HTML.meta:redirect",
                description: "<meta http-equiv=\"refresh\" content=\"0; url=http://example.com\">",
                text: "<meta http-equiv=\"refresh\" content=\"0; url=$URL$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.meta:redirect.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.meta:redirect.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "meta:utf",
                key: "liveTemplates.Zen HTML.meta:utf",
                description: "<meta http-equiv=\"Content-Type\" content=\"text/html;charset=UTF-8\">",
                text: "<meta http-equiv=\"Content-Type\" content=\"text/html;charset=UTF-8\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.meta:utf.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.meta:utf.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "meta:vp",
                key: "liveTemplates.Zen HTML.meta:vp",
                description: "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">",
                text: "<meta name=\"viewport\" content=\"width=$WIDTH$, initial-scale=$INITIAL_SCALE$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.meta:vp.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.meta:vp.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "meta:win",
                key: "liveTemplates.Zen HTML.meta:win",
                description: "<meta http-equiv=\"Content-Type\" content=\"text/html;charset=Win-1251\">",
                text: "<meta http-equiv=\"Content-Type\" content=\"text/html;charset=windows-1251\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.meta:win.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.meta:win.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "mn",
                key: "liveTemplates.Zen HTML.mn",
                description: "<main>...</main>",
                text: "<main>$END$</main>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.mn.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.mn.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "obj",
                key: "liveTemplates.Zen HTML.obj",
                description: "<object data=\"...\" type=\"...\">...</object>",
                text: "<object data=\"$VAR0$\" type=\"$VAR1$\">$END$</object>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.obj.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.obj.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "object",
                key: "liveTemplates.Zen HTML.object",
                description: "<object data=\"...\" type=\"...\">...</object>",
                text: "<object data=\"$VAR0$\" type=\"$VAR1$\">$END$</object>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.object.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.object.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "ol",
                key: "liveTemplates.Zen HTML.ol",
                description: "<ol>...</ol>",
                text: "<ol>$END$</ol>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.ol.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.ol.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "ol+",
                key: "liveTemplates.Zen HTML.ol+",
                description: "",
                text: "<ol>\n<li>$END$</li>\n</ol>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.ol+.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.ol+.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "opt",
                key: "liveTemplates.Zen HTML.opt",
                description: "<option value=\"...\">...</option>",
                text: "<option value=\"$VAR0$\">$END$</option>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.opt.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.opt.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "optg",
                key: "liveTemplates.Zen HTML.optg",
                description: "<optgroup>...</optgroup>",
                text: "<optgroup>$END$</optgroup>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.optg.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.optg.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "optg+",
                key: "liveTemplates.Zen HTML.optg+",
                description: "",
                text: "<optgroup>\n<option value=\"$VAR0$\">$END$</option>\n</optgroup>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.optg+.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.optg+.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "optgroup+",
                key: "liveTemplates.Zen HTML.optgroup+",
                description: "",
                text: "<optgroup>\n<option value=\"$VAR0$\">$END$</option>\n</optgroup>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.optgroup+.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.optgroup+.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "option",
                key: "liveTemplates.Zen HTML.option",
                description: "<option value=\"...\">...</option>",
                text: "<option value=\"$VAR0$\">$END$</option>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.option.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.option.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "out",
                key: "liveTemplates.Zen HTML.out",
                description: "<output>...</output>",
                text: "<output>$END$</output>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.out.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.out.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "p",
                key: "liveTemplates.Zen HTML.p",
                description: "<p>...</p>",
                text: "<p>$END$</p>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.p.expand",
                expand: "None",
                reformat_key: "liveTemplates.Zen HTML.p.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "param",
                key: "liveTemplates.Zen HTML.param",
                description: "<param name=\"...\" value=\"...\">",
                text: "<param name=\"$VAR0$\" value=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.param.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.param.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "pic",
                key: "liveTemplates.Zen HTML.pic",
                description: "<picture></picture>",
                text: "<picture></picture>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.pic.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.pic.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "pic+",
                key: "liveTemplates.Zen HTML.pic+",
                description: "",
                text: "<picture>\n<source srcset=\"$VAR0$\">\n<img src=\"$VAR1$\" alt=\"$VAR2$\"></picture>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.pic+.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.pic+.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "picture",
                key: "liveTemplates.Zen HTML.picture",
                description: "<picture></picture>",
                text: "<picture></picture>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.picture.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.picture.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "prog",
                key: "liveTemplates.Zen HTML.prog",
                description: "<progress>...</progress>",
                text: "<progress>$END$</progress>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.prog.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.prog.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "ri:a",
                key: "liveTemplates.Zen HTML.ri:a",
                description: "<picture><source media=\"(...)\" srcset=\"...\"><img src=\"...\" alt=\"...\"></picture>",
                text: "<picture>\n<source media=\"($VAR0$)\" srcset=\"$VAR1$\">\n<img src=\"$VAR2$\" alt=\"$VAR3$\"></picture>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.ri:a.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.ri:a.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "ri:art",
                key: "liveTemplates.Zen HTML.ri:art",
                description: "<picture><source media=\"(...)\" srcset=\"...\"><img src=\"...\" alt=\"...\"></picture>",
                text: "<picture>\n<source media=\"($VAR0$)\" srcset=\"$VAR1$\">\n<img src=\"$VAR2$\" alt=\"$VAR3$\"></picture>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.ri:art.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.ri:art.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "ri:d",
                key: "liveTemplates.Zen HTML.ri:d",
                description: "<img src=\"...\" alt=\"...\" srcset=\"...\">",
                text: "<img src=\"$VAR1$\" alt=\"$VAR2$\" srcset=\"$VAR0$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.ri:d.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.ri:d.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "ri:dpr",
                key: "liveTemplates.Zen HTML.ri:dpr",
                description: "<img src=\"...\" alt=\"...\" srcset=\"...\">",
                text: "<img src=\"$VAR1$\" alt=\"$VAR2$\" srcset=\"$VAR0$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.ri:dpr.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.ri:dpr.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "ri:t",
                key: "liveTemplates.Zen HTML.ri:t",
                description: "<picture><source srcset=\"...\" type=\"...\"><img src=\"...\" alt=\"...\"></picture>",
                text: "<picture>\n<source srcset=\"$VAR0$\" type=\"$VAR1$\">\n<img src=\"$VAR2$\" alt=\"$VAR3$\"></picture>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.ri:t.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.ri:t.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "ri:type",
                key: "liveTemplates.Zen HTML.ri:type",
                description: "<picture><source srcset=\"...\" type=\"...\"><img src=\"...\" alt=\"...\"></picture>",
                text: "<picture>\n<source srcset=\"$VAR0$\" type=\"$VAR1$\">\n<img src=\"$VAR2$\" alt=\"$VAR3$\"></picture>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.ri:type.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.ri:type.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "ri:v",
                key: "liveTemplates.Zen HTML.ri:v",
                description: "<img src=\"...\" alt=\"...\" sizes=\"...\" srcset=\"...\">",
                text: "<img src=\"$VAR2$\" alt=\"$VAR3$\" sizes=\"$VAR0$\" srcset=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.ri:v.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.ri:v.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "ri:viewport",
                key: "liveTemplates.Zen HTML.ri:viewport",
                description: "<img src=\"...\" alt=\"...\" sizes=\"...\" srcset=\"...\">",
                text: "<img src=\"$VAR2$\" alt=\"$VAR3$\" sizes=\"$VAR0$\" srcset=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.ri:viewport.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.ri:viewport.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "script",
                key: "liveTemplates.Zen HTML.script",
                description: "<script>...</script>",
                text: "<script !src=\"\">$END$</script>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.script.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.script.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "script:src",
                key: "liveTemplates.Zen HTML.script:src",
                description: "<script src=\"...\">...</script>",
                text: "<script src=\"$END$\"></script>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.script:src.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.script:src.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "sect",
                key: "liveTemplates.Zen HTML.sect",
                description: "<section>...</section>",
                text: "<section>$END$</section>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.sect.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.sect.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "select",
                key: "liveTemplates.Zen HTML.select",
                description: "<select name=\"...\" id=\"...\">...</select>",
                text: "<select name=\"$VAR0$\" id=\"$VAR1$\">$END$</select>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.select.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.select.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "select+",
                key: "liveTemplates.Zen HTML.select+",
                description: "",
                text: "<select name=\"$VAR0$\" id=\"$VAR1$\">\n<option value=\"$VAR2$\">$END$</option>\n</select>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.select+.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.select+.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "select:d",
                key: "liveTemplates.Zen HTML.select:d",
                description: "<select name=\"...\" id=\"...\" disabled>...</select>",
                text: "<select name=\"$VAR0$\" id=\"$VAR1$\" disabled>$END$</select>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.select:d.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.select:d.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "select:disabled",
                key: "liveTemplates.Zen HTML.select:disabled",
                description: "<select name=\"...\" id=\"...\" disabled>...</select>",
                text: "<select name=\"$VAR0$\" id=\"$VAR1$\" disabled>$END$</select>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.select:disabled.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.select:disabled.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "source:media",
                key: "liveTemplates.Zen HTML.source:media",
                description: "<source media=\"(...)\" srcset=\"...\">",
                text: "<source media=\"($VAR0$)\" srcset=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.source:media.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.source:media.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "source:media:sizes",
                key: "liveTemplates.Zen HTML.source:media:sizes",
                description: "<source media=\"(...)\" srcset=\"...\" sizes=\"...\">",
                text: "<source media=\"($VAR0$)\" srcset=\"$VAR2$\" sizes=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.source:media:sizes.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.source:media:sizes.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "source:media:type",
                key: "liveTemplates.Zen HTML.source:media:type",
                description: "<source media=\"(...)\" srcset=\"...\" type=\"...\">",
                text: "<source media=\"($VAR0$)\" srcset=\"$VAR1$\" type=\"$VAR2$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.source:media:type.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.source:media:type.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "source:sizes",
                key: "liveTemplates.Zen HTML.source:sizes",
                description: "<source sizes=\"...\" srcset=\"...\">",
                text: "<source sizes=\"$VAR0$\" srcset=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.source:sizes.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.source:sizes.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "source:sizes:type",
                key: "liveTemplates.Zen HTML.source:sizes:type",
                description: "<source sizes=\"...\" srcset=\"...\" type=\"...\">",
                text: "<source sizes=\"$VAR0$\" srcset=\"$VAR1$\" type=\"$VAR2$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.source:sizes:type.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.source:sizes:type.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "source:src",
                key: "liveTemplates.Zen HTML.source:src",
                description: "<source src=\"...\" type=\"...\">",
                text: "<source src=\"$VAR0$\" type=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.source:src.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.source:src.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "source:srcset",
                key: "liveTemplates.Zen HTML.source:srcset",
                description: "<source srcset=\"...\">",
                text: "<source srcset=\"$VAR0$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.source:srcset.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.source:srcset.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "source:type",
                key: "liveTemplates.Zen HTML.source:type",
                description: "<source srcset=\"...\" type=\"...\">",
                text: "<source srcset=\"$VAR0$\" type=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.source:type.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.source:type.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "span",
                key: "liveTemplates.Zen HTML.span",
                description: "<span>...</span>",
                text: "<span>$END$</span>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.span.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.span.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "src",
                key: "liveTemplates.Zen HTML.src",
                description: "<source>",
                text: "<source>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.src.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.src.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "src:m",
                key: "liveTemplates.Zen HTML.src:m",
                description: "<source media=\"(...)\" srcset=\"...\">",
                text: "<source media=\"($VAR0$)\" srcset=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.src:m.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.src:m.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "src:mt",
                key: "liveTemplates.Zen HTML.src:mt",
                description: "<source media=\"(...)\" srcset=\"...\" type=\"...\">",
                text: "<source media=\"($VAR0$)\" srcset=\"$VAR1$\" type=\"$VAR2$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.src:mt.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.src:mt.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "src:mz",
                key: "liveTemplates.Zen HTML.src:mz",
                description: "<source media=\"(...)\" srcset=\"...\" sizes=\"...\">",
                text: "<source media=\"($VAR0$)\" srcset=\"$VAR2$\" sizes=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.src:mz.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.src:mz.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "src:s",
                key: "liveTemplates.Zen HTML.src:s",
                description: "<source srcset=\"...\">",
                text: "<source srcset=\"$VAR0$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.src:s.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.src:s.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "src:sc",
                key: "liveTemplates.Zen HTML.src:sc",
                description: "<source src=\"...\" type=\"...\">",
                text: "<source src=\"$VAR0$\" type=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.src:sc.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.src:sc.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "src:t",
                key: "liveTemplates.Zen HTML.src:t",
                description: "<source srcset=\"...\" type=\"...\">",
                text: "<source srcset=\"$VAR0$\" type=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.src:t.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.src:t.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "src:z",
                key: "liveTemplates.Zen HTML.src:z",
                description: "<source sizes=\"...\" srcset=\"...\">",
                text: "<source sizes=\"$VAR0$\" srcset=\"$VAR1$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.src:z.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.src:z.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "src:zt",
                key: "liveTemplates.Zen HTML.src:zt",
                description: "<source sizes=\"...\" srcset=\"...\" type=\"...\">",
                text: "<source sizes=\"$VAR0$\" srcset=\"$VAR1$\" type=\"$VAR2$\">",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.src:zt.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.src:zt.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "str",
                key: "liveTemplates.Zen HTML.str",
                description: "<strong>...</strong>",
                text: "<strong>$END$</strong>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.str.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.str.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "style",
                key: "liveTemplates.Zen HTML.style",
                description: "<style>...</style>",
                text: "<style>$END$</style>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.style.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.style.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "table",
                key: "liveTemplates.Zen HTML.table",
                description: "<table>...</table>",
                text: "<table>$END$</table>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.table.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.table.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "table+",
                key: "liveTemplates.Zen HTML.table+",
                description: "",
                text: "<table>\n<tr>\n<td>$END$</td>\n</tr>\n</table>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.table+.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.table+.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "tarea",
                key: "liveTemplates.Zen HTML.tarea",
                description: "<textarea name=\"...\" id=\"...\" cols=\"30\" rows=\"10\">...</textarea>",
                text: "<textarea name=\"$VAR0$\" id=\"$VAR1$\" cols=\"$COL$\" rows=\"$ROWS$\">$END$</textarea>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.tarea.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.tarea.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "td",
                key: "liveTemplates.Zen HTML.td",
                description: "<td>...</td>",
                text: "<td>$END$</td>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.td.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.td.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "tem",
                key: "liveTemplates.Zen HTML.tem",
                description: "<template>...</template>",
                text: "<template>$END$</template>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.tem.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.tem.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "textarea",
                key: "liveTemplates.Zen HTML.textarea",
                description: "<textarea name=\"...\" id=\"...\" cols=\"30\" rows=\"10\">...</textarea>",
                text: "<textarea name=\"$VAR0$\" id=\"$VAR1$\" cols=\"$COL$\" rows=\"$ROWS$\">$END$</textarea>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.textarea.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.textarea.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "title",
                key: "liveTemplates.Zen HTML.title",
                description: "<title>...</title>",
                text: "<title>$END$</title>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.title.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.title.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "tr+",
                key: "liveTemplates.Zen HTML.tr+",
                description: "",
                text: "<tr>\n<td>$END$</td>\n</tr>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.tr+.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.tr+.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "ul",
                key: "liveTemplates.Zen HTML.ul",
                description: "<ul>...</ul>",
                text: "<ul>$END$</ul>",
                applies: Applies::None,
                expand_key: "liveTemplates.Zen HTML.ul.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.ul.reformat",
                reformat: false,
            },
            LiveTemplate {
                abbreviation: "ul+",
                key: "liveTemplates.Zen HTML.ul+",
                description: "",
                text: "<ul>\n<li>$END$</li>\n</ul>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.ul+.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.ul+.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "video",
                key: "liveTemplates.Zen HTML.video",
                description: "<video src=\"...\">...</video>",
                text: "<video src=\"$VAR0$\">$END$</video>",
                applies: Applies::HtmlText,
                expand_key: "liveTemplates.Zen HTML.video.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen HTML.video.reformat",
                reformat: true,
            },
        ],
    ),
    (
        "Zen XSL",
        &[
            LiveTemplate {
                abbreviation: "!!!",
                key: "liveTemplates.Zen XSL.!!!",
                description: "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
                text: "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.!!!.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.!!!.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "ap",
                key: "liveTemplates.Zen XSL.ap",
                description: "<xsl:apply-templates select=\"...\" mode=\"...\" />",
                text: "<xsl:apply-templates select=\"$VAR0$\" mode=\"$VAR1$\" />",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.ap.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.ap.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "api",
                key: "liveTemplates.Zen XSL.api",
                description: "<xsl:apply-imports/>",
                text: "<xsl:apply-imports/>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.api.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.api.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "attr",
                key: "liveTemplates.Zen XSL.attr",
                description: "<xsl:attribute name=\"...\">...</xsl:attribute>",
                text: "<xsl:attribute name=\"$VAR0$\">$END$</xsl:attribute>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.attr.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.attr.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "attrs",
                key: "liveTemplates.Zen XSL.attrs",
                description: "<xsl:attribute-set name=\"...\">...</xsl:attribute-set>",
                text: "<xsl:attribute-set name=\"$VAR0$\">$END$</xsl:attribute-set>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.attrs.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.attrs.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "call",
                key: "liveTemplates.Zen XSL.call",
                description: "<xsl:call-template name=\"...\" />",
                text: "<xsl:call-template name=\"$VAR0$\" />",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.call.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.call.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "ch",
                key: "liveTemplates.Zen XSL.ch",
                description: "<xsl:choose>...</xsl:choose>",
                text: "<xsl:choose>$END$</xsl:choose>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.ch.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.ch.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "choose",
                key: "liveTemplates.Zen XSL.choose",
                description: "",
                text: "<xsl:choose>\n  <xsl:when test=\"$VAR0$\">$VAR1$</xsl:when>\n  <xsl:otherwise>$END$</xsl:otherwise>\n</xsl:choose>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.choose.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.choose.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "co",
                key: "liveTemplates.Zen XSL.co",
                description: "<xsl:copy-of select=\"...\" />",
                text: "<xsl:copy-of select=\"$VAR0$\" />",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.co.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.co.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "com",
                key: "liveTemplates.Zen XSL.com",
                description: "<xsl:comment>...</xsl:comment>",
                text: "<xsl:comment>$END$</xsl:comment>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.com.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.com.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "cp",
                key: "liveTemplates.Zen XSL.cp",
                description: "<xsl:copy select=\"...\"/>",
                text: "<xsl:copy select=\"$VAR0$\"/>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.cp.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.cp.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "each",
                key: "liveTemplates.Zen XSL.each",
                description: "<xsl:for-each select=\"...\">...</xsl:for-each>",
                text: "<xsl:for-each select=\"$VAR0$\">$END$</xsl:for-each>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.each.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.each.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "elem",
                key: "liveTemplates.Zen XSL.elem",
                description: "<xsl:element name=\"...\">...</xsl:element>",
                text: "<xsl:element name=\"$VAR0$\">$END$</xsl:element>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.elem.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.elem.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "fall",
                key: "liveTemplates.Zen XSL.fall",
                description: "<xsl:fallback>...</xsl:fallback>",
                text: "<xsl:fallback>$END$</xsl:fallback>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.fall.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.fall.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "for",
                key: "liveTemplates.Zen XSL.for",
                description: "<xsl:for-each select=\"...\">...</xsl:for-each>",
                text: "<xsl:for-each select=\"$VAR0$\">$END$</xsl:for-each>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.for.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.for.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "if",
                key: "liveTemplates.Zen XSL.if",
                description: "<xsl:if test=\"...\">...</xsl:if>",
                text: "<xsl:if test=\"$VAR0$\">$END$</xsl:if>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.if.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.if.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "imp",
                key: "liveTemplates.Zen XSL.imp",
                description: "<xsl:import href=\"...\"/>",
                text: "<xsl:import href=\"$VAR0$\"/>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.imp.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.imp.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "inc",
                key: "liveTemplates.Zen XSL.inc",
                description: "<xsl:include href=\"...\"/>",
                text: "<xsl:include href=\"$VAR0$\"/>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.inc.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.inc.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "key",
                key: "liveTemplates.Zen XSL.key",
                description: "<xsl:key name=\"...\" match=\"...\" use=\"...\" />",
                text: "<xsl:key name=\"$VAR0$\" match=\"$VAR1$\" use=\"$VAR2$\" />",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.key.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.key.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "msg",
                key: "liveTemplates.Zen XSL.msg",
                description: "<xsl:message terminate=\"no\">...</xsl:message>",
                text: "<xsl:message terminate=\"no\">$END$</xsl:message>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.msg.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.msg.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "nam",
                key: "liveTemplates.Zen XSL.nam",
                description: "<namespace-alias stylesheet-prefix=\"...\" result-prefix=\"...\" />",
                text: "<namespace-alias stylesheet-prefix=\"$VAR0$\" result-prefix=\"$END$\" />",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.nam.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.nam.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "num",
                key: "liveTemplates.Zen XSL.num",
                description: "<xsl:number value=\"...\" />",
                text: "<xsl:number value=\"$END$\" />",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.num.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.num.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "ot",
                key: "liveTemplates.Zen XSL.ot",
                description: "<xsl:otherwise></xsl:otherwise>",
                text: "<xsl:otherwise>$END$</xsl:otherwise>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.ot.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.ot.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "par",
                key: "liveTemplates.Zen XSL.par",
                description: "<xsl:param name=\"...\">...</xsl:param>",
                text: "<xsl:param name=\"$VAR0$\" >$END$</xsl:param>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.par.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.par.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "pare",
                key: "liveTemplates.Zen XSL.pare",
                description: "<xsl:param name=\"...\" select=\"...\" />",
                text: "<xsl:param name=\"$VAR0$\" select=\"$VAR1$\" />",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.pare.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.pare.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "pres",
                key: "liveTemplates.Zen XSL.pres",
                description: "<xsl:preserve-space elements=\"...\" />",
                text: "<xsl:preserve-space elements=\"$END$\" />",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.pres.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.pres.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "proc",
                key: "liveTemplates.Zen XSL.proc",
                description: "<xsl:processing-instruction name=\"...\">...</xsl:processing-instruction>",
                text: "<xsl:processing-instruction name=\"$VAR0$\">$END$</xsl:processing-instruction>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.proc.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.proc.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "sort",
                key: "liveTemplates.Zen XSL.sort",
                description: "<xsl:sort select=\"...\" order=\"...\" />",
                text: "<xsl:sort select=\"$VAR0$\" order=\"$VAR1$\" />",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.sort.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.sort.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "strip",
                key: "liveTemplates.Zen XSL.strip",
                description: "<xsl:strip-space elements=\"...\" />>",
                text: "<xsl:strip-space elements=\"$END$\" />",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.strip.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.strip.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "tex",
                key: "liveTemplates.Zen XSL.tex",
                description: "<xsl:text></xsl:text>",
                text: "<xsl:text>$END$</xsl:text>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.tex.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.tex.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "tm",
                key: "liveTemplates.Zen XSL.tm",
                description: "<xsl:template match=\"...\" mode=\"...\">...</xsl:template>",
                text: "<xsl:template match=\"$VAR0$\" mode=\"$VAR1$\">$END$</xsl:template>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.tm.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.tm.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "tmatch",
                key: "liveTemplates.Zen XSL.tmatch",
                description: "<xsl:template match=\"...\" mode=\"...\">...</xsl:template>",
                text: "<xsl:template match=\"$VAR0$\" mode=\"$VAR1$\">$END$</xsl:template>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.tmatch.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.tmatch.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "tn",
                key: "liveTemplates.Zen XSL.tn",
                description: "<xsl:template name=\"...\">...</xsl:template>",
                text: "<xsl:template name=\"$VAR0$\">$END$</xsl:template>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.tn.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.tn.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "tname",
                key: "liveTemplates.Zen XSL.tname",
                description: "<xsl:template name=\"...\">...</xsl:template>",
                text: "<xsl:template name=\"$VAR0$\">$END$</xsl:template>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.tname.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.tname.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "val",
                key: "liveTemplates.Zen XSL.val",
                description: "<xsl:value-of select=\"...\" />",
                text: "<xsl:value-of select=\"$VAR0$\" />",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.val.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.val.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "var",
                key: "liveTemplates.Zen XSL.var",
                description: "<xsl:variable name=\"...\">...</xsl:variable>",
                text: "<xsl:variable name=\"$VAR0$\">$END$</xsl:variable>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.var.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.var.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "vare",
                key: "liveTemplates.Zen XSL.vare",
                description: "<xsl:variable name=\"...\" select=\"...\" />",
                text: "<xsl:variable name=\"$VAR0$\" select=\"$VAR1$\" />",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.vare.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.vare.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "wh",
                key: "liveTemplates.Zen XSL.wh",
                description: "<xsl:when test=\"...\">...</xsl:when>",
                text: "<xsl:when test=\"$VAR0$\">$END$</xsl:when>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.wh.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.wh.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "xsl",
                key: "liveTemplates.Zen XSL.xsl",
                description: "",
                text: "<?xml version=\"1.0\" encoding=\"UTF-8\"?>   <xsl:stylesheet version=\"1.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">\n$VAR0$\n</xsl:stylesheet>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.xsl.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.xsl.reformat",
                reformat: true,
            },
            LiveTemplate {
                abbreviation: "xsl:when",
                key: "liveTemplates.Zen XSL.xsl:when",
                description: "<xsl:when test=\"...\">...</xsl:when>",
                text: "<xsl:when test=\"$VAR0$\">$END$</xsl:when>",
                applies: Applies::XslText,
                expand_key: "liveTemplates.Zen XSL.xsl:when.expand",
                expand: "Default (Tab)",
                reformat_key: "liveTemplates.Zen XSL.xsl:when.reformat",
                reformat: true,
            },
        ],
    ),
];

/// The bundles shipped in the reference app's textmate-plugin, in its order.
const TEXTMATE_BUNDLES: &[(&str, &str)] = &[
    ("adoc", "textMate.adoc"),
    ("bat", "textMate.bat"),
    ("bazel", "textMate.bazel"),
    ("bicep", "textMate.bicep"),
    ("clojure", "textMate.clojure"),
    ("cmake", "textMate.cmake"),
    ("coffeescript", "textMate.coffeescript"),
    ("cpp", "textMate.cpp"),
    ("csharp", "textMate.csharp"),
    ("css", "textMate.css"),
    ("dart", "textMate.dart"),
    ("diff", "textMate.diff"),
    ("docker", "textMate.docker"),
    ("dotenv", "textMate.dotenv"),
    ("erlang", "textMate.erlang"),
    ("fsharp", "textMate.fsharp"),
    ("git-base", "textMate.git-base"),
    ("go", "textMate.go"),
    ("groovy", "textMate.groovy"),
    ("handlebars", "textMate.handlebars"),
    ("hcl", "textMate.hcl"),
    ("hlsl", "textMate.hlsl"),
    ("html", "textMate.html"),
    ("ini", "textMate.ini"),
    ("java", "textMate.java"),
    ("javascript", "textMate.javascript"),
    ("json", "textMate.json"),
    ("jsp", "textMate.jsp"),
    ("julia", "textMate.julia"),
    ("kconfig", "textMate.kconfig"),
    ("kotlin", "textMate.kotlin"),
    ("latex", "textMate.latex"),
    ("less", "textMate.less"),
    ("log", "textMate.log"),
    ("lua", "textMate.lua"),
    ("make", "textMate.make"),
    ("markdown-basics", "textMate.markdown-basics"),
    ("markdown-math", "textMate.markdown-math"),
    ("mdx", "textMate.mdx"),
    ("objective-c", "textMate.objective-c"),
    ("perl", "textMate.perl"),
    ("php", "textMate.php"),
    ("powershell", "textMate.powershell"),
    ("prompt-basics", "textMate.prompt-basics"),
    ("pug", "textMate.pug"),
    ("python", "textMate.python"),
    ("r", "textMate.r"),
    ("razor", "textMate.razor"),
    ("restructuredtext", "textMate.restructuredtext"),
    ("ruby", "textMate.ruby"),
    ("rust", "textMate.rust"),
    ("scss", "textMate.scss"),
    ("search-result", "textMate.search-result"),
    ("shaderlab", "textMate.shaderlab"),
    ("shellscript", "textMate.shellscript"),
    ("sql", "textMate.sql"),
    ("swift", "textMate.swift"),
    ("terraform", "textMate.terraform"),
    ("twig", "textMate.twig"),
    ("typescript-basics", "textMate.typescript-basics"),
    ("vb", "textMate.vb"),
    ("viml", "textMate.viml"),
    ("vue", "textMate.vue"),
    ("xml", "textMate.xml"),
    ("yaml", "textMate.yaml"),
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        CODE_VISION, EMMET_FILTERS, INSPECTIONS, INTENTIONS, LIVE_TEMPLATES, TEXTMATE_BUNDLES,
        group_state,
    };

    #[test]
    fn a_group_is_partly_checked_when_some_items_are() {
        assert_eq!(group_state(0, 3), Some(false));
        assert_eq!(group_state(3, 3), Some(true));
        assert_eq!(group_state(1, 3), None);
    }

    #[test]
    fn every_setting_has_a_key_of_its_own() {
        let keys = INSPECTIONS
            .iter()
            .flat_map(|(_, inspections)| inspections.iter())
            .flat_map(|inspection| [inspection.enabled_key, inspection.severity_key])
            .chain(
                LIVE_TEMPLATES
                    .iter()
                    .flat_map(|(_, templates)| templates.iter())
                    .flat_map(|template| {
                        [template.key, template.expand_key, template.reformat_key]
                    }),
            )
            .chain(
                INTENTIONS
                    .iter()
                    .flat_map(|(_, intentions)| intentions.iter())
                    .map(|intention| intention.key),
            )
            .chain(TEXTMATE_BUNDLES.iter().map(|(_, key)| *key))
            .chain(CODE_VISION.iter().map(|(_, key, _)| *key))
            .chain(EMMET_FILTERS.iter().map(|(key, _)| *key));
        let mut seen = HashSet::new();
        for key in keys {
            // settings.conf is `key=value` lines
            assert!(!key.contains('=') && !key.contains('\n'), "{key}");
            assert!(seen.insert(key), "{key} twice");
        }
    }
}
