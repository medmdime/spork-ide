//! IntelliJ IDEA 2026.3's Beans tool window (`microservices-jvm`'s, on the right; 2026.3 has no
//! Spring tool window, Beans took its place) over the Spring Boot language server's index
//! (`spring/index/beans`): the project's beans by module, two lines each unless Compact List
//! Items, Show From Tests, the selected bean's Documentation in the side panel, a double click
//! going to its definition.

use std::path::PathBuf;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render,
    StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window, div, px,
};
use gpui_kit::component::{h_flex, v_flex};
use serde_json::{Value, json};
use spork_workbench::tokens::{Tokens, Typography as _};
use spork_workbench::tree::{Tree, TreeItem};
use util::ResultExt as _;
use workspace::Workspace;
use workspace::dock::{DockPosition, GearItem, Panel, PanelEvent};

use crate::java_project::JavaProject;

pub mod beans_tool_window {
    gpui::actions!(
        beans_tool_window,
        [
            /// Opens the Beans tool window and focuses it, or gives the focus back to the editor.
            ToggleFocus,
            /// Lists the beans on one line each.
            CompactListItems,
            /// Lists the beans under their module.
            GroupByModule,
            /// Lists the beans of the project's libraries too.
            ShowFromLibraries,
            /// Lists the beans of the project's tests too.
            ShowFromTests,
            /// Shows the selected bean's documentation.
            ShowSidePanel,
        ]
    );
}

/// A bean as the list shows it, read from the index's JSON.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BeanRow {
    pub name: String,
    pub type_name: String,
    /// The annotation that makes it a bean (`@RestController`, `@Bean`), as written.
    pub annotation: Option<String>,
    pub file: Option<PathBuf>,
    pub line: u32,
    /// Its injection points: each one's name and type.
    pub injection_points: Vec<(String, String)>,
}

impl BeanRow {
    fn simple_type(&self) -> &str {
        self.type_name.rsplit('.').next().unwrap_or(&self.type_name)
    }

    fn in_tests(&self) -> bool {
        self.file
            .as_ref()
            .is_some_and(|file| file.components().any(|part| part.as_os_str() == "test"))
    }
}

/// The index's beans as rows, by name.
pub(crate) fn rows(index: &[Value]) -> Vec<BeanRow> {
    let text = |value: &Value, key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let mut rows: Vec<BeanRow> = index
        .iter()
        .map(|bean| {
            let location = bean.get("location");
            let file = location
                .and_then(|location| location.get("uri"))
                .and_then(Value::as_str)
                .and_then(|uri| uri.parse::<lsp::Uri>().ok())
                .and_then(|uri| uri.to_file_path().ok());
            let line = location
                .and_then(|location| location.pointer("/range/start/line"))
                .and_then(Value::as_u64)
                .unwrap_or_default() as u32;
            let annotation = bean
                .get("annotations")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|annotation| annotation["isMetaAnnotation"] != Value::Bool(true))
                .map(|annotation| format!("@{}", text(annotation, "annotationName")));
            BeanRow {
                name: text(bean, "name"),
                type_name: text(bean, "type"),
                annotation,
                file,
                line,
                injection_points: bean
                    .get("injectionPoints")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|point| (text(point, "name"), text(point, "type")))
                    .collect(),
            }
        })
        .collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    rows
}

/// Adds the Beans tool window to a Spring Boot project's workspace.
pub(crate) fn add(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let Some(java) = JavaProject::of(workspace, cx) else {
        return;
    };
    workspace.register_action(
        |workspace, _: &beans_tool_window::ToggleFocus, window, cx| {
            workspace.toggle_panel_focus::<BeansToolWindow>(window, cx);
        },
    );
    let option = |toggle: fn(&mut BeansToolWindow)| {
        move |workspace: &mut Workspace, cx: &mut Context<Workspace>| {
            if let Some(panel) = workspace.panel::<BeansToolWindow>(cx) {
                panel.update(cx, |panel, cx| {
                    toggle(panel);
                    cx.notify();
                });
            }
        }
    };
    let compact = option(|panel| panel.compact = !panel.compact);
    workspace.register_action(
        move |workspace, _: &beans_tool_window::CompactListItems, _, cx| compact(workspace, cx),
    );
    let group = option(|panel| panel.group_by_module = !panel.group_by_module);
    workspace.register_action(
        move |workspace, _: &beans_tool_window::GroupByModule, _, cx| group(workspace, cx),
    );
    let libraries = option(|panel| panel.from_libraries = !panel.from_libraries);
    workspace.register_action(
        move |workspace, _: &beans_tool_window::ShowFromLibraries, _, cx| libraries(workspace, cx),
    );
    let tests = option(|panel| panel.from_tests = !panel.from_tests);
    workspace.register_action(
        move |workspace, _: &beans_tool_window::ShowFromTests, _, cx| tests(workspace, cx),
    );
    let side = option(|panel| panel.side_panel = !panel.side_panel);
    workspace.register_action(
        move |workspace, _: &beans_tool_window::ShowSidePanel, _, cx| side(workspace, cx),
    );
    let panel = cx.new(|cx| BeansToolWindow::new(java, workspace.weak_handle(), cx));
    workspace.add_panel(panel, window, cx);
}

