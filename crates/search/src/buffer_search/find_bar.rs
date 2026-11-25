//! Spork IDE: IntelliJ's find and replace bar over the editor (`EditorSearchSession` and
//! `SearchReplaceComponent` in the New UI with Islands). The mode chevron, the search field with
//! its history and its Cc, W and .* options, the match counter ("3/12", "0 results"), Previous and
//! Next Occurrence, the filter and More popups and Close; under it, in replace mode, the replace
//! field with its history and Preserve case, then Replace, Replace All and Exclude. The search, its
//! options and its history stay Zed's (`BufferSearchBar`); Zed's own look stays for the project
//! search's results and other multibuffers.
//!
//! The filter popup holds Search In Selection only: Zed's search can't tell comments and string
//! literals apart, so IntelliJ's Anywhere, In Comments, In String Literals and the Except… filters
//! are left out. Scroll to Results During Typing is left out of More: Zed always scrolls.

use std::ops::Range;

use editor::{Anchor, Editor, EditorElement, EditorStyle, ToOffset as _};
use gpui::{
    AnyElement, App, ClickEvent, Context, DismissEvent, Entity, Focusable as _, Hsla,
    InteractiveElement, IntoElement, KeyContext, ParentElement as _, Pixels, Point, Role,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Subscription, TextStyle,
    Toggled, Window, anchored, deferred, div, px, relative, svg,
};
use settings::Settings as _;
use spork_workbench::controls::A11yDisabled;
use spork_workbench::tokens::{Tokens, metrics};
use theme::ActiveTheme as _;
use theme_settings::ThemeSettings;
use ui::{ContextMenu, Tooltip, prelude::*};
use workspace::searchable::FilteredSearchRange;
use zed_actions::{outline::ToggleOutline, workspace::CopyPath, workspace::CopyRelativePath};

use super::{BufferSearchBar, Dismiss};
use crate::{
    ReplaceAll, ReplaceNext, SearchOptions, SelectAllMatches, SelectNextMatch, SelectPreviousMatch,
    ToggleCaseSensitive, ToggleRegex, ToggleReplace, ToggleSelection, ToggleWholeWord,
};

/// A row of the bar, its fields and the 22 px buttons beside them (IntelliJ's text area is 28 px
/// high with 3 px around it).
const ROW_HEIGHT: Pixels = px(34.);
const FIELD_HEIGHT: Pixels = px(28.);
/// The fields' share of the bar, `SearchReplaceComponent.DEFAULT_PROP`.
const FIELD_SHARE: f32 = 0.33;
const FIELD_MIN_WIDTH: Pixels = px(240.);
/// The compact buttons of the replace row (Replace, Replace All, Exclude).
const TEXT_BUTTON_HEIGHT: Pixels = px(24.);
const MAX_REPLACE_HISTORY: usize = 50;

gpui::actions!(
    search,
    [
        /// Moves the find bar's results into the Find tool window.
        OpenResultsInFindWindow,
        /// Leaves the current occurrence out of Replace and Replace All, or puts it back.
        ExcludeMatch,
        /// Turns Preserve case on or off for the find bar's replacements.
        TogglePreserveCase,
    ]
);

/// IntelliJ's state the bar keeps beside Zed's.
#[derive(Default)]
pub(super) struct FindBar {
    preserve_case: bool,
    /// The occurrences Exclude left out, in the editor searched, and the query they were found
    /// with: another query starts with none left out.
    excluded: Vec<Range<Anchor>>,
    excluded_query: String,
    replace_history: Vec<String>,
    menu: Option<(Entity<ContextMenu>, Point<Pixels>, Subscription)>,
}

impl BufferSearchBar {
    /// Whether the bar shows IntelliJ's look: over an editor or a terminal, not over the project
    /// search's results or a multibuffer, which keep Zed's.
    pub(super) fn shows_find_bar(&self, cx: &mut Context<Self>) -> bool {
        !self.supported_options(cx).find_in_results && !self.needs_expand_collapse_option(cx)
    }

    /// The search options the find bar shows, for the Find tool window.
    pub fn search_options(&self) -> SearchOptions {
        self.search_options
    }

    pub(super) fn render_find_bar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = Tokens::for_appearance(!cx.theme().appearance().is_light());
        let supported = self.supported_options(cx);
        let replacing = self.replace_enabled && supported.replacement;
        let in_replace = self.replacement_editor.focus_handle(cx).is_focused(window);
        let focus = self.query_editor.focus_handle(cx);
        let has_matches = self.match_count() > 0;

