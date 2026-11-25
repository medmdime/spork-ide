//! Settings ▸ Build Tools, Languages & Frameworks, Tools, Backup and Sync, Advanced Settings, as
//! the reference app's. Diff & Merge's own page is Spork's.

use std::cell::RefCell;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_workbench::Tokens;

use crate::settings_vcs::{not_yet, note, table, tool};
use crate::settings_window::{Node, SettingsWindow, comment, labeled, titled};

/// The pages under Languages & Frameworks ▸ Schemas and DTDs (the XML plugin's), by name.
pub(crate) const SCHEMAS: &[Node] = &[
    Node::page("Default XML Schemas").per_project(),
    Node::page("XML Catalog").per_project(),
];

/// The pages under Tools ▸ Diff & Merge.
pub(crate) const DIFF_MERGE: &[Node] = &[Node::page("External Diff Tools")];

/// An advanced setting's control, chosen by its default as IntelliJ's AdvancedSettingBean does: a
/// checkbox, a number or text field, or a combo box of an enum's labels with the default's.
enum Kind {
    Check(bool),
    Number(&'static str),
    Text(&'static str),
    Choice(&'static [&'static str], &'static str),
}

/// One of the reference app's advanced settings: `advanced.<IntelliJ's id>`, its title, its control, the
/// label after the control and the comment under it.
struct Advanced {
    key: &'static str,
    title: &'static str,
    kind: Kind,
    trailing: &'static str,
    comment: &'static str,
}

impl Advanced {
    const fn new(key: &'static str, title: &'static str, kind: Kind) -> Self {
        Advanced {
            key,
            title,
            kind,
            trailing: "",
            comment: "",
        }
    }

    const fn trailing(mut self, trailing: &'static str) -> Self {
        self.trailing = trailing;
        self
    }

    const fn comment(mut self, comment: &'static str) -> Self {
        self.comment = comment;
        self
    }
}

/// Advanced Settings' search field and "Show modified only", for the Settings window showing them.
struct Search {
    window: EntityId,
    field: Entity<InputState>,
    modified_only: bool,
    _subscription: Subscription,
}

thread_local! {
    static SEARCH: RefCell<Option<Search>> = const { RefCell::new(None) };
}

/// Whether an advanced setting shows for `query` (lowercase): its group or its title has each
/// of the query's words, or its id has all of them as words, as IntelliJ's filter.
fn advanced_matches(group: &str, title: &str, key: &str, query: &str) -> bool {
    let has_all = |text: &str| {
        let text = text.to_lowercase();
        query.split_whitespace().all(|word| text.contains(word))
    };
    let id: Vec<&str> = key.trim_start_matches("advanced.").split('.').collect();
    has_all(group) || has_all(title) || query.split_whitespace().all(|word| id.contains(&word))
}

/// IntelliJ's drop-down link: the choice in the link color, a chevron after it.
fn drop_down(id: &'static str, label: &'static str, cx: &App) -> Stateful<Div> {
    let tokens = Tokens::get(cx);
    h_flex()
        .id(id)
        .cursor_pointer()
        .text_color(tokens.text_link)
        .child(label)
        .child(
            spork_workbench::icon("general/chevronDown")
                .size(px(16.))
                .text_color(tokens.text_link),
        )
        .on_click(not_yet(label))
}

impl SettingsWindow {
    /// The page at `path` when it is one of these, else None.
    pub(crate) fn tools_settings(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let page = match path {
            "Build Tools" => self.build_tools_page(cx),
            "Languages & Frameworks/Schemas and DTDs" => schemas_page(cx),
            "Languages & Frameworks/Schemas and DTDs/Default XML Schemas" => {
                self.default_schemas_page(window, cx)
            }
            "Languages & Frameworks/Schemas and DTDs/XML Catalog" => labeled(
                "Catalog property file:",
                self.opt_path(
                    "xmlCatalog.propertiesFile",
                    "",
                    420.,
                    true,
                    true,
                    window,
                    cx,
                )
                .into_any_element(),
            ),
            "Languages & Frameworks/Template Data Languages" => self.template_languages_page(cx),
            "Tools/Actions on Save" => self.actions_on_save_page(cx),
            "Tools/Diff & Merge/External Diff Tools" => self.external_diff_page(cx),
            "Tools/Startup Tasks" => startup_tasks_page(cx),
            "Tools/Web Browsers and Preview" => self.browsers_page(window, cx),
            "Backup and Sync" => {
                v_flex().child("Sync UI, Code and System settings, Keymaps, Plugins, and Tools.")
            }
            "Advanced Settings" => self.advanced_page(window, cx),
            _ => return None,
        };
        Some(page)
    }

    fn build_tools_page(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .gap_2()
            .child(self.opt_check(
                "buildTools.autoReload",
                "Sync project after changes in the build scripts:",
                true,
                cx,
            ))
            .child(
                v_flex()
                    .pl(px(20.))
                    .gap_1()
                    .child(self.opt_radio(
                        "buildTools.autoReloadType",
                        &["Any changes", "External changes"],
                        "External changes",
                        cx,
                    ))
                    .child(comment(
                        "Changes outside the IDE or from version control.",
                        cx,
                    )),
            )
    }

    fn template_languages_page(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .gap_2()
            .child(labeled(
                "Project Language:",
                self.opt_combo(
                    "templateDataLanguages.project",
                    &["<None>", "DTD", "HTML", "Plain text", "SVG", "XHTML", "XML"],
                    "<None>",
                    160.,
                    cx,
                )
                .into_any_element(),
            ))
            .child(table(
                vec![
                    tool("template-languages", "general/add", "Add", true).into_any_element(),
                    tool("template-languages", "general/remove", "Remove", false)
                        .into_any_element(),
                    tool("template-languages", "general/edit", "Edit", false).into_any_element(),
                ],
                &[("Path", None), ("Language", Some(160.))],
                Vec::new(),
                "New Mapping ⌘N",
                300.,
                cx,
            ))
            .child(note(
                "Template data languages are the underlying languages in template files like those of FreeMarker/Velocity frameworks. To change template data language settings Spork uses for a file, a directory, or the entire project, add its path if necessary and then select a language from the drop-down list.",
                cx,
            ))
    }

    fn default_schemas_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        // The reference app shows no field beside Other doctype until it is picked
        let other = self.value("defaultSchemas.html", "HTML 5") == "Other doctype:";
        let doctype = self.opt_text("defaultSchemas.otherDoctype", "", 320., window, cx);
        v_flex()
            .gap_2()
            .child("Default HTML language level:")
            .child(
                h_flex()
                    .pl(px(20.))
                    .items_end()
                    .gap_2()
                    .child(self.opt_radio(
                        "defaultSchemas.html",
                        &[
                            "HTML 4 (\"http://www.w3.org/TR/html4/loose.dtd\")",
                            "HTML 5",
                            "Other doctype:",
                        ],
                        "HTML 5",
                        cx,
                    ))
                    .when(other, |row| row.child(doctype)),
            )
            .child(div().pt_2().child("XML Schema version:"))
            .child(div().pl(px(20.)).child(self.opt_radio(
                "defaultSchemas.xmlSchema",
                &["XML Schema 1.0", "XML Schema 1.1"],
                "XML Schema 1.0",
                cx,
            )))
    }

    fn actions_on_save_page(&self, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        // the links to other pages show while the pointer is on their row, as IntelliJ's
        let link = |id: &'static str, label: &'static str, row: &'static str, cx: &App| {
            spork_workbench::controls::link(id, label, cx)
                .invisible()
                .group_hover(row, |style| style.visible())
                .on_click(not_yet(label))
        };
        let action = |row: &'static str, check: Stateful<Div>, extras: Vec<AnyElement>| {
            vec![
                h_flex()
                    .group(row)
                    .gap_3()
                    .child(check)
                    .children(extras)
                    .into_any_element(),
                div()
                    .text_color(tokens.text_disabled)
                    .child("Any save")
                    .into_any_element(),
            ]
        };
        let cleanup = v_flex()
            .py_1()
            .child(
                h_flex()
                    .group("on-save-cleanup")
                    .gap_3()
                    .child(self.opt_check(
                        "actionsOnSave.codeCleanup",
                        "Run code cleanup",
                        false,
                        cx,
                    ))
                    .child(link(
                        "on-save-inspections",
                        "Configure inspections…",
                        "on-save-cleanup",
                        cx,
                    ))
                    .child(drop_down("on-save-profile", "Project Profile", cx)),
            )
            .child(comment(
                "Applies fixes from the code cleanup inspections",
                cx,
            ));
        v_flex()
            .gap_2()
            .child(table(
                Vec::new(),
                &[("Action", None), ("Activated On", Some(150.))],
                vec![
                    action(
                        "on-save-reformat",
                        self.opt_check("actionsOnSave.reformat", "Reformat code", false, cx),
                        vec![
                            link("on-save-scope", "Configure scope…", "on-save-reformat", cx)
                                .into_any_element(),
                            drop_down("on-save-reformat-types", "All file types", cx)
                                .into_any_element(),
                            drop_down("on-save-reformat-lines", "Whole file", cx)
                                .into_any_element(),
                        ],
                    ),
                    action(
                        "on-save-imports",
                        self.opt_check(
                            "actionsOnSave.optimizeImports",
                            "Optimize imports",
                            false,
                            cx,
                        ),
                        vec![
                            drop_down("on-save-imports-types", "All file types", cx)
                                .into_any_element(),
                        ],
                    ),
                    action(
                        "on-save-rearrange",
                        self.opt_check("actionsOnSave.rearrange", "Rearrange code", false, cx),
                        vec![
                            link(
                                "on-save-arrangement",
                                "Configure arrangement rules…",
                                "on-save-rearrange",
                                cx,
                            )
                            .into_any_element(),
                            drop_down("on-save-rearrange-types", "All file types", cx)
                                .into_any_element(),
                        ],
                    ),
                    vec![
                        cleanup.into_any_element(),
                        div()
                            .text_color(tokens.text_disabled)
                            .child("Any save")
                            .into_any_element(),
                    ],
                ],
                "",
                260.,
                cx,
            ))
            .child(
                h_flex().child(
                    spork_workbench::controls::link(
                        "on-save-autosave",
                        "Configure autosave options…",
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show("Appearance & Behavior/System Settings", cx)
                    })),
                ),
            )
    }

    fn external_diff_page(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .gap_2()
            .child(self.opt_check("externalDiff.enabled", "Enable external tools", false, cx))
            .child(
                v_flex()
                    .pl(px(20.))
                    .gap_2()
                    .child("Configure external tools:")
                    .child(table(
                        vec![
                            tool("external-tools", "general/add", "Add", true).into_any_element(),
                            tool("external-tools", "general/remove", "Remove", false)
                                .into_any_element(),
                            tool("external-tools", "general/edit", "Edit", false)
                                .into_any_element(),
                        ],
                        &[("", None)],
                        Vec::new(),
                        "Nothing to show",
                        200.,
                        cx,
                    ))
                    .child(
                        div().pt_2().child(
                            "Configure external diff/merge tools associated with a file type:",
                        ),
                    )
                    .child(table(
                        vec![
                            tool("external-file-types", "general/add", "Add", true)
                                .into_any_element(),
                            tool("external-file-types", "general/remove", "Remove", false)
                                .into_any_element(),
                        ],
                        &[
                            ("File Type", None),
                            ("Diff Tool", Some(160.)),
                            ("Merge Tool", Some(160.)),
                        ],
                        vec![vec![
                            div()
                                .font_weight(FontWeight::BOLD)
                                .child("Default")
                                .into_any_element(),
                            "Built-in".into_any_element(),
                            "Built-in".into_any_element(),
                        ]],
                        "",
                        200.,
                        cx,
                    )),
            )
    }

    fn browsers_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        // WebBrowserManager's browsers on macOS: whether each is on by default, its name, family
        // and path
        const BROWSERS: &[(&str, bool, &str, &str, &str)] = &[
            (
                "webBrowsers.chrome",
                true,
                "Chrome",
                "Chrome",
                "Google Chrome",
            ),
            ("webBrowsers.firefox", true, "Firefox", "Firefox", "Firefox"),
            ("webBrowsers.safari", true, "Safari", "Safari", "Safari"),
            ("webBrowsers.opera", false, "Opera", "Chrome", "Opera"),
            (
                "webBrowsers.explorer",
                false,
                "Internet Explorer",
                "Internet Explorer",
                "",
            ),
            (
                "webBrowsers.edge",
                false,
                "Edge",
                "Chrome",
                "Microsoft Edge",
            ),
        ];
        let custom_path = self.opt_text("webBrowsers.customPath", "", 320., window, cx);
        let rows = BROWSERS
            .iter()
            .map(|&(key, active, name, family, path)| {
                vec![
                    self.opt_check(key, "", active, cx).into_any_element(),
                    name.into_any_element(),
                    family.into_any_element(),
                    path.into_any_element(),
                ]
            })
            .collect();
        const RELOAD: &[&str] = &["Disabled", "On Save", "On Change"];
        let indent = px(20.);
        v_flex()
            .gap_3()
            .child(table(
                vec![
                    tool("browsers", "general/add", "Add", true).into_any_element(),
                    tool("browsers", "general/remove", "Remove", false).into_any_element(),
                    tool("browsers", "general/edit", "Edit", false).into_any_element(),
                    tool("browsers", "general/moveUp", "Up", false).into_any_element(),
                    tool("browsers", "general/moveDown", "Down", false).into_any_element(),
                ],
                &[
                    ("", Some(32.)),
                    ("Name", None),
                    ("Family", Some(150.)),
                    ("Path", None),
                ],
                rows,
                "No web browsers configured",
                220.,
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child("Default Browser:")
                    .child(self.opt_combo(
                        "webBrowsers.default",
                        &["System default", "First listed", "Custom path"],
                        "System default",
                        150.,
                        cx,
                    ))
                    .child(custom_path),
            )
            .child(titled("Show browser popup in the editor", cx))
            .child(
                v_flex()
                    .pl(indent)
                    .gap_2()
                    .child(self.opt_check("webBrowsers.popupHtml", "For HTML files", true, cx))
                    .child(self.opt_check("webBrowsers.popupXml", "For XML files", false, cx)),
            )
            .child(titled("Reload behavior", cx))
            .child(
                v_flex()
                    .pl(indent)
                    .gap_2()
                    .child(labeled(
                        "Reload page in browser:",
                        self.opt_combo("webBrowsers.reloadBrowser", RELOAD, "On Save", 140., cx)
                            .into_any_element(),
                    ))
                    .child(labeled(
                        "Reload page in built-in preview:",
                        self.opt_combo("webBrowsers.reloadPreview", RELOAD, "On Save", 140., cx)
                            .into_any_element(),
                    )),
            )
    }

    fn advanced_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let owner = cx.entity_id();
        let current = SEARCH.with_borrow(|search| {
            search
                .as_ref()
                .filter(|search| search.window == owner)
                .map(|search| (search.field.clone(), search.modified_only))
        });
        let (field, modified_only) = match current {
            Some(current) => current,
            None => {
                let field = cx
                    .new(|cx| InputState::new(window, cx).placeholder("Search advanced settings"));
                let _subscription = cx.subscribe(&field, |_, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify()
                    }
                });
                SEARCH.set(Some(Search {
                    window: owner,
                    field: field.clone(),
                    modified_only: false,
                    _subscription,
                }));
                (field, false)
            }
        };
        let query = field.read(cx).value().trim().to_lowercase();
        let mut groups = Vec::new();
        for &(group, settings) in ADVANCED {
            let mut rows = Vec::new();
            for setting in settings {
                let modified = match setting.kind {
                    Kind::Check(default) => self.flag(setting.key, default) != default,
                    Kind::Number(default) | Kind::Text(default) | Kind::Choice(_, default) => {
                        self.value(setting.key, default) != default
                    }
                };
                if (modified_only && !modified)
                    || !advanced_matches(group, setting.title, setting.key, &query)
                {
                    continue;
                }
                rows.push(self.advanced_row(setting, window, cx));
            }
            if !rows.is_empty() {
                groups.push(
                    v_flex()
                        .gap_2()
                        .child(titled(group, cx))
                        .child(v_flex().pl(px(20.)).gap_2().children(rows)),
                );
            }
        }
        let nothing = groups.is_empty();
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        div().flex_1().child(
                            spork_workbench::TextField::new(&field)
                                .prefix(spork_workbench::icon("general/search").size(px(16.))),
                        ),
                    )
                    .child(
                        spork_workbench::controls::labeled_check(
                            "advanced-modified-only",
                            "Show modified only",
                            modified_only,
                            cx,
                        )
                        .on_click(cx.listener(|_, _, _, cx| {
                            SEARCH.with_borrow_mut(|search| {
                                if let Some(search) = search {
                                    search.modified_only = !search.modified_only;
                                }
                            });
                            cx.notify();
                        })),
                    ),
            )
            .when(nothing, |page| {
                page.child(
                    div()
                        .pt_4()
                        .flex()
                        .justify_center()
                        .text_color(Tokens::get(cx).text_secondary)
                        .child("Nothing found"),
                )
            })
            .children(groups)
    }

    /// One advanced setting: its checkbox, or its title before its field or combo box and the
    /// label after it; its comment under it.
    fn advanced_row(
        &mut self,
        setting: &'static Advanced,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let control = match setting.kind {
            Kind::Check(default) => {
                return v_flex()
                    .gap_1()
                    .child(self.opt_check(setting.key, setting.title, default, cx))
                    .when(!setting.comment.is_empty(), |row| {
                        row.child(comment(setting.comment, cx))
                    })
                    .into_any_element();
            }
            Kind::Number(default) => self.opt_text(setting.key, default, 80., window, cx),
            Kind::Text(default) => self.opt_text(setting.key, default, 240., window, cx),
            Kind::Choice(choices, default) => {
                self.opt_combo(setting.key, choices, default, 240., cx)
            }
        };
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .child(format!("{}:", setting.title))
                    .child(control)
                    .when(!setting.trailing.is_empty(), |row| {
                        row.child(setting.trailing)
                    }),
            )
            .when(!setting.comment.is_empty(), |row| {
                row.child(note(setting.comment, cx))
            })
            .into_any_element()
    }
}

