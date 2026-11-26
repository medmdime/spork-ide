//! The coverage in the editor (`CoverageEditorAnnotatorImpl`, `CoverageLineMarkerRenderer`): each
//! line of the coverage shown marked in the gutter's left free painters area, in the scheme's
//! `LINE_FULL_COVERAGE`, `LINE_PARTIAL_COVERAGE` or `LINE_NONE_COVERAGE`; the marks follow the
//! edits, and a line edited loses its mark; a click on a mark shows IntelliJ's popup: its toolbar
//! (Previous and Next Coverage Mark, Show Tests Covering Line, Hide coverage) over the line's hits
//! and branches.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use editor::display_map::DisplayRow;
use editor::scroll::Autoscroll;
use editor::{Editor, EditorEvent, FreePainterMark, FreePainterMarkClick, SelectionEffects};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AnyWindowHandle, App, Context, Entity, EntityId, Global, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Point, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window, anchored,
    deferred, div, px,
};
use multi_buffer::MultiBufferSnapshot;
use serde_json::{Value, json};
use settings::Settings as _;
use spork_workbench::{ActionButton, Tokens};
use text::Point as TextPoint;
use theme::ActiveTheme as _;
use workspace::Workspace;

use crate::data::{Line, Status};
use crate::{Coverage, Data, HideCoverage};

/// A marked editor: its file's lines with their coverage and the text each had when marked.
pub(crate) struct Marked {
    editor: WeakEntity<Editor>,
    path: PathBuf,
    lines: Vec<MarkedLine>,
    _edits: Subscription,
}

#[derive(Clone)]
struct MarkedLine {
    data: Line,
    range: std::ops::Range<editor::Anchor>,
    text: String,
}

/// IntelliJ's colour of a line's status.
fn color(status: Status, cx: &App) -> gpui::Hsla {
    let tokens = Tokens::get(cx);
    match status {
        Status::Full => tokens.line_full_coverage,
        Status::Partial => tokens.line_partial_coverage,
        Status::None => tokens.line_none_coverage,
    }
}

/// IntelliJ's words for a line's status (`CoverageEngine.getLineCoverageStatus`).
fn status_text(status: Status) -> &'static str {
    match status {
        Status::Full => "Fully Covered",
        Status::Partial => "Partially Covered",
        Status::None => "Uncovered",
    }
}

/// The local file an editor shows, when it shows one.
fn editor_path(editor: &Editor, cx: &App) -> Option<PathBuf> {
    let buffer = editor.buffer().read(cx).as_singleton()?;
    let file = buffer.read(cx).file()?;
    let local = file.as_local()?;
    Some(local.abs_path(cx))
}

fn line_text(snapshot: &MultiBufferSnapshot, range: &std::ops::Range<editor::Anchor>) -> String {
    snapshot.text_for_range(range.clone()).collect()
}

impl Coverage {
    /// Marks every editor of the workspace with the coverage shown, or clears them.
    pub(crate) fn mark_editors(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let editors: Vec<Entity<Editor>> = workspace.read(cx).items_of_type::<Editor>(cx).collect();
        for editor in editors {
            self.mark_editor(&editor, cx);
        }
        self.marked
            .retain(|_, marked| marked.editor.upgrade().is_some());
        // the Project tool window's percentages follow
        if let Some(panel) = workspace.read(cx).panel::<project_panel::ProjectPanel>(cx) {
            panel.update(cx, |_, cx| cx.notify());
        }
    }