        let mut key_context = KeyContext::new_with_defaults(cx.platform_info().os);
        key_context.add("BufferSearchBar");
        if in_replace {
            key_context.add("in_replace");
        }

        let options = h_flex()
            .flex_none()
            .gap(px(2.))
            .child(
                tool_button("find-new-line", "inline/newLine", "New Line", tokens).on_click(
                    cx.listener(|this, _, window, cx| {
                        this.query_editor
                            .update(cx, |editor, cx| editor.insert("\n", window, cx));
                    }),
                ),
            )
            .when(supported.case, |row| {
                row.child(
                    option_button(
                        "find-match-case",
                        "inline/matchCase",
                        "Match Case",
                        self.search_options.contains(SearchOptions::CASE_SENSITIVE),
                        tokens,
                    )
                    .tooltip(action_tooltip("Match Case", &ToggleCaseSensitive, &focus))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_search_option(SearchOptions::CASE_SENSITIVE, window, cx)
                    })),
                )
            })
            .when(supported.word, |row| {
                row.child(
                    option_button(
                        "find-words",
                        "inline/exactWords",
                        "Words",
                        self.search_options.contains(SearchOptions::WHOLE_WORD),
                        tokens,
                    )
                    .tooltip(action_tooltip("Words", &ToggleWholeWord, &focus))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_search_option(SearchOptions::WHOLE_WORD, window, cx)
                    })),
                )
            })
            .when(supported.regex, |row| {
                row.child(
                    option_button(
                        "find-regex",
                        "inline/regex",
                        "Regex",
                        self.search_options.contains(SearchOptions::REGEX),
                        tokens,
                    )
                    .tooltip(action_tooltip("Regex", &ToggleRegex, &focus))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_search_option(SearchOptions::REGEX, window, cx)
                    })),
                )
            });

        let error = self.query_error.is_some() || (!self.query(cx).is_empty() && !has_matches);
        let search_field = field(
            "Search",
            focus.is_focused(window),
            tokens,
            cx.theme().appearance().is_light(),
        )
        .child(
            tool_button(
                "find-history",
                "inline/searchHistory",
                "Search History",
                tokens,
            )
            .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                this.show_history(false, event.position(), window, cx)
            })),
        )
        .child(div().flex_1().min_w_0().child(text_input(
            &self.query_editor,
            error.then_some(tokens.text_error),
            cx,
        )))
        .child(options);

        let status = self.status_text(cx).map(|(text, error)| {
            div()
                .id("find-status")
                .role(Role::Label)
                .aria_label(text.clone())
                .flex_none()
                .px(px(6.))
                .text_color(match error {
                    true => tokens.text_error,
                    false => tokens.text_secondary,
                })
                .child(text)
        });

        let search_row = h_flex()
            .h(ROW_HEIGHT)
            .gap(px(2.))
            .child(self.mode_button(supported.replacement, tokens, cx))
            .child(
                div()
                    .flex_none()
                    .w(relative(FIELD_SHARE))
                    .min_w(FIELD_MIN_WIDTH)
                    .child(search_field),
            )
            .child(div().w(px(6.)))
            .children(status)
            .child(
                tool_button("find-previous", "general/up", "Previous Occurrence", tokens)
                    .tooltip(action_tooltip(
                        "Previous Occurrence",
                        &SelectPreviousMatch,
                        &focus,
                    ))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.select_prev_match(&SelectPreviousMatch, window, cx)
                    }))
                    .disabled(!has_matches),
            )
            .child(
                tool_button("find-next", "general/down", "Next Occurrence", tokens)
                    .tooltip(action_tooltip("Next Occurrence", &SelectNextMatch, &focus))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.select_next_match(&SelectNextMatch, window, cx)
                    }))
                    .disabled(!has_matches),
            )
            .when(supported.selection, |row| {
                row.child(
                    tool_button(
                        "find-filter",
                        "general/filter",
                        "Filter Search Results",
                        tokens,
                    )
                    .when(self.selection_search_enabled.is_some(), |button| {
                        button.bg(tokens.toolbar_selected)
                    })
                    .tooltip(Tooltip::text("Filter Search Results"))
                    .on_click(cx.listener(
                        |this, event: &ClickEvent, window, cx| {
                            this.show_filter_menu(event.position(), window, cx)
                        },
                    )),
                )
            })
            .child(
                tool_button(
                    "find-more",
                    "general/moreVertical",
                    "Open in Window, Multiple Cursors",
                    tokens,
                )
                .tooltip(Tooltip::text("Open in Window, Multiple Cursors"))
                .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                    this.show_more_menu(event.position(), window, cx)
                })),
            )
            .child(div().flex_1())
            .child(
                tool_button("find-close", "general/closeSmall", "Close", tokens)
                    .tooltip(action_tooltip("Close", &Dismiss, &focus))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.dismiss(&Dismiss, window, cx)),
                    ),
            );

        let replace_row = replacing.then(|| {
            let replace_focus = self.replacement_editor.focus_handle(cx);
            let replace_field = field(
                "Replace",
                replace_focus.is_focused(window),
                tokens,
                cx.theme().appearance().is_light(),
            )
            .child(
                tool_button(
                    "replace-history",
                    "inline/searchHistory",
                    "Replace History",
                    tokens,
                )
                .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                    this.show_history(true, event.position(), window, cx)
                })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(text_input(&self.replacement_editor, None, cx)),
            )
            .child(
                option_button(
                    "replace-preserve-case",
                    "inline/preserveCase",
                    "Preserve case",
                    self.find_bar.preserve_case,
                    tokens,
                )
                .tooltip(Tooltip::text("Preserve case"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_preserve_case(&TogglePreserveCase, window, cx)
                })),
            );
            let excluded = self.current_match_excluded(cx);
            h_flex()
                .h(ROW_HEIGHT)
                .gap(px(2.))
                .child(div().w(px(metrics::TOOLBAR_BUTTON)).flex_none())
                .child(
                    div()
                        .flex_none()
                        .w(relative(FIELD_SHARE))
                        .min_w(FIELD_MIN_WIDTH)
                        .child(replace_field),
                )
                .child(div().w(px(6.)))
                .child(
                    text_button("replace-current", "Replace", !has_matches, tokens).on_click(
                        cx.listener(|this, _, window, cx| {
                            this.replace_current(&ReplaceNext, window, cx)
                        }),
                    ),
                )
                .child(
                    text_button("replace-all", "Replace All", !has_matches, tokens).on_click(
                        cx.listener(|this, _, window, cx| {
                            this.replace_all_included(&ReplaceAll, window, cx)
                        }),
                    ),
                )
                .child(
                    text_button(
                        "replace-exclude",
                        if excluded { "Include" } else { "Exclude" },
                        self.active_match_index.is_none(),
                        tokens,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.exclude_match(&ExcludeMatch, window, cx)
                    })),
                )
        });

        let menu = self.find_bar.menu.as_ref().map(|(menu, position, _)| {
            deferred(
                anchored()
                    .position(*position)
                    .anchor(gpui::Anchor::TopLeft)
                    .snap_to_window_with_margin(px(8.))
                    .child(menu.clone()),
            )
            .with_priority(1)
        });

        v_flex()
            .id("find-bar")
            .role(Role::Group)
            .aria_label("Find")
            .w_full()
            // the toolbar pads its items: the bar reaches its edges, as IntelliJ's
            .mx(px(-8.))
            .my(px(-6.))
            .pl(px(4.))
            .pr(px(6.))
            .py(px(1.))
            .bg(tokens.editor_search)
            .border_b_1()
            .border_color(tokens.editor_search_border)
            .text_color(tokens.text)
            .key_context(key_context)
            // a click in the bar keeps the focus in its fields, as IntelliJ's; the pane under it
            // would give it to the editor
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    let fields = [&this.query_editor, &this.replacement_editor];
                    if !fields
                        .iter()
                        .any(|field| field.focus_handle(cx).is_focused(window))
                    {
                        this.focus(&this.query_editor.focus_handle(cx), window, cx);
                    }
                }),
            )
            .capture_action(cx.listener(Self::tab))
            .capture_action(cx.listener(Self::backtab))
            .capture_action(cx.listener(Self::toggle_soft_wrap))
            .on_action(cx.listener(|this, _: &ToggleOutline, window, cx| {
                if let Some(item) = &mut this.active_searchable_item {
                    item.relay_action(Box::new(ToggleOutline), window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CopyPath, window, cx| {
                if let Some(item) = &mut this.active_searchable_item {
                    item.relay_action(Box::new(CopyPath), window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CopyRelativePath, window, cx| {
                if let Some(item) = &mut this.active_searchable_item {
                    item.relay_action(Box::new(CopyRelativePath), window, cx);
                }
            }))
            .on_action(cx.listener(Self::previous_history_query))
            .on_action(cx.listener(Self::next_history_query))
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::select_next_match))
            .on_action(cx.listener(Self::select_prev_match))
            .on_action(cx.listener(Self::exclude_match))
            .on_action(cx.listener(Self::toggle_preserve_case))
            .when(supported.replacement, |this| {
                this.on_action(cx.listener(Self::toggle_replace))
                    .on_action(cx.listener(Self::replace_current))
                    .on_action(cx.listener(Self::replace_all_included))
            })
            .when(supported.case, |this| {
                this.on_action(cx.listener(Self::toggle_case_sensitive))
            })
            .when(supported.word, |this| {
                this.on_action(cx.listener(Self::toggle_whole_word))
            })
            .when(supported.regex, |this| {
                this.on_action(cx.listener(Self::toggle_regex))
            })
            .when(supported.selection, |this| {
                this.on_action(cx.listener(Self::toggle_selection))
            })
            .child(search_row)
            .children(replace_row)
            .children(menu)
            .into_any_element()
    }

    /// IntelliJ's mode button: Show Replace Field (›) or Hide Replace Field (⌄).
    fn mode_button(
        &self,
        replacement: bool,
        tokens: &'static Tokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !replacement {
            return div()
                .w(px(metrics::TOOLBAR_BUTTON))
                .flex_none()
                .into_any_element();
        }
        let (icon, label) = match self.replace_enabled {
            true => ("general/chevronDown", "Hide Replace Field"),
            false => ("general/chevronRight", "Show Replace Field"),
        };
        let focus = self.query_editor.focus_handle(cx);
        tool_button("find-mode", icon, label, tokens)
            .aria_toggled(match self.replace_enabled {
                true => Toggled::True,
                false => Toggled::False,
            })
            .tooltip(action_tooltip(label, &ToggleReplace, &focus))
            .on_click(
                cx.listener(|this, _, window, cx| this.toggle_replace(&ToggleReplace, window, cx)),
            )
            .into_any_element()
    }

    fn match_count(&self) -> usize {
        self.active_searchable_item
            .as_ref()
            .and_then(|item| self.searchable_items_with_matches.get(&item.downgrade()))
            .map_or(0, |(matches, _)| matches.len())
    }

    /// IntelliJ's status: "3/12" on an occurrence, "12 results" elsewhere, "0 results" in the
    /// error colour; nothing while the field is empty.
    fn status_text(&self, cx: &App) -> Option<(SharedString, bool)> {
        if self.query(cx).is_empty() {
            return None;
        }
        let count = self.match_count();
        Some(match (self.active_match_index, count) {
            (_, 0) => ("0 results".into(), true),
            (Some(ix), count) => (format!("{}/{count}", ix + 1).into(), false),
            (None, 1) => ("1 result".into(), false),
            (None, count) => (format!("{count} results").into(), false),
        })
    }

    fn show_menu(
        &mut self,
        menu: Entity<ContextMenu>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let subscription = cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, _, cx| {
            this.find_bar.menu.take();
            cx.notify();
        });
        window.focus(&menu.focus_handle(cx), cx);
        self.find_bar.menu = Some((menu, position, subscription));
        cx.notify();
    }

    /// The searches (or replacements) made lately, the latest first; choosing one searches again.
    fn show_history(
        &mut self,
        replace: bool,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries: Vec<String> = match replace {
            true => self
                .find_bar
                .replace_history
                .iter()
                .rev()
                .cloned()
                .collect(),
            false => {
                let mut entries: Vec<String> =
                    self.search_history.iter().map(str::to_owned).collect();
                entries.reverse();
                entries
            }
        };
        if entries.is_empty() {
            return;
        }
        let this = cx.entity().downgrade();
        let menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
            for entry in entries {
                let this = this.clone();
                let label = entry.replace('\n', "⏎");
                menu = menu.entry(label, None, move |window, cx| {
                    let entry = entry.clone();
                    this.update(cx, |this, cx| {
                        let editor = match replace {
                            true => this.replacement_editor.clone(),
                            false => this.query_editor.clone(),
                        };
                        editor.update(cx, |editor, cx| editor.set_text(entry, window, cx));
                        window.focus(&editor.focus_handle(cx), cx);
                    })
                    .ok();
                });
            }
            menu
        });
        self.show_menu(menu, position, window, cx);
    }

    /// The filter popup: Search In Selection (IntelliJ's context filters are left out, see the
    /// module's comment).
    fn show_filter_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let in_selection = self.selection_search_enabled.is_some();
        let focus = self.query_editor.focus_handle(cx);
        let this = cx.entity().downgrade();
        let menu = ContextMenu::build(window, cx, move |menu, _, _| {
            menu.context(focus).toggleable_entry(
                "Search In Selection",
                in_selection,
                IconPosition::Start,
                Some(Box::new(ToggleSelection)),
                move |window, cx| {
                    this.update(cx, |this, cx| {
                        drop(this.set_search_within_selection(
                            (!in_selection).then_some(FilteredSearchRange::Default),
                            window,
                            cx,
                        ));
                    })
                    .ok();
                },
            )
        });
        self.show_menu(menu, position, window, cx);
    }

    /// More (⋮): Open Results in Find Window, then IntelliJ's Multiple Cursors actions.
    fn show_more_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = self.query_editor.focus_handle(cx);
        let editor_focus = self
            .active_searchable_item
            .as_ref()
            .map(|item| item.item_focus_handle(cx));
        let this = cx.entity().downgrade();
        let menu = ContextMenu::build(window, cx, move |menu, _, _| {
            let in_editor = |action: Box<dyn gpui::Action>| {
                let editor_focus = editor_focus.clone();
                move |window: &mut Window, cx: &mut App| {
                    if let Some(editor_focus) = &editor_focus {
                        window.focus(editor_focus, cx);
                    }
                    window.dispatch_action(action.boxed_clone(), cx);
                }
            };
            menu.context(focus)
                .action(
                    "Open Results in Find Window",
                    Box::new(OpenResultsInFindWindow),
                )
                .header("Multiple Cursors")
                .entry(
                    "Add Selection for Next Occurrence",
                    Some(Box::new(editor::actions::SelectNext {
                        replace_newest: false,
                    })),
                    in_editor(Box::new(editor::actions::SelectNext {
                        replace_newest: false,
                    })),
                )
                .entry(
                    "Unselect Occurrence",
                    Some(Box::new(editor::actions::UndoSelection)),
                    in_editor(Box::new(editor::actions::UndoSelection)),
                )
                .entry(
                    "Select All Occurrences",
                    Some(Box::new(SelectAllMatches)),
                    move |window, cx| {
                        this.update(cx, |this, cx| {
                            this.select_all_matches(&SelectAllMatches, window, cx)
                        })
                        .ok();
                    },
                )
        });
        self.show_menu(menu, position, window, cx);
    }

    fn toggle_preserve_case(
        &mut self,
        _: &TogglePreserveCase,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.find_bar.preserve_case = !self.find_bar.preserve_case;
        cx.notify();
    }

    /// The editor searched, when the bar searches one: Exclude and Preserve case work there.
    fn searched_editor(&self, cx: &App) -> Option<Entity<Editor>> {
        self.active_searchable_item
            .as_ref()?
            .act_as_type(std::any::TypeId::of::<Editor>(), cx)?
            .downcast::<Editor>()
            .ok()
    }

    /// The editor's occurrences, in order.
    fn editor_matches(&self) -> Vec<Range<Anchor>> {
        let Some(item) = self.active_searchable_item.as_ref() else {
            return Vec::new();
        };
        let Some((matches, _)) = self.searchable_items_with_matches.get(&item.downgrade()) else {
            return Vec::new();
        };
        (0..matches.len())
            .filter_map(|ix| matches.at(ix).downcast_ref::<Range<Anchor>>().cloned())
            .collect()
    }

    fn is_excluded(&self, range: &Range<Anchor>, editor: &Entity<Editor>, cx: &App) -> bool {
        if self.find_bar.excluded_query != self.query(cx) {
            return false;
        }
        let snapshot = editor.read(cx).buffer().read(cx).snapshot(cx);
        let offsets = |range: &Range<Anchor>| {
            range.start.to_offset(&snapshot)..range.end.to_offset(&snapshot)
        };
        let range = offsets(range);
        self.find_bar
            .excluded
            .iter()
            .any(|excluded| offsets(excluded) == range)
    }

    fn current_match_excluded(&self, cx: &App) -> bool {
        let (Some(ix), Some(editor)) = (self.active_match_index, self.searched_editor(cx)) else {
            return false;
        };
        self.editor_matches()
            .get(ix)
            .is_some_and(|range| self.is_excluded(range, &editor, cx))
    }

    /// Exclude: leaves the current occurrence out of Replace and Replace All (Include puts it
    /// back), then goes to the next one.
    fn exclude_match(&mut self, _: &ExcludeMatch, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(ix), Some(editor)) = (self.active_match_index, self.searched_editor(cx)) else {
            return;
        };
        let Some(range) = self.editor_matches().get(ix).cloned() else {
            return;
        };
        let query = self.query(cx);
        if self.find_bar.excluded_query != query {
            self.find_bar.excluded.clear();
            self.find_bar.excluded_query = query;
        }
        if self.is_excluded(&range, &editor, cx) {
            let snapshot = editor.read(cx).buffer().read(cx).snapshot(cx);
            let start = range.start.to_offset(&snapshot);
            self.find_bar
                .excluded
                .retain(|excluded| excluded.start.to_offset(&snapshot) != start);
        } else {
            self.find_bar.excluded.push(range);
        }
        self.select_next_match(&SelectNextMatch, window, cx);
        cx.notify();
    }

    fn remember_replacement(&mut self, cx: &App) {
        let replacement = self.replacement_editor.read(cx).text(cx);
        if replacement.is_empty() {
            return;
        }
        let history = &mut self.find_bar.replace_history;
        history.retain(|entry| *entry != replacement);
        history.push(replacement);
        if history.len() > MAX_REPLACE_HISTORY {
            history.remove(0);
        }
    }

    /// Whether replacements go through Preserve case: IntelliJ leaves a regular expression's
    /// replacements as they are.
    fn preserves_case(&self) -> bool {
        self.find_bar.preserve_case && !self.search_options.contains(SearchOptions::REGEX)
    }

    /// Replace: the current occurrence, unless excluded, then on to the next one.
    fn replace_current(&mut self, _: &ReplaceNext, window: &mut Window, cx: &mut Context<Self>) {
        self.remember_replacement(cx);
        let (Some(editor), Some(ix)) = (self.searched_editor(cx), self.active_match_index) else {
            // a terminal or another item: Zed's replace
            self.replace_next(&ReplaceNext, window, cx);
            return;
        };
        let Some(range) = self.editor_matches().get(ix).cloned() else {
            return;
        };
        if self.is_excluded(&range, &editor, cx) {
            cx.stop_propagation();
            self.select_next_match(&SelectNextMatch, window, cx);
        } else if self.preserves_case() {
            cx.stop_propagation();
            self.replace_preserving_case(&editor, vec![range], window, cx);
            self.select_next_match(&SelectNextMatch, window, cx);
        } else {
            self.replace_next(&ReplaceNext, window, cx);
        }
    }

    /// Replace All: every occurrence Exclude didn't leave out.
    fn replace_all_included(
        &mut self,
        _: &ReplaceAll,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.remember_replacement(cx);
        let Some(editor) = self.searched_editor(cx) else {
            self.replace_all(&ReplaceAll, window, cx);
            return;
        };
        let kept: Vec<(usize, Range<Anchor>)> = self
            .editor_matches()
            .into_iter()
            .enumerate()
            .filter(|(_, range)| !self.is_excluded(range, &editor, cx))
            .collect();
        if self.preserves_case() {
            let ranges = kept.into_iter().map(|(_, range)| range).collect();
            self.replace_preserving_case(&editor, ranges, window, cx);
            return;
        }
        let replacement = self.replacement(cx);
        if let Some(item) = self.active_searchable_item.as_ref()
            && let Some(query) = self.active_search.as_ref()
            && let Some((matches, token)) =
                self.searchable_items_with_matches.get(&item.downgrade())
        {
            let query = query.as_ref().clone().with_replacement(replacement);
            item.replace_all(
                &mut kept.iter().map(|(ix, _)| matches.at(*ix)),
                &query,
                *token,
                window,
                cx,
            );
        }
    }

    fn replace_preserving_case(
        &mut self,
        editor: &Entity<Editor>,
        ranges: Vec<Range<Anchor>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let replacement = self.replacement(cx);
        let snapshot = editor.read(cx).buffer().read(cx).snapshot(cx);
        let edits: Vec<(Range<Anchor>, String)> = ranges
            .into_iter()
            .map(|range| {
                let found: String = snapshot.text_for_range(range.clone()).collect();
                (range, preserve_case(&replacement, &found))
            })
            .collect();
        if edits.is_empty() {
            return;
        }
        editor.update(cx, |editor, cx| {
            editor.transact(window, cx, |editor, _, cx| editor.edit(edits, cx));
        });
    }
}

