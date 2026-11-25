//! Settings ▸ Version Control and its pages, as the reference app's: IntelliJ's VcsManagerConfigurable lists
//! them by weight, then by name (Git, GitHub and GitLab weigh -500). Commit, Git and GitHub are
//! Spork's own pages; the group's own page lists its pages, as IntelliJ's has no settings.

use std::cell::{Cell, RefCell};

use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_hosting::Service;
use spork_workbench::tokens::metrics;
use spork_workbench::{ActionButton, Tokens, Typography as _};

use crate::notices::Notification;
use crate::settings_window::{Node, SettingsWindow, comment, labeled, titled};

/// The pages under Version Control. IntelliJ marks those of the project; File Status Colors is
/// the IDE's.
pub(crate) const VERSION_CONTROL: &[Node] = &[
    Node::page("Changelists").per_project(),
    Node::page("Commit").per_project(),
    Node::page("Confirmation").per_project(),
    Node::page("Directory Mappings").per_project(),
    Node::page("File Status Colors"),
    Node::page("Issue Navigation").per_project(),
    Node::page("Log").per_project(),
    Node::page("Shelf").per_project(),
    Node::page("Git").per_project(),
    Node::page("GitHub").per_project(),
    Node::page("GitLab").per_project(),
];

/// IntelliJ's file statuses as File Status Colors lists them (sorted by name): the name, the
/// setting that turns its color on, and its color in Islands Dark (as the reference app shows it: Hijacked,
/// Obsolete and Switched keep the text color there) and in Islands Light (expUI_lightScheme.xml
/// over Default); None: the text color.
const FILE_STATUSES: &[(&str, &str, Option<u32>, Option<u32>)] = &[
    (
        "Added",
        "fileStatusColors.ADDED",
        Some(0x73BD79),
        Some(0x067D17),
    ),
    (
        "Added in not active changelist",
        "fileStatusColors.addedOutside",
        Some(0x73BD79),
        Some(0x067D17),
    ),
    (
        "Changelist conflict",
        "fileStatusColors.changelistConflict",
        Some(0xDE6A66),
        Some(0xDE1B2E),
    ),
    (
        "Deleted",
        "fileStatusColors.DELETED",
        Some(0x6F737A),
        Some(0x6C707E),
    ),
    (
        "Deleted from file system",
        "fileStatusColors.IDEA_FILESTATUS_DELETED_FROM_FILE_SYSTEM",
        Some(0x6F737A),
        Some(0x6C707E),
    ),
    (
        "Have changed descendants",
        "fileStatusColors.NOT_CHANGED_RECURSIVE",
        Some(0x70AEFF),
        Some(0x0033B3),
    ),
    (
        "Have immediate changed children",
        "fileStatusColors.NOT_CHANGED_IMMEDIATE",
        Some(0x70AEFF),
        Some(0x0033B3),
    ),
    (
        "Hijacked",
        "fileStatusColors.HIJACKED",
        None,
        Some(0xB28C00),
    ),
    (
        "Ignored",
        "fileStatusColors.IDEA_FILESTATUS_IGNORED",
        Some(0xD69A6B),
        Some(0x8C4F00),
    ),
    (
        "Merged",
        "fileStatusColors.MERGED",
        Some(0xCF84CF),
        Some(0x643CB8),
    ),
    (
        "Merged with conflicts",
        "fileStatusColors.IDEA_FILESTATUS_MERGED_WITH_CONFLICTS",
        Some(0xDE6A66),
        Some(0xDE1B2E),
    ),
    (
        "Merged with property conflicts",
        "fileStatusColors.IDEA_FILESTATUS_MERGED_WITH_PROPERTY_CONFLICTS",
        Some(0xDE6A66),
        Some(0xDE1B2E),
    ),
    (
        "Merged with text and property conflicts",
        "fileStatusColors.IDEA_FILESTATUS_MERGED_WITH_BOTH_CONFLICTS",
        Some(0xDE6A66),
        Some(0xDE1B2E),
    ),
    (
        "Modified",
        "fileStatusColors.MODIFIED",
        Some(0x70AEFF),
        Some(0x0033B3),
    ),
    (
        "Modified in not active changelist",
        "fileStatusColors.modifiedOutside",
        Some(0x70AEFF),
        Some(0x0033B3),
    ),
    (
        "Obsolete",
        "fileStatusColors.OBSOLETE",
        None,
        Some(0x7C7C00),
    ),
    ("Suppressed", "fileStatusColors.SUPPRESSED", None, None),
    (
        "Switched",
        "fileStatusColors.SWITCHED",
        None,
        Some(0x08978F),
    ),
    (
        "Unknown",
        "fileStatusColors.UNKNOWN",
        Some(0xE88F89),
        Some(0xB23247),
    ),
    ("Up to date", "fileStatusColors.NOT_CHANGED", None, None),
];

thread_local! {
    /// What the pages show that isn't a setting, for the Settings window showing it: the row
    /// selected in File Status Colors, and whether GitLab's OAuth group is open.
    static VIEW: Cell<Option<(EntityId, Option<usize>, bool)>> = const { Cell::new(None) };
}

/// A connected account: its server, its user and the server's service.
type Account = (String, String, Option<Service>);

thread_local! {
    /// The connected accounts, read once for the Settings window showing them: git config is a
    /// process each time and pages draw often.
    // shortcut: an account connected while Settings is open shows once it opens again; read
    // again on the connect dialog's success if that matters
    static ACCOUNTS: RefCell<Option<(EntityId, Vec<Account>)>> = const { RefCell::new(None) };
}

