//! The Debug tool window's threads beyond the chooser: IntelliJ's Customize Threads View
//! (`ThreadsViewConfigurable`: what a frame's label shows, the current thread first, the thread
//! groups, synthetic frames), Get Thread Dump (`ThreadDumpPanel` in a "Dump HH:MM:SS" tab: the
//! threads with their state, filtered, each one's text as the language's dump writes it, Copy to
//! Clipboard and Export to Text File) and Export Threads… (IntelliJ's Export Preview: "Export to
//! file:", the dump, Save, Copy, Cancel).

use std::path::PathBuf;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Focusable, Global,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, TaskExt as _, Window, WindowBounds,
    WindowOptions, div, point, px, size,
};
use gpui_kit::component::input::{InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{h_flex, v_flex};
use serde::{Deserialize, Serialize};
use spork_workbench::controls::labeled_check;
use spork_workbench::{ActionButton, Tokens};
use util::ResultExt as _;

/// Customize Threads View's options (`ThreadsViewSettings`), IntelliJ's defaults when unset.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThreadsView {
    pub show_thread_groups: bool,
    pub show_synthetic_frames: bool,
    pub current_thread_on_top: bool,
    pub show_line_number: bool,
    pub show_class_name: bool,
    pub show_package_name: bool,
    pub show_source_file_name: bool,
    pub show_args_types: bool,
}

impl Default for ThreadsView {
    fn default() -> Self {
        Self {
            show_thread_groups: false,
            show_synthetic_frames: true,
            current_thread_on_top: true,
            show_line_number: true,
            show_class_name: true,
            show_package_name: true,
            show_source_file_name: false,
            show_args_types: false,
        }
    }
}

struct ThreadsViewGlobal(ThreadsView);

impl Global for ThreadsViewGlobal {}

const THREADS_VIEW_KEY: &str = "spork_threads_view";

/// The threads view's options, read from the database the first time.
pub fn threads_view(cx: &mut App) -> ThreadsView {
    if let Some(view) = cx.try_global::<ThreadsViewGlobal>() {
        return view.0;
    }
    let view: ThreadsView = db::kvp::KeyValueStore::global(cx)
        .read_kvp(THREADS_VIEW_KEY)
        .log_err()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).log_err())
        .unwrap_or_default();
    cx.set_global(ThreadsViewGlobal(view));
    view
}

fn set_threads_view(view: ThreadsView, cx: &mut App) {
    cx.set_global(ThreadsViewGlobal(view));
    let json = serde_json::to_string(&view);
    let store = db::kvp::KeyValueStore::global(cx);
    cx.background_spawn(async move { store.write_kvp(THREADS_VIEW_KEY.into(), json?).await })
        .detach_and_log_err(cx);
    cx.refresh_windows();
}

/// A JVM frame's label as the threads view's options write it (`StackFrameDescriptorImpl`):
/// `measure(String):44, Debuggee (com.example), Debuggee.java` with every option on.
pub fn jvm_frame_label(
    method: &str,
    args: Option<&str>,
    line: i64,
    class: &str,
    package: Option<&str>,
    source: Option<&str>,
    view: &ThreadsView,
) -> String {
    let mut label = match (view.show_args_types, args) {
        (true, Some(args)) => format!("{method}({args})"),
        _ => method.to_owned(),
    };
    if view.show_line_number {
        label.push_str(&format!(":{line}"));
    }
    if view.show_class_name && !class.is_empty() {
        label.push_str(&format!(", {class}"));
        if let Some(package) =
            package.filter(|package| view.show_package_name && !package.is_empty())
        {
            label.push_str(&format!(" ({package})"));
        }
    }
    if view.show_source_file_name {
        label.push_str(&format!(", {}", source.unwrap_or("Unknown Source")));
    }
    label
}

/// Customize Threads View… (`ThreadsViewConfigurable` in its dialog): OK keeps the options.
pub(crate) fn customize_threads_view(cx: &mut App) {
    let view = threads_view(cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(None, size(px(380.), px(330.)), cx)),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some("Customize Threads View".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.), px(8.))),
        }),
        ..Default::default()
    };
    cx.defer(move |cx| {
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            crate::configurations_dialog::follow_ide_theme(window, cx);
            cx.new(|cx| {
                let focus = cx.focus_handle();
                window.focus(&focus, cx);
                CustomizeThreadsView { view, focus }
            })
        });
        if let Some((handle, _)) = opened.log_err() {
            handle
                .update(cx, |_, window, _| window.activate_window())
                .log_err();
        }
    });
}

