//! What every diff gets beyond the bare view (docs/specs/M08 §2): Find inside it, word wrap,
//! visible whitespace and tab width, a change map beside the scrollbar, and a double-click that
//! opens the file in the editor at that line. Spork keeps each diff's patch text for this, since
//! the diff component doesn't hand its lines back.

use crate::notices::Notification;
use gpui_kit::component::diff::{
    Diff, DiffDecoration, DiffFile, DiffLinePosition, DiffLineRange, DiffSide, DiffState,
};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::{RepoView, View};

/// One code line of a patch, with where it sits. Lines share their file's path and point into
/// the patch's text rather than copying it: a large diff has tens of thousands of them.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PatchLine {
    pub(crate) path: SharedString,
    pub(crate) side: DiffSide,
    pub(crate) line: usize,
    /// `Some(true)` added, `Some(false)` removed, `None` unchanged.
    pub(crate) change: Option<bool>,
    /// Its place among all the patch's lines, for the change map.
    pub(crate) at: usize,
    /// Where its code is in the patch's text (after the `+` / `-` / ` ` marker).
    pub(crate) text: std::ops::Range<usize>,
}

/// The code lines of a unified patch (one file or several), in order.
pub(crate) fn scan(patch: &str) -> Vec<PatchLine> {
    let (mut out, mut path) = (Vec::new(), SharedString::default());
    let (mut old, mut new, mut in_hunk) = (0, 0, false);
    let mut offset = 0;
    for (at, line) in patch.split_inclusive('\n').enumerate() {
        let start = offset;
        offset += line.len();
        let line = line.trim_end_matches(['\n', '\r']);
        if let Some(rest) = line.strip_prefix("diff --git ") {
            in_hunk = false;
            path = rest
                .rsplit_once(" b/")
                .map_or(rest, |(_, b)| b)
                .to_owned()
                .into();
        } else if let Some(hunk) = line.strip_prefix("@@ ") {
            // @@ -old,count +new,count @@
            let mut nums = hunk.split(' ').filter_map(|p| {
                p.trim_start_matches(['-', '+'])
                    .split(',')
                    .next()?
                    .parse::<usize>()
                    .ok()
            });
            (old, new) = (nums.next().unwrap_or(1), nums.next().unwrap_or(1));
            in_hunk = true;
        } else if in_hunk {
            let (marker, _) = line.split_at(line.len().min(1));
            let (side, number, change) = match marker {
                "+" => (DiffSide::Modified, new, Some(true)),
                "-" => (DiffSide::Original, old, Some(false)),
                " " => (DiffSide::Modified, new, None),
                _ => continue, // "\ No newline at end of file"
            };
            out.push(PatchLine {
                path: path.clone(),
                side,
                line: number,
                change,
                at,
                text: start + marker.len()..start + line.len(),
            });
            match marker {
                "+" => new += 1,
                "-" => old += 1,
                _ => (old, new) = (old + 1, new + 1),
            }
        }
    }
    out
}

/// The patch as shown: tabs as `tab_width` spaces (0: left alone), or with `show_ws`, spaces as
/// `·` and tabs as `→` padded to the tab width. Only code lines change, so line numbers and line
/// staging stay exact.
pub(crate) fn display_patch(patch: &str, tab_width: usize, show_ws: bool) -> String {
    if tab_width == 0 && !show_ws {
        return patch.to_owned();
    }
    let tab = tab_width.max(1);
    let mut in_hunk = false;
    let mut out = String::with_capacity(patch.len());
    for line in patch.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            in_hunk = false;
        } else if line.starts_with("@@ ") {
            in_hunk = true;
        } else if in_hunk && line.starts_with(['+', '-', ' ']) {
            let (marker, code) = line.split_at(1);
            out.push_str(marker);
            for c in code.chars() {
                match (c, show_ws) {
                    ('\t', true) => {
                        out.push('→');
                        out.push_str(&" ".repeat(tab - 1));
                    }
                    ('\t', false) => out.push_str(&" ".repeat(tab)),
                    (' ', true) => out.push('·'),
                    (c, _) => out.push(c),
                }
            }
            continue;
        }
        out.push_str(line);
    }
    out
}

