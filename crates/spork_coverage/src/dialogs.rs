//! IntelliJ's coverage dialogs: the Code Coverage question when a run's coverage comes in while
//! other coverage shows (`CoverageDataManagerImpl.askMergeOption`), Run ▸ Manage Coverage
//! Reports… (`CoverageSuiteChooserDialog`, "Choose Coverage Suite to Display") and Generate
//! Coverage Report… (`ExportToHTMLDialog`).

use std::path::PathBuf;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window, div, px,
};
use gpui_kit::component::input::InputState;
use gpui_kit::component::{h_flex, v_flex};
use serde_json::{Value, json};
use spork_settings::coverage::WhenGathered;
use spork_workbench::controls::{check_box, labeled_check};
use spork_workbench::{ActionButton, Button, TextField, Tokens, popup};
use util::ResultExt as _;
use workspace::{ModalView, Workspace};

use crate::{Coverage, Suite, apply, engine, in_window};

/// The Code Coverage question: show the run's coverage in place of the coverage shown, with it,
/// or not at all; "Don't ask again" keeps the answer as the settings' option.
pub struct CodeCoverageQuestion {
    suite: Suite,
    dont_ask: bool,
    coverage: Entity<Coverage>,
    active: Vec<Suite>,
    focus: FocusHandle,
}

/// Asks the Code Coverage question in the workspace's window.
pub(crate) fn ask(
    workspace: &Entity<Workspace>,
    coverage: Entity<Coverage>,
    active: Vec<Suite>,
    suite: Suite,
    cx: &mut App,
) {
    in_window(workspace, cx, move |workspace, window, cx| {
        workspace.toggle_modal(window, cx, move |window, cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            CodeCoverageQuestion {
                suite,
                dont_ask: false,
                coverage,
                active,
                focus,
            }
        });
    });
}

impl CodeCoverageQuestion {
    fn answer(&mut self, answer: WhenGathered, cx: &mut Context<Self>) {
        // IntelliJ keeps the answer, Cancel's excepted (`shouldSaveOptionsOnCancel`)
        if self.dont_ask && answer != WhenGathered::DoNotApply {
            let mut settings = spork_settings::coverage::settings(cx);
            settings.when_gathered = answer;
            spork_settings::coverage::set_settings(settings, cx).log_err();
        }
        apply(
            &self.coverage,
            std::mem::take(&mut self.active),
            self.suite.clone(),
            answer,
            cx,
        );
        cx.emit(DismissEvent);
    }
}

impl EventEmitter<DismissEvent> for CodeCoverageQuestion {}

impl ModalView for CodeCoverageQuestion {}

impl Focusable for CodeCoverageQuestion {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for CodeCoverageQuestion {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let message = format!(
            "Do you want to display coverage data for '{}'?",
            self.suite.title()
        );
        let button = |id: &'static str, label: &'static str, answer: WhenGathered| {
            Button::new(id, label)
                .primary(answer == WhenGathered::Replace)
                .on_click(cx.listener(move |this, _, _, cx| this.answer(answer, cx)))
        };
        spork_vcs::spork_look(cx).child(
            popup::surface(cx)
                .id("code-coverage-question")
                .role(Role::Dialog)
                .aria_label("Code Coverage")
                .w(px(640.))
                .track_focus(&self.focus)
                .key_context("CodeCoverageQuestion menu")
                .on_action(cx.listener(|this, _: &menu::Confirm, _, cx| {
                    this.answer(WhenGathered::Replace, cx)
                }))
                .on_action(cx.listener(|this, _: &menu::Cancel, _, cx| {
                    this.answer(WhenGathered::DoNotApply, cx)
                }))
                .child(popup::title("Code Coverage", cx))
                .child(
                    v_flex()
                        .px(px(16.))
                        .pb(px(14.))
                        .gap(px(12.))
                        .child(
                            div()
                                .id("code-coverage-message")
                                .role(Role::Label)
                                .aria_label(SharedString::from(message.clone()))
                                .child(message),
                        )
                        .child(
                            labeled_check(
                                "code-coverage-dont-ask",
                                "Don't ask again",
                                self.dont_ask,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dont_ask = !this.dont_ask;
                                cx.notify();
                            })),
                        )
                        .child(
                            h_flex()
                                .justify_end()
                                .gap(px(8.))
                                .child(button(
                                    "code-coverage-ignore",
                                    "Do Not Apply Collected Coverage",
                                    WhenGathered::DoNotApply,
                                ))
                                .child(button(
                                    "code-coverage-add",
                                    "Add to Active Suites",
                                    WhenGathered::Add,
                                ))
                                .child(button(
                                    "code-coverage-replace",
                                    "Replace Active Suites",
                                    WhenGathered::Replace,
                                )),
                        ),
                ),
        )
    }
}

