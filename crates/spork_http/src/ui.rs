//! The HTTP Client in the IDE: each request's run icon in the gutter, the `.http` editor's
//! toolbar (Run All Requests in File, "Run with:" and its environments), method and header
//! completion, the runs in Services' "HTTP Request" node with their responses, New ▸ HTTP
//! Request, and Endpoints' Open in HTTP Client.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use collections::HashMap;
use editor::{CompletionProvider, Editor};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, EntityId, EventEmitter, Global,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, TaskExt as _, Window, div,
    px,
};
use gpui_kit::component::h_flex;
use gpui_kit::component::menu::PopupMenuItem;
use language::{Buffer, ToOffset as _};
use serde_json::{Value, json};
use spork_workbench::tokens::{Tokens, Typography as _};
use util::ResultExt as _;
use workspace::item::ItemHandle;
use workspace::{Pane, ToolbarItemEvent, ToolbarItemLocation, ToolbarItemView, Workspace};

use crate::{LANGUAGE, Request, Response};

/// The headers completion offers, IntelliJ's common request headers.
const HEADERS: [&str; 24] = [
    "Accept",
    "Accept-Charset",
    "Accept-Encoding",
    "Accept-Language",
    "Authorization",
    "Cache-Control",
    "Connection",
    "Content-Encoding",
    "Content-Length",
    "Content-Type",
    "Cookie",
    "Date",
    "Expect",
    "Forwarded",
    "From",
    "Host",
    "If-Match",
    "If-Modified-Since",
    "If-None-Match",
    "Origin",
    "Pragma",
    "Range",
    "Referer",
    "User-Agent",
];

/// "No Environment", IntelliJ's choice without one.
const NO_ENVIRONMENT: &str = "No Environment";

pub(crate) fn init(cx: &mut App) {
    // the gutter's markers: the ones set before, then each request's run icon
    let before = cx
        .try_global::<editor::LineMarkers>()
        .map(|markers| markers.0.clone());
    cx.set_global(editor::LineMarkers(Rc::new(move |buffer, window, cx| {
        let mut markers = before
            .as_ref()
            .map(|before| before(buffer, window, cx))
            .unwrap_or_default();
        markers.extend(run_markers(buffer, cx));
        markers
    })));
    cx.observe_new(|editor: &mut Editor, _, cx| {
        let is_http = editor
            .buffer()
            .read(cx)
            .as_singleton()
            .is_some_and(|buffer| is_http_buffer(buffer.read(cx)));
        if is_http {
            editor.set_completion_provider(Some(Rc::new(HttpCompletions)));
        }
    })
    .detach();
    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        let Some(window) = window else { return };
        let requests = cx.new(|_| Requests::default());
        let id = cx.entity_id();
        cx.default_global::<Registry>()
            .0
            .insert(id, requests.clone());
        for pane in workspace.panes().to_vec() {
            add_toolbar(&pane, &requests, window, cx);
        }
        cx.subscribe_in(&cx.entity(), window, move |_, _, event, window, cx| {
            if let workspace::Event::PaneAdded(pane) = event {
                add_toolbar(pane, &requests, window, cx);
            }
        })
        .detach();
        cx.on_release(move |_, cx| {
            cx.default_global::<Registry>().0.remove(&id);
        })
        .detach();
    })
    .detach();
    spork_run::services::register_contributor(
        |workspace, cx| {
            let requests = Requests::of(workspace, cx)?;
            let runs = &requests.read(cx).runs;
            (!runs.is_empty()).then(|| spork_run::services::ContributedGroup {
                title: LANGUAGE.into(),
                icon: "fileTypes/http",
                services: runs
                    .iter()
                    .map(|run| spork_run::services::ContributedService {
                        id: run.id.clone(),
                        label: run.name.clone(),
                        icon: match run.ok {
                            true => "fileTypes/http",
                            false => "status/error",
                        },
                        view: run.view.clone().into(),
                    })
                    .collect(),
            })
        },
        cx,
    );
    project_panel::add_new_items(
        |folder| {
            let folder = folder.to_path_buf();
            vec![project_panel::NewItem {
                label: "HTTP Request".into(),
                run: Rc::new(move |window, cx| new_request_file(folder.clone(), window, cx)),
                after: true,
            }]
        },
        cx,
    );
}

