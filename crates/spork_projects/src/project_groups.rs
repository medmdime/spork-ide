//! IntelliJ's recent project groups (`ProjectGroup`, `CreateNewProjectGroupAction`,
//! `EditProjectGroupAction`, `MoveProjectToGroupAction`): named groups of the recent projects,
//! each a row of the Welcome window that opens and closes, kept in the key-value store.

use std::path::PathBuf;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use spork_workbench::tokens::Tokens;
use util::ResultExt as _;

const KEY: &str = "spork_recent_project_groups";

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct ProjectGroup {
    pub(crate) name: String,
    /// Its projects, by their folders.
    pub(crate) projects: Vec<Vec<PathBuf>>,
    /// Whether its row shows its projects: closed at first, as IntelliJ's.
    pub(crate) expanded: bool,
}

pub(crate) fn read_groups(cx: &App) -> Vec<ProjectGroup> {
    db::kvp::KeyValueStore::global(cx)
        .read_kvp(KEY)
        .log_err()
        .flatten()
        .and_then(|value| serde_json::from_str(&value).log_err())
        .unwrap_or_default()
}

pub(crate) fn write_groups(groups: &[ProjectGroup], cx: &mut App) {
    let value = serde_json::to_string(groups);
    let store = db::kvp::KeyValueStore::global(cx);
    cx.background_spawn(async move { store.write_kvp(KEY.to_owned(), value?).await })
        .detach_and_log_err(cx);
}

/// Moves `project` into the group named `to` (out of any other), or out of every group.
pub(crate) fn move_to_group(groups: &mut [ProjectGroup], project: &[PathBuf], to: Option<&str>) {
    for group in groups.iter_mut() {
        group.projects.retain(|paths| paths != project);
        if Some(group.name.as_str()) == to {
            group.projects.push(project.to_vec());
            group.expanded = true;
        }
    }
}

/// What Edit… refuses, as IntelliJ's `EditProjectGroupAction` says it; `current`, the group's own
/// name, may stay.
pub(crate) fn rename_error(
    groups: &[ProjectGroup],
    current: &str,
    name: &str,
) -> Option<SharedString> {
    let name = name.trim();
    if name.is_empty() {
        return Some("Name cannot be empty".into());
    }
    (name != current && groups.iter().any(|group| group.name == name))
        .then(|| format!("Group '{name}' already exists.").into())
}

/// The name dialog of New Project Group (`Create New Project Group`, OK greyed for a name taken)
/// and Edit… (`Change Group Name`, its errors under the field).
pub(crate) fn name_dialog(
    title: &'static str,
    label: &'static str,
    initial: String,
    error: impl Fn(&str) -> Option<SharedString> + 'static,
    show_error: bool,
    on_ok: impl Fn(String, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let input = cx.new(|cx| {
        let mut input = InputState::new(window, cx);
        input.set_value(initial, window, cx);
        input
    });
    let error = std::rc::Rc::new(error);
    let on_ok = std::rc::Rc::new(on_ok);
    let ok = {
        let (input, error) = (input.clone(), error.clone());
        std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
            let name = input.read(cx).value().to_string();
            if error(&name).is_some() {
                return;
            }
            window.close_dialog(cx);
            on_ok(name, window, cx);
        })
    };
    window
        .subscribe(&input, cx, {
            let ok = ok.clone();
            move |_, event: &InputEvent, window, cx| {
                match event {
                    InputEvent::PressEnter { .. } => ok(window, cx),
                    // the dialog's OK and error follow what is typed
                    InputEvent::Change => window.refresh(),
                    _ => {}
                }
            }
        })
        .detach();
    let field = input.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let tokens = Tokens::get(cx);
        let ok = ok.clone();
        let problem = error(&field.read(cx).value());
        dialog
            .title(spork_workbench::controls::dialog_title(title))
            .on_ok(|_, _, _| false)
            .close_button(false)
            .w(px(380.))
            .child(
                v_flex()
                    .id("project-group-dialog")
                    .role(Role::Group)
                    .aria_label(title)
                    .gap(px(8.))
                    .child(spork_workbench::controls::form_label(label))
                    .child(spork_workbench::TextField::new(&field).aria_label(label))
                    .when_some(problem.clone().filter(|_| show_error), |dialog, problem| {
                        dialog.child(
                            div()
                                .id("project-group-error")
                                .role(Role::Label)
                                .aria_label(problem.clone())
                                .text_color(tokens.text_error)
                                .child(problem),
                        )
                    })
                    .child(
                        h_flex()
                            .pt(px(8.))
                            .gap(px(8.))
                            .justify_end()
                            .child(
                                spork_workbench::Button::new("project-group-cancel", "Cancel")
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            )
                            .child(
                                spork_workbench::Button::new("project-group-ok", "OK")
                                    .primary(true)
                                    .disabled(problem.is_some())
                                    .on_click(move |_, window, cx| ok(window, cx)),
                            ),
                    ),
            )
    });
    input.update(cx, |input, cx| {
        input.focus(window, cx);
        input.select_all(window, cx);
    });
}

#[cfg(test)]
mod tests {
    use super::{ProjectGroup, move_to_group, rename_error};
    use std::path::PathBuf;

    fn group(name: &str, projects: &[&str]) -> ProjectGroup {
        ProjectGroup {
            name: name.into(),
            projects: projects.iter().map(|p| vec![PathBuf::from(p)]).collect(),
            expanded: false,
        }
    }

    #[test]
    fn moves_a_project_between_groups() {
        let mut groups = vec![group("a", &["/x"]), group("b", &[])];
        move_to_group(&mut groups, &[PathBuf::from("/x")], Some("b"));
        assert!(groups[0].projects.is_empty());
        assert_eq!(groups[1].projects, vec![vec![PathBuf::from("/x")]]);
        assert!(groups[1].expanded);
        move_to_group(&mut groups, &[PathBuf::from("/x")], None);
        assert!(groups.iter().all(|group| group.projects.is_empty()));
    }

    #[test]
    fn refuses_empty_and_taken_names() {
        let groups = vec![group("a", &[]), group("b", &[])];
        assert_eq!(
            rename_error(&groups, "a", " ").as_deref(),
            Some("Name cannot be empty")
        );
        assert_eq!(
            rename_error(&groups, "a", "b").as_deref(),
            Some("Group 'b' already exists.")
        );
        assert_eq!(rename_error(&groups, "a", "a"), None);
    }
}
