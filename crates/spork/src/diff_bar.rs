//! The bar above every diff: previous / next change and unified or side-by-side (docs/specs/M08 §2).

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::diff::{
    DiffConflict, DiffConflictResolution, DiffFileStatus, DiffLinePosition, DiffLineRange,
    DiffMode, DiffSide, DiffState,
};
use gpui_kit::component::{ActiveTheme as _, Selectable as _, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::util::ResultExt as _;
use crate::{RepoView, View};

actions!(
    diff,
    [
        /// Compare Next File (⌃⇧→): the next file of the changes the diff walks through.
        CompareNextFile,
        /// Compare Previous File (⌃⇧←).
        ComparePreviousFile,
        /// Collapse All Files: every file of a diff of several folded to its header, or all
        /// unfolded when they are.
        CollapseAllFiles,
        /// Next Conflict (`Diff.NextConflict`): the merge's next unresolved conflict, selected.
        NextConflict,
        /// Previous Conflict (`Diff.PreviousConflict`).
        PreviousConflict,
        /// Accept Left Side (`Diff.ApplyLeftSide`, ⌃⌘→): the selected conflict takes the left
        /// (current) side.
        ApplyLeftSide,
        /// Accept Right Side (`Diff.ApplyRightSide`, ⌃⌘←): the right (incoming) side.
        ApplyRightSide,
        /// `Diff.OpenInEditor` (⌘↓, F4): the diff's file in the editor, at the selected line.
        OpenInEditor,
        /// Next Difference (`NextDiff`, F7): the diff scrolled to its next change.
        NextDifference,
        /// Previous Difference (`PreviousDiff`, ⇧F7).
        PreviousDifference,
    ]
);

pub(crate) fn bind_keys(cx: &mut App) {
    let bindings: Vec<KeyBinding> = [
        (
            "ctrl-shift-right",
            Box::new(CompareNextFile) as Box<dyn Action>,
        ),
        ("ctrl-shift-left", Box::new(ComparePreviousFile)),
        ("ctrl-secondary-right", Box::new(ApplyLeftSide)),
        ("ctrl-secondary-left", Box::new(ApplyRightSide)),
    ]
    .into_iter()
    .filter_map(|(keys, action)| KeyBinding::load(keys, action, None, false, None, cx).log_err())
    .collect();
    cx.bind_keys(bindings);
    // while a diff has the focus; the Commit tree and the log keep theirs for these keys
    let in_diff = || {
        KeyBindingContextPredicate::parse("Diff")
            .ok()
            .map(std::rc::Rc::new)
    };
    // an action's last binding is the one its menu item shows: the reference app's first shortcut goes last
    let editor: Vec<KeyBinding> = [
        ("f4", Box::new(OpenInEditor) as Box<dyn Action>),
        ("secondary-down", Box::new(OpenInEditor)),
        ("f7", Box::new(NextDifference)),
        ("shift-f7", Box::new(PreviousDifference)),
    ]
    .into_iter()
    .filter_map(|(keys, action)| {
        KeyBinding::load(keys, action, in_diff(), false, None, cx).log_err()
    })
    .collect();
    cx.bind_keys(editor);
}

/// The diff commands the reference app reaches from Find Action, for the command palette.
pub(crate) fn commands() -> Vec<(SharedString, Box<dyn Action>)> {
    vec![
        ("Compare Next File".into(), Box::new(CompareNextFile)),
        (
            "Compare Previous File".into(),
            Box::new(ComparePreviousFile),
        ),
        ("Collapse All Files".into(), Box::new(CollapseAllFiles)),
        ("Next Conflict".into(), Box::new(NextConflict)),
        ("Previous Conflict".into(), Box::new(PreviousConflict)),
        ("Accept Left Side".into(), Box::new(ApplyLeftSide)),
        ("Accept Right Side".into(), Box::new(ApplyRightSide)),
        ("Next Difference".into(), Box::new(NextDifference)),
        ("Previous Difference".into(), Box::new(PreviousDifference)),
    ]
}

/// The conflict after (or before) line `at` that `unresolved` keeps, by index; from the first
/// (or last) without a line.
fn step_conflict(
    conflicts: &[DiffConflict],
    at: Option<usize>,
    next: bool,
    unresolved: impl Fn(usize) -> bool,
) -> Option<usize> {
    let start = |ix: usize| conflicts[ix].lines().start;
    // inside a conflict, the step is from that conflict
    let at = at.map(|line| conflict_at(conflicts, line).map_or(line, start));
    let mut candidates = (0..conflicts.len()).filter(|&ix| unresolved(ix));
    match (at, next) {
        (None, true) => candidates.next(),
        (None, false) => candidates.next_back(),
        (Some(at), true) => candidates.find(|&ix| start(ix) > at),
        (Some(at), false) => candidates.rev().find(|&ix| start(ix) < at),
    }
}

/// The conflict holding line `at` of the working file.
fn conflict_at(conflicts: &[DiffConflict], at: usize) -> Option<usize> {
    conflicts.iter().position(|c| c.lines().contains(&at))
}

/// The file after (or before) `current` in `paths`; none past either end.
fn step<'a>(paths: &'a [String], current: &str, next: bool) -> Option<&'a String> {
    let ix = paths.iter().position(|p| p == current)?;
    match next {
        true => paths.get(ix + 1),
        false => ix.checked_sub(1).and_then(|ix| paths.get(ix)),
    }
}

