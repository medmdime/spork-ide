//! IntelliJ's Endpoints tool window (`microservices-plugin`'s, on the right) over the Spring Boot
//! language server's request mappings (its `@/` symbols): each HTTP Server endpoint of the
//! project, by module, filtered, the selected one's details below; a double click goes to its
//! handler. Also where the Spring LS's index is read for the gutter (`spring_markers`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, App, AppContext as _, AsyncApp, Context, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, TaskExt as _, WeakEntity,
    Window, div, px,
};
use gpui_kit::component::{h_flex, v_flex};
use lsp::LanguageServer;
use serde_json::{Value, json};
use spork_workbench::tokens::{Tokens, Typography as _};
use spork_workbench::tree::{Tree, TreeItem};
use util::ResultExt as _;
use workspace::Workspace;
use workspace::dock::{DockPosition, GearItem, Panel, PanelEvent};

use crate::java_project::JavaProject;

pub mod endpoints_tool_window {
    gpui::actions!(
        endpoints_tool_window,
        [
            /// Opens the Endpoints tool window and focuses it, or gives the focus back to the
            /// editor.
            ToggleFocus,
            /// Lists the endpoints on one line each, without their type and framework.
            CompactListItems,
            /// Lists the endpoints under their module.
            GroupByModule,
            /// Lists the endpoints of the project's libraries too.
            ShowFromLibraries,
            /// Lists the endpoints of the project's tests too.
            ShowFromTests,
            /// Shows the selected endpoint's details.
            ShowSidePanel,
        ]
    );
}

/// An HTTP Server endpoint: a request mapping the Spring LS found.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Endpoint {
    pub path: String,
    /// `GET`, `POST`…; none when it takes every method.
    pub methods: Vec<String>,
    pub file: PathBuf,
    /// The mapping's line, from 0.
    pub line: u32,
}