fn is_http_buffer(buffer: &Buffer) -> bool {
    buffer
        .language()
        .is_some_and(|language| language.name().as_ref() == LANGUAGE)
        || buffer
            .file()
            .is_some_and(|file| matches!(file.path().extension(), Some("http") | Some("rest")))
}

/// The workspaces' HTTP Client state, by workspace.
#[derive(Default)]
struct Registry(HashMap<EntityId, Entity<Requests>>);

impl Global for Registry {}

/// A workspace's HTTP Client: the runs Services lists and each file's environment.
#[derive(Default)]
pub(crate) struct Requests {
    runs: Vec<Run>,
    /// The environment each file runs with, `None` for No Environment.
    environments: HashMap<PathBuf, Option<String>>,
    next_id: usize,
}

struct Run {
    id: SharedString,
    name: SharedString,
    ok: bool,
    view: Entity<ResponseView>,
}

impl Requests {
    fn of(workspace: &Workspace, cx: &App) -> Option<Entity<Requests>> {
        cx.try_global::<Registry>()?
            .0
            .get(&workspace.weak_handle().entity_id())
            .cloned()
    }
}

/// The project folder holding `path`, else its own folder.
fn root_of(workspace: &Workspace, path: &Path, cx: &App) -> PathBuf {
    workspace
        .project()
        .read(cx)
        .visible_worktrees(cx)
        .map(|worktree| worktree.read(cx).abs_path().to_path_buf())
        .find(|root| path.starts_with(root))
        .or_else(|| path.parent().map(Path::to_path_buf))
        .unwrap_or_default()
}

/// The variables a request of `path` runs with: the chosen environment's, the file's over them.
fn variables(
    workspace: &Workspace,
    path: &Path,
    file_variables: &[(String, String)],
    cx: &App,
) -> BTreeMap<String, String> {
    let root = root_of(workspace, path, cx);
    let chosen = Requests::of(workspace, cx)
        .and_then(|requests| requests.read(cx).environments.get(path).cloned().flatten());
    let mut variables = chosen
        .and_then(|name| crate::environments(path, &root).remove(&name))
        .unwrap_or_default();
    variables.extend(file_variables.iter().cloned());
    variables
}

/// Runs the requests of `buffer` whose run icons are on `rows` (all of them for Run All), each
/// shown in Services and its response saved and linked under it.
pub(crate) fn run_requests(
    workspace: &Entity<Workspace>,
    buffer: &Entity<Buffer>,
    rows: Option<Vec<u32>>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(path) = buffer
        .read(cx)
        .file()
        .and_then(|file| file.as_local())
        .map(|file| file.abs_path(cx))
    else {
        return;
    };
    let text = buffer.read(cx).text();
    let (requests, file_variables) = crate::parse(&text);
    let file_name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let variables = variables(workspace.read(cx), &path, &file_variables, cx);
    let root = root_of(workspace.read(cx), &path, cx);
    let folder = spork_run::configurations::workspace_folder(&root).join("httpRequests");
    let chosen: Vec<(SharedString, Request)> = requests
        .into_iter()
        .enumerate()
        .filter(|(_, request)| rows.as_ref().is_none_or(|rows| rows.contains(&request.row)))
        .map(|(ix, request)| (request.run_name(&file_name, ix + 1).into(), request))
        .collect();
    run(
        workspace,
        chosen,
        variables,
        folder,
        Some(buffer.clone()),
        window,
        cx,
    );
}

