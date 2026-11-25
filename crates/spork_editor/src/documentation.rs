//! IntelliJ's documentation popup around the language server's hover (`DocumentationPopupUI`,
//! `DocumentationUI`): under the content, the external documentation's link and the definition's
//! location (`locationLabel`), with the toolbar in the bottom right corner: Jump to Source and
//! ⋮, whose menu turns the popup's behaviours on and off, adjusts its font size and navigates.

use std::collections::HashMap;

use editor::actions::{GoToDefinition, OpenDocs};
use editor::look::DocumentationPopup;
use editor::{Editor, EditorSettings, SelectionEffects};
use gpui::{
    AnyElement, App, AsyncApp, EntityId, Global, InteractiveElement as _, IntoElement as _,
    ParentElement as _, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Task,
    TaskExt as _, WeakEntity, Window, div, px,
};
use language::LanguageServerName;
use multi_buffer::Anchor;
use project::lsp_store::lsp_ext_command::OpenDocs as LspOpenDocs;
use project::{LanguageServerToQuery, Project};
use settings::Settings as _;
use spork_workbench::tokens::metrics;
use spork_workbench::{Typography as _, popup};
use text::ToPointUtf16 as _;
use theme::ActiveTheme as _;
use util::ResultExt as _;

use crate::documentation_window::{self, ToggleShowDocumentationPopupFirst};
use crate::popups::{
    MenuEntry, MenuItem, MenuKey, icon_button, keep_popup_open, menu_open, more_button, set_menu,
    shortcut, tokens, under, write_setting,
};

/// The popup's tallest before it scrolls (`DocumentationHtmlUtil.docPopupMaxHeight`).
pub(crate) const MAX_HEIGHT: f32 = 500.;
/// The location's insets (`DocumentationUI.locationLabel`: `spaceBeforeParagraph` above,
/// `LineTooltipRenderer.CONTENT_PADDING` left, `contentOuterPadding` + 2 under), less the popup's
/// own 6 px at the sides; the toolbar 2 px in from the popup's corner with 11 under it.
const FOOTER_TOP: f32 = 4.;
const FOOTER_LEFT: f32 = 14.;
const FOOTER_BOTTOM: f32 = 13.;
/// `iconTextGap` between the location's icon and text.
const LOCATION_GAP: f32 = 6.;
/// IntelliJ's documentation font sizes (`FontSize`), the steps of Adjust Font Size's slider.
const FONT_SIZES: [u32; 7] = [10, 12, 13, 14, 16, 18, 24];
/// The slider's track and its knob.
const SLIDER_WIDTH: f32 = 140.;
const SLIDER_KNOB: f32 = 12.;

