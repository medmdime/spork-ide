//! IntelliJ's main toolbar in the title bar (`MainToolbarNewUI`, reference/idea/toolbars.md):
//! the project widget and Spork's VCS Group on the left; the run widget, Search Everywhere and
//! the settings gear on the right. Built from `spork_workbench`'s controls.

use std::path::Path;

use fs::Fs;
use gpui::{
    Action as _, AnyElement, App, ClickEvent, Context, CursorStyle, IntoElement,
    ParentElement as _, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window, div, px,
};
use gpui_kit::component::Selectable as _;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use settings::{BaseKeymap, Settings as _};
use spork_workbench::tokens::metrics;
use spork_workbench::{ActionButton, ActionSize, ToolbarDropdown};
use theme::{ActiveTheme as _, SystemAppearance, ThemeRegistry};
use util::paths::PathExt as _;

/// The project widget: the project's badge and name, opening the recent projects popup.
#[derive(IntoElement)]
pub(crate) struct ProjectWidget(ToolbarDropdown);

impl ProjectWidget {
    pub(crate) fn new(name: SharedString, root: Option<&Path>) -> Self {
        let slot = root.map_or(0, spork_workbench::shell::project_slot);
        let widget = ToolbarDropdown::new("project-widget")
            .child(spork_workbench::shell::project_badge(&name, slot))
            .child(
                div()
                    .max_w(px(metrics::PROJECT_WIDGET_MAX_WIDTH))
                    .truncate()
                    .child(name.clone()),
            );
        // IntelliJ's tooltip is the project's path, `~/…`
        let widget = match root {
            Some(root) => widget.tooltip(root.compact().to_string_lossy().into_owned()),
            None => widget,
        };
        Self(widget.aria_label(name))
    }
}

impl ui::Clickable for ProjectWidget {
    fn on_click(self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        Self(self.0.on_click(handler))
    }

    fn cursor_style(self, _: CursorStyle) -> Self {
        self
    }
}

impl ui::Toggleable for ProjectWidget {
    fn toggle_state(self, selected: bool) -> Self {
        Self(self.0.selected(selected))
    }
}

impl RenderOnce for ProjectWidget {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        spork_vcs::spork_look(cx).child(self.0)
    }
}

/// Search Everywhere and the settings gear, the right end of the toolbar.
pub(crate) fn search_and_settings() -> [AnyElement; 2] {
    let search = ActionButton::new("search-everywhere", "general/search")
        .size(ActionSize::MainToolbar)
        .tooltip("Search Everywhere")
        .on_click(|_, window, cx| {
            window.dispatch_action(spork_navigate::navigate::SearchEverywhere.boxed_clone(), cx)
        });
    let settings = ActionButton::new("settings-entry-point", "general/settings")
        .size(ActionSize::MainToolbar)
        .tooltip("IDE and Project Settings")
        .dropdown_menu_with_anchor(gpui::Anchor::TopRight, settings_menu);
    [search.into_any_element(), settings.into_any_element()]
}

/// IntelliJ's gear menu (`SettingsEntryPointGroup`), with the items Spork IDE has.
fn settings_menu(menu: PopupMenu, window: &mut Window, cx: &mut Context<PopupMenu>) -> PopupMenu {
    let current = cx.theme().name.clone();
    let mut themes: Vec<_> = ThemeRegistry::global(cx).list();
    themes.sort_by(|a, b| a.name.cmp(&b.name));
    let themes = PopupMenu::build(window, cx, move |menu, _, _| {
        themes.iter().fold(menu, |menu, theme| {
            let (name, appearance) = (theme.name.clone(), theme.appearance);
            menu.item(
                PopupMenuItem::new(name.clone())
                    .checked(name == current)
                    .on_click(move |_, _, cx| set_theme(name.clone(), appearance, cx)),
            )
        })
    });
    let keymap = *BaseKeymap::get_global(cx);
    menu.menu_with_icon(
        "Run Anything",
        spork_workbench::icon("general/runAnything"),
        zed_actions::Spawn::modal().boxed_clone(),
    )
    .separator()
    .menu_with_icon(
        "Settings…",
        spork_workbench::icon("general/settings"),
        zed_actions::OpenSettings.boxed_clone(),
    )
    .menu("Plugins…", zed_actions::Extensions::default().boxed_clone())
    .separator()
    .item(PopupMenuItem::submenu("Theme", themes))
    .submenu("Keymap", window, cx, move |menu, _, _| {
        BaseKeymap::OPTIONS
            .iter()
            .fold(menu, |menu, &(name, value)| {
                menu.item(
                    PopupMenuItem::new(name)
                        .checked(value == keymap)
                        .on_click(move |_, _, cx| set_base_keymap(value, cx)),
                )
            })
    })
    .submenu("View Mode", window, cx, |menu, window, _| {
        let text = match window.is_fullscreen() {
            true => "Exit Full Screen",
            false => "Enter Full Screen",
        };
        menu.item(PopupMenuItem::new(text).on_click(|_, window, _| window.toggle_fullscreen()))
    })
}

fn set_theme(name: SharedString, appearance: theme::Appearance, cx: &mut App) {
    let system = SystemAppearance::global(cx).0;
    settings::update_settings_file(<dyn Fs>::global(cx), cx, move |settings, _| {
        theme_settings::set_theme(settings, name.as_ref(), appearance, system)
    });
}

fn set_base_keymap(keymap: BaseKeymap, cx: &mut App) {
    settings::update_settings_file(<dyn Fs>::global(cx), cx, move |settings, _| {
        settings.base_keymap = Some(keymap.into())
    });
}