struct CustomizeThreadsView {
    view: ThreadsView,
    focus: FocusHandle,
}

impl Focusable for CustomizeThreadsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for CustomizeThreadsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let option = |id: &'static str,
                      label: &'static str,
                      field: fn(&mut ThreadsView) -> &mut bool,
                      this: &mut Self,
                      cx: &mut Context<Self>| {
            labeled_check(id, label, *field(&mut this.view), cx)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let on = field(&mut this.view);
                    *on = !*on;
                    cx.notify();
                }))
                .into_any_element()
        };
        // `ThreadsViewConfigurable.form`'s two groups, in its rows
        let first = vec![
            option(
                "show-thread-groups",
                "Show thread groups",
                |v| &mut v.show_thread_groups,
                self,
                cx,
            ),
            option(
                "show-synthetic-frames",
                "Show stack frames for synthetic methods",
                |v| &mut v.show_synthetic_frames,
                self,
                cx,
            ),
            option(
                "current-thread-on-top",
                "Move current thread to the top",
                |v| &mut v.current_thread_on_top,
                self,
                cx,
            ),
        ];
        let second = vec![
            option(
                "show-line-number",
                "Show line number",
                |v| &mut v.show_line_number,
                self,
                cx,
            ),
            option(
                "show-class-name",
                "Show class name",
                |v| &mut v.show_class_name,
                self,
                cx,
            ),
            option(
                "show-package-name",
                "Show package name",
                |v| &mut v.show_package_name,
                self,
                cx,
            ),
            option(
                "show-source-file-name",
                "Show source file name",
                |v| &mut v.show_source_file_name,
                self,
                cx,
            ),
            option(
                "show-args-types",
                "Show method arguments types",
                |v| &mut v.show_args_types,
                self,
                cx,
            ),
        ];
        v_flex()
            .id("customize-threads-view")
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
            .child(spork_workbench::shell::window_title(
                "Customize Threads View",
                cx,
            ))
            .child(
                v_flex()
                    .flex_1()
                    .px(px(16.))
                    .pt(px(10.))
                    .gap(px(14.))
                    .child(v_flex().gap(px(6.)).children(first))
                    .child(v_flex().gap(px(6.)).children(second)),
            )
            .child(
                h_flex()
                    .flex_none()
                    .justify_end()
                    .gap(px(8.))
                    .px(px(16.))
                    .py(px(10.))
                    .child(
                        spork_workbench::Button::new("threads-view-cancel", "Cancel")
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(
                        spork_workbench::Button::new("threads-view-ok", "OK")
                            .primary(true)
                            .on_click(cx.listener(|this, _, window, cx| {
                                set_threads_view(this.view, cx);
                                window.remove_window();
                            })),
                    ),
            )
    }
}

/// A thread as the language's runtime tells it beyond the protocol: its group and state
/// (Java's `Thread.State`), by its name.
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadInfo {
    pub name: String,
    pub group: String,
    pub state: String,
}

/// IntelliJ's thread status (`DebuggerUtilsEx.getThreadStatusText`) for a Java thread state.
pub(crate) fn thread_status(state: &str) -> &'static str {
    match state {
        "RUNNABLE" => "RUNNING",
        "WAITING" => "WAIT",
        "TIMED_WAITING" => "SLEEPING",
        "BLOCKED" => "MONITOR",
        "NEW" => "NOT_STARTED",
        "TERMINATED" => "ZOMBIE",
        _ => "UNKNOWN",
    }
}

/// A thread of a dump: its name and state as the list shows them (`main`, `waiting`), and its
/// text as the dump writes it.
#[derive(Clone, Debug, PartialEq)]
pub struct DumpThread {
    pub name: String,
    pub state: String,
    pub text: String,
}

