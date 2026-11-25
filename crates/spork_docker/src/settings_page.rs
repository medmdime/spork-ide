//! Settings › Build, Execution, Deployment › Docker (IntelliJ's `DockerConnectionsListConfigurable`):
//! the connections in a list with Add and Remove, the selected one's form beside it (its name and
//! "Connect to Docker daemon with:" choices) and the connection check under it ("Connection
//! successful" or "Cannot connect: …"). The form is also the New Docker Connection dialog's.
//! Edits wait for the Settings dialog's Apply. Left out: SSH, WSL, Minikube and Podman's own
//! choices (a Podman socket is a Unix socket), the TCP socket's Certificates folder (no TLS yet),
//! the virtual machine path mappings and the Tools page's executables.

use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Value, json};
use spork_workbench::{ActionButton, Tokens, Typography as _};

use crate::connections::{self, Connection, ConnectionStore, Kind};
use crate::engine::Engine;

/// What the check of a connection found.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Connecting,
    Successful,
    Failed(SharedString),
}

impl Status {
    pub fn text(&self) -> SharedString {
        match self {
            Status::Connecting => "Connecting…".into(),
            Status::Successful => "Connection successful".into(),
            Status::Failed(error) => format!("Cannot connect: {error}").into(),
        }
    }
}

/// Tries a connection: the daemon's ping and API version, as IntelliJ's check does.
pub fn check(connection: &Connection, cx: &App) -> Task<Status> {
    let url = connections::api_url(connection);
    cx.background_spawn(async move {
        match url.and_then(|url| Engine::connect(&url)) {
            Ok(_) => Status::Successful,
            Err(error) => Status::Failed(format!("{error:#}").into()),
        }
    })
}

/// One connection's form: its name and how it reaches the daemon, checked as it changes.
pub struct ConnectionForm {
    connection: Connection,
    name: Entity<InputState>,
    socket: Entity<InputState>,
    url: Entity<InputState>,
    status: Status,
    checking: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Connection> for ConnectionForm {}

impl ConnectionForm {
    pub fn new(connection: Connection, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let field = |text: &str, window: &mut Window, cx: &mut Context<Self>| {
            let text = text.to_owned();
            cx.new(|cx| {
                let mut input = InputState::new(window, cx);
                input.set_value(text, window, cx);
                input
            })
        };
        let name = field(&connection.name, window, cx);
        let socket = field(&connection.socket, window, cx);
        let url = field(&connection.url, window, cx);
        let edits = |input: &Entity<InputState>, cx: &mut Context<Self>| {
            cx.subscribe(input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.edited(cx);
                }
            })
        };
        let _subscriptions = vec![edits(&name, cx), edits(&socket, cx), edits(&url, cx)];
        let mut form = Self {
            connection,
            name,
            socket,
            url,
            status: Status::Connecting,
            checking: Task::ready(()),
            _subscriptions,
        };
        form.recheck(Duration::ZERO, cx);
        form
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn focus_name(&self, window: &mut Window, cx: &mut App) {
        self.name.update(cx, |name, cx| name.focus(window, cx));
    }

    fn edited(&mut self, cx: &mut Context<Self>) {
        let edited = Connection {
            name: self.name.read(cx).value().to_string(),
            kind: self.connection.kind,
            socket: self.socket.read(cx).value().to_string(),
            url: self.url.read(cx).value().to_string(),
        };
        if edited == self.connection {
            return;
        }
        let reach_changed =
            (&edited.socket, &edited.url) != (&self.connection.socket, &self.connection.url);
        self.connection = edited;
        if reach_changed {
            // one check once the typing stops, not one a keystroke
            self.recheck(Duration::from_millis(300), cx);
        }
        cx.emit(self.connection.clone());
        cx.notify();
    }

    fn set_kind(&mut self, kind: Kind, cx: &mut Context<Self>) {
        if self.connection.kind == kind {
            return;
        }
        self.connection.kind = kind;
        self.recheck(Duration::ZERO, cx);
        cx.emit(self.connection.clone());
        cx.notify();
    }

