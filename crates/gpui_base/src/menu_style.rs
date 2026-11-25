// Modified for Spork IDE: added. Spork's popup menus, one look for gpui-kit's `PopupMenu` (Spork)
// and the IDE's `ContextMenu`: their metrics in rems of the interface's text size (13 / 0.875 px
// in both apps), their surface's shadow and how they write a shortcut. Each app paints them in its
// theme's colors.

use gpui::{BoxShadow, Hsla, Keystroke, Pixels, Rems, hsla, px, rems};

/// Between the menu's edge and its rows.
pub const PADDING: Rems = rems(0.25);
/// Between two rows, and above and below a separator.
pub const ROW_GAP: Rems = rems(0.125);
/// A row's height.
pub const ROW_HEIGHT: Rems = rems(1.625);
/// Between a row's edge and its content.
pub const ROW_PADDING: Rems = rems(0.5);
/// Between a row's icon and its label.
pub const ROW_ICON_GAP: Rems = rems(0.25);
/// A row's icon, check mark or submenu arrow (gpui-kit's `xsmall`).
pub const ICON_SIZE: Rems = rems(0.75);
/// The menu's and its rows' corners.
pub const RADIUS: Pixels = px(4.);
/// A separator's line.
pub const SEPARATOR_WIDTH: Pixels = px(2.);
/// The narrowest and widest a menu is drawn.
pub const MIN_WIDTH: Rems = rems(8.);
pub const MAX_WIDTH: Rems = rems(31.25);
/// The least room between a label and its shortcut, the shortcut's text size and its least width.
pub const SHORTCUT_GAP: Rems = rems(0.75);
pub const SHORTCUT_TEXT: Rems = rems(0.75);
pub const SHORTCUT_MIN_WIDTH: Rems = rems(1.25);

/// Ink every layer of a surface's shadow carries.
pub const SHADOW_INK: f32 = 0.1;
/// The share of the foreground color in the hairline ring standing in for a popup's border.
pub const RING_INK: f32 = 0.1;

/// A popup surface's shadow at `strength` of its full ink: a 1 px `ring` just outside the
/// surface (translucent, so the shadow shows through it) and two blurred layers below.
///
/// The blur radii are Tailwind's `shadow-md` halved: CSS takes a blur as twice the gaussian's
/// deviation, GPUI's shader as the deviation itself.
pub fn popup_shadow(ring: Hsla, strength: f32) -> Vec<BoxShadow> {
    let strength = strength.clamp(0., 1.);
    let ink = hsla(0., 0., 0., SHADOW_INK * strength);
    vec![
        BoxShadow::new(px(0.), px(0.), ring.alpha(ring.a * strength))
            .blur_radius(px(0.))
            .spread_radius(px(1.)),
        BoxShadow::new(px(0.), px(4.), ink)
            .blur_radius(px(3.))
            .spread_radius(px(-1.)),
        BoxShadow::new(px(0.), px(2.), ink)
            .blur_radius(px(2.))
            .spread_radius(px(-2.)),
    ]
}

/// A keystroke as a menu writes it: on macOS its modifiers in `⌃⌥⇧⌘` order and its key's symbol
/// run together (`⌥⇧⌘C`), elsewhere `Ctrl+Alt+Shift+Win+C`.
pub fn keystroke_text(key: &Keystroke) -> String {
    let mac = cfg!(target_os = "macos");
    let mut parts: Vec<&str> = [
        (key.modifiers.control, "⌃", "Ctrl"),
        (key.modifiers.alt, "⌥", "Alt"),
        (key.modifiers.shift, "⇧", "Shift"),
        (key.modifiers.platform, "⌘", "Win"),
    ]
    .into_iter()
    .filter(|(held, _, _)| *held)
    .map(|(_, symbol, name)| if mac { symbol } else { name })
    .collect();

    let key_str = key.key.as_str();
    let named = match (key_str, mac) {
        ("ctrl", true) => Some("⌃"),
        ("ctrl", false) => Some("Ctrl"),
        ("alt", true) => Some("⌥"),
        ("alt", false) => Some("Alt"),
        ("shift", true) => Some("⇧"),
        ("shift", false) => Some("Shift"),
        ("cmd", true) => Some("⌘"),
        ("cmd", false) => Some("Win"),
        ("space", true) => Some("Space"),
        ("backspace", true) => Some("⌫"),
        ("backspace", false) => Some("Backspace"),
        // gpui-kit's `Kbd` has always written forward delete as ⌫ too
        ("delete", true) => Some("⌫"),
        ("delete", false) => Some("Delete"),
        ("escape", true) => Some("⎋"),
        ("escape", false) => Some("Esc"),
        ("enter", true) => Some("⏎"),
        ("enter", false) => Some("Enter"),
        ("pagedown", _) => Some("Page Down"),
        ("pageup", _) => Some("Page Up"),
        ("left", true) => Some("←"),
        ("left", false) => Some("Left"),
        ("right", true) => Some("→"),
        ("right", false) => Some("Right"),
        ("up", true) => Some("↑"),
        ("up", false) => Some("Up"),
        ("down", true) => Some("↓"),
        ("down", false) => Some("Down"),
        _ => None,
    };
    let keys = match named {
        Some(name) => name.to_owned(),
        None => {
            let mut chars = key_str.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        }
    };
    parts.push(&keys);
    parts.join(if mac { "" } else { "+" })
}

#[cfg(test)]
mod tests {
    use super::keystroke_text;
    use gpui::Keystroke;

    #[test]
    fn keystrokes_are_written_as_menus_write_them() {
        fn text(source: &'static str) -> String {
            keystroke_text(&Keystroke::parse(source).unwrap())
        }
        if cfg!(target_os = "macos") {
            assert_eq!(text("cmd-alt-shift-c"), "⌥⇧⌘C");
            assert_eq!(text("f2"), "F2");
            assert_eq!(text("alt-left"), "⌥←");
            assert_eq!(text("pageup"), "Page Up");
        } else {
            assert_eq!(text("ctrl-shift-c"), "Ctrl+Shift+C");
            assert_eq!(text("f2"), "F2");
        }
    }
}
