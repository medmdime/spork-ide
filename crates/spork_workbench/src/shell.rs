//! The window's frame, as in IntelliJ: the tool-window stripe down the left, "islands" (rounded
//! panels) on the window background, the main toolbar's widgets and the status bar.

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::PREFIX;
use crate::tokens::{Tokens, Typography as _, metrics};

/// How much of its color the main toolbar and the stripes keep while the window is in the
/// background, as in IntelliJ (`Island.inactiveAlpha`).
pub const INACTIVE_ALPHA: f32 = 0.56;

/// The toolbars' separator color (`toolbar-border`).
pub fn toolbar_border(cx: &App) -> Hsla {
    Tokens::get(cx).toolbar_border
}

/// A column of tool-window buttons on a window edge: `top` from the top, `bottom` from the
/// bottom, in 40 px cells as IntelliJ's, flush with the main toolbar and the status bar.
/// Children that draw in the text color (⋯) fade with the icons while the window is in the
/// background.
pub fn stripe(
    top: impl IntoIterator<Item = impl IntoElement>,
    bottom: impl IntoIterator<Item = impl IntoElement>,
    window_active: bool,
    tokens: &Tokens,
) -> Div {
    v_flex()
        .w(px(metrics::STRIPE_WIDTH))
        .h_full()
        .flex_none()
        .text_color(stripe_icon_color(
            StripeState::Hidden,
            window_active,
            tokens,
        ))
        .children(top)
        .child(div().flex_1())
        .children(bottom)
}

