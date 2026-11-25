//! IntelliJ's frame around the docks (New UI, Islands; ROADMAP phase 3): the tool-window stripes
//! down the window's left and right edges, the docks and the editor area drawn as islands on the
//! frame's color and the tool windows' header. The stripes, their buttons and the editor tabs are
//! Spork's (`spork_workbench::shell`), drawn in the Islands tokens of the IDE theme's appearance.

use std::sync::Arc;

use collections::HashSet;

use gpui::{
    AnyElement, App, ClickEvent, Context, CursorStyle, Div, ElementId, Entity, EntityId,
    Focusable as _, Render, Role, Stateful, Subscription, Toggled, WeakEntity, Window, svg,
};
use spork_workbench::Tokens;
use spork_workbench::shell::{self, StripeState};
use spork_workbench::tokens::metrics;
use ui::{ButtonLike, ContextMenu, PopoverMenu, Tooltip, prelude::*, right_click_menu};

use crate::Workspace;
use crate::dock::{Dock, DockPosition, PanelHandle};

/// An island's corner radius (`Island.arc` 20 is its diameter).
const ISLAND_RADIUS: Pixels = px(10.);
/// The frame's band around each island, half the 6 px between two (`Island.borderWidth`).
pub const ISLAND_BAND: Pixels = px(3.);
/// A window side without a stripe gets this more frame (`Islands.emptyGap`).
const EMPTY_GAP: Pixels = px(4.);
/// A tool window's header: 41 px tall, its title 12 px in, its actions 8 px from the end.
const HEADER_HEIGHT: Pixels = px(41.);

/// Spork's Islands tokens in the IDE theme's appearance, for the components the IDE shares with
/// Spork.
pub fn tokens(cx: &App) -> &'static Tokens {
    Tokens::for_appearance(!cx.theme().appearance().is_light())
}

/// What IntelliJ calls a tool window and how it shows it, by the persistent name of the panel
/// that stands for it.
struct ToolWindow {
    title: &'static str,
    /// Its 20 px icon among Spork's IntelliJ icons; a panel IntelliJ has no tool window for keeps
    /// its own icon.
    icon: Option<&'static str>,
    /// Whether the panel draws IntelliJ's header itself (the Project tool window's, Spork's).
    own_header: bool,
    /// Its place in its stripe's group, as IntelliJ's default layout has it.
    order: usize,
    /// A split tool window (IntelliJ's `isSplit`): after the line across its stripe.
    split: bool,
}

fn tool_window(name: &str) -> ToolWindow {
    let (title, icon, own_header, order, split) = match name {
        "Project Panel" => ("Project", Some("toolwindows/project"), true, 0, false),
        "Commit" => ("Commit", Some("toolwindows/commit"), true, 1, false),
        "Outline Panel" => ("Structure", Some("toolwindows/structure"), false, 2, true),
        "Bookmarks" => ("Bookmarks", Some("toolwindows/bookmarks"), true, 3, true),
        "Find" => ("Find", Some("toolwindows/find"), true, 0, false),
        "DebugPanel" => ("Debug", Some("toolwindows/debug"), true, 1, false),
        // after Debug: a tie keeps the order the panels were added in, Build's comes later
        "Build" => ("Build", Some("toolwindows/build"), true, 1, false),
        "Run" => ("Run", Some("toolwindows/run"), true, 2, false),
        "Services" => ("Services", Some("toolwindows/services"), true, 3, false),
        "TerminalPanel" => (
            "Terminal",
            Some("terminal/toolwindow/terminal"),
            true,
            4,
            false,
        ),
        "Problems" => ("Problems", Some("toolwindows/problems"), true, 5, false),
        "Git" => ("Git", Some("toolwindows/vcs"), true, 6, false),
        "Notifications" => (
            "Notifications",
            Some("toolwindows/notifications"),
            false,
            0,
            false,
        ),
        "Documentation" => (
            "Documentation",
            Some("toolwindows/documentation"),
            true,
            5,
            false,
        ),
        "TODO" => ("TODO", Some("toolwindows/todo"), true, 0, false),
        "AgentPanel" => (
            "AI Chat",
            Some("toolwindows/toolWindowAskAI"),
            false,
            1,
            false,
        ),
        "Maven" => ("Maven", Some("toolwindows/maven"), true, 2, false),
        "Gradle" => ("Gradle", Some("toolwindows/gradle"), true, 3, false),
        "Hierarchy" => ("Hierarchy", Some("toolwindows/hierarchy"), true, 4, false),
        _ => ("", None, false, usize::MAX, false),
    };
    ToolWindow {
        title,
        icon,
        own_header,
        order,
        split,
    }
}

