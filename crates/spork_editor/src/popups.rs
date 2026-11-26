//! IntelliJ's popups over the editor, drawn into Zed's through [`CodeMenuLook`]: the completion
//! lookup (`LookupImpl`, `LookupCellRenderer`: 24 px rows in the editor's font, the kind's icon,
//! the typed letters in blue, the tail and the right-aligned type in grey, "Press ⏎ to insert, ⇥
//! to replace" under the list), the intentions of ⌥⏎ (`IntentionListStep`: quick fixes with the
//! red bulb), parameter info of ⌘P (`ParameterInfoComponent`: every signature in the interface
//! font, the parameter at the caret bold, the matching signature tinted) and the documentation and
//! problem popups' surfaces (`DocumentationPopupUI`, `DaemonTooltipWithActionRenderer`). The
//! lookup's ⋮ (`LookupUi`'s menu) and the documentation popup's open IntelliJ's popup menus here,
//! drawn over the editor without taking its focus.

use std::rc::Rc;

use editor::Editor;
use editor::look::{
    CodeActionRow, CodeActionRowKind, CodeMenuLook, CompletionRow, CompletionsPopup, PopoverKind,
    SignatureRow,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Anchor, AnyElement, App, Div, ElementId, EntityId, FontWeight, Global, HighlightStyle,
    InteractiveElement as _, IntoElement as _, MouseButton, ParentElement as _, Role, SharedString,
    Stateful, StatefulInteractiveElement as _, Styled as _, StyledText, WeakEntity, Window,
    anchored, deferred, div, px,
};
use lsp::CompletionItemKind;
use settings::Settings as _;
use spork_workbench::popup;
use spork_workbench::tokens::metrics;
use spork_workbench::{Tokens, Typography as _};
use theme::ActiveTheme as _;
use theme_settings::ThemeSettings;

/// Popups' corners (`PopupMenu.borderCornerRadius`).
const RADIUS: f32 = 8.;
/// The completion list's insets above and below its rows (`CompletionPopup.BodyWithAdvertiser`).
const LOOKUP_INSET_TOP: f32 = 4.;
const LOOKUP_INSET_BOTTOM: f32 = 3.;
/// A completion's selection, 4 px in from the sides; its icon 12 px in, its text 4 px after it,
/// 10 px left after the type (`LookupCellRenderer`).
const LOOKUP_SELECTION_INSET: f32 = 4.;
const LOOKUP_ICON_X: f32 = 12.;
const LOOKUP_GAP: f32 = 4.;
const LOOKUP_PADDING_RIGHT: f32 = 10.;
/// `UISettings.maxLookupWidth`, and the rows shown before the list scrolls
/// (`MAX_LOOKUP_LIST_HEIGHT`).
const LOOKUP_MAX_WIDTH: f32 = 500.;
const LOOKUP_ROWS: usize = 11;
/// The bottom row's insets in the New UI (`CompletionPopup.Advertiser.border`: 4, 12, 3, 8) and
/// its ⋮'s size (`ActionToolbar.NAVBAR_MINIMUM_BUTTON_SIZE`).
const ADVERTISER_TOP: f32 = 4.;
const ADVERTISER_LEFT: f32 = 12.;
const ADVERTISER_BOTTOM: f32 = 3.;
const ADVERTISER_RIGHT: f32 = 8.;
const MENU_BUTTON: f32 = 20.;
/// A popup menu's least width, as IntelliJ's list popups.
const MENU_MIN_WIDTH: f32 = 200.;
/// An intention list's insets above and below, its selection 8 px in from the sides.
const ACTIONS_INSET: f32 = 8.;
const ACTIONS_SELECTION_INSET: f32 = 8.;
/// Parameter info: the balloon's insets, a signature's padding, the matching signature's fill 3 px
/// in above and below, the line between two signatures 8 px in.
const SIGNATURES_INSET_X: f32 = 12.;
const SIGNATURES_INSET_Y: f32 = 6.;
const SIGNATURE_PADDING_X: f32 = 8.;
const SIGNATURE_PADDING_Y: f32 = 6.;
/// The documentation popup's least and largest width before it wraps (`DocumentationScrollPane`).
const DOCUMENTATION_MIN_WIDTH: f32 = 300.;
const DOCUMENTATION_MAX_WIDTH: f32 = 500.;