/// Schemas and DTDs: the external resources mapped by URI, and those ignored.
fn schemas_page(cx: &mut Context<SettingsWindow>) -> Div {
    let toolbar = |id: &'static str| {
        vec![
            tool(id, "general/add", "Add", true).into_any_element(),
            tool(id, "general/remove", "Remove", false).into_any_element(),
            tool(id, "general/edit", "Edit", false).into_any_element(),
        ]
    };
    v_flex()
        .gap_2()
        .child("External schemas and DTDs:")
        .child(table(
            toolbar("external-resources"),
            &[("URI", None), ("Location", None), ("Project", Some(56.))],
            Vec::new(),
            "No external resources",
            200.,
            cx,
        ))
        .child(div().pt_2().child("Ignored schemas and DTDs:"))
        .child(table(
            toolbar("ignored-resources"),
            &[("URI", None)],
            Vec::new(),
            "No ignored resources",
            200.,
            cx,
        ))
}

/// Startup Tasks: the run configurations started when the project opens.
fn startup_tasks_page(cx: &mut Context<SettingsWindow>) -> Div {
    v_flex()
        .gap_2()
        .child(
            h_flex()
                .justify_between()
                .child("To be started on project opening:")
                .child(note("Run tasks and tools via run configurations", cx)),
        )
        .child(table(
            vec![
                tool("startup-tasks", "general/add", "Add", true)
                    .popup_marker()
                    .into_any_element(),
                tool("startup-tasks", "general/remove", "Remove", false).into_any_element(),
                tool("startup-tasks", "general/edit", "Edit", false).into_any_element(),
            ],
            &[("Run Configuration", None), ("Shared", Some(64.))],
            Vec::new(),
            "Add run configurations with the + button",
            300.,
            cx,
        ))
}