/// What a popup's footer shows of its symbol, once the language servers answered.
#[derive(Clone, Default)]
struct Footer {
    /// The definition's module, or its library's file, with the icon IntelliJ gives it.
    location: Option<(SharedString, &'static str)>,
    /// IntelliJ's link to the external documentation ("`name` on docs.rs") and its address.
    link: Option<(SharedString, String)>,
}

/// The footers of the documentation popup showing: looked up once it shows.
#[derive(Default)]
struct Footers {
    found: HashMap<EntityId, Footer>,
    looking: HashMap<EntityId, Task<()>>,
}

impl Global for Footers {}

pub(crate) fn parts(
    popup: DocumentationPopup,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let id = popup.id;
    let footers = cx.default_global::<Footers>();
    let footer = footers.found.get(&id).cloned();
    if footer.is_none() && !footers.looking.contains_key(&id) {
        // one popup shows at a time: the last one's answers go
        footers.found.clear();
        footers.looking.clear();
        if let Some(anchor) = popup.anchor {
            let editor = popup.editor.clone();
            let task = cx.spawn(async move |cx| {
                let footer = look_up(editor.clone(), anchor, cx)
                    .await
                    .log_err()
                    .unwrap_or_default();
                cx.update(|cx| {
                    cx.default_global::<Footers>().found.insert(id, footer);
                    editor.update(cx, |_, cx| cx.notify()).ok();
                });
            });
            cx.default_global::<Footers>().looking.insert(id, task);
        }
    }
    Some(footer_row(&popup, footer.unwrap_or_default(), window, cx))
}

/// Asks the language servers where the symbol at `anchor` is defined and where its external
/// documentation is (rust-analyzer's `experimental/externalDocs`).
async fn look_up(
    editor: WeakEntity<Editor>,
    anchor: Anchor,
    cx: &mut AsyncApp,
) -> anyhow::Result<Footer> {
    let (project, definitions, docs, name) = editor
        .update(cx, |editor, cx| {
            let project = editor.project()?.clone();
            let snapshot = editor.buffer().read(cx).snapshot(cx);
            let (text_anchor, _) = snapshot.anchor_to_buffer_anchor(anchor)?;
            let buffer = editor.buffer().read(cx).buffer(text_anchor.buffer_id)?;
            let buffer_snapshot = buffer.read(cx).snapshot();
            let position = text_anchor.to_point_utf16(&buffer_snapshot);
            let (word, _) = buffer_snapshot.surrounding_word(text_anchor, None);
            let name: String = buffer_snapshot.text_for_range(word).collect();
            let definitions =
                project.update(cx, |project, cx| project.definitions(&buffer, position, cx));
            let rust_analyzer = LanguageServerName::new_static("rust-analyzer");
            let server_id = buffer.update(cx, |buffer, cx| {
                project
                    .read(cx)
                    .language_server_id_for_name(buffer, &rust_analyzer, cx)
            });
            let docs = server_id.map(|server_id| {
                project.update(cx, |project, cx| {
                    project.request_lsp(
                        buffer.clone(),
                        LanguageServerToQuery::Other(server_id),
                        LspOpenDocs {
                            position,
                            server_id,
                        },
                        cx,
                    )
                })
            });
            Some((project, definitions, docs, name))
        })?
        .ok_or_else(|| anyhow::anyhow!("no symbol for the documentation popup"))?;
    let target = definitions
        .await?
        .and_then(|links| links.into_iter().next())
        .map(|link| link.target.buffer);
    let location = target.and_then(|buffer| cx.update(|cx| location_of(&project, &buffer, cx)));
    let link = match docs {
        Some(docs) => docs.await.log_err().and_then(|urls| {
            let url = urls.web?;
            let host = url::Url::parse(&url).ok()?.host_str()?.to_owned();
            Some((format!("`{name}` on {host}").into(), url))
        }),
        None => None,
    };
    Ok(Footer { location, link })
}

/// IntelliJ's location of a definition: its module (the project folder) for the project's own
/// code, its file for a library's.
fn location_of(
    project: &gpui::Entity<Project>,
    buffer: &gpui::Entity<language::Buffer>,
    cx: &App,
) -> Option<(SharedString, &'static str)> {
    let file = buffer.read(cx).file()?;
    let worktree = project.read(cx).worktree_for_id(file.worktree_id(cx), cx)?;
    let worktree = worktree.read(cx);
    Some(match worktree.is_visible() {
        true => (worktree.root_name_str().to_owned().into(), "nodes/module"),
        false => (file.file_name(cx).to_owned().into(), "nodes/library"),
    })
}

fn footer_row(
    popup: &DocumentationPopup,
    footer: Footer,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let dark = !cx.theme().appearance().is_light();
    let tokens = tokens(cx);
    let link = footer.link.clone();
    let link_row = footer.link.map(|(text, url)| {
        div()
            .id("documentation-link")
            .role(Role::Link)
            .aria_label(text.clone())
            .flex()
            .items_center()
            .gap(px(2.))
            .pl(px(FOOTER_LEFT - 6.))
            .pt(px(FOOTER_TOP))
            .text_color(tokens.text_link)
            .cursor_pointer()
            .child(text)
            .child(spork_workbench::image_in("ide/externalLink", dark).size(px(12.)))
            .on_click(move |_, _, cx| cx.open_url(&url))
    });
    let location = footer.location.map(|(text, icon)| {
        div()
            .id("documentation-location")
            .role(Role::Label)
            .aria_label(text.clone())
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(px(LOCATION_GAP))
            .text_color(tokens.text_secondary)
            .child(spork_workbench::image_in(icon, dark).size(px(metrics::TOOLBAR_ICON)))
            .child(div().truncate().child(text))
    });
    let toolbar = toolbar(popup, link.map(|(_, url)| url), window, cx);
    div()
        .font(
            theme_settings::ThemeSettings::get_global(cx)
                .ui_font
                .clone(),
        )
        .text_ui()
        .children(link_row)
        .child(
            div()
                .flex()
                .items_center()
                .pl(px(FOOTER_LEFT))
                .pt(px(FOOTER_TOP))
                .pb(px(FOOTER_BOTTOM))
                .children(location)
                .child(div().flex_1())
                .child(toolbar),
        )
        .into_any_element()
}

/// The popup's corner toolbar: Jump to Source and ⋮ (`DocumentationPopupUI`'s `corner`).
fn toolbar(
    popup: &DocumentationPopup,
    external: Option<String>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let id = popup.id;
    let editor = popup.editor.clone();
    let anchor = popup.anchor;
    let external_keys = shortcut(&OpenDocs, window);
    let font_size_open = menu_open(MenuKey::FontSize(id), cx);
    let font_size = font_size_open.then(|| under(font_size_popup(&editor, cx)));
    div()
        .relative()
        .flex()
        .gap(px(2.))
        .child(
            icon_button(
                "documentation-source",
                "general/edit",
                "Jump to Source",
                false,
                cx,
            )
            .on_click({
                let editor = editor.clone();
                move |_, window, cx| {
                    cx.stop_propagation();
                    jump_to_source(&editor, anchor, window, cx)
                }
            }),
        )
        .child(more_button(
            "documentation-more",
            MenuKey::Documentation(id),
            editor.clone(),
            || documentation_menu(id, editor, external, external_keys, cx),
            cx,
        ))
        .children(font_size)
        .into_any_element()
}

/// Jump to Source: the definition of the documented symbol, the popup closing.
fn jump_to_source(
    editor: &WeakEntity<Editor>,
    anchor: Option<Anchor>,
    window: &mut Window,
    cx: &mut App,
) {
    editor
        .update(cx, |editor, cx| {
            if let Some(anchor) = anchor {
                editor.change_selections(SelectionEffects::default(), window, cx, |selections| {
                    selections.select_anchor_ranges([anchor..anchor])
                });
            }
            editor
                .go_to_definition(
                    &GoToDefinition {
                        open_results_in: None,
                    },
                    window,
                    cx,
                )
                .detach_and_log_err(cx);
        })
        .ok();
}

/// The ⋮'s menu, IntelliJ's in its order (`DocumentationPopupUI`'s gear: its secondary actions,
/// then `Documentation.PrimaryGroup`): Restore Size stays greyed (the popup keeps its size, as
/// IntelliJ's before it is resized), so do Back and Forward (the popup doesn't follow links within
/// itself).
fn documentation_menu(
    id: EntityId,
    editor: WeakEntity<Editor>,
    external: Option<String>,
    external_keys: Option<SharedString>,
    cx: &App,
) -> Vec<MenuEntry> {
    let on_mouse_move = EditorSettings::get_global(cx).hover_popover_enabled;
    let during_completion = language::language_settings::AllLanguageSettings::get_global(cx)
        .defaults
        .show_completion_documentation;
    let popup_first = !documentation_window::skip_popup(cx);
    vec![
        MenuEntry::Item(MenuItem {
            icon: Some("toolwindows/documentation"),
            ..MenuItem::new("Open in Documentation Tool Window", {
                let editor = editor.clone();
                move |window, cx| documentation_window::open_in_tool_window(&editor, window, cx)
            })
        }),
        MenuEntry::Item(MenuItem {
            checked: Some(popup_first),
            ..MenuItem::new("Show Documentation Popup First", |window, cx| {
                window.dispatch_action(Box::new(ToggleShowDocumentationPopupFirst), cx)
            })
        }),
        MenuEntry::Item(MenuItem {
            checked: Some(on_mouse_move),
            ..MenuItem::new("Show on Mouse Move", |_, cx| {
                documentation_window::toggle_on_mouse_move(cx)
            })
        }),
        MenuEntry::Item(MenuItem {
            checked: Some(during_completion),
            ..MenuItem::new("Show Automatically During Completion", |_, cx| {
                documentation_window::toggle_during_completion(cx)
            })
        }),
        MenuEntry::Item(MenuItem::new("Adjust Font Size…", {
            let editor = editor.clone();
            move |_, cx| set_menu(Some(MenuKey::FontSize(id)), &editor, cx)
        })),
        MenuEntry::Item(MenuItem {
            enabled: false,
            ..MenuItem::new("Restore Size", |_, _| {})
        }),
        MenuEntry::Separator,
        MenuEntry::Item(MenuItem {
            icon: Some("general/copy"),
            // the menu is drawn with the editor: its popup's content is read once chosen
            ..MenuItem::new("Copy Quick Doc", move |_, cx| {
                documentation_window::copy_popup_doc(&editor, cx)
            })
        }),
        MenuEntry::Item(MenuItem {
            icon: Some("general/left"),
            enabled: false,
            ..MenuItem::new("Back", |_, _| {})
        }),
        MenuEntry::Item(MenuItem {
            icon: Some("general/right"),
            enabled: false,
            ..MenuItem::new("Forward", |_, _| {})
        }),
        MenuEntry::Item(MenuItem {
            icon: Some("toolwindows/web"),
            shortcut: external_keys,
            enabled: external.is_some(),
            ..MenuItem::new("View External Documentation", move |_, cx| {
                if let Some(url) = &external {
                    cx.open_url(url);
                }
            })
        }),
    ]
}

/// Adjust Font Size's popup: "Font size:" and a slider over IntelliJ's seven sizes
/// (`FontSizePopup`), each step applied as it's chosen.
fn font_size_popup(editor: &WeakEntity<Editor>, cx: &App) -> AnyElement {
    let tokens = tokens(cx);
    let current = EditorSettings::get_global(cx).documentation_font_size;
    let chosen = FONT_SIZES
        .iter()
        .position(|size| *size == current)
        .unwrap_or(2);
    let step = (SLIDER_WIDTH - SLIDER_KNOB) / (FONT_SIZES.len() - 1) as f32;
    let knob_x = step * chosen as f32;
    let ticks = FONT_SIZES.iter().enumerate().map(|(ix, size)| {
        let x = step * ix as f32 + SLIDER_KNOB / 2.;
        let size = *size;
        div()
            .id(("font-size", ix))
            .role(Role::Button)
            .aria_label(SharedString::from(size.to_string()))
            .absolute()
            .left(px(x - step / 2.))
            .w(px(step))
            .h_full()
            .cursor_pointer()
            .child(
                div()
                    .absolute()
                    .left(px(step / 2.))
                    .bottom_0()
                    .w(px(1.))
                    .h(px(4.))
                    .bg(tokens.control_border),
            )
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                write_setting(
                    move |settings, _| settings.editor.documentation_font_size = Some(size),
                    cx,
                )
            })
    });
    let editor = editor.clone();
    keep_popup_open(popup::surface(cx))
        .id("documentation-font-size")
        .role(Role::Dialog)
        .aria_label("Font size:")
        .occlude()
        .flex_row()
        .items_center()
        .gap(px(6.))
        .px(px(8.))
        .py(px(6.))
        .on_mouse_down_out(move |_, _, cx| set_menu(None, &editor, cx))
        .child("Font size:")
        .child(
            div()
                .id("font-size-slider")
                .role(Role::Slider)
                .aria_label("Font size")
                .aria_numeric_value(current as f64)
                .relative()
                .w(px(SLIDER_WIDTH))
                .h(px(20.))
                // the track, filled up to the knob (`UIUtil.setSliderIsFilled`)
                .child(
                    div()
                        .absolute()
                        .left(px(SLIDER_KNOB / 2.))
                        .top(px(7.))
                        .w(px(SLIDER_WIDTH - SLIDER_KNOB))
                        .h(px(2.))
                        .rounded(px(1.))
                        .bg(tokens.control_border),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(SLIDER_KNOB / 2.))
                        .top(px(7.))
                        .w(px(knob_x))
                        .h(px(2.))
                        .rounded(px(1.))
                        .bg(tokens.accent),
                )
                .children(ticks)
                .child(
                    div()
                        .absolute()
                        .left(px(knob_x))
                        .top(px(2.))
                        .size(px(SLIDER_KNOB))
                        .rounded_full()
                        .border_1()
                        .border_color(tokens.accent)
                        .bg(tokens.control_bg),
                ),
        )
        .into_any_element()
}