/// `(host, user)` of each account connected on `service`'s servers, for the `owner` window.
fn accounts(owner: EntityId, service: Service) -> Vec<(String, String)> {
    ACCOUNTS.with_borrow_mut(|cache| {
        let list = match cache {
            Some((window, list)) if *window == owner => list,
            _ => {
                let list = spork_git::config::global_matching(r"^spork\..*\.account$")
                    .into_iter()
                    .filter_map(|(key, user)| {
                        let host = key.strip_prefix("spork.")?.strip_suffix(".account")?;
                        Some((host.to_owned(), user, crate::hosting::service_for(host)))
                    })
                    .collect();
                &mut cache.insert((owner, list)).1
            }
        };
        list.iter()
            .filter(|(_, _, on)| *on == Some(service))
            .map(|(host, user, _)| (host.clone(), user.clone()))
            .collect()
    })
}

thread_local! {
    /// The commit message inspection selected in Version Control › Commit.
    static COMMIT_INSPECTION: Cell<usize> = const { Cell::new(0) };
}

/// This window's view state: the selected file status and the open OAuth group.
fn view_state(window: EntityId) -> (Option<usize>, bool) {
    match VIEW.get() {
        Some((owner, status, open)) if owner == window => (status, open),
        _ => (None, false),
    }
}

/// A click on a control for a dialog Spork doesn't have yet: says so, as `opt_button` does.
pub(crate) fn not_yet(what: &'static str) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, window, cx| {
        window.push_notification(
            Notification::info(format!(
                "{} isn't in Spork yet.",
                what.trim_end_matches('…')
            )),
            cx,
        )
    }
}

/// A button of a table's toolbar (IntelliJ's ToolbarDecorator); off while it needs a selected row.
pub(crate) fn tool(
    table: &str,
    icon: &'static str,
    tooltip: &'static str,
    enabled: bool,
) -> ActionButton {
    ActionButton::new(SharedString::from(format!("{table}-{icon}")), icon)
        .tooltip(tooltip)
        .disabled(!enabled)
        .on_click(not_yet(tooltip))
}