/// What Spork keeps of a diff, worked out once when it loads rather than on every frame.
pub(crate) struct DiffInfo {
    pub(crate) text: String,
    lines: Vec<PatchLine>,
    /// The change map: `(place 0..1, added)`, one tick per pixel-ish slot, and the line to jump to.
    ticks: Vec<(f32, bool, usize)>,
    /// Indices into `lines` that hold the find text.
    hits: Vec<usize>,
    /// The changed line runs and how they changed, for their backgrounds.
    blocks: Vec<Block>,
    /// The file of the line under the pointer, which the editor menu acts on.
    hovered: Option<SharedString>,
    /// What a comparison of two contents shows left and right (Clipboard, Local), which Swap
    /// Sides exchanges; none for a diff out of the repository's history.
    pub(crate) sides: Option<(SharedString, SharedString)>,
}

/// The patch from the other side: what it added it removes, each hunk counted the other way.
/// File names stay, so the diff keeps showing the same file.
pub(crate) fn reverse_patch(patch: &str) -> String {
    let mut out = String::with_capacity(patch.len());
    // a run of changed lines, put back removed first as unified diffs have them
    let (mut removed, mut added) = (String::new(), String::new());
    let mut in_hunk = false;
    let mut last_added = false;
    for line in patch.split_inclusive('\n') {
        let flip = in_hunk && (line.starts_with('+') || line.starts_with('-'));
        let no_newline = in_hunk && line.starts_with('\\');
        if !flip && !no_newline {
            out.push_str(&removed);
            out.push_str(&added);
            removed.clear();
            added.clear();
        }
        if flip {
            let (marker, rest) = line.split_at(1);
            last_added = marker == "-";
            let run = if last_added { &mut added } else { &mut removed };
            run.push_str(if last_added { "+" } else { "-" });
            run.push_str(rest);
        } else if no_newline {
            // "\ No newline at end of file" stays with the line before it
            match removed.is_empty() && added.is_empty() {
                true => out.push_str(line),
                false if last_added => added.push_str(line),
                false => removed.push_str(line),
            }
        } else if line.starts_with("diff --git ") {
            in_hunk = false;
            out.push_str(line);
        } else if let Some(hunk) = line.strip_prefix("@@ ") {
            in_hunk = true;
            // @@ -old +new @@ rest
            let mut parts = hunk.splitn(3, ' ');
            match (parts.next(), parts.next(), parts.next()) {
                (Some(old), Some(new), Some(rest)) => out.push_str(&format!(
                    "@@ -{} +{} {rest}",
                    new.trim_start_matches('+'),
                    old.trim_start_matches('-')
                )),
                _ => out.push_str(line),
            }
        } else {
            out.push_str(line);
        }
    }
    out.push_str(&removed);
    out.push_str(&added);
    out
}

/// Changed lines `start..=end` on one side of a file, and how their block changed.
type Block = (
    SharedString,
    DiffSide,
    usize,
    usize,
    spork_workbench::DiffChange,
);

/// The patch's blocks of changed lines, as IntelliJ colors them: lines removed and lines added
/// together are a modification (blue on both sides), else an insertion or a deletion.
fn change_blocks(lines: &[PatchLine]) -> Vec<Block> {
    use spork_workbench::DiffChange::*;
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].change.is_none() {
            i += 1;
            continue;
        }
        // a block: changed lines next to each other in the patch, in one file
        let mut j = i + 1;
        while j < lines.len()
            && lines[j].change.is_some()
            && lines[j].path == lines[i].path
            && lines[j].at == lines[j - 1].at + 1
        {
            j += 1;
        }
        let block = &lines[i..j];
        let numbers = |added| {
            block
                .iter()
                .filter(move |l| l.change == Some(added))
                .map(|l| l.line)
        };
        let kind = match (numbers(false).next(), numbers(true).next()) {
            (None, _) => Inserted,
            (_, None) => Deleted,
            _ => Modified,
        };
        for (added, side) in [(false, DiffSide::Original), (true, DiffSide::Modified)] {
            let mut side_lines = numbers(added);
            if let Some(first) = side_lines.next() {
                let last = side_lines.next_back().unwrap_or(first);
                out.push((block[0].path.clone(), side, first, last, kind));
            }
        }
        i = j;
    }
    out
}