impl Endpoint {
    /// Its handler's class, as IntelliJ's list names the endpoint's source.
    pub fn source(&self) -> String {
        self.file
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn methods_text(&self) -> String {
        self.methods.join(", ")
    }
}

/// A request mapping symbol's name (`@/hello -- GET`, `@/items/{id} -- GET,POST`), as its path
/// and methods.
pub(crate) fn parse_mapping(name: &str) -> Option<(String, Vec<String>)> {
    let name = name.strip_prefix('@')?;
    if !name.starts_with('/') {
        return None;
    }
    let (path, rest) = name.split_once(" -- ").unwrap_or((name, ""));
    let methods = rest
        .split(" - ")
        .next()
        .unwrap_or_default()
        .split(',')
        .map(|method| method.trim().to_owned())
        .filter(|method| !method.is_empty())
        .collect();
    Some((path.trim().to_owned(), methods))
}

/// The Spring LS's request mappings: its `@/` workspace symbols, those of the project's files.
pub(crate) async fn request_mappings(
    spring: &LanguageServer,
    root: &Path,
) -> Result<Vec<Endpoint>> {
    let symbols = spring
        .request::<lsp::request::WorkspaceSymbolRequest>(
            lsp::WorkspaceSymbolParams {
                query: "@/".into(),
                ..Default::default()
            },
            Duration::from_secs(60),
        )
        .await
        .into_response()?;
    let located: Vec<(String, lsp::Location)> = match symbols {
        Some(lsp::WorkspaceSymbolResponse::Flat(symbols)) => symbols
            .into_iter()
            .map(|symbol| (symbol.name, symbol.location))
            .collect(),
        Some(lsp::WorkspaceSymbolResponse::Nested(symbols)) => symbols
            .into_iter()
            .filter_map(|symbol| match symbol.location {
                lsp::OneOf::Left(location) => Some((symbol.name, location)),
                lsp::OneOf::Right(_) => None,
            })
            .collect(),
        None => Vec::new(),
    };
    let mut endpoints: Vec<Endpoint> = located
        .into_iter()
        .filter_map(|(name, location)| {
            let (path, methods) = parse_mapping(&name)?;
            let file = location.uri.to_file_path().ok()?;
            file.starts_with(root).then_some(Endpoint {
                path,
                methods,
                file,
                line: location.range.start.line,
            })
        })
        .collect();
    endpoints.sort_by(|a, b| (&a.path, &a.methods).cmp(&(&b.path, &b.methods)));
    endpoints.dedup();
    Ok(endpoints)
}

/// What the Spring LS knows of a project's beans (`spring/index/beans`): each bean's name, type,
/// place and injection points, as the server writes them.
pub(crate) async fn beans(spring: &LanguageServer, project: &str) -> Result<Vec<Value>> {
    let beans = spring
        .request::<SpringBeans>(json!({ "projectName": project }), Duration::from_secs(60))
        .await
        .into_response()?;
    Ok(beans.as_array().cloned().unwrap_or_default())
}

/// The Spring LS's `spring/index/beans`.
enum SpringBeans {}

impl lsp::request::Request for SpringBeans {
    type Params = Value;
    type Result = Value;
    const METHOD: &'static str = "spring/index/beans";
}

/// The project's Spring index as read last: its endpoints and beans, read again each time the
/// Spring LS says its index changed.
#[derive(Default)]
pub(crate) struct SpringIndex {
    pub endpoints: Vec<Endpoint>,
    /// The beans as the server writes them, and read for the gutter, which lays out each frame.
    pub beans: Vec<Value>,
    pub parsed_beans: std::rc::Rc<[crate::spring_markers::Bean]>,
    pub loaded: bool,
    task: Option<Task<()>>,
}

/// The project's module as IntelliJ and JDT LS name it (the Spring LS knows a project by it): a
/// Maven project's artifact, the folder's name otherwise.
pub(crate) fn module_name(java: &JavaProject) -> String {
    java.pom
        .as_ref()
        .map_or_else(|| java.name(), |pom| pom.artifact_id.clone())
}

/// Reads the project's Spring index again.
pub(crate) fn refresh(java: &mut JavaProject, cx: &mut Context<JavaProject>) {
    let (Some(root), Some(spring)) = (
        java.root.clone(),
        crate::spring_boot::spring_language_server(&java.project, cx),
    ) else {
        return;
    };
    let name = module_name(java);
    java.spring.index.task = Some(cx.spawn(async move |this, cx| {
        let endpoints = request_mappings(&spring, &root).await.log_err();
        let beans = beans(&spring, &name).await.log_err();
        this.update(cx, |this, cx| {
            let index = &mut this.spring.index;
            index.endpoints = endpoints.unwrap_or_default();
            index.beans = beans.unwrap_or_default();
            index.parsed_beans = crate::spring_markers::beans(&index.beans).into();
            index.loaded = true;
            index.task = None;
            cx.notify();
            // the gutter's markers come from the index
            cx.refresh_windows();
        })
        .log_err();
    }));
}

/// Refreshes the index of the project of `this` once the Spring LS says it changed.
pub(crate) fn on_index_updated(this: WeakEntity<JavaProject>, cx: &mut AsyncApp) {
    this.update(cx, refresh).log_err();
}

/// Search Everywhere's Endpoints tab and Go to Endpoint… over a Spring Boot project's endpoints.
pub(crate) fn init(cx: &mut App) {
    spork_navigate::register_endpoints(
        |workspace, cx| {
            let java = JavaProject::of(workspace, cx)?;
            let java = java.read(cx);
            // the Spring LS's index is read in Spring Boot projects only
            java.spring.index.loaded.then(|| {
                java.spring
                    .index
                    .endpoints
                    .iter()
                    .map(|endpoint| {
                        let (file, line) = (endpoint.file.clone(), endpoint.line);
                        spork_navigate::EndpointItem {
                            url: endpoint.path.clone().into(),
                            methods: endpoint.methods_text().into(),
                            location: endpoint.source().into(),
                            open: std::rc::Rc::new(move |workspace, window, cx| {
                                open_at(workspace, file.clone(), line, window, cx)
                            }),
                        }
                    })
                    .collect()
            })
        },
        cx,
    );
}

/// Adds the Endpoints tool window to a Spring Boot project's workspace.
pub(crate) fn add(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let Some(java) = JavaProject::of(workspace, cx) else {
        return;
    };
    workspace.register_action(
        |workspace, _: &endpoints_tool_window::ToggleFocus, window, cx| {
            workspace.toggle_panel_focus::<EndpointsToolWindow>(window, cx);
        },
    );
    let option = |toggle: fn(&mut EndpointsToolWindow)| {
        move |workspace: &mut Workspace, cx: &mut Context<Workspace>| {
            if let Some(panel) = workspace.panel::<EndpointsToolWindow>(cx) {
                panel.update(cx, |panel, cx| {
                    toggle(panel);
                    cx.notify();
                });
            }
        }
    };
    let compact = option(|panel| panel.compact = !panel.compact);
    workspace.register_action(
        move |workspace, _: &endpoints_tool_window::CompactListItems, _, cx| compact(workspace, cx),
    );
    let group = option(|panel| panel.group_by_module = !panel.group_by_module);
    workspace.register_action(
        move |workspace, _: &endpoints_tool_window::GroupByModule, _, cx| group(workspace, cx),
    );
    let libraries = option(|panel| panel.from_libraries = !panel.from_libraries);
    workspace.register_action(
        move |workspace, _: &endpoints_tool_window::ShowFromLibraries, _, cx| {
            libraries(workspace, cx)
        },
    );
    let tests = option(|panel| panel.from_tests = !panel.from_tests);
    workspace.register_action(
        move |workspace, _: &endpoints_tool_window::ShowFromTests, _, cx| tests(workspace, cx),
    );
    let side = option(|panel| panel.side_panel = !panel.side_panel);
    workspace.register_action(
        move |workspace, _: &endpoints_tool_window::ShowSidePanel, _, cx| side(workspace, cx),
    );
    let panel = cx.new(|cx| EndpointsToolWindow::new(java, workspace.weak_handle(), cx));
    workspace.add_panel(panel, window, cx);
}

pub(crate) struct EndpointsToolWindow {
    java: Entity<JavaProject>,
    workspace: WeakEntity<Workspace>,
    focus: FocusHandle,
    selected: Option<Endpoint>,
    compact: bool,
    /// The side panel's tab.
    tab: SideTab,
    group_by_module: bool,
    // shortcut: the Spring LS indexes the project's sources only, so Show From Libraries adds
    // nothing yet; read the libraries' mappings once it indexes them
    from_libraries: bool,
    from_tests: bool,
    side_panel: bool,
    _subscription: Subscription,
}

impl EndpointsToolWindow {
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
            tab: SideTab::HttpClient,
            group_by_module: true,
            from_libraries: false,
            from_tests: false,
            side_panel: true,
            _subscription: subscription,
        }
    }

    /// The endpoints the filters leave: the project's own, its tests' with Show From Tests.
    fn shown(&self, cx: &App) -> Vec<Endpoint> {
        let from_tests = self.from_tests;
        self.java
            .read(cx)
            .spring
            .index
            .endpoints
            .iter()
            .filter(|endpoint| {
                from_tests
                    || !endpoint
                        .file
                        .components()
                        .any(|part| part.as_os_str() == "test")
            })
            .cloned()
            .collect()
    }

    fn row(&self, endpoint: &Endpoint, cx: &mut Context<Self>) -> TreeItem {
        let this = cx.entity().downgrade();
        let clicked = endpoint.clone();
        let source = endpoint.source();
        TreeItem::new(endpoint.path.clone())
            .colored_icon("gutter/web")
            .hint(endpoint.methods_text())
            .when(!self.compact, |item| {
                item.subtitle(format!("{TYPE}  {FRAMEWORK}"))
            })
            .active(self.selected.as_ref() == Some(endpoint))
            .suffix(move |_, _| div().child(source.clone()))
            .on_click(move |event, window, cx| {
                let endpoint = clicked.clone();
                this.update(cx, |this, cx| {
                    this.selected = Some(endpoint.clone());
                    cx.notify();
                    if event.click_count() >= 2 {
                        this.open(&endpoint, window, cx);
                    }
                })
                .log_err();
            })
    }

    /// Goes to the endpoint's handler.
    fn open(&self, endpoint: &Endpoint, window: &mut Window, cx: &mut Context<Self>) {
        let (path, line) = (endpoint.file.clone(), endpoint.line);
        self.workspace
            .update(cx, |workspace, cx| {
                open_at(workspace, path, line, window, cx)
            })
            .log_err();
    }

    fn details(&self, endpoint: &Endpoint, cx: &App) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let java = self.java.read(cx);
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
        v_flex()
            .id("endpoint-details")
            .role(gpui::Role::Group)
            .aria_label("Documentation")
            .p(px(10.))
            .gap(px(6.))
            .child(
                spork_workbench::controls::form_label(
                    format!("{} {}", endpoint.methods_text(), endpoint.path)
                        .trim()
                        .to_owned(),
                )
                .font_weight(FontWeight::BOLD),
            )
            .child(row("Type", TYPE.into()))
            .child(row("Framework", FRAMEWORK.into()))
            .child(row("Module", module_name(java)))
            .child(row("Source", endpoint.source()))
    }
}

