//! IntelliJ's notification balloons as the New UI draws them (`NotificationsManagerImpl`,
//! `BalloonLayoutConfiguration`, `ActionCenterBalloonLayout`, `NotificationBalloonActionProvider`,
//! `BalloonImpl`): the one way Spork and Spork IDE show a notification. A [`Balloon`] draws one
//! and keeps its own time (10 s, or 5 min when sticky, waiting while hovered, then a 200 ms
//! fade); [`stack`] puts a window's balloons in its corner, at most three with a "N more
//! notifications" stub; [`BalloonLayout`] keeps the balloons of a window that has no list of its
//! own (Spork's windows, the IDE's dialogs).

use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::{Notification, NotificationType};
use gpui_kit::component::{NotificationHost, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::controls::{ActionButton, DropDownLink, link};
use crate::tokens::{Tokens, Typography as _};
use crate::{icon, image_in};

/// IntelliJ's `NotificationType`: which icon the balloon shows. An error looks like the others
/// but for its icon (`NotificationsUtil.getIcon`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BalloonType {
    #[default]
    Information,
    Warning,
    Error,
}

impl BalloonType {
    /// `AllIcons.General.BalloonInformation`, `BalloonWarning`, `BalloonError`, which the New UI
    /// maps to `expui/status/info`, `warning`, `error`.
    fn icon(self) -> &'static str {
        match self {
            Self::Information => "status/info",
            Self::Warning => "status/warning",
            Self::Error => "status/error",
        }
    }
}

/// An action link under the balloon's text. A link doesn't close the balloon by itself
/// (`setHideOnAction(false)`): one that expires it (`NotificationAction.createSimpleExpiring`)
/// closes it in `run`.
#[derive(Clone)]
pub struct BalloonAction {
    pub label: SharedString,
    pub run: Rc<dyn Fn(&mut Window, &mut App)>,
}

impl BalloonAction {
    pub fn new(
        label: impl Into<SharedString>,
        run: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            run: Rc::new(run),
        }
    }
}

/// The balloon's box, IntelliJ's at 1×.
pub mod metrics {
    /// The body's width: `BalloonLayoutConfiguration.FixedWidth` (360 and the 10 px shadow
    /// icons on each side) less the notification shadow's 5 px insets.
    pub const WIDTH: f32 = 370.;
    /// Its corners: `Notification.arc` 12, a diameter.
    pub const RADIUS: f32 = 6.;
    /// The content's padding, `Notification.borderInsets` in the New UI: top, left, bottom.
    pub const BORDER_INSETS: [f32; 3] = [4., 4., 6.];
    /// The icon's column (`iconPanelWidth`), the icon 10 px into it, and the gap after it.
    pub const ICON_COLUMN: f32 = 32.;
    pub const ICON_X: f32 = 10.;
    pub const ICON_GAP: f32 = 2.;
    /// The text's width: what is left of the body after the icon's column.
    pub const TEXT_WIDTH: f32 = WIDTH - BORDER_INSETS[1] - ICON_COLUMN - ICON_GAP;
    /// Text wider than this on one line (`MaxWidth`) wraps at [`WRAP_WIDTH`] (`MaxWidthStyle`).
    pub const MAX_LINE_WIDTH: f32 = 320.;
    pub const WRAP_WIDTH: f32 = 240.;
    /// The title stops short of the close button (`closeOffset`: 5, the 16 px icon, 9).
    pub const CLOSE_OFFSET: f32 = 30.;
    /// Between two action links (`actionGap`).
    pub const ACTION_GAP: f32 = 16.;
    /// The close button's 16 px icon from the body's top and right (`rightActionsOffset`), the
    /// ⋮ 12 px before it (`gearCloseSpace`), each lit 2 px around on hover.
    pub const BUTTON_OFFSET: f32 = 9.;
    pub const BUTTON_SPACE: f32 = 12.;
    pub const BUTTON_HOVER: f32 = 2.;
    /// Lines the text shows expanded before it scrolls.
    pub const EXPANDED_LINES: usize = 10;
    /// From the stack to its corner's edges, and between two balloons (`ActionCenterBalloonLayout`'s
    /// 10 px start offset; each balloon's 5 px shadow on either side).
    pub const STACK_INSET: f32 = 10.;
    pub const STACK_GAP: f32 = 10.;
    /// How far the "N more" stub tucks under the balloon above it.
    pub const STUB_OVERLAP: f32 = 7.;
}

