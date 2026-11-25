//! The Refactor menu over the language servers: Refactor This… (⌃T), Rename… (⇧F6) in place with
//! its popup and, pressed again, IntelliJ's Rename dialog with its Preview in the Find tool
//! window; Change Signature… (⌘F6) where a server offers it; Introduce Variable… (⌥⌘V), Introduce
//! Constant… (⌥⌘C), Extract Method… (⌥⌘M) and Inline… (⌥⌘N), each the server's code actions of
//! its kind (IntelliJ's `LspRefactoringSupportProvider` and `LspInlineActionHandler`).

use std::ops::Range;
use std::rc::Rc;

use editor::{Editor, EditorEvent, HighlightKey, SelectionEffects, ToOffset as _};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Context, Div, Entity, Global,
    InteractiveElement as _, IntoElement, ParentElement as _, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, TaskExt as _, WeakEntity, Window,
    div, px,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use language::{Buffer, ToOffset as _};
use lsp::LanguageServerId;
use project::{CodeAction, PrepareRenameResponse, Project};
use spork_navigate::{Occurrence, Usages};
use spork_workbench::controls::{dialog_title, form_label, labeled_check};
use spork_workbench::{Tokens, Typography as _, popup};
use util::ResultExt as _;
use workspace::Workspace;

use crate::{
    Able, Row, Target, action_kind, code, kind_covers, request_and_apply, request_code_actions,
    save_changed, show_hint, show_list_popup,
};

pub(crate) fn register(div: Div, able: &Able, cx: &mut Context<Workspace>) -> Div {
    div.when(able.editor && able.refactor, |div| {
        div.on_action(
            cx.listener(|workspace, _: &code::RefactorThis, window, cx| {
                refactor_this(workspace, window, cx)
            }),
        )
    })
    .when(able.editor && able.rename, |div| {
        div.on_action(
            cx.listener(|workspace, _: &code::Rename, window, cx| rename(workspace, window, cx)),
        )
    })
    .when(able.editor && able.change_signature, |div| {
        div.on_action(
            cx.listener(|workspace, _: &code::ChangeSignature, window, cx| {
                request_and_apply(
                    workspace,
                    "refactor",
                    |action| {
                        kind_covers("refactor.changeSignature", &action_kind(action))
                            || action
                                .lsp_action
                                .title()
                                .to_lowercase()
                                .contains("signature")
                    },
                    "Cannot perform refactoring.",
                    "Change Signature",
                    window,
                    cx,
                )
            }),
        )
    })
    .when(able.editor && able.extract, |div| {
        div.on_action(
            cx.listener(|workspace, _: &code::IntroduceVariable, window, cx| {
                extract(
                    workspace,
                    &["refactor.extract.variable", "refactor.extract.constant"],
                    &["variable"],
                    window,
                    cx,
                )
            }),
        )
        .on_action(
            cx.listener(|workspace, _: &code::IntroduceConstant, window, cx| {
                extract(
                    workspace,
                    &["refactor.extract.constant"],
                    &["constant"],
                    window,
                    cx,
                )
            }),
        )
        .on_action(
            cx.listener(|workspace, _: &code::ExtractMethod, window, cx| {
                extract(
                    workspace,
                    &["refactor.extract.function"],
                    &["function", "method"],
                    window,
                    cx,
                )
            }),
        )
    })
    .when(able.editor && able.inline, |div| {
        div.on_action(cx.listener(|workspace, _: &code::Inline, window, cx| {
            request_and_apply(
                workspace,
                "refactor.inline",
                |_| true,
                "No inline action is available here",
                "Inline",
                window,
                cx,
            )
        }))
    })
}