    fn recheck(&mut self, delay: Duration, cx: &mut Context<Self>) {
        self.status = Status::Connecting;
        let connection = self.connection.clone();
        self.checking = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            // the form may be gone: nothing to show the result in
            let Ok(status) = this.update(cx, |_, cx| check(&connection, cx)) else {
                return;
            };
            let status = status.await;
            this.update(cx, |this, cx| {
                this.status = status;
                cx.notify();
            })
            .ok();
        });
    }

    pub fn e2e_state(&self) -> Value {
        json!({
            "name": self.connection.name,
            "kind": Kind::label(self.connection.kind).trim_end_matches(':'),
            "socket": self.connection.socket,
            "url": self.connection.url,
            "status": self.status.text().as_ref(),
        })
    }
}

impl Render for ConnectionForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let kind = self.connection.kind;
        let radio = |choice: Kind, cx: &mut Context<Self>| {
            spork_workbench::controls::labeled_radio(
                SharedString::from(format!("docker-kind-{choice:?}")),
                choice.label(),
                kind == choice,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_kind(choice, cx)))
        };
        let indented = |child: AnyElement| div().pl(px(20.)).child(child);
        let labeled = |label: &'static str, input: &Entity<InputState>, enabled: bool| {
            h_flex()
                .gap(px(8.))
                .child(
                    div()
                        .flex_none()
                        .w(px(120.))
                        .whitespace_nowrap()
                        .child(label),
                )
                .child(
                    div().w(px(360.)).child(
                        spork_workbench::TextField::new(input)
                            .aria_label(label.trim_end_matches(':'))
                            .disabled(!enabled),
                    ),
                )
                .into_any_element()
        };
        let (icon, color) = match &self.status {
            Status::Connecting => (None, tokens.text_secondary),
            Status::Successful => (Some("general/greenCheckmark"), tokens.text),
            Status::Failed(_) => (Some("status/error"), tokens.text_error),
        };
        let status = self.status.text();
        v_flex()
            .id("docker-connection-form")
            .gap(px(10.))
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(div().flex_none().whitespace_nowrap().child("Name:"))
                    .child(
                        div()
                            .w(px(300.))
                            .child(spork_workbench::TextField::new(&self.name).aria_label("Name")),
                    ),
            )
            .child(div().h(px(1.)).bg(tokens.separator))
            .child(div().child("Connect to Docker daemon with:"))
            .child(
                v_flex()
                    .pl(px(12.))
                    .gap(px(8.))
                    .child(radio(Kind::DockerForMac, cx))
                    .child(radio(Kind::Colima, cx))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .child(radio(Kind::UnixSocket, cx))
                            .child(
                                div().w(px(340.)).child(
                                    spork_workbench::TextField::new(&self.socket)
                                        .aria_label("Unix socket")
                                        .disabled(kind != Kind::UnixSocket),
                                ),
                            ),
                    )
                    .child(radio(Kind::Tcp, cx))
                    .child(indented(labeled(
                        "Engine API URL:",
                        &self.url,
                        kind == Kind::Tcp,
                    ))),
            )
            .child(
                h_flex()
                    .id("docker-connection-status")
                    .role(Role::Label)
                    .aria_label(status.clone())
                    .w_full()
                    .items_start()
                    .pt(px(6.))
                    .gap(px(4.))
                    .text_color(color)
                    .children(icon.map(|icon| spork_workbench::image(icon, cx)))
                    // a long error wraps under the form rather than running off the page
                    .child(div().flex_1().min_w_0().child(status)),
            )
    }
}

/// The Docker page: the connections and the selected one's form.
pub struct SettingsPage {
    saved: Vec<Connection>,
    draft: Vec<Connection>,
    selected: Option<usize>,
    form: Option<(Entity<ConnectionForm>, Subscription)>,
}