/// The vertical spacing of IntelliJ's two layouts (`BalloonLayoutConfiguration.twoLines` and
/// `treeLines`): the space above and below, between the title and the content, the content and
/// the actions, the title and the actions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spacing {
    pub top: f32,
    pub title_content: f32,
    pub content_actions: f32,
    pub title_actions: f32,
    pub bottom: f32,
}

const TWO_LINES: Spacing = Spacing {
    top: 11.,
    title_content: 5.,
    content_actions: 5.,
    title_actions: 5.,
    bottom: 14.,
};

const THREE_LINES: Spacing = Spacing {
    top: 7.,
    title_content: 3.,
    content_actions: 7.,
    title_actions: 0.,
    bottom: 8.,
};

/// IntelliJ's choice of layout (`BalloonLayoutConfiguration.create`): the denser one for a title,
/// text and actions together, or text longer than a line beside a title or actions (two lines
/// alone).
pub fn spacing(title: bool, lines: usize, actions: bool) -> Spacing {
    let text = lines > 0;
    let room = if title || actions { 1 } else { 2 };
    match (title && text && actions) || lines > room {
        true => THREE_LINES,
        false => TWO_LINES,
    }
}

/// How many lines of text show until it is expanded: four, less one for a title and one for
/// actions (`NotificationsManagerImpl`'s `twoLineHeight`).
pub fn collapsed_lines(title: bool, actions: bool) -> usize {
    4 - usize::from(title) - usize::from(actions)
}

/// How long a balloon shows before it fades: 10 s, a sticky one 5 min
/// (`NotificationsManagerImpl`'s `startSmartFadeoutTimer`).
pub fn hides_after(sticky: bool) -> Duration {
    match sticky {
        true => Duration::from_secs(300),
        false => Duration::from_secs(10),
    }
}

/// How long it takes to fade once its time is up (`BalloonImpl`'s 200 ms animation cycle); the
/// close button takes it away at once.
pub const FADE_OUT: Duration = Duration::from_millis(200);

/// How many balloons a stack shows (`ActionCenterBalloonLayout`): one more pushes the oldest out,
/// counted on the stub under the next oldest.
pub const MAX_SHOWN: usize = 3;

/// The balloons pushed out of a stack, which its stub counts under the oldest it shows: the
/// count goes with that balloon, and grows as each new one pushes the oldest out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Overflow {
    pub count: usize,
}

impl Overflow {
    /// A balloon joins `shown` others: whether the oldest of them must go.
    pub fn push(&mut self, shown: usize) -> bool {
        let full = shown >= MAX_SHOWN;
        if full {
            self.count += 1;
        }
        full
    }

    /// The balloon at `index` (oldest first) closed: the stub goes with the oldest.
    pub fn closed(&mut self, index: usize) {
        if index == 0 {
            self.count = 0;
        }
    }
}

/// How many of the actions, `widths` wide in their order, show in a row `available` wide: all
/// when they fit, else as many from the left as fit with "More" (`more` wide) after them, the
/// others in its menu (`CollapseActionsDirection.KEEP_LEFTMOST`). A single action never folds.
pub fn fold_actions(widths: &[f32], available: f32, more: f32) -> usize {
    let row = |n: usize, with_more: bool| {
        let links: f32 = widths[..n].iter().sum();
        let count = n + usize::from(with_more);
        links
            + if with_more { more } else { 0. }
            + metrics::ACTION_GAP * count.saturating_sub(1) as f32
    };
    if widths.len() <= 1 || row(widths.len(), false) <= available {
        return widths.len();
    }
    (0..widths.len())
        .rev()
        .find(|&n| row(n, true) <= available)
        .unwrap_or(0)
}

/// IntelliJ's corner for balloons (`NotificationLocation`), Bottom Right by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BalloonCorner {
    TopRight,
    #[default]
    BottomRight,
    TopLeft,
    BottomLeft,
}

impl BalloonCorner {
    /// Settings' "Notification location:" choice.
    pub fn named(name: &str) -> Self {
        match name {
            "Top Right" => Self::TopRight,
            "Top Left" => Self::TopLeft,
            "Bottom Left" => Self::BottomLeft,
            _ => Self::BottomRight,
        }
    }

    fn top(self) -> bool {
        matches!(self, Self::TopRight | Self::TopLeft)
    }

