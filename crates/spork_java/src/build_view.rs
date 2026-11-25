//! IntelliJ's Build menu over JDT LS's build (`java/buildWorkspace`, `java/buildProjects`): Build
//! Project (⌘F9), Build Module, Rebuild (⇧⌘F9) and Rebuild Project; and the Build tool window
//! (`BuildView`): the build's tree, the project's line with its status, time and duration, then the
//! files with the compiler's errors and warnings and their messages, the selected one's text on
//! the right.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use collections::HashSet;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, Task, TaskExt as _, Window, div,
    px,
};
use gpui_kit::component::{h_flex, v_flex};
use language::{Buffer, DiagnosticSeverity, Point};
use serde_json::{Value, json};
use spork_workbench::tree::{Tree, TreeItem};
use spork_workbench::{ActionButton, Tokens};
use util::ResultExt as _;
use workspace::dock::{DockPosition, Panel, PanelEvent};
use workspace::{OpenOptions, Workspace};

use crate::java_project::{JavaProject, Trigger};

pub mod build {
    gpui::actions!(
        build,
        [
            /// Compiles the project's modified files and the ones depending on them (IntelliJ's
            /// Build Project, ⌘F9).
            BuildProject,
            /// Compiles the modified files of the active file's module.
            BuildModule,
            /// Compiles every file of the active file's module again (⇧⌘F9).
            Rebuild,
            /// Compiles every file of the project again.
            RebuildProject,
            /// Compiles the active Java file again, and what depends on it (⇧⌘F9 in its editor).
            Recompile,
        ]
    );
}

pub mod build_panel {
    gpui::actions!(
        build_panel,
        [
            /// Opens the Build tool window and focuses it, or gives the focus back to the editor.
            ToggleFocus
        ]
    );
}

/// A build: Build (incremental) or Rebuild (every file), of the project or of the active file's
/// module, which is the project: JDT LS sees one module per build file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Scope {
    pub(crate) full: bool,
    pub(crate) module: bool,
    /// Recompile: the active file, marked changed for the incremental build.
    pub(crate) recompile: bool,
}

impl Scope {
    /// IntelliJ's content name (`compiler.content.name.make`, `.rebuild`, `.recompile`).
    fn name(self) -> &'static str {
        match (self.recompile, self.full) {
            (true, _) => "Recompile",
            (false, true) => "Rebuild",
            (false, false) => "Build",
        }
    }
}

/// Adds the Build tool window, once the project says it has a build tool.
pub(crate) fn add(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    if let Some(java) = JavaProject::of(workspace, cx) {
        let view = cx.new(|cx| BuildView::new(java, cx));
        workspace.add_panel(view, window, cx);
    }
}

/// Registers the Build actions on a workspace whose project has a build tool, the module ones
/// while a file of the project is active.
pub(crate) fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &build_panel::ToggleFocus, window, cx| {
        workspace.toggle_panel_focus::<BuildView>(window, cx);
    });
    workspace.register_action_renderer(|div, workspace, _, cx| {
        let Some(java) = JavaProject::of(workspace, cx) else {
            return div;
        };
        if java.read(cx).tool.is_none() {
            return div;
        }
        let in_module = active_file(workspace, cx).is_some_and(|path| {
            java.read(cx)
                .root
                .as_ref()
                .is_some_and(|root| path.starts_with(root))
        });
        let java_file = in_module
            && active_file(workspace, cx).is_some_and(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "java")
            });
        let action = |full: bool, module: bool| {
            move |workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>| {
                let scope = Scope {
                    full,
                    module,
                    recompile: false,
                };
                start(workspace, scope, window, cx)
            }
        };
        div.on_action(cx.listener({
            let run = action(false, false);
            move |workspace, _: &build::BuildProject, window, cx| run(workspace, window, cx)
        }))
        .on_action(cx.listener({
            let run = action(true, false);
            move |workspace, _: &build::RebuildProject, window, cx| run(workspace, window, cx)
        }))
        .when(in_module, |div| {
            div.on_action(cx.listener({
                let run = action(false, true);
                move |workspace, _: &build::BuildModule, window, cx| run(workspace, window, cx)
            }))
            .on_action(cx.listener({
                let run = action(true, true);
                move |workspace, _: &build::Rebuild, window, cx| run(workspace, window, cx)
            }))
        })
        .when(java_file, |div| {
            div.on_action(cx.listener(|workspace, _: &build::Recompile, window, cx| {
                let scope = Scope {
                    full: false,
                    module: true,
                    recompile: true,
                };
                start(workspace, scope, window, cx)
            }))
        })
    });
}

