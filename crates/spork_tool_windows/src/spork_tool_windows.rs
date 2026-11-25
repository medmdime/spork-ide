//! IntelliJ's tool windows of the frame that no other part of Spork IDE owns (ROADMAP, phase 3):
//! Problems (⌘6) at the bottom, over the language servers' diagnostics and Inspect Code's results,
//! and Notifications on the right, over the notifications the IDE shows; TODO at the bottom, over
//! the project search; and the tool window
//! layout actions (Jump to Last Tool Window, Restore Current Layout).

mod layout;
mod notifications;
mod problems;
mod todo;

pub use layout::tool_windows;
pub use notifications::{e2e_notify, notifications_e2e_state, notifications_panel};
pub use problems::problems_panel;
pub use todo::todo_panel;

use std::rc::Rc;

use gpui::App;
use workspace::Workspace;

pub fn init(cx: &mut App) {
    notifications::init(cx);
    cx.set_global(spork_code::inspection::InspectionResults(Rc::new(
        problems::show_inspection,
    )));
    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        let Some(window) = window else { return };
        problems::add_panel(workspace, window, cx);
        notifications::add_panel(workspace, window, cx);
        todo::add_panel(workspace, window, cx);
        layout::register(workspace, window, cx);
    })
    .detach();
}
