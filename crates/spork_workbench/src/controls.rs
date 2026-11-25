//! IntelliJ's controls, built from [`crate::tokens`] alone so they look and behave the same
//! everywhere: buttons, toolbar icon buttons, the main toolbar's dropdown widgets, combo boxes,
//! text fields, checkboxes and links. Those that open a menu work with gpui-component's
//! `DropdownMenu`.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Selectable, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens::{Tokens, Typography as _, metrics};
use crate::{icon, image};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A button: outlined, or the dialog's default (blue). At least 72×28, 13 px text.
#[derive(IntoElement)]
pub struct Button {
    base: Stateful<Div>,
    style: StyleRefinement,
    label: SharedString,
    children: Vec<AnyElement>,
    primary: bool,
    disabled: bool,
    selected: bool,
    shrink: bool,
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            base: div().id(id),
            style: StyleRefinement::default(),
            label: label.into(),
            children: Vec::new(),
            primary: false,
            disabled: false,
            selected: false,
            shrink: false,
            tooltip: None,
            on_click: None,
        }
    }

    /// The dialog's default button: blue, white text.
    pub fn primary(mut self, primary: bool) -> Self {
        self.primary = primary;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// May get narrower than 72 px when room is short, its label cut with an ellipsis.
    pub fn shrink(mut self) -> Self {
        self.shrink = true;
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let (fill, border, text) = match (self.disabled, self.primary) {
            (true, _) => (None, tokens.control_border_disabled, tokens.text_disabled),
            (false, true) => (Some(tokens.accent), tokens.accent, white()),
            (false, false) => (None, tokens.control_border, tokens.text),
        };
        let mut button = self
            .base
            .role(Role::Button)
            .aria_label(self.label.clone())
            .h(px(metrics::CONTROL_HEIGHT))
            .px(px(metrics::BUTTON_PADDING_X))
            .flex()
            .items_center()
            .justify_center()
            .gap_1()
            .rounded(px(metrics::RADIUS))
            .border_1()
            .border_color(border)
            .text_ui()
            .text_color(text)
            .map(|b| match self.shrink {
                true => b.min_w_0().flex_shrink(1.).overflow_hidden(),
                false => b.min_w(px(metrics::BUTTON_MIN_WIDTH)).flex_none(),
            })
            .when_some(fill, |b, fill| b.bg(fill))
            .when(self.selected && !self.primary, |b| b.bg(tokens.pressed))
            .when(!self.disabled, |b| b.cursor_pointer())
            .when_some(self.on_click.filter(|_| !self.disabled), |b, handler| {
                b.on_click(move |event, window, cx| handler(event, window, cx))
            })
            .when_some(self.tooltip, |b, tooltip| {
                b.tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            })
            .child(div().min_w_0().truncate().child(self.label))
            .children(self.children);
        button.style().refine(&self.style);
        A11yDisabled::new(button, self.disabled)
    }
}

/// IntelliJ's option button (JBOptionButton), a dialog's default one: its action, then behind
/// the arrow on its right the other choices, `menu`'s items (Push ▾ Force Push). `plain` for one
/// that isn't the default (the Clone dialog's Log In to gitlab.com…).
#[derive(IntoElement)]
pub struct OptionButton {
    id: ElementId,
    label: SharedString,
    disabled: bool,
    plain: bool,
    on_click: Option<ClickHandler>,
    menu: std::rc::Rc<MenuBuilder>,
}

/// What builds a menu's items, as `dropdown_menu` takes it.
type MenuBuilder = dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu;

