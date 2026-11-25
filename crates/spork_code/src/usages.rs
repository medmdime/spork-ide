//! Navigation over the language servers: Go to Declaration or Usages (⌘B), Implementation(s)
//! (⌥⌘B) with IntelliJ's chooser when several, Type Declaration (⌃⇧B), Super Method (⌘U), Show
//! Usages (⌥⌘F7, IntelliJ's usages popup with its preview), Find Usages (⌥F7, into the Find tool
//! window) and Quick Definition (⌥Space).

use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Context, DismissEvent, Div, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Point, Render, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, Task, TaskExt as _, WeakEntity,
    Window, div, px,
};
use gpui_kit::component::{h_flex, v_flex};
use language::{Buffer, ToOffset as _};
use picker::{PreviewBackend, PreviewLayout};
use project::{LocationLink, Project};
use settings::Settings as _;
use spork_navigate::{Occurrence, Usages};
use spork_workbench::{
    ActionButton, Tokens, TreeRow, TreeState, TreeView, Typography as _, popup, tree_keys,
};
use theme::ActiveTheme as _;
use theme_settings::ThemeSettings;
use util::ResultExt as _;
use workspace::{ModalView, Workspace};

use crate::{Able, Row, Target, caret_origin, code, open_location, show_hint, show_list_popup};