/// The tool windows IntelliJ's default layout leaves off the stripes, which get their button once
/// they first show.
// shortcut: the stripes forget at restart which tool windows have shown, as they forget which were
// removed from them; keep both with the layout once a removed tool window's place is kept
fn off_stripe_by_default() -> HashSet<String> {
    [
        "Bookmarks",
        "Find",
        "DebugPanel",
        "Build",
        "Run",
        "Documentation",
        "TODO",
        "Hierarchy",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

/// A panel's tool-window title: IntelliJ's name for it, or the panel's own.
pub fn tool_window_title(panel: &dyn PanelHandle, window: &Window, cx: &App) -> SharedString {
    match tool_window(panel.persistent_name()).title {
        "" => panel
            .icon_tooltip(window, cx)
            .unwrap_or(panel.persistent_name())
            .into(),
        title => title.into(),
    }
}

/// A panel's IntelliJ tool-window icon (`toolwindows/project`), for the ones IntelliJ has.
pub fn tool_window_icon(panel: &dyn PanelHandle) -> Option<&'static str> {
    tool_window(panel.persistent_name()).icon
}

/// Whether the dock draws IntelliJ's header over the panel: the panels that don't draw it.
pub(crate) fn dock_draws_header(panel: &dyn PanelHandle) -> bool {
    !tool_window(panel.persistent_name()).own_header
}

/// The rounded panel a dock or the editor area is drawn in, on the frame's color. GPUI clips
/// children to a rectangle, so the corners are painted over in the frame's color afterwards, as
/// IntelliJ paints them: a frame-colored border outside the island whose inner edge is the
/// island's rounded outline.
pub fn island(content: impl IntoElement, cx: &App) -> Div {
    let colors = cx.theme().colors();
    let reach = px(8.);
    div()
        .relative()
        .size_full()
        .rounded(ISLAND_RADIUS)
        .overflow_hidden()
        .bg(colors.panel_background)
        .child(content)
        .child(
            div()
                .absolute()
                .top(-reach)
                .left(-reach)
                .right(-reach)
                .bottom(-reach)
                .rounded(ISLAND_RADIUS + reach)
                .border(reach)
                .border_color(colors.background),
        )
}

/// A tool window's header: its title (or what stands for it, the Project tool window's view
/// selector), its actions, then ⋮ with the tool window's options and Hide.
pub fn tool_window_header(
    title: AnyElement,
    actions: impl IntoIterator<Item = AnyElement>,
    panel: Arc<dyn PanelHandle>,
    label: SharedString,
    cx: &App,
) -> Stateful<Div> {
    let hide = {
        let panel = panel.clone();
        move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
            hide_tool_window(panel.as_ref(), window, cx)
        }
    };
    h_flex()
        .id("tool-window-header")
        .role(Role::Toolbar)
        .aria_label(label)
        .w_full()
        .h(HEADER_HEIGHT)
        .flex_none()
        .pl_3()
        .pr_2()
        .gap_1()
        .bg(cx.theme().colors().panel_background)
        .child(h_flex().flex_1().min_w_0().child(title))
        .children(actions)
        .child(
            PopoverMenu::new("tool-window-options")
                .trigger_with_tooltip(
                    header_button(
                        "tool-window-options-button",
                        "general/moreVertical",
                        "Options",
                        cx,
                    ),
                    Tooltip::text("Options"),
                )
                .anchor(gpui::Anchor::TopRight)
                .menu(move |window, cx| Some(tool_window_menu(panel.clone(), window, cx))),
        )
        .child(
            header_button("tool-window-hide", "general/hide", "Hide", cx)
                .tooltip(|_, cx| Tooltip::for_action("Hide", &crate::CloseActiveDock, cx))
                .on_click(hide),
        )
}

