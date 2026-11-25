//! IntelliJ's Documentation tool window (`DocumentationToolWindowManager`), on the right stripe
//! once a documentation goes there: Open in Documentation Tool Window moves the popup's into it,
//! Quick Documentation (F1) shows there while it shows, or always with Show Documentation Popup
//! First off; with Auto-Update from Source it follows the caret. Its title actions are Back,
//! Forward and Jump to Source, its ⋮ the popup's toggles and Auto-Update from Source.

use std::sync::Arc;
use std::time::Duration;

use editor::actions::{GoToDefinition, Hover};
use editor::{Editor, EditorEvent, EditorSettings, SelectionEffects, hover_popover};
use gpui::{
    Action, AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, EventEmitter,
    FocusHandle, Focusable, Global, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, Render, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    Task, TaskExt as _, WeakEntity, Window, div, px,
};
use markdown::{Markdown, MarkdownElement};
use multi_buffer::Anchor;
use settings::Settings as _;
use spork_workbench::Tokens;
use util::ResultExt as _;
use workspace::dock::{DockPosition, GearItem, Panel, PanelEvent, PanelHandle};
use workspace::{Workspace, frame};

use crate::popups::write_setting;

pub mod documentation_panel {
    gpui::actions!(
        documentation_panel,
        [
            /// Opens the Documentation tool window and focuses it, or gives the focus back to the
            /// editor.
            ToggleFocus
        ]
    );
}

gpui::actions!(
    spork_editor,
    [
        /// Shows the documentation of the symbol at the caret: in its popup, or in the
        /// Documentation tool window while it shows or with Show Documentation Popup First off
        /// (IntelliJ's Quick Documentation, F1).
        QuickDocumentation,
        /// Moves the documentation popup's content into the Documentation tool window.
        OpenInDocumentationToolWindow,
        /// Whether Quick Documentation shows its popup before the tool window.
        ToggleShowDocumentationPopupFirst,
        /// Whether the documentation shows when the pointer rests on a symbol.
        ToggleShowDocumentationOnMouseMove,
        /// Whether the documentation shows beside the completions by itself.
        ToggleShowDocumentationDuringCompletion,
        /// Whether the Documentation tool window follows the caret.
        ToggleAutoUpdateFromSource,
    ]
);

/// IntelliJ's `documentation.skip.popup` and `documentation.auto.update`, kept as it keeps them.
const SKIP_POPUP_KEY: &str = "spork_documentation_skip_popup";
const AUTO_UPDATE_KEY: &str = "spork_documentation_auto_update";
/// How long the caret rests before the tool window follows it.
const FOLLOW_DELAY: Duration = Duration::from_millis(300);
/// The documentation's insets in the tool window.
const CONTENT_PADDING: f32 = 12.;

struct DocumentationPrefs {
    skip_popup: bool,
    auto_update: bool,
}

impl Global for DocumentationPrefs {}

fn read_flag(key: &str, default: bool, cx: &App) -> bool {
    db::kvp::KeyValueStore::global(cx)
        .read_kvp(key)
        .log_err()
        .flatten()
        .map_or(default, |value| value == "1")
}

fn write_flag(key: &'static str, on: bool, cx: &mut App) {
    let store = db::kvp::KeyValueStore::global(cx);
    let value = if on { "1" } else { "0" };
    cx.background_spawn(async move { store.write_kvp(key.into(), value.into()).await })
        .detach_and_log_err(cx);
}

fn prefs(cx: &App) -> (bool, bool) {
    cx.try_global::<DocumentationPrefs>()
        .map_or((false, true), |prefs| (prefs.skip_popup, prefs.auto_update))
}

pub(crate) fn init(cx: &mut App) {
    cx.set_global(DocumentationPrefs {
        skip_popup: read_flag(SKIP_POPUP_KEY, false, cx),
        auto_update: read_flag(AUTO_UPDATE_KEY, true, cx),
    });
    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        let Some(window) = window else { return };
        workspace.register_action(
            |workspace, _: &documentation_panel::ToggleFocus, window, cx| {
                workspace.toggle_panel_focus::<DocumentationPanel>(window, cx);
            },
        );
        workspace.register_action(|_, _: &ToggleShowDocumentationPopupFirst, _, cx| {
            let prefs = cx.global_mut::<DocumentationPrefs>();
            prefs.skip_popup = !prefs.skip_popup;
            let skip = prefs.skip_popup;
            write_flag(SKIP_POPUP_KEY, skip, cx);
        });
        workspace.register_action(|_, _: &ToggleAutoUpdateFromSource, _, cx| {
            let prefs = cx.global_mut::<DocumentationPrefs>();
            prefs.auto_update = !prefs.auto_update;
            let on = prefs.auto_update;
            write_flag(AUTO_UPDATE_KEY, on, cx);
        });
        workspace.register_action(|_, _: &ToggleShowDocumentationOnMouseMove, _, cx| {
            toggle_on_mouse_move(cx)
        });
        workspace.register_action(|_, _: &ToggleShowDocumentationDuringCompletion, _, cx| {
            toggle_during_completion(cx)
        });
        let handle = cx.entity().downgrade();
        let panel = cx.new(|cx| DocumentationPanel::new(handle, cx));
        workspace.add_panel(panel, window, cx);
    })
    .detach();
}