/// An Extract/Introduce refactoring: the server's `refactor.extract` actions of one of `kinds`, or
/// (a server that only says `refactor.extract`, rust-analyzer) whose title names what it
/// extracts into.
fn extract(
    workspace: &Workspace,
    kinds: &'static [&'static str],
    words: &'static [&'static str],
    window: &mut Window,
    cx: &mut App,
) {
    request_and_apply(
        workspace,
        "refactor.extract",
        move |action| {
            let kind = action_kind(action);
            let title = action.lsp_action.title().to_lowercase();
            kinds.iter().any(|wanted| kind_covers(wanted, &kind))
                || (kind == "refactor.extract" && words.iter().any(|word| title.contains(word)))
        },
        "No extract action is available here",
        "Extract",
        window,
        cx,
    )
}

/// Refactor This (`RefactoringQuickListPopupAction`): Rename… and Change Signature…, then the
/// server's refactorings at the caret, Extract/Introduce first, numbered.
fn refactor_this(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let Some(target) = Target::of(workspace, cx) else {
        return;
    };
    let able = Able::of(workspace, cx);
    let request = request_code_actions(&target, "refactor", |_| true, cx);
    let rename_key = crate::shortcut(&code::Rename, &target.editor, window, cx);
    let signature_key = crate::shortcut(&code::ChangeSignature, &target.editor, window, cx);
    let workspace = cx.entity().downgrade();
    window
        .spawn(cx, async move |cx| {
            let actions = request.await;
            cx.update(|window, cx| {
                let mut rows = Vec::new();
                if able.rename {
                    let workspace = workspace.clone();
                    rows.push(
                        Row::item("Rename…", move |window, cx| {
                            workspace
                                .update(cx, |workspace, cx| rename(workspace, window, cx))
                                .log_err();
                        })
                        .hint(rename_key.clone()),
                    );
                }
                let (signature, actions): (Vec<CodeAction>, Vec<CodeAction>) =
                    actions.into_iter().partition(|action| {
                        kind_covers("refactor.changeSignature", &action_kind(action))
                    });
                for action in signature {
                    let target = target.clone();
                    rows.push(
                        Row::item("Change Signature…", move |window, cx| {
                            crate::apply_code_action(&target, action.clone(), window, cx)
                        })
                        .hint(signature_key.clone()),
                    );
                }
                let groups: [(Option<&str>, &dyn Fn(&str) -> bool); 3] = [
                    (Some("Extract/Introduce"), &|kind| {
                        kind_covers("refactor.extract", kind)
                    }),
                    (None, &|kind| kind_covers("refactor.inline", kind)),
                    (None, &|kind| {
                        !kind_covers("refactor.extract", kind)
                            && !kind_covers("refactor.inline", kind)
                    }),
                ];
                for (title, in_group) in groups {
                    let group: Vec<&CodeAction> = actions
                        .iter()
                        .filter(|action| in_group(&action_kind(action)))
                        .collect();
                    if group.is_empty() {
                        continue;
                    }
                    if !rows.is_empty() || title.is_some() {
                        rows.push(Row::Separator(title.map(SharedString::from)));
                    }
                    for action in group {
                        let (target, action) = (target.clone(), action.clone());
                        rows.push(Row::item(
                            action.lsp_action.title().to_owned(),
                            move |window, cx| {
                                crate::apply_code_action(&target, action.clone(), window, cx)
                            },
                        ));
                    }
                }
                show_list_popup(
                    &workspace,
                    &target.editor,
                    "Refactor This",
                    rows,
                    true,
                    window,
                    cx,
                );
            })
        })
        .detach_and_log_err(cx);
}

impl Clone for Target {
    fn clone(&self) -> Self {
        Self {
            editor: self.editor.clone(),
            buffer: self.buffer.clone(),
            range: self.range.clone(),
            project: self.project.clone(),
        }
    }
}

/// An in-place rename (IntelliJ's `LspRenameHandler` template): the name edited where it is,
/// boxed; ⏎ renames it and its usages, ⎋ or the caret leaving it puts the old name back, ⇧F6
/// opens the Rename dialog.
struct InplaceRename {
    window: AnyWindowHandle,
    workspace: WeakEntity<Workspace>,
    editor: WeakEntity<Editor>,
    buffer: Entity<Buffer>,
    project: Entity<Project>,
    /// The name in the editor, growing as it is typed.
    range: Range<editor::Anchor>,
    /// Where the popup shows, under the name's start.
    origin: gpui::Point<gpui::Pixels>,
    old_name: String,
    server: Option<LanguageServerId>,
    _subscriptions: Vec<Subscription>,
}