/// Slots in the change map: enough to tell changes apart, few enough to draw cheaply.
const MAP_SLOTS: usize = 400;

impl DiffInfo {
    fn new(text: String, query: &str) -> Self {
        let lines = scan(&text);
        let total = text.lines().count().max(1);
        let mut ticks: Vec<(f32, bool, usize)> = Vec::new();
        let mut taken = std::collections::HashSet::new();
        for (ix, l) in lines.iter().enumerate() {
            let Some(added) = l.change else { continue };
            let slot = l.at * MAP_SLOTS / total;
            if taken.insert((slot, added)) {
                ticks.push((slot as f32 / MAP_SLOTS as f32, added, ix));
            }
        }
        let blocks = change_blocks(&lines);
        let mut info = Self {
            text,
            lines,
            ticks,
            hits: Vec::new(),
            blocks,
            hovered: None,
            sides: None,
        };
        info.find(query);
        info
    }

    fn find(&mut self, query: &str) {
        let query = query.to_lowercase();
        self.hits = if query.is_empty() {
            Vec::new()
        } else {
            (0..self.lines.len())
                .filter(|&i| {
                    self.text[self.lines[i].text.clone()]
                        .to_lowercase()
                        .contains(&query)
                })
                .collect()
        };
    }
}

impl RepoView {
    /// Shows `patch` in `state` as the preferences ask, and keeps its text for Find and the map.
    pub(crate) fn set_patch(
        &mut self,
        state: &Entity<DiffState>,
        patch: &str,
        cx: &mut Context<Self>,
    ) {
        let shown = display_patch(
            patch,
            self.prefs.tab_width as usize,
            self.prefs.show_whitespace,
        );
        self.diff_texts.insert(
            state.entity_id(),
            DiffInfo::new(patch.to_owned(), &self.diff_query),
        );
        let files = DiffFile::parse(&shown).unwrap_or_default();
        state.update(cx, |s, cx| s.set_files(files, cx));
    }

    /// `state`'s changed lines as `+text` / `-text`, in order, for the end-to-end tests.
    pub(crate) fn changed_lines(&self, state: &Entity<DiffState>) -> Vec<String> {
        let Some(info) = self.diff_texts.get(&state.entity_id()) else {
            return Vec::new();
        };
        info.lines
            .iter()
            .filter_map(|line| {
                let sign = if line.change? { '+' } else { '-' };
                Some(format!("{sign}{}", &info.text[line.text.clone()]))
            })
            .collect()
    }

    /// The diff the current view shows, which Find works on.
    pub(crate) fn active_diff(&self) -> Option<Entity<DiffState>> {
        match self.view {
            View::Changes => Some(self.change_diff.clone()),
            View::History | View::Stash | View::CommitDiff => Some(self.diff.clone()),
            View::File => self.file.as_ref().map(|f| f.diff.clone()),
        }
    }

    /// The find text changed: every kept diff looks for it again.
    pub(crate) fn find_again(&mut self) {
        let query = self.diff_query.clone();
        for info in self.diff_texts.values_mut() {
            info.find(&query);
        }
    }

    /// Lines of `state`'s diff containing the find text (any case).
    fn find_matches(&self, state: &Entity<DiffState>) -> Vec<&PatchLine> {
        self.diff_texts
            .get(&state.entity_id())
            .map(|info| info.hits.iter().map(|&i| &info.lines[i]).collect())
            .unwrap_or_default()
    }