/// IntelliJ's decorated table: the toolbar on top, the column headers (none when they are all
/// empty), the rows, or the empty text when there are none. A column without a width takes the
/// space left.
pub(crate) fn table(
    toolbar: Vec<AnyElement>,
    columns: &[(&'static str, Option<f32>)],
    rows: Vec<Vec<AnyElement>>,
    empty: impl IntoElement,
    height: f32,
    cx: &App,
) -> Div {
    let tokens = Tokens::get(cx);
    let line = |cells: Vec<AnyElement>| {
        h_flex()
            .min_h(px(metrics::ROW_HEIGHT))
            .children(cells.into_iter().zip(columns).map(|(cell, (_, width))| {
                let column = div().px_2().truncate();
                match width {
                    Some(width) => column.w(px(*width)).flex_none(),
                    None => column.flex_1().min_w_0(),
                }
                .child(cell)
            }))
    };
    let header = columns.iter().any(|(title, _)| !title.is_empty()).then(|| {
        line(
            columns
                .iter()
                .map(|(title, _)| (*title).into_any_element())
                .collect(),
        )
        .text_color(tokens.text_muted)
        .border_b_1()
        .border_color(tokens.control_border)
    });
    let body = match rows.is_empty() {
        true => div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .text_color(tokens.text_secondary)
            .child(empty),
        false => v_flex().flex_1().children(rows.into_iter().map(line)),
    };
    v_flex()
        .h(px(height))
        .border_1()
        .border_color(tokens.control_border)
        .bg(tokens.control_bg)
        .when(!toolbar.is_empty(), |table| {
            table.child(
                h_flex()
                    .p(px(2.))
                    .gap(px(2.))
                    .border_b_1()
                    .border_color(tokens.control_border)
                    .children(toolbar),
            )
        })
        .children(header)
        .child(body)
}

/// IntelliJ's comment under a table or a labeled row: small grey text, not indented.
pub(crate) fn note(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_ui_small()
        .text_color(Tokens::get(cx).text_secondary)
        .child(text.into())
}

/// An option and the word after its field, on one line ("Limit history to [1000] rows").
fn inline(parts: impl IntoIterator<Item = AnyElement>) -> Div {
    h_flex().gap_2().children(parts)
}

/// The badge after the app's own settings, the reference app's "<app name> setting" (docs/reference-spec.md,
/// #367) with this app's name.
fn reference_app_badge(cx: &App) -> Div {
    let tokens = Tokens::get(cx);
    div()
        .h(px(16.))
        .px(px(6.))
        .flex()
        .items_center()
        .rounded_full()
        .bg(tokens.pressed)
        .text_color(tokens.text_muted)
        .text_ui_xsmall()
        .font_weight(FontWeight::MEDIUM)
        .child(format!("{} setting", crate::ide::app_name(cx)))
}

/// IntelliJ's context help: the question mark that shows `text` on hover.
fn context_help(id: &'static str, text: &'static str, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .child(
            spork_workbench::icon("general/questionMark")
                .size(px(16.))
                .text_color(Tokens::get(cx).icon),
        )
        .tooltip(move |window, cx| Tooltip::new(text).build(window, cx))
}

impl SettingsWindow {
    /// The page at `path` when it is one of these, else None.
    pub(crate) fn vcs_settings(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if path == "Version Control" {
            return Some(self.version_control_page(cx));
        }
        let page = match path.strip_prefix("Version Control/")? {
            "Changelists" => self.changelists_page(cx),
            "Confirmation" => self.confirmation_page(window, cx),
            "Directory Mappings" => self.mappings_page(cx),
            "File Status Colors" => self.file_status_page(cx),
            "Issue Navigation" => issue_navigation_page(cx),
            "Log" => self.log_page(window, cx),
            "Shelf" => self.shelf_page(cx),
            "GitLab" => self.gitlab_page(window, cx),
            _ => return None,
        };
        Some(page)
    }

    /// Version Control › Commit, as the reference app's: Clear initial commit message, the commit message
    /// inspections with their checkboxes, and the severity of the one selected.
    pub(crate) fn vcs_commit_page(&self, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let selected = COMMIT_INSPECTION.get();
        let rows = crate::changes::COMMIT_INSPECTIONS
            .iter()
            .enumerate()
            .map(|(ix, (key, label))| {
                let (key, on) = (*key, self.flag(key, false));
                h_flex()
                    .id(("commit-inspection", ix))
                    .role(Role::Row)
                    .aria_label(*label)
                    .aria_selected(ix == selected)
                    .h(px(metrics::ROW_HEIGHT))
                    .px_2()
                    .gap_2()
                    .when(ix == selected, |row| row.bg(tokens.selection))
                    .child(div().flex_1().min_w_0().truncate().child(*label))
                    .child(
                        spork_workbench::controls::check_box(
                            SharedString::from(format!("{key}-box")),
                            Some(on),
                            cx,
                        )
                        .role(Role::CheckBox)
                        .aria_label(*label)
                        .aria_toggled(if on { Toggled::True } else { Toggled::False })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.set_value(key, if on { "false" } else { "true" });
                            cx.notify();
                        })),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        COMMIT_INSPECTION.set(ix);
                        cx.notify();
                    }))
            })
            .collect::<Vec<_>>();
        let enabled = crate::changes::COMMIT_INSPECTIONS
            .get(selected)
            .is_some_and(|(key, _)| self.flag(key, false));
        v_flex()
            .gap_3()
            .child(self.opt_check(
                "commit.clearInitialMessage",
                "Clear initial commit message",
                true,
                cx,
            ))
            .child(div().pt_3().child("Commit message inspections:"))
            .child(
                h_flex()
                    .items_start()
                    .gap_2()
                    .child(
                        v_flex()
                            .w(px(270.))
                            .h(px(105.))
                            .border_1()
                            .border_color(tokens.control_border)
                            .bg(tokens.control_bg)
                            .children(rows),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .when(!enabled, |label| label.opacity(0.5))
                                    .child("Severity:"),
                            )
                            // shortcut: Warning alone; IntelliJ's other severities once the
                            // hints under the message tell them apart
                            .child(spork_workbench::controls::combo(
                                "commit-inspection-severity",
                                "Warning",
                                !enabled,
                                |menu, _, _| menu.item(PopupMenuItem::new("Warning").checked(true)),
                            )),
                    ),
            )
    }

    /// Version Control › Changelists, as the reference app's: its Conflicts part greyed, as there (Git
    /// tracks no changelist conflicts).
    fn changelists_page(&self, cx: &mut Context<Self>) -> Div {
        let indent = px(20.);
        v_flex()
            .gap_3()
            .child(self.opt_check(
                "changelists.createAutomatically",
                "Create changelists automatically",
                false,
                cx,
            ))
            .child(self.opt_check(
                "changelists.partial",
                "Allow putting changes within one file into different changelists",
                true,
                cx,
            ))
            .child(titled("Inactive Changelist", cx))
            .child(
                v_flex()
                    .pl(indent)
                    .gap_2()
                    .child(self.opt_check(
                        "changelists.highlightInactive",
                        "Highlight files from inactive changelists",
                        false,
                        cx,
                    ))
                    .child(self.opt_check_disabled(
                        "changelists.conflictDialog",
                        "Show dialog on attempt to edit file from inactive changelist",
                        false,
                        cx,
                    ))
                    .child(labeled(
                        "When an empty changelist becomes inactive:",
                        self.opt_combo(
                            "changelists.removeEmpty",
                            &["Show options", "Do nothing", "Remove silently"],
                            "Show options",
                            240.,
                            cx,
                        )
                        .into_any_element(),
                    )),
            )
            .child(titled("Conflicts", cx).opacity(0.5))
            .child(
                v_flex()
                    .pl(indent)
                    .gap_2()
                    .child(self.opt_check_disabled(
                        "changelists.highlightConflicts",
                        "Highlight files with changelist conflicts",
                        true,
                        cx,
                    ))
                    .child(div().opacity(0.5).child("Files with ignored conflicts:"))
                    .child(table(
                        Vec::new(),
                        &[("", None)],
                        Vec::new(),
                        "No ignored files",
                        190.,
                        cx,
                    ))
                    .child(
                        h_flex().justify_end().child(
                            spork_workbench::Button::new("changelists-clear", "Clear")
                                .role(Role::Button)
                                .aria_label("Clear")
                                .disabled(true),
                        ),
                    ),
            )
    }

    /// Version Control › Confirmation, as the reference app's: its controls, choices in its order, and
    /// what it greys.
    fn confirmation_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let indent = px(20.);
        let check_conflicts = self.flag("confirmation.checkConflicts", false);
        let highlight_changed = self.flag("confirmation.highlightChanged", false);
        let limit_history = self.flag("confirmation.limitHistory", true);
        let interval = self.opt_text_enabled(
            "confirmation.conflictsInterval",
            "60",
            80.,
            check_conflicts,
            window,
            cx,
        );
        let days = self.opt_text_enabled(
            "confirmation.changedDays",
            "31",
            72.,
            highlight_changed,
            window,
            cx,
        );
        let rows = self.opt_text_enabled(
            "confirmation.historyRows",
            "1000",
            100.,
            limit_history,
            window,
            cx,
        );
        v_flex()
            .gap_3()
            .child(titled("Confirmation", cx))
            .child(
                v_flex()
                    .pl(indent)
                    .gap_2()
                    .child(inline([
                        "When files are created:".into_any_element(),
                        self.opt_combo(
                            "confirmation.whenCreated",
                            &["Ask", "Do not add", "Add silently"],
                            "Ask",
                            200.,
                            cx,
                        )
                        .into_any_element(),
                        self.opt_check_disabled(
                            "confirmation.outsideFiles",
                            if crate::ide::app_name(cx) == "Spork IDE" {
                                "Apply to files created outside Spork IDE"
                            } else {
                                "Apply to files created outside Spork"
                            },
                            false,
                            cx,
                        )
                        .into_any_element(),
                    ]))
                    .child(inline([
                        "When files are deleted:".into_any_element(),
                        self.opt_combo(
                            "confirmation.whenDeleted",
                            &["Ask", "Do not remove", "Remove silently"],
                            "Ask",
                            244.,
                            cx,
                        )
                        .into_any_element(),
                    ]))
                    .child(inline([
                        "Show options before:".into_any_element(),
                        self.opt_check("confirmation.showUpdate", "Update", true, cx)
                            .into_any_element(),
                    ]))
                    .child(self.opt_check(
                        "confirmation.dropCommits",
                        "Ask for confirmation to drop commits",
                        true,
                        cx,
                    ))
                    .child(comment("Applicable to Git", cx)),
            )
            .child(titled("Changes", cx))
            .child(
                v_flex()
                    .pl(indent)
                    .gap_2()
                    .child(inline([
                        self.opt_check(
                            "confirmation.checkConflicts",
                            "Check for conflicts with the server every",
                            false,
                            cx,
                        )
                        .into_any_element(),
                        interval.into_any_element(),
                        "minutes".into_any_element(),
                    ]))
                    .child(inline([
                        self.opt_check(
                            "confirmation.highlightChanged",
                            "Highlight files changed in the last",
                            false,
                            cx,
                        )
                        .into_any_element(),
                        days.into_any_element(),
                        "days".into_any_element(),
                    ]))
                    .child(comment(
                        "Modified files will be highlighted in the external stack trace and while debugging.",
                        cx,
                    ))
                    .child(self.opt_check(
                        "confirmation.dirtyRecursively",
                        "Highlight directories that contain modified files in the Project tree",
                        false,
                        cx,
                    )),
            )
            .child(labeled(
                "When patch is created:",
                self.opt_combo(
                    "confirmation.patchCreated",
                    &["Ask", "Do nothing", "Show in Finder"],
                    "Ask",
                    226.,
                    cx,
                )
                .into_any_element(),
            ))
            .child(self.opt_check(
                "confirmation.restoreWorkspace",
                "Restore workspace when switching branches",
                true,
                cx,
            ))
            .child(comment(
                "A workspace is a set of opened files, the current run configuration and breakpoints associated with a branch.",
                cx,
            ))
            .child(inline([
                self.opt_check("confirmation.limitHistory", "Limit history to", true, cx)
                    .into_any_element(),
                rows.into_any_element(),
                "rows".into_any_element(),
            ]))
            .child(titled("Gutter", cx))
            .child(
                v_flex()
                    .pl(indent)
                    .gap_2()
                    .child(self.opt_check(
                        "confirmation.gutterMarkers",
                        "Highlight modified lines in the gutter",
                        true,
                        cx,
                    ))
                    .child(
                        v_flex()
                            .pl(indent)
                            .gap_2()
                            .child(self.opt_check(
                                "confirmation.errorStripe",
                                "Highlight modified lines in error stripe on the scrollbar",
                                true,
                                cx,
                            ))
                            .child(self.opt_check(
                                "confirmation.whitespaceColor",
                                "Highlight lines with whitespace-only modifications with a different color",
                                true,
                                cx,
                            )),
                    ),
            )
    }

    /// The group's own page: what it is for, then a link to each of its pages.
    fn version_control_page(&self, cx: &mut Context<Self>) -> Div {
        let links = VERSION_CONTROL.iter().map(|node| {
            let target = format!("Version Control/{}", node.name);
            spork_workbench::controls::link(SharedString::from(target.clone()), node.name, cx)
                .on_click(cx.listener(move |this, _, _, cx| this.show(&target, cx)))
        });
        v_flex()
            .gap(px(7.))
            .child("Configure the settings related to version control used in your project")
            .child(v_flex().pl(px(24.)).gap(px(1.)).children(links))
    }

    fn mappings_page(&self, cx: &mut Context<Self>) -> Div {
        let secondary = Tokens::get(cx).text_secondary;
        // IntelliJ's default mapping, the project and the roots found under it: Spork's one
        let rows = self
            .project_root(cx)
            .map(|_| {
                vec![
                    h_flex()
                        .gap_1()
                        .child("<Project>")
                        .child(div().text_color(secondary).child("Detected 1"))
                        .into_any_element(),
                    "Git".into_any_element(),
                ]
            })
            .into_iter()
            .collect();
        v_flex()
            .gap_2()
            .child(table(
                vec![
                    tool("mappings", "general/add", "Add", true).into_any_element(),
                    tool("mappings", "general/remove", "Remove", false).into_any_element(),
                    tool("mappings", "general/edit", "Edit", false).into_any_element(),
                ],
                &[("Directory", None), ("VCS", Some(75.))],
                rows,
                "Nothing to show",
                492.,
                cx,
            ))
            .child(note(
                "<Project> - Content roots of all modules, all immediate descendants of project base directory, and .idea directory contents",
                cx,
            ))
            .child(
                h_flex()
                    .pt_2()
                    .gap_1()
                    .child(self.opt_check(
                        "mappings.detectAutomatically",
                        "Enable automatic mapping detection",
                        true,
                        cx,
                    ))
                    .child(context_help(
                        "mappings-help",
                        "Add and remove mapping on project structure changes. Detect newly created VCS roots.",
                        cx,
                    )),
            )
    }

    fn file_status_page(&self, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let dark = cx.theme().mode.is_dark();
        let owner = cx.entity_id();
        let (selected, oauth_open) = view_state(owner);
        let color = |status: &(&str, &str, Option<u32>, Option<u32>)| match dark {
            true => status.2,
            false => status.3,
        };
        let rows = FILE_STATUSES
            .iter()
            .enumerate()
            .map(|(ix, status)| {
                let (name, key, ..) = *status;
                let default = color(status).is_some();
                let used = self.flag(key, default);
                // The reference app marks each color its scheme sets over the base scheme: all it has
                let customized = default;
                let foreground = match color(status).filter(|_| used) {
                    Some(hex) => rgb(hex).into(),
                    None => tokens.text,
                };
                h_flex()
                    .id(SharedString::from(format!("file-status-{ix}")))
                    .role(Role::ListItem)
                    .aria_label(name)
                    .h(px(metrics::ROW_HEIGHT))
                    .cursor_pointer()
                    .when(selected == Some(ix), |row| row.bg(tokens.selection))
                    .child(
                        div()
                            .w(px(12.))
                            .flex_none()
                            .text_center()
                            .text_color(tokens.text_muted)
                            .when(customized, |marker| marker.child("*")),
                    )
                    .child(div().px_1().truncate().text_color(foreground).child(name))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        VIEW.set(Some((owner, Some(ix), oauth_open)));
                        cx.notify();
                    }))
            })
            .collect::<Vec<_>>();
        let details = selected.and_then(|ix| FILE_STATUSES.get(ix)).map(|status| {
            let (_, key, ..) = *status;
            let default = color(status).is_some();
            let used = self.flag(key, default);
            let swatch = div()
                .id("file-status-color")
                .w(px(48.))
                .h(px(20.))
                .rounded(px(metrics::RADIUS))
                .border_1()
                .border_color(tokens.control_border)
                .when_some(color(status).filter(|_| used), |swatch, hex| {
                    swatch
                        .bg(rgb(hex))
                        .cursor_pointer()
                        .on_click(not_yet("Choose Color"))
                });
            v_flex()
                .gap_2()
                .child(
                    h_flex()
                        .gap_2()
                        .child(self.opt_check(key, "File Status Color:", default, cx))
                        .child(swatch),
                )
                .child(
                    h_flex().child(
                        spork_workbench::Button::new("file-status-restore", "Restore Default")
                            .disabled(used == default)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.reset_value(key);
                                cx.notify();
                            })),
                    ),
                )
        });
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .items_start()
                    .gap_4()
                    .child(
                        v_flex()
                            .w(px(250.))
                            .h(px(534.))
                            .flex_none()
                            .border_1()
                            .border_color(tokens.control_border)
                            .bg(tokens.control_bg)
                            .children(rows),
                    )
                    .children(details),
            )
            .child(note("* Customized", cx))
    }

    fn log_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let indent = px(20.);
        let delay = self.opt_text("log.issuePreviewDelay", "500", 64., window, cx);
        // the preview's location, greyed while the preview is off
        let location = |this: &Self,
                        key: &'static str,
                        default: &'static str,
                        preview: (&'static str, bool),
                        cx: &mut Context<Self>| {
            let radios = match this.flag(preview.0, preview.1) {
                true => this.opt_radio(key, &["Bottom", "Right"], default, cx),
                false => {
                    let current = this.value(key, default);
                    v_flex()
                        .opacity(0.5)
                        .children(["Bottom", "Right"].map(|choice| {
                            spork_workbench::controls::labeled_radio(
                                SharedString::from(format!("{key}-{choice}")),
                                choice,
                                choice == current,
                                cx,
                            )
                        }))
                }
            };
            let label = div()
                .when(!this.flag(preview.0, preview.1), |label| {
                    label.text_color(Tokens::get(cx).text_disabled)
                })
                .child("Location:");
            inline([
                label.into_any_element(),
                radios.flex_row().gap_4().into_any_element(),
            ])
            .pl(px(20.))
        };
        let columns = |this: &Self, keys: &'static [&'static str; 4], cx: &mut Context<Self>| {
            v_flex().gap_2().child(titled("Visible Columns", cx)).child(
                v_flex()
                    .pl(px(20.))
                    .gap_2()
                    .child(this.opt_check(keys[0], "Author", true, cx))
                    .child(this.opt_check(keys[1], "Hash", false, cx))
                    .child(this.opt_check(keys[2], "Date", true, cx))
                    .child(this.opt_check(keys[3], "GPG Signature", false, cx)),
            )
        };
        v_flex()
            .gap_3()
            .child(titled("Location", cx))
            .child(
                h_flex()
                    .pl(indent)
                    .gap_2()
                    .child(self.opt_check(
                        "log.showInEditor",
                        "Show the log in the editor window",
                        true,
                        cx,
                    ))
                    .child(reference_app_badge(cx)),
            )
            .child(titled("View Options", cx))
            .child(
                v_flex()
                    .pl(indent)
                    .gap_2()
                    .child(self.opt_check(
                        "log.compactReferences",
                        "Show only the first reference for a commit in the table",
                        true,
                        cx,
                    ))
                    .child(self.opt_check("log.tagNames", "Show tag names in the table", false, cx))
                    .child(self.opt_check(
                        "log.commitDate",
                        "Show the time when the change was committed rather than authored",
                        false,
                        cx,
                    ))
                    .child(self.opt_check(
                        "log.referencesLeft",
                        "Show references on the left of commit message",
                        false,
                        cx,
                    ))
                    .child(self.opt_check(
                        "log.changesFromParents",
                        "Display changes to each merged commit separately",
                        false,
                        cx,
                    ))
                    .child(inline([
                        self.opt_check(
                            "log.issuePreview",
                            "Show the issue preview on hover over an issue id in",
                            true,
                            cx,
                        )
                        .into_any_element(),
                        delay.into_any_element(),
                        "ms".into_any_element(),
                    ]))
                    .child(self.opt_check(
                        "log.diffPreview",
                        "Show the diff preview panel",
                        false,
                        cx,
                    ))
                    .child(location(
                        self,
                        "log.diffPreviewLocation",
                        "Bottom",
                        ("log.diffPreview", false),
                        cx,
                    ))
                    .child(columns(
                        self,
                        &[
                            "log.column.author",
                            "log.column.hash",
                            "log.column.date",
                            "log.column.gpg",
                        ],
                        cx,
                    )),
            )
            .child(titled("Indexing", cx))
            .child(
                v_flex()
                    .pl(indent)
                    .gap_1()
                    .child(self.opt_check("log.index", "Enable indexing for project", true, cx))
                    .child(comment(
                        "To improve working with history of changes across IDE",
                        cx,
                    )),
            )
            .child(titled("File History", cx))
            .child(
                v_flex()
                    .pl(indent)
                    .gap_2()
                    .child(self.opt_check(
                        "log.history.details",
                        "Display details panel",
                        false,
                        cx,
                    ))
                    .child(self.opt_check("log.history.fileNames", "Show file names", false, cx))
                    .child(self.opt_check(
                        "log.history.diffPreview",
                        "Show the diff preview panel",
                        true,
                        cx,
                    ))
                    .child(location(
                        self,
                        "log.history.diffPreviewLocation",
                        "Right",
                        ("log.history.diffPreview", true),
                        cx,
                    ))
                    .child(columns(
                        self,
                        &[
                            "log.history.column.author",
                            "log.history.column.hash",
                            "log.history.column.date",
                            "log.history.column.gpg",
                        ],
                        cx,
                    )),
            )
    }

    fn shelf_page(&self, cx: &mut Context<Self>) -> Div {
        let location = self
            .project_root(cx)
            .map(|root| spork_git::shelf::dir(&root).display().to_string())
            .unwrap_or_else(|| "<Project>/shelf".to_owned());
        v_flex()
            .gap_3()
            .child(self.opt_check(
                "shelf.removeApplied",
                "Remove successfully applied files from the shelf",
                false,
                cx,
            ))
            .child(self.opt_check(
                "shelf.baseRevisions",
                "Shelve base revisions of files under distributed version control systems",
                true,
                cx,
            ))
            .child(comment(
                "The base content of files larger than 500K will not be stored",
                cx,
            ))
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        spork_workbench::Button::new("shelf-location", "Change Shelves Location…")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.shelf_location_dialog(window, cx)
                            })),
                    )
                    .child(note(format!("Current location is {location}"), cx)),
            )
    }

    /// Change Shelves Location…, as the reference app's: the default or a custom directory, the shelves
    /// moved there or not; Change Location applies it at once.
    fn shelf_location_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project_root(cx) else {
            return;
        };
        let custom = spork_git::shelf::custom_dir(&root);
        let shown = custom
            .clone()
            .unwrap_or_else(|| spork_git::shelf::default_dir(&root));
        let path = cx.new(|cx| gpui_kit::component::input::InputState::new(window, cx));
        path.update(cx, |state, cx| {
            state.set_value(shown.display().to_string(), window, cx)
        });
        // the custom directory picked, and whether the shelves move there
        let choice = cx.new(|_| (custom.is_some(), false));
        let settings = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let (custom, move_shelves) = *choice.read(cx);
            let pick = |custom: bool| {
                let choice = choice.clone();
                // the dialog draws from `choice`, which no view of the window renders
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    choice.update(cx, |choice, _| choice.0 = custom);
                    window.refresh();
                }
            };
            let browse = {
                let path = path.clone();
                ActionButton::new("shelf-location-browse", "general/openDisk")
                    .tooltip("Browse…")
                    .disabled(!custom)
                    .on_click(move |_, window, cx| {
                        if let Some(dir) = crate::e2e::take_picked().and_then(|mut d| d.pop()) {
                            return path.update(cx, |state, cx| {
                                state.set_value(dir.display().to_string(), window, cx)
                            });
                        }
                        let picked = cx.prompt_for_paths(PathPromptOptions {
                            files: false,
                            directories: true,
                            multiple: false,
                            prompt: Some("Select a directory to store shelves in".into()),
                        });
                        let path = path.clone();
                        window
                            .spawn(cx, async move |cx| {
                                let Ok(Ok(Some(mut dirs))) = picked.await else {
                                    return;
                                };
                                let Some(dir) = dirs.pop() else { return };
                                cx.update(|window, cx| {
                                    path.update(cx, |state, cx| {
                                        state.set_value(dir.display().to_string(), window, cx)
                                    })
                                })
                                .ok();
                            })
                            .detach();
                    })
            };
            let (root, path, settings, choice) =
                (root.clone(), path.clone(), settings.clone(), choice.clone());
            let field = spork_workbench::TextField::new(&path)
                .aria_label("Custom directory:")
                .disabled(!custom)
                .suffix(browse);
            dialog
                .title("Change Shelves Location")
                .width(px(560.))
                .description(
                    v_flex()
                        .gap_3()
                        .child("Store shelves in:")
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    spork_workbench::controls::labeled_radio(
                                        "shelf-location-default",
                                        "Default directory:",
                                        !custom,
                                        cx,
                                    )
                                    .on_click(pick(false)),
                                )
                                .child(note("<project root>/.idea/shelf", cx)),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    spork_workbench::controls::labeled_radio(
                                        "shelf-location-custom",
                                        "Custom directory:",
                                        custom,
                                        cx,
                                    )
                                    .on_click(pick(true)),
                                )
                                .child(div().flex_1().child(field)),
                        )
                        .child(
                            spork_workbench::controls::labeled_check(
                                "shelf-location-move",
                                "Move shelves to the new location",
                                move_shelves,
                                cx,
                            )
                            .on_click(move |_, window, cx| {
                                choice.update(cx, |choice, _| choice.1 = !choice.1);
                                window.refresh();
                            }),
                        ),
                )
                .confirm()
                .ok_text("Change Location")
                .on_ok(move |_, window, cx| {
                    let target =
                        custom.then(|| std::path::PathBuf::from(path.read(cx).value().trim()));
                    match spork_git::shelf::set_dir(&root, target.as_deref(), move_shelves) {
                        Ok(()) => settings.update(cx, |_, cx| cx.notify()),
                        Err(e) => window.push_notification(Notification::error(e.to_string()), cx),
                    }
                    true
                })
        });
    }

    /// IntelliJ's accounts list of the GitHub and GitLab pages: the add menu's `logins`, Remove
    /// and Set as Default, then the accounts connected on `service`'s servers. Logging in opens
    /// the project's connect dialog.
    fn accounts_table(
        &self,
        service: Service,
        logins: &'static [&'static str],
        cx: &mut Context<Self>,
    ) -> Div {
        let id = service.id();
        let this = cx.entity();
        let connect = move |this: &Entity<Self>, cx: &mut App| {
            this.update(cx, |this, cx| {
                this.in_project(Box::new(crate::shell::ConnectAccount), cx)
            })
        };
        let add = ActionButton::new(
            SharedString::from(format!("{id}-accounts-add")),
            "general/add",
        )
        .tooltip("Add")
        .popup_marker()
        .dropdown_menu({
            let this = this.clone();
            move |menu, _, _| {
                logins.iter().fold(menu, |menu, login| {
                    let this = this.clone();
                    menu.item(
                        PopupMenuItem::new(*login).on_click(move |_, _, cx| connect(&this, cx)),
                    )
                })
            }
        });
        let empty = v_flex()
            .items_center()
            .gap_1()
            .child("No accounts added")
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        spork_workbench::controls::link(
                            SharedString::from(format!("{id}-add-account")),
                            "Add account…",
                            cx,
                        )
                        .on_click(move |_, _, cx| connect(&this, cx)),
                    )
                    .child(
                        div()
                            .text_color(Tokens::get(cx).text_secondary)
                            .child("(⌘N)"),
                    ),
            );
        let secondary = Tokens::get(cx).text_secondary;
        let rows = accounts(cx.entity_id(), service)
            .into_iter()
            .map(|(host, user)| {
                vec![
                    h_flex()
                        .gap_2()
                        .child(user)
                        .when(host != service.default_host(), |row| {
                            row.child(div().text_color(secondary).child(host))
                        })
                        .into_any_element(),
                ]
            })
            .collect();
        table(
            vec![
                add.into_any_element(),
                tool(&format!("{id}-accounts"), "general/remove", "Remove", false)
                    .into_any_element(),
                tool(
                    &format!("{id}-accounts"),
                    "actions/checked",
                    "Set as Default",
                    false,
                )
                .into_any_element(),
            ],
            &[("", None)],
            rows,
            empty,
            433.,
            cx,
        )
    }

    /// Version Control ▸ Git, as the reference app's.
    pub(crate) fn git_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let indent = px(20.);
        let secondary = Tokens::get(cx).text_secondary;
        let executable = self.opt_text("git.executable", "", 420., window, cx);
        let large = self.opt_text("git.largeFileMb", "50", 64., window, cx);
        let protected = self.opt_text("git.protectedBranches", "master;main", 420., window, cx);
        let check = |this: &Self,
                     key: &'static str,
                     label: &'static str,
                     on: bool,
                     cx: &mut Context<Self>| {
            this.opt_check(key, label, on, cx).into_any_element()
        };
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .items_start()
                    .gap_2()
                    .child(div().pt_1().child("Path to Git executable:"))
                    .child(
                        v_flex()
                            .gap_2()
                            .child(executable)
                            .child(check(
                                self,
                                "git.executableForProject",
                                "Set this path only for the current project",
                                false,
                                cx,
                            )),
                    )
                    .child(self.opt_button("git-test", "Test", cx)),
            )
            .child(check(
                self,
                "git.excludeIgnored",
                "Automatically exclude ignored directories from the analysis",
                false,
                cx,
            ))
            .child(titled("Commit", cx))
            .child(
                v_flex()
                    .pl(indent)
                    .gap_2()
                    .child(check(self, "git.stagingArea", "Enable staging area", false, cx))
                    .child(comment(
                        "This will disable changelists support. Only for non-modal commit interface.",
                        cx,
                    ))
                    .child(check(
                        self,
                        "git.warnCrlf",
                        "Warn if CRLF line separators are about to be committed",
                        true,
                        cx,
                    ))
                    .child(check(
                        self,
                        "git.warnDetached",
                        "Warn when committing in detached HEAD or during rebase",
                        true,
                        cx,
                    ))
                    .child(inline([
                        check(
                            self,
                            "git.warnLargeFiles",
                            "Warn when committing files larger than",
                            true,
                            cx,
                        ),
                        large.into_any_element(),
                        "MB".into_any_element(),
                    ]))
                    .child(check(
                        self,
                        "git.warnBadNames",
                        "Warn when committing files with names that might cause issues on other systems",
                        true,
                        cx,
                    ))
                    .child(check(
                        self,
                        "git.cherryPickedSuffix",
                        "Add the 'cherry-picked from <hash>' suffix when picking commits pushed to protected branches",
                        true,
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_3()
                            .child(self.opt_button("git-gpg", "Configure GPG Key…", cx))
                            .child(
                                div()
                                    .text_color(secondary)
                                    .child("Commits signing with GPG key is not configured"),
                            ),
                    ),
            )
            .child(titled("Push", cx))
            .child(
                v_flex()
                    .pl(indent)
                    .gap_2()
                    .child(check(
                        self,
                        "git.autoUpdateRejected",
                        "Auto-update if push of the current branch was rejected",
                        false,
                        cx,
                    ))
                    .child(check(
                        self,
                        "git.pushDialog",
                        "Show Push dialog for Commit and Push",
                        true,
                        cx,
                    ))
                    .child(div().pl(indent).child(check(
                        self,
                        "git.pushDialogProtected",
                        "Show Push dialog only when committing to protected branches",
                        false,
                        cx,
                    )))
                    .child(inline([
                        "Protected branches:".into_any_element(),
                        protected.into_any_element(),
                    ]))
                    .child(div().pl(indent).child(check(
                        self,
                        "git.githubProtection",
                        "Load branch protection rules from GitHub",
                        true,
                        cx,
                    )))
                    .child(div().pl(indent).child(comment(
                        "GitHub rules are added to the local rules and synced on every fetch",
                        cx,
                    ))),
            )
            .child(titled("Update", cx))
            .child(
                v_flex()
                    .pl(indent)
                    .gap_2()
                    .child(inline([
                        "Update method:".into_any_element(),
                        self.opt_radio("git.updateMethod", &["Merge", "Rebase"], "Merge", cx)
                            .flex_row()
                            .gap_4()
                            .into_any_element(),
                    ]))
                    .child(inline([
                        "Clean working tree using:".into_any_element(),
                        self.opt_radio("git.cleanTree", &["Stash", "Shelve"], "Shelve", cx)
                            .flex_row()
                            .gap_4()
                            .into_any_element(),
                    ]))
                    .child(inline([
                        "Filter \"Update Project\" information by paths:".into_any_element(),
                        spork_workbench::controls::link("git-update-filter", "All", cx)
                            .on_click(not_yet("Filter by paths"))
                            .into_any_element(),
                    ]))
                    .child(check(
                        self,
                        "git.autoFetch",
                        "Fetch remote changes automatically",
                        true,
                        cx,
                    ))
                    // The reference app greys it: incoming changes are checked by the fetch above
                    .child(
                        spork_workbench::controls::labeled_check(
                            "git.checkIncoming",
                            "Check remote for incoming changes",
                            true,
                            cx,
                        )
                        .opacity(0.5),
                    )
                    .child(labeled(
                        "Fetch tags:",
                        self.opt_combo(
                            "git.fetchTags",
                            &["Auto", "Sync", "Always", "Never"],
                            "Auto",
                            120.,
                            cx,
                        )
                        .into_any_element(),
                    ))
                    .child(check(
                        self,
                        "git.credentialHelper",
                        "Use credential helper",
                        false,
                        cx,
                    )),
            )
            .child(titled("Stash", cx))
            .child(
                v_flex()
                    .pl(indent)
                    .gap_2()
                    .child(check(
                        self,
                        "git.combineStashShelf",
                        "Combine stashes and shelves in one tab",
                        false,
                        cx,
                    ))
                    .child("When \"Show Diff\" is called, compare stashed changes:")
                    .child(self.opt_radio(
                        "git.stashDiff",
                        &["With the local version of a file", "With the parent commit"],
                        "With the local version of a file",
                        cx,
                    )),
            )
    }

    /// Version Control ▸ GitHub, as the reference app's.
    pub(crate) fn github_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let timeout = self.opt_text("github.connectionTimeout", "5", 56., window, cx);
        v_flex()
            .gap_3()
            .child(self.accounts_table(
                Service::GitHub,
                &[
                    "Log In via GitHub…",
                    "Log in with Token…",
                    "Log In to GitHub Enterprise…",
                ],
                cx,
            ))
            .child(self.opt_check(
                "github.cloneSsh",
                "Clone git repositories using ssh",
                false,
                cx,
            ))
            .child(self.opt_check(
                "github.markViewed",
                "Automatically mark opened files as viewed",
                false,
                cx,
            ))
            .child(self.opt_check(
                "github.unreadMarkers",
                "Enable unread markers on Pull Requests in the list",
                true,
                cx,
            ))
            .child(inline([
                "Connection timeout:".into_any_element(),
                timeout.into_any_element(),
                "seconds".into_any_element(),
            ]))
    }

    fn gitlab_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let owner = cx.entity_id();
        let (selected, open) = view_state(owner);
        let clients = self.opt_text("gitlab.oauthClients", "", 420., window, cx);
        v_flex()
            .gap_3()
            .child(self.accounts_table(
                Service::GitLab,
                &[
                    "Log In to gitlab.com…",
                    "Log In to Custom Server…",
                    "Log in with Token…",
                ],
                cx,
            ))
            .child(self.opt_check(
                "gitlab.markViewed",
                "Automatically mark opened files as viewed",
                false,
                cx,
            ))
            .child(self.opt_check("gitlab.cloneSsh", "Clone using SSH", false, cx))
            .child(
                h_flex()
                    .id("gitlab-oauth")
                    .gap_1()
                    .cursor_pointer()
                    .child(
                        spork_workbench::icon(match open {
                            true => "general/chevronDown",
                            false => "general/chevronRight",
                        })
                        .size(px(16.))
                        .text_color(tokens.icon),
                    )
                    .child("Custom Server OAuth Applications")
                    .child(div().flex_1().h(px(1.)).bg(tokens.control_border))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        VIEW.set(Some((owner, selected, !open)));
                        cx.notify();
                    })),
            )
            .when(open, |page| {
                page.child(
                    h_flex()
                        .pl(px(20.))
                        .gap_1()
                        .child(clients)
                        .child(context_help(
                            "gitlab-oauth-help",
                            "Enter one instance per line using this format: https://gitlab.example.com=client_id",
                            cx,
                        )),
                )
            })
    }
}

