//! Spring's gutter icons in Java editors (IntelliJ's Spring line markers), from the Spring Boot
//! language server's index (`crate::endpoints::SpringIndex`): a request mapping's globe on its
//! handler, with Show all endpoints of module and Open in web browser; a bean's icon on a
//! `@Component` class or a `@Bean` method, which navigates to the autowired dependencies; the
//! autowired icon on an injection point, which navigates to its autowired candidates.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use editor::LineMarkers;
use gpui::{AnyElement, App, Entity, IntoElement as _, SharedString, Window, px};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use language::Buffer;
use serde_json::Value;
use util::ResultExt as _;

use crate::endpoints::Endpoint;
use crate::java_project::JavaProject;

pub(crate) fn init(cx: &mut App) {
    cx.set_global(LineMarkers(Rc::new(markers)));
}

/// A bean of the Spring LS's index, as the gutter uses it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Bean {
    pub name: String,
    pub type_name: String,
    pub supertypes: Vec<String>,
    pub place: Option<(PathBuf, u32)>,
    pub injection_points: Vec<InjectionPoint>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InjectionPoint {
    pub name: String,
    pub type_name: String,
    pub place: Option<(PathBuf, u32)>,
}

/// A location's file and line, as the index writes it.
fn place(location: &Value) -> Option<(PathBuf, u32)> {
    let uri = location.get("uri")?.as_str()?;
    let path = uri.parse::<lsp::Uri>().ok()?.to_file_path().ok()?;
    let line = location
        .pointer("/range/start/line")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    Some((path, u32::try_from(line).ok()?))
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The beans of the index's JSON.
pub(crate) fn beans(index: &[Value]) -> Vec<Bean> {
    index
        .iter()
        .map(|bean| Bean {
            name: text(bean, "name"),
            type_name: text(bean, "type"),
            supertypes: bean
                .get("supertypes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            place: bean.get("location").and_then(place),
            injection_points: bean
                .get("injectionPoints")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|point| InjectionPoint {
                    name: text(point, "name"),
                    type_name: text(point, "type"),
                    place: point.get("location").and_then(place),
                })
                .collect(),
        })
        .collect()
}

impl Bean {
    /// Whether the bean can be injected where `type_name` is wanted: its type or a supertype.
    pub fn fits(&self, type_name: &str) -> bool {
        self.type_name == type_name || self.supertypes.iter().any(|other| other == type_name)
    }

    fn simple_type(&self) -> &str {
        simple(&self.type_name)
    }
}

fn simple(type_name: &str) -> &str {
    type_name.rsplit('.').next().unwrap_or(type_name)
}

/// A marker of a file: its line and what it marks.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Marker {
    Mapping(Endpoint),
    Bean(Bean),
    Autowired(InjectionPoint),
}

/// The markers of `file` in the index, each on its line.
pub(crate) fn markers_of(
    endpoints: &[Endpoint],
    beans: &[Bean],
    file: &Path,
) -> Vec<(u32, Marker)> {
    let mut markers: Vec<(u32, Marker)> = endpoints
        .iter()
        .filter(|endpoint| endpoint.file == file)
        .map(|endpoint| (endpoint.line, Marker::Mapping(endpoint.clone())))
        .collect();
    for bean in beans {
        if let Some((path, line)) = &bean.place
            && path == file
        {
            markers.push((*line, Marker::Bean(bean.clone())));
        }
        for point in &bean.injection_points {
            if let Some((path, line)) = &point.place
                && path == file
            {
                markers.push((*line, Marker::Autowired(point.clone())));
            }
        }
    }
    markers
}

/// The row IntelliJ marks for an element whose range starts at `line`: its declaration, after
/// the annotations over it.
fn declaration_row(buffer: &language::BufferSnapshot, line: u32) -> u32 {
    let mut row = line;
    while row < buffer.max_point().row {
        let text: String = buffer
            .text_for_range(text::Point::new(row, 0)..text::Point::new(row, buffer.line_len(row)))
            .collect();
        let text = text.trim();
        // an annotation alone on its line, its arguments closed
        if !(text.starts_with('@') && text.matches('(').count() == text.matches(')').count()) {
            break;
        }
        row += 1;
    }
    row
}

fn markers(buffer: &Entity<Buffer>, _: &mut Window, cx: &mut App) -> Vec<(u32, AnyElement)> {
    let Some(path) = buffer
        .read(cx)
        .file()
        .and_then(|file| file.as_local())
        .map(|file| file.abs_path(cx))
    else {
        return Vec::new();
    };
    if path.extension().is_none_or(|extension| extension != "java") {
        return Vec::new();
    }
    let Some(java) = JavaProject::containing(&path, cx) else {
        return Vec::new();
    };
    let index = &java.read(cx).spring.index;
    let all = index.parsed_beans.clone();
    let found = markers_of(&index.endpoints, &all, &path);
    if found.is_empty() {
        return Vec::new();
    }
    let snapshot = buffer.read(cx).snapshot();
    let mut rows: Vec<(u32, AnyElement)> = Vec::new();
    for (line, marker) in found {
        let row = declaration_row(&snapshot, line);
        // one icon a row, the first found
        if rows.iter().any(|(taken, _)| *taken == row) {
            continue;
        }
        let element = icon(&java, &all, marker, row);
        rows.push((row, element));
    }
    rows
}