impl OptionButton {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        menu: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            disabled: false,
            plain: false,
            on_click: None,
            menu: std::rc::Rc::new(menu),
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Not the dialog's default: a plain button's look.
    pub fn plain(mut self) -> Self {
        self.plain = true;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for OptionButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let (fill, text, border, divider) = match (self.disabled, self.plain) {
            (true, _) => (
                None,
                tokens.text_disabled,
                tokens.control_border_disabled,
                tokens.control_border_disabled,
            ),
            (false, true) => (
                None,
                tokens.text,
                tokens.control_border,
                tokens.control_border,
            ),
            (false, false) => (
                Some(tokens.accent),
                tokens.icon_over_accent,
                tokens.accent,
                tokens.control_border_over_accent,
            ),
        };
        let main = div()
            .id(self.id.clone())
            .role(Role::Button)
            .aria_label(self.label.clone())
            .h_full()
            .px(px(metrics::BUTTON_PADDING_X))
            .flex()
            .items_center()
            .when(!self.disabled, |b| b.cursor_pointer())
            .when_some(self.on_click.filter(|_| !self.disabled), |b, handler| {
                b.on_click(move |event, window, cx| handler(event, window, cx))
            })
            .child(self.label);
        let arrow = OptionArrow {
            base: div().id(ElementId::Name(format!("{:?}-options", self.id).into())),
            style: StyleRefinement::default(),
            disabled: self.disabled,
            plain: self.plain,
            selected: false,
        };
        let arrow = match self.disabled {
            true => arrow.into_any_element(),
            false => {
                let menu = self.menu.clone();
                arrow
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |m, window, cx| {
                        menu(m, window, cx)
                    })
                    .into_any_element()
            }
        };
        let button = h_flex()
            .h(px(metrics::CONTROL_HEIGHT))
            .min_w(px(metrics::BUTTON_MIN_WIDTH))
            .flex_none()
            .rounded(px(metrics::RADIUS))
            .border_1()
            .border_color(border)
            .when_some(fill, |b, fill| b.bg(fill))
            .text_ui()
            .text_color(text)
            .child(main)
            .child(div().w(px(1.)).h(px(16.)).bg(divider))
            .child(arrow);
        A11yDisabled::new(button, self.disabled)
    }
}

/// An option button's arrow, which opens its other choices.
#[derive(IntoElement)]
struct OptionArrow {
    base: Stateful<Div>,
    style: StyleRefinement,
    disabled: bool,
    plain: bool,
    selected: bool,
}

impl RenderOnce for OptionArrow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let mut arrow = self
            .base
            .role(Role::Button)
            .aria_label("Show drop-down menu")
            .h_full()
            .px(px(6.))
            .flex()
            .items_center()
            .when(!self.disabled, |b| b.cursor_pointer())
            .child(icon("general/chevronDown").size(px(16.)).text_color(
                match (self.disabled, self.plain) {
                    (true, _) => tokens.icon_disabled,
                    (false, true) => tokens.icon,
                    (false, false) => tokens.icon_over_accent,
                },
            ));
        arrow.style().refine(&self.style);
        A11yDisabled::new(arrow, self.disabled)
    }
}

/// A control's root element whose accessibility node says it is disabled while `disabled`:
/// GPUI has no `aria_disabled`, so a greyed control would reach assistive technology (and the
/// end-to-end tests) as one that takes input.
pub struct A11yDisabled<E> {
    inner: E,
    disabled: bool,
}

impl<E: Element> A11yDisabled<E> {
    pub fn new(inner: E, disabled: bool) -> Self {
        Self { inner, disabled }
    }
}

impl<E: Element> Element for A11yDisabled<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.inner.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.inner.source_location()
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.inner.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.inner
            .prepaint(id, inspector, bounds, layout, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner
            .paint(id, inspector, bounds, layout, prepaint, window, cx)
    }

    fn a11y_role(&self) -> Option<Role> {
        self.inner.a11y_role()
    }

    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        self.inner.write_a11y_info(node);
        if self.disabled {
            node.set_disabled();
        }
    }

    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Self::PrepaintState,
        builder: &mut A11ySubtreeBuilder,
    ) {
        self.inner.a11y_synthetic_children(prepaint, builder);
    }
}

/// The size of an icon button: in tool-window toolbars (22 px, 16 px icon) or in the main
/// toolbar and stripes (30 px, 20 px icon).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ActionSize {
    #[default]
    Toolbar,
    MainToolbar,
}

/// IntelliJ's toolbar icon button: the icon alone, a rounded fill on hover and while pressed,
/// the pressed fill while toggled on.
#[derive(IntoElement)]
pub struct ActionButton {
    base: Stateful<Div>,
    style: StyleRefinement,
    icon: SharedString,
    size: ActionSize,
    selected: bool,
    disabled: bool,
    popup_marker: bool,
    colored: bool,
    fill: Option<Hsla>,
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

impl ActionButton {
    /// `icon`: an IntelliJ icon name (`general/refresh`); the main toolbar size takes its
    /// `@20x20` variant.
    pub fn new(id: impl Into<ElementId>, icon: impl Into<SharedString>) -> Self {
        Self {
            // a button to accessibility, named by its tooltip
            base: div().id(id).role(Role::Button),
            style: StyleRefinement::default(),
            icon: icon.into(),
            size: ActionSize::default(),
            selected: false,
            disabled: false,
            popup_marker: false,
            colored: false,
            fill: None,
            tooltip: None,
            on_click: None,
        }
    }

    /// A colored button, its icon in `icon-over-accent`: the Merge/Rebase widget's
    /// (`ColoredActionButtonLook`, on `toolbar_run_bg` or `toolbar_stop_bg`).
    pub fn fill(mut self, fill: Hsla) -> Self {
        self.fill = Some(fill);
        self
    }