/// Endpoints' HTTP Client tab's Submit Request: `text`'s requests run as a `.http` file's are,
/// their responses in Services.
pub fn submit_requests(
    workspace: &Entity<Workspace>,
    text: &str,
    root: &Path,
    window: &mut Window,
    cx: &mut App,
) {
    let (requests, variables) = crate::parse(text);
    let chosen = requests
        .into_iter()
        .enumerate()
        .map(|(ix, request)| (request.run_name("Endpoints", ix + 1).into(), request))
        .collect();
    let folder = spork_run::configurations::workspace_folder(root).join("httpRequests");
    run(
        workspace,
        chosen,
        variables.into_iter().collect(),
        folder,
        None,
        window,
        cx,
    );
}

/// Sends the requests in turn, each run shown in Services; with their file's `buffer`, each saved
/// response linked under its request.
fn run(
    workspace: &Entity<Workspace>,
    chosen: Vec<(SharedString, Request)>,
    variables: BTreeMap<String, String>,
    folder: PathBuf,
    buffer: Option<Entity<Buffer>>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(requests) = Requests::of(workspace.read(cx), cx) else {
        return;
    };
    let http = cx.http_client();
    let workspace = workspace.downgrade();
    window
        .spawn(cx, async move |cx| {
            // in turn, as IntelliJ's Run All runs them; their links go in from the last up, so
            // the rows of those before stay as parsed
            let mut links = Vec::new();
            for (name, request) in chosen {
                let request_line = format!(
                    "{} {}",
                    request.method,
                    crate::substitute(&request.url, &variables)
                );
                let response = crate::send(http.clone(), &request, &variables).await;
                let saved = match &response {
                    Ok(response) => save_response(&folder, response).await.log_err(),
                    Err(_) => None,
                };
                let response = response.map_err(|error| SharedString::from(format!("{error:#}")));
                let text = crate::console_text(&request_line, &response, saved.as_deref());
                let ok = response.is_ok();
                if let Some(saved) = saved {
                    links.push((request.clone(), saved));
                }
                let id = requests.update(cx, |requests, cx| {
                    requests.next_id += 1;
                    let id = SharedString::from(format!("http-request-{}", requests.next_id));
                    let view = cx.new(|_| ResponseView {
                        text: text.into(),
                        status: response.as_ref().ok().map(|response| response.status),
                    });
                    requests.runs.push(Run {
                        id: id.clone(),
                        name: name.clone(),
                        ok,
                        view,
                    });
                    cx.notify();
                    id
                });
                workspace.update_in(cx, |workspace, window, cx| {
                    spork_run::services::show_contributed(workspace, Some(id), window, cx)
                })?;
            }
            let Some(buffer) = buffer else {
                return Ok(());
            };
            buffer.update(cx, |buffer, cx| {
                for (request, saved) in links.iter().rev() {
                    link_response(buffer, request, saved, cx);
                }
            });
            let save = workspace.update(cx, |workspace, cx| {
                workspace
                    .project()
                    .update(cx, |project, cx| project.save_buffer(buffer.clone(), cx))
            })?;
            save.await
        })
        .detach_and_log_err(cx);
}

/// Writes a response's body into the responses folder; its file's name.
async fn save_response(folder: &Path, response: &Response) -> anyhow::Result<String> {
    std::fs::create_dir_all(folder)?;
    let path = crate::response_file_name(folder, response);
    std::fs::write(&path, &response.body)?;
    Ok(path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default())
}

/// Puts `<> saved` under the request, newest first, after a blank line as IntelliJ writes it.
fn link_response(buffer: &mut Buffer, request: &Request, saved: &str, cx: &mut Context<Buffer>) {
    let edit = match request.responses.first() {
        Some((row, _)) => {
            let at = text::Point::new(*row, 0);
            (at..at, format!("<> {saved}\n"))
        }
        None => {
            let at = text::Point::new(request.last_row, buffer.line_len(request.last_row));
            (at..at, format!("\n\n<> {saved}"))
        }
    };
    buffer.edit([edit], None, cx);
}

