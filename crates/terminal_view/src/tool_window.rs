//! Spork IDE (ours): IntelliJ's Terminal tool window over Zed's terminal panel: its header (the
//! title, a tab per session of the active pane named "Local", "Local (2)"…, New Tab), a session
//! tab's menu (Rename Session, Close Tab, Close All Tabs, Close Other Tabs, Split Right, Split
//! Down), the Rename Session popup, and Find Action's Increase, Decrease and Reset Font Size in
//! Terminal.

use std::sync::Arc;

use gpui::TaskExt as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action as _, Anchor, AnyElement, App, AppContext as _, Context, Entity, Global,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, anchored, deferred, div, px,
};
use settings::Settings as _;
use spork_workbench::Tokens;
use terminal::terminal_settings::TerminalSettings;
use theme::ActiveTheme as _;
use ui::{
    ButtonCommon as _, Clickable as _, ContextMenu, Label, LabelCommon as _, PopoverMenu, Tooltip,
    h_flex, right_click_menu, v_flex,
};
use workspace::dock::PanelHandle;
use workspace::item::{Item as _, ItemHandle};
use workspace::{Pane, SaveIntent, SplitDirection, SplitDown, SplitMode, SplitRight, frame};

use crate::TerminalView;
use crate::terminal_panel::TerminalPanel;

gpui::actions!(
    terminal,
    [
        /// Renames the selected terminal session (IntelliJ's Rename Session).
        RenameSession,
        /// Makes the terminals' text a point bigger, until the IDE restarts.
        IncreaseFontSize,
        /// Makes the terminals' text a point smaller, until the IDE restarts.
        DecreaseFontSize,
        /// Gives the terminals their font size from the settings again.
        ResetFontSize,
    ]
);

/// The terminals' font size while Increase or Decrease Font Size has changed it.
struct FontSize(Pixels);

impl Global for FontSize {}

/// IntelliJ's smallest and largest editor font sizes (`EditorFontsConstants`).
const FONT_SIZES: std::ops::RangeInclusive<f32> = 4.0..=72.0;

/// The size the terminals draw their text at.
pub(crate) fn font_size(cx: &App) -> Pixels {
    if let Some(size) = cx.try_global::<FontSize>() {
        return size.0;
    }
    match TerminalSettings::get_global(cx).font_size {
        Some(size) => theme_settings::adjusted_font_size(size, cx),
        None => theme_settings::ThemeSettings::get_global(cx).buffer_font_size(cx),
    }
}

fn change_font_size(step: f32, cx: &mut App) {
    let size = f32::from(font_size(cx)) + step;
    if FONT_SIZES.contains(&size) {
        cx.set_global(FontSize(px(size)));
        cx.refresh_windows();
    }
}

/// The panel's actions, on its element so they're there while a terminal has the focus.
pub(crate) fn with_actions(element: gpui::Div, cx: &mut Context<TerminalPanel>) -> gpui::Div {
    element
        .on_action(cx.listener(|panel, _: &RenameSession, window, cx| {
            if let Some(view) = active_session(panel, cx) {
                rename(&view, window, cx);
            }
        }))
        .on_action(|_: &IncreaseFontSize, _, cx| change_font_size(1., cx))
        .on_action(|_: &DecreaseFontSize, _, cx| change_font_size(-1., cx))
        .on_action(|_: &ResetFontSize, _, cx| {
            if cx.has_global::<FontSize>() {
                cx.remove_global::<FontSize>();
                cx.refresh_windows();
            }
        })
}

fn active_session(panel: &TerminalPanel, cx: &App) -> Option<Entity<TerminalView>> {
    panel
        .active_pane
        .read(cx)
        .active_item()?
        .downcast::<TerminalView>()
}