pub(crate) fn init(cx: &mut App) {
    cx.set_global(OpenMenu(None));
    cx.set_global(CodeMenuLook {
        row_height: px(metrics::ROW_HEIGHT),
        completion_rows: LOOKUP_ROWS,
        completion_row: Rc::new(completion_row),
        completions: Rc::new(completions),
        code_action_row: Rc::new(code_action_row),
        code_actions: Rc::new(code_actions),
        popover: Rc::new(popover),
        signatures: Rc::new(signatures),
        documentation: Rc::new(crate::documentation::parts),
        documentation_max_height: px(crate::documentation::MAX_HEIGHT),
        intention_preview: Rc::new(crate::intention_preview::render),
    });
}

/// The popups' menu open now, if any: one at a time, as IntelliJ's.
struct OpenMenu(Option<MenuKey>);

impl Global for OpenMenu {}

/// A popup's menu: the lookup's ⋮ (by the lookup's id), a documentation popup's ⋮ and its font
/// size (by the popup's).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuKey {
    Lookup(usize),
    Documentation(EntityId),
    FontSize(EntityId),
    /// An intention's options, by its title's hash.
    Intention(u64),
}

pub(crate) fn menu_open(key: MenuKey, cx: &App) -> bool {
    cx.try_global::<OpenMenu>()
        .is_some_and(|open| open.0 == Some(key))
}

/// Opens (or with `None` closes) a popup's menu, the editor drawing it again.
pub(crate) fn set_menu(key: Option<MenuKey>, editor: &WeakEntity<Editor>, cx: &mut App) {
    cx.set_global(OpenMenu(key));
    editor.update(cx, |_, cx| cx.notify()).ok();
}

/// An item of a popup's menu.
pub(crate) struct MenuItem {
    pub label: SharedString,
    /// The icon in front, unless `checked` puts IntelliJ's check there.
    pub icon: Option<&'static str>,
    pub checked: Option<bool>,
    pub shortcut: Option<SharedString>,
    pub enabled: bool,
    pub run: Rc<dyn Fn(&mut Window, &mut App)>,
}

impl MenuItem {
    pub(crate) fn new(
        label: impl Into<SharedString>,
        run: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            icon: None,
            checked: None,
            shortcut: None,
            enabled: true,
            run: Rc::new(run),
        }
    }
}

pub(crate) enum MenuEntry {
    Item(MenuItem),
    Separator,
}

/// IntelliJ's ⋮ (`AllIcons.Actions.More`), a 20 px button; its menu opens under it, the item
/// clicked closing it first.
pub(crate) fn more_button(
    id: impl Into<ElementId>,
    key: MenuKey,
    editor: WeakEntity<Editor>,
    entries: impl FnOnce() -> Vec<MenuEntry>,
    cx: &App,
) -> AnyElement {
    let open = menu_open(key, cx);
    let menu = open.then(|| popup_menu(entries(), &editor, cx));
    div()
        .relative()
        .flex_none()
        .child(
            icon_button(id, "general/moreVertical", "More", open, cx).on_click(move |_, _, cx| {
                cx.stop_propagation();
                set_menu((!open).then_some(key), &editor, cx)
            }),
        )
        .children(menu.map(|menu| under(menu)))
        .into_any_element()
}

/// A popup's toolbar button: its icon in a 20 px square, filled while hovered or `pressed`.
pub(crate) fn icon_button(
    id: impl Into<ElementId>,
    icon: &'static str,
    label: &'static str,
    pressed: bool,
    cx: &App,
) -> Stateful<Div> {
    let dark = !cx.theme().appearance().is_light();
    let tokens = tokens(cx);
    let hover = tokens.editor_button_hover;
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .size(px(MENU_BUTTON))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(metrics::RADIUS))
        .cursor_pointer()
        .when(pressed, |button| button.bg(tokens.editor_button_pressed))
        .when(!pressed, |button| {
            button.hover(move |button| button.bg(hover))
        })
        .child(spork_workbench::image_in(icon, dark).size(px(metrics::TOOLBAR_ICON)))
}