pub(crate) fn toggle_on_mouse_move(cx: &mut App) {
    let on = EditorSettings::get_global(cx).hover_popover_enabled;
    write_setting(
        move |settings, _| settings.editor.hover_popover_enabled = Some(!on),
        cx,
    )
}

pub(crate) fn toggle_during_completion(cx: &mut App) {
    let on = language::language_settings::AllLanguageSettings::get_global(cx)
        .defaults
        .show_completion_documentation;
    write_setting(
        move |settings, _| {
            settings
                .project
                .all_languages
                .defaults
                .show_completion_documentation = Some(!on)
        },
        cx,
    )
}

pub(crate) fn skip_popup(cx: &App) -> bool {
    prefs(cx).0
}

/// Quick Documentation (F1): beside the completions as Zed shows it there; in the tool window
/// while it shows or when its popup isn't shown first; in the popup otherwise.
pub(crate) fn quick_documentation(
    editor: &mut Editor,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    let panel = Workspace::for_window(window, cx)
        .and_then(|workspace| workspace.read(cx).panel::<DocumentationPanel>(cx));
    let in_window = !editor.has_visible_completions_menu()
        && panel
            .as_ref()
            .is_some_and(|panel| panel.read(cx).active || skip_popup(cx));
    match (in_window, panel) {
        (true, Some(panel)) => {
            let anchor = editor.selections.newest_anchor().head();
            let editor = cx.entity();
            window.defer(cx, move |window, cx| {
                panel.update(cx, |panel, cx| panel.look_up(&editor, anchor, window, cx));
                open_panel(window, cx);
            });
        }
        _ => hover_popover::hover(editor, &Hover, window, cx),
    }
}

/// Open in Documentation Tool Window: the popup's documentation moves there, the popup closing.
pub(crate) fn open_in_tool_window(editor: &WeakEntity<Editor>, window: &mut Window, cx: &mut App) {
    let Some(editor) = editor.upgrade() else {
        return;
    };
    let Some((markdown, anchor)) = editor.update(cx, |editor, cx| {
        let popover = editor.hover_state.info_popovers.first()?;
        let shown = (popover.parsed_content.clone()?, popover.anchor?);
        hover_popover::hide_hover(editor, cx);
        Some(shown)
    }) else {
        return;
    };
    let name = symbol_name(&editor, anchor, cx);
    let Some(panel) = Workspace::for_window(window, cx)
        .and_then(|workspace| workspace.read(cx).panel::<DocumentationPanel>(cx))
    else {
        return;
    };
    panel.update(cx, |panel, cx| {
        panel.shown = Some(Shown {
            name,
            markdown,
            editor: editor.downgrade(),
            anchor,
        });
        cx.notify();
    });
    open_panel(window, cx);
}

fn open_panel(window: &mut Window, cx: &mut App) {
    if let Some(workspace) = Workspace::for_window(window, cx) {
        workspace.update(cx, |workspace, cx| {
            workspace.open_panel::<DocumentationPanel>(window, cx)
        });
    }
}

/// The word at `anchor`, the documented symbol's name, for the tool window's tab.
fn symbol_name(editor: &Entity<Editor>, anchor: Anchor, cx: &App) -> SharedString {
    let snapshot = editor.read(cx).buffer().read(cx).snapshot(cx);
    let (range, _) = snapshot.surrounding_word(anchor, None);
    snapshot.text_for_range(range).collect::<String>().into()
}

/// Copy Quick Doc: the popup's documentation, as text, onto the clipboard.
pub(crate) fn copy_popup_doc(editor: &WeakEntity<Editor>, cx: &mut App) {
    let Some(markdown) = editor.upgrade().and_then(|editor| {
        editor
            .read(cx)
            .hover_state
            .info_popovers
            .first()?
            .parsed_content
            .clone()
    }) else {
        return;
    };
    let text = markdown.read(cx).source().to_string();
    cx.write_to_clipboard(ClipboardItem::new_string(text));
}

/// What the tool window documents.
struct Shown {
    name: SharedString,
    markdown: Entity<Markdown>,
    editor: WeakEntity<Editor>,
    anchor: Anchor,
}

