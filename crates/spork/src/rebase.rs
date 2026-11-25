//! "Rebasing Commits", the reference app's interactive rebase dialog (Interactively Rebase from Here…):
//! the commits from the picked one up, newest first as the log lists them, each picked, stopped
//! at to edit, reworded, squashed or fixed up into the one under it, or dropped, and moved up or
//! down (GitBundle `rebase.interactive.dialog.*`). Git does the rewrite.

use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_git::{Commit, TodoAction};
use spork_workbench::tokens::metrics;
use spork_workbench::{ActionButton, Button, Tokens, Typography as _};

use crate::RepoView;

/// The dialog's rows as it opened and as they are now, and what it has selected.
pub struct RebasePlan {
    /// Commits after this revision are rewritten.
    pub base: String,
    /// Oldest first, as git replays them; the dialog lists them newest first.
    pub rows: Vec<(Commit, TodoAction)>,
    initial: Vec<(Commit, TodoAction)>,
    /// The reworded commits' new messages, by id.
    messages: Vec<(String, String)>,
    /// The selected rows, as indices into `rows`.
    selected: Vec<usize>,
    /// The selected commit's id, full message and changed files, for the pane beside the table.
    details: Option<Details>,
    /// The commit being reworded and its message editor.
    rewording: Option<(String, Entity<TextareaState>, Subscription)>,
    focus: FocusHandle,
}

/// A commit as the details pane shows it.
struct Details {
    id: String,
    message: String,
    /// Status letter and path of each changed file.
    files: Vec<(char, String)>,
}

impl RebasePlan {
    fn modified(&self) -> bool {
        !self.messages.is_empty()
            || self.rows.len() != self.initial.len()
            || self
                .rows
                .iter()
                .zip(&self.initial)
                .any(|((a, x), (b, y))| a.id != b.id || x != y)
    }

    fn message(&self, id: &str) -> Option<&str> {
        self.messages
            .iter()
            .find(|(m, _)| m == id)
            .map(|(_, message)| message.as_str())
    }

    /// The selection, oldest row first.
    fn sorted(&self) -> Vec<usize> {
        let mut selected = self.selected.clone();
        selected.sort_unstable();
        selected
    }

    fn can_reword(&self) -> bool {
        matches!(self.selected.as_slice(), [ix] if self.rows.get(*ix).is_some_and(|(_, a)| *a != TodoAction::Drop))
    }

    /// Squash and Fixup: one row onto the one under it, or several onto the oldest of them.
    fn can_unite(&self) -> bool {
        match self.selected.as_slice() {
            [] => false,
            [ix] => *ix > 0,
            _ => true,
        }
    }

    fn can_move(&self, up: bool) -> bool {
        let sorted = self.sorted();
        match (sorted.first(), sorted.last()) {
            (Some(_), Some(last)) if up => last + 1 < self.rows.len(),
            (Some(first), Some(_)) => *first > 0,
            _ => false,
        }
    }

    fn set(&mut self, action: TodoAction) {
        for &ix in &self.selected {
            if let Some((commit, row)) = self.rows.get_mut(ix) {
                *row = action;
                // Pick undoes a reword too
                if action == TodoAction::Pick {
                    self.messages.retain(|(id, _)| *id != commit.id);
                }
            }
        }
    }

    /// The selected rows squashed (or fixed up) into the oldest of them, moved right above it.
    fn unite(&mut self, action: TodoAction) {
        let sorted = self.sorted();
        let Some((&root, children)) = sorted.split_first() else {
            return;
        };
        if children.is_empty() {
            if let Some((_, row)) = self.rows.get_mut(root) {
                *row = action;
            }
            return;
        }
        let moved: Vec<(Commit, TodoAction)> = children
            .iter()
            .rev()
            .map(|&ix| self.rows.remove(ix))
            .collect();
        for (offset, (commit, _)) in moved.into_iter().rev().enumerate() {
            self.rows.insert(root + 1 + offset, (commit, action));
        }
        self.selected = (root..=root + children.len()).collect();
    }