pub(crate) struct BeansToolWindow {
    java: Entity<JavaProject>,
    workspace: WeakEntity<Workspace>,
    focus: FocusHandle,
    /// The selected bean, by name.
    selected: Option<String>,
    compact: bool,
    group_by_module: bool,
    // shortcut: the Spring LS indexes the project's sources only, so Show From Libraries adds
    // nothing yet; read the libraries' beans once it indexes them
    from_libraries: bool,
    from_tests: bool,
    side_panel: bool,
    _subscription: Subscription,
}

impl BeansToolWindow {
    fn new(
        java: Entity<JavaProject>,
        workspace: WeakEntity<Workspace>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.observe(&java, |_, _, cx| cx.notify());
        Self {
            java,
            workspace,
            focus: cx.focus_handle(),
            selected: None,
            compact: false,
            group_by_module: true,
            from_libraries: false,
            from_tests: false,
            side_panel: true,
            _subscription: subscription,
        }
    }

    /// The beans the filters leave: the project's own, its tests' with Show From Tests.
    fn shown(&self, cx: &App) -> Vec<BeanRow> {
        rows(&self.java.read(cx).spring.index.beans)
            .into_iter()
            .filter(|bean| self.from_tests || !bean.in_tests())
            .collect()
    }

    fn row(&self, bean: &BeanRow, cx: &mut Context<Self>) -> TreeItem {
        let this = cx.entity().downgrade();
        let clicked = bean.clone();
        let source = bean.simple_type().to_owned();
        let subtitle = bean.annotation.clone().unwrap_or_default();
        TreeItem::new(bean.name.clone())
            .colored_icon("gutter/springBean")
            .active(self.selected.as_deref() == Some(bean.name.as_str()))
            .when(!self.compact && !subtitle.is_empty(), |item| {
                item.subtitle(subtitle)
            })
            .suffix(move |_, _| div().child(source.clone()))
            .on_click(move |event, window, cx| {
                let bean = clicked.clone();
                this.update(cx, |this, cx| {
                    this.selected = Some(bean.name.clone());
                    cx.notify();
                    if event.click_count() >= 2 {
                        this.open(&bean, window, cx);
                    }
                })
                .log_err();
            })
    }

    /// Goes to the bean's definition.
    fn open(&self, bean: &BeanRow, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = bean.file.clone() else {
            return;
        };
        let line = bean.line;
        self.workspace
            .update(cx, |workspace, cx| {
                crate::endpoints::open_at(workspace, file, line, window, cx)
            })
            .log_err();
    }