    fn left(self) -> bool {
        matches!(self, Self::TopLeft | Self::BottomLeft)
    }
}

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// A balloon: its type icon (or one of its own), its title (bold beside text), its text (two
/// to four lines until expanded) or other content, its action links (folding into "More" when
/// they don't fit) and, under the pointer, its ⋮ menu and close button (⌥-click closes them
/// all). It hides after [`hides_after`], waiting while hovered, and fades: `on_close` then runs,
/// as it does for the close button.
#[derive(IntoElement)]
pub struct Balloon {
    id: ElementId,
    kind: BalloonType,
    icon: Option<AnyElement>,
    title: Option<SharedString>,
    text: Option<SharedString>,
    content: Option<AnyElement>,
    actions: Vec<BalloonAction>,
    menu: Vec<Option<BalloonAction>>,
    sticky: bool,
    closable: bool,
    tokens: Option<&'static Tokens>,
    on_close: Option<Handler>,
    on_close_all: Option<Handler>,
}

impl Balloon {
    pub fn new(id: impl Into<ElementId>, kind: BalloonType) -> Self {
        Self {
            id: id.into(),
            kind,
            icon: None,
            title: None,
            text: None,
            content: None,
            actions: Vec::new(),
            menu: Vec::new(),
            sticky: false,
            closable: true,
            tokens: None,
            on_close: None,
            on_close_all: None,
        }
    }

    /// An icon of its own in place of its type's (`Notification.setIcon`).
    pub fn icon(mut self, icon: impl IntoElement) -> Self {
        self.icon = Some(icon.into_any_element());
        self
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn text(mut self, text: impl Into<SharedString>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Content other than text (a Markdown message), after the text if it has one.
    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    pub fn actions(mut self, actions: impl IntoIterator<Item = BalloonAction>) -> Self {
        self.actions.extend(actions);
        self
    }

    /// An item of the menu its ⋮ opens; `None` a separator.
    pub fn menu_item(mut self, item: Option<BalloonAction>) -> Self {
        self.menu.push(item);
        self
    }

    /// A sticky balloon (`NotificationDisplayType.STICKY_BALLOON`) shows for 5 minutes.
    pub fn sticky(mut self, sticky: bool) -> Self {
        self.sticky = sticky;
        self
    }

    /// Without a close button, for a notification its owner closes.
    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    /// The tokens to draw with: the IDE's frame reads the appearance from its own theme,
    /// gpui-kit's otherwise.
    pub fn tokens(mut self, tokens: &'static Tokens) -> Self {
        self.tokens = Some(tokens);
        self
    }

    /// What closes it: the close button, and its time running out.
    pub fn on_close(mut self, on_close: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(on_close));
        self
    }

    /// What ⌥-clicking its close button does: closes every balloon of the stack.
    pub fn on_close_all(mut self, on_close_all: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close_all = Some(Rc::new(on_close_all));
        self
    }
}

/// A balloon's state across frames: whether it is hovered or expanded, and its time.
struct BalloonState {
    window: AnyWindowHandle,
    hovered: bool,
    expanded: bool,
    /// Its time left while not hovered.
    remaining: Duration,
    /// When its time last started running.
    started: Option<Instant>,
    /// When it started fading out.
    fading: Option<Instant>,
    on_close: Option<Handler>,
    _countdown: Option<Task<()>>,
}

impl BalloonState {
    fn new(sticky: bool, window: &Window, cx: &mut Context<Self>) -> Self {
        let mut state = Self {
            window: window.window_handle(),
            hovered: false,
            expanded: false,
            remaining: hides_after(sticky),
            started: None,
            fading: None,
            on_close: None,
            _countdown: None,
        };
        // shortcut: IntelliJ starts the time once the app is active and the user moves or
        // types, and stops it while the app is in the background; this starts it as it shows
        state.count_down(cx);
        state
    }

    fn count_down(&mut self, cx: &mut Context<Self>) {
        let remaining = self.remaining;
        self.started = Some(Instant::now());
        self._countdown = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(remaining).await;
            let faded = this.update(cx, |this, cx| {
                this.fading = Some(Instant::now());
                cx.notify();
            });
            if faded.is_err() {
                return;
            }
            cx.background_executor().timer(FADE_OUT).await;
            let Ok((window, on_close)) =
                this.read_with(cx, |this, _| (this.window, this.on_close.clone()))
            else {
                return;
            };
            if let Some(on_close) = on_close {
                // the window may have closed with the balloon in it
                cx.update_window(window, |_, window, cx| on_close(window, cx))
                    .ok();
            }
        }));
    }

    /// IntelliJ waits while the pointer is over a balloon, bringing a fading one back, and
    /// gives it the rest of its time when the pointer leaves.
    fn set_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.hovered == hovered {
            return;
        }
        self.hovered = hovered;
        match hovered {
            true => {
                self._countdown = None;
                self.fading = None;
                if let Some(started) = self.started.take() {
                    self.remaining = self.remaining.saturating_sub(started.elapsed());
                }
            }
            false => self.count_down(cx),
        }
        cx.notify();
    }

    fn opacity(&self) -> f32 {
        self.fading.map_or(1., |since| {
            1. - (since.elapsed().as_secs_f32() / FADE_OUT.as_secs_f32()).min(1.)
        })
    }
}