/// A tool window header's icon button: an IntelliJ icon, 16 px on a 22 px square (`ActionButton`
/// in a tool window's toolbar).
pub fn header_button(
    id: impl Into<ElementId>,
    icon: &str,
    label: &'static str,
    cx: &App,
) -> ButtonLike {
    let size = px(22.);
    ButtonLike::new(id)
        .width(size)
        .height(size.into())
        .aria_label(label)
        .child(
            h_flex().size_full().justify_center().child(
                svg()
                    .path(format!("wb/icons/{icon}.svg"))
                    .size(px(16.))
                    .flex_none()
                    .text_color(cx.theme().colors().icon),
            ),
        )
}

/// A tool window's title as the header shows it: bold, ellipsized.
pub fn tool_window_title_label(title: impl Into<SharedString>) -> AnyElement {
    Label::new(title)
        .weight(gpui::FontWeight::BOLD)
        .truncate()
        .into_any_element()
}

/// Closes the dock showing `panel`, as the header's Hide does.
fn hide_tool_window(panel: &dyn PanelHandle, window: &mut Window, cx: &mut App) {
    let Some(workspace) = Workspace::for_window(window, cx) else {
        return;
    };
    let position = panel.position(window, cx);
    let panel_id = panel.panel_id();
    workspace.update(cx, |workspace, cx| {
        let dock = workspace.dock_at_position(position);
        if dock.read(cx).visible_panel().map(|panel| panel.panel_id()) == Some(panel_id) {
            workspace.toggle_tool_window(position, panel_id, window, cx);
        }
    });
}

/// A tool window's options (⋮, or a right click on its stripe button): where its dock is.
fn tool_window_menu(
    panel: Arc<dyn PanelHandle>,
    window: &mut Window,
    cx: &mut App,
) -> gpui::Entity<ContextMenu> {
    let current = panel.position(window, cx);
    let gear_items = panel.gear_items(window, cx);
    ContextMenu::build(window, cx, move |mut menu, _, cx| {
        // the panel's own items first, as IntelliJ's additional gear actions
        let has_gear_items = !gear_items.is_empty();
        for item in gear_items {
            let action = item.action;
            let run = move |window: &mut Window, cx: &mut App| {
                window.dispatch_action(action.boxed_clone(), cx)
            };
            menu = match item.checked {
                Some(checked) => {
                    menu.toggleable_entry(item.label, checked, IconPosition::Start, None, run)
                }
                None => menu.entry(item.label, None, run),
            };
        }
        if has_gear_items {
            menu = menu.separator();
        }
        menu = menu.header("Move To");
        for (position, label) in [
            (DockPosition::Left, "Left"),
            (DockPosition::Bottom, "Bottom"),
            (DockPosition::Right, "Right"),
        ] {
            if !panel.position_is_valid(position, cx) {
                continue;
            }
            let panel = panel.clone();
            menu = menu.toggleable_entry(
                label,
                position == current,
                IconPosition::Start,
                None,
                move |window, cx| {
                    if position != current {
                        panel.set_position(position, window, cx);
                    }
                },
            );
        }
        menu.separator()
            .entry("Remove from Sidebar", None, move |window, cx| {
                if let Some(workspace) = Workspace::for_window(window, cx) {
                    workspace.update(cx, |workspace, cx| {
                        workspace.remove_from_sidebar(panel.as_ref(), window, cx)
                    });
                }
            })
    })
}

impl Workspace {
    /// Shows the tool window whose panel is `name` (its persistent name), unless it shows.
    pub fn show_tool_window_named(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let found = self.all_docks().into_iter().find_map(|dock| {
            let dock = dock.read(cx);
            let (index, panel) = dock
                .panels()
                .enumerate()
                .find(|(_, panel)| panel.persistent_name() == name)?;
            let shows = dock.is_open() && dock.active_panel_index() == Some(index);
            (!shows).then(|| (dock.position(), panel.panel_id()))
        });
        if let Some((position, panel_id)) = found {
            self.toggle_tool_window(position, panel_id, window, cx);
        }
    }