/// The run icons of a `.http` buffer's requests (IntelliJ's `AllIcons.Actions.Execute`).
fn run_markers(buffer: &Entity<Buffer>, cx: &mut App) -> Vec<(u32, AnyElement)> {
    if !is_http_buffer(buffer.read(cx)) {
        return Vec::new();
    }
    // shortcut: the file parsed again each layout; keep its requests by version once files
    // grow large
    let (requests, _) = crate::parse(&buffer.read(cx).text());
    requests
        .into_iter()
        .map(|request| {
            let (buffer, row) = (buffer.clone(), request.row);
            let id = SharedString::from(format!("http-run-{row}"));
            let button = spork_workbench::ActionButton::new(id, "run/run")
                .colored()
                .tooltip("Run HTTP Request")
                .on_click(move |_, window, cx| {
                    if let Some(workspace) = Workspace::for_window(window, cx) {
                        run_requests(&workspace, &buffer, Some(vec![row]), window, cx);
                    }
                });
            (row, gpui::Styled::size(button, px(16.)).into_any_element())
        })
        .collect()
}

/// A run's response, as IntelliJ's HTTP Client writes it in its console.
pub(crate) struct ResponseView {
    text: SharedString,
    status: Option<u16>,
}

impl Render for ResponseView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        div()
            .id("http-response")
            .role(Role::Group)
            .aria_label("Response")
            .size_full()
            .p(px(8.))
            .overflow_y_scroll()
            .bg(tokens.island)
            .text_color(tokens.text)
            .font_family("JetBrains Mono")
            .text_ui_small()
            .children(self.text.lines().enumerate().map(|(ix, line)| {
                div()
                    .id(("http-response-line", ix))
                    .role(Role::Label)
                    .aria_label(SharedString::from(line.to_owned()))
                    .min_h(px(16.))
                    .whitespace_nowrap()
                    .child(line.to_owned())
            }))
    }
}

fn add_toolbar(
    pane: &Entity<Pane>,
    requests: &Entity<Requests>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let workspace = cx.entity().downgrade();
    let toolbar = cx.new(|cx| HttpToolbar {
        workspace,
        requests: requests.clone(),
        buffer: None,
        _subscription: cx.observe(requests, |_, _, cx| cx.notify()),
    });
    pane.update(cx, |pane, cx| {
        pane.toolbar()
            .update(cx, |bar, cx| bar.add_item(toolbar, window, cx))
    });
}

/// The `.http` editor's toolbar: Run All Requests in File and "Run with:" the environments.
struct HttpToolbar {
    workspace: gpui::WeakEntity<Workspace>,
    requests: Entity<Requests>,
    buffer: Option<Entity<Buffer>>,
    _subscription: Subscription,
}

impl EventEmitter<ToolbarItemEvent> for HttpToolbar {}

impl ToolbarItemView for HttpToolbar {
    fn set_active_pane_item(
        &mut self,
        item: Option<&dyn ItemHandle>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> ToolbarItemLocation {
        self.buffer = item
            .and_then(|item| item.act_as::<Editor>(cx))
            .and_then(|editor| editor.read(cx).buffer().read(cx).as_singleton())
            .filter(|buffer| is_http_buffer(buffer.read(cx)));
        match self.buffer {
            Some(_) => ToolbarItemLocation::PrimaryLeft,
            None => ToolbarItemLocation::Hidden,
        }
    }
}

impl Render for HttpToolbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (Some(buffer), Some(workspace)) = (self.buffer.clone(), self.workspace.upgrade())
        else {
            return div().into_any_element();
        };
        let path = buffer
            .read(cx)
            .file()
            .and_then(|file| file.as_local())
            .map(|file| file.abs_path(cx));
        let (names, chosen) = match &path {
            Some(path) => {
                let root = root_of(workspace.read(cx), path, cx);
                let names: Vec<String> = crate::environments(path, &root).into_keys().collect();
                let chosen = self
                    .requests
                    .read(cx)
                    .environments
                    .get(path)
                    .cloned()
                    .flatten();
                (names, chosen)
            }
            None => (Vec::new(), None),
        };
        let run_all = spork_workbench::ActionButton::new("http-run-all", "actions/runAll")
            .tooltip("Run All Requests in File")
            .on_click(move |_, window, cx| run_requests(&workspace, &buffer, None, window, cx));
        let requests = self.requests.clone();
        let combo = spork_workbench::controls::dropdown(
            spork_workbench::controls::ComboBox::new(
                "http-environment",
                chosen.unwrap_or_else(|| NO_ENVIRONMENT.to_owned()),
            )
            .aria_label("Run with:"),
            false,
            move |mut menu, _, _| {
                let choices = std::iter::once(None).chain(names.iter().cloned().map(Some));
                for choice in choices {
                    let (requests, path) = (requests.clone(), path.clone());
                    let label = choice.clone().unwrap_or_else(|| NO_ENVIRONMENT.to_owned());
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                        let Some(path) = path.clone() else { return };
                        let choice = choice.clone();
                        requests.update(cx, |requests, cx| {
                            requests.environments.insert(path, choice);
                            cx.notify();
                        });
                    }));
                }
                menu
            },
        );
        h_flex()
            .id("http-toolbar")
            .role(Role::Toolbar)
            .aria_label("HTTP Client")
            .gap(px(8.))
            .child(run_all)
            .child(spork_workbench::controls::form_label("Run with:"))
            .child(div().w(px(160.)).child(combo))
            .into_any_element()
    }
}