#[derive(Default)]
struct Renaming(Option<InplaceRename>);

impl Global for Renaming {}

/// ⇧F6: starts the in-place rename of the name at the caret, or, during one, opens the Rename
/// dialog for it.
fn rename(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    if cx
        .try_global::<Renaming>()
        .and_then(|renaming| renaming.0.as_ref())
        .is_some_and(|renaming| renaming.window == window.window_handle())
    {
        if let Some((target, name, old_name, server)) = stop_inplace(cx) {
            rename_dialog(target, old_name, name, server, window, cx);
        }
        return;
    }
    let Some(target) = Target::of(workspace, cx) else {
        return;
    };
    let caret = target.caret(cx);
    let prepare = target.project.update(cx, |project, cx| {
        project.prepare_rename(target.buffer.clone(), caret, cx)
    });
    let workspace = cx.entity().downgrade();
    window
        .spawn(cx, async move |cx| {
            let prepared = prepare.await;
            cx.update(|window, cx| {
                let editor = target.editor.downgrade();
                let (range, server) = match prepared {
                    Ok(PrepareRenameResponse::Success {
                        range,
                        language_server_id,
                    }) => (range, language_server_id),
                    // a server without prepareRename renames the word at the caret
                    Ok(PrepareRenameResponse::OnlyUnpreparedRenameSupported) => {
                        let snapshot = target.buffer.read(cx).snapshot();
                        let (word, _) = snapshot.surrounding_word(caret.to_offset(&snapshot), None);
                        (
                            snapshot.anchor_before(word.start)..snapshot.anchor_after(word.end),
                            None,
                        )
                    }
                    Ok(PrepareRenameResponse::InvalidPosition) => {
                        return show_hint(
                            &editor,
                            "Cannot perform refactoring. Caret should be positioned at symbol to be renamed",
                            true,
                            window,
                            cx,
                        );
                    }
                    Err(error) => return show_hint(&editor, error.to_string(), true, window, cx),
                };
                start_inplace(workspace.clone(), target, range, server, window, cx);
            })
        })
        .detach_and_log_err(cx);
}

fn start_inplace(
    workspace: WeakEntity<Workspace>,
    target: Target,
    range: Range<text::Anchor>,
    server: Option<LanguageServerId>,
    window: &mut Window,
    cx: &mut App,
) {
    let editor = target.editor.clone();
    let snapshot = editor.read(cx).buffer().read(cx).snapshot(cx);
    let (Some(start), Some(end)) = (
        snapshot.anchor_in_excerpt(range.start),
        snapshot.anchor_in_excerpt(range.end),
    ) else {
        return;
    };
    // the box grows with what is typed at either end of the name
    let (start, end) = (start.to_offset(&snapshot), end.to_offset(&snapshot));
    let range = snapshot.anchor_before(start)..snapshot.anchor_after(end);
    let old_name: String = snapshot.text_for_range(start..end).collect();
    if old_name.is_empty() {
        return;
    }
    let Some(origin) = crate::anchor_origin(&editor, range.start, window, cx) else {
        return;
    };
    let subscriptions = editor.update(cx, |editor, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
            selections.select_ranges([start..end])
        });
        let tokens = Tokens::get(cx);
        let fill = tokens.live_template;
        editor.highlight_background(
            HighlightKey::Rename,
            std::slice::from_ref(&range),
            move |_, _| fill,
            cx,
        );
        let window_handle = window.window_handle();
        vec![
            // ⏎ and ⎋ end it, before the editor's own handlers
            editor.register_action(move |_: &editor::actions::Newline, window, cx| {
                finish_inplace(true, window, cx)
            }),
            editor.register_action(move |_: &editor::actions::Cancel, window, cx| {
                finish_inplace(false, window, cx)
            }),
            cx.subscribe_in(
                &cx.entity(),
                window,
                move |editor, _, event: &EditorEvent, window, cx| {
                    match event {
                        EditorEvent::SelectionsChanged { .. } => {
                            if !caret_in_name(editor, cx) {
                                finish_inplace(false, window, cx);
                            }
                        }
                        EditorEvent::BufferEdited => redraw(window_handle, cx),
                        // the focus going elsewhere in the window, not the window going behind
                        // another app's; the Rename dialog takes the name before the focus
                        EditorEvent::Blurred if window.is_window_active() => {
                            finish_inplace(false, window, cx)
                        }
                        _ => {}
                    }
                },
            ),
        ]
    });
    cx.set_global(Renaming(Some(InplaceRename {
        window: window.window_handle(),
        workspace: workspace.clone(),
        editor: editor.downgrade(),
        buffer: target.buffer.clone(),
        project: target.project,
        range,
        origin,
        old_name,
        server,
        _subscriptions: subscriptions,
    })));
    workspace.update(cx, |_, cx| cx.notify()).ok();
}

