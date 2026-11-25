//! IntelliJ's status bar widgets that Zed has no equivalent of (ROADMAP phase 3): the indent and
//! the read-only lock on the right (the navigation bar on the left is `spork_navigate`'s); and
//! the status bar's right-click menu, IntelliJ's `ViewStatusBarWidgetsGroup`: the widgets turned
//! on and off, then Hide 'the widget clicked'.

use std::rc::Rc;

use collections::HashMap;
use editor::Editor;
use gpui::{
    App, Context, Entity, EntityId, Global, IntoElement, ParentElement, Render, Styled,
    Subscription, TaskExt as _, Window,
};
use project::project_settings::ProjectSettings;
use settings::{EncodingDisplayOptions, Settings as _};
use ui::{ContextMenu, Tooltip, prelude::*};
use workspace::{
    HideStatusItem, ItemHandle, StatusBarMenu, StatusBarSettings, StatusItemView,
    add_hide_button_entry, item::ItemEvent,
};

/// IntelliJ's status bar widgets the right-click menu turns on and off, in IntelliJ's order (its
/// `statusBarWidgetFactory` order), each with its display name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Widget {
    Position,
    LanguageServices,
    LineSeparator,
    Encoding,
    Indentation,
    ReadOnly,
}

impl Widget {
    const ALL: [Widget; 6] = [
        Widget::Position,
        Widget::LanguageServices,
        Widget::LineSeparator,
        Widget::Encoding,
        Widget::Indentation,
        Widget::ReadOnly,
    ];

    fn name(self) -> &'static str {
        match self {
            Widget::Position => "Line:Column Number",
            Widget::LanguageServices => "Language Services",
            Widget::LineSeparator => "Line Separator",
            Widget::Encoding => "File Encoding",
            Widget::Indentation => "Indentation",
            Widget::ReadOnly => "Read-Only Attribute",
        }
    }

    /// Spork IDE's own widgets keep their state as IntelliJ's `StatusBarWidgetSettings`.
    fn hidden_key(self) -> Option<&'static str> {
        match self {
            Widget::Indentation => Some("spork_status_bar_indentation_hidden"),
            Widget::ReadOnly => Some("spork_status_bar_read_only_hidden"),
            _ => None,
        }
    }

    fn is_on(self, cx: &App) -> bool {
        let status_bar = StatusBarSettings::get_global(cx);
        match self {
            Widget::Position => status_bar.cursor_position_button,
            Widget::LanguageServices => ProjectSettings::get_global(cx).global_lsp_settings.button,
            Widget::LineSeparator => status_bar.line_endings_button,
            Widget::Encoding => {
                status_bar.active_encoding_button != EncodingDisplayOptions::Disabled
            }
            Widget::Indentation | Widget::ReadOnly => !cx
                .try_global::<HiddenWidgets>()
                .is_some_and(|hidden| hidden.0.contains(&self)),
        }
    }

    fn set(self, on: bool, cx: &mut App) {
        if let Some(key) = self.hidden_key() {
            let hidden = &mut cx.default_global::<HiddenWidgets>().0;
            hidden.retain(|widget| *widget != self);
            if !on {
                hidden.push(self);
            }
            let store = db::kvp::KeyValueStore::global(cx);
            let value = if on { "0" } else { "1" };
            cx.background_spawn(async move { store.write_kvp(key.into(), value.into()).await })
                .detach_and_log_err(cx);
            cx.refresh_windows();
            return;
        }
        let fs = <dyn fs::Fs>::global(cx);
        settings::update_settings_file(fs, cx, move |settings, _| {
            let status_bar = settings.status_bar.get_or_insert_default();
            match self {
                Widget::Position => status_bar.cursor_position_button = Some(on),
                Widget::LineSeparator => status_bar.line_endings_button = Some(on),
                Widget::Encoding => {
                    status_bar.active_encoding_button = Some(match on {
                        true => EncodingDisplayOptions::Enabled,
                        false => EncodingDisplayOptions::Disabled,
                    })
                }
                Widget::LanguageServices => {
                    settings.global_lsp_settings.get_or_insert_default().button = Some(on)
                }
                Widget::Indentation | Widget::ReadOnly => {}
            }
        });
    }
}

/// Spork IDE's own widgets turned off.
#[derive(Default)]
struct HiddenWidgets(Vec<Widget>);

impl Global for HiddenWidgets {}

/// Which status bar views show which widgets.
#[derive(Default)]
struct WidgetViews(HashMap<EntityId, Widget>);

impl Global for WidgetViews {}

/// Tells the right-click menu that `view` is `widget`, for its Hide entry.
pub fn register_widget(view: EntityId, widget: Widget, cx: &mut App) {
    cx.default_global::<WidgetViews>().0.insert(view, widget);
}

pub fn init(cx: &mut App) {
    let store = db::kvp::KeyValueStore::global(cx);
    let hidden = Widget::ALL
        .into_iter()
        .filter(|widget| {
            widget
                .hidden_key()
                .is_some_and(|key| store.read_kvp(key).ok().flatten().as_deref() == Some("1"))
        })
        .collect();
    cx.set_global(HiddenWidgets(hidden));
    cx.set_global(StatusBarMenu(Rc::new(widgets_menu)));
}