fn active_file(workspace: &Workspace, cx: &App) -> Option<PathBuf> {
    let item = workspace.active_item(cx)?;
    let path = item.project_path(cx)?;
    workspace.project().read(cx).absolute_path(&path, cx)
}

/// Starts a build in the Build tool window, which shows it.
pub(crate) fn start(
    workspace: &mut Workspace,
    scope: Scope,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(view) = workspace.panel::<BuildView>(cx) else {
        return;
    };
    let file = scope
        .recompile
        .then(|| active_file(workspace, cx))
        .flatten();
    view.update(cx, |view, cx| view.start(scope, file, cx));
    workspace.open_panel::<BuildView>(window, cx);
}

#[derive(Clone, Debug, PartialEq)]
struct Message {
    path: PathBuf,
    /// The file's folder from the project's root: `src/main/java/com/example`.
    folder: String,
    point: Point,
    error: bool,
    text: String,
}

#[derive(Clone, Debug, PartialEq)]
enum Outcome {
    Running,
    /// Finished: JDT LS's status said the build succeeded or found errors.
    Finished,
    /// The build couldn't run (no language server) or JDT LS said it failed.
    Failed(String),
    Cancelled,
}

struct BuildRun {
    scope: Scope,
    started: Instant,
    started_at: chrono::DateTime<chrono::Local>,
    elapsed: Option<Duration>,
    outcome: Outcome,
    messages: Vec<Message>,
    /// What the Maven goals run around the build printed (IntelliJ's 'before' and 'after'
    /// tasks).
    log: Vec<String>,
}

impl BuildRun {
    fn errors(&self) -> usize {
        self.messages.iter().filter(|message| message.error).count()
    }

    fn warnings(&self) -> usize {
        self.messages.len() - self.errors()
    }

    fn failed(&self) -> bool {
        matches!(self.outcome, Outcome::Failed(_)) || self.errors() > 0
    }

    /// The project's line: `build finished`, `rebuild failed` (BuildBundle's
    /// `build.messages.finished`, with the content name in lower case).
    fn status(&self) -> String {
        let name = self.scope.name().to_lowercase();
        match self.outcome {
            Outcome::Running => format!("{name} running…"),
            Outcome::Cancelled => format!("{name} cancelled"),
            _ if self.failed() => format!("{name} failed"),
            _ => format!("{name} finished"),
        }
    }

    /// What the console shows for the project's line: the compiler's summary
    /// (`status.compilation.completed.successfully…`).
    fn summary(&self) -> String {
        let Some(elapsed) = self.elapsed else {
            return String::new();
        };
        let took = format_duration(elapsed);
        let plural = |count: usize, one: &str, many: &str| match count {
            1 => format!("1 {one}"),
            count => format!("{count} {many}"),
        };
        match &self.outcome {
            Outcome::Failed(reason) => reason.clone(),
            Outcome::Cancelled => String::new(),
            _ if self.errors() > 0 => format!(
                "Build completed with {} and {} in {took}",
                plural(self.errors(), "error", "errors"),
                plural(self.warnings(), "warning", "warnings")
            ),
            _ if self.warnings() > 0 => format!(
                "Build completed successfully with {} in {took}",
                plural(self.warnings(), "warning", "warnings")
            ),
            _ => format!("Build completed successfully in {took}"),
        }
    }
}

/// Recompile: the file's modification time moved on and JDT LS told it changed, so its
/// incremental build compiles it again (Eclipse's builder follows the resource's timestamp).
async fn mark_changed(file: &Path, server: &lsp::LanguageServer) -> anyhow::Result<()> {
    std::fs::File::options()
        .write(true)
        .open(file)?
        .set_modified(std::time::SystemTime::now())?;
    let uri = lsp::Uri::from_file_path(file)
        .map_err(|()| anyhow::anyhow!("{file:?} isn't a file path"))?;
    server.notify::<lsp::notification::DidChangeWatchedFiles>(
        lsp::DidChangeWatchedFilesParams {
            changes: vec![lsp::FileEvent {
                uri,
                typ: lsp::FileChangeType::CHANGED,
            }],
        },
    )?;
    Ok(())
}

/// A duration as IntelliJ's build tree writes it (`StringUtil.formatDuration`): `520 ms`,
/// `1 sec, 234 ms`, `2 min, 5 sec`.
fn format_duration(elapsed: Duration) -> String {
    let ms = elapsed.as_millis();
    match ms {
        0..1000 => format!("{ms} ms"),
        1000..60_000 => format!("{} sec, {} ms", ms / 1000, ms % 1000),
        _ => format!("{} min, {} sec", ms / 60_000, (ms % 60_000) / 1000),
    }
}