/// A marker's icon, its popup on a click.
fn icon(java: &Entity<JavaProject>, beans: &Rc<[Bean]>, marker: Marker, row: u32) -> AnyElement {
    let (icon, tooltip) = match &marker {
        Marker::Mapping(_) => ("gutter/web", "Request mapping"),
        Marker::Bean(_) => (
            "gutter/springBean",
            "Navigate to the autowired dependencies",
        ),
        Marker::Autowired(_) => ("gutter/springAutowired", "Navigate to autowired candidates"),
    };
    let id = SharedString::from(format!("spring-marker-{row}"));
    let (java, beans) = (java.clone(), beans.clone());
    let button = spork_workbench::ActionButton::new(id, icon)
        .colored()
        .tooltip(tooltip);
    // the gutter's 16 px icon, not a toolbar's button
    gpui::Styled::size(button, px(16.))
        .dropdown_menu(move |menu, _, cx| match &marker {
            Marker::Mapping(endpoint) => mapping_menu(menu, &java, endpoint, cx),
            Marker::Bean(bean) => {
                let using: Vec<(String, (PathBuf, u32))> = beans
                    .iter()
                    .flat_map(|other| {
                        other.injection_points.iter().filter_map(move |point| {
                            bean.fits(&point.type_name)
                                .then(|| {
                                    Some((
                                        format!("{} in {}", point.name, other.simple_type()),
                                        point.place.clone()?,
                                    ))
                                })
                                .flatten()
                        })
                    })
                    .collect();
                let workspace = java.read(cx).workspace.clone();
                places_menu(
                    menu,
                    "Autowired Dependencies",
                    "No usages found",
                    using,
                    workspace,
                )
            }
            Marker::Autowired(point) => {
                let candidates: Vec<(String, (PathBuf, u32))> = beans
                    .iter()
                    .filter(|bean| bean.fits(&point.type_name))
                    .filter_map(|bean| {
                        Some((
                            format!("{} ({})", bean.name, bean.simple_type()),
                            bean.place.clone()?,
                        ))
                    })
                    .collect();
                let workspace = java.read(cx).workspace.clone();
                places_menu(
                    menu,
                    "Choose Autowired Candidate",
                    "No matching autowired candidates found",
                    candidates,
                    workspace,
                )
            }
        })
        .into_any_element()
}

/// A request mapping's popup: Open in HTTP Client, Navigate to related views for a handler that
/// renders a view, Show all endpoints of module, and Open in web browser for a GET.
fn mapping_menu(
    menu: gpui_kit::component::menu::PopupMenu,
    java: &Entity<JavaProject>,
    endpoint: &Endpoint,
    cx: &App,
) -> gpui_kit::component::menu::PopupMenu {
    let workspace = java.read(cx).workspace.clone();
    let port = java
        .read(cx)
        .root
        .as_deref()
        .and_then(server_port)
        .unwrap_or(8080);
    let request = crate::endpoints::request_line(endpoint, &format!("http://localhost:{port}"));
    let menu = {
        let workspace = workspace.clone();
        menu.item(
            PopupMenuItem::new("Open in HTTP Client").on_click(move |_, window, cx| {
                let Some((method, url)) = request.split_once(' ') else {
                    return;
                };
                workspace
                    .update(cx, |workspace, cx| {
                        spork_http::open_in_http_client(workspace, method, url, window, cx)
                    })
                    .log_err();
            }),
        )
    };
    let views = java
        .read(cx)
        .root
        .as_deref()
        .map(|root| related_views(&endpoint.file, endpoint.line, root))
        .unwrap_or_default();
    let menu = match views.into_iter().next() {
        Some(view) => {
            let workspace = workspace.clone();
            menu.item(PopupMenuItem::new("Navigate to related views").on_click(
                move |_, window, cx| {
                    let view = view.clone();
                    workspace
                        .update(cx, |workspace, cx| {
                            crate::endpoints::open_at(workspace, view, 0, window, cx)
                        })
                        .log_err();
                },
            ))
        }
        None => menu,
    };
    let path = endpoint.path.clone();
    let menu = menu.item(PopupMenuItem::new("Show all endpoints of module").on_click(
        move |_, window, cx| {
            let path = path.clone();
            workspace
                .update(cx, |workspace, cx| {
                    crate::endpoints::reveal(workspace, &path, window, cx)
                })
                .log_err();
        },
    ));
    let browsable = endpoint.methods.is_empty() || endpoint.methods.iter().any(|m| m == "GET");
    if !browsable || endpoint.path.contains('{') {
        return menu;
    }
    menu.link(
        "Open in web browser",
        format!("http://localhost:{port}{}", endpoint.path),
    )
}

