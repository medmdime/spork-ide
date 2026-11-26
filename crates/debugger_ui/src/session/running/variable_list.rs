use crate::session::running::{RunningState, memory_view::MemoryView};

use super::stack_frame_list::{StackFrameList, StackFrameListEvent};
use dap::{
    ScopePresentationHint, StackFrameId, VariablePresentationHint, VariablePresentationHintKind,
    VariableReference,
};
use editor::Editor;
use gpui::{
    Action, AnyElement, ClickEvent, ClipboardItem, Context, DismissEvent, Empty, Entity,
    FocusHandle, Focusable, Hsla, MouseDownEvent, Point, Subscription, TaskExt,
    TextStyleRefinement, UniformListScrollHandle, WeakEntity, actions, anchored, deferred,
    uniform_list,
};
use itertools::Itertools;
use menu::{SelectFirst, SelectLast, SelectNext, SelectPrevious};
use project::debugger::{
    dap_command::DataBreakpointContext,
    session::{Session, SessionEvent, Watcher},
};
use std::{collections::HashMap, ops::Range, sync::Arc};
use ui::{
    ContextMenu, ContextMenuEntry, ListItem, ScrollAxes, ScrollableHandle, Tooltip, WithScrollbar,
    prelude::*,
};
use util::{debug_panic, maybe};

static INDENT_STEP_SIZE: Pixels = px(10.0);