/// `content` under the element before it, its right edges aligned, over everything else.
pub(crate) fn under(content: AnyElement) -> AnyElement {
    div()
        .absolute()
        .top_full()
        .right_0()
        .child(
            deferred(
                anchored()
                    .anchor(Anchor::TopRight)
                    .snap_to_window_with_margin(px(8.))
                    .child(div().mt(px(2.)).child(content)),
            )
            .with_priority(2),
        )
        .into_any_element()
}

/// `content` beside the element it's in, a submenu's place, over everything else.
fn beside(content: AnyElement) -> AnyElement {
    div()
        .absolute()
        .top(px(-popup::SELECTION_INSET))
        .left_full()
        .child(
            deferred(
                anchored()
                    .anchor(Anchor::TopLeft)
                    .snap_to_window_with_margin(px(8.))
                    .child(div().ml(px(ACTIONS_SELECTION_INSET)).child(content)),
            )
            .with_priority(2),
        )
        .into_any_element()
}

/// IntelliJ's popup menu: 24 px rows, a check or the item's icon in front, the shortcut
/// right-aligned; a click elsewhere closes it.
fn popup_menu(entries: Vec<MenuEntry>, editor: &WeakEntity<Editor>, cx: &App) -> AnyElement {
    let dark = !cx.theme().appearance().is_light();
    let tokens = tokens(cx);
    let rows = entries.into_iter().enumerate().map(|(ix, entry)| {
        let MenuEntry::Item(item) = entry else {
            return popup::separator(cx).into_any_element();
        };
        let icon = match item.checked {
            Some(true) => Some("actions/checked"),
            Some(false) => None,
            None => item.icon,
        };
        let editor = editor.clone();
        let run = item.run.clone();
        popup::row(("popup-menu-item", ix), false, cx)
            .role(Role::MenuItem)
            .aria_label(item.label.clone())
            .when_some(item.checked, |row, checked| {
                row.aria_toggled(checked.into())
            })
            .child(match icon {
                Some(icon) => spork_workbench::image_in(icon, dark)
                    .size(px(metrics::TOOLBAR_ICON))
                    .into_any_element(),
                None => div()
                    .size(px(metrics::TOOLBAR_ICON))
                    .flex_none()
                    .into_any_element(),
            })
            .child(item.label)
            .children(item.shortcut.map(|keys| popup::hint(keys, false, cx)))
            .when(!item.enabled, |row| {
                row.text_color(tokens.text_disabled).cursor_default()
            })
            .when(item.enabled, |row| {
                row.on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    set_menu(None, &editor, cx);
                    run(window, cx);
                })
            })
            .into_any_element()
    });
    let editor = editor.clone();
    keep_popup_open(popup::surface(cx))
        .id("popup-menu")
        .role(Role::Menu)
        .occlude()
        .min_w(px(MENU_MIN_WIDTH))
        .py(px(popup::SELECTION_INSET))
        .whitespace_nowrap()
        .on_mouse_down_out(move |_, _, cx| set_menu(None, &editor, cx))
        .children(rows)
        .into_any_element()
}

