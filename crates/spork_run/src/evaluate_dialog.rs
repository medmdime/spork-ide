//! The Evaluate dialog (⌥F8, `XDebuggerEvaluationDialog`): "Expression:" over the field with its
//! history (50 expressions, newest first), IntelliJ's hint "Use ⇧⌘⏎ to add to Watches", "Result:"
//! over the result tree (`result = value`, its children loaded as they open, an error in red with
//! the error icon), Evaluate (the default button, ⏎) and Close. It evaluates in the Debug tool
//! window's selected frame and closes when the session ends, as IntelliJ's.

use std::collections::BTreeSet;

use editor::Editor;
use editor::ToOffset as _;
use editor::display_map::ToDisplayPoint as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Context, DismissEvent, Entity, EventEmitter,
    FocusHandle, Focusable, Global, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
    TaskExt as _, WeakEntity, Window, WindowBounds, WindowOptions, anchored, div, point, px, size,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use project::debugger::session::Session;
use spork_workbench::{ActionButton, Tokens};
use util::ResultExt as _;
use workspace::{ModalView, Workspace};

use crate::debug_window::running_state;

/// IntelliJ keeps 50 expressions (`debugger.max.recent.expressions`).
const HISTORY_SIZE: usize = 50;

/// The expressions evaluated, newest first.
// shortcut: kept while the app runs; IntelliJ keeps them in the project's workspace file
#[derive(Default)]
struct History(Vec<String>);

impl Global for History {}

fn remember(expression: &str, cx: &mut App) {
    let expression = expression.trim();
    if expression.is_empty() {
        return;
    }
    let history = &mut cx.default_global::<History>().0;
    history.retain(|old| old != expression);
    history.insert(0, expression.to_owned());
    history.truncate(HISTORY_SIZE);
}

/// The dialog's one window.
struct OpenDialog(AnyWindowHandle, WeakEntity<EvaluateDialog>);

impl Global for OpenDialog {}

/// ⌥F8: the Evaluate dialog on the active debug session.
pub(crate) fn open(workspace: &mut Workspace, _: &mut Window, cx: &mut Context<Workspace>) {
    let Some(running) = running_state(workspace, cx) else {
        return;
    };
    if let Some(handle) = cx.try_global::<OpenDialog>().map(|open| open.0)
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(None, size(px(500.), px(350.)), cx)),
        window_min_size: Some(size(px(450.), px(300.))),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some("Evaluate".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.), px(8.))),
        }),
        ..Default::default()
    };
    let running = running.downgrade();
    cx.defer(move |cx| {
        match gpui_kit::open_window(options, cx, move |window, cx| {
            crate::configurations_dialog::follow_ide_theme(window, cx);
            // opened from a key, it takes the keys that follow
            window.activate_window();
            cx.new(|cx| EvaluateDialog::new(running, window, cx))
        }) {
            Ok((handle, dialog)) => cx.set_global(OpenDialog(handle, dialog.downgrade())),
            Err(error) => log::error!("failed to open Evaluate: {error:#}"),
        }
    });
}

/// What the last Evaluate gave.
enum Outcome {
    Evaluating,
    Value { value: SharedString, reference: u64 },
    Error(SharedString),
}

/// An expression's evaluation and its result tree: the Evaluate dialog's and Quick Evaluate's.
struct Evaluation {
    session: Option<Entity<Session>>,
    outcome: Option<Outcome>,
    /// The result tree's open nodes, by their variables reference.
    expanded: BTreeSet<u64>,
    _evaluating: Task<()>,
}

impl Evaluation {
    fn new(session: Option<Entity<Session>>) -> Self {
        Self {
            session,
            outcome: None,
            expanded: BTreeSet::new(),
            _evaluating: Task::ready(()),
        }
    }