    fn shift(&mut self, up: bool) {
        if !self.can_move(up) {
            return;
        }
        let mut sorted = self.sorted();
        if up {
            sorted.reverse();
        }
        for ix in &mut sorted {
            let to = if up { *ix + 1 } else { *ix - 1 };
            self.rows.swap(*ix, to);
            *ix = to;
        }
        self.selected = sorted;
    }

    fn reset(&mut self) {
        self.rows = self.initial.clone();
        self.messages.clear();
        self.rewording = None;
        self.selected.retain(|ix| *ix < self.rows.len());
    }

    /// The selected commits' subjects, newest first.
    pub(crate) fn selected_subjects(&self) -> Vec<String> {
        let mut sorted = self.sorted();
        sorted.reverse();
        sorted
            .iter()
            .filter_map(|ix| self.rows.get(*ix))
            .map(|(c, _)| c.subject.clone())
            .collect()
    }

    /// The todo list as git would read it: `pick <subject>`, a reworded commit `reword`.
    pub(crate) fn lines(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|(commit, action)| {
                let reworded = self.message(&commit.id).is_some();
                format!("{} {}", verb(*action, reworded), commit.subject)
            })
            .collect()
    }
}

/// The action's name (GitBundle `rebase.entry.action.name.*`), lowercase as git writes it.
fn verb(action: TodoAction, reworded: bool) -> &'static str {
    match action {
        TodoAction::Pick if reworded => "reword",
        TodoAction::Pick => "pick",
        TodoAction::Edit => "edit",
        TodoAction::Squash => "squash",
        TodoAction::Fixup => "fixup",
        TodoAction::Drop => "drop",
    }
}

/// The dialog's title (GitBundle `rebase.interactive.dialog.title`).
const TITLE: &str = "Rebasing Commits";

impl RepoView {
    /// Loads the commits from `id` up and opens "Rebasing Commits" on them.
    pub(crate) fn start_interactive_rebase(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let base = format!("{id}^");
        let range_base = base.clone();
        self.run_op(
            window,
            cx,
            move |root| spork_git::rebase_range(root, &range_base),
            move |this, commits, window, cx| {
                let rows: Vec<(Commit, TodoAction)> =
                    commits.into_iter().map(|c| (c, TodoAction::Pick)).collect();
                let focus = cx.focus_handle();
                this.rebase_plan = Some(RebasePlan {
                    base,
                    initial: rows.clone(),
                    // the newest commit selected, at the table's top
                    selected: rows.len().checked_sub(1).into_iter().collect(),
                    rows,
                    messages: Vec::new(),
                    details: None,
                    rewording: None,
                    focus: focus.clone(),
                });
                this.log_rebase_load_details();
                let view = cx.entity();
                window.open_dialog(cx, move |dialog, window, cx| {
                    let (content, footer) =
                        view.update(cx, |this, cx| this.log_rebase_dialog(window, cx));
                    let (confirm, close) = (view.clone(), view.clone());
                    dialog
                        .title(TITLE)
                        .w(px(960.))
                        .child(content)
                        .footer(footer)
                        // ⏎ starts rebasing; ⌘⏎ in the reword editor keeps the message
                        .on_ok(move |_, window, cx| {
                            confirm.update(cx, |this, cx| {
                                match this
                                    .rebase_plan
                                    .as_ref()
                                    .is_some_and(|p| p.rewording.is_some())
                                {
                                    true => this.log_rebase_apply_reword(window, cx),
                                    false => this.log_rebase_start(window, cx),
                                }
                            });
                            false
                        })
                        // Escape in the reword editor leaves it, the message as it was
                        .on_cancel({
                            let view = view.clone();
                            move |_, window, cx| {
                                view.update(cx, |this, cx| {
                                    let Some(plan) = this.rebase_plan.as_mut() else {
                                        return true;
                                    };
                                    if plan.rewording.take().is_none() {
                                        return true;
                                    }
                                    window.focus(&plan.focus, cx);
                                    cx.notify();
                                    false
                                })
                            }
                        })
                        .on_close(move |_, _, cx| {
                            close.update(cx, |this, _| this.rebase_plan = None)
                        })
                });
                window.focus(&focus, cx);
            },
        );
    }