/// IntelliJ's status bar menu: each widget, checked when on, then Hide 'the clicked widget' (Zed's
/// Hide Button for an item that isn't one of IntelliJ's).
fn widgets_menu(
    menu: ContextMenu,
    view: EntityId,
    hide: Option<HideStatusItem>,
    _: &mut Window,
    cx: &mut App,
) -> ContextMenu {
    let mut menu = menu;
    for widget in Widget::ALL {
        let on = widget.is_on(cx);
        menu = menu.toggleable_entry(
            widget.name(),
            on,
            IconPosition::Start,
            None,
            move |_, cx| widget.set(!on, cx),
        );
    }
    let clicked = cx
        .try_global::<WidgetViews>()
        .and_then(|views| views.0.get(&view).copied());
    match (clicked, hide) {
        (Some(widget), _) => {
            menu.separator()
                .entry(format!("Hide '{}'", widget.name()), None, move |_, cx| {
                    widget.set(false, cx)
                })
        }
        (None, Some(hide)) => add_hide_button_entry(menu.separator(), hide),
        (None, None) => menu,
    }
}

fn shown(widget: Widget, cx: &App) -> bool {
    widget.is_on(cx)
}

/// The active item, re-rendered when it changes.
struct Watched {
    item: Box<dyn ItemHandle>,
    _subscription: Subscription,
}

fn watch<V: Render>(
    item: Option<&dyn ItemHandle>,
    window: &mut Window,
    cx: &mut Context<V>,
) -> Option<Watched> {
    let item = item?;
    let this = cx.entity().downgrade();
    let subscription = match item.downcast::<Editor>() {
        Some(editor) => cx.observe_in(&editor, window, |_, _, _, cx| cx.notify()),
        None => item.subscribe_to_item_events(
            window,
            cx,
            Box::new(move |event, _, cx| {
                if let ItemEvent::UpdateBreadcrumbs | ItemEvent::UpdateTab = event {
                    this.update(cx, |_, cx| cx.notify()).ok();
                }
            }),
        ),
    };
    Some(Watched {
        item: item.boxed_clone(),
        _subscription: subscription,
    })
}

/// The active editor's indent, "4 spaces" or "Tab"; a click opens the settings, where Zed sets it.
#[derive(Default)]
pub struct IndentIndicator {
    editor: Option<(Entity<Editor>, Subscription)>,
}

impl Render for IndentIndicator {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((editor, _)) = self
            .editor
            .as_ref()
            .filter(|_| shown(Widget::Indentation, cx))
        else {
            return div().hidden();
        };
        let settings = editor.read(cx).buffer().read(cx).language_settings(cx);
        let text = match settings.hard_tabs {
            true => "Tab".to_string(),
            false => format!("{} spaces", settings.tab_size),
        };
        div().child(
            Button::new("indent", text)
                .label_size(LabelSize::Small)
                .tab_index(0isize)
                .tooltip(Tooltip::text("Indent"))
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(zed_actions::OpenSettings), cx)
                }),
        )
    }
}

impl StatusItemView for IndentIndicator {
    fn set_active_pane_item(
        &mut self,
        item: Option<&dyn ItemHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor = item
            .and_then(|item| item.downcast::<Editor>())
            .filter(|editor| editor.read(cx).buffer().read(cx).is_singleton())
            .map(|editor| {
                let subscription = cx.observe_in(&editor, window, |_, _, _, cx| cx.notify());
                (editor, subscription)
            });
        cx.notify();
    }

    fn hide_setting(&self, _: &App) -> Option<workspace::HideStatusItem> {
        None
    }
}

/// The lock of the active file: open while it can be edited, closed while it is read-only; a click
/// toggles it, as IntelliJ's "Make file read-only".
#[derive(Default)]
pub struct ReadOnlyLock {
    active: Option<Watched>,
}

impl Render for ReadOnlyLock {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(active) = self.active.as_ref().filter(|_| shown(Widget::ReadOnly, cx)) else {
            return div().hidden();
        };
        if active.item.read_only_toggle_buffer(cx).is_none() {
            return div().hidden();
        }
        let editable = active.item.capability(cx).editable();
        let (icon, label) = match editable {
            true => (IconName::LockOff, "Make file read-only"),
            false => (IconName::Lock, "Make file writable"),
        };
        let item = active.item.boxed_clone();
        div().child(
            IconButton::new("read-only-lock", icon)
                .icon_size(IconSize::Small)
                .icon_color(Color::Muted)
                .aria_label(label)
                .tooltip(Tooltip::text(label))
                .on_click(move |_, window, cx| item.toggle_read_only(window, cx)),
        )
    }
}

impl StatusItemView for ReadOnlyLock {
    fn set_active_pane_item(
        &mut self,
        item: Option<&dyn ItemHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active = watch(item, window, cx);
        cx.notify();
    }

    fn hide_setting(&self, _: &App) -> Option<workspace::HideStatusItem> {
        None
    }
}
