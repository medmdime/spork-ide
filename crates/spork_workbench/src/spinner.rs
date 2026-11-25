//! A loading spinner that always turns. gpui-kit's stops under the system's "Reduce motion",
//! as every GPUI animation does; macOS keeps its own spinners turning there, since showing
//! that work goes on is not decoration. Same builder as gpui-kit's `Spinner`.

use std::sync::OnceLock;
use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, Sizable, Size};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

#[derive(IntoElement)]
pub struct Spinner {
    size: Size,
    color: Option<Hsla>,
}

impl Spinner {
    pub fn new() -> Self {
        Self {
            size: Size::Medium,
            color: None,
        }
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

impl Sizable for Spinner {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl RenderOnce for Spinner {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        // one turn a second, read off the clock: every spinner shows the same angle
        static START: OnceLock<Instant> = OnceLock::new();
        let turn = START
            .get_or_init(Instant::now)
            .elapsed()
            .as_secs_f32()
            .fract();
        // drawn again next frame, for as long as it is on screen
        window.request_animation_frame();
        Icon::new(IconName::Loader)
            .with_size(self.size)
            .when_some(self.color, |icon, color| icon.text_color(color))
            .transform(Transformation::rotate(percentage(turn)))
    }
}
