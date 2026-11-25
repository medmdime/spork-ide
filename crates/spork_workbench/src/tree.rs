//! IntelliJ's tree: 24 px rows, the chevron left of the icon, children indented under it. Its
//! builder follows gpui-kit's `SidebarMenuItem`, so a sidebar menu turns into a tree as it is.

use std::rc::Rc;

use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu};
use gpui_kit::component::{ActiveTheme as _, Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::icon;

const ROW: Rems = rems(1.625);
const INDENT: f32 = 18.;

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type Menu = Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;
type Suffix = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

enum TreeIcon {
    /// Drawn in the text color.
    Mono(Box<Icon>),
    /// An IntelliJ icon in its own colors (`image`).
    Colored(&'static str),
}

/// A tree, its items keyed under `id` so each remembers whether it is open.
#[derive(IntoElement)]
pub struct Tree {
    id: SharedString,
    items: Vec<TreeItem>,
}

impl Tree {
    pub fn new(id: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            items: Vec::new(),
        }
    }

    pub fn children(mut self, items: impl IntoIterator<Item = TreeItem>) -> Self {
        self.items.extend(items);
        self
    }
}

impl RenderOnce for Tree {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = self.id;
        v_flex().w_full().children(
            self.items
                .into_iter()
                .enumerate()
                .map(|(ix, item)| item.render_at(format!("{id}-{ix}").into(), 0, window, cx)),
        )
    }
}

/// A row of a `Tree`, with its own rows under it once opened.
pub struct TreeItem {
    label: SharedString,
    icon: Option<TreeIcon>,
    active: bool,
    default_open: bool,
    click_to_toggle: bool,
    handler: Option<Handler>,
    menu: Option<Menu>,
    suffix: Option<Suffix>,
    hint: Option<SharedString>,
    children: Vec<TreeItem>,
}

impl FluentBuilder for TreeItem {}

impl TreeItem {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            active: false,
            default_open: false,
            click_to_toggle: false,
            handler: None,
            menu: None,
            suffix: None,
            hint: None,
            children: Vec::new(),
        }
    }

    /// An icon drawn in the text color.
    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(TreeIcon::Mono(Box::new(icon.into())));
        self
    }

    /// An IntelliJ icon in its own colors, by name (`"nodes/folder"`).
    pub fn colored_icon(mut self, name: &'static str) -> Self {
        self.icon = Some(TreeIcon::Colored(name));
        self
    }

    /// Shown selected.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Open before it is first toggled.
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// A click opens or closes it, not just a double click or its chevron.
    pub fn click_to_toggle(mut self, toggle: bool) -> Self {
        self.click_to_toggle = toggle;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.handler = Some(Rc::new(handler));
        self
    }

    pub fn context_menu(
        mut self,
        menu: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> Self {
        self.menu = Some(Rc::new(menu));
        self
    }

    /// Shown at the row's end, in gray: counts, states.
    pub fn suffix<E: IntoElement>(
        mut self,
        build: impl Fn(&mut Window, &mut App) -> E + 'static,
    ) -> Self {
        self.suffix = Some(Rc::new(move |window, cx| {
            build(window, cx).into_any_element()
        }));
        self
    }

    /// Gray text right after the label, as IntelliJ's trees write a node's details
    /// (`SimpleTextAttributes.GRAYED_ATTRIBUTES`): a Maven plugin's coordinates, a scope.
    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = TreeItem>) -> Self {
        self.children = children.into_iter().collect();
        self
    }

    fn render_at(
        self,
        id: SharedString,
        depth: usize,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let parent = !self.children.is_empty();
        let open = parent.then(|| window.use_keyed_state(id.clone(), cx, |_, _| self.default_open));
        let is_open = open.as_ref().is_some_and(|o| *o.read(cx));
        let theme = cx.theme();
        let (muted, hover, selected) =
            (theme.muted_foreground, theme.list_hover, theme.list_active);
        let toggle = move |open: &Option<Entity<bool>>, cx: &mut App| {
            if let Some(open) = open {
                open.update(cx, |o, cx| {
                    *o = !*o;
                    cx.notify();
                })
            }
        };
        let chevron = open.clone().map(|state| {
            div()
                .id("chevron")
                .flex_none()
                .text_color(muted)
                .child(
                    icon(if is_open {
                        "general/chevronDown"
                    } else {
                        "general/chevronRight"
                    })
                    .size(px(16.)),
                )
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    toggle(&Some(state.clone()), cx);
                })
        });
        let row_icon = self.icon.map(|i| match i {
            TreeIcon::Mono(icon) => icon.size(px(16.)).flex_none().into_any_element(),
            TreeIcon::Colored(name) => crate::image(name, cx).into_any_element(),
        });
        let (handler, click_to_toggle, state) = (self.handler, self.click_to_toggle, open);
        let row = h_flex()
            .id("row")
            .role(Role::TreeItem)
            .aria_label(self.label.clone())
            .aria_selected(self.active)
            .when(parent, |r| r.aria_expanded(is_open))
            .h(ROW)
            .mx_1()
            .pl(px(6. + INDENT * depth as f32))
            .pr_2()
            .gap_1()
            .rounded(px(4.))
            .when(self.active, |r| r.bg(selected))
            .when(!self.active, |r| r.hover(move |r| r.bg(hover)))
            .child(match chevron {
                Some(chevron) => chevron.into_any_element(),
                None => div().w(px(16.)).flex_none().into_any_element(),
            })
            .children(row_icon)
            .child(match self.hint {
                None => div()
                    .flex_1()
                    .min_w_0()
                    .pl_0p5()
                    .truncate()
                    .child(self.label)
                    .into_any_element(),
                Some(hint) => h_flex()
                    .flex_1()
                    .min_w_0()
                    .pl_0p5()
                    .gap_1()
                    .child(div().flex_none().child(self.label))
                    .child(
                        div()
                            .id("hint")
                            .role(Role::Label)
                            .aria_label(hint.clone())
                            .min_w_0()
                            .truncate()
                            .text_color(muted)
                            .child(hint),
                    )
                    .into_any_element(),
            })
            .when_some(self.suffix, |r, suffix| {
                r.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(suffix(window, cx)),
                )
            })
            .on_click(move |event, window, cx| {
                if click_to_toggle || event.click_count() == 2 {
                    toggle(&state, cx);
                }
                if let Some(handler) = &handler {
                    handler(event, window, cx);
                }
            });
        let row = match self.menu {
            Some(menu) => row
                .context_menu(move |m, window, cx| menu(m, window, cx))
                .into_any_element(),
            None => row.into_any_element(),
        };
        v_flex()
            .id(ElementId::Name(id.clone()))
            .w_full()
            .child(row)
            .when(is_open, |this| {
                this.children(self.children.into_iter().enumerate().map(|(ix, item)| {
                    item.render_at(format!("{id}-{ix}").into(), depth + 1, window, cx)
                }))
            })
            .into_any_element()
    }
}
