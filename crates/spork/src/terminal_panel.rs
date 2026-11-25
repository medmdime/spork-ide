//! The Terminal tool window, as in IntelliJ: tabs of shells in the repository's folder under
//! the other tool windows, its height dragged, maximized or hidden (⌥F12). On quit each tab's
//! screen is saved and comes back, above a new prompt, the next time the repository opens.

use std::path::PathBuf;

use crate::notices::Notification;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_terminal::Terminal;

use crate::RepoView;
use crate::repos;
use crate::util::ResultExt as _;

pub(crate) struct TerminalTab {
    view: Entity<Terminal>,
    name: String,
    _exit: Subscription,
}

/// What a drag on the line above the terminal carries.
#[derive(Clone)]
pub(crate) struct TerminalResize;

impl Render for TerminalResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

fn saved_file(root: &std::path::Path) -> Option<PathBuf> {
    let key: String = root
        .display()
        .to_string()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    Some(
        repos::config_dir()?
            .join("terminals")
            .join(format!("{key}.json")),
    )
}

impl RepoView {
    /// Opens a new terminal tab; `None` and a notification when the shell couldn't start.
    fn new_terminal(
        &mut self,
        name: Option<String>,
        history: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Terminal>> {
        let dir = self.tab_root().unwrap_or_else(|| self.requested.clone());
        let view = match Terminal::start(dir, history, cx) {
            Ok(view) => view,
            Err(e) => {
                self.terminal_error = Some(format!("Couldn't start the terminal: {e}"));
                return None;
            }
        };
        // a shell that ends (`exit`) closes its tab, as in IntelliJ
        let exit = cx.observe(&view, |this, view, cx| {
            if view.read(cx).exited() {
                this.terminals.retain(|t| t.view != view);
                this.active_terminal = this
                    .active_terminal
                    .min(this.terminals.len().saturating_sub(1));
                this.show_terminal &= !this.terminals.is_empty();
                cx.notify();
            }
        });
        let taken: Vec<&str> = self.terminals.iter().map(|t| t.name.as_str()).collect();
        let name = name.unwrap_or_else(|| spork_workbench::shell::next_terminal_name(&taken));
        self.terminals.push(TerminalTab {
            view: view.clone(),
            name,
            _exit: exit,
        });
        self.active_terminal = self.terminals.len() - 1;
        Some(view)
    }

    /// IntelliJ's Run Anything, from the gear menu: a command run in a new Terminal tab, in
    /// the project's folder.
    pub(crate) fn run_anything(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.field.update(cx, |s, cx| s.set_value("", window, cx));
        let (view, input) = (cx.entity(), self.field.clone());
        let muted = spork_workbench::Tokens::get(cx).text_secondary;
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, input) = (view.clone(), input.clone());
            dialog
                .title("Run Anything")
                .description(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().text_color(muted).child(
                            "Runs a command in a new Terminal tab, in the project's folder.",
                        ))
                        .child(spork_workbench::TextField::new(&input)),
                )
                .confirm()
                .ok_text("Run")
                .on_ok(move |_, window, cx| {
                    let command = input.read(cx).value().trim().to_owned();
                    if command.is_empty() {
                        return false;
                    }
                    view.update(cx, |this, cx| {
                        match this.new_terminal(Some(command.clone()), None, cx) {
                            Some(terminal) => terminal.read(cx).run(&command),
                            None => return this.report_terminal_error(window, cx),
                        }
                        this.show_console = false;
                        this.show_terminal = true;
                        this.focus_terminal(window, cx);
                        cx.notify();
                    });
                    true
                })
        });
    }

    fn focus_terminal(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.terminals.get(self.active_terminal) {
            let focus = tab.view.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
        }
    }

    /// Shows or hides the Terminal tool window (⌥F12); the first time, opens a tab.
    /// The Git tool window (⌘9): its console of the git commands Spork ran, in the terminal's
    /// place at the bottom.
    pub(crate) fn toggle_console(&mut self, cx: &mut Context<Self>) {
        self.show_console = !self.show_console;
        if self.show_console {
            self.show_terminal = false;
        }
        cx.notify();
    }

    pub(crate) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_console = false;
        if self.show_terminal {
            self.show_terminal = false;
            window.focus(&self.focus, cx);
            return cx.notify();
        }
        if self.terminals.is_empty() && self.new_terminal(None, None, cx).is_none() {
            return self.report_terminal_error(window, cx);
        }
        self.show_terminal = true;
        self.focus_terminal(window, cx);
        cx.notify();
    }

    fn add_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.new_terminal(None, None, cx).is_none() {
            return self.report_terminal_error(window, cx);
        }
        self.focus_terminal(window, cx);
        cx.notify();
    }

    fn close_terminal(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix < self.terminals.len() {
            self.terminals.remove(ix);
        }
        self.active_terminal = self
            .active_terminal
            .min(self.terminals.len().saturating_sub(1));
        if self.terminals.is_empty() {
            self.show_terminal = false;
            window.focus(&self.focus, cx);
        } else {
            self.focus_terminal(window, cx);
        }
        cx.notify();
    }

    fn report_terminal_error(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(message) = self.terminal_error.take() {
            window.push_notification(Notification::error(message), cx);
        }
    }

    /// Saves the tabs and what each shows, for the next launch.
    pub(crate) fn save_terminals(&self, cx: &App) {
        let Some(file) = self.tab_root().and_then(|root| saved_file(&root)) else {
            return;
        };
        if self.terminals.is_empty() {
            std::fs::remove_file(file).ok();
            return;
        }
        let tabs: Vec<serde_json::Value> = self
            .terminals
            .iter()
            .map(|t| serde_json::json!({ "name": t.name, "text": t.view.read(cx).text() }))
            .collect();
        let saved = serde_json::json!({
            "open": self.show_terminal,
            "active": self.active_terminal,
            "height": f32::from(self.terminal_height),
            "tabs": tabs,
        });
        std::fs::create_dir_all(file.parent().unwrap_or(&file)).log_err();
        std::fs::write(file, saved.to_string()).log_err();
    }

    /// Reopens the tabs saved when Spork last quit with this repository open.
    pub(crate) fn restore_terminals(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self.tab_root().and_then(|root| saved_file(&root)) else {
            return;
        };
        let Some(saved) = std::fs::read_to_string(&file)
            .ok()
            .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        else {
            return;
        };
        let tabs = saved["tabs"].as_array().cloned().unwrap_or_default();
        for tab in &tabs {
            let name = tab["name"].as_str().map(str::to_owned);
            self.new_terminal(name, tab["text"].as_str(), cx);
        }
        let active = saved["active"].as_u64().unwrap_or(0) as usize;
        self.active_terminal = active.min(self.terminals.len().saturating_sub(1));
        self.show_terminal = saved["open"].as_bool().unwrap_or(false) && !self.terminals.is_empty();
        if let Some(height) = saved["height"].as_f64().filter(|h| *h > 0.) {
            self.terminal_height = px(height as f32);
        }
        cx.notify();
    }

    /// `body` with the Terminal tool window under it, while it shows.
    pub(crate) fn with_terminal(&mut self, body: Div, cx: &mut Context<Self>) -> Div {
        if self.show_console {
            let console = self.git_window(cx);
            return self.bottom_panel(body, console, cx);
        }
        let Some(active) = self
            .terminals
            .get(self.active_terminal)
            .filter(|_| self.show_terminal)
        else {
            return body.size_full();
        };
        let terminal = active.view.clone();
        let tabs = self.terminals.iter().enumerate().map(|(ix, tab)| {
            let current = ix == self.active_terminal;
            spork_workbench::shell::header_tab(("terminal-tab", ix), tab.name.clone(), current, cx)
                .pr_0p5()
                .gap_0p5()
                .child(
                    Button::new(("close-terminal", ix))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .tooltip("Close tab")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_terminal(ix, window, cx)
                        })),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.active_terminal = ix;
                    this.focus_terminal(window, cx);
                    cx.notify();
                }))
                // a middle click (the scroll wheel) closes it, as in IntelliJ and browsers
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |this, _, window, cx| this.close_terminal(ix, window, cx)),
                )
        });
        let max = self.terminal_max;
        let header = h_flex()
            .w_full()
            .h(px(40.))
            .flex_none()
            .pl_3()
            .pr_1p5()
            .gap_1()
            .border_b_1()
            .border_color(spork_workbench::Tokens::get(cx).separator)
            .child(
                div()
                    .mr_2()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Terminal"),
            )
            .children(tabs)
            .child(
                Button::new("new-terminal")
                    .ghost()
                    .small()
                    .icon(IconName::Plus)
                    .tooltip("New tab")
                    .on_click(cx.listener(|this, _, window, cx| this.add_terminal(window, cx))),
            )
            .child(div().flex_1())
            .child(
                Button::new("maximize-terminal")
                    .ghost()
                    .small()
                    .icon(if max {
                        IconName::Minimize
                    } else {
                        IconName::Maximize
                    })
                    .tooltip(if max { "Restore" } else { "Maximize" })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.terminal_max = !this.terminal_max;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("hide-terminal")
                    .ghost()
                    .small()
                    .icon(IconName::Minus)
                    .tooltip("Hide (⌥F12)")
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_terminal(window, cx))),
            );
        let panel = v_flex()
            .size_full()
            .child(header)
            .child(div().flex_1().min_h_0().px_3().pb_1().child(terminal));
        self.bottom_panel(body, panel, cx)
    }

    /// `body` above a bottom tool window (the terminal, or Git's console), the gap between them
    /// dragged to resize it.
    fn bottom_panel(&mut self, body: Div, content: Div, cx: &mut Context<Self>) -> Div {
        let max = self.terminal_max && self.show_terminal;
        let tool = match self.show_console {
            true => crate::ActiveTool::Git,
            false => crate::ActiveTool::Terminal,
        };
        let panel = crate::island(cx)
            .capture_any_mouse_down(
                cx.listener(move |this, _, _, cx| this.set_active_tool(Some(tool), cx)),
            )
            .flex_none()
            .when(max, |p| p.flex_1())
            .when(!max, |p| p.h(self.terminal_height))
            .child(content);
        // the gap above the panel is its handle: drag it to make the terminal taller or shorter
        let handle = div()
            .id("terminal-resize")
            .w_full()
            .h(px(6.))
            .flex_none()
            .cursor_row_resize()
            .on_drag(TerminalResize, |drag, _, _, cx| cx.new(|_| drag.clone()));
        v_flex()
            .size_full()
            .on_drag_move(
                cx.listener(|this, event: &DragMoveEvent<TerminalResize>, _, cx| {
                    let room = event.bounds.size.height - px(120.);
                    let height = event.bounds.bottom() - event.event.position.y;
                    this.terminal_height = height.min(room).max(px(80.));
                    cx.notify();
                }),
            )
            .when(!max, |c| {
                c.child(div().flex_1().min_h_0().child(body.size_full()))
                    .child(handle)
            })
            .child(panel)
    }
}