/// The editor's own popup under a menu of it stays open: the pointer over the menu, outside the
/// popup, is kept from the editor, as the documentation popup keeps it.
pub(crate) fn keep_popup_open(menu: Div) -> Div {
    menu.on_mouse_move(|_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// An action's shortcut as a menu shows it (IntelliJ's way of writing it).
pub(crate) fn shortcut(action: &dyn gpui::Action, window: &Window) -> Option<SharedString> {
    let binding = window.highest_precedence_binding_for_action(action)?;
    Some(popup::keystrokes_text(binding.keystrokes()).into())
}

/// Writes a setting of the user's `settings.json`.
pub(crate) fn write_setting(
    update: impl 'static + Send + FnOnce(&mut settings::SettingsContent, &App),
    cx: &mut App,
) {
    settings::update_settings_file(<dyn fs::Fs>::global(cx), cx, update);
}

pub(crate) fn tokens(cx: &App) -> &'static Tokens {
    Tokens::for_appearance(!cx.theme().appearance().is_light())
}

/// A popup's surface: its fill, a 1 px border, rounded corners and a shadow.
fn surface(fill: gpui::Hsla, border: gpui::Hsla) -> Div {
    div()
        .flex()
        .flex_col()
        .bg(fill)
        .border_1()
        .border_color(border)
        .rounded(px(RADIUS))
        .shadow_lg()
        .overflow_hidden()
}

/// The expUI icon IntelliJ gives a completion of this kind; none for keywords and plain words.
fn kind_icon(kind: Option<CompletionItemKind>) -> Option<&'static str> {
    Some(match kind? {
        CompletionItemKind::METHOD | CompletionItemKind::CONSTRUCTOR => "nodes/method",
        CompletionItemKind::FUNCTION => "nodes/function",
        CompletionItemKind::FIELD | CompletionItemKind::ENUM_MEMBER => "nodes/field",
        CompletionItemKind::VARIABLE => "nodes/variable",
        CompletionItemKind::CLASS
        | CompletionItemKind::STRUCT
        | CompletionItemKind::TYPE_PARAMETER => "nodes/class",
        CompletionItemKind::INTERFACE => "nodes/interface",
        CompletionItemKind::ENUM => "nodes/enum",
        CompletionItemKind::CONSTANT | CompletionItemKind::VALUE => "nodes/constant",
        CompletionItemKind::MODULE => "nodes/module",
        CompletionItemKind::PROPERTY => "nodes/property",
        CompletionItemKind::SNIPPET => "nodes/template",
        CompletionItemKind::FILE => "fileTypes/anyType",
        CompletionItemKind::FOLDER => "nodes/folder",
        _ => return None,
    })
}

/// IntelliJ's tail and type from what follows a completion's name: `(name: &str) -> String` is
/// the tail `(name: &str)` and the type `String`, `: u32` the type `u32`; otherwise all of it is
/// the tail and the language server's detail the type.
fn tail_and_type(rest: &str, detail: Option<SharedString>) -> (String, Option<SharedString>) {
    if let Some((tail, type_text)) = rest.rsplit_once(" -> ") {
        return (tail.to_owned(), Some(type_text.trim().to_owned().into()));
    }
    if let Some(type_text) = rest.strip_prefix(": ") {
        return (String::new(), Some(type_text.trim().to_owned().into()));
    }
    let detail = detail.filter(|detail| !detail.is_empty() && !rest.contains(detail.as_ref()));
    (rest.to_owned(), detail)
}

fn completion_row(row: CompletionRow, cx: &App) -> AnyElement {
    let dark = !cx.theme().appearance().is_light();
    let tokens = tokens(cx);
    let settings = ThemeSettings::get_global(cx);
    let name_end = row.filter_range.end.min(row.text.len());
    let name = row.text.get(..name_end).unwrap_or_default().to_owned();
    let rest = row.text.get(name_end..).unwrap_or_default();
    let (tail, type_text) = tail_and_type(rest, row.detail);
    let matched = HighlightStyle {
        color: Some(tokens.lookup_match),
        ..HighlightStyle::default()
    };
    let highlights = row
        .matches
        .iter()
        .filter(|range| range.end <= name_end)
        .map(|range| (range.clone(), matched))
        .collect::<Vec<_>>();
    let keyword = row.kind == Some(CompletionItemKind::KEYWORD);
    let icon = kind_icon(row.kind);
    let label: SharedString = name.clone().into();
    div()
        .id(label.clone())
        .role(Role::ListBoxOption)
        .aria_label(label)
        .aria_selected(row.selected)
        .flex()
        .items_center()
        .h(px(metrics::ROW_HEIGHT))
        .mx(px(LOOKUP_SELECTION_INSET))
        .pl(px(LOOKUP_ICON_X - LOOKUP_SELECTION_INSET))
        .pr(px(LOOKUP_PADDING_RIGHT - LOOKUP_SELECTION_INSET))
        .gap(px(LOOKUP_GAP))
        .rounded(px(metrics::RADIUS))
        .when(row.selected, |row| row.bg(tokens.selection))
        .cursor_pointer()
        .font(settings.buffer_font.clone())
        .text_size(settings.buffer_font_size(cx))
        .text_color(cx.theme().colors().editor_foreground)
        .whitespace_nowrap()
        .child(match icon {
            Some(icon) => spork_workbench::image_in(icon, dark).into_any_element(),
            None => div()
                .size(px(metrics::TOOLBAR_ICON))
                .flex_none()
                .into_any_element(),
        })
        .child(
            div()
                .flex_none()
                .when(keyword, |name| name.font_weight(FontWeight::BOLD))
                .when(row.deprecated, |name| name.line_through())
                .child(StyledText::new(name).with_highlights(highlights)),
        )
        .when(!tail.is_empty(), |row| {
            row.child(
                div()
                    .ml(-px(LOOKUP_GAP))
                    .text_color(tokens.text_secondary)
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(tail),
            )
        })
        .when_some(type_text, |row, type_text| {
            row.child(
                div()
                    .flex_none()
                    .ml_auto()
                    .pl(px(LOOKUP_PADDING_RIGHT))
                    .text_color(tokens.text_secondary)
                    .child(type_text),
            )
        })
        .into_any_element()
}

