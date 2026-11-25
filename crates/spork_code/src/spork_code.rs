//! IntelliJ's code intelligence over Zed's language servers (ROADMAP, phase 7): the Code menu's
//! Reformat Code, Reformat File…, Optimize Imports, Generate… and Surround With…; the Refactor
//! menu's Refactor This…, Rename… (in place, then IntelliJ's Rename dialog), Change Signature…,
//! Extract/Introduce and Inline…; navigation to declarations, implementations, type declarations
//! and super methods, Show Usages, Find Usages into the Find tool window, Quick Definition, the
//! Hierarchy tool window (Call Hierarchy, Type Hierarchy); and Inspect Code….
//!
//! As IntelliJ's own LSP support does (`platform/lsp-impl`), each refactoring is the server's code
//! actions of its kind at the caret: none shows a hint, one runs, several open a chooser; an item
//! the active file's servers can't do is greyed, as IntelliJ greys an unavailable action.

mod code_menu;
mod hierarchy;
pub mod html;
pub mod inspection;
mod refactor;
mod related;
mod usages;

use std::ops::Range;
use std::time::Duration;

use collections::HashSet;
use editor::{Editor, SelectionEffects, scroll::Autoscroll};
use gpui::{
    AnyElement, App, Entity, Focusable, Global, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Point, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Task, TaskExt as _, WeakEntity, Window, anchored, deferred, div, point, px,
};
use language::{Buffer, ToPoint as _};
use project::{CodeAction, Project, ProjectTransaction};
use spork_workbench::{Tokens, Typography as _};
use util::ResultExt as _;
use workspace::Workspace;

pub use spork_navigate::list_popup::Row;
pub(crate) use spork_navigate::list_popup::{
    anchor_origin, anchored_popup, caret_origin, show_list_popup,
};

/// The Generate popup's rows of a language whose plugin has its own (IntelliJ's Java
/// `GenerateGroup`: Constructor, Getter…), in place of its servers' generating code actions;
/// `None` for a file it doesn't handle.
pub type GenerateRows = fn(&Target, &mut Window, &mut App) -> Option<Task<Vec<Row>>>;

