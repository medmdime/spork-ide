//! Build, Execution, Deployment › Debugger's options Spork IDE honours (IntelliJ's
//! `XDebuggerSettings` and Java's `DebuggerSettings`), kept in the key-value store as one JSON
//! object and read by the debugger's crates: `spork_run` (Run to Cursor on a line number) and
//! `spork_java` (java-debug's settings, HotSwap, stepping).

use gpui::{App, AppContext as _, Global, TaskExt as _};
use serde::{Deserialize, Serialize};
use util::ResultExt as _;

/// The options, IntelliJ's defaults when unset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DebuggerSettings {
    /// Debugger: Click line number to perform run to cursor.
    pub click_line_number_runs_to_cursor: bool,
    /// Data Views › Java: Show: Static fields.
    pub show_static_fields: bool,
    /// Data Views › Java: Show: Fully qualified names.
    pub show_qualified_names: bool,
    /// Data Views › Java: Show hex value for primitives.
    pub show_hex: bool,
    /// Data Views › Java: Enable alternative view for Collections classes.
    pub collection_views: bool,
    /// Data Views › Java: Enable 'toString()' object view.
    pub to_string_view: bool,
    /// HotSwap: Build project before reloading classes.
    pub build_before_reload: bool,
    /// HotSwap: Suggest HotSwap in the editor when code is modified (the "Code changed:" toolbar).
    pub suggest_hotswap: bool,
    /// HotSwap: Reload classes after compilation: Always, Never or Ask.
    pub reload_after_compilation: Reload,
    /// Stepping: Skip synthetic methods.
    pub skip_synthetic: bool,
    /// Stepping: Skip constructors.
    pub skip_constructors: bool,
    /// Stepping: Do not step into the classes, and the classes with whether each is on.
    pub step_filters_enabled: bool,
    pub step_filters: Vec<(String, bool)>,
    /// Stepping: Resume only the current thread.
    pub resume_only_current_thread: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reload {
    Always,
    Never,
    Ask,
}

/// IntelliJ's "Do not step into the classes" (`DebuggerSettings.DEFAULT_STEPPING_FILTERS`), its
/// own runtime's and Compose's left out.
pub const STEPPING_FILTERS: [&str; 17] = [
    "com.sun.*",
    "java.*",
    "javax.*",
    "org.omg.*",
    "sun.*",
    "jdk.internal.*",
    "junit.*",
    "org.junit.*",
    "com.yourkit.runtime.*",
    "com.springsource.loaded.*",
    "org.springsource.loaded.*",
    "javassist.*",
    "org.apache.webbeans.*",
    "com.ibm.ws.*",
    "org.mockito.*",
    "kotlin.*",
    "kotlinx.*",
];

impl Default for DebuggerSettings {
    fn default() -> Self {
        Self {
            click_line_number_runs_to_cursor: true,
            show_static_fields: false,
            show_qualified_names: false,
            show_hex: false,
            collection_views: true,
            to_string_view: true,
            build_before_reload: true,
            suggest_hotswap: true,
            reload_after_compilation: Reload::Ask,
            skip_synthetic: true,
            skip_constructors: false,
            step_filters_enabled: true,
            step_filters: STEPPING_FILTERS
                .iter()
                .map(|filter| (filter.to_string(), true))
                .collect(),
            resume_only_current_thread: false,
        }
    }
}

/// The options as set, which the debugger's crates observe for their changes.
pub struct DebuggerSettingsGlobal(DebuggerSettings);

impl Global for DebuggerSettingsGlobal {}

/// The key the options are kept under.
const KEY: &str = "spork_debugger_settings";

/// Reads the options from the store, once (the editor asks for them as it draws).
pub(crate) fn init(cx: &mut App) {
    let settings = db::kvp::KeyValueStore::global(cx)
        .read_kvp(KEY)
        .log_err()
        .flatten()
        .and_then(|value| serde_json::from_str(&value).log_err())
        .unwrap_or_default();
    cx.set_global(DebuggerSettingsGlobal(settings));
}

/// The options.
pub fn settings(cx: &App) -> DebuggerSettings {
    cx.try_global::<DebuggerSettingsGlobal>()
        .map(|DebuggerSettingsGlobal(settings)| settings.clone())
        .unwrap_or_default()
}

/// Keeps the options, for the debugger to read at once and the next sessions.
pub fn set_settings(settings: DebuggerSettings, cx: &mut App) -> anyhow::Result<()> {
    let value = serde_json::to_string(&settings)?;
    cx.set_global(DebuggerSettingsGlobal(settings));
    let store = db::kvp::KeyValueStore::global(cx);
    cx.background_spawn(async move { store.write_kvp(KEY.into(), value).await })
        .detach_and_log_err(cx);
    Ok(())
}