    /// Shows the tool window of `panel_id` in its dock and gives it the focus, or hides it if it
    /// shows, as a click on its stripe button does in IntelliJ.
    pub fn toggle_tool_window(
        &mut self,
        position: DockPosition,
        panel_id: EntityId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dock = self.dock_at_position(position).clone();
        let Some(index) = dock
            .read(cx)
            .panels()
            .position(|panel| panel.panel_id() == panel_id)
        else {
            return;
        };
        let other_is_zoomed = self.zoomed.is_some() && self.zoomed_position != Some(position);
        let shows = {
            let dock = dock.read(cx);
            dock.is_open() && dock.active_panel_index() == Some(index) && !other_is_zoomed
        };
        if shows {
            self.save_open_dock_positions(cx);
            let had_focus = dock.focus_handle(cx).contains_focused(window, cx);
            dock.update(cx, |dock, cx| dock.set_open(false, window, cx));
            if had_focus {
                self.active_pane
                    .update(cx, |pane, cx| window.focus(&pane.focus_handle(cx), cx));
            }
        } else {
            self.dismiss_zoomed_items_to_reveal(Some(position), window, cx);
            dock.update(cx, |dock, cx| {
                dock.activate_panel(index, window, cx);
                dock.set_open(true, window, cx);
                if let Some(panel) = dock.active_panel() {
                    panel.activation_focus_handle(cx).focus(window, cx);
                }
            });
        }
        cx.notify();
        self.serialize_workspace(window, cx);
    }

    /// The project's color glowing behind the main toolbar, Spork's (`shell::project_glow`), in
    /// the color of the project widget's badge: its worktree is the active repository's, else
    /// the first.
    pub(crate) fn project_glow(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<impl IntoElement + use<>> {
        let project = self.project.read(cx);
        let repository = project
            .active_repository(cx)
            .map(|repository| repository.read(cx).work_directory_abs_path.clone());
        let roots: Vec<_> = project
            .visible_worktrees(cx)
            .map(|worktree| worktree.read(cx).abs_path())
            .collect();
        let root = roots
            .iter()
            .find(|root| {
                repository
                    .as_ref()
                    .is_some_and(|repo| root.starts_with(repo))
            })
            .or(roots.first())?;
        let theme = cx.theme();
        Some(shell::project_glow(
            shell::project_slot(root),
            px(100.),
            window.is_window_active(),
            !theme.appearance().is_light(),
            theme.colors().background,
            cx,
        ))
    }

    /// Remove from Sidebar: hides the tool window and takes its button off its stripe; it is then
    /// in the ⋯ popup until shown again.
    fn remove_from_sidebar(
        &mut self,
        panel: &dyn PanelHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let position = panel.position(window, cx);
        let dock = self.dock_at_position(position).clone();
        if dock.read(cx).is_open()
            && dock.read(cx).visible_panel().map(|shown| shown.panel_id()) == Some(panel.panel_id())
        {
            self.toggle_tool_window(position, panel.panel_id(), window, cx);
        }
        let stripe = match position {
            DockPosition::Right => &self.stripes[1],
            _ => &self.stripes[0],
        };
        stripe.update(cx, |stripe, _| {
            stripe.off_sidebar.insert(panel.persistent_name().into())
        });
        for stripe in &self.stripes {
            stripe.update(cx, |_, cx| cx.notify());
        }
    }

    /// The tool windows off the stripes, for the ⋯ popup, by name; `left` and `right` are the
    /// stripes' (the left one's read while it draws).
    fn tool_windows_off_sidebar(
        &self,
        left: &HashSet<String>,
        right: &HashSet<String>,
        window: &Window,
        cx: &App,
    ) -> Vec<Arc<dyn PanelHandle>> {
        let mut panels: Vec<Arc<dyn PanelHandle>> = [
            DockPosition::Left,
            DockPosition::Bottom,
            DockPosition::Right,
        ]
        .into_iter()
        .flat_map(|position| {
            let off = match position {
                DockPosition::Right => right,
                _ => left,
            };
            self.dock_at_position(position)
                .read(cx)
                .panels()
                .filter(|panel| panel.icon(window, cx).is_some())
                .filter(|panel| off.contains(panel.persistent_name()))
                .cloned()
                .collect::<Vec<_>>()
        })
        .collect();
        panels.sort_by_key(|panel| tool_window_title(panel.as_ref(), window, cx));
        panels
    }
}

/// The ⋯ button at the end of the left stripe's tool windows (`MoreSquareStripeButton`), there
/// while a tool window is off the stripes: "More tool windows" lists them to show one.
fn more_button(workspace: WeakEntity<Workspace>, window: &Window, cx: &App) -> AnyElement {
    let tokens = tokens(cx);
    let color = shell::stripe_icon_color(StripeState::Hidden, window.is_window_active(), tokens);
    let trigger = StripeTrigger(
        shell::stripe_button(
            "stripe-more",
            shell::stripe_icon("general/moreHorizontal", color),
            StripeState::Hidden,
            tokens,
        )
        .aria_label("More")
        .tooltip(Tooltip::text("More tool windows")),
    );
    PopoverMenu::new("more-tool-windows")
        .trigger(trigger)
        .anchor(gpui::Anchor::TopLeft)
        .attach(gpui::Anchor::TopRight)
        .menu(move |window, cx| {
            let workspace = workspace.upgrade()?.read(cx);
            let [left, right] = workspace
                .stripes
                .clone()
                .map(|stripe| stripe.read(cx).off_sidebar.clone());
            let panels = workspace.tool_windows_off_sidebar(&left, &right, window, cx);
            Some(ContextMenu::build(window, cx, move |menu, window, cx| {
                // IntelliJ's `minPopupWidth`
                let mut menu = menu.fixed_width(px(300.).into());
                for panel in &panels {
                    let mut entry =
                        ui::ContextMenuEntry::new(tool_window_title(panel.as_ref(), window, cx))
                            .action(panel.toggle_action(window, cx));
                    if let Some(icon) = tool_window_icon(panel.as_ref()) {
                        entry = entry.custom_icon_path(format!("wb/icons/{icon}.svg"));
                    }
                    let action = panel.toggle_action(window, cx);
                    menu = menu.item(entry.handler(move |window, cx| {
                        window.dispatch_action(action.boxed_clone(), cx)
                    }));
                }
                menu
            }))
        })
        .into_any_element()
}

/// A stripe button opening a menu, for `PopoverMenu`.
#[derive(IntoElement)]
struct StripeTrigger(Stateful<Div>);

impl Clickable for StripeTrigger {
    fn on_click(self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        Self(self.0.on_click(handler))
    }