impl RepoView {
    /// The diff commands, on the repository view so they reach whichever diff shows.
    pub(crate) fn diff_actions(&self, el: Div, cx: &mut Context<Self>) -> Div {
        let el = self.current_file_actions(el, cx);
        el.on_action(cx.listener(|this, _: &CompareNextFile, _, cx| this.compare_file(true, cx)))
            .on_action(
                cx.listener(|this, _: &ComparePreviousFile, _, cx| this.compare_file(false, cx)),
            )
            .on_action(cx.listener(|this, _: &CollapseAllFiles, _, cx| this.collapse_all_files(cx)))
            .on_action(cx.listener(|this, _: &NextConflict, _, cx| this.merge_step(true, cx)))
            .on_action(cx.listener(|this, _: &PreviousConflict, _, cx| this.merge_step(false, cx)))
            .on_action(cx.listener(|this, _: &ApplyLeftSide, _, cx| {
                this.merge_accept(DiffConflictResolution::Current, cx)
            }))
            .on_action(cx.listener(|this, _: &ApplyRightSide, _, cx| {
                this.merge_accept(DiffConflictResolution::Incoming, cx)
            }))
            .on_action(cx.listener(|this, _: &OpenInEditor, window, cx| {
                this.diff_open_in_editor(window, cx)
            }))
    }

    /// The conflicted file Local Changes shows in its merge view, and its conflicts.
    pub(crate) fn merge_file(&self, cx: &App) -> Option<(SharedString, Vec<DiffConflict>)> {
        if self.view != View::Changes {
            return None;
        }
        let state = self.change_diff.read(cx);
        let file = state
            .files()
            .iter()
            .find(|f| f.status() == DiffFileStatus::Conflicted)?;
        Some((file.path().clone(), file.conflicts().to_vec()))
    }

    /// The conflict the merge view's selection is in, the reference app's caret.
    pub(crate) fn merge_selected(&self, cx: &App) -> Option<usize> {
        let (_, conflicts) = self.merge_file(cx)?;
        let selected = self.change_diff.read(cx).selected_lines()?;
        conflict_at(&conflicts, selected.start())
    }