    /// Evaluates `expression` in `frame`, the view `V` keeping the evaluation where `of` finds it.
    fn start<V: 'static>(
        &mut self,
        expression: String,
        frame: Option<u64>,
        of: fn(&mut V) -> &mut Evaluation,
        cx: &mut Context<V>,
    ) {
        let Some(session) = self.session.clone().filter(|_| !expression.is_empty()) else {
            return;
        };
        remember(&expression, cx);
        let evaluation = session.update(cx, |session, cx| {
            session.evaluate_expression(expression, frame, cx)
        });
        self.outcome = Some(Outcome::Evaluating);
        self.expanded.clear();
        cx.notify();
        self._evaluating = cx.spawn(async move |this, cx| {
            let outcome = match evaluation.await {
                Ok(response) => Outcome::Value {
                    value: response.result.into(),
                    reference: response.variables_reference,
                },
                Err(error) => Outcome::Error(error.to_string().into()),
            };
            this.update(cx, |this, cx| {
                let evaluation = of(this);
                if let Outcome::Value { reference, .. } = &outcome
                    && *reference > 0
                {
                    // IntelliJ opens the result
                    evaluation.expanded.insert(*reference);
                }
                evaluation.outcome = Some(outcome);
                cx.notify();
            })
            .log_err();
        });
    }

    /// The result tree's rows: `name = value`, its children under it as their nodes open.
    fn rows<V: 'static>(
        &self,
        name: &str,
        of: fn(&mut V) -> &mut Evaluation,
        cx: &mut Context<V>,
    ) -> Vec<AnyElement> {
        let tokens = Tokens::get(cx);
        let mut rows: Vec<AnyElement> = Vec::new();
        match &self.outcome {
            None => {}
            Some(Outcome::Evaluating) => rows.push(
                div()
                    .id("evaluate-evaluating")
                    .px(px(8.))
                    .text_color(tokens.text_secondary)
                    .child("Evaluating…")
                    .into_any_element(),
            ),
            Some(Outcome::Error(error)) => rows.push(
                h_flex()
                    .id("evaluate-error")
                    .role(Role::TreeItem)
                    .aria_label(error.clone())
                    .h(px(24.))
                    .px(px(8.))
                    .gap(px(4.))
                    .child(spork_workbench::image("status/error", cx).size(px(16.)))
                    .child(div().text_color(tokens.text_error).child(error.clone()))
                    .into_any_element(),
            ),
            Some(Outcome::Value { value, reference }) => {
                let (value, reference) = (value.clone(), *reference);
                let mut nodes = vec![(
                    0usize,
                    SharedString::from(format!("{name} = {value}")),
                    reference,
                )];
                let mut ix = 0;
                // depth first: each open node's children under it
                while ix < nodes.len() {
                    let (depth, _, reference) = nodes[ix].clone();
                    if reference > 0 && self.expanded.contains(&reference) {
                        let variables = self
                            .session
                            .as_ref()
                            .map(|session| {
                                session.update(cx, |session, cx| session.variables(reference, cx))
                            })
                            .unwrap_or_default();
                        let under: Vec<_> = variables
                            .into_iter()
                            .map(|variable| {
                                (
                                    depth + 1,
                                    SharedString::from(format!(
                                        "{} = {}",
                                        variable.name, variable.value
                                    )),
                                    variable.variables_reference,
                                )
                            })
                            .collect();
                        nodes.splice(ix + 1..ix + 1, under);
                    }
                    ix += 1;
                }
                for (ix, (depth, label, reference)) in nodes.into_iter().enumerate() {
                    let open = self.expanded.contains(&reference);
                    let chevron = match (reference > 0, open) {
                        (false, _) => None,
                        (true, true) => Some("general/chevronDown"),
                        (true, false) => Some("general/chevronRight"),
                    };
                    rows.push(
                        h_flex()
                            .id(("evaluate-node", ix))
                            .role(Role::TreeItem)
                            .aria_label(label.clone())
                            .when(reference > 0, |row| row.aria_expanded(open))
                            .aria_selected(ix == 0)
                            .h(px(24.))
                            .mx(px(4.))
                            .pl(px(4. + 16. * depth as f32))
                            .gap(px(2.))
                            .rounded(px(4.))
                            .when(ix == 0, |row| row.bg(tokens.selection))
                            .child(
                                div()
                                    .size(px(16.))
                                    .flex_none()
                                    .children(chevron.map(|icon| {
                                        spork_workbench::icon(icon)
                                            .size(px(16.))
                                            .text_color(tokens.icon)
                                    })),
                            )
                            .child(div().truncate().child(label))
                            .when(reference > 0, |row| {
                                row.cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let expanded = &mut of(this).expanded;
                                        if !expanded.remove(&reference) {
                                            expanded.insert(reference);
                                        }
                                        cx.notify();
                                    }))
                            })
                            .into_any_element(),
                    );
                }
            }
        }
        rows
    }
}