    fn cursor_style(self, _: CursorStyle) -> Self {
        self
    }
}

impl Toggleable for StripeTrigger {
    fn toggle_state(self, _: bool) -> Self {
        self
    }
}

impl RenderOnce for StripeTrigger {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.0
    }
}

/// A stripe of tool-window buttons on the window's `side` (left or right): the left one has the
/// left dock's tool windows at the top and the bottom dock's at the bottom, the right one the right
/// dock's. A view of its own, drawn after the workspace's render: a panel may read the workspace to
/// tell whether it has a button (the terminal's does).
pub struct ToolWindowStripe {
    side: DockPosition,
    workspace: WeakEntity<Workspace>,
    /// As last drawn, for resizing the docks beside it.
    width: Pixels,
    /// The tool windows of this side taken off it (Remove from Sidebar), by persistent name.
    off_sidebar: HashSet<String>,
    _subscriptions: Vec<Subscription>,
}

impl ToolWindowStripe {
    pub(crate) fn new(
        side: DockPosition,
        docks: [&Entity<Dock>; 3],
        workspace: WeakEntity<Workspace>,
        cx: &mut Context<Self>,
    ) -> Self {
        let _subscriptions = docks
            .into_iter()
            .map(|dock| cx.observe(dock, |_, _, cx| cx.notify()))
            .collect();
        Self {
            side,
            workspace,
            width: px(metrics::STRIPE_WIDTH),
            off_sidebar: off_stripe_by_default(),
            _subscriptions,
        }
    }

    pub(crate) fn width(&self) -> Pixels {
        self.width
    }

