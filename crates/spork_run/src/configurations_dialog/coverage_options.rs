//! A configuration's Code Coverage options (IntelliJ's `CoverageFragment`): Modify options ▸
//! Code Coverage ▸ Coverage settings, its Specify classes and packages and Exclude classes and
//! packages, each a panel of class patterns (checked when active) with Add… (Add Class…, Add
//! Package…) and Remove, the patterns written to the configuration as typed.

use collections::HashSet;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    div, px,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use spork_workbench::{ActionButton, Tokens};
use util::ResultExt as _;

use super::ConfigurationsDialog;
use crate::configurations::{self, Configuration, CoveragePattern};

/// The fragments' state while the dialog edits a configuration.
#[derive(Default)]
pub(super) struct CoverageRows {
    /// The fragments shown while they have no pattern yet: include (`true`) and exclude.
    shown: HashSet<bool>,
    /// Each pattern: whether it includes, its field and whether it is active.
    rows: Vec<(bool, Entity<InputState>, bool)>,
    selected: Option<usize>,
}

impl CoverageRows {
    fn has(&self, include: bool) -> bool {
        self.shown.contains(&include) || self.rows.iter().any(|(kind, _, _)| *kind == include)
    }
}

impl ConfigurationsDialog {
    /// The configuration's patterns into the fragments' rows.
    pub(super) fn load_coverage(
        &mut self,
        configuration: &Configuration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.coverage = CoverageRows::default();
        for pattern in configurations::coverage_patterns(configuration) {
            self.add_coverage_row(
                pattern.include,
                &pattern.pattern,
                pattern.enabled,
                window,
                cx,
            );
        }
    }

