// Spork IDE: the scrollbars are Spork's, gpui-kit's `Scrollbar` (`gpui_base`), which draws a thin
// thumb over the content without a track, shows it while the content scrolls and while the pointer
// is over it, widens it under the pointer and fades it out after a pause. This keeps Zed's builder
// (`Scrollbars`, `WithScrollbar`) and its settings; Zed's own painting is gone.

use std::any::Any;

use gpui::{
    Along, App, AppContext as _, Axis as ScrollbarAxis, Bounds, Context, Div, ElementId, Entity,
    EntityId, Hsla, InteractiveElement as _, IntoElement, ListState, ParentElement, Pixels, Point,
    Render, ScrollHandle, Size, Stateful, StatefulInteractiveElement, Subscription,
    UniformListScrollHandle, Window, prelude::FluentBuilder as _,
};
use gpui_base::{ScrollbarHandle, ScrollbarMode};
use theme::ActiveTheme as _;

use crate::scrollbars::{ScrollbarAutoHide, ScrollbarVisibility, ShowScrollbar};

/// The strip the editor keeps for its scrollbar: Spork's scrollbar's track.
pub const EDITOR_SCROLLBAR_WIDTH: Pixels = gpui_base::Scrollbar::width();

pub mod scrollbars {
    use gpui::{App, Global};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    /// When to show the scrollbar in the editor.
    ///
    /// Default: auto
    #[derive(Copy, Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum ShowScrollbar {
        /// Show the scrollbar if there's important information or
        /// follow the system's configured behavior.
        #[default]
        Auto,
        /// Match the system's configured behavior.
        System,
        /// Always show the scrollbar.
        Always,
        /// Never show the scrollbar.
        Never,
    }

    pub trait ScrollbarVisibility: 'static {
        fn visibility(&self, cx: &App) -> ShowScrollbar;
    }

    #[derive(Default)]
    pub struct ScrollbarAutoHide(pub bool);

    impl ScrollbarAutoHide {
        pub fn should_hide(&self) -> bool {
            self.0
        }
    }

    impl Global for ScrollbarAutoHide {}
}

/// How a scrollbar shows for a `scrollbar.show` setting: `None` for never.
pub fn scrollbar_mode(setting: ShowScrollbar, cx: &App) -> Option<ScrollbarMode> {
    match setting {
        ShowScrollbar::Never => None,
        ShowScrollbar::Auto => Some(ScrollbarMode::Scrolling),
        ShowScrollbar::System
            if cx
                .try_global::<ScrollbarAutoHide>()
                .is_some_and(ScrollbarAutoHide::should_hide) =>
        {
            Some(ScrollbarMode::Scrolling)
        }
        ShowScrollbar::System | ShowScrollbar::Always => Some(ScrollbarMode::Always),
    }
}

fn get_scrollbar_state<T>(
    mut config: Scrollbars<T>,
    caller_location: &'static std::panic::Location,
    window: &mut Window,
    cx: &mut App,
) -> Entity<ScrollbarStateWrapper<T>>
where
    T: ScrollableHandle,
{
    let element_id = config.id.take().unwrap_or_else(|| caller_location.into());
    window.use_keyed_state(element_id, cx, |_, cx| {
        let parent_id = cx.entity_id();
        ScrollbarStateWrapper(cx.new(|cx| ScrollbarState::new_from_config(config, parent_id, cx)))
    })
}

pub trait WithScrollbar: Sized {
    type Output;