/// The languages with rows of their own, by name.
#[derive(Default)]
struct GenerateProviders(Vec<(&'static str, GenerateRows)>);

impl Global for GenerateProviders {}

/// Gives a language its own rows in the Generate popup (`spork_java`'s for Java), which then
/// shows in its files with or without a language server.
pub fn add_generate_rows(language: &'static str, rows: GenerateRows, cx: &mut App) {
    cx.default_global::<GenerateProviders>()
        .0
        .push((language, rows));
}

/// The rows of `buffer`'s language, when it has its own.
fn generate_provider(buffer: &Entity<Buffer>, cx: &App) -> Option<GenerateRows> {
    let language = buffer.read(cx).language()?.name();
    cx.try_global::<GenerateProviders>()?
        .0
        .iter()
        .find(|(name, _)| *name == language.as_ref())
        .map(|(_, rows)| *rows)
}

/// The rows a language's plugin gives the Generate popup for the target, if it has its own.
pub(crate) fn language_generate_rows(
    target: &Target,
    window: &mut Window,
    cx: &mut App,
) -> Option<Task<Vec<Row>>> {
    generate_provider(&target.buffer, cx)?(target, window, cx)
}

pub mod code {
    gpui::actions!(
        code,
        [
            /// Reformats the selection, or the file without one (IntelliJ's ⌥⌘L).
            ReformatCode,
            /// Opens IntelliJ's Reformat File dialog: its scope and options.
            ReformatFile,
            /// Sorts the file's imports and removes the unused ones, the server's organize
            /// imports.
            OptimizeImports,
            /// Shows the Generate popup: the code the server can generate at the caret.
            Generate,
            /// Shows the Surround With popup: what the server can wrap the selection in.
            SurroundWith,
            /// Moves the statement at the caret below the next one.
            MoveStatementDown,
            /// Moves the statement at the caret above the one before it.
            MoveStatementUp,
            /// Runs the language servers' diagnostics over a scope and shows the problems.
            InspectCode,
            /// Shows the Refactor This popup: the refactorings the server offers at the caret.
            RefactorThis,
            /// Renames the symbol at the caret in place; pressed again, opens the Rename dialog.
            Rename,
            /// Changes the signature of the function at the caret, where the server offers it.
            ChangeSignature,
            /// Extracts the selected expression into a variable.
            IntroduceVariable,
            /// Extracts the selected expression into a constant.
            IntroduceConstant,
            /// Extracts the selected statements into a function.
            ExtractMethod,
            /// Inlines the variable or function at the caret.
            Inline,
            /// Goes to the declaration of the symbol at the caret, or shows its usages there.
            GotoDeclaration,
            /// Goes to the implementation of the symbol at the caret, a chooser when several.
            GotoImplementation,
            /// Goes to the declaration of the type of the symbol at the caret.
            GotoTypeDeclaration,
            /// Goes to the method the one at the caret implements or overrides.
            GotoSuperMethod,
            /// Goes to the test of the active file, or to the file a test tests.
            GotoTest,
            /// Shows the files related to the active one: its tests, or the files it tests.
            GotoRelated,
            /// Shows the usages of the symbol at the caret in a popup.
            ShowUsages,
            /// Finds the usages of the symbol at the caret, in the Find tool window.
            FindUsages,
            /// Shows the definition of the symbol at the caret in a popup.
            QuickDefinition,
            /// Shows the callers of the function at the caret in the Hierarchy tool window.
            CallHierarchy,
            /// Shows the type hierarchy of the type at the caret in the Hierarchy tool window.
            TypeHierarchy,
        ]
    );
}

pub use hierarchy::hierarchy_panel;

/// Registers the actions on every workspace, each where the active file's servers can do it.
pub fn init(cx: &mut App) {
    add_generate_rows("Shell Script", code_menu::shell_rows, cx);
    add_generate_rows("HTML", html::generate_rows, cx);
    if let Some(app_state) = workspace::AppState::try_global(cx) {
        html::register_language(&app_state.languages);
    }
    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        let Some(window) = window else { return };
        // the menus follow the servers starting and stopping
        let lsp_store = workspace.project().read(cx).lsp_store();
        cx.subscribe_in(&lsp_store, window, |_, _, event, _, cx| match event {
            project::LspStoreEvent::LanguageServerAdded(..)
            | project::LspStoreEvent::LanguageServerRemoved(..) => cx.notify(),
            project::LspStoreEvent::LanguageServerUpdate {
                language_server_id,
                message: proto::update_language_server::Variant::WorkEnd(_),
                ..
            } => {
                cx.default_global::<WorkDone>()
                    .0
                    .insert(*language_server_id);
            }
            _ => {}
        })
        .detach();
        workspace.register_action_renderer(|div, workspace, window, cx| {
            let able = Able::of(workspace, cx);
            let div = code_menu::register(div, &able, cx);
            let div = refactor::register(div, &able, cx);
            let div = usages::register(div, &able, cx);
            let div = related::register(div, &able, cx);
            let div = hierarchy::register(div, &able, cx);
            let div = inspection::register(div, &able, cx);
            let div = html::register(div, able.html, cx);
            div.children(overlays(window, cx))
        });
        hierarchy::register_panel(workspace);
    })
    .detach();
}

/// What the active editor's file can do: its language servers' capabilities, IntelliJ's
/// availability of each action.
#[derive(Default)]
pub(crate) struct Able {
    pub(crate) editor: bool,
    pub(crate) format: bool,
    pub(crate) range_format: bool,
    pub(crate) organize_imports: bool,
    pub(crate) fix_all: bool,
    pub(crate) code_actions: bool,
    /// The file's language has Generate rows of its own.
    pub(crate) generate: bool,
    /// The file is HTML: its table actions and Surround with Tag.
    pub(crate) html: bool,
    pub(crate) refactor: bool,
    pub(crate) extract: bool,
    pub(crate) inline: bool,
    pub(crate) change_signature: bool,
    pub(crate) rename: bool,
    pub(crate) definition: bool,
    pub(crate) declaration: bool,
    pub(crate) implementation: bool,
    pub(crate) type_definition: bool,
    pub(crate) references: bool,
    pub(crate) call_hierarchy: bool,
    // shortcut: no Type Hierarchy here, greyed: Zed's lsp-types has no `typeHierarchyProvider` to
    // tell a server that serves it; add it once the fork has the field
}