    /// The icon in its own colors, as the run widget draws Run and Debug (`AllIcons.Actions.Execute`
    /// and `StartDebugger`, green).
    pub fn colored(mut self) -> Self {
        self.colored = true;
        self
    }

    /// IntelliJ's small triangle in the bottom-right corner of a button that opens a menu.
    pub fn popup_marker(mut self) -> Self {
        self.popup_marker = true;
        self
    }

    pub fn size(mut self, size: ActionSize) -> Self {
        self.size = size;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Also its accessible name, without the shortcut written after it (`Show Diff (⌘D)`).
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        let tooltip = tooltip.into();
        self.base = self.base.aria_label(without_shortcut(&tooltip).to_owned());
        self.tooltip = Some(tooltip);
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for ActionButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let (side, icon_side, radius, name) = match self.size {
            ActionSize::Toolbar => (
                metrics::TOOLBAR_BUTTON,
                metrics::TOOLBAR_ICON,
                metrics::RADIUS,
                self.icon.to_string(),
            ),
            // an icon without a 20 px variant shows at 16 px, as IntelliJ draws it
            ActionSize::MainToolbar if !crate::icon::has_large(&self.icon) => (
                metrics::MAIN_TOOLBAR_BUTTON,
                metrics::TOOLBAR_ICON,
                metrics::RADIUS_LARGE,
                self.icon.to_string(),
            ),
            ActionSize::MainToolbar => (
                metrics::MAIN_TOOLBAR_BUTTON,
                metrics::MAIN_TOOLBAR_ICON,
                metrics::RADIUS_LARGE,
                format!("{}@20x20", self.icon),
            ),
        };
        // a colored button blends the hover and pressed fills over its own
        let (hover, pressed) = match self.fill {
            Some(fill) => (fill.blend(tokens.hover), fill.blend(tokens.pressed)),
            None => (tokens.hover, tokens.pressed),
        };
        let mut button = self
            .base
            .size(px(side))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(radius))
            .text_color(match (self.disabled, self.fill) {
                (true, _) => tokens.icon_disabled,
                (false, Some(_)) => tokens.icon_over_accent,
                (false, None) => tokens.icon,
            })
            .when_some(self.fill, |b, fill| b.bg(fill))
            .when(self.selected, |b| b.bg(pressed))
            .when(!self.disabled, |b| {
                b.cursor_pointer()
                    .hover(move |b| b.bg(hover))
                    .active(move |b| b.bg(pressed))
            })
            .when_some(self.on_click.filter(|_| !self.disabled), |b, handler| {
                b.on_click(move |event, window, cx| handler(event, window, cx))
            })
            .when_some(self.tooltip, |b, tooltip| {
                b.tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            })
            // greyed, it takes the disabled icon color as the others do
            .map(|b| match self.colored && !self.disabled {
                true => b.child(crate::image(&name, cx).size(px(icon_side))),
                false => b.child(icon(&name).size(px(icon_side))),
            })
            .when(self.popup_marker, |b| {
                let color = tokens.icon;
                b.relative().child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            // a 4 px right triangle, 2 px in from the corner
                            let corner = bounds.bottom_right() - point(px(2.), px(2.));
                            let mut path = PathBuilder::fill();
                            path.move_to(corner - point(px(0.), px(4.)));
                            path.line_to(corner);
                            path.line_to(corner - point(px(4.), px(0.)));
                            path.close();
                            if let Ok(path) = path.build() {
                                window.paint_path(path, color);
                            }
                        },
                    )
                    .absolute()
                    .size_full(),
                )
            });
        button.style().refine(&self.style);
        A11yDisabled::new(button, self.disabled)
    }
}

/// A main-toolbar widget (the project, the branch): its contents, a chevron, IntelliJ's hover.
#[derive(IntoElement)]
pub struct ToolbarDropdown {
    base: Stateful<Div>,
    style: StyleRefinement,
    children: Vec<AnyElement>,
    selected: bool,
    tooltip: Option<SharedString>,
}

impl ToolbarDropdown {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: div().id(id),
            style: StyleRefinement::default(),
            children: Vec::new(),
            selected: false,
            tooltip: None,
        }
    }

    /// Also its accessible name, unless `aria_label` gives another after it.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        let tooltip = tooltip.into();
        self.base = self.base.aria_label(tooltip.clone());
        self.tooltip = Some(tooltip);
        self
    }
}