/// IntelliJ's `formatPrettyDateTime`: today's and yesterday's by their time, others by date.
fn pretty_time(millis: i64) -> String {
    use chrono::{Local, TimeZone as _};
    let Some(time) = Local.timestamp_millis_opt(millis).single() else {
        return String::new();
    };
    let today = Local::now().date_naive();
    let clock = time.format("%-I:%M %p");
    match (today - time.date_naive()).num_days() {
        0 => format!("Today {clock}"),
        1 => format!("Yesterday {clock}"),
        _ => time.format("%-m/%-d/%y, %-I:%M %p").to_string(),
    }
}

/// "Choose Coverage Suite to Display": the suites known, those shown checked.
pub struct ManageCoverageReports {
    coverage: Entity<Coverage>,
    /// The suites listed and whether each is checked.
    suites: Vec<(Suite, bool)>,
    selected: Option<usize>,
    focus: FocusHandle,
    _list: Subscription,
}

/// Run ▸ Manage Coverage Reports… (⌥⌘F6).
pub(crate) fn manage(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let Some(coverage) = Coverage::of(workspace, cx) else {
        return;
    };
    coverage.update(cx, |coverage, cx| coverage.list(cx));
    workspace.toggle_modal(window, cx, move |window, cx| {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let suites = listed(&coverage, cx);
        let _list = cx.observe(
            &coverage,
            |this: &mut ManageCoverageReports, coverage, cx| {
                let checked: Vec<PathBuf> = this
                    .suites
                    .iter()
                    .filter(|(_, on)| *on)
                    .map(|(suite, _)| suite.file.clone())
                    .collect();
                this.suites = coverage
                    .read(cx)
                    .suites
                    .iter()
                    .map(|suite| (suite.clone(), checked.contains(&suite.file)))
                    .collect();
                cx.notify();
            },
        );
        ManageCoverageReports {
            coverage,
            suites,
            selected: None,
            focus,
            _list,
        }
    });
}

fn listed(coverage: &Entity<Coverage>, cx: &App) -> Vec<(Suite, bool)> {
    let coverage = coverage.read(cx);
    coverage
        .suites
        .iter()
        .filter(|suite| suite.file.is_file())
        .map(|suite| {
            let on = coverage
                .active
                .iter()
                .any(|active| active.file == suite.file);
            (suite.clone(), on)
        })
        .collect()
}