/// The side panel's tabs, IntelliJ's `endpointsSidePanelProvider`s in their order.
#[derive(Clone, Copy, PartialEq)]
enum SideTab {
    HttpClient,
    Documentation,
    Examples,
}

/// The endpoints' type and framework, as the list's second line and Documentation write them.
const TYPE: &str = "HTTP Server";
const FRAMEWORK: &str = "Spring MVC";

impl EndpointsToolWindow {
    /// The side panel: its tabs over the selected endpoint's Documentation or Examples.
    fn side_panel(&self, endpoint: &Endpoint, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let tab =
            |id: &'static str, label: &'static str, which: SideTab, cx: &mut Context<Self>| {
                let on = self.tab == which;
                div()
                    .id(id)
                    .role(gpui::Role::Tab)
                    .aria_label(label)
                    .aria_selected(on)
                    .h(px(28.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(on, |tab| tab.bg(tokens.tab_selected))
                    .when(!on, |tab| tab.hover(move |tab| tab.bg(tokens.hover)))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tab = which;
                        cx.notify();
                    }))
            };
        let content = match self.tab {
            SideTab::HttpClient => self.http_client(endpoint, cx).into_any_element(),
            SideTab::Documentation => self.details(endpoint, cx).into_any_element(),
            SideTab::Examples => self.examples(endpoint, cx).into_any_element(),
        };
        v_flex()
            .flex_none()
            .max_h(px(320.))
            .border_t_1()
            .border_color(tokens.separator)
            .child(
                h_flex()
                    .id("endpoint-side-tabs")
                    .role(gpui::Role::TabList)
                    .aria_label("Endpoint Side Panel")
                    .px(px(6.))
                    .py(px(2.))
                    .gap(px(2.))
                    .border_b_1()
                    .border_color(tokens.separator)
                    .child(tab(
                        "endpoint-tab-http-client",
                        "HTTP Client",
                        SideTab::HttpClient,
                        cx,
                    ))
                    .child(tab(
                        "endpoint-tab-documentation",
                        "Documentation",
                        SideTab::Documentation,
                        cx,
                    ))
                    .child(tab(
                        "endpoint-tab-examples",
                        "Examples",
                        SideTab::Examples,
                        cx,
                    )),
            )
            .child(content)
    }

    /// HTTP Client: the endpoint's request as the HTTP Client writes it, Submit Request running it
    /// (its response in Services) and Open in HTTP Client.
    // shortcut: the request shown as text, where IntelliJ embeds an editor of it with the response
    // under it
    fn http_client(&self, endpoint: &Endpoint, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let request = request_line(endpoint, &self.base_url(cx));
        let (submit, open) = (request.clone(), request.clone());
        let (java, workspace) = (self.java.clone(), self.workspace.clone());
        let open_workspace = workspace.clone();
        v_flex()
            .id("endpoint-http-client")
            .role(gpui::Role::Group)
            .aria_label("HTTP Client")
            .p(px(10.))
            .gap(px(8.))
            .child(
                div()
                    .id("endpoint-request")
                    .role(gpui::Role::Label)
                    .aria_label(request.clone())
                    .p(px(8.))
                    .rounded(px(4.))
                    .bg(tokens.panel)
                    .font_family("JetBrains Mono")
                    .child(request),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(
                        spork_workbench::Button::new("endpoint-submit", "Submit Request").on_click(
                            move |_, window, cx| {
                                let (Some(workspace), Some(root)) =
                                    (workspace.upgrade(), java.read(cx).root.clone())
                                else {
                                    return;
                                };
                                spork_http::submit_requests(&workspace, &submit, &root, window, cx);
                            },
                        ),
                    )
                    .child(
                        spork_workbench::controls::link(
                            "endpoint-open-http-client",
                            "Open in HTTP Client",
                            cx,
                        )
                        .on_click(move |_, window, cx| {
                            let Some((method, url)) = open.split_once(' ') else {
                                return;
                            };
                            open_workspace
                                .update(cx, |workspace, cx| {
                                    spork_http::open_in_http_client(
                                        workspace, method, url, window, cx,
                                    )
                                })
                                .log_err();
                        }),
                    ),
            )
    }

    /// Examples: the endpoint called from a client, IntelliJ's cURL client generator's command.
    // shortcut: cURL alone, where IntelliJ lists each installed client generator (Retrofit…) with
    // Framework Languages, Framework Versions and Show Boilerplate Code
    fn examples(&self, endpoint: &Endpoint, cx: &App) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let command = curl_command(endpoint, self.base_url(cx).as_str());
        v_flex()
            .id("endpoint-examples")
            .role(gpui::Role::Group)
            .aria_label("Examples")
            .p(px(10.))
            .gap(px(6.))
            .child(spork_workbench::controls::form_label("cURL").text_color(tokens.text_secondary))
            .child(
                div()
                    .id("endpoint-example")
                    .role(gpui::Role::Label)
                    .aria_label(command.clone())
                    .p(px(8.))
                    .rounded(px(4.))
                    .bg(tokens.panel)
                    .font_family("JetBrains Mono")
                    .whitespace_normal()
                    .child(command),
            )
    }

    /// Where the application listens: `http://localhost` and its `server.port`.
    fn base_url(&self, cx: &App) -> String {
        let port = self
            .java
            .read(cx)
            .root
            .as_deref()
            .and_then(crate::spring_markers::server_port)
            .unwrap_or(8080);
        format!("http://localhost:{port}")
    }
}