    fn buttons(
        &self,
        workspace: &Workspace,
        position: DockPosition,
        window: &Window,
        cx: &App,
    ) -> Vec<AnyElement> {
        let dock = workspace.dock_at_position(position).read(cx);
        let other_is_zoomed =
            workspace.zoomed.is_some() && workspace.zoomed_position != Some(position);
        let shown = dock
            .is_open()
            .then(|| dock.active_panel_index())
            .flatten()
            .filter(|_| !other_is_zoomed);
        let mut panels: Vec<(usize, Arc<dyn PanelHandle>)> = dock
            .panels()
            .cloned()
            .enumerate()
            // a panel without an icon is one its settings hide
            .filter(|(_, panel)| panel.icon(window, cx).is_some())
            .filter(|(index, panel)| {
                shown == Some(*index) || !self.off_sidebar.contains(panel.persistent_name())
            })
            .collect();
        panels.sort_by_key(|(index, panel)| {
            let tool_window = tool_window(panel.persistent_name());
            (tool_window.split, tool_window.order, *index)
        });
        // IntelliJ's line before the first split tool window, when one comes before it
        let line = panels
            .iter()
            .position(|(_, panel)| tool_window(panel.persistent_name()).split)
            .filter(|first| *first > 0);
        let mut buttons: Vec<AnyElement> = panels
            .into_iter()
            .map(|(index, panel)| {
                let state = match shown == Some(index) {
                    false => StripeState::Hidden,
                    true if panel.panel_focus_handle(cx).contains_focused(window, cx) => {
                        StripeState::Active
                    }
                    true => StripeState::Open,
                };
                self.button(position, panel, state, window, cx)
            })
            .collect();
        if let Some(line) = line {
            buttons.insert(line, shell::stripe_separator(tokens(cx)).into_any_element());
        }
        buttons
    }

    fn button(
        &self,
        position: DockPosition,
        panel: Arc<dyn PanelHandle>,
        state: StripeState,
        window: &Window,
        cx: &App,
    ) -> AnyElement {
        let tokens = tokens(cx);
        let name = panel.persistent_name();
        let title = tool_window_title(panel.as_ref(), window, cx);
        let color = shell::stripe_icon_color(state, window.is_window_active(), tokens);
        let icon = match tool_window(name).icon {
            Some(icon) => shell::stripe_icon(icon, color).into_any_element(),
            None => match panel.icon(window, cx) {
                Some(icon) => Icon::new(icon)
                    .color(Color::Custom(color))
                    .into_any_element(),
                None => gpui::Empty.into_any_element(),
            },
        };
        let icon = match panel.stripe_badge(window, cx) {
            Some(dot) => shell::stripe_badge(icon, dot, state, tokens).into_any_element(),
            None => icon,
        };
        let action = panel.toggle_action(window, cx);
        let panel_id = panel.panel_id();
        let tooltip = title.clone();
        let workspace = self.workspace.clone();
        let button = shell::stripe_button(ElementId::Name(name.into()), icon, state, tokens)
            .debug_selector(|| format!("STRIPE-{name}"))
            .aria_label(title)
            .aria_toggled(match state {
                StripeState::Hidden => Toggled::False,
                _ => Toggled::True,
            })
            .tooltip(move |_, cx| Tooltip::for_action(tooltip.clone(), &*action, cx))
            .on_click(move |_, window, cx| {
                workspace
                    .update(cx, |workspace, cx| {
                        workspace.toggle_tool_window(position, panel_id, window, cx)
                    })
                    .ok();
            });
        right_click_menu(ElementId::Name(format!("stripe-menu-{name}").into()))
            .trigger(move |_, _, _| button)
            .menu(move |window, cx| tool_window_menu(panel.clone(), window, cx))
            .into_any_element()
    }
}