fn completions(
    list: AnyElement,
    popup: CompletionsPopup,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let tokens = tokens(cx);
    let settings = ThemeSettings::get_global(cx);
    let (insert, replace) = match cfg!(target_os = "macos") {
        true => ("⏎", "⇥"),
        false => ("Enter", "Tab"),
    };
    let documentation_keys = shortcut(&editor::actions::Hover, window);
    let menu = more_button(
        "lookup-more",
        MenuKey::Lookup(popup.id),
        popup.editor.clone(),
        || lookup_menu(popup.auto_documentation, documentation_keys, cx),
        cx,
    );
    // the list is clipped to the rounded corners, the ⋮'s menu is not; a click on the popup
    // stays in it, the editor under it keeping its caret and the lookup
    div()
        .id("lookup")
        .occlude()
        .child(
            surface(tokens.lookup, tokens.popup_border)
                .max_w(px(LOOKUP_MAX_WIDTH))
                .child(
                    div()
                        .pt(px(LOOKUP_INSET_TOP))
                        .pb(px(LOOKUP_INSET_BOTTOM))
                        .child(list),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .pt(px(ADVERTISER_TOP))
                        .pl(px(ADVERTISER_LEFT))
                        .pb(px(ADVERTISER_BOTTOM))
                        .pr(px(ADVERTISER_RIGHT))
                        .gap(px(LOOKUP_GAP))
                        .bg(tokens.popup_advertiser)
                        .font(settings.ui_font.clone())
                        // the label font a step smaller (`Popup.Advertiser.fontSizeOffset`)
                        .text_ui_small()
                        .text_color(tokens.text_secondary)
                        .whitespace_nowrap()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .child(format!("Press {insert} to insert, {replace} to replace")),
                        )
                        .child(div().font(settings.ui_font.clone()).text_ui().child(menu)),
                ),
        )
        .into_any_element()
}

/// The lookup's ⋮ (`LookupUi`): Sort by Name, Quick Documentation while the documentation
/// doesn't show by itself, then Code Completion Settings. Quick Definition is left out: Spork
/// IDE has no such popup yet.
fn lookup_menu(
    auto_documentation: bool,
    documentation_keys: Option<SharedString>,
    cx: &App,
) -> Vec<MenuEntry> {
    let by_name = editor::EditorSettings::get_global(cx).completion_sort_alphabetically;
    let mut entries = vec![MenuEntry::Item(MenuItem {
        checked: Some(by_name),
        ..MenuItem::new("Sort by Name", move |_, cx| {
            write_setting(
                move |settings, _| settings.editor.completion_sort_alphabetically = Some(!by_name),
                cx,
            )
        })
    })];
    if !auto_documentation {
        entries.push(MenuEntry::Item(MenuItem {
            shortcut: documentation_keys,
            ..MenuItem::new("Quick Documentation", |window, cx| {
                window.dispatch_action(Box::new(editor::actions::Hover), cx)
            })
        }));
    }
    entries.push(MenuEntry::Separator);
    entries.push(MenuEntry::Item(MenuItem {
        icon: Some("general/settings"),
        ..MenuItem::new("Code Completion Settings", |window, cx| {
            if let Ok(action) = cx.build_action("settings_dialog::ConfigureCodeCompletion", None) {
                window.dispatch_action(action, cx);
            }
        })
    }));
    entries
}