    /// Marks an editor with the coverage shown of its file.
    pub(crate) fn mark_editor(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let id = editor.entity_id();
        let data = self.data.clone();
        let newest = self.active.iter().map(|suite| suite.timestamp).max();
        let Some(path) = editor_path(editor.read(cx), cx) else {
            return;
        };
        let lines = match (&data, newest) {
            (Some(data), Some(newest)) if !changed_since(&path, newest) => data.lines_of(&path),
            _ => Default::default(),
        };
        if lines.is_empty() {
            if self.marked.remove(&id).is_some() {
                editor.update(cx, |editor, cx| {
                    editor.set_free_painter_marks(Vec::new(), cx)
                });
            }
            return;
        }
        let snapshot = editor.read(cx).buffer().read(cx).snapshot(cx);
        let max_row = snapshot.max_point().row;
        let marked: Vec<MarkedLine> = lines
            .into_values()
            .filter(|line| line.line >= 1 && line.line - 1 <= max_row)
            .map(|line| {
                let row = line.line - 1;
                let range = snapshot.anchor_before(TextPoint::new(row, 0))
                    ..snapshot.anchor_after(TextPoint::new(
                        row,
                        snapshot.line_len(multi_buffer::MultiBufferRow(row)),
                    ));
                MarkedLine {
                    text: line_text(&snapshot, &range),
                    data: line,
                    range,
                }
            })
            .collect();
        let edits = cx.subscribe(editor, |this: &mut Self, editor, event, cx| {
            if matches!(event, EditorEvent::BufferEdited) {
                this.edited(&editor, cx);
            }
        });
        self.marked.insert(
            id,
            Marked {
                editor: editor.downgrade(),
                path,
                lines: marked,
                _edits: edits,
            },
        );
        self.apply_marks(id, cx);
    }

    /// The editor's text changed: a line whose text did is unmarked (IntelliJ disposes of the
    /// highlighters of the lines changed), the others follow.
    fn edited(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let id = editor.entity_id();
        let snapshot = editor.read(cx).buffer().read(cx).snapshot(cx);
        let Some(marked) = self.marked.get_mut(&id) else {
            return;
        };
        let before = marked.lines.len();
        marked
            .lines
            .retain(|line| line_text(&snapshot, &line.range) == line.text);
        if marked.lines.len() != before {
            self.apply_marks(id, cx);
        }
    }

    fn apply_marks(&self, id: EntityId, cx: &mut Context<Self>) {
        let Some(marked) = self.marked.get(&id) else {
            return;
        };
        let Some(editor) = marked.editor.upgrade() else {
            return;
        };
        let marks = marked
            .lines
            .iter()
            .map(|line| FreePainterMark {
                range: line.range.clone(),
                color: color(line.data.status(), cx),
            })
            .collect();
        editor.update(cx, |editor, cx| editor.set_free_painter_marks(marks, cx));
    }
}

/// Whether the file changed on disk after the coverage was gathered: its lines no longer match
/// the data's (IntelliJ then shows "Coverage data outdated" instead of the marks).
// shortcut: a file changed since the run shows no marks; map its lines through Local History as
// IntelliJ does (`LineHistoryMapper`) once the history keeps the file's former text
fn changed_since(path: &Path, timestamp: i64) -> bool {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .is_some_and(|modified| modified.as_millis() as i64 > timestamp)
}

/// The popup showing, at most one in the app.
#[derive(Default)]
pub(crate) struct CoveragePopup(Option<Popup>);

impl Global for CoveragePopup {}

struct Popup {
    window: AnyWindowHandle,
    origin: Point<Pixels>,
    coverage: WeakEntity<Coverage>,
    editor: WeakEntity<Editor>,
    path: PathBuf,
    /// The line, from 1.
    line: u32,
    status: Status,
    report: String,
    previous: Option<(u32, Status)>,
    next: Option<(u32, Status)>,
    /// The tests that covered the line, when the suites tracked each test.
    tests: Option<Vec<String>>,
    /// Show Tests Covering Line was chosen.
    showing_tests: bool,
    _closes: Subscription,
}

pub(crate) fn init(cx: &mut App) {
    cx.set_global(CoveragePopup::default());
    cx.set_global(FreePainterMarkClick(Rc::new(
        |editor, ix, row, window, cx| show(editor, ix, Some(row), window, cx),
    )));
    cx.observe_keystrokes(|event, _, cx| {
        if event.keystroke.key == "escape" {
            close(cx);
        }
    })
    .detach();
}

pub(crate) fn close(cx: &mut App) {
    if cx
        .try_global::<CoveragePopup>()
        .is_some_and(|popup| popup.0.is_some())
    {
        cx.set_global(CoveragePopup(None));
    }
}

/// The coverage of the editor's window's workspace.
fn coverage_of(window: &Window, cx: &App) -> Option<Entity<Coverage>> {
    let workspace = Workspace::for_window(window, cx)?;
    Coverage::of(workspace.read(cx), cx)
}