pub struct DocumentationPanel {
    workspace: WeakEntity<Workspace>,
    focus: FocusHandle,
    shown: Option<Shown>,
    /// Whether its dock shows it.
    active: bool,
    _workspace: Subscription,
    /// The active editor, whose caret the tool window follows.
    _editor: Option<Subscription>,
    lookup: Task<()>,
}

impl DocumentationPanel {
    fn new(workspace: WeakEntity<Workspace>, cx: &mut Context<Self>) -> Self {
        let _workspace = match workspace.upgrade() {
            Some(workspace) => cx.subscribe(&workspace, |this, workspace, event, cx| {
                if let workspace::Event::ActiveItemChanged = event {
                    this.follow_active_editor(&workspace, cx);
                }
            }),
            None => Subscription::new(|| {}),
        };
        Self {
            workspace,
            focus: cx.focus_handle(),
            shown: None,
            active: false,
            _workspace,
            _editor: None,
            lookup: Task::ready(()),
        }
    }

    fn follow_active_editor(&mut self, workspace: &Entity<Workspace>, cx: &mut Context<Self>) {
        self._editor = workspace
            .read(cx)
            .active_item_as::<Editor>(cx)
            .map(|editor| {
                cx.subscribe(&editor, |this, editor, event, cx| {
                    if let EditorEvent::SelectionsChanged { local: true } = event {
                        this.caret_moved(editor, cx);
                    }
                })
            });
    }

    fn caret_moved(&mut self, editor: Entity<Editor>, cx: &mut Context<Self>) {
        if !self.active || !prefs(cx).1 {
            return;
        }
        let anchor = editor.read(cx).selections.newest_anchor().head();
        self.lookup = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FOLLOW_DELAY).await;
            this.update(cx, |this, cx| this.request(&editor, anchor, cx))
                .ok();
        });
    }

    /// Shows the documentation of the symbol at `anchor`, when the language servers have one.
    fn look_up(
        &mut self,
        editor: &Entity<Editor>,
        anchor: Anchor,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request(editor, anchor, cx);
    }

    fn request(&mut self, editor: &Entity<Editor>, anchor: Anchor, cx: &mut Context<Self>) {
        let Some((project, buffer, position)) = editor.update(cx, |editor, cx| {
            let project = editor.project()?.clone();
            let snapshot = editor.buffer().read(cx).snapshot(cx);
            let (text_anchor, _) = snapshot.anchor_to_buffer_anchor(anchor)?;
            let buffer = editor.buffer().read(cx).buffer(text_anchor.buffer_id)?;
            Some((project, buffer, text_anchor))
        }) else {
            return;
        };
        let name = symbol_name(editor, anchor, cx);
        let hover = project.update(cx, |project, cx| project.hover(&buffer, position, cx));
        let languages = project.read(cx).languages().clone();
        let editor = editor.downgrade();
        self.lookup = cx.spawn(async move |this, cx| {
            let Some(hovers) = hover.await else {
                return;
            };
            let blocks: Vec<_> = hovers
                .iter()
                .flat_map(|hover| hover.contents.iter().cloned())
                .collect();
            let language = hovers.iter().find_map(|hover| hover.language.clone());
            let text = hover_popover::combine_hover_blocks(&blocks);
            if text.trim().is_empty() {
                return;
            }
            this.update(cx, |this, cx| {
                let markdown = cx.new(|cx| {
                    Markdown::new(
                        text.into(),
                        Some(languages),
                        language.map(|language| language.name()),
                        cx,
                    )
                });
                this.shown = Some(Shown {
                    name,
                    markdown,
                    editor,
                    anchor,
                });
                cx.notify();
            })
            .ok();
        });
    }

    fn jump_to_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(shown) = &self.shown else {
            return;
        };
        let (editor, anchor) = (shown.editor.clone(), shown.anchor);
        let Some(editor) = editor.upgrade() else {
            return;
        };
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| {
                workspace.activate_item(&editor, true, true, window, cx)
            });
        }
        editor.update(cx, |editor, cx| {
            editor.change_selections(SelectionEffects::default(), window, cx, |selections| {
                selections.select_anchor_ranges([anchor..anchor])
            });
            editor
                .go_to_definition(
                    &GoToDefinition {
                        open_results_in: None,
                    },
                    window,
                    cx,
                )
                .detach_and_log_err(cx);
        });
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let panel: Arc<dyn PanelHandle> = Arc::new(cx.entity());
        let tab = self.shown.as_ref().map(|shown| {
            div()
                .id("documentation-tab")
                .role(Role::Tab)
                .aria_label(shown.name.clone())
                .aria_selected(true)
                .px(px(8.))
                .py(px(2.))
                .rounded(px(4.))
                .bg(tokens.tab_selected)
                .border_1()
                .border_color(tokens.tab_selected_border)
                .child(shown.name.clone())
        });
        let title = gpui_kit::component::h_flex()
            .gap(px(12.))
            .child(frame::tool_window_title_label("Documentation"))
            .children(tab)
            .into_any_element();
        let has_content = self.shown.is_some();
        // Back and Forward stay greyed: the documentation doesn't follow its links in place
        let actions = vec![
            spork_workbench::ActionButton::new("documentation-back", "general/left")
                .tooltip("Back")
                .disabled(true)
                .into_any_element(),
            spork_workbench::ActionButton::new("documentation-forward", "general/right")
                .tooltip("Forward")
                .disabled(true)
                .into_any_element(),
            spork_workbench::ActionButton::new("documentation-edit-source", "general/edit")
                .tooltip("Jump to Source")
                .disabled(!has_content)
                .on_click(cx.listener(|this, _, window, cx| this.jump_to_source(window, cx)))
                .into_any_element(),
        ];
        frame::tool_window_header(title, actions, panel, "Documentation".into(), cx)
            .into_any_element()
    }
}