    /// Enter / the arrows in the find box: the next or previous match, wrapping around.
    pub(crate) fn find_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(state) = self.active_diff() else {
            return;
        };
        let matches = self.find_matches(&state);
        if matches.is_empty() {
            return cx.notify();
        }
        let n = matches.len() as isize;
        let ix = (self.diff_find_ix as isize + delta).rem_euclid(n) as usize;
        let hit = matches[ix];
        let at = DiffLinePosition::new(hit.path.clone(), hit.side, hit.line);
        self.diff_find_ix = ix;
        state.update(cx, |s, cx| s.scroll_to_line(at, cx));
        cx.notify();
    }

    /// "3 of 12" for the find box; empty when not searching.
    pub(crate) fn find_count(&self) -> String {
        let Some(state) = self.active_diff() else {
            return String::new();
        };
        match self.find_matches(&state).len() {
            _ if self.diff_query.is_empty() => String::new(),
            0 => "No matches".into(),
            n => format!("{} of {n}", self.diff_find_ix.min(n - 1) + 1),
        }
    }

    /// The diff of `state` with wrap, find highlights, the change map and double-click to edit.
    pub(crate) fn diff_view(&self, state: &Entity<DiffState>, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (found, added, removed) = (theme.warning.opacity(0.35), theme.green, theme.danger);
        let info = self.diff_texts.get(&state.entity_id());
        // IntelliJ's colors for the changed lines; the find hits after them, so they win
        let blocks = info.map(|i| &i.blocks[..]).unwrap_or_default();
        let marks = blocks
            .iter()
            .map(|(path, side, start, end, change)| {
                DiffDecoration::new(DiffLineRange::new(path.clone(), *side, *start, *end))
                    .with_background(spork_workbench::diff_color(*change, cx))
            })
            .chain(self.find_matches(state).into_iter().map(|m| {
                DiffDecoration::new(DiffLineRange::new(m.path.clone(), m.side, m.line, m.line))
                    .with_background(found)
            }))
            .collect::<Vec<_>>();
        let view = cx.entity();
        let id = state.entity_id();
        let diff = Diff::new(state)
            .soft_wrap(self.prefs.wrap_diffs)
            .change_background(false)
            .decorations(marks)
            .on_line_click({
                let view = view.clone();
                move |at, event, window, cx| {
                    if event.click_count() == 2 {
                        let at = at.clone();
                        view.update(cx, |this, cx| this.edit_at_line(&at, window, cx));
                    }
                }
            })
            .on_line_hover({
                let view = view.clone();
                move |at, _, cx| {
                    let path = at.map(|at| at.path().clone());
                    view.update(cx, |this, _| {
                        if let (Some(info), Some(path)) = (this.diff_texts.get_mut(&id), path) {
                            info.hovered = Some(path);
                        }
                    })
                }
            })
            .size_full();
        let diff = div()
            .id(("diff-editor", id.as_u64()))
            .size_full()
            .child(diff)
            .context_menu({
                let state = state.clone();
                move |menu, _, cx| editor_menu(menu, &view, &state, cx)
            });
        // the change map: changed lines placed by their share of the patch
        let (ticks, lines) = info
            .map(|info| (&info.ticks[..], &info.lines[..]))
            .unwrap_or_default();
        let has_map = !ticks.is_empty();
        let ticks = ticks.iter().filter_map(|&(place, is_added, ix)| {
            let color = if is_added { added } else { removed };
            let line = lines.get(ix)?;
            let at = DiffLinePosition::new(line.path.clone(), line.side, line.line);
            let state = state.clone();
            Some(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(relative(place))
                    .h(px(2.))
                    .bg(color)
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let at = at.clone();
                        state.update(cx, |s, cx| s.scroll_to_line(at, cx))
                    }),
            )
        });
        let (next, previous) = (state.clone(), state.clone());
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .on_action(move |_: &crate::diff_bar::NextDifference, _, cx| {
                next.update(cx, |s, cx| s.next_change(cx))
            })
            .on_action(move |_: &crate::diff_bar::PreviousDifference, _, cx| {
                previous.update(cx, |s, cx| s.previous_change(cx))
            })
            .child(diff)
            .when(has_map, |d| {
                d.child(
                    div()
                        .id("change-map")
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .right(px(14.))
                        .w(px(5.))
                        .children(ticks),
                )
            })
    }

    /// Compare with Clipboard: the clipboard's text against `path`'s working copy, shown in
    /// `state` in place of what it showed.
    // shortcut: replaces the diff instead of opening its own window; give it a tab once diffs
    // can open several.
    fn compare_with_clipboard(
        &mut self,
        state: Entity<DiffState>,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) else {
            return window.push_notification(Notification::info("The clipboard holds no text"), cx);
        };
        if let Some(file) = self.file.as_mut().filter(|f| f.diff == state) {
            file.set_comparison(Some("Clipboard vs Local".into()));
        }
        self.run_op(
            window,
            cx,
            move |root| spork_git::file::diff_text_with_file(root, &text, &path),
            move |this, patch, _, cx| {
                this.set_patch(&state, &patch, cx);
                if let Some(info) = this.diff_texts.get_mut(&state.entity_id()) {
                    info.sides = Some(("Clipboard".into(), "Local".into()));
                }
            },
        );
    }

    /// Swap Sides (`SwapSidesInDiffWindow`): a comparison of two contents shown the other way
    /// round, the right side's text on the left.
    pub(crate) fn swap_diff_sides(&mut self, state: &Entity<DiffState>, cx: &mut Context<Self>) {
        let Some((text, (left, right))) = self
            .diff_texts
            .get(&state.entity_id())
            .and_then(|info| Some((info.text.clone(), info.sides.clone()?)))
        else {
            return;
        };
        self.set_patch(state, &reverse_patch(&text), cx);
        if let Some(info) = self.diff_texts.get_mut(&state.entity_id()) {
            info.sides = Some((right.clone(), left.clone()));
        }
        if let Some(file) = self.file.as_mut().filter(|f| &f.diff == state) {
            file.set_comparison(Some(format!("{right} vs {left}")));
        }
        cx.notify();
    }

    /// `Diff.OpenInEditor` (⌘↓, F4): the shown diff's file in the editor at its selected line,
    /// the reference app's caret, else its first file.
    pub(crate) fn diff_open_in_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.active_diff() else {
            return;
        };
        let state = state.read(cx);
        let at = state
            .selected_lines()
            .map(|range| DiffLinePosition::new(range.path().clone(), range.side(), range.start()))
            .or_else(|| {
                let file = state.files().first()?;
                Some(DiffLinePosition::new(
                    file.path().clone(),
                    DiffSide::Modified,
                    1,
                ))
            });
        if let Some(at) = at {
            self.edit_at_line(&at, window, cx);
        }
    }

    /// Double-click on a line: the working copy of that file in the editor, at that line where
    /// the editor takes one (VS Code, Cursor, Zed), else just the file.
    fn edit_at_line(&mut self, at: &DiffLinePosition, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let file = repo.root.join(at.path().as_ref());
        if !file.exists() {
            return window.push_notification(
                Notification::info("That file isn't in the working copy any more"),
                cx,
            );
        }
        if self.ide {
            let line = (at.side() == DiffSide::Modified).then(|| at.line());
            return cx.emit(crate::ide::IdeEvent::OpenFile { path: file, line });
        }
        let editor = self.prefs.editor.to_lowercase();
        let scheme = ["cursor", "zed", "code"]
            .into_iter()
            .find(|e| editor.contains(e))
            .map(|e| if e == "code" { "vscode" } else { e });
        match scheme {
            Some(scheme) if at.side() == DiffSide::Modified => {
                cx.open_url(&format!("{scheme}://file{}:{}", file.display(), at.line()))
            }
            _ => self.open_in_editor(Some(file), window, cx),
        }
    }
}