/// Whether the editor's caret is in the name being renamed.
fn caret_in_name(editor: &Editor, cx: &App) -> bool {
    let Some(renaming) = cx.try_global::<Renaming>().and_then(|r| r.0.as_ref()) else {
        return true;
    };
    let snapshot = editor.buffer().read(cx).snapshot(cx);
    let selection = editor.selections.newest_anchor();
    let (start, end) = (
        renaming.range.start.to_offset(&snapshot),
        renaming.range.end.to_offset(&snapshot),
    );
    let (head, tail) = (
        selection.head().to_offset(&snapshot),
        selection.tail().to_offset(&snapshot),
    );
    (start..=end).contains(&head) && (start..=end).contains(&tail)
}

fn redraw(window: AnyWindowHandle, cx: &mut App) {
    let workspace = cx
        .try_global::<Renaming>()
        .and_then(|renaming| renaming.0.as_ref())
        .filter(|renaming| renaming.window == window)
        .map(|renaming| renaming.workspace.clone());
    if let Some(workspace) = workspace {
        cx.defer(move |cx| {
            workspace.update(cx, |_, cx| cx.notify()).ok();
        });
    }
}

/// Ends the in-place rename: the old name goes back where it was typed over, as IntelliJ
/// restores the document; answers the target, the name typed, the old name and the server.
fn stop_inplace(cx: &mut App) -> Option<(Target, String, String, Option<LanguageServerId>)> {
    if !cx.has_global::<Renaming>() {
        return None;
    }
    let renaming = cx.global_mut::<Renaming>().0.take()?;
    let editor = renaming.editor.upgrade()?;
    let name = editor.update(cx, |editor, cx| {
        let snapshot = editor.buffer().read(cx).snapshot(cx);
        let range =
            renaming.range.start.to_offset(&snapshot)..renaming.range.end.to_offset(&snapshot);
        let name: String = snapshot.text_for_range(range.clone()).collect();
        editor.clear_background_highlights(HighlightKey::Rename, cx);
        if name != renaming.old_name {
            editor.buffer().update(cx, |buffer, cx| {
                buffer.edit([(range, renaming.old_name.clone())], None, cx)
            });
        }
        name
    });
    // ⇧F6 again stops it from within the workspace's update
    let workspace = renaming.workspace.clone();
    cx.defer(move |cx| {
        workspace.update(cx, |_, cx| cx.notify()).ok();
    });
    // where the name starts in its file, for the server's rename
    let (_, start) = editor
        .read(cx)
        .buffer()
        .read(cx)
        .text_anchor_for_position(renaming.range.start, cx)?;
    let target = Target {
        editor,
        buffer: renaming.buffer.clone(),
        range: start..start,
        project: renaming.project.clone(),
    };
    Some((target, name, renaming.old_name, renaming.server))
}

fn finish_inplace(rename: bool, window: &mut Window, cx: &mut App) {
    // a change from within an editor update runs after it
    window.defer(cx, move |window, cx| {
        let Some((target, name, old_name, server)) = stop_inplace(cx) else {
            return;
        };
        if rename && is_new_name(&name, &old_name) {
            perform_rename(&target, name, old_name, server, false, false, window, cx);
        }
    });
}

