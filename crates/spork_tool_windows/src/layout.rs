//! IntelliJ's tool window layout actions: Jump to Last Tool Window (F12, `JumpToLastWindowAction`:
//! the tool window that had the focus last, shown and focused again) and Window ▸ Tool Window
//! Layouts' Default and Restore Current Layout (⇧F12, `RestoreDefaultLayoutAction`): the Project
//! tool window open on the left, the others hidden, each side at its default size.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{Context, EntityId, Focusable as _, Window};
use workspace::Workspace;

pub mod tool_windows {
    gpui::actions!(
        tool_windows,
        [
            /// Shows and focuses the tool window that had the focus last.
            JumpToLastToolWindow,
            /// Restores the current tool window layout: Spork IDE has only its default one.
            RestoreDefaultLayout,
            /// Resets the tool window layout to the factory default.
            RestoreFactoryDefaultLayout,
        ]
    );
}

/// The Project tool window's panel, the one IntelliJ's default layout shows.
const PROJECT_PANEL: &str = "Project Panel";

pub(crate) fn register(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    // IntelliJ's `lastActiveToolWindowId`: the panel a dock's focus went into last
    let last: Rc<Cell<Option<EntityId>>> = Rc::default();
    for dock in workspace.all_docks() {
        let (dock, last) = (dock.clone(), last.clone());
        cx.on_focus_in(&dock.focus_handle(cx), window, move |_, _, cx| {
            if let Some(panel) = dock.read(cx).visible_panel() {
                last.set(Some(panel.panel_id()));
            }
        })
        .detach();
    }
    workspace.register_action(
        move |workspace, _: &tool_windows::JumpToLastToolWindow, window, cx| {
            let Some(panel_id) = last.get() else {
                return;
            };
            let found = workspace.all_docks().into_iter().find_map(|dock| {
                let dock = dock.read(cx);
                let panel = dock.panels().find(|panel| panel.panel_id() == panel_id)?;
                let shows = dock.is_open()
                    && dock.visible_panel().map(|panel| panel.panel_id()) == Some(panel_id);
                Some((dock.position(), shows, panel.activation_focus_handle(cx)))
            });
            match found {
                Some((_, true, focus)) => window.focus(&focus, cx),
                Some((position, false, _)) => {
                    workspace.toggle_tool_window(position, panel_id, window, cx)
                }
                None => {}
            }
        },
    );
    workspace.register_action(
        |workspace, _: &tool_windows::RestoreDefaultLayout, window, cx| {
            restore_default_layout(workspace, window, cx)
        },
    );
    workspace.register_action(
        |workspace, _: &tool_windows::RestoreFactoryDefaultLayout, window, cx| {
            restore_default_layout(workspace, window, cx)
        },
    );
}

fn restore_default_layout(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    for dock in workspace
        .all_docks()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>()
    {
        dock.update(cx, |dock, cx| {
            // shortcut: each side's shown tool window (all of the left's, which resize together)
            // gets its default size back, not the hidden ones of the bottom and right; activating
            // each to reset it would start a terminal in the Terminal tool window
            dock.reset_panel_sizes(window, cx);
            match dock.panel_index_for_persistent_name(PROJECT_PANEL, cx) {
                Some(ix) => {
                    dock.activate_panel(ix, window, cx);
                    dock.set_open(true, window, cx);
                }
                None => dock.set_open(false, window, cx),
            }
        });
    }
    let pane = workspace.active_pane().clone();
    window.focus(&pane.focus_handle(cx), cx);
    cx.notify();
}