/// An intention's options (`IntentionManagerSettings`' "Edit intention settings" and
/// `DisableIntentionAction`): Spork IDE keeps the titles turned off in a setting. Run
/// configurations have none.
fn intention_options(title: SharedString, editor: &WeakEntity<Editor>, cx: &App) -> AnyElement {
    let entries = vec![
        MenuEntry::Item(MenuItem::new("Edit intention settings", |window, cx| {
            if let Ok(action) = cx.build_action("settings_dialog::ConfigureIntentions", None) {
                window.dispatch_action(action, cx);
            }
        })),
        MenuEntry::Item(MenuItem::new(
            format!("Disable '{title}'"),
            move |window, cx| {
                let title = title.to_string();
                write_setting(
                    move |settings, _| {
                        let disabled = settings
                            .editor
                            .disabled_code_actions
                            .get_or_insert_default();
                        if !disabled.contains(&title) {
                            disabled.push(title);
                        }
                    },
                    cx,
                );
                window.dispatch_action(Box::new(editor::actions::Cancel), cx);
            },
        )),
    ];
    popup_menu(entries, editor, cx)
}

fn code_action_row(row: CodeActionRow, cx: &App) -> AnyElement {
    let dark = !cx.theme().appearance().is_light();
    let tokens = tokens(cx);
    let settings = ThemeSettings::get_global(cx);
    // an error's fix gets IntelliJ's red bulb, a weaker problem's the yellow one; IntelliJ's
    // plain intentions have none in the New UI (`CachedIntentions.getIcon`)
    let icon = match row.kind {
        CodeActionRowKind::QuickFix => Some("codeInsight/quickfixBulb"),
        CodeActionRowKind::InspectionFix => Some("codeInsight/intentionBulb"),
        CodeActionRowKind::Action | CodeActionRowKind::ModifyRunConfiguration => None,
        CodeActionRowKind::Run => Some("run/run"),
        CodeActionRowKind::Debug => Some("run/debug"),
        CodeActionRowKind::RunWithCoverage => Some("run/runWithCoverage"),
    };
    // the run icon's popup: IntelliJ's executor actions, with their keys
    let runs = matches!(
        row.kind,
        CodeActionRowKind::Run
            | CodeActionRowKind::Debug
            | CodeActionRowKind::RunWithCoverage
            | CodeActionRowKind::ModifyRunConfiguration
    );
    let shortcut = match row.kind {
        CodeActionRowKind::Run => Some("⌃⇧R"),
        CodeActionRowKind::Debug => Some("⌃⇧D"),
        _ => None,
    };
    let options = (!runs).then(|| {
        let key = MenuKey::Intention({
            let mut hasher = std::hash::DefaultHasher::new();
            std::hash::Hash::hash(&row.label, &mut hasher);
            std::hash::Hasher::finish(&hasher)
        });
        let open = menu_open(key, cx);
        let editor = row.editor.clone();
        div()
            .flex_none()
            .ml_auto()
            .child(
                icon_button(
                    SharedString::from(format!("{}-options", row.label)),
                    "general/chevronRight",
                    "Options",
                    open,
                    cx,
                )
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    set_menu((!open).then_some(key), &editor, cx)
                }),
            )
            .children(open.then(|| beside(intention_options(row.label.clone(), &row.editor, cx))))
    });
    div()
        .id(row.label.clone())
        .role(Role::ListBoxOption)
        .aria_label(row.label.clone())
        .aria_selected(row.selected)
        .flex()
        .items_center()
        .h(px(metrics::ROW_HEIGHT))
        .mx(px(ACTIONS_SELECTION_INSET))
        .px(px(LOOKUP_GAP))
        .gap(px(LOOKUP_GAP))
        .rounded(px(metrics::RADIUS))
        .when(row.selected, |row| row.bg(tokens.selection))
        .cursor_pointer()
        .font(settings.ui_font.clone())
        .text_ui()
        .text_color(tokens.text)
        .whitespace_nowrap()
        .child(match icon {
            Some(icon) => spork_workbench::image_in(icon, dark).into_any_element(),
            None => div()
                .size(px(metrics::TOOLBAR_ICON))
                .flex_none()
                .into_any_element(),
        })
        .child(row.label)
        .children(shortcut.map(|shortcut| {
            div()
                .ml_auto()
                .pl(px(LOOKUP_GAP * 2.))
                .text_color(tokens.text_secondary)
                .child(shortcut)
        }))
        .children(options)
        .into_any_element()
}

