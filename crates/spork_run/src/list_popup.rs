//! IntelliJ's list popups (`ListPopupImpl`) the debugger and the run surfaces show: a title, the
//! rows (an icon and the text), in groups under their titles, the arrows, ⏎ and Esc,
//! and the speed search IntelliJ's list popups have: typed letters keep the rows that contain
//! them. Smart Step Into's chooser, Attach to Process, Show Running List and Stop Background
//! Processes are built from it.

use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, DismissEvent, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, Render, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use gpui_kit::component::{h_flex, v_flex};
use spork_workbench::{Tokens, popup};
use workspace::{ModalView, Workspace};

pub(crate) struct Row {
    pub label: SharedString,
    /// A coloured icon (`spork_workbench::image`).
    pub icon: Option<&'static str>,
    /// The title of the group the row starts, shown above it.
    pub group: Option<SharedString>,
}

impl Row {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            group: None,
        }
    }

    pub fn image(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn group(mut self, title: impl Into<SharedString>) -> Self {
        self.group = Some(title.into());
        self
    }
}

type Confirm = Rc<dyn Fn(usize, &mut Window, &mut App)>;

pub(crate) struct ListPopup {
    title: SharedString,
    rows: Vec<Row>,
    /// What an empty list says.
    empty: SharedString,
    width: f32,
    /// The speed search's letters.
    typed: String,
    /// The selected row among those shown.
    selected: usize,
    confirm: Confirm,
    focus: FocusHandle,
}

impl ListPopup {
    /// Shows the popup over `workspace`; ⏎ or a click calls `confirm` with the row's index in
    /// `rows`, after the popup closes.
    pub fn show(
        workspace: &mut Workspace,
        title: impl Into<SharedString>,
        rows: Vec<Row>,
        empty: impl Into<SharedString>,
        width: f32,
        confirm: impl Fn(usize, &mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let (title, empty) = (title.into(), empty.into());
        let confirm: Confirm = Rc::new(confirm);
        workspace.toggle_modal(window, cx, move |window, cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            Self {
                title,
                rows,
                empty,
                width,
                typed: String::new(),
                selected: 0,
                confirm,
                focus,
            }
        });
    }

    /// The rows the speed search keeps, by their index in `rows`.
    fn shown(&self) -> Vec<usize> {
        let typed = self.typed.to_lowercase();
        (0..self.rows.len())
            .filter(|ix| typed.is_empty() || self.rows[*ix].label.to_lowercase().contains(&typed))
            .collect()
    }

    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
        let confirm = self.confirm.clone();
        // the popup is gone before what it starts opens a popup or a dialog of its own
        window.defer(cx, move |window, cx| confirm(ix, window, cx));
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self.shown();
        let keystroke = &event.keystroke;
        match keystroke.key.as_str() {
            "up" => self.selected = self.selected.saturating_sub(1),
            "down" => self.selected = (self.selected + 1).min(shown.len().saturating_sub(1)),
            "enter" => {
                if let Some(ix) = shown.get(self.selected) {
                    self.confirm(*ix, window, cx);
                }
                return;
            }
            "escape" if !self.typed.is_empty() => self.typed.clear(),
            "escape" => return cx.emit(DismissEvent),
            "backspace" => {
                self.typed.pop();
            }
            _ => match keystroke.key_char.as_deref() {
                Some(typed)
                    if !keystroke.modifiers.platform
                        && !keystroke.modifiers.control
                        && typed.chars().all(|c| !c.is_control()) =>
                {
                    self.typed.push_str(typed);
                    self.selected = 0;
                }
                _ => return,
            },
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl EventEmitter<DismissEvent> for ListPopup {}

impl ModalView for ListPopup {}

impl Focusable for ListPopup {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ListPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let shown = self.shown();
        self.selected = self.selected.min(shown.len().saturating_sub(1));
        let mut rows = Vec::new();
        let mut group = None;
        for (position, ix) in shown.iter().copied().enumerate() {
            let row = &self.rows[ix];
            // a group's title over its first row the search keeps
            let row_group = self.rows[..=ix]
                .iter()
                .rev()
                .find_map(|row| row.group.clone());
            if row_group.is_some() && row_group != group {
                rows.push(
                    popup::labeled_separator(
                        row_group.clone().unwrap_or_default(),
                        position == 0,
                        cx,
                    )
                    .into_any_element(),
                );
                group = row_group;
            }
            let selected = position == self.selected;
            let icon = row
                .icon
                .map(|icon| spork_workbench::image(icon, cx).size(px(16.)));
            rows.push(
                popup::row(("list-popup-row", ix), selected, cx)
                    .role(Role::MenuItem)
                    .aria_label(row.label.clone())
                    .aria_selected(selected)
                    .children(icon)
                    .child(div().min_w_0().truncate().child(row.label.clone()))
                    .on_click(cx.listener(move |this, _, window, cx| this.confirm(ix, window, cx)))
                    .into_any_element(),
            );
        }
        let empty = rows.is_empty();
        spork_vcs::spork_look(cx).child(
            popup::surface(cx)
                .id("list-popup")
                .role(Role::Dialog)
                .aria_label(self.title.clone())
                .track_focus(&self.focus)
                .key_context("ListPopup")
                .on_key_down(cx.listener(Self::key_down))
                .w(px(self.width))
                .child(popup::title(self.title.clone(), cx))
                .when(!self.typed.is_empty(), |popup| {
                    popup.child(
                        h_flex()
                            .id("list-popup-search")
                            .role(Role::TextInput)
                            .aria_label(SharedString::from(self.typed.clone()))
                            .flex_none()
                            .mx(px(12.))
                            .mb(px(4.))
                            .gap(px(4.))
                            .child(
                                spork_workbench::icon("general/search")
                                    .size(px(16.))
                                    .text_color(tokens.icon),
                            )
                            .child(self.typed.clone()),
                    )
                })
                .child(
                    v_flex()
                        .id("list-popup-rows")
                        .role(Role::Menu)
                        .aria_label(self.title.clone())
                        .max_h(px(400.))
                        .overflow_y_scroll()
                        .pb(px(6.))
                        .children(rows)
                        .when(empty, |list| {
                            list.child(
                                div()
                                    .px(px(16.))
                                    .py(px(6.))
                                    .text_color(tokens.text_secondary)
                                    .child(self.empty.clone()),
                            )
                        }),
                ),
        )
    }
}