    /// The side panel's Documentation: the bean, its type, how it is declared, where, and what
    /// it injects.
    fn documentation(&self, bean: &BeanRow, cx: &App) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let row = |label: &'static str, value: String| {
            h_flex()
                .gap(px(8.))
                .child(
                    spork_workbench::controls::form_label(label)
                        .w(px(80.))
                        .text_color(tokens.text_secondary),
                )
                .child(spork_workbench::controls::form_label(value).min_w_0())
        };
        let source = bean
            .file
            .as_ref()
            .and_then(|file| file.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        v_flex()
            .flex_none()
            .border_t_1()
            .border_color(tokens.separator)
            .child(
                h_flex()
                    .id("bean-side-tabs")
                    .role(gpui::Role::TabList)
                    .aria_label("Bean Side Panel")
                    .px(px(6.))
                    .py(px(2.))
                    .border_b_1()
                    .border_color(tokens.separator)
                    .child(
                        div()
                            .id("bean-tab-documentation")
                            .role(gpui::Role::Tab)
                            .aria_label("Documentation")
                            .aria_selected(true)
                            .h(px(28.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .bg(tokens.tab_selected)
                            .child("Documentation"),
                    ),
            )
            .child(
                v_flex()
                    .id("bean-documentation")
                    .role(gpui::Role::Group)
                    .aria_label("Documentation")
                    .p(px(10.))
                    .gap(px(6.))
                    .child(
                        spork_workbench::controls::form_label(bean.name.clone())
                            .font_weight(FontWeight::BOLD),
                    )
                    .child(row("Type", bean.type_name.clone()))
                    .children(
                        bean.annotation
                            .clone()
                            .map(|annotation| row("Declared", annotation)),
                    )
                    .child(row("Source", source))
                    .children(bean.injection_points.iter().map(|(name, type_name)| {
                        let simple = type_name.rsplit('.').next().unwrap_or(type_name);
                        row("Injects", format!("{name}: {simple}"))
                    })),
            )
    }
}

impl EventEmitter<PanelEvent> for BeansToolWindow {}

impl Focusable for BeansToolWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for BeansToolWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let shown = self.shown(cx);
        let loaded = self.java.read(cx).spring.index.loaded;
        let items: Vec<TreeItem> = shown.iter().map(|bean| self.row(bean, cx)).collect();
        let list = match (loaded, items.is_empty()) {
            (false, _) => div()
                .p(px(10.))
                .text_color(tokens.text_secondary)
                .child("Loading beans…")
                .into_any_element(),
            (true, true) => div()
                .p(px(10.))
                .text_color(tokens.text_secondary)
                .child("No beans found")
                .into_any_element(),
            (true, false) => {
                let items = match self.group_by_module {
                    true => vec![
                        TreeItem::new(crate::endpoints::module_name(self.java.read(cx)))
                            .colored_icon("nodes/module")
                            .default_open(true)
                            .children(items),
                    ],
                    false => items,
                };
                Tree::new("beans").children(items).into_any_element()
            }
        };
        let side = self
            .selected
            .as_ref()
            .filter(|_| self.side_panel)
            .and_then(|name| shown.iter().find(|bean| &bean.name == name))
            .map(|bean| self.documentation(bean, cx).into_any_element());
        let side = match (self.side_panel, side) {
            (_, Some(side)) => Some(side),
            (true, None) => Some(
                div()
                    .flex_none()
                    .p(px(10.))
                    .border_t_1()
                    .border_color(tokens.separator)
                    .text_color(tokens.text_secondary)
                    .child(
                        div()
                            .id("beans-documentation-empty")
                            .role(gpui::Role::Label)
                            .aria_label("Select bean to view documentation")
                            .child("Select bean to view documentation"),
                    )
                    .into_any_element(),
            ),
            (false, None) => None,
        };
        v_flex()
            .id("beans-tool-window")
            .track_focus(&self.focus)
            .role(gpui::Role::Group)
            .aria_label("Beans")
            .size_full()
            .text_ui()
            .child(
                div()
                    .id("beans-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
            .children(side)
    }
}

impl Panel for BeansToolWindow {
    fn persistent_name() -> &'static str {
        "Beans"
    }

    fn panel_key() -> &'static str {
        "BeansToolWindow"
    }

    fn position(&self, _: &Window, _: &App) -> DockPosition {
        DockPosition::Right
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        position == DockPosition::Right
    }

    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}

    fn default_size(&self, _: &Window, _: &App) -> Pixels {
        px(360.)
    }

    fn icon(&self, _: &Window, _: &App) -> Option<ui::IconName> {
        Some(ui::IconName::ListTree)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some("Beans")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(beans_tool_window::ToggleFocus)
    }

    // the third Spring wave's tool windows take 320–329
    fn activation_priority(&self) -> u32 {
        320
    }

    fn gear_items(&self, _: &Window, _: &App) -> Vec<GearItem> {
        let item = |label: &'static str, checked: bool, action: Box<dyn Action>| GearItem {
            label: label.into(),
            checked: Some(checked),
            action,
        };
        vec![
            item(
                "Compact List Items",
                self.compact,
                Box::new(beans_tool_window::CompactListItems),
            ),
            item(
                "Group by Module",
                self.group_by_module,
                Box::new(beans_tool_window::GroupByModule),
            ),
            item(
                "Show From Libraries",
                self.from_libraries,
                Box::new(beans_tool_window::ShowFromLibraries),
            ),
            item(
                "Show From Tests",
                self.from_tests,
                Box::new(beans_tool_window::ShowFromTests),
            ),
            item(
                "Show Side Panel",
                self.side_panel,
                Box::new(beans_tool_window::ShowSidePanel),
            ),
        ]
    }
}

/// What the end-to-end tests read of the Beans tool window: the beans listed and the selected one.
pub(crate) fn e2e_state(workspace: Option<&Workspace>, cx: &App) -> Value {
    let Some(panel) = workspace.and_then(|workspace| workspace.panel::<BeansToolWindow>(cx)) else {
        return Value::Null;
    };
    let panel = panel.read(cx);
    json!({
        "shown": panel.shown(cx).iter().map(|bean| bean.name.clone()).collect::<Vec<_>>(),
        "selected": panel.selected,
        "compact": panel.compact,
    })
}

#[cfg(test)]
mod tests {
    use super::rows;
    use serde_json::json;

    #[test]
    fn a_bean_row_from_the_index() {
        let index = vec![json!({
            "name": "helloController",
            "type": "com.example.demo.HelloController",
            "location": {"uri": "file:///p/src/main/java/com/example/demo/HelloController.java",
                         "range": {"start": {"line": 7, "character": 13}}},
            "annotations": [
                {"annotationName": "RestController", "isMetaAnnotation": false},
                {"annotationName": "Controller", "isMetaAnnotation": true}
            ],
            "injectionPoints": [{"name": "greeter", "type": "com.example.demo.Greeter"}]
        })];
        let rows = rows(&index);
        assert_eq!(rows[0].name, "helloController");
        assert_eq!(rows[0].simple_type(), "HelloController");
        assert_eq!(rows[0].annotation.as_deref(), Some("@RestController"));
        assert_eq!(rows[0].line, 7);
        assert!(!rows[0].in_tests());
        assert_eq!(
            rows[0].injection_points,
            [("greeter".into(), "com.example.demo.Greeter".into())]
        );
    }
}