impl RenderOnce for ToolbarDropdown {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let (hover, pressed) = (tokens.hover, tokens.pressed);
        // IntelliJ's dropdown widget: 5 px above and below, 10 px left, 6 px right
        let mut widget = self
            .base
            .h(px(metrics::MAIN_TOOLBAR_BUTTON))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .pl(px(10.))
            .pr(px(6.))
            .rounded(px(metrics::RADIUS_LARGE))
            .text_ui()
            .text_color(tokens.text)
            .cursor_pointer()
            .hover(move |w| w.bg(hover))
            .when(self.selected, |w| w.bg(pressed))
            .role(Role::Button)
            .when_some(self.tooltip, |w, tooltip| {
                w.tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            })
            .children(self.children)
            .child(
                icon("general/chevronDown")
                    .size(px(16.))
                    .text_color(tokens.icon),
            );
        widget.style().refine(&self.style);
        widget
    }
}

/// IntelliJ's non-editable combo box: the choice and a chevron; add `dropdown_menu`.
#[derive(IntoElement)]
pub struct ComboBox {
    base: Stateful<Div>,
    style: StyleRefinement,
    label: SharedString,
    /// What assistive technology names it by: the label before it in a form.
    name: Option<SharedString>,
    selected: bool,
    disabled: bool,
}

impl ComboBox {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            base: div().id(id),
            style: StyleRefinement::default(),
            label: label.into(),
            name: None,
            selected: false,
            disabled: false,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn aria_label(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }
}

impl RenderOnce for ComboBox {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let (text, border) = match self.disabled {
            true => (tokens.text_disabled, tokens.control_border_disabled),
            false => (tokens.text, tokens.control_border),
        };
        // padding 1, 9, 1, 6 and a 16 px chevron, as IntelliJ's
        let mut combo = self
            .base
            .role(Role::ComboBox)
            .aria_value(self.label.clone())
            .when_some(self.name, |combo, name| combo.aria_label(name))
            .h(px(metrics::CONTROL_HEIGHT))
            .min_w(px(180.))
            .flex()
            .items_center()
            .gap_1()
            .pl(px(9.))
            .pr(px(6.))
            .rounded(px(metrics::RADIUS))
            .bg(tokens.control_bg_raised)
            .border_1()
            .border_color(match self.selected {
                true => tokens.focus,
                false => border,
            })
            .text_ui()
            .text_color(text)
            .when(!self.disabled, |c| c.cursor_pointer())
            .child(div().flex_1().min_w_0().truncate().child(self.label))
            .child(
                icon("general/chevronDown")
                    .size(px(16.))
                    .text_color(tokens.icon),
            );
        combo.style().refine(&self.style);
        A11yDisabled::new(combo, self.disabled)
    }
}

/// A combo box of `menu`'s choices.
pub fn combo(
    id: impl Into<ElementId>,
    current: impl Into<SharedString>,
    disabled: bool,
    menu: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
) -> AnyElement {
    dropdown(
        ComboBox::new(id, current).disabled(disabled),
        disabled,
        menu,
    )
}

/// [`combo`] named for assistive technology by the label before it, `width` wide (as IntelliJ
/// sizes a combo box to its longest choice), or as narrow as combo boxes get with 0.
pub fn labeled_combo(
    id: impl Into<ElementId>,
    name: impl Into<SharedString>,
    current: impl Into<SharedString>,
    width: f32,
    disabled: bool,
    menu: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
) -> AnyElement {
    let combo = ComboBox::new(id, current)
        .aria_label(name)
        .disabled(disabled)
        .when(width > 0., |combo| combo.w(px(width)).min_w(px(width)));
    dropdown(combo, disabled, menu)
}

fn dropdown(
    combo: ComboBox,
    disabled: bool,
    menu: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
) -> AnyElement {
    match disabled {
        true => combo.into_any_element(),
        false => combo
            .dropdown_menu(move |m, window, cx| {
                menu(m.scrollable(true).max_h(px(420.)), window, cx)
            })
            .into_any_element(),
    }
}

/// IntelliJ's text field: 28 px, the island's fill, a 1 px control border that becomes a
/// 2 px blue ring with the focus; a search icon or other prefix, a clear button.
#[derive(IntoElement)]
pub struct TextField {
    state: Entity<InputState>,
    style: StyleRefinement,
    prefix: Option<AnyElement>,
    suffix: Option<AnyElement>,
    clearable: bool,
    mask_toggle: bool,
    in_popup: bool,
    disabled: bool,
    aria_label: Option<SharedString>,
}