/// Shows the popup of the editor's mark `ix`, under the row clicked or else under the caret's.
fn show(
    editor: &mut Editor,
    ix: usize,
    row: Option<DisplayRow>,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> bool {
    let Some(coverage) = coverage_of(window, cx) else {
        return false;
    };
    let id = cx.entity_id();
    let Some((lines, path)) = coverage
        .read(cx)
        .marked
        .get(&id)
        .map(|marked| (marked.lines.clone(), marked.path.clone()))
    else {
        return false;
    };
    let Some(line) = lines.get(ix).cloned() else {
        return false;
    };
    let snapshot = editor.buffer().read(cx).snapshot(cx);
    let buffer_row = editor::ToPoint::to_point(&line.range.start, &snapshot).row;
    let display = editor.display_snapshot(cx);
    let row = row.unwrap_or_else(|| {
        display
            .point_to_display_point(TextPoint::new(buffer_row, 0), text::Bias::Left)
            .row()
    });
    let (Some(below), Some(mark)) = (
        editor.text_origin_below_row(row),
        editor.free_painter_mark_point(buffer_row, cx),
    ) else {
        return false;
    };
    let status = line.data.status();
    // the next line, round the file, whose status differs (`BaseGotoCoveredLineAction`)
    let rows: Vec<(u32, Status)> = lines
        .iter()
        .map(|line| {
            (
                editor::ToPoint::to_point(&line.range.start, &snapshot).row,
                line.data.status(),
            )
        })
        .collect();
    let next = rows
        .iter()
        .skip(ix + 1)
        .chain(rows.iter().take(ix))
        .find(|(_, other)| *other != status)
        .copied();
    let previous = rows
        .iter()
        .take(ix)
        .rev()
        .chain(rows.iter().skip(ix + 1).rev())
        .find(|(_, other)| *other != status)
        .copied();
    let tracked = coverage
        .read(cx)
        .active
        .iter()
        .any(|suite| suite.track_per_test);
    let tests = coverage
        .read(cx)
        .data
        .as_ref()
        .filter(|_| tracked)
        .map(|data| data.tests_of_line(&path, line.data.line));
    let text = line_text(&snapshot, &line.range);
    let rest = snapshot
        .text_for_range(line.range.start..snapshot.anchor_after(snapshot.max_point()))
        .collect::<String>();
    let report = brief_report(&line.data, &text, &rest);
    let _closes = cx.subscribe_self(|_, event: &EditorEvent, cx| match event {
        EditorEvent::Edited { .. }
        | EditorEvent::BufferEdited
        | EditorEvent::ScrollPositionChanged {
            autoscroll: false, ..
        } => close(cx),
        _ => {}
    });
    let popup = Popup {
        window: window.window_handle(),
        origin: gpui::point(mark.x - IntellijMark::HALF, below.y),
        coverage: coverage.downgrade(),
        editor: cx.entity().downgrade(),
        path,
        line: line.data.line,
        status,
        report,
        previous,
        next,
        tests,
        showing_tests: false,
        _closes,
    };
    cx.set_global(CoveragePopup(Some(popup)));
    true
}

/// Half a mark's width: the popup lines up with the mark's left edge.
struct IntellijMark;

impl IntellijMark {
    const HALF: Pixels = px(4.);
}

/// The caret to `row`, then its popup once the editor has scrolled there (`moveToLine`).
fn move_to(editor: &Entity<Editor>, row: u32, window: &mut Window, cx: &mut App) {
    close(cx);
    editor.update(cx, |editor, cx| {
        let point = TextPoint::new(row, 0);
        editor.change_selections(
            SelectionEffects::scroll(Autoscroll::center()),
            window,
            cx,
            |selections| selections.select_ranges([point..point]),
        );
    });
    let editor = editor.downgrade();
    window.on_next_frame(move |window, cx| {
        editor
            .update(cx, |editor, cx| {
                let snapshot = editor.buffer().read(cx).snapshot(cx);
                let ix = coverage_of(window, cx).and_then(|coverage| {
                    let marked = coverage.read(cx).marked.get(&cx.entity_id())?;
                    marked.lines.iter().position(|line| {
                        editor::ToPoint::to_point(&line.range.start, &snapshot).row == row
                    })
                });
                if let Some(ix) = ix {
                    show(editor, ix, None, window, cx);
                }
            })
            .ok();
    });
}

/// The conditions of a line's `if`, `while` or `?:`, each operand of its `&&` and `||` one, as
/// IntelliJ lists them against the line's jumps.
fn conditions(text: &str) -> Vec<String> {
    let condition = ["if", "while"].iter().find_map(|keyword| {
        let at = find_keyword(text, keyword)?;
        let open = at + text[at..].find('(')?;
        let close = matching(text, open, '(', ')')?;
        Some(text[open + 1..close].to_owned())
    });
    let condition = condition.or_else(|| {
        let question = text.find(" ? ")?;
        let start = text[..question]
            .rfind(['=', '(', ','])
            .map_or(0, |at| at + 1);
        Some(text[start..question].trim().to_owned())
    });
    let Some(condition) = condition else {
        return Vec::new();
    };
    let mut leaves = Vec::new();
    operands(&condition, &mut leaves);
    leaves
}

/// The operands of a condition's `&&` and `||`, at every depth: each a jump of its own.
fn operands(condition: &str, leaves: &mut Vec<String>) {
    let mut part = condition.trim();
    while part.starts_with('(') && matching(part, 0, '(', ')') == Some(part.len() - 1) {
        part = part[1..part.len() - 1].trim();
    }
    let bytes = part.as_bytes();
    let mut parts = Vec::new();
    let (mut depth, mut start, mut ix) = (0i32, 0, 0);
    while ix < bytes.len() {
        match bytes[ix] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'&' | b'|' if depth == 0 && bytes.get(ix + 1) == Some(&bytes[ix]) => {
                parts.push(&part[start..ix]);
                ix += 1;
                start = ix + 1;
            }
            _ => {}
        }
        ix += 1;
    }
    if parts.is_empty() {
        let text = part.split_whitespace().collect::<Vec<_>>().join(" ");
        if !text.is_empty() {
            leaves.push(text);
        }
        return;
    }
    parts.push(&part[start..]);
    for part in parts {
        operands(part, leaves);
    }
}