    /// Changes the plan; the details pane follows the selection.
    fn log_rebase_update(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut RebasePlan)) {
        if let Some(plan) = self.rebase_plan.as_mut() {
            f(plan);
            self.log_rebase_load_details();
            cx.notify();
        }
    }

    /// The single selected commit's full message and changed files, for the details pane.
    fn log_rebase_load_details(&mut self) {
        let root = match &self.repo {
            Some(Ok(repo)) => repo.root.clone(),
            _ => return,
        };
        let Some(plan) = self.rebase_plan.as_mut() else {
            return;
        };
        let id = match plan.selected.as_slice() {
            [ix] => plan.rows.get(*ix).map(|(c, _)| c.id.clone()),
            _ => None,
        };
        if plan.details.as_ref().map(|d| &d.id) == id.as_ref() {
            return;
        }
        plan.details = id.map(|id| {
            let message = spork_git::commit_message(&root, &id).unwrap_or_default();
            let files = spork_git::commit_files(&root, &id).unwrap_or_default();
            Details { id, message, files }
        });
    }

    fn log_rebase_select(&mut self, ix: usize, modifiers: Modifiers, cx: &mut Context<Self>) {
        self.log_rebase_update(cx, |plan| {
            let anchor = plan.selected.last().copied();
            match (modifiers.platform, modifiers.shift, anchor) {
                (true, _, _) if plan.selected.contains(&ix) => plan.selected.retain(|s| *s != ix),
                (true, _, _) => plan.selected.push(ix),
                (_, true, Some(anchor)) => {
                    plan.selected = (anchor.min(ix)..=anchor.max(ix)).collect();
                }
                _ => plan.selected = vec![ix],
            }
        });
    }

    /// Reword: the selected commit's message in an editor in place of its row; ⌘⏎ keeps it.
    fn log_rebase_reword(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(plan) = &self.rebase_plan else {
            return;
        };
        if !plan.can_reword() {
            return;
        }
        let Some((commit, _)) = plan.selected.first().and_then(|ix| plan.rows.get(*ix)) else {
            return;
        };
        let id = commit.id.clone();
        let message = match (plan.message(&id), &self.repo) {
            (Some(message), _) => message.to_owned(),
            (None, Some(Ok(repo))) => {
                spork_git::commit_message(&repo.root, &id).unwrap_or_default()
            }
            _ => commit.subject.clone(),
        };
        let input = cx.new(|cx| TextareaState::new(window, cx));
        input.update(cx, |s, cx| {
            s.set_value(message, window, cx);
            s.focus(window, cx)
        });
        // ⌘⏎ keeps the message (GitBundle rebase.interactive.dialog.reword.hint.text)
        let subscription = cx.subscribe_in(&input, window, |this, _, event, window, cx| {
            if let InputEvent::PressEnter {
                secondary: true, ..
            } = event
            {
                this.log_rebase_apply_reword(window, cx)
            }
        });
        self.log_rebase_update(cx, |plan| plan.rewording = Some((id, input, subscription)));
    }

    /// ⌘⏎ in the reword editor: the commit gets the message (unless it is the one it has).
    fn log_rebase_apply_reword(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, input, _)) = self.rebase_plan.as_mut().and_then(|p| p.rewording.take())
        else {
            return;
        };
        let message = input.read(cx).value().trim_end().to_owned();
        let original = match &self.repo {
            Some(Ok(repo)) => spork_git::commit_message(&repo.root, &id),
            _ => None,
        };
        let Some(plan) = self.rebase_plan.as_mut() else {
            return;
        };
        plan.messages.retain(|(m, _)| *m != id);
        if !message.trim().is_empty() && original.as_deref() != Some(message.as_str()) {
            plan.messages.push((id, message));
        }
        window.focus(&plan.focus, cx);
        cx.notify();
    }

    fn log_rebase_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        // the reword editor in the table has the keys
        let rewording = self
            .rebase_plan
            .as_ref()
            .is_some_and(|p| p.rewording.is_some());
        if modifiers.platform || modifiers.control || rewording {
            return;
        }
        let plain = !modifiers.alt && !modifiers.shift;
        match (keystroke.key.as_str(), modifiers.alt) {
            ("up", true) => self.log_rebase_update(cx, |plan| plan.shift(true)),
            ("down", true) => self.log_rebase_update(cx, |plan| plan.shift(false)),
            ("up" | "down", false) => {
                let Some(plan) = &self.rebase_plan else {
                    return;
                };
                // the table lists the newest first: up is a newer row
                let next = match (plan.selected.last(), keystroke.key.as_str()) {
                    (Some(ix), "up") if ix + 1 < plan.rows.len() => ix + 1,
                    (Some(ix), "down") if *ix > 0 => ix - 1,
                    (None, _) if !plan.rows.is_empty() => plan.rows.len() - 1,
                    _ => return,
                };
                self.log_rebase_select(next, Modifiers::default(), cx)
            }
            ("p", false) if plain => self.log_rebase_update(cx, |plan| plan.set(TodoAction::Pick)),
            ("e", false) if plain => self.log_rebase_update(cx, |plan| plan.set(TodoAction::Edit)),
            ("r" | "f2", false) if plain => self.log_rebase_reword(window, cx),
            ("f6", false) if modifiers.shift => self.log_rebase_reword(window, cx),
            ("s", false) if plain => self.log_rebase_unite(TodoAction::Squash, cx),
            ("f", false) if plain => self.log_rebase_unite(TodoAction::Fixup, cx),
            ("d" | "delete" | "backspace", false) if plain => {
                self.log_rebase_update(cx, |plan| plan.set(TodoAction::Drop))
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn log_rebase_unite(&mut self, action: TodoAction, cx: &mut Context<Self>) {
        self.log_rebase_update(cx, |plan| {
            if plan.can_unite() {
                plan.unite(action)
            }
        });
    }

    /// Start Rebasing: the plan handed to git, the reworded messages with it.
    fn log_rebase_start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(plan) = self.rebase_plan.take() else {
            return;
        };
        window.close_dialog(cx);
        let todo: Vec<_> = plan.rows.iter().map(|(c, a)| (c.id.clone(), *a)).collect();
        let (base, messages) = (plan.base, plan.messages);
        // GitBundle rebase.notification.editing.title and
        // in.memory.rebase.log.interactive.action.notification.successful
        let done = match todo.iter().any(|(_, a)| *a == TodoAction::Edit) {
            true => "Rebase stopped for editing",
            false => "Rebase successful",
        };
        self.run_outcome(window, cx, done.into(), move |root| {
            spork_git::interactive_rebase_reworded(root, &base, &todo, &messages)
        });
    }

    /// The dialog's body (its toolbar, the commits, the selected one's details) and its footer
    /// (Reset, Cancel, Start Rebasing).
    fn log_rebase_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Stateful<Div>, Div) {
        let Some(plan) = &self.rebase_plan else {
            return (div().id("rebasing-commits"), div());
        };
        let tokens = Tokens::get(cx);
        let view = cx.entity();
        let any = !plan.selected.is_empty();
        let separator = || {
            div()
                .w(px(1.))
                .h(px(16.))
                .mx(px(3.))
                .bg(spork_workbench::shell::toolbar_border(cx))
        };
        let icon = |id: &'static str, name: &str, tip: &'static str| {
            ActionButton::new(id, name.to_owned()).tooltip(tip)
        };
        let fixup = move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
            let view = view.clone();
            menu.item(PopupMenuItem::new("Fixup").on_click(move |_, _, cx| {
                view.update(cx, |this, cx| this.log_rebase_unite(TodoAction::Fixup, cx))
            }))
        };
        let toolbar =
            h_flex()
                .id("rebase-toolbar")
                .role(Role::Toolbar)
                .aria_label("Rebasing Commits Toolbar")
                .h(px(32.))
                .gap_0p5()
                .child(
                    icon("rebase-up", "general/moveUp", "Move Up (⌥↑)")
                        .disabled(!plan.can_move(true))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.log_rebase_update(cx, |p| p.shift(true))
                        })),
                )
                .child(
                    icon("rebase-down", "general/moveDown", "Move Down (⌥↓)")
                        .disabled(!plan.can_move(false))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.log_rebase_update(cx, |p| p.shift(false))
                        })),
                )
                .child(separator())
                .child(
                    icon("rebase-pick", "general/undo", "Pick (P)")
                        .disabled(!any)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.log_rebase_update(cx, |p| p.set(TodoAction::Pick))
                        })),
                )
                .child(
                    icon("rebase-edit", "run/pause", "Edit (E)")
                        .disabled(!any)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.log_rebase_update(cx, |p| p.set(TodoAction::Edit))
                        })),
                )
                .child(separator())
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("rebase-reword", "Reword")
                                .tooltip("Reword (R)")
                                .disabled(!plan.can_reword())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.log_rebase_reword(window, cx)
                                })),
                        )
                        .child(
                            h_flex()
                                .child(
                                    Button::new("rebase-squash", "Squash")
                                        .tooltip("Squash (S)")
                                        .disabled(!plan.can_unite())
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.log_rebase_unite(TodoAction::Squash, cx)
                                        })),
                                )
                                .child(
                                    icon("rebase-unite-more", "general/chevronDown", "Fixup (F)")
                                        .disabled(!plan.can_unite())
                                        .dropdown_menu(fixup),
                                ),
                        )
                        .child(
                            Button::new("rebase-drop", "Drop")
                                .tooltip("Drop (D)")
                                .disabled(!any)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.log_rebase_update(cx, |p| p.set(TodoAction::Drop))
                                })),
                        ),
                );
        let rows: Vec<AnyElement> = (0..plan.rows.len())
            .rev()
            .map(|ix| self.log_rebase_row(ix, window, cx))
            .collect();
        let Some(plan) = &self.rebase_plan else {
            return (div().id("rebasing-commits"), div());
        };
        let table = v_flex()
            .id("rebase-commits")
            .role(Role::Table)
            .aria_label("Rebasing Commits Table")
            .track_focus(&plan.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.log_rebase_key(event, window, cx)
            }))
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .border_1()
            .border_color(tokens.separator)
            .children(rows);
        let details = self.log_rebase_details(cx);
        let body = v_flex()
            .id("rebasing-commits")
            .role(Role::Group)
            .aria_label(TITLE)
            .gap_1()
            .child(toolbar)
            .child(h_flex().h(px(380.)).gap_2().child(table).child(details));
        let modified = self.rebase_plan.as_ref().is_some_and(RebasePlan::modified);
        let reset = spork_workbench::controls::link("rebase-reset", "Reset", cx)
            .when(!modified, |link| {
                link.text_color(tokens.text_disabled).cursor_default()
            })
            .when(modified, |link| {
                link.on_click(cx.listener(|this, _, _, cx| {
                    this.log_rebase_update(cx, RebasePlan::reset);
                }))
            });
        let reset = spork_workbench::controls::A11yDisabled::new(reset, !modified);
        let footer = h_flex()
            .w_full()
            .gap_2()
            .child(reset)
            .child(div().flex_1())
            .child(Button::new("rebase-cancel", "Cancel").on_click(cx.listener(
                |this, _, window, cx| {
                    this.rebase_plan = None;
                    window.close_dialog(cx)
                },
            )))
            .child(
                Button::new("rebase-start", "Start Rebasing")
                    .primary(true)
                    .on_click(cx.listener(|this, _, window, cx| this.log_rebase_start(window, cx))),
            );
        (body, footer)
    }

    /// One commit of the table: its node (indented when melded into the commit under it), its
    /// subject (struck out when dropped, the new one when reworded) or the reword editor.
    fn log_rebase_row(&self, ix: usize, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(plan) = &self.rebase_plan else {
            return div().into_any_element();
        };
        let Some((commit, action)) = plan.rows.get(ix) else {
            return div().into_any_element();
        };
        let tokens = Tokens::get(cx);
        let mono = cx.theme().mono_font_family.clone();
        if let Some((_, input, _)) = plan.rewording.as_ref().filter(|(id, ..)| *id == commit.id) {
            // GitBundle rebase.interactive.dialog.reword.hint.text
            return v_flex()
                .id(("rebase-reword", ix))
                .p_1()
                .gap_1()
                .child(Textarea::new(input).h(px(96.)))
                .child(
                    div()
                        .text_ui_small()
                        .text_color(tokens.text_muted)
                        .child("Press ⌘⏎ to apply changes"),
                )
                .into_any_element();
        }
        let reworded = plan.message(&commit.id);
        let subject = reworded
            .and_then(|m| m.lines().next())
            .unwrap_or(&commit.subject)
            .to_owned();
        let selected = plan.selected.contains(&ix);
        let focused = plan.focus.contains_focused(window, cx);
        let melded = matches!(action, TodoAction::Squash | TodoAction::Fixup);
        let dropped = *action == TodoAction::Drop;
        let view = cx.entity();
        let menu_view = view.clone();
        h_flex()
            .id(("rebase-row", ix))
            .role(Role::Row)
            .aria_label(subject.clone())
            .aria_selected(selected)
            .h(px(metrics::ROW_HEIGHT))
            .px_2()
            .gap_2()
            .when(selected, |row| {
                row.bg(match focused {
                    true => tokens.selection,
                    false => tokens.selection_inactive,
                })
            })
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                let modifiers = event.modifiers;
                view.update(cx, |this, cx| {
                    this.log_rebase_select(ix, modifiers, cx);
                    if let Some(plan) = &this.rebase_plan {
                        window.focus(&plan.focus, cx);
                    }
                })
            })
            .context_menu(move |menu, _, cx| rebase_menu(menu, &menu_view, cx))
            .child(
                div()
                    .flex_none()
                    .w(px(28.))
                    .when(melded, |cell| cell.pl(px(12.)))
                    .child(div().size(px(8.)).rounded_full().bg(match dropped {
                        true => tokens.text_disabled,
                        false => tokens.icon,
                    })),
            )
            .when(*action == TodoAction::Edit, |row| {
                row.child(spork_workbench::icon("run/pause").size(px(14.)))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(dropped, |d| {
                        d.line_through().text_color(tokens.text_disabled)
                    })
                    .child(subject),
            )
            .child(
                div()
                    .flex_none()
                    .font_family(mono)
                    .text_color(tokens.text_muted)
                    .child(commit.id.chars().take(8).collect::<String>()),
            )
            .into_any_element()
    }

    /// Beside the table: the selected commit's changed files, then its message, author and
    /// hash.
    fn log_rebase_details(&self, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let pane = v_flex()
            .w(px(340.))
            .flex_none()
            .h_full()
            .p_2()
            .gap_2()
            .border_1()
            .border_color(tokens.separator);
        let Some(plan) = &self.rebase_plan else {
            return pane;
        };
        let commit = match plan.selected.as_slice() {
            [ix] => plan.rows.get(*ix).map(|(c, _)| c),
            _ => None,
        };
        let (Some(commit), Some((_, message, files))) = (
            commit,
            plan.details.as_ref().map(|d| (&d.id, &d.message, &d.files)),
        ) else {
            let text = match plan.selected.len() {
                0 => "No commits selected".to_owned(),
                n => format!("{n} commits selected"),
            };
            return pane
                .items_center()
                .justify_center()
                .text_color(tokens.text_muted)
                .child(text);
        };
        let files = files.iter().enumerate().map(|(ix, (_, path))| {
            let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
            h_flex()
                .id(("rebase-file", ix))
                .role(Role::TreeItem)
                .aria_label(name.to_owned())
                .h(px(metrics::ROW_HEIGHT))
                .gap_1p5()
                .child(spork_workbench::image(spork_workbench::file_icon(name), cx))
                .child(div().flex_none().child(name.to_owned()))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(tokens.text_muted)
                        .child(dir.to_owned()),
                )
        });
        pane.child(
            v_flex()
                .id("rebase-changes")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .children(files),
        )
        .child(
            v_flex()
                .pt_2()
                .gap_2()
                .border_t_1()
                .border_color(tokens.separator)
                .child(div().whitespace_normal().child(message.clone()))
                .child(
                    div()
                        .text_color(tokens.text_muted)
                        .child(format!("{} <{}>", commit.author, commit.email)),
                )
                .child(
                    div()
                        .text_color(tokens.text_muted)
                        .child(commit.id.chars().take(8).collect::<String>()),
                ),
        )
    }
}

