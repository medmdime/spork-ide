//! Smart Step Into's in-place chooser (IntelliJ's `debugger.smart.step.inplace`, on by default):
//! the line's calls highlighted in the editor (`DEBUGGER_SMART_STEP_INTO_TARGET`), the chosen one
//! in the selection's colours; ←, →, ⇥ and ⇧⇥ choose, ⏎ steps into it and ⎋ leaves, as the hint
//! under the line says.

use std::ops::Range;

use dap::StepInTarget;
use editor::display_map::HighlightKey;
use editor::{Anchor, Editor};
use gpui::{
    App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, HighlightStyle,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Point, Render, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, anchored, div,
    point, px,
};
use project::debugger::session::{Session, ThreadId};
use spork_workbench::Tokens;
use theme::ActiveTheme as _;
use workspace::{ModalView, Workspace};

/// `message.smart.step`
const HINT: &str = "Choose a method to step into with arrow keys.\nTo exit the \u{2018}smart step into\u{2019} mode, press Esc.";

pub(crate) struct SmartStepChooser {
    editor: WeakEntity<Editor>,
    session: Entity<Session>,
    thread: ThreadId,
    /// Each call's target id, label and range in the editor.
    targets: Vec<(u64, SharedString, Range<Anchor>)>,
    selected: usize,
    position: Point<gpui::Pixels>,
    focus: FocusHandle,
}

impl SmartStepChooser {
    /// Chooses among `targets` in the workspace's editor showing the frame; false when the editor
    /// isn't the frame's or a target has no place on its line (the popup chooses then).
    pub(crate) fn show(
        workspace: &mut Workspace,
        session: Entity<Session>,
        thread: ThreadId,
        targets: &[StepInTarget],
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> bool {
        let Some(editor) = workspace.active_item_as::<Editor>(cx) else {
            return false;
        };
        let (ranges, row) = {
            let editor = editor.read(cx);
            let buffer = editor.buffer().read(cx);
            if !buffer.is_singleton() {
                return false;
            }
            let snapshot = buffer.snapshot(cx);
            let ranges: Option<Vec<_>> = targets
                .iter()
                .map(|target| {
                    // the adapter counts lines and columns from 1
                    let start =
                        text::Point::new(target.line? as u32 - 1, target.column? as u32 - 1);
                    let end = text::Point::new(
                        target.end_line.or(target.line)? as u32 - 1,
                        target.end_column? as u32 - 1,
                    );
                    Some((
                        target.id,
                        SharedString::from(target.label.clone()),
                        snapshot.anchor_before(start)..snapshot.anchor_after(end),
                        start.row,
                    ))
                })
                .collect();
            let Some(ranges) = ranges else {
                return false;
            };
            let row = ranges
                .first()
                .map(|(_, _, _, row)| *row)
                .unwrap_or_default();
            (
                ranges
                    .into_iter()
                    .map(|(id, label, range, _)| (id, label, range))
                    .collect::<Vec<_>>(),
                row,
            )
        };
        let position = editor.update(cx, |editor, cx| {
            let snapshot = editor.snapshot(window, cx);
            let row = editor::display_map::ToDisplayPoint::to_display_point(
                &text::Point::new(row, 0),
                &snapshot.display_snapshot,
            )
            .row();
            editor.text_origin_below_row(row)
        });
        let Some(position) = position else {
            return false;
        };
        let editor = editor.downgrade();
        workspace.toggle_modal(window, cx, |_, cx| {
            let chooser = Self {
                editor,
                session,
                thread,
                targets: ranges,
                selected: 0,
                position,
                focus: cx.focus_handle(),
            };
            chooser.highlight(cx);
            chooser
        });
        true
    }

    /// The calls in the target colour, the chosen one in the selection's.
    fn highlight(&self, cx: &mut App) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let syntax = cx.theme().syntax().clone();
        let style = |name: &str, fallback: gpui::Hsla| {
            syntax.style_for_name(name).unwrap_or(HighlightStyle {
                background_color: Some(fallback),
                ..HighlightStyle::default()
            })
        };
        let colors = cx.theme().colors();
        let target = style(
            "debugger.smart_step_into.target",
            colors.editor_document_highlight_read_background,
        );
        let selection = style(
            "debugger.smart_step_into.selection",
            colors.editor_document_highlight_write_background,
        );
        let (chosen, others): (Vec<_>, Vec<_>) = self
            .targets
            .iter()
            .enumerate()
            .map(|(ix, (_, _, range))| (ix, range.clone()))
            .partition(|(ix, _)| *ix == self.selected);
        editor.update(cx, |editor, cx| {
            editor.highlight_text(
                HighlightKey::DebuggerSmartStepInto(0),
                others.into_iter().map(|(_, range)| range).collect(),
                target,
                cx,
            );
            editor.highlight_text(
                HighlightKey::DebuggerSmartStepInto(1),
                chosen.into_iter().map(|(_, range)| range).collect(),
                selection,
                cx,
            );
        });
    }