impl TextField {
    pub fn new(state: &Entity<InputState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
            prefix: None,
            suffix: None,
            clearable: false,
            mask_toggle: false,
            in_popup: false,
            disabled: false,
            aria_label: None,
        }
    }

    /// What assistive technology calls the field: its form label ("Branch Name:").
    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    /// A popup's search field: the popup's fill and no focus ring, as IntelliJ's
    /// `SearchFieldWithExtension`.
    pub fn in_popup(mut self) -> Self {
        self.in_popup = true;
        self
    }

    pub fn prefix(mut self, prefix: impl IntoElement) -> Self {
        self.prefix = Some(prefix.into_any_element());
        self
    }

    pub fn suffix(mut self, suffix: impl IntoElement) -> Self {
        self.suffix = Some(suffix.into_any_element());
        self
    }

    /// A clear button while there is text, as IntelliJ's search fields.
    pub fn cleanable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    /// For secrets: an eye that shows the text.
    pub fn mask_toggle(mut self) -> Self {
        self.mask_toggle = true;
        self
    }

    /// IntelliJ's fields have one size; kept so gpui-component's `small()` calls read the same.
    pub fn small(self) -> Self {
        self
    }

    /// Greyed and read-only, as IntelliJ's disabled fields.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for TextField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let state = self.state.read(cx);
        let focused = state.focus_handle(cx).is_focused(window) && !self.in_popup;
        let empty = state.value().is_empty();
        let target = self.state.clone();
        // the closeSmall icon, its hovered look over it
        let clear = (self.clearable && !empty).then(|| {
            div()
                .id("clear")
                .group("clear")
                .relative()
                .size(px(16.))
                .flex_none()
                .cursor_pointer()
                .child(image("general/closeSmall", cx))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .invisible()
                        .group_hover("clear", |s| s.visible())
                        .child(image("general/closeSmallHovered", cx)),
                )
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    target.update(cx, |state, cx| {
                        state.set_value("", window, cx);
                        cx.emit(InputEvent::Change);
                    })
                })
        });
        let mut field = h_flex()
            .h(px(metrics::CONTROL_HEIGHT))
            .min_w(px(49.))
            .px(px(6.))
            .gap(px(4.))
            .rounded(px(metrics::RADIUS))
            .bg(match self.in_popup {
                true => tokens.popup,
                false => tokens.control_bg,
            })
            .border_1()
            .border_color(match (focused, self.disabled) {
                (true, _) => tokens.focus,
                (false, true) => tokens.control_border_disabled,
                (false, false) => tokens.control_border,
            })
            .when(self.disabled, |f| f.text_color(tokens.text_disabled))
            // the second pixel of IntelliJ's 2 px focus ring, outside the border
            .when(focused, |f| {
                f.shadow(vec![BoxShadow {
                    color: tokens.focus,
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(metrics::FOCUS_RING - 1.),
                    inset: false,
                }])
            })
            .text_ui()
            .children(
                self.prefix
                    .map(|prefix| div().flex_none().text_color(tokens.icon).child(prefix)),
            )
            .child(
                Input::new(&self.state)
                    .appearance(false)
                    .disabled(self.disabled)
                    .when_some(self.aria_label, |input, label| input.aria_label(label))
                    .when(self.mask_toggle, |input| input.mask_toggle())
                    .px_0()
                    .h_full()
                    .flex_1(),
            )
            .children(clear)
            .children(self.suffix);
        field.style().refine(&self.style);
        field
    }
}

/// IntelliJ's checkbox: 16 px, 3 px corners; blue with a white tick when checked, a white bar
/// when partly checked (`None`). Add `on_click`.
pub fn check_box(id: impl Into<ElementId>, state: Option<bool>, cx: &App) -> Stateful<Div> {
    check_box_look(id, state, true, cx)
}

/// [`check_box`], greyed in IntelliJ's disabled colors while not `enabled`.
fn check_box_look(
    id: impl Into<ElementId>,
    state: Option<bool>,
    enabled: bool,
    cx: &App,
) -> Stateful<Div> {
    let tokens = Tokens::get(cx);
    let on = state != Some(false);
    let mark = match enabled {
        true => white(),
        false => tokens.checkbox_mark_disabled,
    };
    div()
        .id(id)
        .size(px(metrics::CHECKBOX))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.))
        .when(enabled, |b| b.cursor_pointer())
        .when(on && enabled, |b| b.bg(tokens.checkbox_checked))
        .when(!on && enabled, |b| {
            b.bg(tokens.checkbox_fill)
                .border_1()
                .border_color(tokens.checkbox_border)
        })
        .when(!enabled, |b| {
            b.bg(tokens.checkbox_fill_disabled)
                .border_1()
                .border_color(tokens.checkbox_border_disabled)
        })
        .when(state == Some(true), |b| {
            b.child(icon("actions/checked").size(px(14.)).text_color(mark))
        })
        .when(state.is_none(), |b| {
            b.child(div().w(px(8.)).h(px(2.)).rounded(px(1.)).bg(mark))
        })
}