/// The selected line of the tree: the project's, or a message's.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Selected {
    Root,
    Message(usize),
}

/// IntelliJ's Build tool window.
pub(crate) struct BuildView {
    java: Entity<JavaProject>,
    run: Option<BuildRun>,
    selected: Option<Selected>,
    /// The file the last Recompile compiled, for Rerun.
    recompiled: Option<PathBuf>,
    focus: FocusHandle,
    task: Task<()>,
    _subscription: gpui::Subscription,
}

impl BuildView {
    fn new(java: Entity<JavaProject>, cx: &mut Context<Self>) -> Self {
        let _subscription = cx.observe(&java, |_, _, cx| cx.notify());
        Self {
            java,
            run: None,
            selected: None,
            recompiled: None,
            focus: cx.focus_handle(),
            task: Task::ready(()),
            _subscription,
        }
    }

    fn start(&mut self, scope: Scope, recompiled: Option<PathBuf>, cx: &mut Context<Self>) {
        self.recompiled = recompiled.clone();
        let java = self.java.read(cx);
        let project = java.project.clone();
        let root = java.root.clone();
        let server = java.language_server(cx);
        let before = java.goals_on(Trigger::of(scope.full, true));
        let before = (!before.is_empty()).then(|| java.run_goals(before, cx));
        let after = java.goals_on(Trigger::of(scope.full, false));
        // IntelliJ saves every document before it compiles
        let dirty: HashSet<Entity<Buffer>> = project
            .read(cx)
            .opened_buffers(cx)
            .into_iter()
            .filter(|buffer| buffer.read(cx).is_dirty())
            .collect();
        let save = project.update(cx, |project, cx| project.save_buffers(dirty, cx));
        self.run = Some(BuildRun {
            scope,
            started: Instant::now(),
            started_at: chrono::Local::now(),
            elapsed: None,
            outcome: Outcome::Running,
            messages: Vec::new(),
            log: Vec::new(),
        });
        self.selected = Some(Selected::Root);
        cx.notify();
        self.task = cx.spawn(async move |this, cx| {
            save.await.log_err();
            if let (Some(file), Some(server)) = (&recompiled, &server) {
                mark_changed(file, server).await.log_err();
            }
            if let Some(before) = before {
                let ran = before.await;
                let failed = ran.is_err();
                this.update(cx, |this, cx| this.log_tasks("before", ran, cx))
                    .ok();
                if failed {
                    return;
                }
            }
            let outcome = match (server, &root) {
                (Some(server), Some(root)) => {
                    match crate::java_project::build(&server, root, scope).await {
                        Ok(true) | Ok(false) => Outcome::Finished,
                        Err(error) if error.to_string() == CANCELLED => Outcome::Cancelled,
                        Err(error) => Outcome::Failed(format!("{error:#}")),
                    }
                }
                _ => Outcome::Failed("The Java language server isn't running".into()),
            };
            // the compiler's markers reach the client as diagnostics just after the build
            // shortcut: a fixed wait; follow the server's diagnostics once Zed tells their end
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            let messages = match (&outcome, &root) {
                (Outcome::Finished, Some(root)) => messages(&project, root, cx).await,
                _ => Vec::new(),
            };
            let built =
                outcome == Outcome::Finished && !messages.iter().any(|message| message.error);
            if built && !after.is_empty() {
                let ran = this.update(cx, |this, cx| this.java.read(cx).run_goals(after, cx));
                if let Ok(ran) = ran {
                    let ran = ran.await;
                    this.update(cx, |this, cx| this.log_tasks("after", ran, cx))
                        .ok();
                }
            }
            this.update(cx, |this, cx| {
                if let Some(run) = &mut this.run {
                    run.elapsed = Some(run.started.elapsed());
                    run.outcome = outcome;
                    run.messages = messages;
                }
                cx.notify();
            })
            .ok();
        });
    }

