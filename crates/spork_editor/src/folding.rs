//! IntelliJ's New UI gutter and folding (`EditorGutterLayout`, `FoldingAnchorsOverlayStrategy`):
//! the line numbers first, then the icons, the folding outline and the change markers; the
//! outline's 9 px chevrons (⌄ on an expanded region while the pointer is over the gutter, › on a
//! collapsed one), and a collapsed region's `...` on the folded text colours.

use std::rc::Rc;

use editor::{FoldPlaceholderLook, FoldToggleIcon, IntellijGutter};
use gpui::{
    App, InteractiveElement as _, IntoElement as _, ParentElement as _, Styled as _, div, img, px,
};
use settings::Settings as _;
use spork_workbench::Tokens;
use theme::ActiveTheme as _;
use theme_settings::ThemeSettings;

/// The folding outline's icons (`AllIcons.Gutter.Fold`, `Unfold`): 9 px.
const FOLD_ICON: f32 = 9.;

pub(crate) fn init(cx: &mut App) {
    cx.set_global(IntellijGutter);
    cx.set_global(FoldToggleIcon(Rc::new(|folded, cx| {
        let name = match folded {
            true => "unfold",
            false => "fold",
        };
        let suffix = match cx.theme().appearance().is_light() {
            true => "",
            false => "_dark",
        };
        img(format!("wb/icons/gutter/{name}{suffix}.svg"))
            .size(px(FOLD_ICON))
            .flex_none()
            .into_any_element()
    })));
    cx.set_global(FoldPlaceholderLook(Rc::new(|fold_id, cx| {
        let tokens = Tokens::for_appearance(!cx.theme().appearance().is_light());
        let settings = ThemeSettings::get_global(cx);
        let line_height = settings.buffer_font_size(cx) * settings.line_height();
        // Islands' rounded highlighting: a sixth of the line's height (`HighlighterVisualModel`)
        div()
            .id(fold_id)
            .font(settings.buffer_font.clone())
            .text_color(tokens.folded_text)
            .bg(tokens.folded_text_bg)
            .rounded(line_height / 6.)
            .size_full()
            .child("...")
    })));
}