/// IntelliJ's radio button: a 16 px circle in the checkbox colors, a 6 px white dot when on;
/// its label 4 px on, both clickable. Add `on_click`.
pub fn labeled_radio(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    on: bool,
    cx: &App,
) -> Stateful<Div> {
    radio_look(id, label, on, true, cx)
}

/// [`labeled_radio`] greyed: it takes no click, and says so to assistive technology.
pub fn disabled_radio(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    on: bool,
    cx: &App,
) -> A11yDisabled<Stateful<Div>> {
    A11yDisabled::new(radio_look(id, label, on, false, cx), true)
}

fn radio_look(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    on: bool,
    enabled: bool,
    cx: &App,
) -> Stateful<Div> {
    let tokens = Tokens::get(cx);
    let circle = div()
        .size(px(metrics::CHECKBOX))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .map(|b| match (on, enabled) {
            (true, true) => b
                .bg(tokens.checkbox_checked)
                .child(div().size(px(6.)).rounded_full().bg(white())),
            (false, true) => b
                .bg(tokens.checkbox_fill)
                .border_1()
                .border_color(tokens.checkbox_border),
            (on, false) => b
                .bg(tokens.checkbox_fill_disabled)
                .border_1()
                .border_color(tokens.checkbox_border_disabled)
                .when(on, |b| {
                    b.child(
                        div()
                            .size(px(6.))
                            .rounded_full()
                            .bg(tokens.checkbox_mark_disabled),
                    )
                }),
        });
    let label = label.into();
    h_flex()
        .id(id.into())
        .role(Role::RadioButton)
        .aria_label(label.clone())
        .aria_toggled(on.into())
        .gap(px(4.))
        .when(enabled, |radio| radio.cursor_pointer())
        .when(!enabled, |radio| radio.text_color(tokens.text_disabled))
        .child(circle)
        .child(label)
}

/// A checkbox with its label 4 px on, both clickable. Add `on_click`.
pub fn labeled_check(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    checked: bool,
    cx: &App,
) -> Stateful<Div> {
    labeled_check_state(id, label, Some(checked), cx)
}

/// [`labeled_check`] of a group whose items may be partly on: `None` shows IntelliJ's dash.
pub fn labeled_check_state(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    state: Option<bool>,
    cx: &App,
) -> Stateful<Div> {
    labeled_check_look(id, label, state, true, cx)
}

fn labeled_check_look(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    state: Option<bool>,
    enabled: bool,
    cx: &App,
) -> Stateful<Div> {
    let (id, label) = (id.into(), label.into());
    h_flex()
        .id(id.clone())
        .role(Role::CheckBox)
        .aria_label(label.clone())
        .aria_toggled(state.map_or(Toggled::Mixed, Toggled::from))
        .gap(px(4.))
        .when(enabled, |check| check.cursor_pointer())
        .when(!enabled, |check| {
            check.text_color(Tokens::get(cx).text_disabled)
        })
        .child(check_box_look(
            SharedString::from(format!("{id}-box")),
            state,
            enabled,
            cx,
        ))
        .child(label)
}

/// IntelliJ's on/off switch (`OnOffButton`, `DarculaOnOffButtonUI-spec.md`): a 26×16 pill, the
/// accent with a 10 px white notch on its right when on, a translucent track with an 8 px white
/// ring on its left when off. Find Action shows a toggle action's state with it.
pub fn switch(id: impl Into<ElementId>, on: bool, cx: &App) -> Stateful<Div> {
    let tokens = Tokens::get(cx);
    let notch = match on {
        true => div().size(px(10.)).ml(px(13.)).rounded_full().bg(white()),
        false => div()
            .size(px(8.))
            .ml(px(4.))
            .rounded_full()
            .border_2()
            .border_color(white()),
    };
    div()
        .id(id)
        .role(Role::Switch)
        .aria_toggled(on.into())
        .flex_none()
        .w(px(metrics::SWITCH_WIDTH))
        .h(px(metrics::SWITCH_HEIGHT))
        .flex()
        .items_center()
        .rounded_full()
        .bg(match on {
            true => tokens.accent,
            false => tokens.toggle_off_bg,
        })
        .child(notch)
}

/// [`labeled_check`] greyed: it takes no click, and says so to assistive technology.
pub fn disabled_check(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    checked: bool,
    cx: &App,
) -> A11yDisabled<Stateful<Div>> {
    A11yDisabled::new(
        labeled_check_look(id, label, Some(checked), false, cx),
        true,
    )
}