/// A 16 px icon button of the balloon's corner, lit 2 px around it on hover
/// (`NotificationBalloonActionProvider.paintHover`).
fn corner_button(id: &'static str, name: &str, tooltip: &'static str) -> ActionButton {
    let side = px(16. + 2. * metrics::BUTTON_HOVER);
    ActionButton::new(id, name.to_owned())
        .tooltip(tooltip)
        .w(side)
        .h(side)
}

/// The width `text` takes on one line in `window`'s text style.
fn text_width(text: &str, window: &Window) -> f32 {
    let style = window.text_style();
    let font_size = style.font_size.to_pixels(window.rem_size());
    let run = style.to_run(text.len());
    text.lines()
        .map(|line| {
            let run = TextRun {
                len: line.len(),
                ..run.clone()
            };
            f32::from(
                window
                    .text_system()
                    .shape_line(line.to_owned().into(), font_size, &[run], None)
                    .width,
            )
        })
        .fold(0., f32::max)
}

/// How many lines `text` takes wrapped at `width`.
fn line_count(text: &SharedString, width: Option<f32>, window: &Window) -> usize {
    let style = window.text_style();
    let font_size = style.font_size.to_pixels(window.rem_size());
    let run = style.to_run(text.len());
    window
        .text_system()
        .shape_text(text.clone(), font_size, &[run], width.map(px), None)
        .map_or(1, |lines| {
            lines
                .iter()
                .map(|line| line.wrap_boundaries().len() + 1)
                .sum()
        })
}

fn menu_with(menu: PopupMenu, items: &[Option<BalloonAction>]) -> PopupMenu {
    items.iter().fold(menu, |menu, item| match item {
        Some(item) => {
            let run = item.run.clone();
            menu.item(
                PopupMenuItem::new(item.label.clone())
                    .on_click(move |_, window, cx| run(window, cx)),
            )
        }
        None => menu.separator(),
    })
}

