//! Changed images in Local Changes: before and after side by side instead of "Binary files
//! differ" (docs/specs/M08 §3).
// shortcut: side by side, overlay (onion skin) and swipe; no pixel highlight yet.

use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState, SliderValue};
use gpui_kit::component::{ActiveTheme as _, Selectable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::RepoView;

/// Image formats gpui can draw, by file extension.
pub(crate) fn image_format(path: &str) -> Option<ImageFormat> {
    let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => ImageFormat::Png,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "gif" => ImageFormat::Gif,
        "webp" => ImageFormat::Webp,
        "svg" => ImageFormat::Svg,
        "bmp" => ImageFormat::Bmp,
        "ico" => ImageFormat::Ico,
        "tif" | "tiff" => ImageFormat::Tiff,
        _ => return None,
    })
}

/// The two versions of a changed image; `None` where the file doesn't exist (added / deleted).
pub(crate) struct ImagePair {
    before: Option<(Arc<Image>, usize)>,
    after: Option<(Arc<Image>, usize)>,
    mode: Mode,
    /// Overlay: the new version's opacity. Swipe: where the divider is, from the left.
    amount: Entity<SliderState>,
    _amount_changes: Subscription,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    SideBySide,
    /// Both versions stacked, the new one see-through, to spot small changes.
    Overlay,
    /// Both stacked, the old one left of a divider and the new one right of it.
    Swipe,
}

impl RepoView {
    /// Unstaged: the index vs the working copy. Staged: HEAD vs the index.
    pub(crate) fn load_image_pair(
        &mut self,
        path: String,
        staged: bool,
        format: ImageFormat,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(repo)) = &self.repo else { return };
        let root = repo.root.clone();
        self._change_diff_load = Some(cx.spawn(async move |this, cx| {
            let pair = cx
                .background_spawn(async move {
                    let blob = |spec: String| spork_git::file::blob(&root, &spec).ok();
                    let (before, after) = if staged {
                        (blob(format!("HEAD:{path}")), blob(format!(":{path}")))
                    } else {
                        (
                            blob(format!(":{path}")),
                            std::fs::read(root.join(&path)).ok(),
                        )
                    };
                    let image = |bytes: Option<Vec<u8>>| {
                        bytes.map(|b| {
                            let len = b.len();
                            (Arc::new(Image::from_bytes(format, b)), len)
                        })
                    };
                    (image(before), image(after))
                })
                .await;
            this.update(cx, |this, cx| {
                let amount = cx.new(|_| {
                    SliderState::new()
                        .min(0.)
                        .max(1.)
                        .step(0.01)
                        .default_value(0.5)
                });
                let _amount_changes =
                    cx.subscribe(&amount, |_, _, _: &SliderEvent, cx| cx.notify());
                this.change_images = Some(ImagePair {
                    before: pair.0,
                    after: pair.1,
                    mode: Mode::SideBySide,
                    amount,
                    _amount_changes,
                });
                cx.notify();
            })
            .ok();
        }));
    }

    pub(crate) fn image_pair_view(&self, cx: &mut Context<Self>) -> Option<Div> {
        let pair = self.change_images.as_ref()?;
        let theme = cx.theme();
        let (muted, border, accent) = (theme.muted_foreground, theme.border, theme.primary);
        let side = |label: &str, version: &Option<(Arc<Image>, usize)>| {
            let caption = match version {
                Some((_, len)) => format!("{label} · {}", size(*len)),
                None => format!("{label} · none"),
            };
            v_flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .p_3()
                .gap_2()
                .child(div().text_xs().text_color(muted).child(caption))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .border_1()
                        .border_color(border)
                        .children(version.as_ref().map(|(image, _)| {
                            img(image.clone())
                                .max_w_full()
                                .max_h_full()
                                .object_fit(ObjectFit::Contain)
                        })),
                )
        };
        let both = pair.before.is_some() && pair.after.is_some();
        let toggle = |id: &'static str, label: &'static str, mode: Mode| {
            Button::new(id)
                .ghost()
                .xsmall()
                .label(label)
                .selected(pair.mode == mode)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(p) = &mut this.change_images {
                        p.mode = mode;
                    }
                    cx.notify();
                }))
        };
        let hint = match pair.mode {
            Mode::SideBySide => None,
            Mode::Overlay => Some("new version's opacity"),
            Mode::Swipe => Some("old on the left, new on the right"),
        };
        let bar = h_flex()
            .flex_none()
            .px_2()
            .py_0p5()
            .gap_1()
            .border_b_1()
            .border_color(border)
            .child(toggle("img-side", "Side by side", Mode::SideBySide))
            .child(toggle("img-overlay", "Overlay", Mode::Overlay))
            .child(toggle("img-swipe", "Swipe", Mode::Swipe))
            .when_some(hint, |bar, hint| {
                bar.child(div().w(px(160.)).px_2().child(Slider::new(&pair.amount)))
                    .child(div().text_xs().text_color(muted).child(hint))
            });
        let amount = match pair.amount.read(cx).value() {
            SliderValue::Single(v) => v,
            SliderValue::Range(_, v) => v,
        };
        let stacked = |image: &Arc<Image>| {
            img(image.clone())
                .absolute()
                .size_full()
                .object_fit(ObjectFit::Contain)
        };
        let frame = || div().relative().size_full().border_1().border_color(border);
        let body = match (&pair.before, &pair.after) {
            (Some((before, _)), Some((after, _))) if pair.mode == Mode::Overlay => {
                div().flex_1().min_h_0().p_3().child(
                    frame()
                        .child(stacked(before))
                        .child(stacked(after).opacity(amount)),
                )
            }
            (Some((before, _)), Some((after, _))) if pair.mode == Mode::Swipe => {
                // the old image is clipped to the divider; inside the clip it keeps the frame's
                // full width (1/x of the clip), so both line up pixel for pixel
                let x = amount.max(0.001);
                div().flex_1().min_h_0().p_3().child(
                    frame()
                        .child(stacked(after))
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .h_full()
                                .w(relative(x))
                                .overflow_hidden()
                                .child(stacked(before).w(relative(1. / x))),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .h_full()
                                .w(px(2.))
                                .left(relative(x))
                                .bg(accent),
                        ),
                )
            }
            _ => h_flex()
                .flex_1()
                .min_h_0()
                .child(side("Before", &pair.before))
                .child(side("After", &pair.after)),
        };
        Some(
            v_flex()
                .size_full()
                .when(both, |v| v.child(bar))
                .child(body),
        )
    }
}

fn size(bytes: usize) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}