/// An endpoint's request as the HTTP Client writes it: `GET http://localhost:8080/hello`.
pub(crate) fn request_line(endpoint: &Endpoint, base: &str) -> String {
    let method = endpoint.methods.first().map_or("GET", String::as_str);
    format!("{method} {base}{}", endpoint.path)
}

/// An endpoint's cURL command, as IntelliJ's HTTP Client converts a request to cURL.
pub(crate) fn curl_command(endpoint: &Endpoint, base: &str) -> String {
    let method = endpoint.methods.first().map_or("GET", String::as_str);
    format!("curl -X {method} --location \"{base}{}\"", endpoint.path)
}

/// Show all endpoints of module: the Endpoints tool window, the endpoint at `path` selected.
pub(crate) fn reveal(
    workspace: &mut Workspace,
    path: &str,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(panel) = workspace.panel::<EndpointsToolWindow>(cx) else {
        return;
    };
    panel.update(cx, |panel, cx| {
        panel.selected = panel
            .java
            .read(cx)
            .spring
            .index
            .endpoints
            .iter()
            .find(|endpoint| endpoint.path == path)
            .cloned();
        cx.notify();
    });
    workspace.open_panel::<EndpointsToolWindow>(window, cx);
}

/// Opens `path` with its caret on `line`.
pub(crate) fn open_at(
    workspace: &mut Workspace,
    path: PathBuf,
    line: u32,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let open = workspace.open_abs_path(path, workspace::OpenOptions::default(), window, cx);
    cx.spawn_in(window, async move |_, cx| {
        let item = open.await?;
        if let Some(editor) = item.downcast::<editor::Editor>() {
            editor.update_in(cx, |editor, window, cx| {
                let point = text::Point::new(line, 0);
                editor.change_selections(
                    editor::SelectionEffects::scroll(editor::scroll::Autoscroll::center()),
                    window,
                    cx,
                    |selections| selections.select_ranges([point..point]),
                );
            })?;
        }
        anyhow::Ok(())
    })
    .detach_and_log_err(cx);
}