/// Where a keyword starts as a word in `text`.
fn find_keyword(text: &str, keyword: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = text[from..].find(keyword) {
        let at = from + found;
        let before = text[..at].chars().next_back();
        let after = text[at + keyword.len()..].chars().next();
        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !word(before) && !word(after) {
            return Some(at);
        }
        from = at + keyword.len();
    }
    None
}

/// The byte where the bracket opened at `open` closes.
fn matching(text: &str, open: usize, opening: char, closing: char) -> Option<usize> {
    let mut depth = 0;
    for (ix, c) in text[open..].char_indices() {
        if c == opening {
            depth += 1;
        } else if c == closing {
            depth -= 1;
            if depth == 0 {
                return Some(open + ix);
            }
        }
    }
    None
}

/// A switch's selector on the line and whether its block has a default label, from the line on.
fn switch_of(text: &str, rest: &str) -> Option<(String, bool)> {
    let at = find_keyword(text, "switch").or_else(|| find_keyword(text, "when"))?;
    let open = at + text[at..].find('(')?;
    let close = matching(text, open, '(', ')')?;
    let mut selector = text[open + 1..close].trim();
    while selector.starts_with('(') && matching(selector, 0, '(', ')') == Some(selector.len() - 1) {
        selector = selector[1..selector.len() - 1].trim();
    }
    let body = rest.find('{').and_then(|brace| {
        let end = matching(rest, brace, '{', '}')?;
        Some(&rest[brace..=end])
    });
    let has_default = body.is_some_and(|body| {
        find_keyword(body, "default").is_some() || find_keyword(body, "else").is_some()
    });
    Some((selector.to_owned(), has_default))
}