fn is_new_name(name: &str, old_name: &str) -> bool {
    !name.trim().is_empty() && name != old_name
}

/// Renames through the server, then, as asked, the old name in comments and strings and in the
/// files of other languages, and saves the files changed.
#[allow(clippy::too_many_arguments)]
fn perform_rename(
    target: &Target,
    name: String,
    old_name: String,
    server: Option<LanguageServerId>,
    in_comments: bool,
    in_text: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let position = target.range.start;
    let rename = target.project.update(cx, |project, cx| {
        project.perform_rename(target.buffer.clone(), position, name.clone(), server, cx)
    });
    let target = target.clone();
    window
        .spawn(cx, async move |cx| {
            let transaction = match rename.await {
                Ok(transaction) => transaction,
                Err(error) => {
                    let editor = target.editor.downgrade();
                    cx.update(|window, cx| show_hint(&editor, error.to_string(), true, window, cx))
                        .ok();
                    return Err(error);
                }
            };
            save_changed(&target.project, transaction, cx).await?;
            if in_comments || in_text {
                let language = cx.update(|_, cx| target.buffer.read(cx).language().cloned())?;
                let search = cx.update(|_, cx| {
                    non_code_occurrences(
                        &target.project,
                        &old_name,
                        language,
                        in_comments,
                        in_text,
                        cx,
                    )
                })?;
                let occurrences = search.await;
                let changed = cx.update(|_, cx| replace(&occurrences, &name, cx))?;
                target
                    .project
                    .update(cx, |project, cx| project.save_buffers(changed, cx))
                    .await?;
            }
            anyhow::Ok(())
        })
        .detach_and_log_err(cx);
}

/// The old name as a whole word where the server doesn't rename it: in comments and strings of
/// files of `language` (`in_comments`), anywhere in the files of other languages (`in_text`).
fn non_code_occurrences(
    project: &Entity<Project>,
    name: &str,
    language: Option<std::sync::Arc<language::Language>>,
    in_comments: bool,
    in_text: bool,
    cx: &mut App,
) -> gpui::Task<Vec<Occurrence>> {
    let Some(query) = project::search::SearchQuery::text(
        name,
        true,
        true,
        false,
        Default::default(),
        Default::default(),
        false,
        None,
    )
    .log_err() else {
        return gpui::Task::ready(Vec::new());
    };
    let search = spork_navigate::find_occurrences(project.clone(), query, 1000, cx);
    cx.spawn(async move |cx| {
        let (found, _) = search.await;
        cx.update(|cx| {
            found
                .into_iter()
                .filter(|occurrence| {
                    let buffer = occurrence.buffer.read(cx);
                    let same_language = match (buffer.language(), &language) {
                        (Some(found), Some(language)) => found.name() == language.name(),
                        _ => false,
                    };
                    match same_language {
                        true => {
                            in_comments && {
                                let snapshot = buffer.snapshot();
                                snapshot
                                    .language_scope_at(occurrence.range.start)
                                    .and_then(|scope| scope.override_name().map(str::to_owned))
                                    .is_some_and(|scope| {
                                        scope.starts_with("comment") || scope.starts_with("string")
                                    })
                            }
                        }
                        false => in_text,
                    }
                })
                .collect()
        })
    })
}

/// Replaces each occurrence with `name`; answers the files changed.
fn replace(
    occurrences: &[Occurrence],
    name: &str,
    cx: &mut App,
) -> collections::HashSet<Entity<Buffer>> {
    let mut changed = collections::HashSet::default();
    for occurrence in occurrences {
        occurrence.buffer.update(cx, |buffer, cx| {
            buffer.edit([(occurrence.range.clone(), name)], None, cx)
        });
        changed.insert(occurrence.buffer.clone());
    }
    changed
}