/// Completion in a `.http` file: the methods where a request starts, the headers after its
/// request line.
struct HttpCompletions;

impl CompletionProvider for HttpCompletions {
    fn completions(
        &self,
        buffer: &Entity<Buffer>,
        buffer_position: language::Anchor,
        _: editor::CompletionContext,
        _: &mut Window,
        cx: &mut Context<Editor>,
    ) -> Task<anyhow::Result<Vec<project::CompletionResponse>>> {
        let buffer = buffer.read(cx);
        let offset = buffer_position.to_offset(buffer);
        let typed = buffer
            .reversed_chars_at(offset)
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .count();
        let start = buffer.anchor_before(offset - typed);
        let row = buffer.offset_to_point(offset).row;
        // IntelliJ completes a line's first word: a method, or a header's name
        if offset - typed != buffer.point_to_offset(text::Point::new(row, 0)) {
            return Task::ready(Ok(Vec::new()));
        }
        let (requests, _) = crate::parse(&buffer.text());
        // in a request's headers: after its request line, no blank line before the caret's
        let blank = |line: u32| {
            buffer
                .text_for_range(
                    text::Point::new(line, 0)..text::Point::new(line, buffer.line_len(line)),
                )
                .collect::<String>()
                .trim()
                .is_empty()
        };
        let in_headers = requests
            .iter()
            .rev()
            .find(|request| request.row < row)
            .is_some_and(|request| (request.row + 1..row).all(|line| !blank(line)));
        let items: Vec<(String, String)> = match in_headers {
            true => HEADERS
                .iter()
                .map(|header| (header.to_string(), format!("{header}: ")))
                .collect(),
            false => crate::METHODS
                .iter()
                .map(|method| (method.to_string(), format!("{method} ")))
                .collect(),
        };
        let completions = items
            .into_iter()
            .map(|(label, new_text)| project::Completion {
                replace_range: start..buffer_position,
                label: language::CodeLabel::plain(label, None),
                new_text,
                documentation: None,
                source: project::CompletionSource::Custom,
                icon_path: None,
                icon_color: None,
                match_start: None,
                snippet_deduplication_key: None,
                insert_text_mode: None,
                confirm: None,
                group: None,
            })
            .collect();
        Task::ready(Ok(vec![project::CompletionResponse {
            completions,
            display_options: project::CompletionDisplayOptions::default(),
            is_incomplete: false,
        }]))
    }

    fn is_completion_trigger(
        &self,
        buffer: &Entity<Buffer>,
        position: language::Anchor,
        text: &str,
        _: bool,
        cx: &mut Context<Editor>,
    ) -> bool {
        let buffer = buffer.read(cx);
        let offset = position.to_offset(buffer);
        let row = buffer.offset_to_point(offset).row;
        let line_start = buffer.point_to_offset(text::Point::new(row, 0));
        let typed: String = buffer.text_for_range(line_start..offset).collect();
        text.chars().all(|c| c.is_ascii_alphabetic())
            && typed.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    }
}