/// `replacement` in the case of `found`, as IntelliJ's `PreserveCaseUtil.replaceWithCaseRespect`:
/// all capitals, or the first letter's case.
pub fn preserve_case(replacement: &str, found: &str) -> String {
    let letters: Vec<char> = found.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return replacement.to_owned();
    }
    if letters.iter().all(|c| c.is_uppercase()) {
        return replacement.to_uppercase();
    }
    // a replacement in capitals takes the found word's case from its first letter only
    let replacement = match replacement.chars().any(|c| c.is_lowercase()) {
        true => replacement.to_owned(),
        false => replacement.to_lowercase(),
    };
    let mut chars = replacement.chars();
    let Some(first) = chars.next() else {
        return replacement;
    };
    let first: String = match found.chars().next().is_some_and(char::is_uppercase) {
        true => first.to_uppercase().collect(),
        false => first.to_lowercase().collect(),
    };
    first + chars.as_str()
}

/// A field of the bar: a rounded border, the accent's while focused, and in a light theme the
/// text field's fill (`SearchReplaceComponent.updateTextComponentBorder`).
fn field(
    label: &'static str,
    focused: bool,
    tokens: &'static Tokens,
    light: bool,
) -> Stateful<Div> {
    h_flex()
        .id(label)
        .role(Role::Group)
        .aria_label(label)
        .h(FIELD_HEIGHT)
        .pl(px(3.))
        .pr(px(3.))
        .gap(px(2.))
        .rounded(px(metrics::RADIUS))
        .border_1()
        .border_color(match focused {
            true => tokens.focus,
            false => tokens.control_border,
        })
        .when(light, |field| field.bg(tokens.control_bg))
}