actions!(
    variable_list,
    [
        /// Expands the selected variable entry to show its children.
        ExpandSelectedEntry,
        /// Collapses the selected variable entry to hide its children.
        CollapseSelectedEntry,
        /// Copies the variable name to the clipboard.
        CopyVariableName,
        /// Copies the variable value to the clipboard.
        CopyVariableValue,
        /// Edits the value of the selected variable.
        EditVariable,
        /// Adds the selected variable to the watch list.
        AddWatch,
        /// Removes the selected variable from the watch list.
        RemoveWatch,
        /// Jump to variable's memory location.
        GoToMemory,
    ]
);

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct EntryState {
    depth: usize,
    is_expanded: bool,
    has_children: bool,
    parent_reference: VariableReference,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub(crate) struct EntryPath {
    pub leaf_name: Option<SharedString>,
    pub indices: Arc<[SharedString]>,
}

impl EntryPath {
    fn for_watcher(expression: impl Into<SharedString>) -> Self {
        Self {
            leaf_name: Some(expression.into()),
            indices: Arc::new([]),
        }
    }

    fn for_scope(scope_name: impl Into<SharedString>) -> Self {
        Self {
            leaf_name: Some(scope_name.into()),
            indices: Arc::new([]),
        }
    }

    fn with_name(&self, name: SharedString) -> Self {
        Self {
            leaf_name: Some(name),
            indices: self.indices.clone(),
        }
    }

    /// Create a new child of this variable path
    fn with_child(&self, name: SharedString) -> Self {
        Self {
            leaf_name: None,
            indices: self
                .indices
                .iter()
                .cloned()
                .chain(std::iter::once(name))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum DapEntry {
    Watcher(Watcher),
    Variable(dap::Variable),
    Scope(dap::Scope),
}

impl DapEntry {
    fn as_watcher(&self) -> Option<&Watcher> {
        match self {
            DapEntry::Watcher(watcher) => Some(watcher),
            _ => None,
        }
    }

    fn as_variable(&self) -> Option<&dap::Variable> {
        match self {
            DapEntry::Variable(dap) => Some(dap),
            _ => None,
        }
    }

    fn as_scope(&self) -> Option<&dap::Scope> {
        match self {
            DapEntry::Scope(dap) => Some(dap),
            _ => None,
        }
    }

    #[cfg(test)]
    fn name(&self) -> &str {
        match self {
            DapEntry::Watcher(watcher) => &watcher.expression,
            DapEntry::Variable(dap) => &dap.name,
            DapEntry::Scope(dap) => &dap.name,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ListEntry {
    entry: DapEntry,
    path: EntryPath,
}

impl ListEntry {
    fn as_watcher(&self) -> Option<&Watcher> {
        self.entry.as_watcher()
    }

    fn as_variable(&self) -> Option<&dap::Variable> {
        self.entry.as_variable()
    }

    fn as_scope(&self) -> Option<&dap::Scope> {
        self.entry.as_scope()
    }

    fn item_id(&self) -> ElementId {
        use std::fmt::Write;
        let mut id = match &self.entry {
            DapEntry::Watcher(watcher) => format!("watcher-{}", watcher.expression),
            DapEntry::Variable(dap) => format!("variable-{}", dap.name),
            DapEntry::Scope(dap) => format!("scope-{}", dap.name),
        };
        for name in self.path.indices.iter() {
            _ = write!(id, "-{}", name);
        }
        SharedString::from(id).into()
    }

    fn item_value_id(&self) -> ElementId {
        use std::fmt::Write;
        let mut id = match &self.entry {
            DapEntry::Watcher(watcher) => format!("watcher-{}", watcher.expression),
            DapEntry::Variable(dap) => format!("variable-{}", dap.name),
            DapEntry::Scope(dap) => format!("scope-{}", dap.name),
        };
        for name in self.path.indices.iter() {
            _ = write!(id, "-{}", name);
        }
        _ = write!(id, "-value");
        SharedString::from(id).into()
    }
}

struct VariableColor {
    name: Option<Hsla>,
    value: Option<Hsla>,
}

pub struct VariableList {
    entries: Vec<ListEntry>,
    max_width_index: Option<usize>,
    entry_states: HashMap<EntryPath, EntryState>,
    selected_stack_frame_id: Option<StackFrameId>,
    list_handle: UniformListScrollHandle,
    session: Entity<Session>,
    selection: Option<EntryPath>,
    open_context_menu: Option<(Entity<ContextMenu>, Point<Pixels>, Subscription)>,
    focus_handle: FocusHandle,
    edited_path: Option<(EntryPath, Entity<Editor>)>,
    disabled: bool,
    memory_view: Entity<MemoryView>,
    weak_running: WeakEntity<RunningState>,
    _subscriptions: Vec<Subscription>,
}

/// Spork IDE: the debug adapters whose frames' only scope the variables tree doesn't show as a
/// node (Java's), by name.
#[derive(Default)]
pub struct FlatScopeAdapters(pub collections::HashSet<SharedString>);

impl gpui::Global for FlatScopeAdapters {}

/// Spork IDE: Toggle Field Watchpoint on a variable, set once by the app: given the session and
/// the adapter's data id, whether it made the watchpoint (else the session's data breakpoint).
#[derive(Clone)]
pub struct FieldWatchpointHook(pub std::rc::Rc<dyn Fn(&Entity<Session>, &str, &mut App) -> bool>);

impl gpui::Global for FieldWatchpointHook {}

/// Spork IDE: what the app's actions on a value (IntelliJ's `XDebugger.ValueGroup`) can do with
/// the selected one, given by the app for its adapters' sessions.
#[derive(Clone, Debug, Default)]
pub struct ValueActions {
    /// Mark Object…, the value having an object to mark: whether it is marked (Unmark Object).
    pub mark: Option<bool>,
    pub jump_to_source: bool,
    pub jump_to_type_source: bool,
    /// View as: each renderer that applies and whether it shows the value.
    pub renderers: Vec<(SharedString, bool)>,
    /// Show types: whether values show their `{Type@id}`, where the app serves it.
    pub show_types: Option<bool>,
}

/// Spork IDE: a selected value's `ValueActions`, given the session, the value and its depth in
/// the tree (1 for a frame's own variables, under their scope); `None` for a session the app
/// doesn't serve.
#[derive(Clone)]
pub struct ValueActionsHook(
    pub std::rc::Rc<dyn Fn(&Entity<Session>, &dap::Variable, usize, &App) -> Option<ValueActions>>,
);

impl gpui::Global for ValueActionsHook {}

/// Spork IDE: a value's IntelliJ markup (Mark Object…): its label, drawn before its name in its
/// colour (`XValueNodeImpl`'s `[label] `).
#[derive(Clone)]
pub struct ValueMarkupHook(
    pub std::rc::Rc<dyn Fn(&Entity<Session>, &dap::Variable, &App) -> Option<(SharedString, Hsla)>>,
);

impl gpui::Global for ValueMarkupHook {}

fn value_markup(
    session: &Entity<Session>,
    variable: &dap::Variable,
    cx: &App,
) -> Option<(SharedString, Hsla)> {
    let hook = cx.try_global::<ValueMarkupHook>()?;
    (hook.0)(session, variable, cx)
}

/// Spork IDE: the watches aren't in the variables tree (IntelliJ's Separate Watches): the app
/// shows them.
#[derive(Default)]
pub struct SeparateWatches(pub bool);

impl gpui::Global for SeparateWatches {}

/// Spork IDE: what the app's watches toolbar acts on.
impl VariableList {
    /// The tree built again, as Separate Watches changed.
    pub fn rebuild(&mut self, cx: &mut Context<Self>) {
        self.build_entries(cx);
        cx.notify();
    }

    /// The selected variable or watch as the adapter's variable (a watch named by its
    /// expression), with its depth in the tree.
    pub fn selected_variable(&self) -> Option<(dap::Variable, usize)> {
        let selection = self.selection.as_ref()?;
        let entry = self.entries.iter().find(|entry| &entry.path == selection)?;
        let depth = self
            .entry_states
            .get(&entry.path)
            .map_or(0, |state| state.depth);
        let variable = match &entry.entry {
            DapEntry::Variable(variable) => variable.clone(),
            DapEntry::Watcher(watcher) => dap::Variable {
                name: watcher.expression.to_string(),
                value: watcher.value.to_string(),
                type_: None,
                presentation_hint: watcher.presentation_hint.clone(),
                evaluate_name: Some(watcher.expression.to_string()),
                variables_reference: watcher.variables_reference,
                named_variables: None,
                indexed_variables: None,
                memory_reference: None,
                declaration_location_reference: None,
                value_location_reference: None,
            },
            DapEntry::Scope(_) => return None,
        };
        Some((variable, depth))
    }

    fn markup_of(&self, variable: &dap::Variable, cx: &App) -> Option<(SharedString, Hsla)> {
        value_markup(&self.session, variable, cx)
    }

    /// The rows, `name = value` (a scope by its name, a marked value after its `[label]`), for
    /// the app's end-to-end tests.
    pub fn rows(&self, cx: &App) -> Vec<String> {
        self.entries
            .iter()
            .map(|entry| match &entry.entry {
                DapEntry::Watcher(watcher) => format!("{} = {}", watcher.expression, watcher.value),
                DapEntry::Variable(variable) => {
                    let row = format!("{} = {}", variable.name, variable.value);
                    match self.markup_of(variable, cx) {
                        Some((label, _)) => format!("[{label}] {row}"),
                        None => row,
                    }
                }
                DapEntry::Scope(scope) => scope.name.clone(),
            })
            .collect()
    }

    /// The selected watch's or variable's expression, and whether it is a watch.
    pub fn selected_value(&self) -> Option<(SharedString, bool)> {
        let selection = self.selection.as_ref()?;
        let entry = self.entries.iter().find(|entry| &entry.path == selection)?;
        match &entry.entry {
            DapEntry::Watcher(watcher) => Some((watcher.expression.clone(), true)),
            DapEntry::Variable(variable) => Some((
                variable
                    .evaluate_name
                    .clone()
                    .unwrap_or_else(|| variable.name.clone())
                    .into(),
                false,
            )),
            DapEntry::Scope(_) => None,
        }
    }
}

impl VariableList {
    pub(crate) fn new(
        session: Entity<Session>,
        stack_frame_list: Entity<StackFrameList>,
        memory_view: Entity<MemoryView>,
        weak_running: WeakEntity<RunningState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();

        let _subscriptions = vec![
            cx.subscribe(&stack_frame_list, Self::handle_stack_frame_list_events),
            cx.subscribe(&session, |this, _, event, cx| match event {
                SessionEvent::HistoricSnapshotSelected => {
                    this.selection.take();
                    this.edited_path.take();
                    this.selected_stack_frame_id.take();
                    this.build_entries(cx);
                }
                SessionEvent::Stopped(_) => {
                    this.selection.take();
                    this.edited_path.take();
                    this.selected_stack_frame_id.take();
                }
                SessionEvent::Variables | SessionEvent::Watchers => {
                    this.build_entries(cx);
                }
                _ => {}
            }),
            cx.on_focus_out(&focus_handle, window, |this, _, _, cx| {
                this.edited_path.take();
                cx.notify();
            }),
        ];

        let list_state = UniformListScrollHandle::default();

        Self {
            list_handle: list_state,
            session,
            focus_handle,
            _subscriptions,
            selected_stack_frame_id: None,
            selection: None,
            open_context_menu: None,
            disabled: false,
            edited_path: None,
            entries: Default::default(),
            max_width_index: None,
            entry_states: Default::default(),
            weak_running,
            memory_view,
        }
    }

    pub(super) fn disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        let old_disabled = std::mem::take(&mut self.disabled);
        self.disabled = disabled;
        if old_disabled != disabled {
            cx.notify();
        }
    }

    pub(super) fn has_open_context_menu(&self) -> bool {
        self.open_context_menu.is_some()
    }

    fn build_entries(&mut self, cx: &mut Context<Self>) {
        let Some(stack_frame_id) = self.selected_stack_frame_id else {
            return;
        };

        let mut entries = vec![];

        let scopes: Vec<_> = self.session.update(cx, |session, cx| {
            session.scopes(stack_frame_id, cx).to_vec()
        });

        let mut contains_local_scope = false;

        let mut stack = scopes
            .into_iter()
            .rev()
            .filter(|scope| {
                if scope
                    .presentation_hint
                    .as_ref()
                    .map(|hint| *hint == ScopePresentationHint::Locals)
                    .unwrap_or(scope.name.to_lowercase().starts_with("local"))
                {
                    contains_local_scope = true;
                }

                scope.expensive
                    || self.session.update(cx, |session, cx| {
                        !session.variables(scope.variables_reference, cx).is_empty()
                    })
            })
            .map(|scope| {
                (
                    scope.variables_reference,
                    scope.variables_reference,
                    EntryPath::for_scope(&scope.name),
                    DapEntry::Scope(scope),
                )
            })
            .collect::<Vec<_>>();
        // Spork IDE: a frame's only scope (Java's "Local") isn't a node for the adapters the app
        // names: its variables are the tree's top level, as IntelliJ lists them
        let adapter = self.session.read(cx).adapter();
        let flat = cx
            .try_global::<FlatScopeAdapters>()
            .is_some_and(|flat| flat.0.contains(adapter.0.as_ref()));
        let flat_scope = stack
            .first()
            .filter(|_| flat && stack.len() == 1)
            .map(|(_, _, path, _)| path.clone());

        let mut watches = self.session.read(cx).watchers().clone();
        // Spork IDE: Separate Watches shows them in a view of their own
        if cx
            .try_global::<SeparateWatches>()
            .is_some_and(|separate| separate.0)
        {
            watches.clear();
        }
        // Spork IDE: the watches after the scopes and in their order, as IntelliJ lists them
        // (the stack pops its last first)
        stack.splice(
            0..0,
            watches
                .into_values()
                .rev()
                .map(|watcher| {
                    (
                        watcher.variables_reference,
                        watcher.variables_reference,
                        EntryPath::for_watcher(watcher.expression.clone()),
                        DapEntry::Watcher(watcher),
                    )
                })
                .collect::<Vec<_>>(),
        );

        let scopes_count = stack.len();

        while let Some((container_reference, variables_reference, mut path, dap_kind)) = stack.pop()
        {
            match &dap_kind {
                DapEntry::Watcher(watcher) => path = path.with_child(watcher.expression.clone()),
                DapEntry::Variable(dap) => path = path.with_name(dap.name.clone().into()),
                DapEntry::Scope(dap) => path = path.with_child(dap.name.clone().into()),
            }

            let is_flat_scope = dap_kind.as_scope().is_some()
                && flat_scope
                    .as_ref()
                    .is_some_and(|scope| scope.leaf_name == path.indices.first().cloned());
            let under_flat_scope = dap_kind.as_variable().is_some()
                && flat_scope.as_ref().is_some_and(|scope| {
                    let first = path.indices.first();
                    scope.leaf_name.as_ref() == first
                        && first.is_some_and(|first| {
                            !self.session.read(cx).watchers().contains_key(first)
                        })
                });
            let var_state = self
                .entry_states
                .entry(path.clone())
                .and_modify(|state| {
                    state.parent_reference = container_reference;
                    state.has_children = variables_reference != 0;
                    if is_flat_scope {
                        state.is_expanded = true;
                    }
                })
                .or_insert(EntryState {
                    depth: path.indices.len() - usize::from(under_flat_scope),
                    is_expanded: is_flat_scope
                        || dap_kind.as_scope().is_some_and(|scope| {
                            !scope.expensive
                                && ((scopes_count == 1 && !contains_local_scope)
                                    || scope
                                        .presentation_hint
                                        .as_ref()
                                        .map(|hint| *hint == ScopePresentationHint::Locals)
                                        .unwrap_or(scope.name.to_lowercase().starts_with("local")))
                        }),
                    parent_reference: container_reference,
                    has_children: variables_reference != 0,
                });

            if !is_flat_scope {
                entries.push(ListEntry {
                    entry: dap_kind,
                    path: path.clone(),
                });
            }

            if var_state.is_expanded {
                let children = self
                    .session
                    .update(cx, |session, cx| session.variables(variables_reference, cx));
                stack.extend(children.into_iter().rev().map(|child| {
                    (
                        variables_reference,
                        child.variables_reference,
                        path.with_child(child.name.clone().into()),
                        DapEntry::Variable(child),
                    )
                }));
            }
        }

        self.entries = entries;

        let text_pixels = ui::TextSize::Default.pixels(cx).to_f64() as f32;
        let indent_size = INDENT_STEP_SIZE.to_f64() as f32;

        self.max_width_index = self
            .entries
            .iter()
            .map(|entry| match &entry.entry {
                DapEntry::Scope(scope) => scope.name.len() as f32 * text_pixels,
                DapEntry::Variable(variable) => {
                    (variable.value.len() + variable.name.len()) as f32 * text_pixels
                        + (entry.path.indices.len() as f32 * indent_size)
                }
                DapEntry::Watcher(watcher) => {
                    (watcher.value.len() + watcher.expression.len()) as f32 * text_pixels
                        + (entry.path.indices.len() as f32 * indent_size)
                }
            })
            .position_max_by(|left, right| left.total_cmp(right));

        cx.notify();
    }

    fn handle_stack_frame_list_events(
        &mut self,
        _: Entity<StackFrameList>,
        event: &StackFrameListEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            StackFrameListEvent::SelectedStackFrameChanged(stack_frame_id) => {
                self.selected_stack_frame_id = Some(*stack_frame_id);
                self.session.update(cx, |session, cx| {
                    session.refresh_watchers(*stack_frame_id, cx);
                });
                self.build_entries(cx);
            }
            StackFrameListEvent::BuiltEntries => {}
        }
    }

    pub fn completion_variables(&self, _cx: &mut Context<Self>) -> Vec<dap::Variable> {
        self.entries
            .iter()
            .filter_map(|entry| match &entry.entry {
                DapEntry::Variable(dap) => Some(dap.clone()),
                DapEntry::Scope(_) | DapEntry::Watcher { .. } => None,
            })
            .collect()
    }

    fn render_entries(
        &mut self,
        ix: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        ix.into_iter()
            .filter_map(|ix| {
                let (entry, state) = self
                    .entries
                    .get(ix)
                    .and_then(|entry| Some(entry).zip(self.entry_states.get(&entry.path)))?;

                match &entry.entry {
                    DapEntry::Watcher { .. } => {
                        Some(self.render_watcher(entry, *state, window, cx))
                    }
                    DapEntry::Variable(_) => Some(self.render_variable(entry, *state, window, cx)),
                    DapEntry::Scope(_) => Some(self.render_scope(entry, *state, cx)),
                }
            })
            .collect()
    }

    pub(crate) fn toggle_entry(&mut self, var_path: &EntryPath, cx: &mut Context<Self>) {
        let Some(entry) = self.entry_states.get_mut(var_path) else {
            log::error!("Could not find variable list entry state to toggle");
            return;
        };

        entry.is_expanded = !entry.is_expanded;
        self.build_entries(cx);
    }

    fn select_first(&mut self, _: &SelectFirst, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel(&Default::default(), window, cx);
        if let Some(variable) = self.entries.first() {
            self.selection = Some(variable.path.clone());
            self.build_entries(cx);
        }
    }

    fn select_last(&mut self, _: &SelectLast, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel(&Default::default(), window, cx);
        if let Some(variable) = self.entries.last() {
            self.selection = Some(variable.path.clone());
            self.build_entries(cx);
        }
    }

    fn select_prev(&mut self, _: &SelectPrevious, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel(&Default::default(), window, cx);
        if let Some(selection) = &self.selection {
            let index = self.entries.iter().enumerate().find_map(|(ix, var)| {
                if &var.path == selection && ix > 0 {
                    Some(ix.saturating_sub(1))
                } else {
                    None
                }
            });

            if let Some(new_selection) =
                index.and_then(|ix| self.entries.get(ix).map(|var| var.path.clone()))
            {
                self.selection = Some(new_selection);
                self.build_entries(cx);
            } else {
                self.select_last(&SelectLast, window, cx);
            }
        } else {
            self.select_last(&SelectLast, window, cx);
        }
    }

    fn select_next(&mut self, _: &SelectNext, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel(&Default::default(), window, cx);
        if let Some(selection) = &self.selection {
            let index = self.entries.iter().enumerate().find_map(|(ix, var)| {
                if &var.path == selection {
                    Some(ix.saturating_add(1))
                } else {
                    None
                }
            });

            if let Some(new_selection) =
                index.and_then(|ix| self.entries.get(ix).map(|var| var.path.clone()))
            {
                self.selection = Some(new_selection);
                self.build_entries(cx);
            } else {
                self.select_first(&SelectFirst, window, cx);
            }
        } else {
            self.select_first(&SelectFirst, window, cx);
        }
    }

    fn cancel(&mut self, _: &menu::Cancel, window: &mut Window, cx: &mut Context<Self>) {
        self.edited_path.take();
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    fn confirm(&mut self, _: &menu::Confirm, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some((var_path, editor)) = self.edited_path.take() {
            let Some(state) = self.entry_states.get(&var_path) else {
                return;
            };

            let variables_reference = state.parent_reference;
            let Some(name) = var_path.leaf_name else {
                return;
            };

            let Some(stack_frame_id) = self.selected_stack_frame_id else {
                return;
            };

            let value = editor.read(cx).text(cx);

            self.session.update(cx, |session, cx| {
                session.set_variable_value(
                    stack_frame_id,
                    variables_reference,
                    name.into(),
                    value,
                    cx,
                )
            });
        }
    }

    fn collapse_selected_entry(
        &mut self,
        _: &CollapseSelectedEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ref selected_entry) = self.selection {
            let Some(entry_state) = self.entry_states.get_mut(selected_entry) else {
                debug_panic!("Trying to toggle variable in variable list that has an no state");
                return;
            };

            if !entry_state.is_expanded || !entry_state.has_children {
                self.select_prev(&SelectPrevious, window, cx);
            } else {
                entry_state.is_expanded = false;
                self.build_entries(cx);
            }
        }
    }

    fn expand_selected_entry(
        &mut self,
        _: &ExpandSelectedEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(selected_entry) = &self.selection {
            let Some(entry_state) = self.entry_states.get_mut(selected_entry) else {
                debug_panic!("Trying to toggle variable in variable list that has an no state");
                return;
            };

            if entry_state.is_expanded || !entry_state.has_children {
                self.select_next(&SelectNext, window, cx);
            } else {
                entry_state.is_expanded = true;
                self.build_entries(cx);
            }
        }
    }

    fn jump_to_variable_memory(
        &mut self,
        _: &GoToMemory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        _ = maybe!({
            let selection = self.selection.as_ref()?;
            let entry = self.entries.iter().find(|entry| &entry.path == selection)?;
            let var = entry.entry.as_variable()?;
            let memory_reference = var.memory_reference.as_deref()?;

            let sizeof_expr = if var.type_.as_ref().is_some_and(|t| {
                t.chars()
                    .all(|c| c.is_whitespace() || c.is_alphabetic() || c == '*')
            }) {
                var.type_.as_deref()
            } else {
                var.evaluate_name
                    .as_deref()
                    .map(|name| name.strip_prefix("/nat ").unwrap_or_else(|| name))
            };
            self.memory_view.update(cx, |this, cx| {
                this.go_to_memory_reference(
                    memory_reference,
                    sizeof_expr,
                    self.selected_stack_frame_id,
                    cx,
                );
            });
            let weak_panel = self.weak_running.clone();

            window.defer(cx, move |window, cx| {
                _ = weak_panel.update(cx, |this, cx| {
                    this.activate_item(
                        crate::persistence::DebuggerPaneItem::MemoryView,
                        window,
                        cx,
                    );
                });
            });
            Some(())
        });
    }

    fn deploy_list_entry_context_menu(
        &mut self,
        entry: ListEntry,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (supports_set_variable, supports_data_breakpoints, _supports_go_to_memory) =
            self.session.read_with(cx, |session, _| {
                (
                    session
                        .capabilities()
                        .supports_set_variable
                        .unwrap_or_default(),
                    session
                        .capabilities()
                        .supports_data_breakpoints
                        .unwrap_or_default(),
                    session
                        .capabilities()
                        .supports_read_memory_request
                        .unwrap_or_default(),
                )
            });
        let can_toggle_data_breakpoint = entry
            .as_variable()
            .filter(|_| supports_data_breakpoints)
            .and_then(|variable| {
                let variables_reference = self
                    .entry_states
                    .get(&entry.path)
                    .map(|state| state.parent_reference)?;
                Some(self.session.update(cx, |session, cx| {
                    session.data_breakpoint_info(
                        Arc::new(DataBreakpointContext::Variable {
                            variables_reference,
                            name: variable.name.clone(),
                            bytes: None,
                        }),
                        None,
                        cx,
                    )
                }))
            });

        let focus_handle = self.focus_handle.clone();
        cx.spawn_in(window, async move |this, cx| {
            let can_toggle_data_breakpoint = if let Some(task) = can_toggle_data_breakpoint {
                task.await
            } else {
                None
            };
            cx.update(|window, cx| {
                // Spork IDE: IntelliJ's `XDebugger.Variables.Tree.Popup`, in its order; the app's
                // actions where it serves the session (`ValueActionsHook`), what the debug
                // adapter protocol can't do greyed
                let app_action = |cx: &App, name: &str, data: Option<serde_json::Value>| {
                    cx.build_action(name, data).ok()
                };
                let evaluate = app_action(cx, "run::EvaluateExpression", None);
                let inspect = app_action(cx, "run::Inspect", None);
                let mark = app_action(cx, "run::MarkObject", None);
                let jump_to_source = app_action(cx, "run::JumpToSource", None);
                let jump_to_type_source = app_action(cx, "run::JumpToTypeSource", None);
                let show_types_action = app_action(cx, "run::ShowTypes", None);
                let selected = this
                    .update(cx, |this, _| this.selected_variable())
                    .ok()
                    .flatten();
                let actions = selected
                    .as_ref()
                    .and_then(|(variable, depth)| {
                        let hook = cx.try_global::<ValueActionsHook>()?.clone();
                        let session = this.read_with(cx, |this, _| this.session.clone()).ok()?;
                        (hook.0)(&session, variable, *depth, cx)
                    })
                    .unwrap_or_default();
                let renderers: Vec<(SharedString, bool, Option<Box<dyn Action>>)> = actions
                    .renderers
                    .iter()
                    .map(|(renderer, on)| {
                        let action = app_action(
                            cx,
                            "run::ViewAs",
                            Some(serde_json::json!({ "renderer": renderer })),
                        );
                        (renderer.clone(), *on, action)
                    })
                    .collect();
                let auto = std::rc::Rc::new(app_action(
                    cx,
                    "run::ViewAs",
                    Some(serde_json::json!({ "renderer": "" })),
                ));
                let renderers = std::rc::Rc::new(renderers);
                let context_menu = ContextMenu::build(window, cx, |menu, _, _| {
                    let greyed = |label: &'static str| ContextMenuEntry::new(label).disabled(true);
                    let app = |menu: ContextMenu,
                               label: &'static str,
                               action: &Option<Box<dyn Action>>,
                               enabled: bool| match action {
                        Some(action) if enabled => menu.action(label, action.boxed_clone()),
                        _ => menu.item(greyed(label)),
                    };
                    let variable = entry.as_variable().is_some();
                    let menu = menu.item(greyed("New Class Level Watch…"));
                    let menu = app(menu, "Inspect…", &inspect, selected.is_some());
                    let menu = match actions.mark {
                        Some(true) => app(menu, "Unmark Object", &mark, true),
                        Some(false) => app(menu, "Mark Object…", &mark, true),
                        None => menu.item(greyed("Mark Object…")),
                    };
                    let menu = menu
                        .action_disabled_when(
                            !supports_set_variable,
                            "Set Value…",
                            EditVariable.boxed_clone(),
                        )
                        .action("Copy Value", CopyVariableValue.boxed_clone())
                        .item(greyed("Compare Value with Clipboard"))
                        .action("Copy Name", CopyVariableName.boxed_clone())
                        .separator();
                    let menu = app(menu, "Evaluate Expression…", &evaluate, true);
                    let menu = menu
                        .when(variable, |menu| {
                            menu.action("Add to Watches", AddWatch.boxed_clone())
                        })
                        .when(entry.as_watcher().is_some(), |menu| {
                            menu.action("Remove Watch", RemoveWatch.boxed_clone())
                        })
                        // IntelliJ's for a field: java-debug's watchpoints are on fields
                        .when_some(can_toggle_data_breakpoint, |menu, _| {
                            menu.action(
                                "Toggle Field Watchpoint",
                                crate::ToggleDataBreakpoint {
                                    access_type: Some(dap::DataBreakpointAccessType::Write),
                                }
                                .boxed_clone(),
                            )
                        })
                        .item(greyed("Show Referring Objects…"))
                        .separator();
                    let menu = app(
                        menu,
                        "Jump To Source",
                        &jump_to_source,
                        actions.jump_to_source,
                    );
                    let menu = app(
                        menu,
                        "Jump To Type Source",
                        &jump_to_type_source,
                        actions.jump_to_type_source,
                    );
                    let menu = menu.separator().item(greyed("View Text"));
                    let menu = match renderers.is_empty() {
                        true => menu.item(greyed("View as")),
                        false => {
                            let (renderers, auto) = (renderers.clone(), auto.clone());
                            menu.submenu("View as", move |menu, _, _| {
                                let mut menu = menu.item(greyed("Create…")).separator();
                                if let Some(auto) = &*auto {
                                    menu = menu.action("Auto", auto.boxed_clone()).separator();
                                }
                                renderers.iter().fold(
                                    menu,
                                    |menu, (label, on, action)| match action {
                                        Some(action) => menu.action_checked(
                                            label.clone(),
                                            action.boxed_clone(),
                                            *on,
                                        ),
                                        None => menu.item(
                                            ContextMenuEntry::new(label.clone()).disabled(true),
                                        ),
                                    },
                                )
                            })
                        }
                    };
                    let menu = menu
                        .item(greyed("Filter…"))
                        .item(greyed("Adjust Range…"))
                        .separator();
                    let menu = match (actions.show_types, &show_types_action) {
                        (Some(on), Some(action)) => {
                            menu.action_checked("Show types", action.boxed_clone(), on)
                        }
                        _ => menu.item(greyed("Show types")),
                    };
                    menu.item(greyed("Mute Renderers"))
                        .item(greyed("Customize Data Views…"))
                        .context(focus_handle.clone())
                });
                _ = this.update(cx, |this, cx| {
                    cx.focus_view(&context_menu, window);
                    let subscription = cx.subscribe_in(
                        &context_menu,
                        window,
                        |this, _, _: &DismissEvent, window, cx| {
                            if this.open_context_menu.as_ref().is_some_and(|context_menu| {
                                context_menu.0.focus_handle(cx).contains_focused(window, cx)
                            }) {
                                cx.focus_self(window);
                            }
                            this.open_context_menu.take();
                            cx.notify();
                        },
                    );

                    this.open_context_menu = Some((context_menu, position, subscription));
                });
            })
        })
        .detach();
    }

    fn toggle_data_breakpoint(
        &mut self,
        data_info: &crate::ToggleDataBreakpoint,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self
            .selection
            .as_ref()
            .and_then(|selection| self.entries.iter().find(|entry| &entry.path == selection))
        else {
            return;
        };

        let Some((name, var_ref)) = entry.as_variable().map(|var| &var.name).zip(
            self.entry_states
                .get(&entry.path)
                .map(|state| state.parent_reference),
        ) else {
            return;
        };

        let context = Arc::new(DataBreakpointContext::Variable {
            variables_reference: var_ref,
            name: name.clone(),
            bytes: None,
        });
        let data_breakpoint = self.session.update(cx, |session, cx| {
            session.data_breakpoint_info(context.clone(), None, cx)
        });

        let session = self.session.downgrade();
        let access_type = data_info.access_type;
        cx.spawn(async move |_, cx| {
            let Some((data_id, access_types)) = data_breakpoint
                .await
                .and_then(|info| Some((info.data_id?, info.access_types)))
            else {
                return;
            };
            // Spork IDE: the app's field watchpoint, kept with the breakpoints
            let handled = session.upgrade().is_some_and(|session| {
                cx.update(|cx| match cx.try_global::<FieldWatchpointHook>().cloned() {
                    Some(FieldWatchpointHook(hook)) => hook(&session, &data_id, cx),
                    None => false,
                })
            });
            if handled {
                return;
            }

            // Because user's can manually add this action to the keymap
            // we check if access type is supported
            let access_type = match access_types {
                None => None,
                Some(access_types) => {
                    if access_type.is_some_and(|access_type| access_types.contains(&access_type)) {
                        access_type
                    } else {
                        None
                    }
                }
            };
            _ = session.update(cx, |session, cx| {
                session.create_data_breakpoint(
                    context,
                    data_id.clone(),
                    dap::DataBreakpoint {
                        data_id,
                        access_type,
                        condition: None,
                        hit_condition: None,
                    },
                    cx,
                );
                cx.notify();
            });
        })
        .detach();
    }

    fn copy_variable_name(
        &mut self,
        _: &CopyVariableName,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.selection.as_ref() else {
            return;
        };

        let Some(entry) = self.entries.iter().find(|entry| &entry.path == selection) else {
            return;
        };

        let variable_name = match &entry.entry {
            DapEntry::Variable(dap) => dap.name.clone(),
            DapEntry::Watcher(watcher) => watcher.expression.to_string(),
            DapEntry::Scope(_) => return,
        };

        cx.write_to_clipboard(ClipboardItem::new_string(variable_name));
    }

    fn copy_variable_value(
        &mut self,
        _: &CopyVariableValue,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.selection.as_ref() else {
            return;
        };

        let Some(entry) = self.entries.iter().find(|entry| &entry.path == selection) else {
            return;
        };

        match &entry.entry {
            DapEntry::Variable(dap) => {
                let fallback_value = dap.value.clone();
                let expression = dap
                    .evaluate_name
                    .clone()
                    .unwrap_or_else(|| dap.name.clone());
                let frame_id = self.selected_stack_frame_id;
                let task = self.session.update(cx, |session, cx| {
                    session.evaluate_variable_value(expression, frame_id, cx)
                });
                cx.spawn(async move |_, cx| {
                    let value = task.await.unwrap_or(fallback_value);
                    cx.update(|cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(value));
                    });
                })
                .detach();
            }
            DapEntry::Watcher(watcher) => {
                cx.write_to_clipboard(ClipboardItem::new_string(watcher.value.to_string()));
            }
            DapEntry::Scope(_) => return,
        }
    }

    fn edit_variable(&mut self, _: &EditVariable, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.selection.as_ref() else {
            return;
        };

        let Some(entry) = self.entries.iter().find(|entry| &entry.path == selection) else {
            return;
        };

        let variable_value = match &entry.entry {
            DapEntry::Watcher(watcher) => watcher.value.to_string(),
            DapEntry::Variable(variable) => variable.value.clone(),
            DapEntry::Scope(_) => return,
        };

        let editor = Self::create_variable_editor(&variable_value, window, cx);
        self.edited_path = Some((entry.path.clone(), editor));

        cx.notify();
    }

    fn add_watcher(&mut self, _: &AddWatch, _: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.selection.as_ref() else {
            return;
        };

        let Some(entry) = self.entries.iter().find(|entry| &entry.path == selection) else {
            return;
        };

        let Some(variable) = entry.as_variable() else {
            return;
        };

        let Some(stack_frame_id) = self.selected_stack_frame_id else {
            return;
        };

        let add_watcher_task = self.session.update(cx, |session, cx| {
            let expression = variable
                .evaluate_name
                .clone()
                .unwrap_or_else(|| variable.name.clone());

            session.add_watcher(expression.into(), stack_frame_id, cx)
        });

        cx.spawn(async move |this, cx| {
            add_watcher_task.await?;

            this.update(cx, |this, cx| {
                this.build_entries(cx);
            })
        })
        .detach_and_log_err(cx);
    }

    fn remove_watcher(&mut self, _: &RemoveWatch, _: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.selection.as_ref() else {
            return;
        };

        let Some(entry) = self.entries.iter().find(|entry| &entry.path == selection) else {
            return;
        };

        let Some(watcher) = entry.as_watcher() else {
            return;
        };

        self.session.update(cx, |session, _| {
            session.remove_watcher(watcher.expression.clone());
        });
        self.build_entries(cx);
    }

    #[track_caller]
    #[cfg(test)]
    pub(crate) fn assert_visual_entries(&self, expected: Vec<&str>) {
        const INDENT: &str = "    ";

        let entries = &self.entries;
        let mut visual_entries = Vec::with_capacity(entries.len());
        for entry in entries {
            let state = self
                .entry_states
                .get(&entry.path)
                .expect("If there's a variable entry there has to be a state that goes with it");

            visual_entries.push(format!(
                "{}{} {}{}",
                INDENT.repeat(state.depth - 1),
                if state.is_expanded { "v" } else { ">" },
                entry.entry.name(),
                if self.selection.as_ref() == Some(&entry.path) {
                    " <=== selected"
                } else {
                    ""
                }
            ));
        }

        pretty_assertions::assert_eq!(expected, visual_entries);
    }

    #[track_caller]
    #[cfg(test)]
    pub(crate) fn scopes(&self) -> Vec<dap::Scope> {
        self.entries
            .iter()
            .filter_map(|entry| match &entry.entry {
                DapEntry::Scope(scope) => Some(scope),
                _ => None,
            })
            .cloned()
            .collect()
    }

    #[track_caller]
    #[cfg(test)]
    pub(crate) fn variables_per_scope(&self) -> Vec<(dap::Scope, Vec<dap::Variable>)> {
        let mut scopes: Vec<(dap::Scope, Vec<_>)> = Vec::new();
        let mut idx = 0;

        for entry in self.entries.iter() {
            match &entry.entry {
                DapEntry::Watcher { .. } => continue,
                DapEntry::Variable(dap) => scopes[idx].1.push(dap.clone()),
                DapEntry::Scope(scope) => {
                    if !scopes.is_empty() {
                        idx += 1;
                    }

                    scopes.push((scope.clone(), Vec::new()));
                }
            }
        }

        scopes
    }

    #[track_caller]
    #[cfg(test)]
    pub(crate) fn variables(&self) -> Vec<dap::Variable> {
        self.entries
            .iter()
            .filter_map(|entry| match &entry.entry {
                DapEntry::Variable(variable) => Some(variable),
                _ => None,
            })
            .cloned()
            .collect()
    }

    fn create_variable_editor(default: &str, window: &mut Window, cx: &mut App) -> Entity<Editor> {
        let editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);

            let refinement = TextStyleRefinement {
                font_size: Some(
                    TextSize::XSmall
                        .rems(cx)
                        .to_pixels(window.rem_size())
                        .into(),
                ),
                ..Default::default()
            };
            editor.set_text_style_refinement(refinement);
            editor.set_text(default, window, cx);
            editor.select_all(&editor::actions::SelectAll, window, cx);
            editor
        });
        editor.focus_handle(cx).focus(window, cx);
        editor
    }

    fn variable_color(
        &self,
        presentation_hint: Option<&VariablePresentationHint>,
        value: &str,
        type_hint: Option<&str>,
        cx: &Context<Self>,
    ) -> VariableColor {
        let syntax_color_for = |name| {
            cx.theme()
                .syntax()
                .style_for_name(name)
                .and_then(|style| style.color)
        };
        let name = if self.disabled {
            Some(Color::Disabled.color(cx))
        } else {
            match presentation_hint
                .as_ref()
                .and_then(|hint| hint.kind.as_ref())
                .unwrap_or(&VariablePresentationHintKind::Unknown)
            {
                VariablePresentationHintKind::Class
                | VariablePresentationHintKind::BaseClass
                | VariablePresentationHintKind::InnerClass
                | VariablePresentationHintKind::MostDerivedClass => syntax_color_for("type"),
                VariablePresentationHintKind::Data => syntax_color_for("variable"),
                VariablePresentationHintKind::Unknown | _ => syntax_color_for("variable"),
            }
        };
        let value = self
            .disabled
            .then(|| Color::Disabled.color(cx))
            .or_else(|| {
                syntax_color_for(Self::syntax_token_for_value(value, type_hint))
                    .or_else(|| syntax_color_for("variable.special"))
            });

        VariableColor { name, value }
    }

    // pattern match by variable value as types are called different things
    fn syntax_token_for_value(value: &str, type_hint: Option<&str>) -> &'static str {
        if let Some(hint) = type_hint {
            match hint.trim().to_ascii_lowercase().as_str() {
                "bool" | "boolean" => return "boolean",
                "nonetype" | "null" | "nil" | "none" | "undefined" | "void" => return "comment",
                "str" | "string" | "char" | "&str" | "string_view" | "std::string" => {
                    return "string";
                }
                "int" | "integer" | "long" | "short" | "byte" | "float" | "double" | "number"
                | "int8" | "int16" | "int32" | "int64" | "uint8" | "uint16" | "uint32"
                | "uint64" | "usize" | "isize" | "size_t" | "f32" | "f64" => return "number",
                _ => {}
            }
        }

        let trimmed = value.trim();
        let is_quoted = |quote: char| {
            trimmed.len() >= 2 && trimmed.starts_with(quote) && trimmed.ends_with(quote)
        };
        if is_quoted('"') || is_quoted('\'') {
            return "string";
        }
        match trimmed {
            "true" | "false" | "True" | "False" | "TRUE" | "FALSE" => "boolean",
            "nil" | "null" | "None" | "NULL" | "nullptr" | "undefined" | "NoneType" => "comment",
            _ if !trimmed.is_empty() && trimmed.parse::<f64>().is_ok() => "number",
            _ => "variable.special",
        }
    }

    fn render_variable_value(
        &self,
        entry: &ListEntry,
        variable_color: &VariableColor,
        value: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !value.is_empty() {
            div()
                .w_full()
                .id(entry.item_value_id())
                .map(|this| {
                    if let Some((_, editor)) = self
                        .edited_path
                        .as_ref()
                        .filter(|(path, _)| path == &entry.path)
                    {
                        this.child(div().size_full().px_2().child(editor.clone()))
                    } else {
                        this.text_color(cx.theme().colors().text_muted)
                            .when(
                                !self.disabled
                                    && self
                                        .session
                                        .read(cx)
                                        .capabilities()
                                        .supports_set_variable
                                        .unwrap_or_default(),
                                |this| {
                                    let path = entry.path.clone();
                                    let variable_value = value.clone();
                                    this.on_click(cx.listener(
                                        move |this, click: &ClickEvent, window, cx| {
                                            if click.click_count() < 2 {
                                                return;
                                            }
                                            let editor = Self::create_variable_editor(
                                                &variable_value,
                                                window,
                                                cx,
                                            );
                                            this.edited_path = Some((path.clone(), editor));

                                            cx.notify();
                                        },
                                    ))
                                },
                            )
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .child(
                                        Label::new("=  ")
                                            .single_line()
                                            .size(LabelSize::Small)
                                            .color(Color::Muted)
                                            .flex_shrink_0(),
                                    )
                                    .child(
                                        Label::new(value.clone())
                                            .single_line()
                                            .truncate()
                                            .size(LabelSize::Small)
                                            .color(Color::Muted)
                                            .when_some(variable_color.value, |this, color| {
                                                this.color(Color::from(color))
                                            })
                                            .flex_1(),
                                    ),
                            )
                            .tooltip(Tooltip::text(value))
                    }
                })
                .into_any_element()
        } else {
            Empty.into_any_element()
        }
    }

    fn center_truncate_string(s: &str, mut max_chars: usize) -> String {
        const ELLIPSIS: &str = "...";
        const MIN_LENGTH: usize = 3;

        max_chars = max_chars.max(MIN_LENGTH);

        let char_count = s.chars().count();
        if char_count <= max_chars {
            return s.to_string();
        }

        if ELLIPSIS.len() + MIN_LENGTH > max_chars {
            return s.chars().take(MIN_LENGTH).collect();
        }

        let available_chars = max_chars - ELLIPSIS.len();

        let start_chars = available_chars / 2;
        let end_chars = available_chars - start_chars;
        let skip_chars = char_count - end_chars;

        let mut start_boundary = 0;
        let mut end_boundary = s.len();

        for (i, (byte_idx, _)) in s.char_indices().enumerate() {
            if i == start_chars {
                start_boundary = byte_idx.max(MIN_LENGTH);
            }

            if i == skip_chars {
                end_boundary = byte_idx;
            }
        }

        if start_boundary >= end_boundary {
            return s.chars().take(MIN_LENGTH).collect();
        }

        format!("{}{}{}", &s[..start_boundary], ELLIPSIS, &s[end_boundary..])
    }

    fn render_watcher(
        &self,
        entry: &ListEntry,
        state: EntryState,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(watcher) = &entry.as_watcher() else {
            debug_panic!("Called render watcher on non watcher variable list entry variant");
            return div().into_any_element();
        };

        let variable_color =
            self.variable_color(watcher.presentation_hint.as_ref(), &watcher.value, None, cx);

        let is_selected = self
            .selection
            .as_ref()
            .is_some_and(|selection| selection == &entry.path);
        let var_ref = watcher.variables_reference;

        let colors = get_entry_color(cx);
        let bg_hover_color = if !is_selected {
            colors.hover
        } else {
            colors.default
        };
        let border_color = if is_selected {
            colors.marked_active
        } else {
            colors.default
        };
        let path = entry.path.clone();

        let weak = cx.weak_entity();
        let focus_handle = self.focus_handle.clone();
        let watcher_len = (f32::from(self.list_handle.content_size().width / 12.0).floor()) - 3.0;
        let watcher_len = watcher_len as usize;

        div()
            .id(entry.item_id())
            // Spork IDE: the row as the app's tests and screen readers find it
            .role(gpui::Role::TreeItem)
            .aria_label(SharedString::from(format!(
                "{} = {}",
                watcher.expression, watcher.value
            )))
            .aria_selected(is_selected)
            .group("variable_list_entry")
            .pl_2()
            .border_1()
            .border_r_2()
            .border_color(border_color)
            .flex()
            .w_full()
            .h_full()
            .hover(|style| style.bg(bg_hover_color))
            .on_click(cx.listener({
                let path = path.clone();
                move |this, _, _window, cx| {
                    this.selection = Some(path.clone());
                    cx.notify();
                }
            }))
            .child(
                ListItem::new(SharedString::from(format!(
                    "watcher-{}",
                    watcher.expression
                )))
                .selectable(false)
                .disabled(self.disabled)
                .selectable(false)
                .indent_level(state.depth)
                .indent_step_size(INDENT_STEP_SIZE)
                .always_show_disclosure_icon(true)
                .when(var_ref > 0, |list_item| {
                    list_item.toggle(state.is_expanded).on_toggle(cx.listener({
                        let var_path = entry.path.clone();
                        move |this, _, _, cx| {
                            this.session.update(cx, |session, cx| {
                                session.variables(var_ref, cx);
                            });

                            this.toggle_entry(&var_path, cx);
                        }
                    }))
                })
                .on_secondary_mouse_down(cx.listener({
                    let path = path.clone();
                    let entry = entry.clone();
                    move |this, event: &MouseDownEvent, window, cx| {
                        this.selection = Some(path.clone());
                        this.deploy_list_entry_context_menu(
                            entry.clone(),
                            event.position,
                            window,
                            cx,
                        );
                        cx.stop_propagation();
                    }
                }))
                .child(
                    h_flex()
                        .gap_1()
                        .text_ui_sm(cx)
                        .w_full()
                        .child(
                            Label::new(&Self::center_truncate_string(
                                watcher.expression.as_ref(),
                                watcher_len,
                            ))
                            .when_some(variable_color.name, |this, color| {
                                this.color(Color::from(color))
                            }),
                        )
                        .child(self.render_variable_value(
                            entry,
                            &variable_color,
                            watcher.value.to_string(),
                            cx,
                        )),
                )
                .end_slot(
                    IconButton::new(
                        SharedString::from(format!("watcher-{}-remove-button", watcher.expression)),
                        IconName::Close,
                    )
                    .on_click({
                        move |_, window, cx| {
                            weak.update(cx, |variable_list, cx| {
                                variable_list.selection = Some(path.clone());
                                variable_list.remove_watcher(&RemoveWatch, window, cx);
                            })
                            .ok();
                        }
                    })
                    .tooltip(move |_window, cx| {
                        Tooltip::for_action_in("Remove Watch", &RemoveWatch, &focus_handle, cx)
                    })
                    .icon_size(ui::IconSize::Indicator),
                ),
            )
            .into_any()
    }

    fn render_scope(
        &self,
        entry: &ListEntry,
        state: EntryState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(scope) = entry.as_scope() else {
            debug_panic!("Called render scope on non scope variable list entry variant");
            return div().into_any_element();
        };

        let var_ref = scope.variables_reference;
        let is_selected = self
            .selection
            .as_ref()
            .is_some_and(|selection| selection == &entry.path);

        let colors = get_entry_color(cx);
        let bg_hover_color = if !is_selected {
            colors.hover
        } else {
            colors.default
        };
        let border_color = if is_selected {
            colors.marked_active
        } else {
            colors.default
        };
        let path = entry.path.clone();

        div()
            .id(var_ref as usize)
            .group("variable_list_entry")
            .pl_2()
            .border_1()
            .border_r_2()
            .border_color(border_color)
            .flex()
            .w_full()
            .h_full()
            .hover(|style| style.bg(bg_hover_color))
            .on_click(cx.listener({
                move |this, _, _window, cx| {
                    this.selection = Some(path.clone());
                    cx.notify();
                }
            }))
            .child(
                ListItem::new(SharedString::from(format!("scope-{}", var_ref)))
                    .selectable(false)
                    .disabled(self.disabled)
                    .indent_level(state.depth)
                    .indent_step_size(px(10.))
                    .always_show_disclosure_icon(true)
                    .toggle(state.is_expanded)
                    .on_toggle({
                        let var_path = entry.path.clone();
                        cx.listener(move |this, _, _, cx| this.toggle_entry(&var_path, cx))
                    })
                    .child(
                        div()
                            .text_ui(cx)
                            .w_full()
                            .truncate()
                            .when(self.disabled, |this| {
                                this.text_color(Color::Disabled.color(cx))
                            })
                            .child(scope.name.clone()),
                    ),
            )
            .into_any()
    }

    fn render_variable(
        &self,
        variable: &ListEntry,
        state: EntryState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(dap) = &variable.as_variable() else {
            debug_panic!("Called render variable on non variable variable list entry variant");
            return div().into_any_element();
        };

        let variable_color = self.variable_color(
            dap.presentation_hint.as_ref(),
            &dap.value,
            dap.type_.as_deref(),
            cx,
        );

        let var_ref = dap.variables_reference;
        let colors = get_entry_color(cx);
        let is_selected = self
            .selection
            .as_ref()
            .is_some_and(|selected_path| *selected_path == variable.path);

        let bg_hover_color = if !is_selected {
            colors.hover
        } else {
            colors.default
        };
        let border_color = if is_selected && self.focus_handle.contains_focused(window, cx) {
            colors.marked_active
        } else {
            colors.default
        };
        let path = variable.path.clone();
        div()
            .id(variable.item_id())
            // Spork IDE: the row as the app's tests and screen readers find it
            .role(gpui::Role::TreeItem)
            .aria_label(SharedString::from(format!("{} = {}", dap.name, dap.value)))
            .aria_selected(is_selected)
            .group("variable_list_entry")
            .pl_2()
            .border_1()
            .border_r_2()
            .border_color(border_color)
            .h_4()
            .size_full()
            .hover(|style| style.bg(bg_hover_color))
            .on_click(cx.listener({
                let path = path.clone();
                move |this, _, _window, cx| {
                    this.selection = Some(path.clone());
                    cx.notify();
                }
            }))
            .child(
                ListItem::new(SharedString::from(format!(
                    "variable-item-{}-{}",
                    dap.name, state.depth
                )))
                .disabled(self.disabled)
                .selectable(false)
                .indent_level(state.depth)
                .indent_step_size(INDENT_STEP_SIZE)
                .always_show_disclosure_icon(true)
                .when(var_ref > 0, |list_item| {
                    list_item.toggle(state.is_expanded).on_toggle(cx.listener({
                        let var_path = variable.path.clone();
                        move |this, _, _, cx| {
                            this.session.update(cx, |session, cx| {
                                session.variables(var_ref, cx);
                            });

                            this.toggle_entry(&var_path, cx);
                        }
                    }))
                })
                .on_secondary_mouse_down(cx.listener({
                    let entry = variable.clone();
                    move |this, event: &MouseDownEvent, window, cx| {
                        this.selection = Some(path.clone());
                        this.deploy_list_entry_context_menu(
                            entry.clone(),
                            event.position,
                            window,
                            cx,
                        );
                        cx.stop_propagation();
                    }
                }))
                .child(
                    h_flex()
                        .gap_1()
                        .text_ui_sm(cx)
                        .w_full()
                        // Spork IDE: the value's markup, `[label] ` in its colour
                        .when_some(self.markup_of(dap, cx), |row, (label, color)| {
                            row.child(
                                div()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(color)
                                    .child(format!("[{label}]")),
                            )
                        })
                        .child(
                            Label::new(&dap.name).when_some(variable_color.name, |this, color| {
                                this.color(Color::from(color))
                            }),
                        )
                        .child(self.render_variable_value(
                            variable,
                            &variable_color,
                            dap.value.clone(),
                            cx,
                        )),
                ),
            )
            .into_any()
    }
}