    /// Next / Previous Conflict: selects the next unresolved conflict after the selection and
    /// scrolls to it.
    fn merge_step(&mut self, next: bool, cx: &mut Context<Self>) {
        let Some((path, conflicts)) = self.merge_file(cx) else {
            return;
        };
        let state = self.change_diff.clone();
        let at = state.read(cx).selected_lines().map(|range| range.start());
        let unresolved = |ix| state.read(cx).conflict_resolution(&path, ix).is_none();
        let Some(ix) = step_conflict(&conflicts, at, next, unresolved) else {
            return;
        };
        let conflict = &conflicts[ix];
        // the conflict's own lines, between its markers; a part may be empty
        let first = conflict
            .current_lines()
            .start
            .min(conflict.incoming_lines().start);
        let last = conflict.incoming_lines().end.max(first + 1) - 1;
        state.update(cx, |s, cx| {
            s.set_selected_lines(
                Some(DiffLineRange::new(
                    path.clone(),
                    DiffSide::Modified,
                    first,
                    last,
                )),
                cx,
            );
            s.scroll_to_line(DiffLinePosition::new(path, DiffSide::Modified, first), cx);
        });
        cx.notify();
    }

    /// Accept Left / Right Side: the selected conflict resolved with that side.
    fn merge_accept(&mut self, side: DiffConflictResolution, cx: &mut Context<Self>) {
        let (Some((path, _)), Some(ix)) = (self.merge_file(cx), self.merge_selected(cx)) else {
            return;
        };
        self.change_diff
            .update(cx, |s, cx| s.resolve_conflict(&path, ix, Some(side), cx));
        cx.notify();
    }

    /// The diff of several files the current view shows: a commit's or a stash's.
    fn files_diff(&self) -> Option<Entity<DiffState>> {
        match self.view {
            View::CommitDiff | View::Stash => Some(self.diff.clone()),
            View::History if self.selected.is_some() => Some(self.diff.clone()),
            _ => None,
        }
    }

    /// Compare Next / Previous File: Local Changes moves to the next changed file of its list,
    /// a commit's diff to its next file.
    fn compare_file(&mut self, next: bool, cx: &mut Context<Self>) {
        if self.view == View::Changes {
            let Some(selected) = self.change_selected.clone() else {
                return;
            };
            let paths: Vec<String> = self
                .status_entries(selected.staged)
                .into_iter()
                .map(|e| e.path.clone())
                .collect();
            if let Some(path) = step(&paths, &selected.path, next).cloned() {
                let staged = selected.staged;
                self.select_change(Some(crate::changes::ChangeSelection { path, staged }), cx);
            }
            return;
        }
        let Some(state) = self.files_diff() else {
            return;
        };
        let paths: Vec<String> = state
            .read(cx)
            .files()
            .iter()
            .map(|f| f.path().to_string())
            .collect();
        let current = self
            .commit_file_selected
            .clone()
            .or_else(|| paths.first().cloned());
        if let Some(path) = current.and_then(|c| step(&paths, &c, next).cloned()) {
            state.update(cx, |s, cx| s.scroll_to_file(&path, cx));
            self.commit_file_selected = Some(path);
            cx.notify();
        }
    }

    fn collapse_all_files(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.files_diff() else {
            return;
        };
        state.update(cx, |s, cx| {
            let paths: Vec<SharedString> = s.files().iter().map(|f| f.path().clone()).collect();
            let fold = !paths.iter().all(|p| s.is_file_collapsed(p));
            for path in paths {
                s.set_file_collapsed(&path, fold, cx);
            }
        });
        cx.notify();
    }