impl RenderOnce for Balloon {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = self.tokens.unwrap_or_else(|| Tokens::get(cx));
        let sticky = self.sticky;
        let state = window.use_keyed_state(self.id.clone(), cx, |window, cx| {
            BalloonState::new(sticky, window, cx)
        });
        state.update(cx, |state, _| state.on_close = self.on_close.clone());
        let (hovered, expanded, opacity) = {
            let state = state.read(cx);
            (state.hovered, state.expanded, state.opacity())
        };
        if opacity < 1. {
            window.request_animation_frame();
        }
        let has_title = self.title.is_some();
        let has_actions = !self.actions.is_empty();
        let wrap = self.text.as_ref().and_then(|text| {
            (text_width(text, window) > metrics::MAX_LINE_WIDTH).then_some(metrics::WRAP_WIDTH)
        });
        let lines = self
            .text
            .as_ref()
            .map_or(0, |text| line_count(text, wrap, window));
        let shown_lines = collapsed_lines(has_title, has_actions);
        let collapses = lines > shown_lines;
        let spacing = spacing(
            has_title,
            lines.max(usize::from(self.content.is_some())),
            has_actions,
        );
        let more_width = text_width("More", window) + 17.;
        let widths: Vec<f32> = self
            .actions
            .iter()
            .map(|action| text_width(&action.label, window))
            .collect();
        let shown_actions = fold_actions(&widths, metrics::TEXT_WIDTH, more_width);
        let (row, folded) = self.actions.split_at(shown_actions);
        let line_height = window.line_height();
        let toggle = {
            let state = state.clone();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                state.update(cx, |state, cx| {
                    state.expanded = !state.expanded;
                    cx.notify();
                })
            }
        };
        let text = self.text.clone().map(|text| {
            let content = div()
                .when_some(wrap, |text, width| text.max_w(px(width)))
                .child(text);
            match (collapses, expanded) {
                (false, _) => content.into_any_element(),
                // clipped after its last line, a chevron at its end; a click opens it
                (true, false) => div()
                    .id("balloon-text")
                    .relative()
                    .cursor_pointer()
                    .on_click(toggle.clone())
                    .child(content.line_clamp(shown_lines).text_ellipsis())
                    .child(
                        div()
                            .id("balloon-expand")
                            .absolute()
                            .right(px(metrics::BUTTON_OFFSET))
                            .bottom_0()
                            .h(line_height)
                            .flex()
                            .items_center()
                            .role(Role::Button)
                            .aria_label("Expand")
                            .child(
                                icon("general/chevronDown")
                                    .size(px(16.))
                                    .text_color(tokens.icon),
                            ),
                    )
                    .into_any_element(),
                (true, true) => v_flex()
                    .child(
                        div()
                            .id("balloon-text")
                            .max_h(line_height * metrics::EXPANDED_LINES as f32)
                            .overflow_y_scroll()
                            .child(content),
                    )
                    .child(
                        h_flex().justify_end().pr(px(metrics::BUTTON_OFFSET)).child(
                            div()
                                .id("balloon-collapse")
                                .role(Role::Button)
                                .aria_label("Collapse")
                                .cursor_pointer()
                                .on_click(toggle.clone())
                                .child(
                                    icon("general/chevronUp")
                                        .size(px(16.))
                                        .text_color(tokens.icon),
                                ),
                        ),
                    )
                    .into_any_element(),
            }
        });
        let has_text = text.is_some() || self.content.is_some();
        let links = row.iter().enumerate().map(|(ix, action)| {
            let run = action.run.clone();
            link(("balloon-action", ix), action.label.clone(), cx)
                .on_click(move |_, window, cx| run(window, cx))
        });
        // gpui-kit's menus need its theme, which Zed's crates' tests don't set up
        let menus = cx.has_global::<gpui_kit::component::Theme>();
        let more = (!folded.is_empty()).then(|| {
            let folded: Vec<Option<BalloonAction>> = folded.iter().cloned().map(Some).collect();
            let link = DropDownLink::new("balloon-more", "More");
            match menus {
                true => link
                    .dropdown_menu(move |menu, _, _| menu_with(menu, &folded))
                    .into_any_element(),
                false => link.into_any_element(),
            }
        });
        let body = v_flex()
            .flex_1()
            .min_w_0()
            .when_some(self.title.clone(), |body, title| {
                body.child(
                    div()
                        .pr(px(metrics::CLOSE_OFFSET))
                        .truncate()
                        .when(has_text, |title| title.font_weight(FontWeight::BOLD))
                        .child(title),
                )
            })
            .when(has_title && has_text, |body| {
                body.child(div().h(px(spacing.title_content)))
            })
            .children(text)
            .children(self.content)
            .when(has_actions, |body| {
                let gap = match has_text {
                    true => spacing.content_actions,
                    false if has_title => spacing.title_actions,
                    false => 0.,
                };
                body.child(
                    h_flex()
                        .mt(px(gap))
                        .gap(px(metrics::ACTION_GAP))
                        .children(links)
                        .children(more),
                )
            });
        let icon = match self.icon {
            Some(icon) => icon,
            None => image_in(self.kind.icon(), tokens.is_dark())
                .size(px(16.))
                .into_any_element(),
        };
        let label = self
            .title
            .clone()
            .or_else(|| self.text.clone())
            .unwrap_or_default();
        let hover_state = state.clone();
        let buttons = hovered.then(|| {
            let menu = self.menu.clone();
            let (close, close_all) = (self.on_close.clone(), self.on_close_all.clone());
            let corner = metrics::BUTTON_OFFSET - metrics::BUTTON_HOVER;
            h_flex()
                .absolute()
                .top(px(corner))
                .right(px(corner))
                .gap(px(metrics::BUTTON_SPACE - 2. * metrics::BUTTON_HOVER))
                .when(!menu.is_empty() && menus, |buttons| {
                    buttons.child(
                        corner_button(
                            "balloon-menu",
                            "general/moreVertical",
                            "Turn off or change behavior",
                        )
                        .dropdown_menu(move |popup, _, _| menu_with(popup, &menu)),
                    )
                })
                .when(self.closable, |buttons| {
                    buttons.child(
                        corner_button(
                            "balloon-close",
                            "general/close",
                            "Close. ⌥click to close all notifications",
                        )
                        .on_click(move |event, window, cx| {
                            let handler = match event.modifiers().alt {
                                true => close_all.clone().or_else(|| close.clone()),
                                false => close.clone(),
                            };
                            if let Some(handler) = handler {
                                handler(window, cx);
                            }
                        }),
                    )
                })
        });
        div()
            .id(self.id)
            .role(Role::Group)
            .aria_label(label)
            .occlude()
            .w(px(metrics::WIDTH))
            .flex_none()
            .relative()
            .opacity(opacity)
            .rounded(px(metrics::RADIUS))
            .border_1()
            .border_color(tokens.feedback_border)
            .bg(tokens.feedback)
            .shadow(vec![BoxShadow {
                color: tokens.balloon_shadow,
                offset: point(px(0.), px(0.)),
                blur_radius: px(5.),
                spread_radius: px(0.),
                inset: false,
            }])
            .pt(px(metrics::BORDER_INSETS[0] + spacing.top))
            .pl(px(metrics::BORDER_INSETS[1]))
            .pb(px(metrics::BORDER_INSETS[2] + spacing.bottom))
            .text_ui()
            .line_height(relative(16. / 13.))
            .text_color(tokens.text)
            .on_hover(move |hovered, _, cx| {
                hover_state.update(cx, |state, cx| state.set_hovered(*hovered, cx))
            })
            .child(
                h_flex()
                    .items_start()
                    .gap(px(metrics::ICON_GAP))
                    .child(
                        div()
                            .w(px(metrics::ICON_COLUMN))
                            .flex_none()
                            .pl(px(metrics::ICON_X))
                            .child(icon),
                    )
                    .child(body),
            )
            .children(buttons)
    }
}