impl Able {
    fn of(workspace: &Workspace, cx: &App) -> Self {
        let Some(editor) = workspace.active_item_as::<Editor>(cx) else {
            return Self::default();
        };
        let Some(buffer) = editor.read(cx).buffer().read(cx).as_singleton() else {
            return Self::default();
        };
        let servers = capabilities(workspace.project(), &buffer, cx);
        let any = |f: &dyn Fn(&lsp::ServerCapabilities) -> bool| servers.iter().any(f);
        let kind = |kind: &str| any(&|caps| offers_kind(caps, kind));
        Self {
            editor: true,
            // Zed also formats with a language's external formatter or Prettier, as IntelliJ
            // always can: Reformat Code is never greyed in an editor
            format: true,
            range_format: any(&|caps| {
                caps.document_range_formatting_provider
                    .as_ref()
                    .is_some_and(|provider| !matches!(provider, lsp::OneOf::Left(false)))
            }),
            organize_imports: any(&|caps| lists_kind(caps, "source.organizeImports")),
            fix_all: any(&|caps| lists_kind(caps, "source.fixAll")),
            code_actions: kind(""),
            generate: generate_provider(&buffer, cx).is_some(),
            html: buffer
                .read(cx)
                .language()
                .is_some_and(|language| language.name().as_ref() == "HTML"),
            refactor: kind("refactor"),
            extract: kind("refactor.extract"),
            inline: kind("refactor.inline"),
            change_signature: any(&|caps| lists_kind(caps, "refactor.changeSignature")),
            rename: any(&|caps| {
                caps.rename_provider
                    .as_ref()
                    .is_some_and(|provider| !matches!(provider, lsp::OneOf::Left(false)))
            }),
            definition: any(&|caps| {
                caps.definition_provider
                    .as_ref()
                    .is_some_and(|provider| !matches!(provider, lsp::OneOf::Left(false)))
            }),
            declaration: any(&|caps| {
                caps.declaration_provider.as_ref().is_some_and(|provider| {
                    !matches!(provider, lsp::DeclarationCapability::Simple(false))
                })
            }),
            implementation: any(&|caps| {
                caps.implementation_provider
                    .as_ref()
                    .is_some_and(|provider| {
                        !matches!(
                            provider,
                            lsp::ImplementationProviderCapability::Simple(false)
                        )
                    })
            }),
            type_definition: any(&|caps| {
                caps.type_definition_provider
                    .as_ref()
                    .is_some_and(|provider| {
                        !matches!(
                            provider,
                            lsp::TypeDefinitionProviderCapability::Simple(false)
                        )
                    })
            }),
            references: any(&|caps| {
                caps.references_provider
                    .as_ref()
                    .is_some_and(|provider| !matches!(provider, lsp::OneOf::Left(false)))
            }),
            call_hierarchy: any(&|caps| {
                caps.call_hierarchy_provider
                    .as_ref()
                    .is_some_and(|provider| {
                        !matches!(provider, lsp::CallHierarchyServerCapability::Simple(false))
                    })
            }),
        }
    }
}

/// The capabilities of the language servers running for `buffer`.
// shortcut: the servers' static capabilities only, not what they register later; read the
// dynamic registrations too once a server needs them
pub(crate) fn capabilities(
    project: &Entity<Project>,
    buffer: &Entity<Buffer>,
    cx: &App,
) -> Vec<lsp::ServerCapabilities> {
    let store = project.read(cx).lsp_store().read(cx);
    store
        .language_server_ids_for_opened_buffer(buffer.read(cx).remote_id())
        .into_iter()
        .flatten()
        .filter_map(|id| store.language_server_for_id(*id))
        .map(|server| server.capabilities())
        .collect()
}