/// Issue Navigation: the patterns that link issue ids in commit messages to their tracker.
fn issue_navigation_page(cx: &mut Context<SettingsWindow>) -> Div {
    let add = ActionButton::new("issues-add", "general/add")
        .tooltip("Add…")
        .popup_marker()
        .dropdown_menu(|menu, _, _| {
            menu.item(
                PopupMenuItem::new("Add YouTrack Pattern")
                    .on_click(not_yet("Add YouTrack Pattern")),
            )
            .item(PopupMenuItem::new("Add JIRA Pattern").on_click(not_yet("Add JIRA Pattern")))
            .item(
                PopupMenuItem::new("Add Issue Navigation Link")
                    .on_click(not_yet("Add Issue Navigation Link")),
            )
        });
    v_flex()
        .gap_3()
        .child(format!(
            "{} will search for the specified patterns in check-in comments and link them to issues in your issue tracker",
            crate::ide::app_name(cx)
        ))
        .child(table(
            vec![
                add.into_any_element(),
                tool("issues", "general/edit", "Edit", false).into_any_element(),
                tool("issues", "general/remove", "Remove", false).into_any_element(),
            ],
            &[("Issue", None), ("Link", None)],
            Vec::new(),
            "No patterns configured",
            514.,
            cx,
        ))
}