/// The whole dump's text (`ThreadDumpPanel`'s export): each thread's, a blank line between.
pub(crate) fn dump_text(threads: &[DumpThread]) -> String {
    threads
        .iter()
        .map(|thread| thread.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// A "Dump HH:MM:SS" tab's content (`ThreadDumpPanel`).
pub(crate) struct DumpView {
    pub(crate) title: SharedString,
    pub(crate) threads: Vec<DumpThread>,
    selected: usize,
    filter: Entity<InputState>,
    /// The project's folder, where Export to Text File writes by default.
    folder: Option<PathBuf>,
    _filter: Subscription,
}

impl DumpView {
    pub(crate) fn new(
        threads: Vec<DumpThread>,
        folder: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx));
        let subscription = cx.subscribe(&filter, |this: &mut Self, _, event, cx| {
            if let InputEvent::Change = event {
                this.selected = 0;
                cx.notify();
            }
        });
        Self {
            title: format!("Dump {}", chrono::Local::now().format("%H:%M:%S")).into(),
            threads,
            selected: 0,
            filter,
            folder,
            _filter: subscription,
        }
    }

    /// The threads the filter keeps: those whose text holds it.
    fn shown(&self, cx: &App) -> Vec<usize> {
        let filter = self.filter.read(cx).value().trim().to_lowercase();
        (0..self.threads.len())
            .filter(|ix| {
                filter.is_empty() || self.threads[*ix].text.to_lowercase().contains(&filter)
            })
            .collect()
    }

    /// The selected thread's text, for the end-to-end tests.
    pub(crate) fn selected_text(&self, cx: &App) -> Option<String> {
        let shown = self.shown(cx);
        let ix = shown.get(self.selected)?;
        Some(self.threads.get(*ix)?.text.clone())
    }
}

impl Render for DumpView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let shown = self.shown(cx);
        let rows: Vec<_> = shown
            .iter()
            .enumerate()
            .filter_map(|(row, ix)| {
                let thread = self.threads.get(*ix)?;
                let selected = row == self.selected;
                Some(
                    h_flex()
                        .id(("dump-thread", row))
                        .role(Role::ListItem)
                        .aria_label(SharedString::from(format!(
                            "{} {}",
                            thread.name, thread.state
                        )))
                        .aria_selected(selected)
                        .h(px(24.))
                        .mx(px(6.))
                        .px(px(6.))
                        .gap(px(6.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .when(selected, |item| item.bg(tokens.selection))
                        .when(!selected, |item| {
                            item.hover(move |item| item.bg(tokens.selection_hover))
                        })
                        .child(div().truncate().child(thread.name.clone()))
                        .child(
                            div()
                                .text_color(tokens.text_secondary)
                                .child(thread.state.clone()),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.selected = row;
                            cx.notify();
                        })),
                )
            })
            .collect();
        let text = shown
            .get(self.selected)
            .and_then(|ix| self.threads.get(*ix))
            .map(|thread| thread.text.clone())
            .unwrap_or_default();
        let all = dump_text(&self.threads);
        let (copied, exported) = (all.clone(), all);
        let folder = self.folder.clone();
        h_flex()
            .id("thread-dump")
            .role(Role::Group)
            .aria_label(self.title.clone())
            .size_full()
            .items_start()
            .child(
                v_flex()
                    .id("thread-dump-toolbar")
                    .role(Role::Toolbar)
                    .aria_label("Thread Dump Toolbar")
                    .flex_none()
                    .w(px(32.))
                    .h_full()
                    .pt(px(4.))
                    .items_center()
                    .gap(px(4.))
                    .border_r_1()
                    .border_color(tokens.separator)
                    .child(
                        ActionButton::new("dump-copy", "general/copy")
                            .tooltip("Copy to Clipboard")
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()))
                            }),
                    )
                    .child(
                        ActionButton::new("dump-export", "general/export")
                            .tooltip("Export to Text File")
                            .on_click(move |_, _, cx| {
                                export_preview(folder.clone(), exported.clone(), cx)
                            }),
                    ),
            )
            .child(
                v_flex()
                    .w(px(280.))
                    .flex_none()
                    .h_full()
                    .border_r_1()
                    .border_color(tokens.separator)
                    .child(
                        h_flex()
                            .flex_none()
                            .px(px(6.))
                            .py(px(4.))
                            .gap(px(6.))
                            .child("Filter:")
                            .child(div().flex_1().child(
                                spork_workbench::TextField::new(&self.filter).aria_label("Filter"),
                            )),
                    )
                    .child(
                        v_flex()
                            .id("dump-threads")
                            .role(Role::List)
                            .aria_label("Threads")
                            .flex_1()
                            .py(px(4.))
                            .overflow_y_scroll()
                            .children(rows),
                    ),
            )
            .child(
                div()
                    .id("dump-text")
                    .role(Role::Group)
                    .aria_label(SharedString::from(text.clone()))
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .p(px(8.))
                    .overflow_y_scroll()
                    .font_family("JetBrains Mono")
                    .text_color(tokens.console_text)
                    .children(
                        text.lines().map(|line| {
                            div().whitespace_nowrap().child(line.replace('\t', "    "))
                        }),
                    ),
            )
    }
}