/// Whether a server serves code actions of `kind`: it serves code actions and lists no kinds,
/// or lists the kind, one above it or one under it (IntelliJ's `supportsCodeActionsOfKind`).
pub(crate) fn offers_kind(capabilities: &lsp::ServerCapabilities, kind: &str) -> bool {
    match &capabilities.code_action_provider {
        None => false,
        Some(lsp::CodeActionProviderCapability::Simple(simple)) => *simple,
        Some(lsp::CodeActionProviderCapability::Options(options)) => {
            options.code_action_kinds.as_ref().is_none_or(|kinds| {
                kinds.iter().any(|listed| {
                    kind_covers(listed.as_str(), kind) || kind_covers(kind, listed.as_str())
                })
            })
        }
    }
}

/// Whether a server lists `kind` or a kind under it (IntelliJ's
/// `supportsExplicitlyCodeActionOfKind`): organize imports needs the server to say it has it.
pub(crate) fn lists_kind(capabilities: &lsp::ServerCapabilities, kind: &str) -> bool {
    match &capabilities.code_action_provider {
        Some(lsp::CodeActionProviderCapability::Options(options)) => {
            options.code_action_kinds.as_ref().is_some_and(|kinds| {
                kinds
                    .iter()
                    .any(|listed| kind_covers(kind, listed.as_str()))
            })
        }
        _ => false,
    }
}

/// Whether `kind` is `parent` or under it: `refactor.extract` is under `refactor`, not under
/// `refactor.ex`; every kind is under the empty one.
pub(crate) fn kind_covers(parent: &str, kind: &str) -> bool {
    parent.is_empty()
        || kind == parent
        || kind
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('.'))
}

/// The active editor, its file and the selection in it (the caret when empty).
pub struct Target {
    pub editor: Entity<Editor>,
    pub buffer: Entity<Buffer>,
    pub range: Range<text::Anchor>,
    pub project: Entity<Project>,
}

impl Target {
    pub fn of(workspace: &Workspace, cx: &App) -> Option<Self> {
        let editor = workspace.active_item_as::<Editor>(cx)?;
        let (buffer, range) = {
            let editor = editor.read(cx);
            let selection = editor.selections.newest_anchor();
            let multibuffer = editor.buffer().read(cx);
            let (buffer, start) = multibuffer.text_anchor_for_position(selection.start, cx)?;
            let (_, end) = multibuffer.text_anchor_for_position(selection.end, cx)?;
            (buffer, start..end)
        };
        Some(Self {
            editor,
            buffer,
            range,
            project: workspace.project().clone(),
        })
    }

    /// The caret, the selection's head.
    pub(crate) fn caret(&self, cx: &App) -> text::Anchor {
        let editor = self.editor.read(cx);
        let head = editor.selections.newest_anchor().head();
        editor
            .buffer()
            .read(cx)
            .text_anchor_for_position(head, cx)
            .map_or(self.range.start, |(_, anchor)| anchor)
    }
}

/// The server's code actions of `kind` for the selection, those `keep` keeps.
pub(crate) fn request_code_actions(
    target: &Target,
    kind: &str,
    keep: impl Fn(&CodeAction) -> bool + 'static,
    cx: &mut App,
) -> Task<Vec<CodeAction>> {
    let kinds = (!kind.is_empty()).then(|| vec![lsp::CodeActionKind::from(kind.to_owned())]);
    let request = target.project.update(cx, |project, cx| {
        project.code_actions(&target.buffer, target.range.clone(), kinds, cx)
    });
    let kind = kind.to_owned();
    cx.spawn(async move |_| {
        let actions = request.await.log_err().flatten().unwrap_or_default();
        actions
            .into_iter()
            // a server may answer with kinds it wasn't asked for; a kindless action is trusted
            .filter(|action| {
                action
                    .lsp_action
                    .action_kind()
                    .is_none_or(|found| kind_covers(&kind, found.as_str()))
            })
            .filter(|action| keep(action))
            .collect()
    })
}