impl Render for ToolWindowStripe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(workspace) = self.workspace.upgrade() else {
            return div().into_any_element();
        };
        let workspace = workspace.read(cx);
        // a tool window shown is back on its stripe, as IntelliJ's
        for position in [
            DockPosition::Left,
            DockPosition::Bottom,
            DockPosition::Right,
        ] {
            let dock = workspace.dock_at_position(position).read(cx);
            if let Some(panel) = dock.visible_panel().filter(|_| dock.is_open()) {
                self.off_sidebar.remove(panel.persistent_name());
            }
        }
        let more = match self.side {
            DockPosition::Left => {
                let right = workspace.stripes[1].read(cx).off_sidebar.clone();
                (!workspace
                    .tool_windows_off_sidebar(&self.off_sidebar, &right, window, cx)
                    .is_empty())
                .then(|| more_button(self.workspace.clone(), window, cx))
            }
            _ => None,
        };
        let (top, bottom) = match self.side {
            DockPosition::Left => (DockPosition::Left, Some(DockPosition::Bottom)),
            _ => (DockPosition::Right, None),
        };
        let top = self.buttons(workspace, top, window, cx);
        let bottom = bottom
            .map(|position| self.buttons(workspace, position, window, cx))
            .unwrap_or_default();
        if top.is_empty() && bottom.is_empty() && more.is_none() {
            // the islands' own band stays beside the frame's gap
            self.width = EMPTY_GAP + ISLAND_BAND;
            return div().w(self.width).h_full().into_any_element();
        }
        self.width = px(metrics::STRIPE_WIDTH);
        let label = match self.side {
            DockPosition::Left => "Left stripe",
            _ => "Right stripe",
        };
        shell::stripe(
            top.into_iter().chain(more),
            bottom,
            window.is_window_active(),
            tokens(cx),
        )
        .id(label)
        .role(Role::Toolbar)
        .aria_label(label)
        .into_any_element()
    }
}

gpui::actions!(
    workspace,
    [
        /// Stretch to Left: the active tool window's edge to the left (⌃⌥←).
        StretchToolWindowLeft,
        /// Stretch to Right: the active tool window's edge to the right (⌃⌥→).
        StretchToolWindowRight,
        /// Stretch to Top: the active tool window's edge up (⌃⌥↑).
        StretchToolWindowUp,
        /// Stretch to Bottom: the active tool window's edge down (⌃⌥↓).
        StretchToolWindowDown,
    ]
);

/// IntelliJ's Resize ▸ Stretch to Left/Right/Top/Bottom (`ResizeToolWindowAction`): the focused
/// tool window's free edge moves five letters (five lines) that way, a side tool window's
/// sideways, a bottom one's up and down.
// shortcut: the focused tool window only; IntelliJ falls back on the last active one
pub fn register_stretch_actions(workspace: &mut Workspace) {
    fn stretch(
        workspace: &mut Workspace,
        horizontal: bool,
        toward_end: bool,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let Some(dock) = workspace
            .all_docks()
            .into_iter()
            .find(|dock| dock.focus_handle(cx).contains_focused(window, cx))
            .cloned()
        else {
            return;
        };
        let position = dock.read(cx).position();
        let grows = match (position, horizontal) {
            (DockPosition::Left, true) => toward_end,
            (DockPosition::Right, true) => !toward_end,
            (DockPosition::Bottom, false) => !toward_end,
            _ => return,
        };
        let Some(size) = workspace.dock_size(&dock.read(cx), window, cx) else {
            return;
        };
        // `ide.windowSystem.hScrollChars` and `vScrollChars`: five "W" wide, five lines high
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let text_system = window.text_system();
        let letter = text_system
            .em_width(text_system.resolve_font(&style.font()), font_size)
            .unwrap_or(font_size);
        // a label's height: the font's size and IntelliJ's leading
        let step = match horizontal {
            true => letter * 5.,
            false => font_size * 1.25 * 5.,
        };
        workspace.resize_dock(
            position,
            if grows { size + step } else { size - step },
            window,
            cx,
        );
    }
    workspace.register_action(|workspace, _: &StretchToolWindowLeft, window, cx| {
        stretch(workspace, true, false, window, cx)
    });
    workspace.register_action(|workspace, _: &StretchToolWindowRight, window, cx| {
        stretch(workspace, true, true, window, cx)
    });
    workspace.register_action(|workspace, _: &StretchToolWindowUp, window, cx| {
        stretch(workspace, false, false, window, cx)
    });
    workspace.register_action(|workspace, _: &StretchToolWindowDown, window, cx| {
        stretch(workspace, false, true, window, cx)
    });
}