impl Render for DocumentationPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let header = self.header(cx);
        let body = self.shown.as_ref().map(|shown| {
            let mut style = hover_popover::hover_markdown_style(window, cx);
            style.base_text_style.font_size =
                px(EditorSettings::get_global(cx).documentation_font_size as f32).into();
            div()
                .id("documentation-content")
                .role(Role::Document)
                .aria_label("Documentation")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(CONTENT_PADDING))
                .child(MarkdownElement::new(shown.markdown.clone(), style))
        });
        spork_vcs::spork_look(cx).size_full().child(
            gpui_kit::component::v_flex()
                .size_full()
                .bg(tokens.island)
                .track_focus(&self.focus)
                .key_context("DocumentationPanel")
                .child(header)
                .children(body),
        )
    }
}

impl Focusable for DocumentationPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EventEmitter<PanelEvent> for DocumentationPanel {}

impl Panel for DocumentationPanel {
    fn persistent_name() -> &'static str {
        "Documentation"
    }

    fn panel_key() -> &'static str {
        "DocumentationToolWindow"
    }

    fn position(&self, _: &Window, _: &App) -> DockPosition {
        DockPosition::Right
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        position != DockPosition::Bottom
    }

    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}

    fn default_size(&self, _: &Window, _: &App) -> Pixels {
        px(400.)
    }

    // IntelliJ's `shouldBeAvailable = false`: no stripe button until a documentation goes there
    fn icon(&self, _: &Window, _: &App) -> Option<ui::IconName> {
        self.shown.as_ref().map(|_| ui::IconName::Book)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some("Documentation")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(documentation_panel::ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        60
    }

    // unavailable until a documentation goes there: the tool window lists leave it out
    fn enabled(&self, _: &App) -> bool {
        self.shown.is_some()
    }

    fn set_active(&mut self, active: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.active = active;
        if active {
            // the dock shows it while the workspace updates
            cx.defer_in(window, |this, _, cx| {
                if let Some(workspace) = this.workspace.upgrade() {
                    this.follow_active_editor(&workspace, cx);
                }
            });
        }
    }

    fn gear_items(&self, _: &Window, cx: &App) -> Vec<GearItem> {
        let (skip_popup, auto_update) = prefs(cx);
        let on_mouse_move = EditorSettings::get_global(cx).hover_popover_enabled;
        let during_completion = language::language_settings::AllLanguageSettings::get_global(cx)
            .defaults
            .show_completion_documentation;
        let item = |label: &'static str, checked: bool, action: Box<dyn Action>| GearItem {
            label: label.into(),
            checked: Some(checked),
            action,
        };
        vec![
            item(
                "Show Documentation Popup First",
                !skip_popup,
                Box::new(ToggleShowDocumentationPopupFirst),
            ),
            item(
                "Show on Mouse Move",
                on_mouse_move,
                Box::new(ToggleShowDocumentationOnMouseMove),
            ),
            item(
                "Show Automatically During Completion",
                during_completion,
                Box::new(ToggleShowDocumentationDuringCompletion),
            ),
            item(
                "Auto-Update from Source",
                auto_update,
                Box::new(ToggleAutoUpdateFromSource),
            ),
        ]
    }
}

/// The tool window's state for the end-to-end tests: the symbol shown and its documentation.
pub fn e2e_state(workspace: &Workspace, cx: &App) -> serde_json::Value {
    let Some(panel) = workspace.panel::<DocumentationPanel>(cx) else {
        return serde_json::Value::Null;
    };
    let panel = panel.read(cx);
    let (skip_popup, auto_update) = prefs(cx);
    serde_json::json!({
        "shown": panel.shown.as_ref().map(|shown| shown.name.to_string()),
        "text": panel.shown.as_ref().map(|shown| shown.markdown.read(cx).source().to_string()),
        "popup_first": !skip_popup,
        "auto_update": auto_update,
    })
}