    /// Logs what Maven's 'before' or 'after' tasks printed; a failed one fails the build.
    fn log_tasks(&mut self, when: &str, ran: Result<String>, cx: &mut Context<Self>) {
        let Some(run) = &mut self.run else {
            return;
        };
        run.log.push(format!("Running '{when}' tasks"));
        match ran {
            Ok(output) => run.log.extend(output.lines().map(str::to_owned)),
            Err(error) => {
                let output = format!("{error:#}");
                run.log.extend(output.lines().map(str::to_owned));
                run.elapsed = Some(run.started.elapsed());
                run.outcome = Outcome::Failed(format!("Running '{when}' tasks failed"));
            }
        }
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        // dropping the request cancels it at the server
        self.task = Task::ready(());
        if let Some(run) = &mut self.run
            && run.outcome == Outcome::Running
        {
            run.elapsed = Some(run.started.elapsed());
            run.outcome = Outcome::Cancelled;
        }
        cx.notify();
    }

    fn rerun(&mut self, cx: &mut Context<Self>) {
        if let Some(scope) = self.run.as_ref().map(|run| run.scope) {
            self.start(scope, self.recompiled.clone(), cx);
        }
    }

    fn open(&self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(message) = self.run.as_ref().and_then(|run| run.messages.get(ix)) else {
            return;
        };
        let Some(workspace) = self.java.read(cx).workspace.upgrade() else {
            return;
        };
        let (path, point) = (message.path.clone(), message.point);
        let open = workspace.update(cx, |workspace, cx| {
            workspace.open_abs_path(path, OpenOptions::default(), window, cx)
        });
        window
            .spawn(cx, async move |cx| {
                let item = open.await?;
                cx.update(|window, cx| {
                    if let Some(editor) = item.downcast::<editor::Editor>() {
                        editor.update(cx, |editor, cx| {
                            editor.go_to_singleton_buffer_point(point, window, cx)
                        });
                    }
                })
            })
            .detach_and_log_err(cx);
    }

    fn tree(&self, cx: &mut Context<Self>) -> Vec<TreeItem> {
        let Some(run) = &self.run else {
            return Vec::new();
        };
        let tokens = Tokens::get(cx);
        let this = cx.entity().downgrade();
        // the files, in the order the messages came, each with its messages
        let mut files: Vec<(&Path, &str, Vec<usize>)> = Vec::new();
        for (ix, message) in run.messages.iter().enumerate() {
            match files.iter_mut().find(|(path, _, _)| *path == message.path) {
                Some((_, _, messages)) => messages.push(ix),
                None => files.push((&message.path, &message.folder, vec![ix])),
            }
        }
        let file_items = files.into_iter().map(|(path, folder, messages)| {
            let errors = messages
                .iter()
                .filter(|ix| run.messages[**ix].error)
                .count();
            let warnings = messages.len() - errors;
            let counts = [
                (errors, "error", "errors"),
                (warnings, "warning", "warnings"),
            ]
            .into_iter()
            .filter(|(count, _, _)| *count > 0)
            .map(|(count, one, many)| match count {
                1 => format!("1 {one}"),
                count => format!("{count} {many}"),
            })
            .collect::<Vec<_>>()
            .join(", ");
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            TreeItem::new(name)
                .colored_icon("fileTypes/java")
                .hint(format!("{folder} {counts}"))
                .default_open(true)
                .children(messages.into_iter().map(|ix| {
                    let message = &run.messages[ix];
                    let first_line = message.text.lines().next().unwrap_or_default().to_owned();
                    let this = this.clone();
                    TreeItem::new(first_line)
                        .colored_icon(match message.error {
                            true => "status/error",
                            false => "status/warning",
                        })
                        .hint(format!(":{}", message.point.row + 1))
                        .active(self.selected == Some(Selected::Message(ix)))
                        .on_click(move |event, window, cx| {
                            this.update(cx, |this, cx| {
                                this.selected = Some(Selected::Message(ix));
                                if event.click_count() == 2 {
                                    this.open(ix, window, cx);
                                }
                                cx.notify();
                            })
                            .ok();
                        })
                }))
        });
        let name = self.java.read(cx).name();
        let elapsed = run.elapsed.map(format_duration);
        let at = run.started_at.format("%d/%m/%Y, %H:%M").to_string();
        let icon = match run.outcome {
            Outcome::Running => None,
            Outcome::Cancelled => Some("status/warning"),
            _ if run.failed() => Some("status/error"),
            _ => Some("status/success"),
        };
        let running = run.outcome == Outcome::Running;
        let muted = tokens.text_secondary;
        let root = TreeItem::new(format!("{name}: {}", run.status()))
            .when_some(icon, |item, icon| item.colored_icon(icon))
            .when(running, |item| {
                item.suffix(|_, _| spork_workbench::Spinner::new())
            })
            .hint(format!("at {at}"))
            .active(self.selected == Some(Selected::Root))
            .default_open(true)
            .when_some(elapsed, |item, elapsed| {
                item.suffix(move |_, _| div().text_color(muted).child(elapsed.clone()))
            })
            .on_click({
                let this = this.clone();
                move |_, _, cx| {
                    this.update(cx, |this, cx| {
                        this.selected = Some(Selected::Root);
                        cx.notify();
                    })
                    .ok();
                }
            })
            .children(file_items);
        vec![root]
    }

