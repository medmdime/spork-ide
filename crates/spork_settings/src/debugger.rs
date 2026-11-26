//! Build, Execution, Deployment › Debugger's options Spork IDE honours (IntelliJ's
//! `XDebuggerSettings` and Java's `DebuggerSettings`), kept in the key-value store as one JSON
//! object and read by the debugger's crates: `spork_run` (Run to Cursor on a line number) and
//! `spork_java` (java-debug's settings, HotSwap, stepping).

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, Global, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Styled as _, Subscription, TaskExt as _, Window,
    WindowBounds, WindowOptions, point, px, size,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use serde::{Deserialize, Serialize};
use spork_workbench::Tokens;
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
    /// A new breakpoint's suspend policy is Thread (the breakpoints' Make Default; All when
    /// off).
    pub default_suspend_thread: bool,
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
            default_suspend_thread: false,
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

/// A one-line question (`Messages.showInputDialog`), as the stepping filters' Add Pattern… and the
/// frames' Add Stepping Filter… ask: `title`, `label` over the field holding `text`, OK giving
/// `answered` what it holds.
pub fn ask_text(
    title: &'static str,
    label: Option<&'static str>,
    text: String,
    answered: Rc<dyn Fn(String, &mut App)>,
    cx: &mut App,
) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(None, size(px(420.), px(160.)), cx)),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some(title.into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.), px(8.))),
        }),
        ..Default::default()
    };
    cx.defer(move |cx| {
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            crate::follow_ide_theme(window, cx);
            cx.new(|cx| {
                let field = cx.new(|cx| InputState::new(window, cx));
                field.update(cx, |field, cx| {
                    field.set_value(text, window, cx);
                    field.focus(window, cx);
                });
                let entered = cx.subscribe_in(
                    &field,
                    window,
                    |this: &mut TextDialog, _, event, window, cx| {
                        if let InputEvent::PressEnter { .. } = event {
                            this.confirm(window, cx)
                        }
                    },
                );
                TextDialog {
                    title,
                    label,
                    field,
                    answered,
                    focus: cx.focus_handle(),
                    _entered: entered,
                }
            })
        });
        if let Some((handle, _)) = opened.log_err() {
            handle
                .update(cx, |_, window, _| window.activate_window())
                .log_err();
        }
    });
}

struct TextDialog {
    title: &'static str,
    label: Option<&'static str>,
    field: Entity<InputState>,
    answered: Rc<dyn Fn(String, &mut App)>,
    focus: FocusHandle,
    _entered: Subscription,
}

impl TextDialog {
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.field.read(cx).value().trim().to_owned();
        if text.is_empty() {
            return;
        }
        (self.answered)(text, cx);
        window.remove_window();
    }
}

impl Focusable for TextDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TextDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        v_flex()
            .id("text-dialog")
            .track_focus(&self.focus)
            .size_full()
            .bg(tokens.panel)
            .text_color(tokens.text)
            .on_key_down(|event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    window.remove_window();
                }
            })
            .child(spork_workbench::shell::window_title(self.title, cx))
            .child(
                v_flex()
                    .flex_1()
                    .px(px(16.))
                    .pt(px(12.))
                    .gap(px(6.))
                    .children(self.label)
                    .child(
                        spork_workbench::TextField::new(&self.field)
                            .aria_label(self.label.unwrap_or(self.title)),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .justify_end()
                    .gap(px(8.))
                    .px(px(16.))
                    .py(px(10.))
                    .child(
                        spork_workbench::Button::new("text-dialog-cancel", "Cancel")
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(
                        spork_workbench::Button::new("text-dialog-ok", "OK")
                            .primary(true)
                            .on_click(cx.listener(|this, _, window, cx| this.confirm(window, cx))),
                    ),
            )
    }
}