/// The popup's text (`JavaCoverageEngine.createBriefReport`): the hits, then each condition's
/// true and false hits and each switch's cases, or the branches covered when the line's code
/// doesn't tell its conditions; `rest` is the file from the line on.
fn brief_report(line: &Line, text: &str, rest: &str) -> String {
    const INDENT: &str = "  ";
    let default_report = || {
        let (total, covered) = line.branches();
        match total {
            0 => format!("Hits: {}", line.hits),
            total => format!("Hits: {}\nCovered {covered}/{total} branches", line.hits),
        }
    };
    let mut report = format!("Hits: {}", line.hits);
    let mut outcomes = 0u64;
    if !line.jumps.is_empty() {
        let conditions = conditions(text);
        if conditions.len() < line.jumps.len() {
            return default_report();
        }
        for (condition, jump) in conditions.iter().zip(&line.jumps) {
            report.push_str(&format!(
                "\n{INDENT}{condition}\n{INDENT}{INDENT}true hits: {}\n{INDENT}{INDENT}false hits: {}",
                jump.true_hits, jump.false_hits
            ));
            outcomes += u64::from(jump.true_hits) + u64::from(jump.false_hits);
        }
    }
    if !line.switches.is_empty() {
        let Some((selector, has_default)) = switch_of(text, rest) else {
            return default_report();
        };
        if line.switches.len() > 1 {
            return default_report();
        }
        for switch in &line.switches {
            report.push_str(&format!("\n{INDENT}{selector}"));
            let mut all_hit = true;
            for (key, hits) in switch.keys.iter().zip(&switch.hits) {
                all_hit &= *hits > 0;
                report.push_str(&format!("\n{INDENT}{INDENT}case {key}: {hits}"));
            }
            let partial_by_default = all_hit && line.status() != Status::Full;
            if has_default || partial_by_default || switch.default_hits > 0 {
                report.push_str(&format!(
                    "\n{INDENT}{INDENT}default: {}",
                    switch.default_hits
                ));
            }
            outcomes += switch.hits.iter().map(|hits| u64::from(*hits)).sum::<u64>()
                + u64::from(switch.default_hits);
        }
    }
    if u64::from(line.hits) > outcomes && outcomes > 0 {
        report.push_str(&format!(
            "\nUnknown outcome: {}",
            u64::from(line.hits) - outcomes
        ));
    }
    report
}

/// A test as IntelliJ's coverage listener names it (`com.example.CalculatorTest,signs`), as
/// the popup lists it: `CalculatorTest.signs`.
fn test_title(test: &str) -> String {
    let (class, method) = test.split_once(',').unwrap_or((test, ""));
    let simple = class.rsplit('.').next().unwrap_or(class);
    match method.is_empty() {
        true => simple.to_owned(),
        false => format!("{simple}.{method}"),
    }
}

/// Opens a test's class at its method.
fn open_test(test: &str, data: &Data, window: &mut Window, cx: &mut App) {
    let (class, method) = test.split_once(',').unwrap_or((test, ""));
    let file = data
        .classes
        .iter()
        .find(|known| known.name == class)
        .and_then(|known| known.file.clone())
        .or_else(|| {
            let workspace = Workspace::for_window(window, cx)?;
            let project = workspace.read(cx).project().read(cx);
            let root = project.visible_worktrees(cx).next()?.read(cx).abs_path();
            find_class_file(&root, class)
        });
    let Some(file) = file else {
        return;
    };
    let line = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| {
            text.lines()
                .position(|text| find_keyword(text, method).is_some() && text.contains('('))
        })
        .unwrap_or(0) as u32;
    let Some(workspace) = Workspace::for_window(window, cx) else {
        return;
    };
    workspace.update(cx, |workspace, cx| {
        crate::view::open_at(workspace, file, line, window, cx)
    });
}

/// A test class's source in the project's test roots.
fn find_class_file(root: &Path, class: &str) -> Option<PathBuf> {
    let relative: PathBuf = class.split('.').collect();
    ["src/test/java", "src/test/kotlin"]
        .iter()
        .find_map(|folder| {
            ["java", "kt"].iter().find_map(|extension| {
                let file = root.join(folder).join(relative.with_extension(extension));
                file.is_file().then_some(file)
            })
        })
}