impl EventEmitter<PanelEvent> for EndpointsToolWindow {}

impl Focusable for EndpointsToolWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for EndpointsToolWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let shown = self.shown(cx);
        let loaded = self.java.read(cx).spring.index.loaded;
        let rows: Vec<TreeItem> = shown
            .iter()
            .map(|endpoint| self.row(endpoint, cx))
            .collect();
        let list = match (loaded, rows.is_empty()) {
            (false, _) => div()
                .p(px(10.))
                .text_color(tokens.text_secondary)
                .child("Loading endpoints…")
                .into_any_element(),
            (true, true) => div()
                .p(px(10.))
                .text_color(tokens.text_secondary)
                .child("No endpoints found.")
                .into_any_element(),
            (true, false) => {
                let items = match self.group_by_module {
                    true => vec![
                        TreeItem::new(module_name(self.java.read(cx)))
                            .colored_icon("nodes/module")
                            .default_open(true)
                            .children(rows),
                    ],
                    false => rows,
                };
                Tree::new("endpoints").children(items).into_any_element()
            }
        };
        let details = self
            .selected
            .as_ref()
            .filter(|_| self.side_panel)
            .filter(|selected| shown.contains(selected))
            .map(|selected| self.side_panel(selected, cx));
        v_flex()
            .id("endpoints-tool-window")
            .track_focus(&self.focus)
            .role(gpui::Role::Group)
            .aria_label("Endpoints")
            .size_full()
            .text_ui()
            .child(
                div()
                    .id("endpoints-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
            .children(details)
    }
}

