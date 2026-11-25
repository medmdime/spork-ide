//! The look every Spork module shares, after IntelliJ's New UI "Islands": the light and dark
//! themes, the Inter and JetBrains Mono fonts, and IntelliJ's icons (Apache-2.0, see
//! `assets/icons/NOTICE-IntelliJ.txt`). Kept apart from the git client so the database grid and
//! the editor reuse it as they are.

pub mod balloon;
pub mod controls;
mod icon;
pub mod popup;
pub mod shell;
mod spinner;
pub mod tokens;
pub mod tree;
pub mod tree_keys;
mod vcs;

use std::borrow::Cow;

use gpui_kit::component::{Theme, ThemeRegistry};
use gpui_kit::{App, AssetSource, Result, SharedString};

pub use controls::{ActionButton, ActionSize, Button, ComboBox, TextField, ToolbarDropdown};
pub use icon::{file_icon, icon, image, image_in};
pub use spinner::Spinner;
pub use tokens::{Tokens, Typography};
pub use tree_keys::{TreeRow, TreeState, TreeView, tree_keys};
pub use vcs::{
    DiffChange, FileStatus, RefLabel, diff_color, file_status_color, log_dim_color, ref_label_color,
};

thread_local! {
    /// What `{"do": "pick"}` of the end-to-end tests chose for the next file picker.
    static PICKED: std::cell::RefCell<Option<Vec<std::path::PathBuf>>> =
        const { std::cell::RefCell::new(None) };
}

/// For the end-to-end tests: the next file picker that asks through [`prompt_for_paths`] gets
/// these paths and doesn't open, as a test can't drive the system's.
pub fn pick_next(paths: Vec<std::path::PathBuf>) {
    PICKED.set(Some(paths));
}

/// The system's file picker, or the paths a test picked with [`pick_next`].
pub fn prompt_for_paths(
    options: gpui_kit::PathPromptOptions,
    cx: &App,
) -> futures::channel::oneshot::Receiver<anyhow::Result<Option<Vec<std::path::PathBuf>>>> {
    match PICKED.take() {
        Some(paths) => {
            let (sender, receiver) = futures::channel::oneshot::channel();
            // the receiver is right here: a send can't fail
            sender.send(Ok(Some(paths))).ok();
            receiver
        }
        None => cx.prompt_for_paths(options),
    }
}

#[derive(rust_embed::RustEmbed)]
#[folder = "assets"]
#[include = "icons/**/*.svg"]
#[include = "fonts/*.ttf"]
struct Embedded;

/// Where this crate's assets sit in the asset source, next to gpui-kit's `icons/…`.
const PREFIX: &str = "wb/";

/// Where gpui-kit's own icons are drawn from (`icon::install`): the IDE's icons have the same
/// paths (`icons/check.svg`), and its asset source sends this prefix here.
const KIT: &str = "kit/";

/// gpui-kit's assets plus this crate's, which are under `wb/`. Pass it to `with_assets`.
pub struct Assets;

impl Assets {
    /// Whether `path` is one only these assets have, which an app with assets of its own (the
    /// IDE) loads from here.
    pub fn serves(path: &str) -> bool {
        path.starts_with(PREFIX) || path.starts_with(KIT)
    }
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(path) = path.strip_prefix(PREFIX) {
            return Ok(Embedded::get(path).map(|file| file.data));
        }
        gpui_kit::assets::AllAssets.load(path.strip_prefix(KIT).unwrap_or(path))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        match path.strip_prefix(PREFIX) {
            Some(dir) => Ok(Embedded::iter()
                .filter(|name| name.starts_with(dir))
                .map(|name| format!("{PREFIX}{name}").into())
                .collect()),
            None => gpui_kit::assets::AllAssets.list(path),
        }
    }
}

/// The interface's text size when the user hasn't chosen one, as in IntelliJ.
pub const DEFAULT_TEXT_SIZE: f32 = 13.;

/// Sets the interface's text size. gpui-kit's widgets draw their text at 0.875 of the base size
/// (`text_sm`), so the base becomes `size / 0.875`; everything sized in rems (rows, icons,
/// spacing) grows with it. Call after each theme change: a theme sets the base anew.
pub fn set_text_size(size: f32, cx: &mut App) {
    Theme::update(cx, |theme| theme.font_size = gpui_kit::px(size / 0.875));
}

/// Loads the fonts, makes the Islands themes the light and dark ones `Theme::change` picks from,
/// and draws gpui-kit's own icons (menus, inputs, checkboxes…) with IntelliJ's. Call once, after
/// `gpui_kit::init`.
pub fn install(cx: &mut App) {
    let fonts = Embedded::iter()
        .filter(|name| name.ends_with(".ttf"))
        .filter_map(|name| Embedded::get(&name))
        .map(|file| file.data)
        .collect();
    cx.text_system()
        .add_fonts(fonts)
        .expect("bundled fonts are valid");
    let registry = ThemeRegistry::global_mut(cx);
    registry
        .load_themes_from_str(include_str!("../assets/themes/islands.json"))
        .expect("bundled theme is valid JSON");
    let themes = registry.themes();
    let (light, dark) = (
        themes["Islands Light"].clone(),
        themes["Islands Dark"].clone(),
    );
    Theme::update(cx, |theme| {
        theme.light_theme = light;
        theme.dark_theme = dark;
    });
    icon::install(cx);
}