/// The kind of a code action, empty when it has none.
pub(crate) fn action_kind(action: &CodeAction) -> String {
    action
        .lsp_action
        .action_kind()
        .map(|kind| kind.as_str().to_owned())
        .unwrap_or_default()
}

/// Runs a code action, then saves the files it changed: IntelliJ's refactorings leave no file
/// unsaved.
pub fn apply_code_action(target: &Target, action: CodeAction, window: &mut Window, cx: &mut App) {
    let apply = target.project.update(cx, |project, cx| {
        project.apply_code_action(target.buffer.clone(), action, true, cx)
    });
    let project = target.project.clone();
    let editor = target.editor.downgrade();
    window
        .spawn(cx, async move |cx| {
            let transaction = match apply.await {
                Ok(transaction) => transaction,
                Err(error) => {
                    cx.update(|window, cx| show_hint(&editor, error.to_string(), true, window, cx))
                        .ok();
                    return Err(error);
                }
            };
            save_changed(&project, transaction, cx).await
        })
        .detach_and_log_err(cx);
}

/// Saves the files a refactoring changed.
pub(crate) async fn save_changed(
    project: &Entity<Project>,
    transaction: ProjectTransaction,
    cx: &mut gpui::AsyncWindowContext,
) -> anyhow::Result<()> {
    let buffers: HashSet<Entity<Buffer>> = transaction.0.into_keys().collect();
    if buffers.is_empty() {
        return Ok(());
    }
    project
        .update(cx, |project, cx| project.save_buffers(buffers, cx))
        .await
}

/// IntelliJ's `requestAndApplyCodeActions`: the server's code actions of `kind` that `keep`
/// keeps; none shows `nothing` as a hint, one runs, several open a chooser titled `title`.
pub(crate) fn request_and_apply(
    workspace: &Workspace,
    kind: &'static str,
    keep: impl Fn(&CodeAction) -> bool + 'static,
    nothing: &'static str,
    title: &'static str,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(target) = Target::of(workspace, cx) else {
        return;
    };
    let request = request_code_actions(&target, kind, keep, cx);
    let workspace = workspace.weak_handle();
    window
        .spawn(cx, async move |cx| {
            let actions = request.await;
            cx.update(|window, cx| match actions.len() {
                0 => show_hint(&target.editor.downgrade(), nothing, true, window, cx),
                1 => {
                    if let Some(action) = actions.into_iter().next() {
                        apply_code_action(&target, action, window, cx);
                    }
                }
                _ => {
                    let rows = actions
                        .into_iter()
                        .map(|action| {
                            let target = Target {
                                editor: target.editor.clone(),
                                buffer: target.buffer.clone(),
                                range: target.range.clone(),
                                project: target.project.clone(),
                            };
                            Row::item(action.lsp_action.title().to_owned(), move |window, cx| {
                                apply_code_action(&target, action.clone(), window, cx)
                            })
                        })
                        .collect();
                    show_list_popup(&workspace, &target.editor, title, rows, false, window, cx);
                }
            })
        })
        .detach_and_log_err(cx);
}

/// Moves the caret to `point` of the file `path`, opening it in the active pane.
pub(crate) fn open_location(
    workspace: &mut Workspace,
    buffer: Entity<Buffer>,
    range: Range<text::Anchor>,
    window: &mut Window,
    cx: &mut gpui::Context<Workspace>,
) {
    let editor = workspace.open_project_item::<Editor>(
        Some(workspace.active_pane().clone()),
        buffer.clone(),
        true,
        true,
        false,
        true,
        window,
        cx,
    );
    let start = range.start.to_point(buffer.read(cx));
    editor.update(cx, |editor, cx| {
        editor.change_selections(
            SelectionEffects::scroll(Autoscroll::center()),
            window,
            cx,
            |selections| selections.select_ranges([start..start]),
        );
    });
}