/// The views a request mapping's handler renders (IntelliJ's "Related views"): the templates
/// named by the view names it returns, in a `@Controller` that isn't a `@RestController`.
// shortcut: the handler's text, its `return "view"` and `new ModelAndView("view")`, and Spring
// Boot's template folder, where IntelliJ resolves the views with the configured view resolvers;
// the first view goes, where IntelliJ lists several
pub(crate) fn related_views(file: &Path, line: u32, root: &Path) -> Vec<PathBuf> {
    static VIEW: std::sync::LazyLock<Option<regex::Regex>> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"(?:return\s+|ModelAndView\(\s*)"([^"]+)""#).log_err()
    });
    let Ok(text) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    if text.contains("@RestController") {
        return Vec::new();
    }
    let mut body = String::new();
    let mut depth = 0;
    let mut opened = false;
    for line in text.lines().skip(line as usize) {
        body.push_str(line);
        body.push('\n');
        for c in line.chars() {
            match c {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if opened && depth <= 0 {
            break;
        }
    }
    let templates = root.join("src/main/resources/templates");
    let Some(view) = VIEW.as_ref() else {
        return Vec::new();
    };
    view.captures_iter(&body)
        .filter_map(|captures| Some(captures.get(1)?.as_str().to_owned()))
        .filter(|name| !name.starts_with("redirect:") && !name.starts_with("forward:"))
        .filter_map(|name| {
            ["html", "ftlh", "mustache", "tpl"]
                .iter()
                .map(|extension| templates.join(format!("{name}.{extension}")))
                .find(|path| path.is_file())
        })
        .collect()
}

/// The application's `server.port`, as its `application.properties` sets it.
pub(crate) fn server_port(root: &Path) -> Option<u16> {
    let text =
        std::fs::read_to_string(root.join("src/main/resources/application.properties")).ok()?;
    text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "server.port").then(|| value.trim().parse().ok())?
    })
}

/// A popup of places to go to, under its title; `empty` without any.
fn places_menu(
    menu: gpui_kit::component::menu::PopupMenu,
    title: &'static str,
    empty: &'static str,
    places: Vec<(String, (PathBuf, u32))>,
    workspace: gpui::WeakEntity<workspace::Workspace>,
) -> gpui_kit::component::menu::PopupMenu {
    let mut menu = menu.label(title);
    if places.is_empty() {
        return menu.item(PopupMenuItem::new(empty).disabled(true));
    }
    for (label, (path, line)) in places {
        let workspace = workspace.clone();
        menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
            let path = path.clone();
            workspace
                .update(cx, |workspace, cx| {
                    crate::endpoints::open_at(workspace, path, line, window, cx)
                })
                .log_err();
        }));
    }
    menu
}

#[cfg(test)]
mod tests {
    use super::{Marker, beans, markers_of, related_views};
    use crate::endpoints::Endpoint;
    use serde_json::json;
    use std::path::{Path, PathBuf};

    #[test]
    fn a_files_markers_come_from_the_index() {
        let root = if cfg!(windows) { "C:/p" } else { "/p" };
        let uri = |file: &str| format!("file://{root}/{file}");
        let index = vec![
            json!({
                "name": "helloController",
                "type": "com.example.HelloController",
                "location": {"uri": uri("HelloController.java"), "range": {"start": {"line": 6, "character": 0}}},
                "injectionPoints": [{
                    "name": "service",
                    "type": "com.example.GreetingService",
                    "location": {"uri": uri("HelloController.java"), "range": {"start": {"line": 9, "character": 4}}},
                }],
            }),
            json!({
                "name": "greetingService",
                "type": "com.example.GreetingService",
                "supertypes": ["java.lang.Object"],
                "location": {"uri": uri("GreetingService.java"), "range": {"start": {"line": 3, "character": 0}}},
            }),
        ];
        let all = beans(&index);
        assert!(all[1].fits("com.example.GreetingService"));
        let file = PathBuf::from(format!("{root}/HelloController.java"));
        let endpoint = Endpoint {
            path: "/hello".into(),
            methods: vec!["GET".into()],
            file: file.clone(),
            line: 11,
        };
        let found = markers_of(std::slice::from_ref(&endpoint), &all, &file);
        let lines: Vec<u32> = found.iter().map(|(line, _)| *line).collect();
        assert_eq!(lines, [11, 6, 9]);
        assert!(matches!(found[2].1, Marker::Autowired(ref point) if point.name == "service"));
        assert!(markers_of(&[], &all, Path::new("/elsewhere.java")).is_empty());
    }

    #[test]
    fn a_controllers_handler_names_its_view() -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        let templates = root.path().join("src/main/resources/templates");
        std::fs::create_dir_all(&templates)?;
        std::fs::write(templates.join("greeting.html"), "<p>Hi</p>")?;
        let file = root.path().join("GreetingController.java");
        std::fs::write(
            &file,
            "@Controller\nclass GreetingController {\n    @GetMapping(\"/greeting\")\n    String greeting() {\n        return \"greeting\";\n    }\n\n    @GetMapping(\"/away\")\n    String away() {\n        return \"redirect:/greeting\";\n    }\n}\n",
        )?;
        assert_eq!(
            related_views(&file, 2, root.path()),
            [templates.join("greeting.html")]
        );
        assert!(related_views(&file, 7, root.path()).is_empty());
        Ok(())
    }
}
