//! IntelliJ's inspection widget (`EditorMarkupModelImpl`'s traffic light) in a file editor's top
//! right corner: the file's problems from the language servers' diagnostics, counted as IntelliJ
//! counts them (errors, warnings, and weak warnings for information and hints), each an icon and
//! its count, the green check when there are none; Previous and Next Highlighted Error beside them.
//! Hovering it tells the counts ("1 error, 2 warnings"), a click on them goes to the next problem.
//! F2 and ⇧F2 go to the next and previous problem of the highest severity the file has, as
//! IntelliJ's Next Highlighted Error does by default (`NEXT_ERROR_ACTION_GOES_TO_ERRORS_FIRST`).

use std::rc::Rc;

use editor::actions::Hover;
use editor::scroll::Autoscroll;
use editor::{Editor, EditorCornerWidget, MultiBufferOffset, SelectionEffects, hover_popover};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action as _, AnyElement, App, AppContext as _, ClickEvent, Context, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, div, img, px, svg,
};
use language::DiagnosticSeverity;
use project::project_settings::{GoToDiagnosticSeverity, GoToDiagnosticSeverityFilter};
use settings::Settings as _;
use spork_workbench::tokens::metrics;
use spork_workbench::{Tokens, Typography as _};
use theme::ActiveTheme as _;
use theme_settings::ThemeSettings;

gpui::actions!(
    spork_editor,
    [
        /// Goes to the next problem of the highest severity the file has (IntelliJ's Next
        /// Highlighted Error, F2).
        GoToNextError,
        /// Goes to the previous problem of the highest severity the file has (IntelliJ's Previous
        /// Highlighted Error, ⇧F2).
        GoToPreviousError,
    ]
);

/// The toolbar's empty border and its buttons' insets (`EditorInspectionsActionToolbar`).
const INSET: f32 = 2.;
/// A button's content: the status icon's height (`statusIconSize`), an arrow button's width.
const BUTTON: f32 = 18.;
/// The counts' padding (`LEFT_RIGHT_INDENT`) and the space between two (`INTER_GROUP_OFFSET`).
const PADDING: f32 = 5.;
const BETWEEN: f32 = 6.;
/// How long after Next Highlighted Error moves the caret the problem's popup shows: the editor
/// has scrolled to it by then.
const SCROLL_SETTLED: std::time::Duration = std::time::Duration::from_millis(100);
/// The hover popup's padding and least width (`TrafficLightPopup`).
const POPUP_PADDING: f32 = 10.;
const POPUP_MIN_WIDTH: f32 = 296.;

/// A file's problems by IntelliJ's severities.
#[derive(Default, Clone, Copy, Debug, PartialEq)]
struct Problems {
    errors: usize,
    warnings: usize,
    weak_warnings: usize,
}

impl Problems {
    fn of(editor: &Editor, cx: &App) -> Self {
        let snapshot = editor.buffer().read(cx).snapshot(cx);
        let mut problems = Self::default();
        // shortcut: counted at each frame, cheap for a file's few diagnostics; keep the counts
        // per buffer version if a file with thousands of them shows it
        for entry in snapshot.diagnostics_in_range(MultiBufferOffset(0)..snapshot.len()) {
            if !entry.diagnostic.is_primary {
                continue;
            }
            match entry.diagnostic.severity {
                DiagnosticSeverity::ERROR => problems.errors += 1,
                DiagnosticSeverity::WARNING => problems.warnings += 1,
                _ => problems.weak_warnings += 1,
            }
        }
        problems
    }

    fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The severity F2 and ⇧F2 go through: the highest the file has.
    fn highest(&self) -> GoToDiagnosticSeverityFilter {
        if self.errors > 0 {
            GoToDiagnosticSeverityFilter::Only(GoToDiagnosticSeverity::Error)
        } else if self.warnings > 0 {
            GoToDiagnosticSeverityFilter::Only(GoToDiagnosticSeverity::Warning)
        } else {
            GoToDiagnosticSeverityFilter::Range {
                min: GoToDiagnosticSeverity::Hint,
                max: GoToDiagnosticSeverity::Information,
            }
        }
    }