/// The reference app's advanced settings: IntelliJ's groups in their order (by name), each group's settings
/// in the order their plugins declare them (`advancedSetting` in the plugin XMLs; titles,
/// trailing labels and comments from their bundles). Windows-only and hidden settings are left
/// out, as the reference app on macOS shows them.
const ADVANCED: &[(&str, &[Advanced])] = &[
    (
        "Appearance",
        &[
            Advanced::new("advanced.ide.ui.theme.custom.islands", "Enable Islands UI for custom themes", Kind::Check(false)).comment("Requires restart"),
        ],
    ),
    (
        "Bookmarks",
        &[
            Advanced::new("advanced.show.line.bookmarks.in.popup", "Show only line bookmarks in popup", Kind::Check(true)),
            Advanced::new("advanced.tasks.enable.bookmark.context.on.branch.switch", "Restore bookmark context on branch switching", Kind::Check(true)),
        ],
    ),
    (
        "Code Review Tools",
        &[
            Advanced::new("advanced.enable.combined.diff.for.codereview", "All-in-one Diff for Code Reviews", Kind::Check(false)).comment("If enabled, all changed files in code reviews are shown in a single scrollable Diff view"),
            Advanced::new("advanced.show.review.threads.with.increased.margins", "Keep increased margins for Diff threads", Kind::Check(true)),
        ],
    ),
    (
        "Debugger",
        &[
            Advanced::new("advanced.debugger.max.recent.expressions", "Maximum number of recent expressions", Kind::Number("50")),
            Advanced::new("advanced.debugger.inlay.run.to.cursor", "Show inlay Run to Cursor popup", Kind::Check(true)).comment("Note, inlay Run to Cursor is implemented for the new UI only. In the classic UI, the click on line number will execute Run to Cursor."),
        ],
    ),
    (
        "Documentation Components",
        &[
            Advanced::new("advanced.documentation.components.enable.inline.code.highlighting", "Basic syntax highlighting of inline code", Kind::Choice(&["No highlighting", "As plain code", "With keywords highlighted"], "As plain code")),
            Advanced::new("advanced.documentation.components.enable.code.blocks.highlighting", "Basic syntax highlighting of multiline code blocks", Kind::Check(true)),
            Advanced::new("advanced.documentation.components.enable.highlighting.of.links", "Syntax highlighting of links on code elements", Kind::Check(false)),
            Advanced::new("advanced.documentation.components.enable.code.background", "Add background to inline and multiline code blocks", Kind::Check(true)),
            Advanced::new("advanced.documentation.auto.show.in.modal.dialogs", "Show QuickDoc automatically during code completion in modal dialogs", Kind::Check(false)),
        ],
    ),
    (
        "Editor",
        &[
            Advanced::new("advanced.editor.undo.transparent.caret.movement", "Use undo transparent caret movement", Kind::Check(false)),
            Advanced::new("advanced.editor.skip.copy.and.cut.for.empty.selection", "Don't copy/cut the current line when invoking the Copy or Cut action with no selection", Kind::Check(false)),
            Advanced::new("advanced.editor.skip.selecting.line.after.copy.empty.selection", "Don't select the copied line after invoking the Copy action with no selection", Kind::Check(false)),
            Advanced::new("advanced.editor.paste.line.copied.from.empty.selection", "When pasting a line copied with no selection", Kind::Choice(&["Paste above the caret line", "Paste at the caret position", "Trim if pasted middle line"], "Paste above the caret line")),
            Advanced::new("advanced.editor.show.special.chars", "Render special characters, such as control codes, using their Unicode name abbreviations", Kind::Check(true)),
            Advanced::new("advanced.editor.show.zoom.indicator", "Show zoom indicator", Kind::Check(true)),
            Advanced::new("advanced.editor.tab.painting", "Tab character rendering", Kind::Choice(&["Long arrow", "> symbol", "Horizontal line"], "Horizontal line")),
            Advanced::new("advanced.editor.distraction.free.margin", "Left margin in distraction free mode", Kind::Number("-1")).trailing("pixels").comment("Specify -1 to use automatically calculated margin (centering the editor)"),
            Advanced::new("advanced.editor.gutter.linenumber.font.size.delta", "Line number's font delta", Kind::Number("-1")).trailing("points").comment("This value determines the increase or decrease in the editor line number font size compared to the editor's font size"),
            Advanced::new("advanced.editor.soft.wrap.force.limit", "Force soft wrap in documents with lines longer than", Kind::Number("100000")).trailing("characters").comment("Increasing this limit will negatively affect editor performance"),
            Advanced::new("advanced.editor.selection.expand-whitespaces", "Select whitespace characters with 'Extend Selection'", Kind::Check(true)),
            Advanced::new("advanced.editor.comment.line.caret.down", "Move caret down after Comment with Line Comment action", Kind::Check(true)),
            Advanced::new("advanced.floating.codeToolbar.hide", "Hide floating toolbar for code editing", Kind::Check(false)),
        ],
    ),
    (
        "Editor Tabs",
        &[
            Advanced::new("advanced.editor.open.inactive.splitter", "When navigating to a file, prefer selecting existing tab in inactive split pane", Kind::Check(true)),
            Advanced::new("advanced.editor.reuse.not.modified.tabs", "Open declaration source in the same tab", Kind::Check(false)).comment("When navigating to a method/class/variable declaration, the source file that contains the declaration will replace the current tab if there are no changes"),
            Advanced::new("advanced.editor.open.tabs.in.main.window", "Open declaration source called from a detached window in the main IDE window", Kind::Check(false)).comment("When navigating to a method/class/variable declaration from a tab in the detached window, new tabs will be opened in the main IDE window. Overrides 'Open declaration source in the same tab'."),
            Advanced::new("advanced.editor.maximize.on.double.click", "Perform 'Hide All Tool Windows' / 'Restore Windows' with double-click on editor tab", Kind::Check(true)),
            Advanced::new("advanced.editor.maximize.in.splits.on.double.click", "Perform 'Maximize Editor' / 'Normalize Splits' with double-click on editor tab", Kind::Check(false)),
            Advanced::new("advanced.editor.normalize.splits", "Equalize proportions in nested splits", Kind::Check(false)).comment("Adjust splitters to keep space equally distributed after split/unsplit."),
            Advanced::new("advanced.editor.keep.pinned.tabs.on.left", "Keep pinned tabs on the left", Kind::Check(true)),
        ],
    ),
    (
        "Find/Replace",
        &[
            Advanced::new("advanced.ide.similar.usages.clustering.enable", "Enable similar usages clustering in Find Usages view", Kind::Check(true)),
            Advanced::new("advanced.ide.usages.page.size", "Maximum number of results to show in Find in Files/Show Usages preview", Kind::Number("100")).comment("Increasing this limit can significantly increase IDE memory usage"),
            Advanced::new("advanced.ide.find.result.count.warning.limit", "Limit at which the \"Too many results found\" warning is shown", Kind::Number("1000")),
        ],
    ),
    (
        "GitHub",
        &[
            Advanced::new("advanced.github.aliases", "GitHub host aliases", Kind::Text("github.com")).comment("Comma-separated list of host aliases. An \"alias=server\" entry maps the alias to a GitHub server URL or host. An \"alias\" entry maps the alias to github.com. Example: work=github.example.com, legacy=https://git.example.com:8443/github, old-alias"),
        ],
    ),
    (
        "GitLab",
        &[
            Advanced::new("advanced.gitlab.aliases", "GitLab host aliases", Kind::Text("gitlab.com")).comment("Comma-separated list of host aliases. An \"alias=server\" entry maps the alias to a GitLab server URL or host. An \"alias\" entry maps the alias to gitlab.com. Example: work=gitlab.example.com, legacy=http://git.example.com:8080/gitlab, old-alias"),
        ],
    ),
    (
        "IDE",
        &[
            Advanced::new("advanced.localHistory.daysToKeep", "Duration of storing changes in Local History", Kind::Number("5")).trailing("days").comment("Do not use Local History as the primary version control system since it may become corrupted if the IDE hangs"),
            Advanced::new("advanced.ide.max.recent.projects", "Maximum number of recent projects", Kind::Number("50")),
            Advanced::new("advanced.ide.max.recent.files", "Maximum number of recent files", Kind::Number("50")),
            Advanced::new("advanced.ide.max.recent.locations", "Maximum number of recent locations", Kind::Number("25")),
            Advanced::new("advanced.welcome.screen.non.modal.enabled", "Show Home window when the last project is closed", Kind::Check(true)).comment("If disabled, shows the Welcome screen"),
        ],
    ),
    (
        "Project View",
        &[
            Advanced::new("advanced.bigger.font.in.project.view", "Increase font size in Project view", Kind::Check(false)).comment("Requires IDE restart"),
            Advanced::new("advanced.edit.source.on.enter.key.request.focus.in.editor", "Move focus to editor when Enter is pressed", Kind::Check(true)),
            Advanced::new("advanced.ide.tree.collapse.recursively", "When collapsing a node, also collapse all expanded nodes under it", Kind::Check(true)),
            Advanced::new("advanced.project.view.do.not.autoscroll.to.libraries", "When navigating to a library class, do not select it in the project tree", Kind::Check(false)),
            Advanced::new("advanced.project.view.show.scratches.and.consoles", "Enable Scratches and Consoles in the Project view", Kind::Check(true)),
        ],
    ),
    (
        "Run/Debug",
        &[
            Advanced::new("advanced.temporary.configurations.limit", "Temporary configurations limit", Kind::Number("5")),
            Advanced::new("advanced.confirm.rerun.with.termination", "Confirm rerun with process termination", Kind::Check(true)),
            Advanced::new("advanced.start.run.configurations.pinned", "Make configurations pinned by default", Kind::Check(false)),
            Advanced::new("advanced.max.recent.run.configurations", "Maximum number of recent run configurations", Kind::Number("5")),
        ],
    ),
    (
        "Search Everywhere",
        &[
            Advanced::new("advanced.search.everywhere.wait.for.contributors", "Wait for all contributors to finish before showing results", Kind::Check(true)).comment("Enabling this option will stop the search items to change their position in the displayed search results. This process might cause a delay."),
            Advanced::new("advanced.search.everywhere.show.results.notification", "Show notifications above search results", Kind::Check(true)),
            Advanced::new("advanced.search.everywhere.contributors.wait.timeout", "Contributors waiting timeout (ms)", Kind::Number("2000")),
            Advanced::new("advanced.search.everywhere.recent.at.top", "Show recent files at the top of results list", Kind::Check(true)).comment("When enabled, recent files are always displayed at the top of the results list. Therefore, even if these files match the search pattern less precisely than other results, they will remain at the top."),
            Advanced::new("advanced.se.text.search", "Show text search results in Search Everywhere", Kind::Check(true)).comment("Show text search results in the \"All\" tab and display the \"Text\" tab in Search Everywhere"),
        ],
    ),
    (
        "Search Scopes in Find, Replace, Rename",
        &[
            Advanced::new("advanced.ide.remember.last.search.scope", "Keep last selected search scope", Kind::Check(false)),
        ],
    ),
    (
        "Startup",
        &[
            Advanced::new("advanced.ide.open.readme.md.on.startup", "Open README.md file if there are no open files on the first project startup", Kind::Check(false)),
        ],
    ),
    (
        "Suggested refactoring",
        &[
            Advanced::new("advanced.promote.suggested.refactoring.in.editor", "Show a hint for suggested refactoring in the editor", Kind::Check(false)),
        ],
    ),
    (
        "Terminal",
        &[
            Advanced::new("advanced.terminal.buffer.max.lines.count", "Terminal scrollback buffer size", Kind::Number("5000")).trailing("lines"),
            Advanced::new("advanced.terminal.type.ahead", "Typeahead", Kind::Check(true)),
            Advanced::new("advanced.terminal.type.ahead.latency.threshold", "Typeahead latency threshold", Kind::Number("100")).trailing("ms"),
            Advanced::new("advanced.terminal.use.1.0.line.spacing.for.alternative.screen.buffer", "Use 1.0 line spacing for alternative screen buffer (mc, vim, nano, etc.)", Kind::Check(true)),
            Advanced::new("advanced.terminal.fill.character.background.including.line.spacing", "Fill character background including line spacing", Kind::Check(false)),
            Advanced::new("advanced.terminal.character.encoding", "Terminal character encoding", Kind::Text("UTF-8")),
            Advanced::new("advanced.new.terminal.output.capacity.kb", "New terminal output capacity", Kind::Number("1024")).trailing("KB"),
            Advanced::new("advanced.terminal.shift.enter.sends.esc.cr", "Send Esc+CR on Shift+Enter", Kind::Check(true)).comment("Enables sending Esc+Carriage Return on Shift+Enter, allowing CLI applications to distinguish Shift+Enter from Enter. Applies to the JediTerm emulator. The Ghostty emulator sends the xterm sequence for Shift+Enter instead."),
        ],
    ),
    (
        "Tool Windows",
        &[
            Advanced::new("advanced.ide.always.show.tool.window.header.icons", "Always show tool window header icons", Kind::Check(false)).comment("If this option is disabled, tool window header icons are shown only on mouse hover or when the tool window is focused"),
            Advanced::new("advanced.ide.tool.window.header.dnd", "Allow dragging tool windows by header to move to another location", Kind::Check(true)),
        ],
    ),
    (
        "User Interface",
        &[
            Advanced::new("advanced.ide.ui.native.file.chooser", "Use native file chooser dialog on Windows/macOS", Kind::Check(true)).comment("When enabled, the IDE will use an OS-specific file chooser dialog instead of the custom one for opening files/directories."),
            Advanced::new("advanced.merge.main.menu", "Merge main menu with window title", Kind::Check(true)),
            Advanced::new("advanced.ide.show.fileType.icon.in.titleBar", "Show file type icon in IDE frame header", Kind::Check(false)).comment("On macOS, the icon can be used to drag a file to a different application"),
            Advanced::new("advanced.ide.macos.disable.native.shortcut.symbols", "Use words instead of symbols for macOS keyboard shortcuts", Kind::Check(false)).comment("Change shortcut symbols ⎋, ⌫, ⌥, and others to Esc, Backspace, Option, etc"),
            Advanced::new("advanced.ide.trigger.menu.actions.on.rmb.release", "Activate menu items on right mouse button release", Kind::Check(true)).comment("For macOS and Linux only"),
            Advanced::new("advanced.ide.cycle.scrolling", "Cyclic scrolling in lists and trees", Kind::Check(true)).comment("Select the last element when pressing Up on the first element, and vice versa"),
            Advanced::new("advanced.ide.settings.move.mouse.on.default.button", "Position mouse cursor on default button in dialogs", Kind::Check(false)),
            Advanced::new("advanced.ide.suppress.double.click.handler", "Disable double modifier key shortcuts", Kind::Check(false)).comment("Shift-Shift for Search Everywhere, Ctrl-Ctrl for Run Anything"),
            Advanced::new("advanced.ide.ui.non.modal.settings.window", "Show settings in a non-modal dialog", Kind::Check(true)).comment("Settings dialog stays open alongside the editor without blocking other IDE actions."),
            Advanced::new("advanced.ide.ui.simplified.splash.image", "Use simplified splash screen", Kind::Check(false)),
        ],
    ),
    (
        "Version Control",
        &[
            Advanced::new("advanced.show.diff.as.editor.tab", "Open Diff as Editor Tab", Kind::Check(true)).comment("If enabled, Diff is shown in an editor tab instead of a separate window"),
            Advanced::new("advanced.enable.combined.diff", "All-in-One Diff", Kind::Check(false)).comment("If enabled, all changed files are shown in a single scrollable Diff view"),
            Advanced::new("advanced.vcs.annotations.preload", "Load file annotations from VCS when file is opened in editor", Kind::Check(false)).comment("This allows showing them faster on demand"),
            Advanced::new("advanced.vcs.process.ignored", "Highlight ignored files", Kind::Check(true)).comment("Request the list of the ignored files from the VCS and show their status in the IDE"),
            Advanced::new("advanced.vcs.commit.tool.window", "Enable Commit tool window", Kind::Check(true)).comment("Show the Local Changes and Shelf tabs in the separate Commit tool window"),
            Advanced::new("advanced.vcs.non.modal.commit.toggle.ui", "Toggle commit controls", Kind::Check(false)).comment("Hide commit panel and checkboxes after commit is performed and allow toggling commit UI in Local Changes"),
            Advanced::new("advanced.vcs.non.modal.commit.close.in.windowed.mode", "Automatically close the Commit tool window in Window/Float mode after committing", Kind::Check(true)),
            Advanced::new("advanced.vcs.push.all.with.commits", "Select all repositories with new commits for push", Kind::Check(false)).comment("Preselect all repositories with commits to be pushed by default"),
            Advanced::new("advanced.vcs.allow.force.push.without.confirmation", "Do not show a warning when force pushing", Kind::Check(false)).comment("When force pushing to unprotected branches, no confirmation will be asked"),
            Advanced::new("advanced.git.non.modal.commit", "Use modal commit interface for Git and Mercurial", Kind::Check(false)),
        ],
    ),
    (
        "Version Control. Git",
        &[
            Advanced::new("advanced.git.clone.recurse.submodules", "Recursively clone submodules in the project", Kind::Check(true)).comment("When enabled, Checkout from Git calls 'git clone --recurse-submodules', i.e. clones the main repository with all submodules if there are any"),
            Advanced::new("advanced.git.read.content.with", "Apply content transformation when reading from Git", Kind::Choice(&["None", "Filters", "TextConv"], "Filters")).comment("Use '--filters' or '--textconv' flags for 'git cat-file' command when reading file content from Git"),
            Advanced::new("advanced.git.use.push.force.with.lease", "Use Safe Force Push", Kind::Check(true)).comment("Use '--force-with-lease' when calling force push from IDE instead of just '--force'"),
            Advanced::new("advanced.git.update.incoming.outgoing.info", "Check for incoming and outgoing commits", Kind::Check(true)).comment("Show incoming and outgoing commits for branches"),
            Advanced::new("advanced.git.update.info.auto.open.enabled", "Automatically open the 'Update Info' tab after updating the project", Kind::Check(true)),
            Advanced::new("advanced.git.commit.do.not.run.commit.hooks", "Do not run Git commit hooks", Kind::Check(false)).comment("When enabled, commit always runs with '--no-verify' option"),
            Advanced::new("advanced.git.branch.cleanup.symbol", "Branch name cleanup symbol", Kind::Text("-")).comment("Symbol that will be used instead prohibited symbols in Git branch names"),
            Advanced::new("advanced.git.recent.projects.show.branch", "Show branch names in the list of recent projects", Kind::Choice(&["Never", "Projects with identical names", "All projects"], "All projects")).comment("Branch names will be shown on the Welcome screen and in the list of recent projects in the main menu"),
            Advanced::new("advanced.git.search.everywhere.tab.enabled", "Show Git tab in Search Everywhere", Kind::Check(false)),
        ],
    ),
];

#[cfg(test)]
mod tests {
    use super::advanced_matches;

    #[test]
    fn advanced_search_finds_titles_groups_and_ids() {
        let find = |query| {
            advanced_matches(
                "Version Control. Git",
                "Use Safe Force Push",
                "advanced.git.use.push.force.with.lease",
                query,
            )
        };
        assert!(find(""));
        assert!(find("safe force"));
        assert!(find("version control"));
        assert!(find("lease git"));
        assert!(!find("lea"));
        assert!(!find("terminal"));
    }
}