/// A row of the Settings dialog's tree (IntelliJ's `SettingsTreeView`, measured on IntelliJ
/// IDEA 2026.3): 25 px, its chevron (`open`, for a group) 13 px in and 16 px more a level, the
/// name 5 px after it, a top-level group's name bold; the page shown selected (greyed while the
/// tree hasn't the focus); a page with changes not applied in `Tree.modifiedItemForeground`
/// (the link color) unless selected. Children added go on its right, as
/// [`settings_project_mark`] and [`count_badge`]. Add `on_click`.
#[allow(clippy::too_many_arguments)]
pub fn settings_tree_row(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    depth: usize,
    open: Option<bool>,
    selected: bool,
    focused: bool,
    modified: bool,
    cx: &App,
) -> Stateful<Div> {
    let tokens = Tokens::get(cx);
    let label = label.into();
    let selection = match focused {
        true => tokens.selection,
        false => tokens.selection_inactive,
    };
    h_flex()
        .id(id.into())
        .role(Role::TreeItem)
        .aria_label(label.clone())
        .aria_selected(selected)
        .when_some(open, |row, open| row.aria_expanded(open))
        .h(px(SETTINGS_TREE_ROW))
        .flex_none()
        .pl(px(SETTINGS_TREE_INSET + depth as f32 * SETTINGS_TREE_INDENT))
        .pr(px(10.))
        .gap(px(5.))
        .cursor_pointer()
        .when(depth == 0, |row| row.font_weight(FontWeight::BOLD))
        .when(modified && !selected, |row| {
            row.text_color(tokens.text_link)
        })
        .when(selected, |row| row.bg(selection))
        .when(!selected, |row| {
            row.hover(move |row| row.bg(tokens.selection_hover))
        })
        .child(
            div()
                .size(px(16.))
                .flex_none()
                .when_some(open, |chevron, open| {
                    chevron.child(
                        icon(match open {
                            true => "general/chevronDown",
                            false => "general/chevronRight",
                        })
                        .size(px(16.))
                        .text_color(tokens.icon),
                    )
                }),
        )
        .child(div().flex_1().min_w_0().truncate().child(label))
}

/// The Settings tree's rows (`SettingsTreeView`'s renderer, 25 px in IntelliJ IDEA 2026.3), a
/// level's indent and the first chevron's inset (`getLeftMargin`: 3 + 16 a level, and the
/// renderer's 10).
const SETTINGS_TREE_ROW: f32 = 25.;
const SETTINGS_TREE_INDENT: f32 = 16.;
const SETTINGS_TREE_INSET: f32 = 13.;

/// The mark on a Settings tree row whose page is the project's (`AllIcons.General.
/// ProjectConfigurable`, at its own 9 px), its explanation in a tooltip.
pub fn settings_project_mark(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    const TEXT: &str = "Settings marked with this icon are only applied to the current project. Non-marked settings are applied to all projects.";
    div()
        .id(id)
        .flex_none()
        .child(image("general/projectConfigurable", cx).size(px(9.)))
        .tooltip(|window, cx| Tooltip::new(TEXT).build(window, cx))
}