/// The text in a field, in the interface's font as IntelliJ's text area draws it.
fn text_input(editor: &Entity<Editor>, color: Option<Hsla>, cx: &App) -> impl IntoElement {
    let settings = ThemeSettings::get_global(cx);
    let theme = cx.theme();
    let text = TextStyle {
        color: color.unwrap_or(theme.colors().text),
        font_family: settings.ui_font.family.clone(),
        font_features: settings.ui_font.features.clone(),
        font_fallbacks: settings.ui_font.fallbacks.clone(),
        font_size: settings.ui_font_size(cx).into(),
        font_weight: settings.ui_font.weight,
        line_height: relative(1.3),
        ..TextStyle::default()
    };
    EditorElement::new(
        editor,
        EditorStyle {
            background: gpui::transparent_black(),
            local_player: theme.players().local(),
            text,
            ..EditorStyle::default()
        },
    )
}

/// IntelliJ's 22 px toolbar button, its 16 px icon in the icon colour.
fn tool_button(
    id: &'static str,
    icon: &str,
    label: &'static str,
    tokens: &'static Tokens,
) -> ToolButton {
    ToolButton {
        base: div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .size(px(metrics::TOOLBAR_BUTTON))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(metrics::RADIUS)),
        icon: format!("wb/icons/{icon}.svg").into(),
        tokens,
        disabled: false,
        on_click: None,
    }
}