/// The reference app's `Diff.EditorPopupMenu` on a diff of the repository's files: Compare with Clipboard
/// and Annotate with Git Blame, for the file under the pointer (the diff's first one otherwise).
/// Its blank-diff and code-review groups belong to windows Spork doesn't have.
fn editor_menu(
    menu: PopupMenu,
    view: &Entity<RepoView>,
    state: &Entity<DiffState>,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let hovered = view
        .read(cx)
        .diff_texts
        .get(&state.entity_id())
        .and_then(|info| info.hovered.clone());
    let path = hovered.or_else(|| state.read(cx).files().first().map(|f| f.path().clone()));
    let Some(path) = path.map(|p| p.to_string()) else {
        return menu;
    };
    let compare = {
        let (view, state, path) = (view.clone(), state.clone(), path.clone());
        PopupMenuItem::new("Compare with Clipboard")
            .icon(spork_workbench::icon("actions/diffWithClipboard"))
            .on_click(move |_, window, cx| {
                let (state, path) = (state.clone(), path.clone());
                view.update(cx, |this, cx| {
                    this.compare_with_clipboard(state, path, window, cx)
                })
            })
    };
    let annotate = {
        let view = view.clone();
        PopupMenuItem::new("Annotate with Git Blame").on_click(move |_, _, cx| {
            let path = path.clone();
            view.update(cx, |this, cx| this.open_file(path, true, cx))
        })
    };
    menu.item(compare).item(annotate)
}