    /// IntelliJ's counts line (`*.severity.count.message`, joined with ", "), or its "No problems
    /// found".
    fn summary(&self) -> String {
        if self.is_empty() {
            return "No problems found".to_owned();
        }
        [
            (self.errors, "error", "errors"),
            (self.warnings, "warning", "warnings"),
            (self.weak_warnings, "weak warning", "weak warnings"),
        ]
        .into_iter()
        .filter(|(count, _, _)| *count > 0)
        .map(|(count, one, many)| match count {
            1 => format!("1 {one}"),
            _ => format!("{count} {many}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
    }
}

pub(crate) fn init(cx: &mut App) {
    cx.set_global(EditorCornerWidget(Rc::new(widget)));
}

/// A floating toolbar another crate shows in a file editor's top right corner, left of the
/// inspections (IntelliJ's `FloatingToolbarProvider`s: HotSwap's "Code changed:").
pub type FloatingToolbar =
    Rc<dyn Fn(&mut Editor, &mut Window, &mut Context<Editor>) -> Option<AnyElement>>;

#[derive(Default)]
struct FloatingToolbars(Vec<FloatingToolbar>);

impl gpui::Global for FloatingToolbars {}

/// Shows `toolbar` in every file editor's top right corner while it draws something.
pub fn add_floating_toolbar(toolbar: FloatingToolbar, cx: &mut App) {
    cx.default_global::<FloatingToolbars>().0.push(toolbar);
}

/// Registers F2 and ⇧F2 on a file editor.
pub(crate) fn register(editor: &mut Editor, handle: WeakEntity<Editor>) {
    editor
        .register_action({
            let handle = handle.clone();
            move |_: &GoToNextError, window, cx| {
                handle
                    .update(cx, |editor, cx| go_to_problem(editor, true, window, cx))
                    .ok();
            }
        })
        .detach();
    editor
        .register_action(move |_: &GoToPreviousError, window, cx| {
            handle
                .update(cx, |editor, cx| go_to_problem(editor, false, window, cx))
                .ok();
        })
        .detach();
}

/// IntelliJ's GotoNextError and GotoPreviousError: the caret to the next (or previous) problem of
/// the highest severity the file has, from the first (or last) past the end, then its tooltip.
fn go_to_problem(editor: &mut Editor, next: bool, window: &mut Window, cx: &mut Context<Editor>) {
    let severity = Problems::of(editor, cx).highest();
    let snapshot = editor.buffer().read(cx).snapshot(cx);
    let mut starts: Vec<MultiBufferOffset> = snapshot
        .diagnostics_in_range(MultiBufferOffset(0)..snapshot.len())
        .filter(|entry| entry.diagnostic.is_primary && severity.matches(entry.diagnostic.severity))
        .map(|entry| entry.range.start)
        .collect();
    starts.sort();
    starts.dedup();
    let caret = editor
        .selections
        .newest::<MultiBufferOffset>(&editor.display_snapshot(cx))
        .head();
    let target = match next {
        true => starts
            .iter()
            .find(|start| **start > caret)
            .or(starts.first()),
        false => starts
            .iter()
            .rev()
            .find(|start| **start < caret)
            .or(starts.last()),
    };
    let Some(&target) = target else {
        return;
    };
    editor.change_selections(
        SelectionEffects::scroll(Autoscroll::center()),
        window,
        cx,
        |selections| selections.select_ranges([target..target]),
    );
    // once the editor has scrolled there, as scrolling hides a hover
    cx.spawn_in(window, async move |editor, cx| {
        cx.background_executor().timer(SCROLL_SETTLED).await;
        editor
            .update_in(cx, |editor, window, cx| {
                hover_popover::hover(editor, &Hover, window, cx)
            })
            .ok();
    })
    .detach();
}

fn widget(
    editor: &mut Editor,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> Option<AnyElement> {
    if !editor.mode().is_full() || !editor.buffer().read(cx).is_singleton() {
        return None;
    }
    let problems = Problems::of(editor, cx);
    let dark = !cx.theme().appearance().is_light();
    let tokens = Tokens::for_appearance(dark);
    let handle = cx.entity().downgrade();
    let go = move |next: bool| {
        let handle = handle.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            handle
                .update(cx, |editor, cx| go_to_problem(editor, next, window, cx))
                .ok();
        }
    };
    let button = |id: &'static str| {
        div()
            .id(id)
            .role(Role::Button)
            .flex_none()
            .h(px(BUTTON))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(metrics::RADIUS))
            .cursor_pointer()
            .hover(|button| button.bg(tokens.editor_button_hover))
            .active(|button| button.bg(tokens.editor_button_pressed))
    };
    // the label font two points smaller (`TrafficLightButton`)
    let count = |icon: &str, color: Hsla, count: usize| {
        div()
            .flex()
            .items_center()
            .gap(px(1.))
            .child(
                svg()
                    .path(format!("wb/icons/general/inspections/{icon}.svg"))
                    .size(px(metrics::TOOLBAR_ICON))
                    .flex_none()
                    .text_color(color),
            )
            .child(count.to_string())
    };
    let summary: SharedString = problems.summary().into();
    let counts = button("inspection-widget-counts")
        .aria_label(summary.clone())
        .px(px(PADDING))
        .gap(px(BETWEEN))
        .text_ui_xsmall()
        .text_color(tokens.inspection_count)
        .when(problems.is_empty(), |button| {
            let suffix = if dark { "_dark" } else { "" };
            button.child(
                img(format!(
                    "wb/icons/general/inspections/inspectionsOK{suffix}.svg"
                ))
                .size(px(metrics::TOOLBAR_ICON))
                .flex_none(),
            )
        })
        .when(problems.errors > 0, |button| {
            button.child(count(
                "inspectionsError",
                tokens.inspection_error,
                problems.errors,
            ))
        })
        .when(problems.warnings > 0, |button| {
            button.child(count(
                "inspectionsWarning",
                tokens.inspection_warning,
                problems.warnings,
            ))
        })
        .when(problems.weak_warnings > 0, |button| {
            button.child(count(
                "inspectionsWarning",
                tokens.inspection_weak_warning,
                problems.weak_warnings,
            ))
        })
        .tooltip(move |_, cx| {
            cx.new(|_| ProblemsPopup {
                summary: summary.clone(),
            })
            .into()
        })
        // IntelliJ's widget opens the Problems tool window; its arrows go from problem to problem
        .on_click(|_, window, cx| {
            window.dispatch_action(
                spork_tool_windows::problems_panel::ToggleFocus.boxed_clone(),
                cx,
            )
        });
    let arrow = |id: &'static str, icon: &str, label: &'static str| {
        button(id).w(px(BUTTON)).aria_label(label).child(
            svg()
                .path(format!("wb/icons/general/{icon}.svg"))
                .size(px(metrics::TOOLBAR_ICON))
                .text_color(tokens.icon),
        )
    };
    let toolbar = div()
        .flex()
        .items_center()
        .p(px(INSET))
        .bg(cx.theme().colors().editor_background)
        .rounded(px(metrics::RADIUS))
        .child(counts)
        .when(!problems.is_empty(), |toolbar| {
            toolbar
                .child(
                    arrow(
                        "inspection-widget-previous",
                        "chevronUp",
                        "Previous Highlighted Error",
                    )
                    .on_click(go(false)),
                )
                .child(
                    arrow(
                        "inspection-widget-next",
                        "chevronDown",
                        "Next Highlighted Error",
                    )
                    .on_click(go(true)),
                )
        });
    Some(
        div()
            .id("inspection-widget")
            .occlude()
            .p(px(INSET))
            .flex()
            .items_center()
            .gap(px(BETWEEN))
            .children(
                cx.try_global::<FloatingToolbars>()
                    .map(|toolbars| toolbars.0.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|toolbar| toolbar(editor, window, cx)),
            )
            .child(toolbar)
            // a Markdown file's layout actions, IntelliJ's beside its inspections
            .children(crate::markdown::editor_corner(editor, window, cx))
            // drawn where the selection is, over the text
            .children(crate::markdown_styling::floating_toolbar(
                editor, window, cx,
            ))
            .into_any_element(),
    )
}

/// The widget's hover popup (`TrafficLightPopup`): the counts, or that there are none.
struct ProblemsPopup {
    summary: SharedString,
}

impl Render for ProblemsPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::for_appearance(!cx.theme().appearance().is_light());
        let settings = ThemeSettings::get_global(cx);
        div()
            .min_w(px(POPUP_MIN_WIDTH))
            .p(px(POPUP_PADDING))
            .bg(tokens.popup)
            .border_1()
            .border_color(tokens.popup_border)
            .rounded(px(metrics::RADIUS_LARGE))
            .font(settings.ui_font.clone())
            .text_ui()
            .text_color(tokens.text)
            .child(self.summary.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::Problems;

    #[test]
    fn the_summary_reads_as_intellij_s() {
        let problems = Problems {
            errors: 1,
            warnings: 2,
            weak_warnings: 1,
        };
        assert_eq!(problems.summary(), "1 error, 2 warnings, 1 weak warning");
        assert_eq!(Problems::default().summary(), "No problems found");
        let warnings = Problems {
            warnings: 1,
            ..Problems::default()
        };
        assert_eq!(warnings.summary(), "1 warning");
    }
}