struct EvaluateDialog {
    running: WeakEntity<debugger_ui::RunningState>,
    expression: Entity<InputState>,
    evaluation: Evaluation,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EvaluateDialog {
    fn new(
        running: WeakEntity<debugger_ui::RunningState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let expression = cx.new(|cx| InputState::new(window, cx));
        let mut subscriptions = vec![cx.subscribe_in(
            &expression,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { secondary, shift } = event {
                    match secondary & shift {
                        true => this.add_to_watches(window, cx),
                        false => this.evaluate(cx),
                    }
                }
            },
        )];
        let session = running
            .upgrade()
            .map(|running| running.read(cx).session().clone());
        if let Some(session) = &session {
            // the result tree's children load later; the dialog closes with the session
            subscriptions.push(cx.observe_in(session, window, |_, session, window, cx| {
                if session.read(cx).is_terminated() {
                    window.remove_window();
                }
                cx.notify();
            }));
        }
        expression.update(cx, |field, cx| field.focus(window, cx));
        Self {
            running,
            expression,
            evaluation: Evaluation::new(session),
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    fn text(&self, cx: &App) -> String {
        self.expression.read(cx).value().trim().to_owned()
    }

    fn frame(&self, cx: &App) -> Option<u64> {
        self.running.upgrade()?.read(cx).opened_stack_frame(cx)
    }

    /// Evaluate (⏎): the expression's value in the selected frame.
    fn evaluate(&mut self, cx: &mut Context<Self>) {
        let (expression, frame) = (self.text(cx), self.frame(cx));
        self.evaluation.start(
            expression,
            frame,
            |this: &mut Self| &mut this.evaluation,
            cx,
        );
    }

    /// ⇧⌘⏎: the expression becomes a watch of the Debug tool window.
    fn add_to_watches(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let expression = self.text(cx);
        let (Some(session), Some(frame)) = (self.evaluation.session.clone(), self.frame(cx)) else {
            return;
        };
        if expression.is_empty() {
            return;
        }
        remember(&expression, cx);
        session
            .update(cx, |session, cx| {
                session.add_watcher(expression.into(), frame, cx)
            })
            .detach_and_log_err(cx);
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        remember(&self.text(cx), cx);
        window.remove_window();
    }

    fn history_button(&self) -> impl IntoElement {
        let expression = self.expression.downgrade();
        ActionButton::new("evaluate-history", "general/chevronDown")
            .tooltip("History")
            .dropdown_menu(move |menu, _, cx| {
                let history = cx
                    .try_global::<History>()
                    .map(|history| history.0.clone())
                    .unwrap_or_default();
                if history.is_empty() {
                    return menu.item(PopupMenuItem::new("No history").disabled(true));
                }
                history.into_iter().fold(menu, |menu, old| {
                    let expression = expression.clone();
                    let text = old.clone();
                    menu.item(PopupMenuItem::new(old).on_click(move |_, window, cx| {
                        expression
                            .update(cx, |field, cx| {
                                field.set_value(text.clone(), window, cx);
                                field.focus(window, cx);
                            })
                            .log_err();
                    }))
                })
            })
    }

    /// The result tree: `result = value`, its children under it as their nodes open.
    fn tree(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let rows = self
            .evaluation
            .rows("result", |this: &mut Self| &mut this.evaluation, cx);
        v_flex()
            .id("evaluate-result")
            .role(Role::Tree)
            .aria_label("Result")
            .flex_1()
            .min_h_0()
            .py(px(4.))
            .border_1()
            .border_color(tokens.separator)
            .rounded(px(4.))
            .bg(tokens.island)
            .overflow_y_scroll()
            .children(rows)
            .into_any_element()
    }
}

impl Focusable for EvaluateDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for EvaluateDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let tree = self.tree(cx);
        let empty = self.text(cx).is_empty();
        v_flex()
            .id("evaluate-dialog")
            .key_context("EvaluateDialog")
            .track_focus(&self.focus)
            .size_full()
            .bg(tokens.panel)
            .text_color(tokens.text)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.close(window, cx);
                }
            }))
            .child(spork_workbench::shell::window_title("Evaluate", cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .px(px(16.))
                    .pt(px(10.))
                    .gap(px(4.))
                    .child(div().child("Expression:"))
                    // the field's own keys have no ⇧⌘⏎: it adds the watch before them
                    .child(
                        div()
                            .capture_key_down(cx.listener(
                                |this, event: &gpui::KeyDownEvent, window, cx| {
                                    let keystroke = &event.keystroke;
                                    if keystroke.key == "enter"
                                        && keystroke.modifiers.shift
                                        && keystroke.modifiers.platform
                                    {
                                        cx.stop_propagation();
                                        this.add_to_watches(window, cx);
                                    }
                                },
                            ))
                            .child(
                                spork_workbench::TextField::new(&self.expression)
                                    .aria_label("Expression")
                                    .suffix(self.history_button()),
                            ),
                    )
                    .child(
                        div()
                            .id("evaluate-watches-hint")
                            .role(Role::Label)
                            .aria_label("Use ⇧⌘⏎ to add to Watches")
                            .flex_none()
                            .pt(px(2.))
                            .pb(px(6.))
                            .text_right()
                            .text_xs()
                            .text_color(tokens.text_secondary)
                            .child("Use ⇧⌘⏎ to add to Watches"),
                    )
                    .child(div().child("Result:"))
                    .child(tree),
            )
            .child(
                h_flex()
                    .flex_none()
                    .justify_end()
                    .gap(px(8.))
                    .px(px(16.))
                    .py(px(10.))
                    .child(
                        spork_workbench::Button::new("evaluate-close", "Close")
                            .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    )
                    .child(
                        spork_workbench::Button::new("evaluate-evaluate", "Evaluate")
                            .primary(true)
                            .disabled(empty)
                            .on_click(cx.listener(|this, _, _, cx| this.evaluate(cx))),
                    ),
            )
    }
}