    /// The files of the shown diff of several, and those folded to their header, for the
    /// end-to-end tests.
    pub(crate) fn shown_diff_files(&self, collapsed: bool, cx: &App) -> Vec<String> {
        self.files_diff()
            .map(|state| {
                let state = state.read(cx);
                state
                    .files()
                    .iter()
                    .filter(|f| !collapsed || state.is_file_collapsed(f.path()))
                    .map(|f| f.path().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn diff_bar(
        &self,
        id: &'static str,
        state: &Entity<DiffState>,
        cx: &mut Context<Self>,
    ) -> Div {
        let step = |suffix: &str, icon: IconName, tip: &'static str, next: bool| {
            let state = state.clone();
            Button::new(SharedString::from(format!("{id}-{suffix}")))
                .ghost()
                .xsmall()
                .icon(icon)
                .tooltip(tip)
                .on_click(move |_, _, cx| {
                    state.update(cx, |s, cx| {
                        if next {
                            s.next_change(cx)
                        } else {
                            s.previous_change(cx)
                        }
                    })
                })
        };
        let split = self.diff_mode == DiffMode::Split;
        let ignore_ws = self.prefs.ignore_whitespace;
        // The reference app's Diff.ViewerToolbar: Swap Sides for a comparison of two contents
        let swappable = self
            .diff_texts
            .get(&state.entity_id())
            .is_some_and(|info| info.sides.is_some());
        h_flex()
            .flex_none()
            .px_2()
            .py_0p5()
            .gap_1()
            .border_b_1()
            .border_color(cx.theme().border)
            // Local Changes keeps exact diffs: line staging builds patches from them
            .when(id != "changes", |bar| {
                bar.child(
                    Button::new(SharedString::from(format!("{id}-ws")))
                        .ghost()
                        .xsmall()
                        .label("Ignore whitespace")
                        .selected(ignore_ws)
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_ignore_whitespace(cx))),
                )
                .child({
                    let state = state.clone();
                    Button::new(SharedString::from(format!("{id}-whole")))
                        .ghost()
                        .xsmall()
                        .label("Entire file")
                        .selected(self.whole_file)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.whole_file = !this.whole_file;
                            let whole = this.whole_file;
                            state.update(cx, |s, cx| {
                                if whole {
                                    s.expand_unchanged(cx)
                                } else {
                                    s.collapse_unchanged(cx)
                                }
                            });
                            cx.notify();
                        }))
                })
            })
            .child(
                Button::new(SharedString::from(format!("{id}-wrap")))
                    .ghost()
                    .xsmall()
                    .label("Wrap")
                    .tooltip("Wrap long lines")
                    .selected(self.prefs.wrap_diffs)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.prefs.wrap_diffs = !this.prefs.wrap_diffs;
                        this.prefs.save();
                        cx.notify();
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("{id}-show-ws")))
                    .ghost()
                    .xsmall()
                    .label("Show spaces")
                    .tooltip("Show spaces as · and tabs as →")
                    .selected(self.prefs.show_whitespace)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.prefs.show_whitespace = !this.prefs.show_whitespace;
                        this.prefs.save();
                        this.redisplay_diffs(cx);
                    })),
            )
            .child(div().flex_1())
            .child(
                div()
                    .w(px(160.))
                    .child(spork_workbench::TextField::new(&self.diff_find).cleanable(true)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.find_count()),
            )
            .child(step(
                "prev",
                IconName::ChevronUp,
                "Previous Difference (⇧F7)",
                false,
            ))
            .child(step(
                "next",
                IconName::ChevronDown,
                "Next Difference (F7)",
                true,
            ))
            .child(
                Button::new(SharedString::from(format!("{id}-split")))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Columns2)
                    .tooltip("Side by side")
                    .selected(split)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mode = if split {
                            DiffMode::Unified
                        } else {
                            DiffMode::Split
                        };
                        this.set_diff_mode(mode, cx)
                    })),
            )
            .when(swappable, |bar| {
                let state = state.clone();
                bar.child(
                    h_flex()
                        .id(SharedString::from(format!("{id}-viewer-toolbar")))
                        .role(Role::Toolbar)
                        .aria_label("Diff.ViewerToolbar")
                        .child(
                            spork_workbench::ActionButton::new(
                                SharedString::from(format!("{id}-swap")),
                                "diff/arrowLeftRight",
                            )
                            .tooltip("Swap Sides")
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.swap_diff_sides(&state, cx)),
                            ),
                        ),
                )
            })
    }

    /// Flips "Ignore whitespace", remembers it, and reloads the read-only diffs on screen.
    fn toggle_ignore_whitespace(&mut self, cx: &mut Context<Self>) {
        self.prefs.ignore_whitespace = !self.prefs.ignore_whitespace;
        self.prefs.save();
        if let Some(ix) = self.selected {
            self.select(ix, cx);
        }
        if let Some(ix) = self.file.as_ref().and_then(|f| f.selected) {
            self.select_file_commit(ix, cx);
        }
        cx.notify();
    }

    /// Shows every open diff again from its text, after the whitespace or tab settings change.
    pub(crate) fn redisplay_diffs(&mut self, cx: &mut Context<Self>) {
        let file_diff = self.file.as_ref().map(|f| f.diff.clone());
        for state in [self.diff.clone(), self.change_diff.clone()]
            .into_iter()
            .chain(file_diff)
        {
            if let Some((text, sides)) = self
                .diff_texts
                .get(&state.entity_id())
                .map(|i| (i.text.clone(), i.sides.clone()))
            {
                self.set_patch(&state, &text, cx);
                // the same comparison, shown anew
                if let Some(info) = self.diff_texts.get_mut(&state.entity_id()) {
                    info.sides = sides;
                }
            }
        }
        cx.notify();
    }

    /// Switches every open diff and remembers the choice for next launch.
    fn set_diff_mode(&mut self, mode: DiffMode, cx: &mut Context<Self>) {
        self.diff_mode = mode;
        self.prefs.split_diffs = mode == DiffMode::Split;
        self.prefs.save();
        let file_diff = self.file.as_ref().map(|f| f.diff.clone());
        for state in [self.diff.clone(), self.change_diff.clone()]
            .into_iter()
            .chain(file_diff)
        {
            state.update(cx, |s, cx| s.set_mode(mode, cx));
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    // explicit: `super::*` would pull in gpui's `test` attribute, which shadows #[test]
    use super::{step, step_conflict};
    use gpui_kit::component::diff::DiffFile;

    #[test]
    fn steps_through_unresolved_conflicts() {
        // conflicts on working-file lines 2..7 and 9..14
        let text = "a\n<<<<<<< HEAD\nO1\n=======\nT1\n>>>>>>> m1\nb\n<<<<<<< HEAD\nO2\n=======\nT2\n>>>>>>> m1\nc\n";
        let file = DiffFile::parse_conflicts("m.txt", text).unwrap();
        let conflicts = file.conflicts();
        let all = |_| true;
        assert_eq!(step_conflict(conflicts, None, true, all), Some(0));
        assert_eq!(step_conflict(conflicts, None, false, all), Some(1));
        // from inside the first conflict to the second, and back
        assert_eq!(step_conflict(conflicts, Some(3), true, all), Some(1));
        assert_eq!(step_conflict(conflicts, Some(9), false, all), Some(0));
        assert_eq!(step_conflict(conflicts, Some(9), true, all), None);
        // resolved conflicts are passed over
        assert_eq!(step_conflict(conflicts, Some(9), false, |ix| ix != 0), None);
        assert_eq!(step_conflict(conflicts, None, true, |ix| ix != 0), Some(1));
    }

    #[test]
    fn steps_through_files_without_wrapping() {
        let paths = ["a".to_owned(), "b".to_owned(), "c".to_owned()];
        assert_eq!(step(&paths, "a", true).map(String::as_str), Some("b"));
        assert_eq!(step(&paths, "b", false).map(String::as_str), Some("a"));
        assert_eq!(step(&paths, "c", true), None);
        assert_eq!(step(&paths, "a", false), None);
        assert_eq!(step(&paths, "z", true), None);
    }
}