/// The sessions of every pane of the panel.
fn sessions(panel: &TerminalPanel, cx: &App) -> Vec<Entity<TerminalView>> {
    panel
        .center
        .panes()
        .into_iter()
        .flat_map(|pane| {
            pane.read(cx)
                .items()
                .filter_map(|item| item.downcast::<TerminalView>())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// A session joining the panel gets IntelliJ's name for it, unless it has one, and the header
/// follows its changes (its name, a rename starting).
pub(crate) fn name_session(
    panel: &TerminalPanel,
    item: &dyn ItemHandle,
    cx: &mut Context<TerminalPanel>,
) {
    let Some(view) = item.downcast::<TerminalView>() else {
        return;
    };
    cx.observe(&view, |_, _, cx| cx.notify()).detach();
    if view.read(cx).custom_title().is_some() || view.read(cx).terminal().read(cx).task().is_some()
    {
        return;
    }
    let names: Vec<String> = sessions(panel, cx)
        .iter()
        .filter(|other| *other != &view)
        .filter_map(|other| other.read(cx).custom_title().map(str::to_owned))
        .collect();
    let taken: Vec<&str> = names.iter().map(String::as_str).collect();
    let name = spork_workbench::shell::next_terminal_name(&taken);
    view.update(cx, |view, cx| view.set_custom_title(Some(name), cx));
}

/// Rename Session: the session's name in IntelliJ's popup over its tab.
fn rename(view: &Entity<TerminalView>, window: &mut Window, cx: &mut App) {
    view.update(cx, |view, cx| {
        view.rename_terminal(&crate::RenameTerminal, window, cx)
    });
}

/// The panel's header: the title, the sessions of the active pane, New Tab; then the tool
/// window's ⋮ and Hide.
pub(crate) fn header(panel: &TerminalPanel, cx: &mut Context<TerminalPanel>) -> AnyElement {
    let pane = panel.active_pane.clone();
    let active = pane.read(cx).active_item_index();
    let items: Vec<(usize, Entity<TerminalView>)> = pane
        .read(cx)
        .items()
        .enumerate()
        .filter_map(|(ix, item)| Some((ix, item.downcast::<TerminalView>()?)))
        .collect();
    let tabs: Vec<AnyElement> = items
        .into_iter()
        .map(|(ix, view)| session_tab(ix, ix == active, &pane, view, cx))
        .collect();
    let new_tab = frame::header_button("terminal-new-tab", "general/add", "New Tab", cx)
        .tooltip(|_, cx| Tooltip::for_action("New Tab", &workspace::NewTerminal::default(), cx))
        .on_click(|_, window, cx| {
            window.dispatch_action(workspace::NewTerminal::default().boxed_clone(), cx)
        });
    let predefined = PopoverMenu::new("terminal-predefined-session")
        .trigger_with_tooltip(
            frame::header_button(
                "terminal-predefined-session-button",
                "general/chevronDown",
                "New Predefined Session",
                cx,
            ),
            Tooltip::text("New Predefined Session"),
        )
        .anchor(Anchor::TopLeft)
        .menu(|window, cx| {
            Some(ContextMenu::build(window, cx, |menu, _, _| {
                detected_shells()
                    .into_iter()
                    .fold(menu, |menu, (name, path)| {
                        let session = name.clone();
                        menu.entry(name, None, move |window, cx| {
                            new_session(path.clone(), session.clone(), window, cx)
                        })
                    })
            }))
        });
    let title = h_flex()
        .gap(px(12.))
        .child(frame::tool_window_title_label("Terminal"))
        .child(
            h_flex()
                .gap(px(2.))
                .children(tabs)
                .child(new_tab)
                .child(predefined),
        )
        .into_any_element();
    let handle: Arc<dyn PanelHandle> = Arc::new(cx.entity());
    frame::tool_window_header(title, Vec::new(), handle, "Terminal".into(), cx).into_any_element()
}

/// The shells New Predefined Session offers, as IntelliJ's `TerminalShellsDetectionUtil` finds
/// them: bash, zsh, fish and pwsh in the usual folders, named by their path when one is in
/// several.
// shortcut: no "Settings" item after them, which would open Settings › Tools › Terminal; add it
// once the Settings dialog takes a page to open on from this crate
fn detected_shells() -> Vec<(String, std::path::PathBuf)> {
    const FOLDERS: [&str; 4] = ["/bin", "/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"];
    const SHELLS: [&str; 4] = ["bash", "zsh", "fish", "pwsh"];
    let found: Vec<(&str, std::path::PathBuf)> = FOLDERS
        .iter()
        .flat_map(|folder| {
            SHELLS
                .iter()
                .map(move |shell| (*shell, std::path::Path::new(folder).join(shell)))
        })
        .filter(|(_, path)| path.is_file())
        .collect();
    shell_names(&found)
}

/// Each shell's name, with its path after it when another shell found has the same name.
fn shell_names(found: &[(&str, std::path::PathBuf)]) -> Vec<(String, std::path::PathBuf)> {
    found
        .iter()
        .map(
            |(name, path)| match found.iter().filter(|(other, _)| other == name).count() {
                1 => (name.to_string(), path.clone()),
                _ => (format!("{name} ({})", path.display()), path.clone()),
            },
        )
        .collect()
}

/// A predefined session: `shell` in a new tab named after it.
fn new_session(shell: std::path::PathBuf, name: String, window: &mut Window, cx: &mut App) {
    let Some(workspace) = workspace::Workspace::for_window(window, cx) else {
        return;
    };
    let Some(panel) = workspace.read(cx).panel::<TerminalPanel>(cx) else {
        return;
    };
    let cwd = crate::default_working_directory(workspace.read(cx), cx);
    let shell = task::Shell::Program(shell.to_string_lossy().into_owned());
    let terminal = workspace.update(cx, |workspace, cx| {
        workspace.project().update(cx, |project, cx| {
            project.create_terminal_with_shell(cwd, shell, cx)
        })
    });
    window
        .spawn(cx, async move |cx| {
            let terminal = terminal.await?;
            workspace.update_in(cx, |workspace, window, cx| {
                let view = cx.new(|cx| {
                    let mut view = TerminalView::new(
                        terminal,
                        workspace.weak_handle(),
                        workspace.database_id(),
                        workspace.project().downgrade(),
                        window,
                        cx,
                    );
                    view.set_custom_title(Some(name), cx);
                    view
                });
                let pane = panel.read(cx).active_pane.clone();
                pane.update(cx, |pane, cx| {
                    pane.add_item(Box::new(view), true, true, None, window, cx)
                });
            })
        })
        .detach_and_log_err(cx);
}

/// A session's tab: its name and Close, a click selecting it, a right click its menu; while it's
/// being renamed, IntelliJ's popup over it.
fn session_tab(
    ix: usize,
    selected: bool,
    pane: &Entity<Pane>,
    view: Entity<TerminalView>,
    cx: &mut Context<TerminalPanel>,
) -> AnyElement {
    let tokens = Tokens::for_appearance(!cx.theme().appearance.is_light());
    let name: SharedString = view.read(cx).tab_content_text(0, cx);
    let item_id = view.entity_id();
    let focus = view.read(cx).focus_handle.clone();
    let renaming = view.read(cx).rename_editor();
    let close_tab = {
        let pane = pane.clone();
        move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
            cx.stop_propagation();
            close(&pane, window, cx, move |id| id == item_id);
        }
    };
    let tab = h_flex()
        .id(("terminal-session", ix))
        .role(Role::Tab)
        .aria_label(name.clone())
        .aria_selected(selected)
        .relative()
        .h(px(28.))
        .pl(px(8.))
        .pr(px(2.))
        .gap(px(2.))
        .rounded(px(6.))
        .border_1()
        .cursor_pointer()
        .map(|tab| match selected {
            true => tab
                .bg(tokens.tab_selected_inactive)
                .border_color(tokens.tab_selected_border_inactive),
            false => tab
                .border_color(gpui::transparent_black())
                .hover(move |tab| tab.bg(tokens.hover)),
        })
        .child(Label::new(name).single_line())
        .child(
            frame::header_button(
                ("terminal-close-session", ix),
                "general/closeSmall",
                "Close Tab",
                cx,
            )
            .tooltip(Tooltip::text("Close Tab"))
            .on_click(close_tab),
        )
        .on_click({
            let pane = pane.clone();
            move |_, window, cx| {
                pane.update(cx, |pane, cx| {
                    pane.activate_item(ix, true, true, window, cx)
                });
            }
        })
        .children(renaming.map(|editor| rename_popup(&view, editor, cx)));
    let pane = pane.clone();
    right_click_menu(("terminal-session-menu", ix))
        .trigger(move |_, _, _| tab)
        .menu(move |window, cx| {
            let (pane, view, focus) = (pane.clone(), view.clone(), focus.clone());
            ContextMenu::build(window, cx, move |menu, _, _| {
                let (rename_view, close_pane, all_pane, others_pane, right_pane, down_pane) = (
                    view.clone(),
                    pane.clone(),
                    pane.clone(),
                    pane.clone(),
                    pane.clone(),
                    pane.clone(),
                );
                menu.context(focus.clone())
                    .entry("Rename Session", None, move |window, _| {
                        // two frames on, after the menu's own focusing (`right_click_menu` focuses
                        // it two frames after it opens) and giving the focus back, so the field
                        // keeps it
                        let view = rename_view.clone();
                        window.on_next_frame(move |window, _| {
                            window.on_next_frame(move |window, cx| rename(&view, window, cx))
                        });
                        window.refresh();
                    })
                    .separator()
                    .entry("Close Tab", None, move |window, cx| {
                        close(&close_pane, window, cx, move |id| id == item_id)
                    })
                    .entry("Close All Tabs", None, move |window, cx| {
                        close(&all_pane, window, cx, |_| true)
                    })
                    .entry("Close Other Tabs", None, move |window, cx| {
                        close(&others_pane, window, cx, move |id| id != item_id)
                    })
                    .separator()
                    .entry(
                        "Split Right",
                        Some(SplitRight::default().boxed_clone()),
                        move |window, cx| split(&right_pane, ix, SplitDirection::Right, window, cx),
                    )
                    .entry(
                        "Split Down",
                        Some(SplitDown::default().boxed_clone()),
                        move |window, cx| split(&down_pane, ix, SplitDirection::Down, window, cx),
                    )
            })
        })
        .into_any_element()
}

/// Split Right and Split Down: a new session beside the tab's, as IntelliJ's terminal splits.
fn split(
    pane: &Entity<Pane>,
    ix: usize,
    direction: SplitDirection,
    window: &mut Window,
    cx: &mut App,
) {
    pane.update(cx, |pane, cx| {
        pane.activate_item(ix, true, true, window, cx);
        pane.split(direction, SplitMode::ClonePane, window, cx);
    });
}

/// Closes the sessions `which` picks, then gives the focus to the session left selected, as
/// IntelliJ does (the tab's menu had it).
fn close(
    pane: &Entity<Pane>,
    window: &mut Window,
    cx: &mut App,
    which: impl Fn(gpui::EntityId) -> bool + 'static,
) {
    let closing = pane.update(cx, |pane, cx| {
        pane.close_items(window, cx, SaveIntent::Close, &which)
    });
    let pane = pane.downgrade();
    window
        .spawn(cx, async move |cx| {
            closing.await?;
            pane.update_in(cx, |pane, window, cx| pane.focus_active_item(window, cx))
        })
        .detach_and_log_err(cx);
}

/// IntelliJ's Rename Session popup (`RenamePopup`): "Session name:" over the name, selected;
/// ⏎ renames, ⎋ or a click elsewhere leaves the name as it was.
fn rename_popup(
    view: &Entity<TerminalView>,
    editor: Entity<editor::Editor>,
    cx: &App,
) -> AnyElement {
    let colors = cx.theme().colors();
    let (confirm, cancel) = (view.downgrade(), view.downgrade());
    let popup = v_flex()
        .id("terminal-rename-session")
        .role(Role::Dialog)
        .aria_label("Session name:")
        .occlude()
        .w(px(220.))
        .p(px(8.))
        .gap(px(2.))
        .rounded(px(6.))
        .border_1()
        .border_color(colors.border)
        .bg(colors.elevated_surface_background)
        .shadow_md()
        .child(Label::new("Session name:").weight(gpui::FontWeight::BOLD))
        .child(
            div()
                .px(px(6.))
                .py(px(3.))
                .rounded(px(4.))
                .border_1()
                .border_color(colors.border_focused)
                .bg(colors.editor_background)
                .child(editor.clone()),
        )
        .on_action(move |_: &menu::Confirm, window, cx| {
            // an empty name isn't taken: the field stays, as IntelliJ's outlines it in red
            if editor.read(cx).text(cx).trim().is_empty() {
                return;
            }
            confirm
                .update(cx, |view, cx| view.finish_renaming(true, window, cx))
                .ok();
        })
        .on_action(move |_: &menu::Cancel, window, cx| {
            cancel
                .update(cx, |view, cx| view.finish_renaming(false, window, cx))
                .ok();
        });
    // above the tab, as IntelliJ's balloon points at it
    div()
        .absolute()
        .top_0()
        .left_0()
        .child(deferred(anchored().anchor(Anchor::BottomLeft).child(popup)).with_priority(1))
        .into_any_element()
}

/// What the end-to-end tests read of the Terminal tool window: the active pane's `sessions` (their
/// names), the `active` one, how many `panes` it's split into, whether a session is `renaming`,
/// and the terminals' `font_size`.
pub fn e2e_state(workspace: &workspace::Workspace, cx: &App) -> serde_json::Value {
    let Some(panel) = workspace.panel::<TerminalPanel>(cx) else {
        return serde_json::Value::Null;
    };
    let panel = panel.read(cx);
    let pane = panel.active_pane.read(cx);
    let names: Vec<SharedString> = pane
        .items()
        .filter_map(|item| item.downcast::<TerminalView>())
        .map(|view| view.read(cx).tab_content_text(0, cx))
        .collect();
    let active = active_session(panel, cx).map(|view| view.read(cx).tab_content_text(0, cx));
    let renaming = sessions(panel, cx)
        .iter()
        .any(|view| view.read(cx).is_renaming());
    serde_json::json!({
        "sessions": names,
        "active": active,
        "panes": panel.center.panes().len(),
        "renaming": renaming,
        "font_size": f32::from(font_size(cx)),
    })
}

#[cfg(test)]
mod tests {
    use super::shell_names;
    use std::path::PathBuf;

    #[test]
    fn a_shell_found_twice_is_named_by_its_path() {
        let found = [
            ("bash", PathBuf::from("/bin/bash")),
            ("zsh", PathBuf::from("/bin/zsh")),
            ("bash", PathBuf::from("/opt/homebrew/bin/bash")),
        ];
        let names: Vec<String> = shell_names(&found)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            names,
            ["bash (/bin/bash)", "zsh", "bash (/opt/homebrew/bin/bash)"]
        );
    }
}