/// Quick Evaluate Expression (⌥⌘F8, `XQuickEvaluateHandler`): the selection, or the word at the
/// caret, evaluated in the selected frame, its value in IntelliJ's hint under it (`name = value`,
/// an error in red, a value's children as its node opens); ⎋ or a click elsewhere closes it.
pub(crate) fn quick_evaluate(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let (Some(running), Some(editor)) = (
        running_state(workspace, cx),
        workspace.active_item_as::<Editor>(cx),
    ) else {
        return;
    };
    let Some((expression, position)) = editor.update(cx, |editor, cx| {
        let buffer = editor.buffer().read(cx).snapshot(cx);
        let selection = *editor.selections.newest_anchor();
        let (start, end) = (
            selection.start.to_offset(&buffer),
            selection.end.to_offset(&buffer),
        );
        let range = match start == end {
            true => buffer.surrounding_word(start, None).0,
            false => start.min(end)..start.max(end),
        };
        let expression: String = buffer.text_for_range(range.clone()).collect();
        let snapshot = editor.snapshot(window, cx);
        let display = buffer
            .anchor_before(range.start)
            .to_display_point(&snapshot);
        let origin = editor.text_origin_below_row(display.row())?;
        let details = editor.text_layout_details(window, cx);
        let x = snapshot.x_for_display_point(display, &details);
        Some((expression.trim().to_owned(), point(origin.x + x, origin.y)))
    }) else {
        return;
    };
    if expression.is_empty() {
        return;
    }
    let session = running.read(cx).session().clone();
    let frame = running.read(cx).opened_stack_frame(cx);
    workspace.toggle_modal(window, cx, move |window, cx| {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut evaluation = Evaluation::new(Some(session));
        evaluation.start(
            expression.clone(),
            frame,
            |this: &mut QuickEvaluate| &mut this.evaluation,
            cx,
        );
        QuickEvaluate {
            name: expression,
            position,
            evaluation,
            focus,
        }
    });
}

/// Quick Evaluate's hint (`XValueHint`).
struct QuickEvaluate {
    name: String,
    position: gpui::Point<gpui::Pixels>,
    evaluation: Evaluation,
    focus: FocusHandle,
}

impl EventEmitter<DismissEvent> for QuickEvaluate {}

impl ModalView for QuickEvaluate {}

impl Focusable for QuickEvaluate {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for QuickEvaluate {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let rows = self
            .evaluation
            .rows(&self.name, |this: &mut Self| &mut this.evaluation, cx);
        anchored()
            .position(self.position + point(px(0.), px(4.)))
            .snap_to_window_with_margin(px(8.))
            .child(
                spork_vcs::spork_look(cx).child(
                    spork_workbench::popup::surface(cx)
                        .id("quick-evaluate")
                        .role(Role::Tree)
                        .aria_label("Quick Evaluate")
                        .key_context("QuickEvaluate menu")
                        .track_focus(&self.focus)
                        .occlude()
                        .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
                        .min_w(px(200.))
                        .max_w(px(600.))
                        .py(px(4.))
                        .text_color(tokens.text)
                        .children(rows),
                ),
            )
    }
}

/// What the open Evaluate dialog shows, for the end-to-end tests: its result or error.
pub(crate) fn e2e_evaluate(cx: &App) -> serde_json::Value {
    let Some(dialog) = cx
        .try_global::<OpenDialog>()
        .and_then(|open| open.1.upgrade())
    else {
        return serde_json::Value::Null;
    };
    match &dialog.read(cx).evaluation.outcome {
        None => serde_json::json!({}),
        Some(Outcome::Evaluating) => serde_json::json!({"evaluating": true}),
        Some(Outcome::Error(error)) => serde_json::json!({"error": error.as_ref()}),
        Some(Outcome::Value { value, .. }) => serde_json::json!({"result": value.as_ref()}),
    }
}