/// A hint over the caret, IntelliJ's `HintManager` hint: an error (`showErrorHint`) or
/// information, gone with the next key or after a few seconds.
struct Hint {
    window: gpui::AnyWindowHandle,
    origin: Point<Pixels>,
    text: SharedString,
    error: bool,
    _expires: Task<()>,
    _hide: gpui::Subscription,
}

#[derive(Default)]
struct Hints(Option<Hint>);

impl Global for Hints {}

/// How long a hint stays, as IntelliJ's error hints (`showErrorHint`'s 3000 ms).
const HINT_TIMEOUT: Duration = Duration::from_millis(3000);

/// Shows `text` over the caret, as IntelliJ's `HintManager` hint.
pub fn show_hint(
    editor: &WeakEntity<Editor>,
    text: impl Into<SharedString>,
    error: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(editor) = editor.upgrade() else {
        return;
    };
    let Some(origin) = caret_origin(&editor, window, cx) else {
        return;
    };
    let line_height = window.line_height();
    let workspace = editor
        .read(cx)
        .workspace()
        .map(|workspace| workspace.downgrade());
    let expires = cx.spawn({
        let workspace = workspace.clone();
        async move |cx| {
            cx.background_executor().timer(HINT_TIMEOUT).await;
            cx.update(|cx| {
                cx.set_global(Hints(None));
                if let Some(workspace) = workspace {
                    workspace.update(cx, |_, cx| cx.notify()).ok();
                }
            });
        }
    });
    // a key typed after it shows hides it (`HIDE_BY_ANY_KEY`), not the one that asked for it
    let shown = std::time::Instant::now();
    let hide = cx.observe_keystrokes(move |_, _, cx| {
        if shown.elapsed() > Duration::from_millis(300) {
            cx.defer(|cx| {
                cx.set_global(Hints(None));
                cx.refresh_windows();
            });
        }
    });
    cx.set_global(Hints(Some(Hint {
        window: window.window_handle(),
        // above the caret's line, as IntelliJ shows it (`HintManager.ABOVE`)
        origin: point(origin.x, origin.y - line_height - px(4.)),
        text: text.into(),
        error,
        _expires: expires,
        _hide: hide,
    })));
    if let Some(workspace) = workspace {
        cx.defer(move |cx| {
            workspace.update(cx, |_, cx| cx.notify()).ok();
        });
    }
}

/// What spork_code draws over the workspace: the hint, the in-place rename's popup.
fn overlays(window: &Window, cx: &App) -> Vec<AnyElement> {
    let mut drawn: Vec<AnyElement> = refactor::render_rename_popup(window, cx)
        .into_iter()
        .collect();
    if let Some(hint) = cx
        .try_global::<Hints>()
        .and_then(|hints| hints.0.as_ref())
        .filter(|hint| hint.window == window.window_handle())
    {
        let tokens = Tokens::get(cx);
        let label = div()
            .id("code-hint")
            .role(Role::Tooltip)
            .aria_label(hint.text.clone())
            .px(px(8.))
            .py(px(4.))
            .rounded(px(4.))
            .border_1()
            .border_color(tokens.hint_border)
            .bg(match hint.error {
                true => tokens.error_hint,
                false => tokens.information_hint,
            })
            .text_ui()
            .text_color(tokens.text)
            .child(hint.text.clone());
        drawn.push(
            deferred(
                anchored()
                    .position(hint.origin)
                    .anchor(gpui::Anchor::BottomLeft)
                    .snap_to_window_with_margin(px(8.))
                    .child(spork_vcs::spork_look(cx).child(label)),
            )
            .with_priority(2)
            .into_any_element(),
        );
    }
    drawn
}