impl ManageCoverageReports {
    /// Add: suites from data files on disk, listed and checked.
    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = spork_workbench::prompt_for_paths(
            gpui_kit::PathPromptOptions {
                files: true,
                directories: false,
                multiple: true,
                prompt: None,
            },
            cx,
        );
        let root = self.coverage.read(cx).root(cx).unwrap_or_default();
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            this.update(cx, |this, cx| {
                for file in paths.into_iter().filter(|path| path.is_file()) {
                    let name = file
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let timestamp = std::fs::metadata(&file)
                        .and_then(|metadata| metadata.modified())
                        .ok()
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map_or(0, |time| time.as_millis() as i64);
                    let suite = Suite {
                        name,
                        file,
                        timestamp,
                        root: root.clone(),
                        branch_coverage: true,
                        track_per_test: false,
                    };
                    this.coverage
                        .update(cx, |coverage, cx| coverage.remember(&suite, cx));
                    this.suites.retain(|(listed, _)| listed.file != suite.file);
                    this.suites.insert(0, (suite, true));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Remove takes the selected suite off the list; Delete removes its file too.
    fn remove(&mut self, delete: bool, cx: &mut Context<Self>) {
        let Some(ix) = self.selected.take() else {
            return;
        };
        let Some((suite, _)) = self.suites.get(ix).cloned() else {
            return;
        };
        self.suites.remove(ix);
        self.coverage.update(cx, |coverage, cx| {
            coverage.forget(&[suite.file], delete, cx)
        });
        cx.notify();
    }

    fn show_selected(&mut self, cx: &mut Context<Self>) {
        let suites: Vec<Suite> = self
            .suites
            .iter()
            .filter(|(_, on)| *on)
            .map(|(suite, _)| suite.clone())
            .collect();
        self.coverage
            .update(cx, |coverage, cx| coverage.show(suites, cx));
        cx.emit(DismissEvent);
    }
}

impl EventEmitter<DismissEvent> for ManageCoverageReports {}

impl ModalView for ManageCoverageReports {}

impl Focusable for ManageCoverageReports {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ManageCoverageReports {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let selected = self.selected;
        let rows = self.suites.iter().enumerate().map(|(ix, (suite, on))| {
            let on = *on;
            let label = SharedString::from(suite.name.clone());
            h_flex()
                .id(("coverage-suite", ix))
                .role(Role::ListItem)
                .aria_label(label.clone())
                .aria_selected(selected == Some(ix))
                .h(px(24.))
                .px(px(6.))
                .gap(px(4.))
                .cursor_pointer()
                .when(selected == Some(ix), |row| row.bg(tokens.selection))
                .child(
                    check_box(("coverage-suite-check", ix), Some(on), cx)
                        .role(Role::CheckBox)
                        .aria_label(label.clone())
                        .aria_toggled(on.into())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some((_, on)) = this.suites.get_mut(ix) {
                                *on = !*on;
                            }
                            cx.notify();
                        })),
                )
                .child(spork_workbench::image("runConfigurations/coverage", cx).size(px(16.)))
                .child(label)
                .child(
                    div()
                        .text_color(tokens.text_secondary)
                        .child(format!(" ({})", pretty_time(suite.timestamp))),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.selected = Some(ix);
                    cx.notify();
                }))
        });
        let list = v_flex()
            .id("coverage-suites")
            .role(Role::List)
            .aria_label("Coverage suites")
            .flex_1()
            .min_h(px(300.))
            .overflow_y_scroll()
            .border_1()
            .border_color(tokens.separator)
            .when(self.suites.is_empty(), |list| {
                list.items_center().justify_center().child(
                    div()
                        .text_color(tokens.text_muted)
                        .child("No coverage suites configured."),
                )
            })
            .children(rows);
        let has_selection = self.selected.is_some();
        let toolbar = h_flex()
            .id("coverage-suites-toolbar")
            .role(Role::Toolbar)
            .aria_label("Coverage suites")
            .gap(px(4.))
            .child(
                ActionButton::new("coverage-suites-add", "general/add")
                    .tooltip("Add")
                    .on_click(cx.listener(|this, _, window, cx| this.add(window, cx))),
            )
            .child(
                ActionButton::new("coverage-suites-remove", "general/remove")
                    .tooltip("Remove")
                    .disabled(!has_selection)
                    .on_click(cx.listener(|this, _, _, cx| this.remove(false, cx))),
            )
            .child(
                ActionButton::new("coverage-suites-delete", "general/delete")
                    .tooltip("Delete")
                    .disabled(!has_selection)
                    .on_click(cx.listener(|this, _, _, cx| this.remove(true, cx))),
            );
        spork_vcs::spork_look(cx).child(
            popup::surface(cx)
                .id("coverage-suite-chooser")
                .role(Role::Dialog)
                .aria_label("Choose Coverage Suite to Display")
                .w(px(420.))
                .track_focus(&self.focus)
                .key_context("ManageCoverageReports menu")
                .on_action(cx.listener(|this, _: &menu::Confirm, _, cx| this.show_selected(cx)))
                .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
                .child(popup::title("Choose Coverage Suite to Display", cx))
                .child(
                    v_flex()
                        .px(px(16.))
                        .pb(px(14.))
                        .gap(px(8.))
                        .child(toolbar)
                        .child(list)
                        .child(
                            h_flex()
                                .justify_end()
                                .gap(px(8.))
                                .child(
                                    Button::new("coverage-suites-cancel", "Cancel")
                                        .on_click(cx.listener(|_, _, _, cx| cx.emit(DismissEvent))),
                                )
                                .child(Button::new("coverage-suites-none", "No Coverage").on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.coverage.update(cx, |coverage, cx| coverage.hide(cx));
                                        cx.emit(DismissEvent);
                                    }),
                                ))
                                .child(
                                    Button::new("coverage-suites-show", "Show Selected")
                                        .primary(true)
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.show_selected(cx)),
                                        ),
                                ),
                        ),
                ),
        )
    }
}

/// Generate Coverage Report…: the folder to write it into and whether to open it.
pub struct GenerateReport {
    coverage: Entity<Coverage>,
    workspace: WeakEntity<Workspace>,
    folder: Entity<InputState>,
    open_in_browser: bool,
    focus: FocusHandle,
}

/// The folder and the check box as last used, for the next report (IntelliJ keeps them in the
/// workspace).
#[derive(Default)]
struct LastReport(Option<(String, bool)>);

impl gpui::Global for LastReport {}

/// Generate Coverage Report…, with the coverage shown.
pub(crate) fn generate_report(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(coverage) = Coverage::of(workspace, cx) else {
        return;
    };
    if !coverage.read(cx).is_shown() {
        return;
    }
    let root = coverage.read(cx).root(cx).unwrap_or_default();
    let (folder, open_in_browser) = cx
        .try_global::<LastReport>()
        .and_then(|last| last.0.clone())
        .unwrap_or_else(|| (root.join("htmlReport").to_string_lossy().into_owned(), true));
    let handle = workspace.weak_handle();
    workspace.toggle_modal(window, cx, move |window, cx| {
        let focus = cx.focus_handle();
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(folder, window, cx);
            input
        });
        window.focus(&focus, cx);
        GenerateReport {
            coverage,
            workspace: handle,
            folder: input,
            open_in_browser,
            focus,
        }
    });
}

