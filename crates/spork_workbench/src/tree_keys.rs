//! IntelliJ's keys for its trees and lists, one model the IDE's trees and lists share: the moves
//! of `TreeAction` (↑↓ cycling, ← collapsing or going to the parent, → expanding or going to the
//! first child, Home/End, Page Up/Down by the visible rows less three, the next and previous
//! sibling), ⇧ extending the selection from its anchor, ⌘A, Expand and Collapse All (⌘=, ⌘-),
//! Scroll to Center (⌃L) and speed search (`SpeedSearchBase`): typing finds the first row from
//! the selection on that matches, its match highlighted, ↑↓ Home End among the matches, ⎋ ends
//! it, ←→ ⏎ Page Up/Down end it and act.
//!
//! A view holds a [`TreeState`], implements [`TreeView`] and wraps its tree in [`tree_keys`];
//! its key context then needs `menu` (Zed's list keys) and `tree` (these, in the keymap). Its own
//! handlers of the same actions go after (⎋ ends speed search before closing a popup), or before
//! to take a key over (the Switcher's ←→ between its lists).

use std::ops::{Range, RangeInclusive};

use gpui_kit::component::h_flex;
use gpui_kit::*;

use crate::tokens::{Tokens, metrics};

actions!(
    tree,
    [
        /// Adds the previous row to the selection (⇧↑).
        SelectPreviousExtend,
        /// Adds the next row to the selection (⇧↓).
        SelectNextExtend,
        /// Selects up to the first row (⇧Home).
        SelectFirstExtend,
        /// Selects up to the last row (⇧End).
        SelectLastExtend,
        /// Selects the row a page up (Page Up).
        PageUp,
        /// Selects the row a page down (Page Down).
        PageDown,
        /// Selects up to the row a page up (⇧Page Up).
        PageUpExtend,
        /// Selects up to the row a page down (⇧Page Down).
        PageDownExtend,
        /// Adds the parent row to the selection (⇧←).
        SelectParentExtend,
        /// Expands the row, or adds its first child to the selection (⇧→).
        SelectChildExtend,
        /// Selects the parent row without collapsing the selected one.
        SelectParentNoCollapse,
        /// Selects the next row at the same level under the same parent.
        SelectNextSibling,
        /// Selects the previous row at the same level under the same parent.
        SelectPreviousSibling,
        /// Selects every row (⌘A).
        SelectAll,
        /// Expands every row (⌘=).
        ExpandAll,
        /// Collapses every row (⌘-).
        CollapseAll,
        /// Scrolls the selected row to the middle (⌃L).
        ScrollToCenter,
        /// Shows the speed search field (⌘F).
        SpeedSearch,
    ]
);

/// A row of a tree or list, as its keys see it.
#[derive(Clone, Debug)]
pub struct TreeRow<K> {
    pub key: K,
    pub depth: usize,
    /// `Some(open)` for a row with children, `None` for a leaf.
    pub expanded: Option<bool>,
    /// What speed search looks for.
    pub text: SharedString,
}

impl<K> TreeRow<K> {
    pub fn new(
        key: K,
        depth: usize,
        expanded: Option<bool>,
        text: impl Into<SharedString>,
    ) -> Self {
        Self {
            key,
            depth,
            expanded,
            text: text.into(),
        }
    }

    /// A list's row: no level, no children.
    pub fn item(key: K, text: impl Into<SharedString>) -> Self {
        Self::new(key, 0, None, text)
    }
}