/// The stub under the oldest balloon that counts those pushed out ("2 more notifications");
/// a click shows them all in the Notifications tool window.
fn stub(count: usize, corner: BalloonCorner, tokens: &Tokens, on_click: Handler) -> Stateful<Div> {
    let text: SharedString = match count {
        1 => "1 more notification".into(),
        count => format!("{count} more notifications").into(),
    };
    let (top, bottom) = match corner.top() {
        true => (4., 10.),
        false => (10., 4.),
    };
    div()
        .id("balloon-stub")
        .role(Role::Link)
        .aria_label(text.clone())
        .w(px(metrics::WIDTH))
        .pt(px(top))
        .pb(px(bottom))
        .flex()
        .justify_center()
        .rounded(px(metrics::RADIUS))
        .border_1()
        .border_color(tokens.feedback_border)
        .bg(tokens.balloon_more)
        .text_ui_small()
        .text_color(tokens.text_muted)
        .cursor_pointer()
        .on_click(move |_, window, cx| on_click(window, cx))
        .child(text)
}

/// A window's balloons in their corner, `balloons` oldest first, 10 px from the edges of the
/// area it covers (between the stripes, above the status bar): the newest farthest from the
/// corner, the stub under the oldest when `overflow` counts balloons pushed out, `show_all`
/// what clicking it does.
pub fn stack(
    corner: BalloonCorner,
    balloons: Vec<AnyElement>,
    overflow: Overflow,
    show_all: impl Fn(&mut Window, &mut App) + 'static,
    tokens: &Tokens,
) -> Div {
    let mut balloons = balloons.into_iter();
    let oldest = balloons.next().map(|oldest| {
        let stub = (overflow.count > 0).then(|| {
            stub(overflow.count, corner, tokens, Rc::new(show_all)).map(|stub| match corner.top() {
                true => stub.mb(px(-metrics::STUB_OVERLAP)),
                false => stub.mt(px(-metrics::STUB_OVERLAP)),
            })
        });
        // the stub first, so the balloon draws over the part it tucks under
        v_flex()
            .when(!corner.top(), |group| group.flex_col_reverse())
            .children(stub)
            .child(oldest)
            .into_any_element()
    });
    v_flex()
        .absolute()
        .when(!corner.top(), |stack| stack.flex_col_reverse())
        .map(|stack| match corner.left() {
            true => stack.left(px(metrics::STACK_INSET)),
            false => stack.right(px(metrics::STACK_INSET)),
        })
        .map(|stack| match corner.top() {
            true => stack.top(px(metrics::STACK_INSET)),
            false => stack.bottom(px(metrics::STACK_INSET)),
        })
        .gap(px(metrics::STACK_GAP))
        .children(oldest)
        .children(balloons)
}

/// A balloon as a list keeps it: what a [`Balloon`] shows.
#[derive(Clone)]
pub struct BalloonSpec {
    pub kind: BalloonType,
    pub title: Option<SharedString>,
    pub text: SharedString,
    pub actions: Vec<BalloonAction>,
    /// Its ⋮ menu; `None` a separator.
    pub menu: Vec<Option<BalloonAction>>,
    pub sticky: bool,
}