fn code_actions(list: AnyElement, cx: &App) -> AnyElement {
    let tokens = tokens(cx);
    surface(tokens.popup, tokens.popup_border)
        .py(px(ACTIONS_INSET))
        .child(list)
        .into_any_element()
}

fn popover(surface: Stateful<Div>, kind: PopoverKind, cx: &App) -> Stateful<Div> {
    let tokens = tokens(cx);
    let font_size = editor::EditorSettings::get_global(cx).documentation_font_size;
    let surface = surface
        .role(Role::Group)
        .aria_label(match kind {
            PopoverKind::Documentation => "Documentation",
            PopoverKind::Problem => "Problem",
        })
        .border_1()
        .border_color(tokens.popup_border)
        .rounded(px(RADIUS))
        .shadow_lg()
        .overflow_hidden();
    match kind {
        // Zed's content keeps 8 px around it: IntelliJ's 14 at the sides and 10 at the top; the
        // text at Adjust Font Size's size against IntelliJ's 13 (the footer keeps its own)
        PopoverKind::Documentation => surface
            .when(font_size != 13, |surface| {
                surface.text_size(
                    ThemeSettings::get_global(cx).buffer_font_size(cx) * (font_size as f32 / 13.),
                )
            })
            .bg(tokens.lookup)
            .min_w(px(DOCUMENTATION_MIN_WIDTH))
            .max_w(px(DOCUMENTATION_MAX_WIDTH))
            .px(px(6.))
            .pt(px(2.)),
        // Zed's content keeps 32 px on its right for its copy button
        PopoverKind::Problem => surface
            .bg(tokens.popup)
            .text_color(tokens.text)
            .pt(px(10.))
            .pb(px(8.))
            .pl(px(20.)),
    }
}

fn signatures(rows: Vec<SignatureRow>, current: usize, cx: &App) -> AnyElement {
    let tokens = tokens(cx);
    let settings = ThemeSettings::get_global(cx);
    let colors = cx.theme().colors();
    let several = rows.len() > 1;
    let bold = HighlightStyle {
        font_weight: Some(FontWeight::BOLD),
        ..HighlightStyle::default()
    };
    let matching = rows.get(current).map(|row| row.label.clone());
    surface(colors.editor_background, tokens.parameter_info_border)
        .id("parameter-info")
        .role(Role::Label)
        .when_some(matching, |info, label| info.aria_label(label))
        .px(px(SIGNATURES_INSET_X - SIGNATURE_PADDING_X))
        .py(px(SIGNATURES_INSET_Y))
        .font(settings.ui_font.clone())
        .text_ui()
        .text_color(colors.editor_foreground)
        .children(rows.into_iter().enumerate().map(|(ix, row)| {
            let matching = ix == current;
            div()
                .when(ix > 0, |row| {
                    row.child(
                        div()
                            .mx(px(SIGNATURE_PADDING_X))
                            .h(px(1.))
                            .bg(tokens.popup_border),
                    )
                })
                .child(
                    div()
                        .my(px(3.))
                        .px(px(SIGNATURE_PADDING_X))
                        .py(px(SIGNATURE_PADDING_Y - 3.))
                        .rounded(px(metrics::RADIUS))
                        .when(several && matching, |row| {
                            row.bg(tokens.parameter_info_current)
                        })
                        .when(several && !matching, |row| {
                            row.text_color(tokens.parameter_info_other)
                        })
                        .child(
                            StyledText::new(row.label)
                                .with_highlights(row.active_parameter.map(|range| (range, bold))),
                        ),
                )
        }))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::tail_and_type;

    #[test]
    fn a_label_splits_into_intellij_s_tail_and_type() {
        assert_eq!(
            tail_and_type("(name: &str) -> String", None),
            ("(name: &str)".to_owned(), Some("String".into()))
        );
        assert_eq!(
            tail_and_type(": u32", None),
            (String::new(), Some("u32".into()))
        );
        assert_eq!(
            tail_and_type("", Some("fn()".into())),
            (String::new(), Some("fn()".into()))
        );
    }
}