impl GenerateReport {
    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = spork_workbench::prompt_for_paths(
            gpui_kit::PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: Some("Select Output Directory".into()),
            },
            cx,
        );
        let input = self.folder.clone();
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(path) = paths.into_iter().next()
            {
                input
                    .update_in(cx, |input, window, cx| {
                        input.set_value(path.to_string_lossy().into_owned(), window, cx)
                    })
                    .ok();
            }
        })
        .detach();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let folder = self.folder.read(cx).value().trim().to_owned();
        if folder.is_empty() {
            return;
        }
        cx.set_global(LastReport(Some((folder.clone(), self.open_in_browser))));
        let (Some(engine), Some(workspace)) = (engine(cx), self.workspace.upgrade()) else {
            return;
        };
        let suites = self.coverage.read(cx).active.clone();
        let report = workspace.update(cx, |workspace, cx| {
            engine.report(&suites, PathBuf::from(&folder), workspace, cx)
        });
        let open = self.open_in_browser;
        let weak = workspace.downgrade();
        cx.spawn(async move |_, cx| {
            let result = report.await;
            cx.update(|cx| match result {
                Ok(index) => {
                    REPORTED.with(|reported| reported.replace(Some(index.clone())));
                    if open {
                        cx.open_url(&format!("file://{}", index.to_string_lossy()));
                    }
                }
                Err(error) => {
                    if let Some(workspace) = weak.upgrade() {
                        workspace.update(cx, move |workspace, cx| workspace.show_error(error, cx));
                    }
                }
            });
        })
        .detach();
        cx.emit(DismissEvent);
    }
}

thread_local! {
    /// The last report written, for the end-to-end tests.
    static REPORTED: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

impl EventEmitter<DismissEvent> for GenerateReport {}

impl ModalView for GenerateReport {}

impl Focusable for GenerateReport {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for GenerateReport {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let names: Vec<String> = self
            .coverage
            .read(cx)
            .active
            .iter()
            .map(Suite::title)
            .collect();
        let title = format!("Generate Coverage Report for: '{}'", names.join(", "));
        spork_vcs::spork_look(cx).child(
            popup::surface(cx)
                .id("generate-coverage-report")
                .role(Role::Dialog)
                .aria_label(SharedString::from(title.clone()))
                .w(px(520.))
                .track_focus(&self.focus)
                .key_context("GenerateCoverageReport menu")
                .on_action(cx.listener(|this, _: &menu::Confirm, _, cx| this.save(cx)))
                .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
                .child(popup::title(title, cx))
                .child(
                    v_flex()
                        .px(px(16.))
                        .pb(px(14.))
                        .gap(px(8.))
                        .child(spork_workbench::controls::form_label("Output directory:"))
                        .child(
                            h_flex()
                                .gap(px(4.))
                                .child(div().flex_1().child(
                                    TextField::new(&self.folder).aria_label("Output directory:"),
                                ))
                                .child(
                                    ActionButton::new("coverage-report-browse", "nodes/folder")
                                        .tooltip("Browse…")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.browse(window, cx)
                                        })),
                                ),
                        )
                        .child(
                            labeled_check(
                                "coverage-report-open",
                                "Open generated HTML in browser",
                                self.open_in_browser,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_in_browser = !this.open_in_browser;
                                cx.notify();
                            })),
                        )
                        .child(
                            h_flex()
                                .justify_end()
                                .gap(px(8.))
                                .child(
                                    Button::new("coverage-report-cancel", "Cancel")
                                        .on_click(cx.listener(|_, _, _, cx| cx.emit(DismissEvent))),
                                )
                                .child(
                                    Button::new("coverage-report-save", "Save")
                                        .primary(true)
                                        .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                                ),
                        ),
                ),
        )
    }
}

/// The dialog showing, for the end-to-end tests: the Manage Coverage Reports list, each suite
/// with whether it is checked, and the last report written.
pub(crate) fn e2e_state(workspace: &Workspace, cx: &App) -> Value {
    let suites = workspace
        .active_modal::<ManageCoverageReports>(cx)
        .map(|chooser| {
            chooser
                .read(cx)
                .suites
                .iter()
                .map(|(suite, on)| json!({"name": suite.name, "checked": on}))
                .collect::<Vec<_>>()
        });
    let reported = REPORTED.with(|reported| reported.borrow().clone());
    json!({
        "suites": suites,
        "question": workspace.active_modal::<CodeCoverageQuestion>(cx).is_some(),
        "report": reported,
    })
}