impl Focusable for VariableList {
    fn focus_handle(&self, _: &App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for VariableList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .track_focus(&self.focus_handle)
            .key_context("VariableList")
            .id("variable-list")
            .group("variable-list")
            .size_full()
            .on_action(cx.listener(Self::select_first))
            .on_action(cx.listener(Self::select_last))
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::expand_selected_entry))
            .on_action(cx.listener(Self::collapse_selected_entry))
            .on_action(cx.listener(Self::copy_variable_name))
            .on_action(cx.listener(Self::copy_variable_value))
            .on_action(cx.listener(Self::edit_variable))
            .on_action(cx.listener(Self::add_watcher))
            .on_action(cx.listener(Self::remove_watcher))
            .on_action(cx.listener(Self::toggle_data_breakpoint))
            .on_action(cx.listener(Self::jump_to_variable_memory))
            .child(
                uniform_list(
                    "variable-list",
                    self.entries.len(),
                    cx.processor(move |this, range: Range<usize>, window, cx| {
                        this.render_entries(range, window, cx)
                    }),
                )
                .track_scroll(&self.list_handle)
                .with_width_from_item(self.max_width_index)
                .with_sizing_behavior(gpui::ListSizingBehavior::Auto)
                .with_horizontal_sizing_behavior(gpui::ListHorizontalSizingBehavior::Unconstrained)
                .gap_1_5()
                .size_full()
                .flex_grow_1(),
            )
            .children(self.open_context_menu.as_ref().map(|(menu, position, _)| {
                deferred(
                    anchored()
                        .position(*position)
                        .anchor(gpui::Anchor::TopLeft)
                        .child(menu.clone()),
                )
                .with_priority(1)
            }))
            // .vertical_scrollbar_for(&self.list_handle, window, cx)
            .custom_scrollbars(
                ui::Scrollbars::new(ScrollAxes::Both)
                    .tracked_scroll_handle(&self.list_handle)
                    .with_track_along(ScrollAxes::Both, cx.theme().colors().panel_background)
                    .tracked_entity(cx.entity_id()),
                window,
                cx,
            )
    }
}