/// IntelliJ's HTTP Request file template, its example request pointed at the local server.
const TEMPLATE: &str = "### GET request to example server\nGET http://localhost:8080/\n\n###\n";

/// New ▸ HTTP Request: IntelliJ's "New HTTP Request" name popup, then `<name>.http` from its
/// template in `folder`, opened.
fn new_request_file(folder: PathBuf, window: &mut Window, cx: &mut App) {
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::component::input::{InputEvent, InputState};
    let input = cx.new(|cx| InputState::new(window, cx));
    let create = {
        let input = input.clone();
        move |window: &mut Window, cx: &mut App| {
            let name = input.read(cx).value().trim().to_owned();
            if name.is_empty() {
                return;
            }
            window.close_dialog(cx);
            let name = match name.ends_with(".http") {
                true => name,
                false => format!("{name}.http"),
            };
            let path = folder.join(name);
            if !path.exists() && std::fs::write(&path, TEMPLATE).log_err().is_none() {
                return;
            }
            if let Some(workspace) = Workspace::for_window(window, cx) {
                workspace.update(cx, |workspace, cx| {
                    workspace
                        .open_abs_path(path, workspace::OpenOptions::default(), window, cx)
                        .detach_and_log_err(cx);
                });
            }
        }
    };
    window
        .subscribe(&input, cx, move |_, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                create(window, cx);
            }
        })
        .detach();
    let field = input.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(spork_workbench::controls::dialog_title("New HTTP Request"))
            .on_ok(|_, _, _| false)
            .w(px(360.))
            .centered()
            .overlay(false)
            .close_button(false)
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(spork_workbench::controls::form_label("Name:"))
                    .child(
                        div()
                            .flex_1()
                            .child(spork_workbench::TextField::new(&field).aria_label("Name:")),
                    ),
            )
    });
    input.update(cx, |input, cx| input.focus(window, cx));
}

/// Endpoints' Open in HTTP Client: the endpoint's request added to the scratch
/// `generated-requests.http` (IntelliJ's), which opens on it.
pub fn open_in_http_client(
    workspace: &mut Workspace,
    method: &str,
    url: &str,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let path = project_panel::scratches_folder().join("generated-requests.http");
    let request = format!("###\n{method} {url}\n");
    let written = std::fs::create_dir_all(project_panel::scratches_folder())
        .and_then(|()| {
            let existing = std::fs::read_to_string(&path).unwrap_or_default();
            let text = match existing.is_empty() {
                true => request.clone(),
                false => format!("{}\n\n{request}", existing.trim_end()),
            };
            std::fs::write(&path, text)
        })
        .log_err();
    if written.is_none() {
        return;
    }
    let open = workspace.open_abs_path(path, workspace::OpenOptions::default(), window, cx);
    cx.spawn_in(window, async move |_, cx| {
        let item = open.await?;
        if let Some(editor) = item.downcast::<Editor>() {
            editor.update_in(cx, |editor, window, cx| {
                editor.move_to_end(&Default::default(), window, cx);
            })?;
        }
        anyhow::Ok(())
    })
    .detach_and_log_err(cx);
}

/// What the end-to-end tests read of the HTTP Client: each run's name and console text.
pub fn e2e_state(workspace: &Workspace, cx: &App) -> Value {
    let Some(requests) = Requests::of(workspace, cx) else {
        return Value::Null;
    };
    let requests = requests.read(cx);
    json!({
        "runs": requests
            .runs
            .iter()
            .map(|run| {
                let view = run.view.read(cx);
                json!({"name": run.name.as_ref(), "status": view.status, "text": view.text.as_ref()})
            })
            .collect::<Vec<_>>(),
        "environments": requests
            .environments
            .iter()
            .map(|(path, chosen)| (path.display().to_string(), json!(chosen)))
            .collect::<serde_json::Map<_, _>>(),
    })
}