/// The table's right-click menu: the toolbar's actions, then View Git Commands; Spork's AI plan
/// suggestion last.
fn rebase_menu(menu: PopupMenu, view: &Entity<RepoView>, cx: &mut Context<PopupMenu>) -> PopupMenu {
    let Some((any, reword, unite)) = view
        .read(cx)
        .rebase_plan
        .as_ref()
        .map(|p| (!p.selected.is_empty(), p.can_reword(), p.can_unite()))
    else {
        return menu;
    };
    let set = |label: &'static str, action: TodoAction, enabled: bool| {
        let view = view.clone();
        PopupMenuItem::new(label)
            .disabled(!enabled)
            .on_click(move |_, _, cx| {
                view.update(cx, |this, cx| match action {
                    TodoAction::Squash | TodoAction::Fixup => this.log_rebase_unite(action, cx),
                    _ => this.log_rebase_update(cx, |p| p.set(action)),
                })
            })
    };
    let reword_item = {
        let view = view.clone();
        PopupMenuItem::new("Reword")
            .disabled(!reword)
            .on_click(move |_, window, cx| {
                view.update(cx, |this, cx| this.log_rebase_reword(window, cx))
            })
    };
    let commands = {
        let view = view.clone();
        PopupMenuItem::new("View Git Commands").on_click(move |_, window, cx| {
            let lines = view.read(cx).rebase_plan.as_ref().map(|p| {
                (
                    p.lines(),
                    p.rows.iter().map(|(c, _)| c.id.clone()).collect(),
                )
            });
            if let Some((lines, ids)) = lines {
                git_commands_dialog(lines, ids, window, cx)
            }
        })
    };
    let suggest = {
        let view = view.clone();
        PopupMenuItem::new("✦ Suggest Plan with AI").on_click(move |_, window, cx| {
            view.update(cx, |this, cx| {
                let Some(plan) = &this.rebase_plan else {
                    return;
                };
                let commits = plan
                    .rows
                    .iter()
                    .map(|(c, _)| (c.id.chars().take(7).collect(), c.subject.clone()))
                    .collect();
                this.ai_start(crate::ai::AiTask::RebasePlan(commits), window, cx)
            })
        })
    };
    menu.item(set("Pick", TodoAction::Pick, any))
        .item(set("Edit", TodoAction::Edit, any))
        .item(reword_item)
        .item(set("Squash", TodoAction::Squash, unite))
        .item(set("Fixup", TodoAction::Fixup, unite))
        .item(set("Drop", TodoAction::Drop, any))
        .separator()
        .item(commands)
        .separator()
        .item(suggest)
}