pub(crate) fn register(div: Div, able: &Able, cx: &mut Context<Workspace>) -> Div {
    div.when(able.editor && able.definition, |div| {
        div.on_action(
            cx.listener(|workspace, _: &code::GotoDeclaration, window, cx| {
                go_to(workspace, Kind::Declaration, window, cx)
            }),
        )
        .on_action(
            cx.listener(|workspace, _: &code::QuickDefinition, window, cx| {
                quick_definition(workspace, window, cx)
            }),
        )
    })
    .when(able.editor && able.implementation, |div| {
        div.on_action(
            cx.listener(|workspace, _: &code::GotoImplementation, window, cx| {
                go_to(workspace, Kind::Implementation, window, cx)
            }),
        )
    })
    .when(able.editor && able.type_definition, |div| {
        div.on_action(
            cx.listener(|workspace, _: &code::GotoTypeDeclaration, window, cx| {
                go_to(workspace, Kind::Type, window, cx)
            }),
        )
    })
    .when(able.editor && able.declaration, |div| {
        div.on_action(
            cx.listener(|workspace, _: &code::GotoSuperMethod, window, cx| {
                go_to(workspace, Kind::Super, window, cx)
            }),
        )
    })
    .when(able.editor && able.references, |div| {
        div.on_action(cx.listener(|workspace, _: &code::ShowUsages, window, cx| {
            show_usages(workspace, window, cx)
        }))
        .on_action(cx.listener(|workspace, _: &code::FindUsages, window, cx| {
            find_usages(workspace, window, cx)
        }))
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Declaration,
    Implementation,
    Type,
    Super,
}

/// The name at the caret, for the popups' titles.
pub(crate) fn name_at(buffer: &Entity<Buffer>, position: text::Anchor, cx: &App) -> String {
    let snapshot = buffer.read(cx).snapshot();
    let (word, _) = snapshot.surrounding_word(position.to_offset(&snapshot), None);
    snapshot.text_for_range(word).collect()
}

/// Whether a link points at the caret: the caret is on the declaration itself.
fn at_caret(link: &LocationLink, buffer: &Entity<Buffer>, caret: text::Anchor, cx: &App) -> bool {
    if &link.target.buffer != buffer {
        return false;
    }
    let snapshot = buffer.read(cx).snapshot();
    let caret = caret.to_offset(&snapshot);
    let range =
        link.target.range.start.to_offset(&snapshot)..link.target.range.end.to_offset(&snapshot);
    range.start <= caret && caret <= range.end
}

/// Goes to what `kind` finds at the caret: one place goes there, several open IntelliJ's
/// chooser; ⌘B on a declaration shows its usages instead.
fn go_to(workspace: &mut Workspace, kind: Kind, window: &mut Window, cx: &mut Context<Workspace>) {
    let Some(target) = Target::of(workspace, cx) else {
        return;
    };
    let caret = target.caret(cx);
    let request = target.project.update(cx, |project, cx| match kind {
        Kind::Declaration => project.definitions(&target.buffer, caret, cx),
        Kind::Implementation => project.implementations(&target.buffer, caret, cx),
        Kind::Type => project.type_definitions(&target.buffer, caret, cx),
        Kind::Super => project.declarations(&target.buffer, caret, cx),
    });
    let name = name_at(&target.buffer, caret, cx);
    let workspace_handle = cx.entity().downgrade();
    window
        .spawn(cx, async move |cx| {
            let links = request.await.log_err().flatten().unwrap_or_default();
            cx.update(|window, cx| {
                let on_itself = !links.is_empty()
                    && links
                        .iter()
                        .all(|link| at_caret(link, &target.buffer, caret, cx));
                if kind == Kind::Declaration && on_itself {
                    return workspace_handle
                        .update(cx, |workspace, cx| show_usages(workspace, window, cx))
                        .log_err()
                        .unwrap_or_default();
                }
                let mut links: Vec<LocationLink> = links
                    .into_iter()
                    .filter(|link| !at_caret(link, &target.buffer, caret, cx))
                    .collect();
                // in file order, as IntelliJ's chooser lists them
                links.sort_by_cached_key(|link| {
                    let buffer = link.target.buffer.read(cx);
                    (
                        buffer.file().map(|file| file.path().clone()),
                        link.target.range.start.to_offset(&buffer.snapshot()),
                    )
                });
                let editor = target.editor.downgrade();
                match (links.len(), kind) {
                    (0, Kind::Super) => {}
                    (0, Kind::Implementation) => {
                        show_hint(&editor, "No implementations found", true, window, cx)
                    }
                    (0, _) => show_hint(
                        &editor,
                        "Cannot find declaration to go to",
                        true,
                        window,
                        cx,
                    ),
                    (1, _) => {
                        if let Some(link) = links.into_iter().next() {
                            workspace_handle
                                .update(cx, |workspace, cx| {
                                    open_location(
                                        workspace,
                                        link.target.buffer,
                                        link.target.range,
                                        window,
                                        cx,
                                    )
                                })
                                .log_err();
                        }
                    }
                    (count, _) => {
                        let title = match kind {
                            Kind::Implementation => {
                                format!("Choose Implementation of {name} ({count} found)")
                            }
                            Kind::Super => "Choose super method".to_owned(),
                            _ => "Choose Declaration".to_owned(),
                        };
                        let rows = links
                            .into_iter()
                            .filter_map(|link| {
                                let occurrence = Occurrence::new(
                                    &link.target.buffer,
                                    link.target.range.clone(),
                                    cx,
                                )?;
                                let workspace = workspace_handle.clone();
                                let hint = format!(
                                    "{}:{}",
                                    occurrence.file_name(),
                                    occurrence.point.row + 1
                                );
                                Some(
                                    Row::item(occurrence.line, move |window, cx| {
                                        workspace
                                            .update(cx, |workspace, cx| {
                                                open_location(
                                                    workspace,
                                                    link.target.buffer.clone(),
                                                    link.target.range.clone(),
                                                    window,
                                                    cx,
                                                )
                                            })
                                            .log_err();
                                    })
                                    .hint(Some(hint.into())),
                                )
                            })
                            .collect();
                        show_list_popup(
                            &workspace_handle,
                            &target.editor,
                            title,
                            rows,
                            false,
                            window,
                            cx,
                        );
                    }
                }
            })
        })
        .detach_and_log_err(cx);
}

/// The usages of the symbol at `position`: its references without its declarations, IntelliJ's
/// usages, in file order.
fn usages_of(
    project: &Entity<Project>,
    buffer: &Entity<Buffer>,
    position: text::Anchor,
    cx: &mut App,
) -> Task<Vec<Occurrence>> {
    let (references, definitions) = project.update(cx, |project, cx| {
        (
            project.references(buffer, position, cx),
            project.definitions(buffer, position, cx),
        )
    });
    cx.spawn(async move |cx| {
        let references = references.await.log_err().flatten().unwrap_or_default();
        let definitions = definitions.await.log_err().flatten().unwrap_or_default();
        cx.update(|cx| {
            let is_declaration = |location: &language::Location| {
                definitions.iter().any(|link| {
                    link.target.buffer == location.buffer && {
                        let snapshot = location.buffer.read(cx).snapshot();
                        link.target.range.start.to_offset(&snapshot)
                            == location.range.start.to_offset(&snapshot)
                    }
                })
            };
            let mut occurrences: Vec<Occurrence> = references
                .into_iter()
                .filter(|location| !is_declaration(location))
                .filter_map(|location| Occurrence::new(&location.buffer, location.range, cx))
                .collect();
            occurrences.sort_by(|a, b| {
                (a.path.path.as_unix_str(), a.point).cmp(&(b.path.path.as_unix_str(), b.point))
            });
            occurrences.dedup_by(|a, b| a.path == b.path && a.point == b.point);
            occurrences
        })
    })
}

/// ⌥F7: the usages in a tab of the Find tool window, which Rerun searches again.
fn find_usages(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let Some(target) = Target::of(workspace, cx) else {
        return;
    };
    let caret = target.caret(cx);
    let name = name_at(&target.buffer, caret, cx);
    let search = usages_of(&target.project, &target.buffer, caret, cx);
    let workspace_handle = cx.entity().downgrade();
    window
        .spawn(cx, async move |cx| {
            let occurrences = search.await;
            cx.update(|window, cx| {
                let rerun = {
                    let (project, buffer) = (target.project.clone(), target.buffer.clone());
                    std::rc::Rc::new(move |_: &mut Window, cx: &mut App| {
                        usages_of(&project, &buffer, caret, cx)
                    })
                };
                let usages = Usages {
                    title: format!("Usages of {name}").into(),
                    description: format!("Usages of {name} in Project Files").into(),
                    root: "Usages".into(),
                    occurrences,
                    rerun: Some(rerun),
                    refactor: None,
                };
                workspace_handle
                    .update(cx, |workspace, cx| {
                        spork_navigate::show_usages(workspace, usages, window, cx)
                    })
                    .log_err();
            })
        })
        .detach_and_log_err(cx);
}

/// ⌥⌘F7: IntelliJ's Show Usages popup under the caret.
fn show_usages(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let Some(target) = Target::of(workspace, cx) else {
        return;
    };
    let caret = target.caret(cx);
    let name = name_at(&target.buffer, caret, cx);
    let search = usages_of(&target.project, &target.buffer, caret, cx);
    let workspace_handle = cx.entity().downgrade();
    window
        .spawn(cx, async move |cx| {
            let occurrences = search.await;
            cx.update(|window, cx| {
                let editor = target.editor.downgrade();
                if occurrences.is_empty() {
                    return show_hint(
                        &editor,
                        "No usages found in Project Files",
                        false,
                        window,
                        cx,
                    );
                }
                let Some(origin) = caret_origin(&target.editor, window, cx) else {
                    return;
                };
                let project = target.project.clone();
                let (buffer, workspace) = (target.buffer.clone(), workspace_handle.clone());
                workspace_handle
                    .update(cx, |workspace_ref, cx| {
                        workspace_ref.toggle_modal(window, cx, move |window, cx| {
                            let preview =
                                picker_preview::editor_preview(project.clone(), window, cx);
                            let mut popup = UsagesPopup {
                                name: name.into(),
                                occurrences,
                                selected: {
                                    let mut selected = TreeState::list();
                                    selected.select(Some(0));
                                    selected
                                },
                                origin,
                                focus: cx.focus_handle(),
                                preview,
                                workspace,
                                project,
                                buffer,
                                caret,
                            };
                            popup.update_preview(window, cx);
                            popup
                        })
                    })
                    .log_err();
            })
        })
        .detach_and_log_err(cx);
}

impl TreeView for UsagesPopup {
    type Key = usize;

    fn tree(&mut self) -> &mut TreeState<usize> {
        &mut self.selected
    }

    fn tree_rows(&self, _: &App) -> Vec<TreeRow<usize>> {
        self.occurrences
            .iter()
            .enumerate()
            .map(|(ix, occurrence)| TreeRow::item(ix, occurrence.line.clone()))
            .collect()
    }

    fn selection_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.update_preview(window, cx);
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.selected.lead().copied() {
            self.choose(ix, window, cx);
        }
    }

    // shortcut: no speed search yet; IntelliJ's filters the usages, worth it with the popup's
    // filters
    fn speed_search(&self) -> bool {
        false
    }
}

/// IntelliJ's Show Usages popup (`ShowUsagesAction`): its header (the title, the count, Open in
/// Find Tool Window), a row per usage (its file's icon, its line with the name marked, the file
/// and line), and the selected usage's code under them.
struct UsagesPopup {
    name: SharedString,
    occurrences: Vec<Occurrence>,
    /// The selected usage, by its place in `occurrences`.
    selected: TreeState<usize>,
    origin: Point<Pixels>,
    focus: FocusHandle,
    preview: Arc<dyn PreviewBackend>,
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    buffer: Entity<Buffer>,
    caret: text::Anchor,
}

impl EventEmitter<DismissEvent> for UsagesPopup {}

impl ModalView for UsagesPopup {}

impl Focusable for UsagesPopup {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl UsagesPopup {
    fn update_preview(&mut self, window: &mut Window, cx: &mut App) {
        match self
            .selected
            .lead()
            .and_then(|ix| self.occurrences.get(*ix))
        {
            Some(occurrence) => self.preview.update(occurrence.preview(cx), window, cx),
            None => self.preview.clear(cx),
        }
    }

    fn choose(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(occurrence) = self.occurrences.get(ix).cloned() else {
            return;
        };
        cx.emit(DismissEvent);
        self.workspace
            .update(cx, |workspace, cx| {
                open_location(workspace, occurrence.buffer, occurrence.range, window, cx)
            })
            .log_err();
    }

    /// Open in Find Tool Window: the same usages in a tab of the Find tool window.
    fn open_in_find_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
        let (project, buffer, caret) = (self.project.clone(), self.buffer.clone(), self.caret);
        let usages = Usages {
            title: format!("Usages of {}", self.name).into(),
            description: format!("Usages of {} in Project Files", self.name).into(),
            root: "Usages".into(),
            occurrences: self.occurrences.clone(),
            rerun: Some(std::rc::Rc::new(move |_: &mut Window, cx: &mut App| {
                usages_of(&project, &buffer, caret, cx)
            })),
            refactor: None,
        };
        self.workspace
            .update(cx, |workspace, cx| {
                spork_navigate::show_usages(workspace, usages, window, cx)
            })
            .log_err();
    }
}

impl Render for UsagesPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let count = match self.occurrences.len() {
            1 => "1 usage".to_owned(),
            n => format!("{n} usages"),
        };
        let title = format!("Usages of {}", self.name);
        let rows: Vec<AnyElement> = self
            .occurrences
            .iter()
            .enumerate()
            .map(|(ix, occurrence)| {
                let selected = self.selected.lead() == Some(&ix);
                let line = &occurrence.line;
                let found = occurrence.found.clone();
                let (before, at, after) = match (
                    line.get(..found.start),
                    line.get(found.clone()),
                    line.get(found.end..),
                ) {
                    (Some(before), Some(at), Some(after)) => (before, at, after),
                    _ => (line.as_ref(), "", ""),
                };
                let file = occurrence.file_name();
                popup::row(("usage", ix), selected, cx)
                    .role(Role::MenuItem)
                    .aria_label(line.clone())
                    .aria_selected(selected)
                    .child(spork_workbench::image(
                        spork_workbench::file_icon(&file),
                        cx,
                    ))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(ThemeSettings::get_global(cx).buffer_font.family.clone())
                            .child(before.to_owned())
                            .child(
                                div()
                                    .bg(cx.theme().colors().search_match_background)
                                    .child(at.to_owned()),
                            )
                            .child(after.to_owned()),
                    )
                    .child(popup::hint(
                        format!("{file} {}", occurrence.point.row + 1),
                        selected,
                        cx,
                    ))
                    .on_click(
                        cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                            this.selected.select(Some(ix));
                            this.update_preview(window, cx);
                            if event.click_count() >= 2 {
                                this.choose(ix, window, cx);
                            }
                            cx.notify();
                        }),
                    )
                    .into_any_element()
            })
            .collect();
        let surface = tree_keys(
            popup::surface(cx)
                .id("show-usages")
                .role(Role::Dialog)
                .aria_label(SharedString::from(title.clone()))
                .w(px(640.))
                .track_focus(&self.focus)
                .key_context("ShowUsages menu tree"),
            cx,
        )
        .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            h_flex()
                .flex_none()
                .h(px(32.))
                .pl(px(12.))
                .pr(px(8.))
                .gap(px(8.))
                .border_b_1()
                .border_color(tokens.popup_border)
                .child(
                    div()
                        .id("show-usages-title")
                        .role(Role::Label)
                        .aria_label(SharedString::from(title.clone()))
                        .font_weight(gpui::FontWeight::BOLD)
                        .child(title),
                )
                .child(
                    div()
                        .id("show-usages-count")
                        .role(Role::Label)
                        .aria_label(SharedString::from(count.clone()))
                        .text_color(tokens.text_secondary)
                        .child(count),
                )
                .child(div().flex_1())
                .child(
                    ActionButton::new("show-usages-open-find", "general/pin")
                        .tooltip("Open in Find Tool Window")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_in_find_window(window, cx)),
                        ),
                ),
        )
        .child(
            v_flex()
                .id("show-usages-rows")
                .role(Role::List)
                .aria_label("Usages")
                .max_h(px(300.))
                .py(px(4.))
                .track_scroll(self.selected.scroll_handle())
                .overflow_y_scroll()
                .children(rows),
        )
        .child(
            div()
                .h(px(200.))
                .border_t_1()
                .border_color(tokens.popup_border)
                .child(self.preview.render(PreviewLayout::Below, cx)),
        );
        crate::anchored_popup(self.origin, surface, cx)
    }
}