/// The in-place rename's popup under the name: the name it will get and IntelliJ's
/// advertisement for the dialog (`inplace.refactoring.advertisement.text`).
pub(crate) fn render_rename_popup(window: &Window, cx: &App) -> Option<AnyElement> {
    let renaming = cx.try_global::<Renaming>()?.0.as_ref()?;
    if renaming.window != window.window_handle() {
        return None;
    }
    let editor = renaming.editor.upgrade()?;
    let snapshot = editor.read(cx).buffer().read(cx).snapshot(cx);
    let name: String = snapshot
        .text_for_range(
            renaming.range.start.to_offset(&snapshot)..renaming.range.end.to_offset(&snapshot),
        )
        .collect();
    let advertisement = format!(
        "Press {} to show dialog with more options",
        crate::shortcut(&code::Rename, &editor, window, cx).unwrap_or_else(|| "⇧F6".into())
    );
    let tokens = Tokens::get(cx);
    let content = popup::surface(cx)
        .id("rename-popup")
        .role(Role::Menu)
        .aria_label("Rename")
        .min_w(px(180.))
        .pt(px(4.))
        .child(
            popup::row("rename-name", true, cx)
                .role(Role::MenuItem)
                .aria_label(SharedString::from(name.clone()))
                .child(name),
        )
        .child(
            div()
                .id("rename-advertisement")
                .role(Role::Label)
                .aria_label(SharedString::from(advertisement.clone()))
                .mt(px(4.))
                .px(px(12.))
                .py(px(4.))
                .border_t_1()
                .border_color(tokens.popup_border)
                .text_ui_small()
                .text_color(tokens.text_secondary)
                .child(advertisement),
        );
    // the suggestion's text over the name's, as IntelliJ's lookup lines them up: the row's 12 px
    // and the popup's border to the left
    let origin = renaming.origin - gpui::point(px(13.), px(0.));
    Some(crate::anchored_popup(origin, content, cx).into_any_element())
}