/// View Git Commands: the plan as git's todo list (GitBundle
/// `rebase.interactive.dialog.git.commands*`), oldest first.
fn git_commands_dialog(lines: Vec<String>, ids: Vec<String>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let tokens = Tokens::get(cx);
        let cell = |text: String, width: Option<f32>| {
            div()
                .when_some(width, |d, w| d.w(px(w)).flex_none())
                .when(width.is_none(), |d| d.flex_1().min_w_0().truncate())
                .child(text)
        };
        let header = h_flex()
            .h(px(metrics::ROW_HEIGHT))
            .px_2()
            .gap_2()
            .text_color(tokens.text_muted)
            .border_b_1()
            .border_color(tokens.separator)
            .child(cell("Action".into(), Some(80.)))
            .child(cell("Hash".into(), Some(80.)))
            .child(cell("Subject".into(), None));
        let rows = lines.iter().zip(&ids).map(|(line, id)| {
            let (action, subject) = line.split_once(' ').unwrap_or((line, ""));
            h_flex()
                .h(px(metrics::ROW_HEIGHT))
                .px_2()
                .gap_2()
                .child(cell(action.to_owned(), Some(80.)))
                .child(cell(id.chars().take(8).collect(), Some(80.)))
                .child(cell(subject.to_owned(), None))
        });
        dialog.title("Git Commands").w(px(560.)).child(
            v_flex()
                .id("git-commands")
                .role(Role::Table)
                .aria_label("Git Commands")
                .child(header)
                .children(rows),
        )
    });
}