/// ⌥Space: IntelliJ's Quick Definition popup (`ImplementationViewComponent`), the definition's
/// file and its code, Edit Source going there.
fn quick_definition(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let Some(target) = Target::of(workspace, cx) else {
        return;
    };
    let caret = target.caret(cx);
    let request = target.project.update(cx, |project, cx| {
        project.definitions(&target.buffer, caret, cx)
    });
    let name = name_at(&target.buffer, caret, cx);
    let workspace_handle = cx.entity().downgrade();
    window
        .spawn(cx, async move |cx| {
            let links = request.await.log_err().flatten().unwrap_or_default();
            cx.update(|window, cx| {
                let editor = target.editor.downgrade();
                let Some(occurrence) = links
                    .into_iter()
                    .find_map(|link| Occurrence::new(&link.target.buffer, link.target.range, cx))
                else {
                    return show_hint(
                        &editor,
                        "Cannot find declaration to go to",
                        true,
                        window,
                        cx,
                    );
                };
                let Some(origin) = caret_origin(&target.editor, window, cx) else {
                    return;
                };
                let project = target.project.clone();
                let workspace = workspace_handle.clone();
                workspace_handle
                    .update(cx, |workspace_ref, cx| {
                        workspace_ref.toggle_modal(window, cx, move |window, cx| {
                            let preview = picker_preview::editor_preview(project, window, cx);
                            preview.update(occurrence.preview(cx), window, cx);
                            QuickDefinition {
                                name: name.into(),
                                occurrence,
                                origin,
                                focus: cx.focus_handle(),
                                preview,
                                workspace,
                            }
                        })
                    })
                    .log_err();
            })
        })
        .detach_and_log_err(cx);
}