/// The popup for the window being drawn, drawn over everything.
pub(crate) fn render(window: &Window, cx: &App) -> Option<AnyElement> {
    let popup = cx.global::<CoveragePopup>().0.as_ref()?;
    if popup.window != window.window_handle() {
        return None;
    }
    let tokens = Tokens::get(cx);
    let theme = cx.theme();
    let settings = theme_settings::ThemeSettings::get_global(cx);
    let font_size = settings.buffer_font_size(cx);
    let editor = popup.editor.clone();
    let go = |target: Option<(u32, Status)>| {
        let editor = editor.clone();
        move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
            let (Some((row, _)), Some(editor)) = (target, editor.upgrade()) else {
                return;
            };
            move_to(&editor, row, window, cx);
        }
    };
    let tooltip = |direction: &str, target: Option<(u32, Status)>, keys: &str| match target {
        Some((_, status)) => format!("{direction} {} ({keys})", status_text(status)),
        None => format!("{direction} Coverage Mark ({keys})"),
    };
    let border = tokens.coverage_hint_border;
    let tests_button = popup.tests.as_ref().map(|_| {
        ActionButton::new("coverage-tests", "nodes/testSourceFolder")
            .tooltip("Show Tests Covering Line")
            .on_click(|_, _, cx| {
                if let Some(popup) = cx.global_mut::<CoveragePopup>().0.as_mut() {
                    popup.showing_tests = !popup.showing_tests;
                }
                cx.refresh_windows();
            })
    });
    let coverage = popup.coverage.clone();
    let toolbar = div()
        .id("coverage-hint-toolbar")
        .role(Role::Toolbar)
        .aria_label("Coverage")
        .flex()
        .items_center()
        .gap(px(2.))
        .px(px(4.))
        .py(px(3.))
        .bg(tokens.island)
        .child(
            ActionButton::new("coverage-previous", "general/up")
                .tooltip(tooltip("Previous", popup.previous, "⌥⇧↑"))
                .disabled(popup.previous.is_none())
                .on_click(go(popup.previous)),
        )
        .child(
            ActionButton::new("coverage-next", "general/down")
                .tooltip(tooltip("Next", popup.next, "⌥⇧↓"))
                .disabled(popup.next.is_none())
                .on_click(go(popup.next)),
        )
        .children(tests_button)
        .child(
            div()
                .id("coverage-hide")
                .role(Role::Link)
                .aria_label("Hide coverage")
                .ml(px(6.))
                .px(px(2.))
                .cursor_pointer()
                .text_color(tokens.text_link)
                .child("Hide coverage")
                .on_click(move |_, window, cx| {
                    close(cx);
                    if let Some(coverage) = coverage.upgrade() {
                        coverage.update(cx, |coverage, cx| coverage.hide(cx));
                    } else {
                        window.dispatch_action(Box::new(HideCoverage), cx);
                    }
                }),
        );
    let report = div()
        .id("coverage-report")
        .role(Role::Label)
        .aria_label(SharedString::from(popup.report.clone()))
        .px(px(8.))
        .py(px(4.))
        .border_t_1()
        .border_color(border)
        .bg(theme.colors().editor_background)
        .text_color(theme.colors().editor_foreground)
        .font(settings.buffer_font.clone())
        .text_size(font_size)
        .line_height(settings.line_height() * font_size)
        .whitespace_nowrap()
        .children(
            popup
                .report
                .lines()
                .map(|line| div().child(line.to_owned())),
        );
    let tests = popup
        .tests
        .as_ref()
        .filter(|_| popup.showing_tests)
        .map(|tests| {
            let class = coverage_class(popup, cx);
            let title = format!("Tests Covering Line {class}:{}", popup.line);
            let data = popup
                .coverage
                .upgrade()
                .and_then(|coverage| coverage.read(cx).data.clone());
            div()
                .id("coverage-tests")
                .role(Role::List)
                .aria_label(SharedString::from(title.clone()))
                .flex()
                .flex_col()
                .border_t_1()
                .border_color(border)
                .bg(tokens.island)
                .py(px(4.))
                .child(
                    div()
                        .px(px(8.))
                        .pb(px(2.))
                        .text_color(tokens.text_secondary)
                        .child(title),
                )
                .children(tests.iter().enumerate().map(|(ix, test)| {
                    let title = test_title(test);
                    let (test, data) = (test.clone(), data.clone());
                    div()
                        .id(("coverage-test", ix))
                        .role(Role::ListItem)
                        .aria_label(SharedString::from(title.clone()))
                        .px(px(8.))
                        .py(px(2.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .cursor_pointer()
                        .hover(|row| row.bg(tokens.hover))
                        .child(spork_workbench::image("nodes/test", cx).size(px(16.)))
                        .child(title)
                        .on_click(move |_, window, cx| {
                            close(cx);
                            if let Some(data) = &data {
                                open_test(&test, data, window, cx);
                            }
                        })
                }))
                .when(tests.is_empty(), |list| {
                    list.child(
                        div()
                            .px(px(8.))
                            .text_color(tokens.text_secondary)
                            .child("No tests found"),
                    )
                })
        });
    let panel = div()
        .id("coverage-popup")
        .role(Role::Dialog)
        .aria_label(SharedString::from(format!(
            "Coverage: {}",
            status_text(popup.status)
        )))
        .occlude()
        .flex()
        .flex_col()
        .border_1()
        .border_color(border)
        .on_mouse_down_out(|_, _, cx| close(cx))
        .child(toolbar)
        .child(report)
        .children(tests);
    Some(
        deferred(
            anchored()
                .position(popup.origin)
                .snap_to_window_with_margin(px(8.))
                .child(spork_vcs::spork_look(cx).child(panel)),
        )
        .with_priority(1)
        .into_any_element(),
    )
}

/// The class of the popup's line, for Show Tests Covering Line's title.
fn coverage_class(popup: &Popup, cx: &App) -> String {
    popup
        .coverage
        .upgrade()
        .and_then(|coverage| {
            let data = coverage.read(cx).data.clone()?;
            data.classes
                .iter()
                .filter(|class| class.file.as_deref() == Some(popup.path.as_path()))
                .find(|class| class.lines.iter().any(|line| line.line == popup.line))
                .map(|class| class.name.clone())
        })
        .unwrap_or_default()
}

/// The active editor's marks, for the end-to-end tests: each line's number and status.
pub(crate) fn e2e_marks(workspace: &Workspace, cx: &App) -> Value {
    let (Some(coverage), Some(editor)) = (
        Coverage::of(workspace, cx),
        workspace.active_item_as::<Editor>(cx),
    ) else {
        return json!([]);
    };
    let Some(marked) = coverage.read(cx).marked.get(&editor.entity_id()) else {
        return json!([]);
    };
    let snapshot = editor.read(cx).buffer().read(cx).snapshot(cx);
    let shown = editor.read(cx).free_painter_marks().len();
    marked
        .lines
        .iter()
        .take(shown)
        .map(|line| {
            let row = editor::ToPoint::to_point(&line.range.start, &snapshot).row + 1;
            let status = match line.data.status() {
                Status::Full => "full",
                Status::Partial => "partial",
                Status::None => "none",
            };
            json!(format!("{row} {status}"))
        })
        .collect()
}

/// The popup, for the end-to-end tests: its line, its report and its buttons' tooltips.
pub(crate) fn e2e_popup(cx: &App) -> Value {
    let Some(popup) = cx.try_global::<CoveragePopup>().and_then(|p| p.0.as_ref()) else {
        return Value::Null;
    };
    json!({
        "line": popup.line,
        "report": popup.report,
        "tests": popup.tests.as_ref().map(|tests| tests.iter().map(|t| test_title(t)).collect::<Vec<_>>()),
    })
}

#[cfg(test)]
mod tests {
    use super::{brief_report, conditions};
    use crate::data::{Jump, Line, Switch};

    #[test]
    fn tells_conditions_as_intellij_does() {
        assert_eq!(conditions("        if (number > 0) {"), ["number > 0"]);
        assert_eq!(conditions("    while ((a && b) || !c) {"), ["a", "b", "!c"]);
        let line = Line {
            line: 13,
            hits: 3,
            method: "sign(I)".into(),
            jumps: vec![Jump {
                true_hits: 2,
                false_hits: 1,
            }],
            switches: Vec::new(),
        };
        assert_eq!(
            brief_report(&line, "        if (number > 0) {", ""),
            "Hits: 3\n  number > 0\n    true hits: 2\n    false hits: 1"
        );
        let switch = Line {
            line: 16,
            hits: 1,
            method: "sign(I)".into(),
            jumps: Vec::new(),
            switches: vec![Switch {
                keys: vec![-1, 0],
                hits: vec![0, 1],
                default_hits: 0,
            }],
        };
        let rest = "switch (number) {\n case 0:\n return 1;\n default:\n return 2;\n}\n";
        assert_eq!(
            brief_report(&switch, "        switch (number) {", rest),
            "Hits: 1\n  number\n    case -1: 0\n    case 0: 1\n    default: 0"
        );
    }
}
