// Modified for Spork IDE: added. IntelliJ's UI Options every menu follows, gpui-kit's `PopupMenu`
// (Spork's) and the IDE's `ContextMenu` alike: whether rows show their icons (Display icons in
// menu items). The app sets it from its settings; it is on until it does.

use std::sync::atomic::{AtomicBool, Ordering};

static SHOW_ICONS: AtomicBool = AtomicBool::new(true);

/// Whether menu rows show their icons.
pub fn show_icons() -> bool {
    SHOW_ICONS.load(Ordering::Relaxed)
}

pub fn set_show_icons(show: bool) {
    SHOW_ICONS.store(show, Ordering::Relaxed);
}