/// Export Threads… (`ExportToFileUtil.chooseFileAndExport`): IntelliJ's Export Preview, the
/// dump going to `threads_report.txt` in the project's folder unless the field says otherwise.
pub(crate) fn export_preview(folder: Option<PathBuf>, text: String, cx: &mut App) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(None, size(px(720.), px(500.)), cx)),
        window_min_size: Some(size(px(400.), px(300.))),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some("Export Preview".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.), px(8.))),
        }),
        ..Default::default()
    };
    let path = folder.unwrap_or_default().join("threads_report.txt");
    cx.defer(move |cx| {
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            crate::configurations_dialog::follow_ide_theme(window, cx);
            cx.new(|cx| {
                let file = cx.new(|cx| InputState::new(window, cx));
                file.update(cx, |field, cx| {
                    field.set_value(path.to_string_lossy().into_owned(), window, cx)
                });
                let report = cx.new(|cx| TextareaState::new(window, cx));
                report.update(cx, |field, cx| field.set_value(text, window, cx));
                let focus = cx.focus_handle();
                window.focus(&focus, cx);
                ExportPreview {
                    file,
                    report,
                    focus,
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

struct ExportPreview {
    file: Entity<InputState>,
    report: Entity<TextareaState>,
    focus: FocusHandle,
}

impl Focusable for ExportPreview {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ExportPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        v_flex()
            .id("export-preview")
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
            .child(spork_workbench::shell::window_title("Export Preview", cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .px(px(16.))
                    .pt(px(10.))
                    .gap(px(8.))
                    .child(
                        h_flex().gap(px(8.)).child("Export to file:").child(
                            div().flex_1().child(
                                spork_workbench::TextField::new(&self.file)
                                    .aria_label("Export to file"),
                            ),
                        ),
                    )
                    .child(
                        div().flex_1().min_h_0().child(
                            Textarea::new(&self.report)
                                .h_full()
                                .aria_label("Export Preview"),
                        ),
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
                        spork_workbench::Button::new("export-cancel", "Cancel")
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(
                        spork_workbench::Button::new("export-copy", "Copy").on_click(cx.listener(
                            |this, _, window, cx| {
                                let text = this.report.read(cx).value().to_string();
                                cx.write_to_clipboard(ClipboardItem::new_string(text));
                                window.remove_window();
                            },
                        )),
                    )
                    .child(
                        spork_workbench::Button::new("export-save", "Save")
                            .primary(true)
                            .on_click(cx.listener(|this, _, window, cx| {
                                // shortcut: an existing file is overwritten; IntelliJ asks to
                                // overwrite or append
                                let path = PathBuf::from(this.file.read(cx).value().trim());
                                let text = this.report.read(cx).value().to_string();
                                if std::fs::write(&path, text).log_err().is_some() {
                                    window.remove_window();
                                }
                            })),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{ThreadsView, jvm_frame_label};

    #[test]
    fn a_frame_label_follows_the_threads_view() {
        let default = ThreadsView::default();
        let label = |view: &ThreadsView| {
            jvm_frame_label(
                "measure",
                Some("String"),
                44,
                "Debuggee",
                Some("com.example"),
                Some("Debuggee.java"),
                view,
            )
        };
        assert_eq!(label(&default), "measure:44, Debuggee (com.example)");
        let all = ThreadsView {
            show_source_file_name: true,
            show_args_types: true,
            ..default
        };
        assert_eq!(
            label(&all),
            "measure(String):44, Debuggee (com.example), Debuggee.java"
        );
        let bare = ThreadsView {
            show_line_number: false,
            show_class_name: false,
            ..default
        };
        assert_eq!(label(&bare), "measure");
    }
}