impl SettingsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = ConnectionStore::global(cx);
        let saved = store.read(cx).list().to_vec();
        let mut page = Self {
            draft: saved.clone(),
            saved,
            selected: None,
            form: None,
        };
        let first = (!page.draft.is_empty()).then_some(0);
        page.select(first, window, cx);
        page
    }

    pub fn modified(&self) -> bool {
        self.draft != self.saved
    }

    pub fn apply(&mut self, cx: &mut App) {
        let store = ConnectionStore::global(cx);
        store.update(cx, |store, cx| store.set(self.draft.clone(), cx));
        self.saved = self.draft.clone();
    }

    fn select(&mut self, ix: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = ix.filter(|ix| *ix < self.draft.len());
        self.form = self.selected.and_then(|ix| {
            let connection = self.draft.get(ix)?.clone();
            let form = cx.new(|cx| ConnectionForm::new(connection, window, cx));
            let subscription = cx.subscribe(&form, move |this, _, edited: &Connection, cx| {
                if let Some(connection) = this.draft.get_mut(ix) {
                    *connection = edited.clone();
                }
                cx.notify();
            });
            Some((form, subscription))
        });
        cx.notify();
    }

    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let taken: Vec<&str> = self.draft.iter().map(|c| c.name.as_str()).collect();
        let name = connections::unique_name(&taken);
        self.draft.push(Connection::new(name));
        self.select(Some(self.draft.len() - 1), window, cx);
        if let Some((form, _)) = &self.form {
            let name = form.read(cx).name.clone();
            name.update(cx, |name, cx| name.focus(window, cx));
        }
    }

    fn remove(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.selected.filter(|ix| *ix < self.draft.len()) else {
            return;
        };
        self.draft.remove(ix);
        let next = match self.draft.len() {
            0 => None,
            len => Some(ix.min(len - 1)),
        };
        self.select(next, window, cx);
    }

    pub fn e2e_state(&self, cx: &App) -> Value {
        json!({
            "connections": self.draft.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            "selected": self.selected,
            "form": self.form.as_ref().map(|(form, _)| form.read(cx).e2e_state()),
        })
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let rows = self.draft.iter().enumerate().map(|(ix, connection)| {
            let selected = self.selected == Some(ix);
            let name = SharedString::from(connection.name.clone());
            h_flex()
                .id(("docker-connection", ix))
                .role(Role::ListBoxOption)
                .aria_label(name.clone())
                .aria_selected(selected)
                .h(px(spork_workbench::tokens::metrics::ROW_HEIGHT))
                .px(px(8.))
                .gap(px(4.))
                .cursor_pointer()
                .when(selected, |row| row.bg(tokens.selection))
                .child(spork_workbench::image("webReferences/server", cx))
                .child(name)
                .on_click(cx.listener(move |this, _, window, cx| this.select(Some(ix), window, cx)))
        });
        let list = v_flex()
            .w(px(220.))
            .flex_none()
            .h_full()
            .border_1()
            .border_color(tokens.control_border)
            .child(
                h_flex()
                    .id("docker-connections-toolbar")
                    .role(Role::Toolbar)
                    .aria_label("Docker connections")
                    .gap(px(2.))
                    .p(px(2.))
                    .border_b_1()
                    .border_color(tokens.separator)
                    .child(
                        ActionButton::new("docker-connection-add", "general/add")
                            .tooltip("Add")
                            .on_click(cx.listener(|this, _, window, cx| this.add(window, cx))),
                    )
                    .child(
                        ActionButton::new("docker-connection-remove", "general/remove")
                            .tooltip("Remove")
                            .disabled(self.selected.is_none())
                            .on_click(cx.listener(|this, _, window, cx| this.remove(window, cx))),
                    ),
            )
            .child(
                v_flex()
                    .id("docker-connections")
                    .role(Role::List)
                    .aria_label("Docker connections")
                    .flex_1()
                    .overflow_y_scroll()
                    .children(rows),
            );
        let details = match &self.form {
            Some((form, _)) => form.clone().into_any_element(),
            None => div()
                .text_color(tokens.text_secondary)
                .child("Nothing to show")
                .into_any_element(),
        };
        h_flex()
            .id("docker-settings")
            .size_full()
            .items_start()
            .px(px(16.))
            .pb(px(16.))
            .gap(px(16.))
            .text_ui()
            .child(list)
            .child(div().flex_1().min_w_0().child(details))
    }
}
