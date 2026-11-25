//! Spork IDE: the look the app gives the editor's code menus and popovers (IntelliJ's completion
//! lookup, intention list, parameter info, documentation and problem popups), set once with
//! [`CodeMenuLook`]; Zed's otherwise. The editor keeps their behaviour: the lists, their keys,
//! clicks and scrolling, where they show.

use std::{ops::Range, rc::Rc};

use gpui::{
    AnyElement, App, Div, Entity, EntityId, FontWeight, Global, HighlightStyle, Pixels,
    SharedString, Stateful, WeakEntity, Window,
};
use language::Buffer;
use lsp::CompletionItemKind;
use multi_buffer::Anchor;

use crate::Editor;

/// A completion as the app draws it.
pub struct CompletionRow {
    /// The label, the name at `filter_range`, what follows it (its parameters, its type) after.
    pub text: SharedString,
    pub filter_range: Range<usize>,
    /// What the query matched, in `text`.
    pub matches: Vec<Range<usize>>,
    pub kind: Option<CompletionItemKind>,
    /// The language server's detail (the type, often).
    pub detail: Option<SharedString>,
    pub deprecated: bool,
    pub selected: bool,
}

/// The completions' popup, for its bottom row.
pub struct CompletionsPopup {
    pub editor: WeakEntity<Editor>,
    /// The menu's id: a new one each time completions open.
    pub id: usize,
    /// Whether the selected item's documentation shows beside the list by itself.
    pub auto_documentation: bool,
}

/// A documentation popup, for the parts the app draws around its content.
pub struct DocumentationPopup {
    pub editor: WeakEntity<Editor>,
    /// The popup's identity, the same while it shows.
    pub id: EntityId,
    /// Where the documented symbol is.
    pub anchor: Option<Anchor>,
}

/// The intention selected in the code actions' list, for its preview beside it.
pub struct IntentionPreview {
    pub editor: WeakEntity<Editor>,
    pub buffer: Entity<Buffer>,
    pub action: project::CodeAction,
}

/// What a code action is, for its icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeActionRowKind {
    /// A language server's quick fix (`quickfix`) of an error.
    QuickFix,
    /// A quick fix of a warning or a weaker problem.
    InspectionFix,
    /// Any other language server's action (refactorings, source actions).
    Action,
    /// A task or a debug scenario to run.
    Run,
}

pub struct CodeActionRow {
    pub editor: WeakEntity<Editor>,
    pub label: SharedString,
    pub kind: CodeActionRowKind,
    pub selected: bool,
}

/// One signature of a parameter info popup.
pub struct SignatureRow {
    pub label: SharedString,
    /// The parameter at the caret, in `label`.
    pub active_parameter: Option<Range<usize>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopoverKind {
    /// Quick documentation, a symbol's hover.
    Documentation,
    /// A problem's (a diagnostic's) hover.
    Problem,
}

#[derive(Clone)]
pub struct CodeMenuLook {
    /// A row of the completions and the code actions lists.
    pub row_height: Pixels,
    /// The most completions shown before the list scrolls.
    pub completion_rows: usize,
    pub completion_row: Rc<dyn Fn(CompletionRow, &App) -> AnyElement>,
    /// The completions' list in its popup.
    pub completions: Rc<dyn Fn(AnyElement, CompletionsPopup, &mut Window, &mut App) -> AnyElement>,
    pub code_action_row: Rc<dyn Fn(CodeActionRow, &App) -> AnyElement>,
    /// The code actions' list in its popup.
    pub code_actions: Rc<dyn Fn(AnyElement, &App) -> AnyElement>,
    /// A hover's surface, its content already in it.
    pub popover: Rc<dyn Fn(Stateful<Div>, PopoverKind, &App) -> Stateful<Div>>,
    /// The signatures, and the index of the one that matches.
    pub signatures: Rc<dyn Fn(Vec<SignatureRow>, usize, &App) -> AnyElement>,
    /// A documentation popup's footer, under its content: its toolbar, the definition's place.
    pub documentation: Rc<dyn Fn(DocumentationPopup, &mut Window, &mut App) -> Option<AnyElement>>,
    /// The documentation popup's tallest, before it scrolls.
    pub documentation_max_height: Pixels,
    /// The selected intention's preview, beside the list.
    pub intention_preview:
        Rc<dyn Fn(IntentionPreview, &mut Window, &mut App) -> Option<AnyElement>>,
}

impl Global for CodeMenuLook {}

impl CodeMenuLook {
    pub(crate) fn get(cx: &App) -> Option<Self> {
        cx.try_global::<Self>().cloned()
    }
}

/// The span the signature's highlights mark as the active parameter (the language server's
/// extra bold).
pub(crate) fn active_parameter(
    highlights: &[(Range<usize>, HighlightStyle)],
) -> Option<Range<usize>> {
    highlights
        .iter()
        .filter(|(_, style)| style.font_weight == Some(FontWeight::EXTRA_BOLD))
        .map(|(range, _)| range.clone())
        .reduce(|span, range| span.start.min(range.start)..span.end.max(range.end))
}