    /// The console's text: the selected message with its place, or the build's summary.
    fn console_text(&self) -> String {
        let Some(run) = &self.run else {
            return String::new();
        };
        match self.selected {
            Some(Selected::Message(ix)) => run
                .messages
                .get(ix)
                .map(|message| {
                    format!(
                        "{}:{}:{}\njava: {}",
                        message.path.display(),
                        message.point.row + 1,
                        message.point.column + 1,
                        message.text
                    )
                })
                .unwrap_or_default(),
            _ => {
                let mut lines = run.log.clone();
                lines.push(run.summary());
                lines.join("\n")
            }
        }
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let panel = Arc::new(cx.entity());
        let tab = self.run.as_ref().map(|_| {
            h_flex()
                .id("build-output-tab")
                .role(Role::Tab)
                .aria_label("Build Output")
                .aria_selected(true)
                .h(px(28.))
                .px(px(8.))
                .gap(px(4.))
                .rounded(px(6.))
                .border_1()
                .bg(tokens.tab_selected)
                .border_color(tokens.tab_selected_border)
                .child("Build Output")
                .child(
                    ActionButton::new("close-build-output", "general/closeSmall")
                        .tooltip("Close")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.stop(cx);
                            this.run = None;
                            this.selected = None;
                            cx.notify();
                        })),
                )
        });
        let title = h_flex()
            .gap(px(12.))
            .child(workspace::frame::tool_window_title_label("Build"))
            .children(tab)
            .into_any_element();
        workspace::frame::tool_window_header(title, Vec::new(), panel, "Build".into(), cx)
            .into_any_element()
    }

    /// The toolbar down the tree's left (`BuildView`'s restart actions): Rerun and Stop.
    // shortcut: no Compiler Properties, Pin or the warnings filter: Spork IDE has no compiler
    // settings page, one build tab and no filtered tree yet
    fn left_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self
            .run
            .as_ref()
            .is_some_and(|run| run.outcome == Outcome::Running);
        let name = self.run.as_ref().map_or("Build", |run| run.scope.name());
        v_flex()
            .id("build-left-toolbar")
            .role(Role::Toolbar)
            .aria_label("Build Toolbar")
            .flex_none()
            .w(px(32.))
            .pt(px(4.))
            .items_center()
            .gap(px(4.))
            .child(
                ActionButton::new("build-rerun", "build/build")
                    .tooltip(format!("Rerun '{name}'"))
                    .disabled(running || self.run.is_none())
                    .on_click(cx.listener(|this, _, _, cx| this.rerun(cx))),
            )
            .child(
                ActionButton::new("build-stop", "run/stop")
                    .tooltip("Stop")
                    .disabled(!running)
                    .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
            )
    }
}

impl Render for BuildView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let header = self.header(cx);
        let items = self.tree(cx);
        let console = self.console_text();
        let has_run = self.run.is_some();
        spork_vcs::spork_look(cx).size_full().child(
            v_flex()
                .size_full()
                .bg(tokens.island)
                .track_focus(&self.focus)
                .key_context("BuildToolWindow")
                .child(header)
                .when(has_run, |this| {
                    this.child(
                        h_flex()
                            .flex_1()
                            .min_h_0()
                            .items_start()
                            .child(self.left_toolbar(cx))
                            .child(
                                div()
                                    .id("build-tree")
                                    .role(Role::Tree)
                                    .aria_label("Build Messages")
                                    .w(px(480.))
                                    .flex_none()
                                    .h_full()
                                    .py(px(4.))
                                    .overflow_y_scroll()
                                    .child(Tree::new("build-tree").children(items)),
                            )
                            .child(div().w(px(1.)).h_full().bg(tokens.separator))
                            .child(
                                div()
                                    .id("build-console")
                                    .role(Role::Log)
                                    .aria_label("Build Console")
                                    .aria_value(console.clone())
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .p(px(8.))
                                    .overflow_y_scroll()
                                    .font_family(SharedString::from("JetBrains Mono"))
                                    .text_size(px(13.))
                                    .children(console.lines().map(|line| {
                                        div().whitespace_nowrap().child(line.to_owned())
                                    })),
                            ),
                    )
                }),
        )
    }
}