    fn add_coverage_row(
        &mut self,
        include: bool,
        text: &str,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = text.to_owned();
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(text, window, cx);
            input
        });
        let subscription = cx.subscribe_in(&input, window, |this, _, event, _, cx| {
            if let InputEvent::Change = event {
                this.write_coverage(cx);
            }
        });
        self._fields.push(subscription);
        self.coverage.rows.push((include, input, enabled));
    }

    fn write_coverage(&mut self, cx: &mut Context<Self>) {
        let patterns: Vec<CoveragePattern> = self
            .coverage
            .rows
            .iter()
            .map(|(include, input, enabled)| CoveragePattern {
                pattern: input.read(cx).value().trim().to_owned(),
                include: *include,
                enabled: *enabled,
            })
            .collect();
        if let Some(configuration) = self.edited_mut() {
            match patterns.is_empty() {
                true => {
                    configuration
                        .entry
                        .remove(configurations::COVERAGE_PATTERNS);
                }
                false => configurations::set_coverage_patterns(configuration, &patterns),
            }
        }
        cx.notify();
    }

    /// Modify options' Code Coverage group: its Coverage settings submenu.
    pub(super) fn coverage_menu(
        menu: PopupMenu,
        this: gpui::WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let (include, exclude) = match this.upgrade() {
            Some(dialog) => {
                let rows = &dialog.read(cx).coverage;
                (rows.has(true), rows.has(false))
            }
            None => (false, false),
        };
        menu.label("Code Coverage")
            .submenu("Coverage settings", window, cx, move |menu, _, _| {
                let item = |label: &'static str, on: bool, kind: bool| {
                    let this = this.clone();
                    PopupMenuItem::new(label)
                        .checked(on)
                        .on_click(move |_, window, cx| {
                            this.update(cx, |this, cx| {
                                this.toggle_coverage_fragment(kind, window, cx)
                            })
                            .log_err();
                        })
                };
                menu.item(item("Specify classes and packages", include, true))
                    .item(item("Exclude classes and packages", exclude, false))
            })
    }

    /// Shows a fragment, or hides it and clears its patterns (IntelliJ's, on Apply).
    fn toggle_coverage_fragment(&mut self, include: bool, _: &mut Window, cx: &mut Context<Self>) {
        if self.coverage.has(include) {
            self.coverage.shown.remove(&include);
            self.coverage.rows.retain(|(kind, _, _)| *kind != include);
            self.coverage.selected = None;
            self.write_coverage(cx);
        } else {
            self.coverage.shown.insert(include);
            cx.notify();
        }
    }

    /// The fragments shown, each its titled panel of patterns.
    pub(super) fn coverage_fragments(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        [true, false]
            .into_iter()
            .filter(|include| self.coverage.has(*include))
            .map(|include| self.coverage_fragment(include, cx))
            .collect()
    }

    fn coverage_fragment(&self, include: bool, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let title = match include {
            true => "Packages and classes to include in coverage data",
            false => "Packages and classes to exclude from coverage data",
        };
        let id = match include {
            true => "coverage-include",
            false => "coverage-exclude",
        };
        let rows: Vec<AnyElement> = self
            .coverage
            .rows
            .iter()
            .enumerate()
            .filter(|(_, (kind, _, _))| *kind == include)
            .map(|(ix, (_, input, enabled))| {
                let selected = self.coverage.selected == Some(ix);
                h_flex()
                    .id(SharedString::from(format!("{id}-row-{ix}")))
                    .role(Role::Row)
                    .px(px(4.))
                    .py(px(1.))
                    .gap(px(4.))
                    .when(selected, |row| row.bg(tokens.selection))
                    .child(
                        spork_workbench::controls::check_box(
                            SharedString::from(format!("{id}-active-{ix}")),
                            Some(*enabled),
                            cx,
                        )
                        .aria_label("Is Active")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some((_, _, enabled)) = this.coverage.rows.get_mut(ix) {
                                *enabled = !*enabled;
                            }
                            this.write_coverage(cx);
                        })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(spork_workbench::TextField::new(input).aria_label("Pattern")),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.coverage.selected = Some(ix);
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        let this = cx.entity().downgrade();
        let add = ActionButton::new(SharedString::from(format!("{id}-add")), "general/add")
            .tooltip("Add…")
            .dropdown_menu(move |menu, _, _| {
                let item = |label: &'static str| {
                    let this = this.clone();
                    PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        this.update(cx, |this, cx| {
                            this.add_coverage_row(include, "", true, window, cx);
                            this.coverage.selected = Some(this.coverage.rows.len() - 1);
                            if let Some((_, input, _)) = this.coverage.rows.last() {
                                input.update(cx, |input, cx| input.focus(window, cx));
                            }
                            this.write_coverage(cx);
                        })
                        .log_err();
                    })
                };
                menu.item(item("Add Class…")).item(item("Add Package…"))
            });
        let remove_selected = self.coverage.selected.filter(|ix| {
            self.coverage
                .rows
                .get(*ix)
                .is_some_and(|row| row.0 == include)
        });
        let toolbar = h_flex()
            .id(SharedString::from(format!("{id}-toolbar")))
            .role(Role::Toolbar)
            .aria_label(title)
            .gap(px(2.))
            .p(px(2.))
            .border_b_1()
            .border_color(tokens.separator)
            .child(add)
            .child(
                ActionButton::new(SharedString::from(format!("{id}-remove")), "general/remove")
                    .tooltip("Remove")
                    .disabled(remove_selected.is_none())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(ix) = remove_selected {
                            this.coverage.rows.remove(ix);
                            this.coverage.selected = None;
                            this.write_coverage(cx);
                        }
                    })),
            );
        let body = match empty {
            true => div()
                .h(px(64.))
                .flex()
                .items_center()
                .justify_center()
                .text_color(tokens.text_secondary)
                .child(
                    div()
                        .id(SharedString::from(format!("{id}-empty")))
                        .role(Role::Label)
                        .aria_label("No class patterns configured")
                        .child("No class patterns configured"),
                )
                .into_any_element(),
            false => v_flex().children(rows).into_any_element(),
        };
        v_flex()
            .gap(px(4.))
            .child(div().font_weight(gpui::FontWeight::BOLD).child(title))
            .child(
                v_flex()
                    .id(id)
                    .role(Role::Table)
                    .aria_label(title)
                    .border_1()
                    .border_color(tokens.separator)
                    .child(toolbar)
                    .child(body),
            )
            .into_any_element()
    }
}