    fn clear(&self, cx: &mut App) {
        if let Some(editor) = self.editor.upgrade() {
            editor.update(cx, |editor, cx| {
                editor.clear_highlights(HighlightKey::DebuggerSmartStepInto(0), cx);
                editor.clear_highlights(HighlightKey::DebuggerSmartStepInto(1), cx);
            });
        }
    }

    fn choose(&mut self, by: isize, cx: &mut Context<Self>) {
        let count = self.targets.len() as isize;
        self.selected = (self.selected as isize + by).rem_euclid(count.max(1)) as usize;
        self.highlight(cx);
        cx.notify();
    }

    fn step(&mut self, cx: &mut Context<Self>) {
        if let Some((id, _, _)) = self.targets.get(self.selected) {
            let (id, thread) = (*id, self.thread);
            self.session
                .update(cx, |session, cx| session.step_in_target(thread, id, cx));
        }
        self.dismiss(cx);
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.clear(cx);
        cx.emit(DismissEvent);
    }

    /// What the end-to-end tests read: the calls' labels and the chosen one.
    pub(crate) fn e2e_state(&self) -> serde_json::Value {
        serde_json::json!({
            "targets": self.targets.iter().map(|(_, label, _)| label.to_string()).collect::<Vec<_>>(),
            "selected": self.targets.get(self.selected).map(|(_, label, _)| label.to_string()),
        })
    }
}

impl EventEmitter<DismissEvent> for SmartStepChooser {}

impl ModalView for SmartStepChooser {
    fn on_before_dismiss(
        &mut self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> workspace::DismissDecision {
        self.clear(cx);
        workspace::DismissDecision::Dismiss(true)
    }
}

impl Focusable for SmartStepChooser {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SmartStepChooser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        anchored()
            .position(self.position + point(px(0.), px(4.)))
            .snap_to_window_with_margin(px(8.))
            .child(
                spork_vcs::spork_look(cx).child(
                    spork_workbench::popup::surface(cx)
                        .id("smart-step-into")
                        .role(Role::Dialog)
                        .aria_label("Smart Step Into")
                        .key_context("SmartStepInto")
                        .track_focus(&self.focus)
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                            let back = event.keystroke.modifiers.shift;
                            match event.keystroke.key.as_str() {
                                "left" | "up" => this.choose(-1, cx),
                                "right" | "down" => this.choose(1, cx),
                                "tab" => this.choose(if back { -1 } else { 1 }, cx),
                                "enter" => this.step(cx),
                                "escape" => this.dismiss(cx),
                                _ => return,
                            }
                            cx.stop_propagation();
                        }))
                        .px(px(8.))
                        .py(px(6.))
                        .text_color(tokens.text)
                        .children(HINT.lines().map(|line| div().child(line))),
                ),
            )
    }
}