/// IntelliJ's Rename dialog (`RenameDialog`): the new name, Search in comments and strings,
/// Search for text occurrences, Preview, Cancel and Refactor.
fn rename_dialog(
    target: Target,
    old_name: String,
    name: String,
    server: Option<LanguageServerId>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let input = cx.new(|cx| InputState::new(window, cx).default_value(name.clone()));
    let options = cx.new(|_| (false, false));
    let label = format!("Rename '{old_name}' and its usages to:");
    let workspace_handle = cx.entity().downgrade();
    let refactor = {
        let (input, options, target, old_name) = (
            input.clone(),
            options.clone(),
            target.clone(),
            old_name.clone(),
        );
        Rc::new(move |window: &mut Window, cx: &mut App| {
            let name = input.read(cx).value().trim().to_owned();
            if !is_new_name(&name, &old_name) {
                return;
            }
            let (in_comments, in_text) = *options.read(cx);
            window.close_dialog(cx);
            perform_rename(
                &target,
                name,
                old_name.clone(),
                server,
                in_comments,
                in_text,
                window,
                cx,
            );
        })
    };
    let preview = {
        let (input, options, target, old_name) =
            (input.clone(), options.clone(), target, old_name.clone());
        Rc::new(move |window: &mut Window, cx: &mut App| {
            let name = input.read(cx).value().trim().to_owned();
            if !is_new_name(&name, &old_name) {
                return;
            }
            let (in_comments, in_text) = *options.read(cx);
            // the dialog's choices, kept for the preview's Refactor
            let (target, old_name) = (target.clone(), old_name.clone());
            let references = target.project.update(cx, |project, cx| {
                project.references(&target.buffer, target.range.start, cx)
            });
            let language = target.buffer.read(cx).language().cloned();
            let others = non_code_occurrences(
                &target.project,
                &old_name,
                language,
                in_comments,
                in_text,
                cx,
            );
            let workspace = workspace_handle.clone();
            window.close_dialog(cx);
            window
                .spawn(cx, async move |cx| {
                    let references = references.await.log_err().flatten().unwrap_or_default();
                    let others = others.await;
                    cx.update(|window, cx| {
                        let mut occurrences: Vec<Occurrence> = references
                            .into_iter()
                            .filter_map(|location| {
                                Occurrence::new(&location.buffer, location.range, cx)
                            })
                            .collect();
                        occurrences.extend(others);
                        let do_refactor: Rc<dyn Fn(&mut Window, &mut App)> = {
                            let (target, name, old_name) =
                                (target.clone(), name.clone(), old_name.clone());
                            Rc::new(move |window, cx| {
                                perform_rename(
                                    &target,
                                    name.clone(),
                                    old_name.clone(),
                                    server,
                                    in_comments,
                                    in_text,
                                    window,
                                    cx,
                                )
                            })
                        };
                        let usages = Usages {
                            title: "Refactoring Preview".into(),
                            description: format!("Rename '{old_name}' to '{name}'").into(),
                            root: "References to be changed".into(),
                            occurrences,
                            rerun: None,
                            refactor: Some(do_refactor),
                        };
                        workspace
                            .update(cx, |workspace, cx| {
                                spork_navigate::show_usages(workspace, usages, window, cx)
                            })
                            .log_err();
                    })
                })
                .detach_and_log_err(cx);
        })
    };
    window
        .subscribe(&input, cx, {
            let refactor = refactor.clone();
            move |_, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => refactor(window, cx),
                InputEvent::Change => window.refresh(),
                _ => {}
            }
        })
        .detach();
    let field = input.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let ready = is_new_name(field.read(cx).value().trim(), &old_name);
        let (in_comments, in_text) = *options.read(cx);
        let (refactor, preview) = (refactor.clone(), preview.clone());
        let toggle = |which: usize| {
            let options = options.clone();
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                options.update(cx, |options, _| match which {
                    0 => options.0 = !options.0,
                    _ => options.1 = !options.1,
                });
                window.refresh();
            }
        };
        dialog
            .title(dialog_title("Rename"))
            .on_ok(|_, _, _| false)
            .w(px(520.))
            .close_button(false)
            .child(
                v_flex()
                    .id("rename-dialog")
                    .role(Role::Group)
                    .aria_label("Rename")
                    .gap(px(10.))
                    .child(form_label(label.clone()))
                    .child(spork_workbench::TextField::new(&field).aria_label(label.clone()))
                    .child(
                        h_flex()
                            .gap(px(16.))
                            .child(
                                labeled_check(
                                    "rename-in-comments",
                                    "Search in comments and strings",
                                    in_comments,
                                    cx,
                                )
                                .on_click(toggle(0)),
                            )
                            .child(
                                labeled_check(
                                    "rename-text-occurrences",
                                    "Search for text occurrences",
                                    in_text,
                                    cx,
                                )
                                .on_click(toggle(1)),
                            ),
                    )
                    .child(
                        h_flex()
                            .pt(px(8.))
                            .gap(px(8.))
                            .justify_end()
                            .child(
                                spork_workbench::Button::new("rename-preview", "Preview")
                                    .disabled(!ready)
                                    .on_click(move |_, window, cx| preview(window, cx)),
                            )
                            .child(
                                spork_workbench::Button::new("dialog-cancel", "Cancel")
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            )
                            .child(
                                spork_workbench::Button::new("dialog-ok", "Refactor")
                                    .primary(true)
                                    .disabled(!ready)
                                    .on_click(move |_, window, cx| refactor(window, cx)),
                            ),
                    ),
            )
    });
    input.update(cx, |input, cx| input.focus(window, cx));
}

/// The name an in-place rename shows, for the end-to-end tests.
pub(crate) fn renaming_name(cx: &App) -> Option<String> {
    let renaming = cx.try_global::<Renaming>()?.0.as_ref()?;
    let editor = renaming.editor.upgrade()?;
    let snapshot = editor.read(cx).buffer().read(cx).snapshot(cx);
    Some(
        snapshot
            .text_for_range(
                renaming.range.start.to_offset(&snapshot)..renaming.range.end.to_offset(&snapshot),
            )
            .collect(),
    )
}