struct QuickDefinition {
    name: SharedString,
    occurrence: Occurrence,
    origin: Point<Pixels>,
    focus: FocusHandle,
    preview: Arc<dyn PreviewBackend>,
    workspace: WeakEntity<Workspace>,
}

impl EventEmitter<DismissEvent> for QuickDefinition {}

impl ModalView for QuickDefinition {}

impl Focusable for QuickDefinition {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl QuickDefinition {
    fn edit_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
        let occurrence = self.occurrence.clone();
        self.workspace
            .update(cx, |workspace, cx| {
                open_location(workspace, occurrence.buffer, occurrence.range, window, cx)
            })
            .log_err();
    }
}

impl Render for QuickDefinition {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let title = format!("Definition of {}", self.name);
        let file = self.occurrence.file_name();
        let location = format!("{file}:{}", self.occurrence.point.row + 1);
        let surface = popup::surface(cx)
            .id("quick-definition")
            .role(Role::Dialog)
            .aria_label(SharedString::from(title))
            .w(px(600.))
            .track_focus(&self.focus)
            .key_context("QuickDefinition menu")
            .on_action(
                cx.listener(|this, _: &menu::Confirm, window, cx| this.edit_source(window, cx)),
            )
            .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                h_flex()
                    .flex_none()
                    .h(px(32.))
                    .pl(px(12.))
                    .pr(px(8.))
                    .gap(px(6.))
                    .border_b_1()
                    .border_color(tokens.popup_border)
                    .child(spork_workbench::image(
                        spork_workbench::file_icon(&file),
                        cx,
                    ))
                    .child(
                        div()
                            .id("quick-definition-location")
                            .role(Role::Label)
                            .aria_label(SharedString::from(location.clone()))
                            .text_ui()
                            .child(location),
                    )
                    .child(div().flex_1())
                    .child(
                        ActionButton::new("quick-definition-edit-source", "general/edit")
                            .tooltip("Edit Source")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.edit_source(window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .h(px(220.))
                    .child(self.preview.render(PreviewLayout::Below, cx)),
            );
        crate::anchored_popup(self.origin, surface, cx)
    }
}