impl BalloonSpec {
    /// What a gpui-kit notification says, as a balloon that isn't sticky.
    pub fn from_notification(note: &Notification) -> Option<Self> {
        let (kind, title, message) = note.summary();
        let text = message.or_else(|| title.clone())?;
        let kind = match kind {
            NotificationType::Error => BalloonType::Error,
            NotificationType::Warning => BalloonType::Warning,
            NotificationType::Info | NotificationType::Success => BalloonType::Information,
        };
        Some(Self {
            kind,
            title: title.filter(|title| *title != text),
            text,
            actions: Vec::new(),
            menu: Vec::new(),
            sticky: false,
        })
    }
}

/// The balloons of a window with no list of its own, drawn over it by gpui-kit's layers in a
/// project window's frame: below the 40 px toolbar, above the status bar, between the stripes.
pub struct BalloonLayout {
    balloons: Vec<(usize, BalloonSpec)>,
    next: usize,
    overflow: Overflow,
    corner: BalloonCorner,
    show_all: Option<Handler>,
}

impl BalloonLayout {
    fn new() -> Self {
        Self {
            balloons: Vec::new(),
            next: 0,
            overflow: Overflow::default(),
            corner: BalloonCorner::default(),
            show_all: None,
        }
    }

    pub fn push(&mut self, spec: BalloonSpec, cx: &mut Context<Self>) {
        if self.overflow.push(self.balloons.len()) {
            self.balloons.remove(0);
        }
        self.balloons.push((self.next, spec));
        self.next += 1;
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.balloons.clear();
        self.overflow = Overflow::default();
        cx.notify();
    }

    /// The corner the balloons show in (Settings' "Notification location:").
    pub fn set_corner(&mut self, corner: BalloonCorner, cx: &mut Context<Self>) {
        self.corner = corner;
        cx.notify();
    }

    /// What clicking the "N more" stub does besides closing them all: shows the Notifications
    /// tool window.
    pub fn on_show_all(&mut self, show_all: impl Fn(&mut Window, &mut App) + 'static) {
        self.show_all = Some(Rc::new(show_all));
    }

    /// For the end-to-end tests: each balloon shown, oldest first, and the stub's count.
    pub fn e2e_state(&self) -> serde_json::Value {
        let balloons: Vec<serde_json::Value> = self
            .balloons
            .iter()
            .map(|(_, spec)| {
                serde_json::json!({
                    "title": spec.title,
                    "text": spec.text,
                    "type": format!("{:?}", spec.kind),
                    "sticky": spec.sticky,
                    "actions": spec.actions.iter().map(|action| action.label.clone()).collect::<Vec<_>>(),
                })
            })
            .collect();
        serde_json::json!({"balloons": balloons, "more": self.overflow.count})
    }

    fn close(&mut self, id: usize, cx: &mut Context<Self>) {
        if let Some(index) = self.balloons.iter().position(|(other, _)| *other == id) {
            self.balloons.remove(index);
            self.overflow.closed(index);
            cx.notify();
        }
    }
}

impl Render for BalloonLayout {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        let tokens = Tokens::get(cx);
        let close_all = {
            let this = this.clone();
            move |_: &mut Window, cx: &mut App| {
                this.update(cx, |this, cx| this.clear(cx)).ok();
            }
        };
        let balloons = self
            .balloons
            .iter()
            .map(|(id, spec)| {
                let id = *id;
                let close = {
                    let this = this.clone();
                    move |_: &mut Window, cx: &mut App| {
                        this.update(cx, |this, cx| this.close(id, cx)).ok();
                    }
                };
                // Spork's actions expire their notification
                let expiring = |action: &BalloonAction| {
                    let (run, close) = (action.run.clone(), close.clone());
                    BalloonAction::new(action.label.clone(), move |window, cx| {
                        close(window, cx);
                        run(window, cx);
                    })
                };
                spec.menu
                    .iter()
                    .fold(
                        Balloon::new(("balloon", id), spec.kind)
                            .when_some(spec.title.clone(), Balloon::title)
                            .text(spec.text.clone())
                            .actions(spec.actions.iter().map(expiring))
                            .sticky(spec.sticky)
                            .on_close(close.clone())
                            .on_close_all(close_all.clone()),
                        |balloon, item| balloon.menu_item(item.as_ref().map(expiring)),
                    )
                    .into_any_element()
            })
            .collect();
        let show_all = self.show_all.clone();
        let stack = stack(
            self.corner,
            balloons,
            self.overflow,
            move |window, cx| {
                close_all(window, cx);
                if let Some(show_all) = &show_all {
                    show_all(window, cx);
                }
            },
            tokens,
        );
        div()
            .absolute()
            .inset_0()
            .top(px(40.))
            .bottom(rems(1.875))
            .left(px(crate::tokens::metrics::STRIPE_WIDTH))
            .right(px(crate::tokens::metrics::STRIPE_WIDTH))
            .child(stack)
    }
}