#[cfg(test)]
mod tests {
    // explicit: `super::*` would pull in gpui's `test` attribute, which shadows #[test]
    use super::{display_patch, reverse_patch, scan};

    #[test]
    fn swaps_a_patch_sides() {
        let patch = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,3 +1,4 @@ fn a\n x\n-old\n+new\n+more\n y\n@@ -9 +10 @@\n-last\n\\ No newline at end of file\n+last\n";
        let swapped = reverse_patch(patch);
        assert_eq!(
            swapped,
            "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,4 +1,3 @@ fn a\n x\n-new\n-more\n+old\n y\n@@ -10 +9 @@\n-last\n+last\n\\ No newline at end of file\n"
        );
        assert_eq!(reverse_patch(&swapped), patch);
        // a comparison with the clipboard's copy keeps showing the repository's file
        let clipboard = "diff --git a/tmp/clip/README.md b/README.md\nindex 7898192..422c2b7 100644\n--- a/tmp/clip/README.md\n+++ b/README.md\n@@ -1 +1,3 @@\n # Fixture\n+two\n+changed\n";
        let files = gpui_kit::component::diff::DiffFile::parse(&reverse_patch(clipboard)).unwrap();
        assert_eq!(files[0].path().as_ref(), "README.md");
    }
    use gpui_kit::component::diff::DiffSide;

    const PATCH: &str = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n fn a() {\n-\tlet x = 1;\n+\tlet x = 2;\n }\n";

    #[test]
    fn blocks_are_inserted_deleted_or_modified() {
        use spork_workbench::DiffChange::*;
        let patch = "diff --git a/f b/f\n@@ -1,5 +1,5 @@\n a\n-b\n+B\n+c\n d\n-e\n f\n+g\n";
        let blocks: Vec<_> = super::change_blocks(&scan(patch))
            .into_iter()
            .map(|(_, side, start, end, change)| (side, start, end, change))
            .collect();
        assert_eq!(
            blocks,
            [
                (DiffSide::Original, 2, 2, Modified),
                (DiffSide::Modified, 2, 3, Modified),
                (DiffSide::Original, 4, 4, Deleted),
                (DiffSide::Modified, 6, 6, Inserted),
            ]
        );
    }

    #[test]
    fn scans_lines_with_their_numbers() {
        let lines = scan(PATCH);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].path, "src/a.rs");
        assert_eq!(
            (lines[1].side, lines[1].line, lines[1].change),
            (DiffSide::Original, 2, Some(false))
        );
        assert_eq!(
            (lines[2].side, lines[2].line, lines[2].change),
            (DiffSide::Modified, 2, Some(true))
        );
        assert_eq!((lines[3].line, lines[3].change), (3, None));
        assert_eq!(lines[2].at, 6);
        assert_eq!(&PATCH[lines[2].text.clone()], "\tlet x = 2;");
    }

    #[test]
    fn shows_tabs_and_spaces() {
        assert_eq!(display_patch(PATCH, 0, false), PATCH);
        let wide = display_patch(PATCH, 2, false);
        assert!(wide.contains("+  let x = 2;") && wide.contains("+++ b/src/a.rs"));
        let ws = display_patch(PATCH, 4, true);
        assert!(ws.contains("+→   let·x·=·2;"));
        assert!(ws.contains(" fn·a()·{")); // the context marker stays a space
        assert_eq!(ws.lines().count(), PATCH.lines().count());
    }
}