struct EntryColors {
    default: Hsla,
    hover: Hsla,
    marked_active: Hsla,
}

fn get_entry_color(cx: &Context<VariableList>) -> EntryColors {
    let colors = cx.theme().colors();

    EntryColors {
        default: colors.panel_background,
        hover: colors.ghost_element_hover,
        marked_active: colors.ghost_element_selected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_center_truncate_string() {
        // Test string shorter than limit - should not be truncated
        assert_eq!(VariableList::center_truncate_string("short", 10), "short");

        // Test exact length - should not be truncated
        assert_eq!(
            VariableList::center_truncate_string("exactly_10", 10),
            "exactly_10"
        );

        // Test simple truncation
        assert_eq!(
            VariableList::center_truncate_string("value->value2->value3->value4", 20),
            "value->v...3->value4"
        );

        // Test with very long expression
        assert_eq!(
            VariableList::center_truncate_string(
                "object->property1->property2->property3->property4->property5",
                30
            ),
            "object->prope...ty4->property5"
        );

        // Test edge case with limit equal to ellipsis length
        assert_eq!(VariableList::center_truncate_string("anything", 3), "any");

        // Test edge case with limit less than ellipsis length
        assert_eq!(VariableList::center_truncate_string("anything", 2), "any");

        // Test with UTF-8 characters
        assert_eq!(
            VariableList::center_truncate_string("café->résumé->naïve->voilà", 15),
            "café->...>voilà"
        );

        // Test with emoji (multi-byte UTF-8)
        assert_eq!(
            VariableList::center_truncate_string("😀->happy->face->😎->cool", 15),
            "😀->hap...->cool"
        );
    }
}