impl Panel for EndpointsToolWindow {
    fn persistent_name() -> &'static str {
        "Endpoints"
    }

    fn panel_key() -> &'static str {
        "EndpointsToolWindow"
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
        Some("Endpoints")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(endpoints_tool_window::ToggleFocus)
    }

    // Spring's tool windows take 150–159
    fn activation_priority(&self) -> u32 {
        150
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
                Box::new(endpoints_tool_window::CompactListItems),
            ),
            item(
                "Group by Module",
                self.group_by_module,
                Box::new(endpoints_tool_window::GroupByModule),
            ),
            item(
                "Show From Libraries",
                self.from_libraries,
                Box::new(endpoints_tool_window::ShowFromLibraries),
            ),
            item(
                "Show From Tests",
                self.from_tests,
                Box::new(endpoints_tool_window::ShowFromTests),
            ),
            item(
                "Show Side Panel",
                self.side_panel,
                Box::new(endpoints_tool_window::ShowSidePanel),
            ),
        ]
    }
}

/// What the end-to-end tests read of the Spring index and the Endpoints tool window.
pub(crate) fn e2e_state(java: &JavaProject, workspace: Option<&Workspace>, cx: &App) -> Value {
    let index = &java.spring.index;
    let panel = workspace.and_then(|workspace| workspace.panel::<EndpointsToolWindow>(cx));
    json!({
        "loaded": index.loaded,
        "endpoints": index
            .endpoints
            .iter()
            .map(|endpoint| format!("{} {}", endpoint.methods_text(), endpoint.path).trim().to_owned())
            .collect::<Vec<_>>(),
        "beans": index.beans,
        "selected": panel.and_then(|panel| panel.read(cx).selected.as_ref().map(|e| e.path.clone())),
    })
}

#[cfg(test)]
mod tests {
    use super::parse_mapping;

    #[test]
    fn a_mapping_symbol_names_its_path_and_methods() {
        assert_eq!(
            parse_mapping("@/hello -- GET"),
            Some(("/hello".into(), vec!["GET".into()]))
        );
        assert_eq!(
            parse_mapping("@/items/{id} -- GET,POST - Accept: application/json"),
            Some(("/items/{id}".into(), vec!["GET".into(), "POST".into()]))
        );
        assert_eq!(parse_mapping("@/all"), Some(("/all".into(), vec![])));
        assert_eq!(
            parse_mapping("@+ 'helloController' (@RestController) HelloController"),
            None
        );
    }
}