/// The line between groups of stripe buttons (IntelliJ's `StripeButtonSeparator`, before the
/// first split tool window).
pub fn stripe_separator(tokens: &Tokens) -> Div {
    div()
        .w_full()
        .h(px(10.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(div().w(px(24.)).h(px(1.)).bg(tokens.toolbar_border))
}

/// A stripe button's look: its tool window hidden, open, or open with the focus (IntelliJ's
/// `ToolWindow.Button.selectedBackground`, the accent, under a white icon).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripeState {
    Hidden,
    Open,
    Active,
}

/// A stripe icon's color: white over the accent, else the icons' color, at 56 % while the window
/// is in the background.
pub fn stripe_icon_color(state: StripeState, window_active: bool, tokens: &Tokens) -> Hsla {
    match (state, window_active) {
        (StripeState::Active, _) => white(),
        (_, true) => tokens.icon,
        (_, false) => tokens.icon.opacity(INACTIVE_ALPHA),
    }
}

/// A tool window's 20 px stripe icon: `toolwindows/commit` draws `commit@20x20`.
pub fn stripe_icon(name: &str, color: Hsla) -> Svg {
    svg()
        .path(format!("{PREFIX}icons/{name}@20x20.svg"))
        .size(px(20.))
        .flex_none()
        .text_color(color)
}

/// A stripe icon with IntelliJ's dot at its top right (`BadgeDotProvider`: 7 px, its center at
/// 16.5, 3.5 of the 20 px icon), cut out 1.5 px around from the button's fill.
pub fn stripe_badge(icon: impl IntoElement, dot: Hsla, state: StripeState, tokens: &Tokens) -> Div {
    let ring = match state {
        StripeState::Hidden => tokens.frame,
        StripeState::Open => tokens.frame.blend(tokens.pressed),
        StripeState::Active => tokens.accent,
    };
    div().relative().size(px(20.)).child(icon).child(
        div()
            .absolute()
            .left(px(11.5))
            .top(px(-1.5))
            .size(px(10.))
            .rounded_full()
            .border(px(1.5))
            .border_color(ring)
            .bg(dot),
    )
}

/// A tool-window button: its icon on a 30 px square in a 40 px cell, the square filled while
/// the tool window shows. Add its label, tooltip and `on_click`.
pub fn stripe_button(
    id: impl Into<ElementId>,
    icon: impl IntoElement,
    state: StripeState,
    tokens: &Tokens,
) -> Stateful<Div> {
    let hover = tokens.hover;
    div()
        .id(id)
        .group("stripe-button")
        .w_full()
        .h(px(40.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .role(Role::Button)
        .child(
            div()
                .size(px(30.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(metrics::RADIUS_LARGE))
                .map(|b| match state {
                    StripeState::Hidden => b.group_hover("stripe-button", move |b| b.bg(hover)),
                    StripeState::Open => b.bg(tokens.pressed),
                    StripeState::Active => b.bg(tokens.accent),
                })
                .child(icon),
        )
}

/// A rounded panel on the window background: a tool window, or the main area.
pub fn island(cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .group("island")
        .h_full()
        .min_w_0()
        .rounded(px(10.))
        .bg(theme.background)
        .overflow_hidden()
}

/// The title row of a tool window, its actions after the title.
pub fn tool_window_header(title: impl Into<SharedString>) -> Div {
    h_flex()
        .w_full()
        .h(px(41.))
        .flex_none()
        .pl_3()
        .pr_2()
        .gap_1()
        .child(
            div()
                .flex_1()
                .font_weight(FontWeight::BOLD)
                .truncate()
                .child(title.into()),
        )
}

/// A tool window's header buttons, after its title or tabs.
pub fn tool_window_actions() -> Div {
    h_flex().gap_0p5()
}

/// The vertical toolbar on a tool window tab's left (the Git console's, the Worktrees tab's).
pub fn side_toolbar() -> Div {
    v_flex()
        .w(px(36.))
        .h_full()
        .flex_none()
        .items_center()
        .pt_1()
        .gap_0p5()
}

/// An editor tab, IntelliJ's pill: 28 px tall, 6 px corners, 4 px in from its slot; the selected
/// one filled and outlined (blue while its area has the focus, grey otherwise), the others at
/// 75 % until hovered. Children go in its row: icon, name, close button.
pub fn editor_tab(
    id: impl Into<ElementId>,
    selected: bool,
    focused: bool,
    tokens: &Tokens,
) -> Stateful<Div> {
    let (fill, border) = tab_colors(focused, tokens);
    let hover = tokens.hover;
    h_flex()
        .id(id)
        .group("")
        .h(px(28.))
        .mx_1()
        .px_2()
        .gap_1p5()
        .flex_none()
        .rounded(px(metrics::RADIUS_LARGE))
        .border_1()
        .cursor_pointer()
        .when(selected, |t| t.bg(fill).border_color(border))
        .when(!selected, |t| {
            t.border_color(transparent_black())
                .opacity(0.75)
                .hover(move |t| t.bg(hover).opacity(1.))
        })
}

/// A button inside an editor tab: its close button (`general/close`) or a pinned tab's pin, a
/// 12 px icon in the text color on a square that lights up under the pointer, as gpui-kit's
/// smallest ghost button. Add its label, tooltip and `on_click`.
pub fn tab_button(id: impl Into<ElementId>, icon: &str, tokens: &Tokens) -> Stateful<Div> {
    let hover = tokens.hover;
    div()
        .id(id)
        .size(rems(1.25))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(metrics::RADIUS))
        .cursor_pointer()
        .role(Role::Button)
        .hover(move |b| b.bg(hover))
        // a press doesn't move the focus, as a button's doesn't
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
        .child(
            svg()
                .path(format!("{PREFIX}icons/{icon}.svg"))
                .size(rems(0.75))
                .text_color(tokens.text),
        )
}

/// A selected tab's fill and outline: blue while its area has the focus, grey otherwise.
fn tab_colors(focused: bool, tokens: &Tokens) -> (Hsla, Hsla) {
    match focused {
        true => (tokens.tab_selected, tokens.tab_selected_border),
        false => (
            tokens.tab_selected_inactive,
            tokens.tab_selected_border_inactive,
        ),
    }
}

/// A tab in a tool window's header (Commit | Stash, Git's Console): a 28 px pill, the selected
/// one grey and outlined. Add `on_click`.
pub fn header_tab(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    let tokens = Tokens::get(cx);
    let (fill, border) = tab_colors(false, tokens);
    let hover = tokens.hover;
    h_flex()
        .id(id)
        .h(px(28.))
        .px(px(8.))
        .rounded(px(6.))
        .border_1()
        .cursor_pointer()
        .when(selected, |t| t.bg(fill).border_color(border))
        .when(!selected, |t| {
            t.border_color(transparent_black())
                .hover(move |t| t.bg(hover))
        })
        .child(label.into())
}

/// A new terminal session's name, IntelliJ's: "Local", then "Local (2)", "Local (3)"…, the
/// first name no open session has.
pub fn next_terminal_name(taken: &[&str]) -> String {
    (1..)
        .map(|n| match n {
            1 => "Local".to_owned(),
            n => format!("Local ({n})"),
        })
        .find(|name| !taken.contains(&name.as_str()))
        .unwrap_or_default()
}

/// The row of editor tabs above the main area: 40 px, its tabs from 3 px in.
pub fn editor_tabs() -> Div {
    h_flex().w_full().h(px(40.)).flex_none().px(px(3.))
}

/// The project's badge in the main toolbar: up to two initials on a color picked from the name,
/// so each repository keeps its color.
pub fn project_badge(name: &str, slot: usize) -> Div {
    // IntelliJ's avatar slots (RecentProject.ColorN.Avatar): Start, End
    const SLOTS: [(u32, u32); 9] = [
        (0xE08855, 0xE9806F),
        (0xB08B14, 0xBB7F19),
        (0xA1A359, 0x87AA59),
        (0x3B92B8, 0x6183EC),
        (0x3574F0, 0x7A64F0),
        (0xC84D8F, 0xA956CF),
        (0x955AE0, 0xA84DE0),
        (0x24A394, 0x279CCD),
        (0x5FAD65, 0x3D968B),
    ];
    let (start, end) = SLOTS[slot % SLOTS.len()];
    div()
        .size(px(20.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        // from End at the bottom left to Start at the top right
        .bg(linear_gradient(
            45.,
            linear_color_stop(rgb(end), 0.),
            linear_color_stop(rgb(start), 1.),
        ))
        .text_color(white())
        .font_family("JetBrains Mono")
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(initials(name))
}

/// A project's initials, by IntelliJ's rule: the two capitals of a CamelCase name
/// ("MyProject" → MP), else the first letters of its first and last words ("my-cool-app" →
/// MA), else its first letter.
pub fn initials(name: &str) -> String {
    let capitals: Vec<char> = name.chars().filter(char::is_ascii_uppercase).collect();
    if let [first, .., last] = capitals[..] {
        return [first, last].iter().collect();
    }
    let words: Vec<&str> = name
        .split([' ', ',', '-', '_', '.'])
        .filter(|w| !w.is_empty())
        .collect();
    let first_letter = |w: &str| w.chars().next().into_iter().flat_map(char::to_uppercase);
    match words[..] {
        [first, .., last] => first_letter(first).chain(first_letter(last)).collect(),
        [only] => first_letter(only).collect(),
        [] => String::new(),
    }
}

/// The project's color slot (0–8): the one IntelliJ or the reference app gave it if it was opened there
/// (their `recentProjects.xml`), so it looks the same in both, else one picked from its name.
pub fn project_slot(root: &std::path::Path) -> usize {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{LazyLock, Mutex};
    static SLOTS: LazyLock<Mutex<HashMap<PathBuf, usize>>> = LazyLock::new(Default::default);
    if let Some(slot) = SLOTS.lock().ok().and_then(|s| s.get(root).copied()) {
        return slot;
    }
    let name = root
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    // shortcut: IntelliJ hands out slots from a stored counter; a hash needs no storage.
    let hash = name
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    let slot = intellij_slot(root).unwrap_or(hash as usize % 9);
    if let Ok(mut slots) = SLOTS.lock() {
        slots.insert(root.to_owned(), slot);
    }
    slot
}

/// The slot IntelliJ-based IDEs saved for `root` (`RecentProjectColorInfo associatedIndex`).
fn intellij_slot(root: &std::path::Path) -> Option<usize> {
    let home = std::path::PathBuf::from(std::env::var_os("HOME")?);
    let key = match root.strip_prefix(&home) {
        Ok(rest) => format!("$USER_HOME$/{}", rest.display()),
        Err(_) => root.display().to_string(),
    };
    let entry = format!("<entry key=\"{key}\">");
    let configs = ["Library/Application Support", ".config"]
        .into_iter()
        .flat_map(|base| ["detachhead", "JetBrains"].map(|vendor| home.join(base).join(vendor)))
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flatten()
        .flatten();
    configs.into_iter().find_map(|ide| {
        let text = std::fs::read_to_string(ide.path().join("options/recentProjects.xml")).ok()?;
        slot_in(&text, &entry)
    })
}

/// The `associatedIndex` inside the `<entry …>` that starts with `entry`.
fn slot_in(text: &str, entry: &str) -> Option<usize> {
    let rest = &text[text.find(entry)?..];
    let rest = &rest[..rest.find("</entry>")?];
    let index = &rest[rest.find("associatedIndex=\"")? + 17..];
    let digits: String = index.chars().take_while(char::is_ascii_digit).collect();
    Some(digits.parse::<usize>().ok()? % 9)
}

/// Whether [`project_glow`] paints the project's color (IntelliJ's Use project colors in main
/// toolbar): it does until the app says otherwise.
pub struct ProjectColors(pub bool);

impl Global for ProjectColors {}

/// The project's color behind the frame, as IntelliJ's Islands paint it
/// (`IslandsGradientPainter.renderFrameGradientImage`): 85 % of the project's color at the
/// project widget (`center_x`), fading linearly to the frame's color at the window's left edge,
/// 600 px to the right and 300 px down; the islands cover it. At 56 % while the window is in
/// the background. Put it first in the window's root, over the frame's color (`frame`).
pub fn project_glow(
    slot: usize,
    center_x: Pixels,
    window_active: bool,
    dark: bool,
    frame: Hsla,
    cx: &App,
) -> impl IntoElement + use<> {
    const DARK: [u32; 9] = [
        0x664832, 0x5E4724, 0x45522F, 0x335661, 0x394773, 0x613861, 0x503D70, 0x325959, 0x365439,
    ];
    const LIGHT: [u32; 9] = [
        0xF7D7CB, 0xF5E6B8, 0xE2F2C9, 0xCBE4F7, 0xD4D2FC, 0xECD3F5, 0xE4D3F5, 0xCEF0E9, 0xCEEDD4,
    ];
    // RecentProject.MainToolbarGradient.width / .height, and the band each paint covers
    const WIDTH: f32 = 600.;
    const HEIGHT: f32 = 300.;
    const BAND: f32 = 2.;
    let start = match dark {
        true => DARK[slot % 9],
        false => LIGHT[slot % 9],
    };
    let (frame, start) = (Rgba::from(frame), Rgba::from(Hsla::from(rgb(start))));
    let mix = |from: f32, to: f32| from + (to - from) * 0.85;
    let peak = Hsla::from(Rgba {
        r: mix(frame.r, start.r),
        g: mix(frame.g, start.g),
        b: mix(frame.b, start.b),
        a: 1.,
    });
    let strength = match (cx.try_global::<ProjectColors>(), window_active) {
        (Some(ProjectColors(false)), _) => 0.,
        (_, true) => 1.,
        (_, false) => INACTIVE_ALPHA,
    };
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let origin = bounds.origin;
            let mut y = 0.;
            while y < HEIGHT {
                // bilinear: the band's color at the peak fades with its distance from the top
                let color = peak.opacity(strength * (1. - (y + BAND / 2.) / HEIGHT));
                let clear = color.opacity(0.);
                let top = origin.y + px(y);
                window.paint_quad(fill(
                    Bounds::new(point(origin.x, top), size(center_x, px(BAND))),
                    linear_gradient(
                        90.,
                        linear_color_stop(clear, 0.),
                        linear_color_stop(color, 1.),
                    ),
                ));
                window.paint_quad(fill(
                    Bounds::new(point(origin.x + center_x, top), size(px(WIDTH), px(BAND))),
                    linear_gradient(
                        90.,
                        linear_color_stop(color, 0.),
                        linear_color_stop(clear, 1.),
                    ),
                ));
                y += BAND;
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// A dialog window's title row under a transparent title bar, so it follows the app's theme
/// rather than the OS's: the title centered over a line, the traffic lights at its left.
pub fn window_title(title: impl Into<SharedString>, cx: &App) -> Div {
    let tokens = Tokens::get(cx);
    h_flex()
        .w_full()
        .h(px(28.))
        .flex_none()
        .justify_center()
        .bg(tokens.frame)
        .border_b_1()
        .border_color(tokens.separator)
        .text_ui()
        .text_color(tokens.text_muted)
        .font_weight(FontWeight::SEMIBOLD)
        .child(title.into())
}

/// The bar along the window's bottom edge, its text muted.
pub fn status_bar(tokens: &Tokens) -> Div {
    h_flex()
        .w_full()
        .h(rems(1.875))
        .flex_none()
        .px_3()
        .gap_2()
        .text_ui()
        .text_color(tokens.text_muted)
}

#[cfg(test)]
mod tests {
    #[test]
    fn terminal_names_fill_gaps() {
        assert_eq!(super::next_terminal_name(&[]), "Local");
        assert_eq!(super::next_terminal_name(&["Local"]), "Local (2)");
        assert_eq!(
            super::next_terminal_name(&["Local", "Local (3)"]),
            "Local (2)"
        );
    }

    #[test]
    fn initials_follow_intellij() {
        assert_eq!(super::initials("MyProject"), "MP");
        assert_eq!(super::initials("my-cool-app"), "MA");
        assert_eq!(super::initials("ubod"), "U");
        assert_eq!(super::initials("spork_git"), "SG");
        assert_eq!(super::initials(""), "");
    }

    #[test]
    fn reads_intellijs_slot() {
        let xml = r#"<entry key="$USER_HOME$/Projects/ubod">
            <option name="colorInfo"><RecentProjectColorInfo associatedIndex="7" /></option>
        </entry><entry key="$USER_HOME$/Projects/other"></entry>"#;
        assert_eq!(
            super::slot_in(xml, r#"<entry key="$USER_HOME$/Projects/ubod">"#),
            Some(7)
        );
        assert_eq!(
            super::slot_in(xml, r#"<entry key="$USER_HOME$/Projects/other">"#),
            None
        );
    }
}