/// A search option in a field (Cc, W, .*, Preserve case): on, it takes `toolbar_selected`.
fn option_button(
    id: &'static str,
    icon: &str,
    label: &'static str,
    on: bool,
    tokens: &'static Tokens,
) -> ToolButton {
    tool_button(id, icon, label, tokens)
        .aria_toggled(match on {
            true => Toggled::True,
            false => Toggled::False,
        })
        .when(on, |button| button.bg(tokens.toolbar_selected))
}

/// A toolbar button, greyed (and left alone by the pointer) while it can't act.
struct ToolButton {
    base: Stateful<Div>,
    icon: SharedString,
    tokens: &'static Tokens,
    disabled: bool,
    on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
}

impl ToolButton {
    fn map(mut self, f: impl FnOnce(Stateful<Div>) -> Stateful<Div>) -> Self {
        self.base = f(self.base);
        self
    }

    fn tooltip(self, tooltip: impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static) -> Self {
        self.map(|base| base.tooltip(tooltip))
    }

    fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    fn aria_toggled(self, toggled: Toggled) -> Self {
        self.map(|base| base.aria_toggled(toggled))
    }

    fn bg(self, fill: Hsla) -> Self {
        self.map(|base| base.bg(fill))
    }

    fn when(self, condition: bool, then: impl FnOnce(Self) -> Self) -> Self {
        match condition {
            true => then(self),
            false => self,
        }
    }

    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl IntoElement for ToolButton {
    type Element = A11yDisabled<Stateful<Div>>;

    fn into_element(self) -> Self::Element {
        let tokens = self.tokens;
        let base = match (self.disabled, self.on_click) {
            (false, Some(handler)) => self
                .base
                .cursor_pointer()
                .hover(move |button| button.bg(tokens.hover))
                .active(move |button| button.bg(tokens.pressed))
                .on_click(handler),
            _ => self.base,
        };
        let icon = svg()
            .path(self.icon)
            .size(px(metrics::TOOLBAR_ICON))
            .text_color(match self.disabled {
                true => tokens.icon_disabled,
                false => tokens.icon,
            });
        A11yDisabled::new(base.child(icon), self.disabled)
    }
}

/// The replace row's buttons: IntelliJ's compact buttons with their text.
fn text_button(
    id: &'static str,
    label: &'static str,
    disabled: bool,
    tokens: &'static Tokens,
) -> TextButton {
    TextButton {
        base: div()
            .id(id)
            .debug_selector(move || id.into())
            .role(Role::Button)
            .aria_label(label)
            .h(TEXT_BUTTON_HEIGHT)
            .px(px(10.))
            .flex_none()
            .flex()
            .items_center()
            .rounded(px(metrics::RADIUS))
            .border_1()
            .border_color(tokens.control_border)
            .text_color(match disabled {
                true => tokens.text_disabled,
                false => tokens.text,
            })
            .when(!disabled, |button| {
                button
                    .cursor_pointer()
                    .hover(move |button| button.bg(tokens.hover))
                    .active(move |button| button.bg(tokens.pressed))
            })
            .child(label),
        disabled,
    }
}

struct TextButton {
    base: Stateful<Div>,
    disabled: bool,
}

impl TextButton {
    fn on_click(
        self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> A11yDisabled<Stateful<Div>> {
        let base = match self.disabled {
            true => self.base,
            false => self.base.on_click(handler),
        };
        A11yDisabled::new(base, self.disabled)
    }
}

fn action_tooltip<A: gpui::Action + Clone>(
    label: &'static str,
    action: &A,
    focus: &gpui::FocusHandle,
) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    let action = action.clone();
    let focus = focus.clone();
    move |_, cx| Tooltip::for_action_in(label, &action, &focus, cx)
}

#[cfg(test)]
mod tests {
    use super::preserve_case;

    #[test]
    fn replacements_take_the_found_case() {
        // IntelliJ's PreserveCaseUtilTest.testReplaceWithCaseRespect
        for (replacement, found, expected) in [
            ("foo", "Bar", "Foo"),
            ("foo", "bar", "foo"),
            ("foo", "BAR", "FOO"),
            ("Foo", "Bar", "Foo"),
            ("Foo", "bar", "foo"),
            ("FOO", "Bar", "Foo"),
            ("FOO", "bar", "foo"),
            ("FOO", "BAR", "FOO"),
            ("fooBar", "Bar", "FooBar"),
            ("fooBar", "bar", "fooBar"),
            ("DEF1", "abc1", "def1"),
            ("DEF1", "Abc1", "Def1"),
            ("abc", "A1", "ABC"),
            ("Report", "display Preferences", "report"),
            ("Report", "Display preferences", "Report"),
            ("MyTest", "USERCODE", "MYTEST"),
            ("MyTest", "UserCode", "MyTest"),
            ("MyTest", "userCode", "myTest"),
        ] {
            assert_eq!(
                preserve_case(replacement, found),
                expected,
                "{replacement} for {found}"
            );
        }
    }
}