    fn custom_scrollbars<T>(
        self,
        config: Scrollbars<T>,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::Output
    where
        T: ScrollableHandle;

    #[track_caller]
    fn vertical_scrollbar_for<ScrollHandle: ScrollableHandle + Clone>(
        self,
        scroll_handle: &ScrollHandle,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::Output {
        self.custom_scrollbars(
            Scrollbars::new(ScrollAxes::Vertical)
                .tracked_scroll_handle(scroll_handle)
                .ensure_id(core::panic::Location::caller()),
            window,
            cx,
        )
    }
}

impl WithScrollbar for Stateful<Div> {
    type Output = Self;

    #[track_caller]
    fn custom_scrollbars<T>(
        self,
        config: Scrollbars<T>,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::Output
    where
        T: ScrollableHandle,
    {
        let scrollbar = get_scrollbar_state(config, std::panic::Location::caller(), window, cx);
        render_scrollbar(scrollbar, self, cx)
    }
}

impl WithScrollbar for Div {
    type Output = Stateful<Div>;

    #[track_caller]
    fn custom_scrollbars<T>(
        self,
        config: Scrollbars<T>,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::Output
    where
        T: ScrollableHandle,
    {
        let scrollbar = get_scrollbar_state(config, std::panic::Location::caller(), window, cx);
        // We know this ID stays consistent as long as the element is rendered for
        // consecutive frames, which is sufficient for our use case here
        let scrollbar_entity_id = scrollbar.entity_id();

        render_scrollbar(
            scrollbar,
            self.id(("track-scroll", scrollbar_entity_id)),
            cx,
        )
    }
}

fn render_scrollbar<T>(
    scrollbar: Entity<ScrollbarStateWrapper<T>>,
    div: Stateful<Div>,
    cx: &App,
) -> Stateful<Div>
where
    T: ScrollableHandle,
{
    let state = &scrollbar.read(cx).0;

    div.when_some(state.read(cx).handle_to_track(), |this, handle| {
        this.track_scroll(handle).when_some(
            state.read(cx).visible_axes(),
            |this, axes| match axes {
                ScrollAxes::Horizontal => this.overflow_x_scroll(),
                ScrollAxes::Vertical => this.overflow_y_scroll(),
                ScrollAxes::Both => this.overflow_scroll(),
            },
        )
    })
    .child(state.clone())
}

pub enum ScrollAxes {
    Horizontal,
    Vertical,
    Both,
}

impl ScrollAxes {
    fn apply_to(self, mut along: Point<bool>) -> Point<bool> {
        match self {
            Self::Horizontal => along.x = true,
            Self::Vertical => along.y = true,
            Self::Both => along = Point::new(true, true),
        }
        along
    }
}

#[derive(Clone)]
enum Handle<T: ScrollableHandle> {
    Tracked(T),
    Untracked(fn() -> T),
}

#[derive(Clone)]
pub struct Scrollbars<T: ScrollableHandle = ScrollHandle> {
    id: Option<ElementId>,
    get_visibility: fn(&App) -> ShowScrollbar,
    tracked_entity: Option<Option<EntityId>>,
    scrollable_handle: Handle<T>,
    along: Point<bool>,
}

impl Scrollbars {
    pub fn new(show_along: ScrollAxes) -> Self {
        Self::new_with_setting(show_along, |_| ShowScrollbar::default())
    }

    pub fn always_visible(show_along: ScrollAxes) -> Self {
        Self::new_with_setting(show_along, |_| ShowScrollbar::Always)
    }

    pub fn for_settings<S: ScrollbarVisibility + Default>() -> Scrollbars {
        Scrollbars::new_with_setting(ScrollAxes::Both, |cx| S::default().visibility(cx))
    }
}

impl Scrollbars {
    fn new_with_setting(show_along: ScrollAxes, get_visibility: fn(&App) -> ShowScrollbar) -> Self {
        Self {
            id: None,
            get_visibility,
            scrollable_handle: Handle::Untracked(ScrollHandle::new),
            tracked_entity: None,
            along: show_along.apply_to(Point::default()),
        }
    }
}

impl<ScrollHandle: ScrollableHandle> Scrollbars<ScrollHandle> {
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = Some(id.into());
        self
    }

    fn ensure_id(mut self, id: impl Into<ElementId>) -> Self {
        if self.id.is_none() {
            self.id = Some(id.into());
        }
        self
    }

    /// Notify the current context whenever this scrollbar gets a scroll event
    pub fn notify_content(mut self) -> Self {
        self.tracked_entity = Some(None);
        self
    }

    /// Set a parent model which should be notified whenever this scrollbar gets a scroll event.
    pub fn tracked_entity(mut self, entity_id: EntityId) -> Self {
        self.tracked_entity = Some(Some(entity_id));
        self
    }

    pub fn tracked_scroll_handle<TrackedHandle: ScrollableHandle>(
        self,
        tracked_scroll_handle: &TrackedHandle,
    ) -> Scrollbars<TrackedHandle> {
        let Self {
            id,
            tracked_entity,
            along,
            get_visibility,
            ..
        } = self;

        Scrollbars {
            scrollable_handle: Handle::Tracked(tracked_scroll_handle.clone()),
            id,
            tracked_entity,
            along,
            get_visibility,
        }
    }

    pub fn show_along(mut self, along: ScrollAxes) -> Self {
        self.along = along.apply_to(self.along);
        self
    }

    // Spork IDE: Spork's scrollbars draw no track and keep no room beside the content, so these
    // show the scrollbar along `along` as `show_along` does
    pub fn with_track_along(self, along: ScrollAxes, _background_color: Hsla) -> Self {
        self.show_along(along)
    }

    pub fn with_stable_track_along(self, along: ScrollAxes, _background_color: Hsla) -> Self {
        self.show_along(along)
    }
}

/// Lets scrollbars observe settings without `ui` depending on the settings crate.
struct ScrollbarSettingsChanged;

impl gpui::Global for ScrollbarSettingsChanged {}

pub fn observe_scrollbar_settings<T: gpui::Global>(cx: &mut App) {
    cx.observe_global::<T>(|cx| cx.set_global(ScrollbarSettingsChanged))
        .detach();
    cx.observe_global::<ScrollbarAutoHide>(|cx| cx.set_global(ScrollbarSettingsChanged))
        .detach();
}

/// This is used to ensure notifies within the state do not notify the parent
/// unintentionally.
struct ScrollbarStateWrapper<T: ScrollableHandle>(Entity<ScrollbarState<T>>);

/// A scrollbar's handle and settings, kept across frames; Spork's scrollbar keeps its own
/// visibility and hover state.
struct ScrollbarState<T: ScrollableHandle = ScrollHandle> {
    notify_id: Option<EntityId>,
    manually_added: bool,
    scroll_handle: T,
    get_visibility: fn(&App) -> ShowScrollbar,
    along: Point<bool>,
    _subscriptions: Vec<Subscription>,
}

impl<T: ScrollableHandle> ScrollbarState<T> {
    fn new_from_config(config: Scrollbars<T>, parent_id: EntityId, cx: &mut Context<Self>) -> Self {
        let (manually_added, scroll_handle) = match config.scrollable_handle {
            Handle::Tracked(handle) => (true, handle),
            Handle::Untracked(func) => (false, func()),
        };
        ScrollbarState {
            notify_id: config.tracked_entity.map(|id| id.unwrap_or(parent_id)),
            manually_added,
            scroll_handle,
            get_visibility: config.get_visibility,
            along: config.along,
            _subscriptions: vec![
                cx.observe_global::<ScrollbarSettingsChanged>(|_, cx| cx.notify()),
                // Spork's scrollbar notifies this view when it scrolls the content (a drag, a
                // click on the track): the content's view repaints it at its new offset
                cx.observe_self(|this, cx| {
                    if let Some(entity_id) = this.notify_id {
                        App::notify(cx, entity_id);
                    }
                }),
            ],
        }
    }

    fn visible_axes(&self) -> Option<ScrollAxes> {
        match (self.along.x, self.along.y) {
            (false, false) => None,
            (false, true) => Some(ScrollAxes::Vertical),
            (true, false) => Some(ScrollAxes::Horizontal),
            (true, true) => Some(ScrollAxes::Both),
        }
    }

    fn handle_to_track<Handle: ScrollableHandle>(&self) -> Option<&Handle> {
        (!self.manually_added)
            .then(|| (&self.scroll_handle as &dyn Any).downcast_ref::<Handle>())
            .flatten()
    }
}

impl<T: ScrollableHandle> Render for ScrollbarState<T> {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mode = scrollbar_mode((self.get_visibility)(cx), cx);
        let axis = match self.visible_axes() {
            Some(ScrollAxes::Horizontal) => gpui_base::ScrollbarAxis::Horizontal,
            Some(ScrollAxes::Vertical) => gpui_base::ScrollbarAxis::Vertical,
            Some(ScrollAxes::Both) => gpui_base::ScrollbarAxis::Both,
            None => return gpui::Empty.into_any_element(),
        };
        let Some(mode) = mode else {
            return gpui::Empty.into_any_element();
        };
        let colors = cx.theme().colors();
        let (thumb, hover, active) = (
            colors.scrollbar_thumb_background,
            colors.scrollbar_thumb_hover_background,
            colors.scrollbar_thumb_active_background,
        );
        gpui_base::Scrollbar::new(&BaseHandle(self.scroll_handle.clone()))
            .id(("scrollbar", cx.entity_id()))
            .axis(axis)
            .mode(mode)
            // the IDE's theme gives the colors: Spork's in the Islands themes
            .styles(|styles| {
                styles
                    .thumb(|style| style.bg(thumb))
                    .thumb_hover(|style| style.bg(hover))
                    .thumb_active(|style| style.bg(active))
            })
            .into_any_element()
    }
}

/// A scrollable handle as Spork's scrollbar reads it.
#[derive(Clone)]
struct BaseHandle<T: ScrollableHandle>(T);

impl<T: ScrollableHandle> ScrollbarHandle for BaseHandle<T> {
    fn viewport_bounds(&self) -> Bounds<Pixels> {
        self.0.viewport()
    }

    fn offset(&self) -> Point<Pixels> {
        self.0.offset()
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        self.0.set_offset(offset);
    }

    fn content_size(&self) -> Size<Pixels> {
        self.0.content_size()
    }

    fn start_drag(&self) {
        self.0.drag_started();
    }

    fn end_drag(&self) {
        self.0.drag_ended();
    }
}

impl ScrollableHandle for UniformListScrollHandle {
    fn max_offset(&self) -> Point<Pixels> {
        self.0.borrow().base_handle.max_offset()
    }

    fn set_offset(&self, point: Point<Pixels>) {
        self.0.borrow().base_handle.set_offset(point);
    }

    fn offset(&self) -> Point<Pixels> {
        self.0.borrow().base_handle.offset()
    }

    fn viewport(&self) -> Bounds<Pixels> {
        self.0.borrow().base_handle.bounds()
    }
}

impl ScrollableHandle for ListState {
    fn max_offset(&self) -> Point<Pixels> {
        self.max_offset_for_scrollbar()
    }

    fn set_offset(&self, point: Point<Pixels>) {
        self.set_offset_from_scrollbar(point);
    }

    fn offset(&self) -> Point<Pixels> {
        self.scroll_px_offset_for_scrollbar()
    }

    fn drag_started(&self) {
        self.scrollbar_drag_started();
    }

    fn drag_ended(&self) {
        self.scrollbar_drag_ended();
    }

    fn viewport(&self) -> Bounds<Pixels> {
        self.viewport_bounds()
    }
}

impl ScrollableHandle for ScrollHandle {
    fn max_offset(&self) -> Point<Pixels> {
        self.max_offset()
    }

    fn set_offset(&self, point: Point<Pixels>) {
        self.set_offset(point);
    }

    fn offset(&self) -> Point<Pixels> {
        self.offset()
    }

    fn viewport(&self) -> Bounds<Pixels> {
        self.bounds()
    }
}

pub trait ScrollableHandle: 'static + Any + Sized + Clone {
    fn max_offset(&self) -> Point<Pixels>;
    fn set_offset(&self, point: Point<Pixels>);
    fn offset(&self) -> Point<Pixels>;
    fn viewport(&self) -> Bounds<Pixels>;
    fn drag_started(&self) {}
    fn drag_ended(&self) {}

    fn scrollable_along(&self, axis: ScrollbarAxis) -> bool {
        self.max_offset().along(axis) > Pixels::ZERO
    }
    fn content_size(&self) -> Size<Pixels> {
        self.viewport().size + self.max_offset().into()
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::{ScrollbarAutoHide, ScrollbarMode, ShowScrollbar, scrollbar_mode};

    #[gpui::test]
    fn settings_choose_spork_scrollbar_modes(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert_eq!(scrollbar_mode(ShowScrollbar::Never, cx), None);
            assert_eq!(
                scrollbar_mode(ShowScrollbar::Auto, cx),
                Some(ScrollbarMode::Scrolling)
            );
            assert_eq!(
                scrollbar_mode(ShowScrollbar::Always, cx),
                Some(ScrollbarMode::Always)
            );
            assert_eq!(
                scrollbar_mode(ShowScrollbar::System, cx),
                Some(ScrollbarMode::Always)
            );
            cx.set_global(ScrollbarAutoHide(true));
            assert_eq!(
                scrollbar_mode(ShowScrollbar::System, cx),
                Some(ScrollbarMode::Scrolling)
            );
        });
    }
}