/// IntelliJ's grey count badge (`Badge.graySecondary*`): the Settings tree's on Plugins, 16 px
/// high, 12 px text.
pub fn count_badge(count: usize, cx: &App) -> Div {
    let tokens = Tokens::get(cx);
    div()
        .flex_none()
        .h(px(16.))
        .min_w(px(19.))
        .px(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(tokens.badge_gray)
        .text_color(tokens.badge_gray_text)
        .text_ui_small()
        .font_weight(FontWeight::NORMAL)
        .child(count.to_string())
}

/// The round "?" button at the start of a dialog's buttons (Help, `DialogWrapper`'s on macOS):
/// 23 px, a 1 px border. Add `on_click`.
pub fn help_button(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(SharedString::from("Help"))
        .size(px(23.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(Tokens::get(cx).control_border)
        .text_color(Tokens::get(cx).icon)
        .cursor_pointer()
        .child("?")
        .tooltip(|window, cx| Tooltip::new("Show help contents").build(window, cx))
}

/// The title row of a window whose title bar is the app's (the Settings window of IntelliJ IDEA
/// 2026.3): 32 px, on the window's own background, the title after the traffic lights.
pub fn window_header(title: impl Into<SharedString>, cx: &App) -> Div {
    h_flex()
        .w_full()
        .h(px(WINDOW_HEADER_HEIGHT))
        .flex_none()
        .pl(px(84.))
        .text_ui()
        .text_color(Tokens::get(cx).text_muted)
        .font_weight(FontWeight::SEMIBOLD)
        .child(title.into())
}

/// [`window_header`]'s height, and where its traffic lights go (`TitlebarOptions`).
pub const WINDOW_HEADER_HEIGHT: f32 = 32.;
pub const WINDOW_HEADER_TRAFFIC_LIGHTS: Point<Pixels> = point(px(9.), px(9.));

/// IntelliJ's context help (`ContextHelpLabel`): the New UI's question mark in a circle beside a
/// setting, saying more about it in a tooltip.
pub fn context_help(id: impl Into<ElementId>, text: &'static str, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Image)
        .aria_label(text)
        .child(image("general/questionMark", cx).size(px(16.)))
        .tooltip(move |window, cx| Tooltip::new(text).build(window, cx))
}

/// A link: IntelliJ's link color, underlined on hover. Add `on_click`.
pub fn link(id: impl Into<ElementId>, label: impl Into<SharedString>, cx: &App) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id)
        .role(Role::Link)
        .aria_label(label.clone())
        .text_color(Tokens::get(cx).text_link)
        .cursor_pointer()
        .hover(|l| l.underline())
        .child(label)
}

/// IntelliJ's DropDownLink (a git dialog's Modify options): a link and a chevron; add
/// `dropdown_menu`.
#[derive(IntoElement)]
pub struct DropDownLink {
    base: Stateful<Div>,
    style: StyleRefinement,
    label: SharedString,
    selected: bool,
    chevron: bool,
}

impl DropDownLink {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            base: div().id(id),
            style: StyleRefinement::default(),
            label: label.into(),
            selected: false,
            chevron: true,
        }
    }

    /// The link alone, as the Push dialog's remote, which opens the remotes when clicked.
    pub fn without_chevron(mut self) -> Self {
        self.chevron = false;
        self
    }
}

impl RenderOnce for DropDownLink {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = Tokens::get(cx).text_link;
        let mut link = self
            .base
            .role(Role::Link)
            .aria_label(self.label.clone())
            .flex()
            .items_center()
            .gap(px(2.))
            .text_ui()
            .text_color(color)
            .cursor_pointer()
            .child(self.label)
            .when(self.chevron, |l| {
                l.child(icon("general/chevronDown").size(px(16.)).text_color(color))
            });
        link.style().refine(&self.style);
        link
    }
}

macro_rules! interactive {
    ($($ty:ident),*) => {$(
        impl Styled for $ty {
            fn style(&mut self) -> &mut StyleRefinement {
                &mut self.style
            }
        }
        impl InteractiveElement for $ty {
            fn interactivity(&mut self) -> &mut Interactivity {
                self.base.interactivity()
            }
        }
        impl StatefulInteractiveElement for $ty {}
        impl Selectable for $ty {
            fn selected(mut self, selected: bool) -> Self {
                self.selected = selected;
                self
            }
            fn is_selected(&self) -> bool {
                self.selected
            }
        }
        impl DropdownMenu for $ty {}
    )*};
}

interactive!(
    Button,
    ActionButton,
    ToolbarDropdown,
    ComboBox,
    DropDownLink,
    OptionArrow
);

impl ParentElement for Button {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}

impl ParentElement for ToolbarDropdown {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}

impl Styled for TextField {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

/// A tooltip's action name, without the shortcut IntelliJ writes after it: `Show Diff (⌘D)`.
fn without_shortcut(tooltip: &str) -> &str {
    match tooltip.rsplit_once(" (") {
        Some((name, _)) if tooltip.ends_with(')') => name,
        _ => tooltip,
    }
}

/// A dialog's title, which assistive technology reads as its heading.
pub fn dialog_title(title: impl Into<SharedString>) -> Stateful<Div> {
    let title = title.into();
    div()
        .id("dialog-title")
        .role(Role::Heading)
        .aria_label(title.clone())
        .child(title)
}

/// A form's label ("New name:"), in front of its field.
pub fn form_label(text: impl Into<SharedString>) -> Stateful<Div> {
    let text = text.into();
    div()
        .id(text.clone())
        .role(Role::Label)
        .aria_label(text.clone())
        .flex_none()
        .child(text)
}

#[cfg(test)]
mod tests {
    use super::without_shortcut;

    #[test]
    fn names_a_button_without_its_shortcut() {
        assert_eq!(without_shortcut("Show Diff (⌘D)"), "Show Diff");
        assert_eq!(without_shortcut("View Options"), "View Options");
        assert_eq!(without_shortcut("Compare (Base)x"), "Compare (Base)x");
    }
}