/// The windows' balloon layouts, each made as a balloon first shows in its window.
#[derive(Default)]
struct Layouts(HashMap<WindowId, Entity<BalloonLayout>>);

impl Global for Layouts {}

/// `window`'s balloon layout, made if it has none yet.
pub fn layout_of(window: &Window, cx: &mut App) -> Entity<BalloonLayout> {
    let id = window.window_handle().window_id();
    if let Some(layout) = layout(window, cx) {
        return layout;
    }
    let layout = cx.new(|_| BalloonLayout::new());
    cx.default_global::<Layouts>().0.insert(id, layout.clone());
    layout
}

/// `window`'s balloon layout, if a balloon showed in it.
pub fn layout(window: &Window, cx: &App) -> Option<Entity<BalloonLayout>> {
    let id = window.window_handle().window_id();
    cx.try_global::<Layouts>()?.0.get(&id).cloned()
}

/// Draws `window`'s balloons, for gpui-kit's [`NotificationHost`].
pub fn layer(window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    Some(layout(window, cx)?.into_any_element())
}

/// Shows each gpui-kit notification pushed to a window as a balloon in the window's layout, as
/// `push` makes it (`None` leaves it out).
pub fn host_notifications(
    push: impl Fn(Notification, &mut Window, &mut App) -> Option<BalloonSpec> + 'static,
    cx: &mut App,
) {
    cx.on_window_closed(|cx, closed| {
        if cx.has_global::<Layouts>() {
            cx.global_mut::<Layouts>().0.remove(&closed);
        }
    })
    .detach();
    cx.set_global(NotificationHost {
        push: Rc::new(move |note, window, cx| {
            if let Some(spec) = push(note, window, cx) {
                layout_of(window, cx).update(cx, |layout, cx| layout.push(spec, cx));
            }
        }),
        layer: Rc::new(layer),
    });
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_SHOWN, Overflow, THREE_LINES, TWO_LINES, collapsed_lines, fold_actions, hides_after,
        spacing,
    };

    #[test]
    fn sticky_balloons_stay_five_minutes() {
        assert_eq!(hides_after(false).as_secs(), 10);
        assert_eq!(hides_after(true).as_secs(), 300);
    }

    #[test]
    fn three_show_then_the_oldest_goes_onto_the_stub() {
        let mut overflow = Overflow::default();
        for shown in 0..MAX_SHOWN {
            assert!(!overflow.push(shown));
        }
        assert!(overflow.push(MAX_SHOWN));
        assert!(overflow.push(MAX_SHOWN));
        assert_eq!(overflow.count, 2);
        // closing a newer balloon keeps the stub, closing the oldest takes it
        overflow.closed(2);
        assert_eq!(overflow.count, 2);
        overflow.closed(0);
        assert_eq!(overflow.count, 0);
    }

    #[test]
    fn actions_fold_into_more_from_the_right() {
        // they fit: no More
        assert_eq!(fold_actions(&[50., 50.], 116., 40.), 2);
        // the third doesn't: two links and More would be 50+50+40+32 = 172
        assert_eq!(fold_actions(&[50., 50., 50.], 172., 40.), 2);
        assert_eq!(fold_actions(&[50., 50., 50.], 171., 40.), 1);
        // not even one beside More
        assert_eq!(fold_actions(&[200., 10.], 100., 40.), 0);
        // a single action never folds
        assert_eq!(fold_actions(&[500.], 100., 40.), 1);
    }

    #[test]
    fn text_shows_two_to_four_lines_until_expanded() {
        assert_eq!(collapsed_lines(false, false), 4);
        assert_eq!(collapsed_lines(true, false), 3);
        assert_eq!(collapsed_lines(true, true), 2);
    }

    #[test]
    fn the_denser_layout_for_title_text_and_actions() {
        assert_eq!(spacing(true, 1, true), THREE_LINES);
        assert_eq!(spacing(true, 1, false), TWO_LINES);
        assert_eq!(spacing(false, 2, false), TWO_LINES);
        assert_eq!(spacing(false, 3, false), THREE_LINES);
        assert_eq!(spacing(true, 2, false), THREE_LINES);
    }
}