/// A view whose tree or list takes IntelliJ's keys.
pub trait TreeView: Sized + 'static {
    type Key: Clone + PartialEq + 'static;

    fn tree(&mut self) -> &mut TreeState<Self::Key>;

    /// The rows shown, in order.
    fn tree_rows(&self, cx: &App) -> Vec<TreeRow<Self::Key>>;

    /// Opens or closes `key`'s row.
    fn set_expanded(
        &mut self,
        _key: &Self::Key,
        _expanded: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// Expand All and Collapse All.
    fn set_all_expanded(&mut self, _expanded: bool, _window: &mut Window, _cx: &mut Context<Self>) {
    }

    /// The selection moved: what follows it (a preview) follows.
    fn selection_changed(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    /// ⏎ on the selection.
    fn confirm(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    /// Whether typing starts speed search: a view filtering its rows as typed keeps the keys.
    fn speed_search(&self) -> bool {
        true
    }
}

/// The selection, its scroll and its speed search.
pub struct TreeState<K> {
    lead: Option<K>,
    /// Where an extended selection started; `None` while one row is selected.
    anchor: Option<K>,
    scroll: ScrollHandle,
    /// What speed search was given, while its field shows.
    search: Option<String>,
    /// A list's: Page Up and Page Down move by the visible rows less one (`ScrollingUtil`), a
    /// tree's by those less three (`TreeAction`).
    list: bool,
}

impl<K: Clone + PartialEq> Default for TreeState<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Clone + PartialEq> TreeState<K> {
    pub fn new() -> Self {
        Self {
            lead: None,
            anchor: None,
            scroll: ScrollHandle::new(),
            search: None,
            list: false,
        }
    }

    /// A list's state: Page Up and Page Down as IntelliJ's lists move.
    pub fn list() -> Self {
        Self {
            list: true,
            ..Self::new()
        }
    }

    /// The row the keys move from: the one selected, or the last one an extended selection
    /// reached.
    pub fn lead(&self) -> Option<&K> {
        self.lead.as_ref()
    }

    /// Selects `key` alone.
    pub fn select(&mut self, key: Option<K>) {
        self.lead = key;
        self.anchor = None;
    }

    /// A click on `key`'s row: ⇧ extends the selection to it.
    pub fn click(&mut self, key: K, modifiers: &Modifiers) {
        match modifiers.shift {
            true => self.extend_to(key),
            false => self.select(Some(key)),
        }
    }

    fn extend_to(&mut self, key: K) {
        if self.anchor.is_none() {
            self.anchor = self.lead.clone().or_else(|| Some(key.clone()));
        }
        self.lead = Some(key);
    }

    /// The rows selected, as positions in `rows`.
    pub fn selected_range(&self, rows: &[TreeRow<K>]) -> Option<RangeInclusive<usize>> {
        let lead = position(rows, self.lead.as_ref()?)?;
        let anchor = self
            .anchor
            .as_ref()
            .and_then(|anchor| position(rows, anchor))
            .unwrap_or(lead);
        Some(lead.min(anchor)..=lead.max(anchor))
    }

    /// The keys of the rows selected, in order.
    pub fn selected(&self, rows: &[TreeRow<K>]) -> Vec<K> {
        match self.selected_range(rows) {
            Some(range) => rows[range].iter().map(|row| row.key.clone()).collect(),
            None => Vec::new(),
        }
    }

    /// The list to give `track_scroll`: its children must be the rows, in order.
    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll
    }

    /// What speed search was given, while its field shows.
    pub fn search(&self) -> Option<&str> {
        self.search.as_deref()
    }

    pub fn end_search(&mut self) {
        self.search = None;
    }

    /// Where `text` matches what speed search was given, to highlight.
    pub fn matches(&self, text: &str) -> Option<Vec<Range<usize>>> {
        let query = self.search.as_deref().filter(|query| !query.is_empty())?;
        matching_fragments(query, text)
    }

    /// The speed search field, above the tree's top left corner: put it in the tree's parent,
    /// made `relative`. Its text turns red while no row matches.
    pub fn render_search(&self, rows: &[TreeRow<K>], cx: &App) -> Option<AnyElement> {
        let query = self.search.as_ref()?;
        let tokens = Tokens::get(cx);
        let found = query.is_empty() || rows.iter().any(|row| self.matches(&row.text).is_some());
        Some(
            h_flex()
                .id("speed-search")
                .role(Role::TextInput)
                .aria_label("Speed search")
                .aria_value(SharedString::from(query.clone()))
                .absolute()
                .left_0()
                .top(px(-SEARCH_HEIGHT))
                .h(px(SEARCH_HEIGHT))
                .px(px(8.))
                .gap(px(10.))
                .bg(tokens.island)
                .border_1()
                .border_color(tokens.control_border)
                .text_color(match found {
                    true => tokens.text,
                    false => tokens.text_error,
                })
                .child(crate::icon("general/search").size(px(16.)))
                .child(query.clone())
                .into_any_element(),
        )
    }

    fn scroll_to(&self, ix: usize) {
        self.scroll.scroll_to_item(ix);
    }

    /// The rows Page Up and Page Down move by.
    fn page(&self) -> usize {
        let visible = (self.scroll.bounds().size.height / px(metrics::ROW_HEIGHT)) as usize;
        visible.saturating_sub(if self.list { 1 } else { 3 }).max(1)
    }

    fn scroll_to_center(&self, ix: usize) {
        let Some(item) = self.scroll.bounds_for_item(ix) else {
            return;
        };
        let bounds = self.scroll.bounds();
        let max = self.scroll.max_offset();
        let y = bounds.top() + bounds.size.height / 2. - item.top() - item.size.height / 2.;
        let mut offset = self.scroll.offset();
        offset.y = y.clamp(-max.y, px(0.));
        self.scroll.set_offset(offset);
    }

    /// The matching row after (or before) `from`, around the end as IntelliJ's cycle scrolling.
    fn next_match(&self, rows: &[TreeRow<K>], from: usize, step: Step) -> Option<usize> {
        let found = |ix: &usize| self.matches(&rows[*ix].text).is_some();
        let len = rows.len();
        match step {
            Step::First => (0..len).find(found),
            Step::Last => (0..len).rev().find(found),
            Step::Next => (1..=len).map(|ahead| (from + ahead) % len).find(found),
            Step::Previous => (1..=len)
                .map(|back| (from + len - back % len) % len)
                .find(found),
            _ => None,
        }
    }
}

fn position<K: PartialEq>(rows: &[TreeRow<K>], key: &K) -> Option<usize> {
    rows.iter().position(|row| &row.key == key)
}

const SEARCH_HEIGHT: f32 = 26.;

/// A move of the keys.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Step {
    Previous,
    Next,
    First,
    Last,
    PageUp,
    PageDown,
    /// ←: collapses an open row, else goes to its parent.
    Parent,
    ParentNoCollapse,
    /// →: expands a closed row, else goes to the next row (an open row's first child).
    Child,
    NextSibling,
    PreviousSibling,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Outcome {
    Select(usize),
    Expand(usize),
    Collapse(usize),
}

/// Where `step` goes from `lead` in `rows`, as IntelliJ's `TreeAction`; `cycle` lets ↑↓ go around
/// the ends (not while extending the selection).
fn navigate<K>(
    rows: &[TreeRow<K>],
    lead: Option<usize>,
    step: Step,
    page: usize,
    cycle: bool,
) -> Option<Outcome> {
    let len = rows.len();
    if len == 0 {
        return None;
    }
    let last = len - 1;
    let Some(lead) = lead.filter(|lead| *lead < len) else {
        return Some(Outcome::Select(match step {
            Step::Last | Step::PageDown => last,
            _ => 0,
        }));
    };
    let row = &rows[lead];
    let parent = || (0..lead).rev().find(|ix| rows[*ix].depth < row.depth);
    let select = |ix: usize| (ix != lead).then_some(Outcome::Select(ix));
    match step {
        Step::Next if lead == last => cycle.then(|| select(0)).flatten(),
        Step::Next => select(lead + 1),
        Step::Previous if lead == 0 => cycle.then(|| select(last)).flatten(),
        Step::Previous => select(lead - 1),
        Step::First => select(0),
        Step::Last => select(last),
        Step::PageDown => select((lead + page).min(last)),
        Step::PageUp => select(lead.saturating_sub(page)),
        Step::Child => match row.expanded {
            Some(false) => Some(Outcome::Expand(lead)),
            _ if lead < last => select(lead + 1),
            _ => None,
        },
        Step::Parent if row.expanded == Some(true) => Some(Outcome::Collapse(lead)),
        Step::Parent | Step::ParentNoCollapse => match parent() {
            Some(parent) => select(parent),
            // a top row's parent is the hidden root: the open row above, as `TreeAction`
            None => select(
                (0..lead)
                    .rev()
                    .find(|ix| rows[*ix].expanded == Some(true))
                    .unwrap_or(0),
            ),
        },
        Step::NextSibling => rows[lead + 1..]
            .iter()
            .position(|other| other.depth <= row.depth)
            .map(|ahead| lead + 1 + ahead)
            .filter(|ix| rows[*ix].depth == row.depth)
            .and_then(select),
        Step::PreviousSibling => (0..lead)
            .rev()
            .find(|ix| rows[*ix].depth <= row.depth)
            .filter(|ix| rows[*ix].depth == row.depth)
            .and_then(select),
    }
}

/// Whether a row's text matches what speed search was given, and where: IntelliJ's
/// `SpeedSearchComparator(false)`, the text anywhere in the row, or else its words' starts
/// (`gre` in `greeting.rs`, `grr` too), case aside.
// shortcut: a substring, else fragments each starting a word; IntelliJ's `MinusculeMatcher`
// also weighs camel humps and typos, worth porting once a list needs its ranking
pub fn matching_fragments(query: &str, text: &str) -> Option<Vec<Range<usize>>> {
    let fold = |ch: char| ch.to_lowercase().next().unwrap_or(ch);
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let pattern: Vec<char> = query.chars().map(fold).collect();
    if pattern.is_empty() {
        return None;
    }
    let folded: Vec<char> = chars.iter().map(|(_, ch)| fold(*ch)).collect();
    let byte = |ix: usize| chars.get(ix).map_or(text.len(), |(at, _)| *at);
    if let Some(start) = (0..folded.len()).find(|start| folded[*start..].starts_with(&pattern)) {
        return Some(vec![byte(start)..byte(start + pattern.len())]);
    }
    let starts: Vec<bool> = (0..chars.len())
        .map(|ix| {
            ix == 0 || {
                let (previous, current) = (chars[ix - 1].1, chars[ix].1);
                !previous.is_alphanumeric()
                    || (current.is_uppercase() && previous.is_lowercase())
                    || (current.is_ascii_digit() != previous.is_ascii_digit())
            }
        })
        .collect();
    let fragments = word_fragments(&pattern, &folded, &starts, 0)?;
    Some(
        fragments
            .into_iter()
            .map(|range| byte(range.start)..byte(range.end))
            .collect(),
    )
}

/// `pattern` as fragments of `text` from `from` on, each at a word's start (by char index).
fn word_fragments(
    pattern: &[char],
    text: &[char],
    starts: &[bool],
    from: usize,
) -> Option<Vec<Range<usize>>> {
    if pattern.is_empty() {
        return Some(Vec::new());
    }
    for start in (from..text.len()).filter(|ix| starts[*ix] && text[*ix] == pattern[0]) {
        let run = text[start..]
            .iter()
            .zip(pattern)
            .take_while(|(a, b)| a == b)
            .count();
        for taken in (1..=run).rev() {
            if let Some(mut rest) = word_fragments(&pattern[taken..], text, starts, start + taken) {
                rest.insert(0, start..start + taken);
                return Some(rest);
            }
        }
    }
    None
}

/// `text` with `ranges` highlighted as speed search marks a match.
pub fn highlighted(
    text: impl Into<SharedString>,
    ranges: Option<Vec<Range<usize>>>,
    cx: &App,
) -> StyledText {
    let tokens = Tokens::get(cx);
    let style = HighlightStyle {
        color: Some(tokens.search_match_text),
        background_color: Some(tokens.search_match),
        ..Default::default()
    };
    StyledText::new(text.into()).with_highlights(
        ranges
            .unwrap_or_default()
            .into_iter()
            .map(|range| (range, style)),
    )
}

/// Gives `element` (the view's focused element) the keys of a tree or list.
pub fn tree_keys<V: TreeView, E: InteractiveElement>(element: E, cx: &mut Context<V>) -> E {
    macro_rules! steps {
        ($element:expr, $($action:ty => $step:expr, $extend:expr;)*) => {
            $element$(.on_action(cx.listener(|this, _: &$action, window, cx| {
                apply(this, $step, $extend, window, cx)
            })))*
        };
    }
    steps!(element,
        menu::SelectNext => Step::Next, false;
        menu::SelectPrevious => Step::Previous, false;
        menu::SelectFirst => Step::First, false;
        menu::SelectLast => Step::Last, false;
        menu::SelectChild => Step::Child, false;
        menu::SelectParent => Step::Parent, false;
        PageUp => Step::PageUp, false;
        PageDown => Step::PageDown, false;
        SelectParentNoCollapse => Step::ParentNoCollapse, false;
        SelectNextSibling => Step::NextSibling, false;
        SelectPreviousSibling => Step::PreviousSibling, false;
        SelectNextExtend => Step::Next, true;
        SelectPreviousExtend => Step::Previous, true;
        SelectFirstExtend => Step::First, true;
        SelectLastExtend => Step::Last, true;
        PageUpExtend => Step::PageUp, true;
        PageDownExtend => Step::PageDown, true;
        SelectParentExtend => Step::ParentNoCollapse, true;
        SelectChildExtend => Step::Child, true;
    )
    .on_action(cx.listener(|this, _: &SelectAll, window, cx| {
        let rows = this.tree_rows(cx);
        let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
            return;
        };
        let state = this.tree();
        state.search = None;
        state.anchor = Some(first.key.clone());
        state.lead = Some(last.key.clone());
        this.selection_changed(window, cx);
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &ExpandAll, window, cx| {
        this.set_all_expanded(true, window, cx);
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &CollapseAll, window, cx| {
        this.set_all_expanded(false, window, cx);
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &ScrollToCenter, _, cx| {
        let rows = this.tree_rows(cx);
        let state = this.tree();
        if let Some(ix) = state.lead.as_ref().and_then(|lead| position(&rows, lead)) {
            state.scroll_to_center(ix);
            cx.notify();
        }
    }))
    .on_action(cx.listener(|this, _: &SpeedSearch, _, cx| {
        if !this.speed_search() {
            cx.propagate();
            return;
        }
        let state = this.tree();
        if state.search.is_none() {
            state.search = Some(String::new());
        }
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| {
        this.tree().search = None;
        this.confirm(window, cx);
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &menu::Cancel, _, cx| {
        // ⎋ ends speed search, and only that; else what holds the tree handles it
        if this.tree().search.take().is_some() {
            cx.notify();
        } else {
            cx.propagate();
        }
    }))
    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
        if type_into_search(this, &event.keystroke, window, cx) {
            cx.stop_propagation();
        }
    }))
}

/// Speed search's typing: a character adds to what was given and selects the first row from
/// the selection on that matches; ⌫ takes the last one back.
fn type_into_search<V: TreeView>(
    this: &mut V,
    keystroke: &Keystroke,
    window: &mut Window,
    cx: &mut Context<V>,
) -> bool {
    let modifiers = keystroke.modifiers;
    if modifiers.platform || modifiers.control || modifiers.alt || modifiers.function {
        return false;
    }
    if !this.speed_search() {
        return false;
    }
    let state = this.tree();
    if keystroke.key == "backspace" {
        return match state.search.as_mut() {
            Some(query) => {
                query.pop();
                cx.notify();
                true
            }
            None => false,
        };
    }
    let Some(typed) = keystroke
        .key_char
        .clone()
        .filter(|typed| !typed.is_empty() && typed.chars().all(|ch| !ch.is_control()))
    else {
        return false;
    };
    state.search.get_or_insert_default().push_str(&typed);
    let rows = this.tree_rows(cx);
    let state = this.tree();
    let from = state
        .lead
        .as_ref()
        .and_then(|lead| position(&rows, lead))
        .unwrap_or(0);
    // the selection itself first, then on from it
    let found = (0..rows.len())
        .map(|ahead| (from + ahead) % rows.len())
        .find(|ix| state.matches(&rows[*ix].text).is_some());
    if let Some(ix) = found {
        state.select(Some(rows[ix].key.clone()));
        state.scroll_to(ix);
        this.selection_changed(window, cx);
    }
    cx.notify();
    true
}

fn apply<V: TreeView>(
    this: &mut V,
    step: Step,
    extend: bool,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    let rows = this.tree_rows(cx);
    let state = this.tree();
    // a list's ← and → are its columns' (`ListActions`): nothing in a list of one column
    if state.list && matches!(step, Step::Child | Step::Parent | Step::ParentNoCollapse) {
        return;
    }
    let lead = state.lead.as_ref().and_then(|lead| position(&rows, lead));
    if state.search.is_some() {
        match step {
            Step::Next | Step::Previous | Step::First | Step::Last if !extend => {
                if let Some(ix) = state.next_match(&rows, lead.unwrap_or(0), step) {
                    state.select(Some(rows[ix].key.clone()));
                    state.scroll_to(ix);
                    this.selection_changed(window, cx);
                }
                cx.notify();
                return;
            }
            _ => state.search = None,
        }
    }
    match navigate(&rows, lead, step, state.page(), !extend) {
        Some(Outcome::Select(ix)) => {
            let key = rows[ix].key.clone();
            match extend {
                true => state.extend_to(key),
                false => state.select(Some(key)),
            }
            state.scroll_to(ix);
            this.selection_changed(window, cx);
        }
        Some(Outcome::Expand(ix)) => this.set_expanded(&rows[ix].key, true, window, cx),
        Some(Outcome::Collapse(ix)) => this.set_expanded(&rows[ix].key, false, window, cx),
        None => {}
    }
    cx.notify();
}

#[cfg(test)]
mod tests {
    use super::{Outcome, Step, TreeRow, matching_fragments, navigate};

    /// a (open) > b, c (closed); d leaf; e (open) > f
    fn rows() -> Vec<TreeRow<char>> {
        vec![
            TreeRow::new('a', 0, Some(true), "a"),
            TreeRow::new('b', 1, None, "b"),
            TreeRow::new('c', 1, Some(false), "c"),
            TreeRow::new('d', 0, None, "d"),
            TreeRow::new('e', 0, Some(true), "e"),
            TreeRow::new('f', 1, None, "f"),
        ]
    }

    #[test]
    fn moves_as_intellij_trees() {
        let rows = rows();
        let go = |lead, step| navigate(&rows, Some(lead), step, 2, true);
        assert_eq!(go(0, Step::Child), Some(Outcome::Select(1)));
        assert_eq!(go(2, Step::Child), Some(Outcome::Expand(2)));
        assert_eq!(go(1, Step::Child), Some(Outcome::Select(2)));
        assert_eq!(go(0, Step::Parent), Some(Outcome::Collapse(0)));
        assert_eq!(go(2, Step::Parent), Some(Outcome::Select(0)));
        assert_eq!(go(4, Step::ParentNoCollapse), Some(Outcome::Select(0)));
        assert_eq!(go(5, Step::Next), Some(Outcome::Select(0)));
        assert_eq!(navigate(&rows, Some(5), Step::Next, 2, false), None);
        assert_eq!(go(0, Step::Previous), Some(Outcome::Select(5)));
        assert_eq!(go(1, Step::PageDown), Some(Outcome::Select(3)));
        assert_eq!(go(4, Step::PageDown), Some(Outcome::Select(5)));
        assert_eq!(go(1, Step::PageUp), Some(Outcome::Select(0)));
        assert_eq!(go(1, Step::NextSibling), Some(Outcome::Select(2)));
        assert_eq!(go(2, Step::NextSibling), None);
        assert_eq!(go(0, Step::NextSibling), Some(Outcome::Select(3)));
        assert_eq!(go(4, Step::PreviousSibling), Some(Outcome::Select(3)));
        assert_eq!(
            navigate(&rows, None, Step::Next, 2, true),
            Some(Outcome::Select(0))
        );
    }

    #[test]
    fn speed_search_matches_words() {
        assert_eq!(matching_fragments("eet", "greeting.rs"), Some(vec![2..5]));
        assert_eq!(matching_fragments("GRE", "greeting.rs"), Some(vec![0..3]));
        assert_eq!(
            matching_fragments("grr", "greeting.rs"),
            Some(vec![0..2, 9..10])
        );
        assert_eq!(matching_fragments("mr", "main.rs"), Some(vec![0..1, 5..6]));
        assert_eq!(matching_fragments("grt", "greeting.rs"), None);
        assert_eq!(matching_fragments("", "main.rs"), None);
    }
}