/// The shortcut of `action` in `editor`, IntelliJ's way (`⇧F6`).
pub fn shortcut(
    action: &dyn gpui::Action,
    editor: &Entity<Editor>,
    window: &Window,
    cx: &App,
) -> Option<SharedString> {
    // the editor's keys: an action handler runs outside the frame's context stack
    let binding =
        window.highest_precedence_binding_for_action_in(action, &editor.focus_handle(cx))?;
    let mac = cfg!(target_os = "macos");
    let text = binding
        .keystrokes()
        .iter()
        .map(|keystroke| {
            let modifiers = keystroke.modifiers();
            let mut text = String::new();
            for (held, symbol, name) in [
                (modifiers.control, "⌃", "Ctrl+"),
                (modifiers.alt, "⌥", "Alt+"),
                (modifiers.shift, "⇧", "Shift+"),
                (modifiers.platform, "⌘", "Meta+"),
            ] {
                if held {
                    text.push_str(if mac { symbol } else { name });
                }
            }
            let key = keystroke.key();
            text.push_str(&match key {
                "enter" if mac => "⏎".to_owned(),
                "delete" if mac => "⌦".to_owned(),
                "backspace" if mac => "⌫".to_owned(),
                "up" if mac => "↑".to_owned(),
                "down" if mac => "↓".to_owned(),
                "left" if mac => "←".to_owned(),
                "right" if mac => "→".to_owned(),
                "space" => "Space".to_owned(),
                key => {
                    let mut chars = key.chars();
                    chars
                        .next()
                        .map(|first| first.to_uppercase().chain(chars).collect())
                        .unwrap_or_default()
                }
            });
            text
        })
        .collect::<Vec<_>>()
        .join(" ");
    Some(text.into())
}

#[cfg(test)]
mod tests {
    use super::kind_covers;

    #[test]
    fn a_kind_covers_its_own_subtree() {
        assert!(kind_covers("refactor", "refactor.extract"));
        assert!(kind_covers("refactor.extract", "refactor.extract"));
        assert!(kind_covers("", "quickfix"));
        assert!(!kind_covers("refactor.ex", "refactor.extract"));
        assert!(!kind_covers(
            "refactor.extract.variable",
            "refactor.extract"
        ));
        assert!(!kind_covers("refactor.inline2", "refactor.inline"));
    }
}

/// The language servers that have finished some work (indexing, loading the project): the
/// end-to-end tests' sign that a server has started its work, not only its process.
#[derive(Default)]
struct WorkDone(HashSet<lsp::LanguageServerId>);

impl Global for WorkDone {}

/// What the end-to-end tests read of code intelligence (crates/spork_e2e): `ready`, whether the
/// active file's language servers run, have finished some work and have none left (indexing); `renaming`, the
/// name an in-place rename shows; `inspection`, the last Inspect Code run's scope and problems
/// (`path:line message`) and the files they are in.
pub fn e2e_state(workspace: &Workspace, cx: &App) -> serde_json::Value {
    let project = workspace.project().read(cx);
    let servers = workspace
        .active_item_as::<Editor>(cx)
        .and_then(|editor| editor.read(cx).buffer().read(cx).as_singleton())
        .map(|buffer| capabilities(workspace.project(), &buffer, cx).len())
        .unwrap_or_default();
    let busy = project
        .language_server_statuses(cx)
        .any(|(_, status)| !status.pending_work.is_empty());
    let worked = cx
        .try_global::<WorkDone>()
        .is_some_and(|done| !done.0.is_empty());
    let inspection = inspection::last_inspection(cx).map(|inspection| {
        let mut files: Vec<&str> = inspection
            .problems
            .iter()
            .map(|problem| problem.path.path.as_unix_str())
            .collect();
        files.dedup();
        serde_json::json!({
            "scope": inspection.scope.as_ref(),
            "files": files,
            "problems": inspection
                .problems
                .iter()
                .map(|problem| format!(
                    "{}:{} {}",
                    problem.path.path.as_unix_str(),
                    problem.point.row + 1,
                    problem.message
                ))
                .collect::<Vec<_>>(),
        })
    });
    serde_json::json!({
        "ready": servers > 0 && worked && !busy,
        "renaming": refactor::renaming_name(cx),
        "inspection": inspection,
    })
}