impl Focusable for BuildView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EventEmitter<PanelEvent> for BuildView {}

impl Panel for BuildView {
    fn persistent_name() -> &'static str {
        "Build"
    }

    fn panel_key() -> &'static str {
        "BuildToolWindow"
    }

    fn position(&self, _: &Window, _: &App) -> DockPosition {
        DockPosition::Bottom
    }

    // shortcut: the tool window stays where IntelliJ puts it, as the Run tool window does
    fn position_is_valid(&self, position: DockPosition) -> bool {
        position == DockPosition::Bottom
    }

    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}

    fn default_size(&self, _: &Window, _: &App) -> Pixels {
        px(300.)
    }

    fn icon(&self, _: &Window, _: &App) -> Option<ui::IconName> {
        Some(ui::IconName::ToolHammer)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some("Build")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(build_panel::ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        70
    }
}

/// The text JDT LS's cancelled status turns into.
pub(crate) const CANCELLED: &str = "Build cancelled";

/// The compiler's errors and warnings in the project's Java files, as JDT LS published them.
async fn messages(
    project: &Entity<project::Project>,
    root: &Path,
    cx: &mut gpui::AsyncApp,
) -> Vec<Message> {
    let paths: Vec<project::ProjectPath> = cx.update(|cx| {
        let mut paths: Vec<project::ProjectPath> = project
            .read(cx)
            .diagnostic_summaries(false, cx)
            .filter(|(path, _, summary)| {
                path.path.extension() == Some("java")
                    && summary.error_count + summary.warning_count > 0
            })
            .map(|(path, _, _)| path)
            .collect();
        paths.dedup();
        paths
    });
    let mut messages = Vec::new();
    for path in paths {
        let Some(buffer) = project
            .update(cx, |project, cx| project.open_buffer(path.clone(), cx))
            .await
            .log_err()
        else {
            continue;
        };
        cx.update(|cx| {
            let Some(abs_path) = project.read(cx).absolute_path(&path, cx) else {
                return;
            };
            let folder = abs_path
                .parent()
                .and_then(|folder| folder.strip_prefix(root).ok())
                .map(|folder| folder.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let snapshot = buffer.read(cx).snapshot();
            for entry in snapshot.diagnostics_in_range::<_, Point>(0..snapshot.len(), false) {
                let diagnostic = &entry.diagnostic;
                if !diagnostic.is_primary
                    || diagnostic.source.as_deref() != Some("Java")
                    || !matches!(
                        diagnostic.severity,
                        DiagnosticSeverity::ERROR | DiagnosticSeverity::WARNING
                    )
                {
                    continue;
                }
                messages.push(Message {
                    path: abs_path.clone(),
                    folder: folder.clone(),
                    point: entry.range.start,
                    error: diagnostic.severity == DiagnosticSeverity::ERROR,
                    text: diagnostic.message.to_string(),
                });
            }
        });
    }
    messages
}

/// What the end-to-end tests read of the Build tool window: the project's line, the files'
/// messages (`Main.java:6 error: …`) and the console's text.
pub(crate) fn e2e_state(workspace: &Workspace, cx: &App) -> Value {
    let Some(view) = workspace.panel::<BuildView>(cx) else {
        return Value::Null;
    };
    let view = view.read(cx);
    let Some(run) = &view.run else {
        return Value::Null;
    };
    json!({
        "status": run.status(),
        "running": run.outcome == Outcome::Running,
        "errors": run.errors(),
        "warnings": run.warnings(),
        "messages": run.messages.iter().map(|message| format!(
            "{}:{} {}: {}",
            message.path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default(),
            message.point.row + 1,
            if message.error { "error" } else { "warning" },
            message.text,
        )).collect::<Vec<_>>(),
        "console": view.console_text(),
        // the Maven tasks' lines that tell what ran and how it ended
        "log": run.log.iter().filter(|line| line.starts_with("Running '") || line.contains("BUILD ")).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::format_duration;
    use std::time::Duration;

    #[test]
    fn writes_a_duration_as_intellij_does() {
        assert_eq!(format_duration(Duration::from_millis(520)), "520 ms");
        assert_eq!(
            format_duration(Duration::from_millis(1234)),
            "1 sec, 234 ms"
        );
        assert_eq!(format_duration(Duration::from_secs(125)), "2 min, 5 sec");
    }
}
