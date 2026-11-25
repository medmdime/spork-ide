//! Services' Docker nodes, IntelliJ's Docker plugin (`DockerMainServiceViewContributor`): a node
//! per connection (Connect, Disconnect, Edit Configuration, Delete Connections, Pull Image…, Clean
//! Up), under it once connected each Compose project with its services and their containers, then
//! Containers (the others, running and stopped), Images, Networks and Volumes. The selected node's
//! tabs show on the right: a container's Dashboard, Log (followed), Files, and once asked for its
//! Attached Console, Inspection and Terminal; an image's Dashboard, and its Layers, Labels and
//! Inspection; a network's and a volume's Dashboard and Inspection; the Images group's pull
//! console. Everything goes through the Engine API (`spork_docker`); Compose's Start All, Stop and
//! Down run the `docker compose` CLI when it is installed. Left out: Exec, Show Processes, Open
//! Project, Save as a New Image, Push Image, Copy Docker Image, Create Network and Create Volume,
//! Compose services' own actions (Scale, Start, Stop), the View Options' Docker filters, Docker
//! registries and dev containers, editing the Dashboard's variables, ports and volumes, and the
//! Files tab's editing, downloads and uploads.

use std::io::Read as _;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context as _, Result};
use futures::StreamExt as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, AsyncWindowContext, ClipboardItem, Context, DismissEvent,
    Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Role, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, WeakEntity, Window, div, px,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use project::Project;
use serde_json::{Value, json};
use spork_docker::connections::{self, Connection, ConnectionStore};
use spork_docker::engine::{
    self, COMPOSE_DIRECTORY, COMPOSE_FILES, COMPOSE_PROJECT, COMPOSE_SERVICE, Container, Engine,
    Image, Layer, Network, NewContainer, Output, Prune, Socket, Volume,
};
use spork_docker::exec::{EXEC_VAR, ExecRequest};
use spork_workbench::tree::TreeItem;
use spork_workbench::{ActionButton, Button, Tokens, popup};
use terminal::Terminal;
use terminal_view::TerminalView;
use util::ResultExt as _;
use workspace::{ModalView, Workspace};

/// The Settings page of the connections.
const SETTINGS_PAGE: &str = "Build, Execution, Deployment/Docker";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Group {
    Containers,
    Images,
    Networks,
    Volumes,
}

impl Group {
    const ALL: [Group; 4] = [
        Group::Containers,
        Group::Images,
        Group::Networks,
        Group::Volumes,
    ];

    fn title(self) -> &'static str {
        match self {
            Group::Containers => "Containers",
            Group::Images => "Images",
            Group::Networks => "Networks",
            Group::Volumes => "Volumes",
        }
    }
}

/// A node of a connection's tree, by its connection's name and its object's id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Node {
    Server(SharedString),
    Group(SharedString, Group),
    Compose(SharedString, String),
    Service(SharedString, String, String),
    Container(SharedString, String),
    Image(SharedString, String),
    Network(SharedString, String),
    Volume(SharedString, String),
}

impl Node {
    fn server(&self) -> &SharedString {
        match self {
            Node::Server(server)
            | Node::Group(server, _)
            | Node::Compose(server, _)
            | Node::Service(server, _, _)
            | Node::Container(server, _)
            | Node::Image(server, _)
            | Node::Network(server, _)
            | Node::Volume(server, _) => server,
        }
    }
}

enum State {
    Disconnected,
    Connecting,
    Connected(Engine),
    Failed(SharedString),
}

#[derive(Clone, Default)]
struct Snapshot {
    containers: Vec<Container>,
    images: Vec<Image>,
    networks: Vec<Network>,
    volumes: Vec<Volume>,
}

impl Snapshot {
    fn load(engine: &Engine) -> Result<Snapshot> {
        let mut containers = engine.containers()?;
        containers.sort_by(|a, b| a.name().cmp(b.name()));
        let mut images = engine.images()?;
        images.sort_by_key(image_title);
        let mut networks = engine.networks()?;
        networks.sort_by(|a, b| a.name.cmp(&b.name));
        let mut volumes = engine.volumes()?;
        volumes.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Snapshot {
            containers,
            images,
            networks,
            volumes,
        })
    }

    fn container(&self, id: &str) -> Option<&Container> {
        self.containers.iter().find(|container| container.id == id)
    }

    fn image(&self, id: &str) -> Option<&Image> {
        self.images.iter().find(|image| image.id == id)
    }

    /// The Compose projects with their services and the services' containers, by name.
    fn compose(&self) -> Vec<(String, Vec<(String, Vec<&Container>)>)> {
        let mut projects: Vec<(String, Vec<(String, Vec<&Container>)>)> = Vec::new();
        for container in &self.containers {
            let Some(project) = container.label(COMPOSE_PROJECT) else {
                continue;
            };
            let service = container.label(COMPOSE_SERVICE).unwrap_or_default();
            let services = match projects.iter_mut().find(|(name, _)| name == project) {
                Some((_, services)) => services,
                None => {
                    projects.push((project.to_owned(), Vec::new()));
                    match projects.last_mut() {
                        Some((_, services)) => services,
                        None => continue,
                    }
                }
            };
            match services.iter_mut().find(|(name, _)| name == service) {
                Some((_, containers)) => containers.push(container),
                None => services.push((service.to_owned(), vec![container])),
            }
        }
        projects.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, services) in &mut projects {
            services.sort_by(|a, b| a.0.cmp(&b.0));
        }
        projects
    }

    fn exists(&self, node: &Node) -> bool {
        match node {
            Node::Server(_) | Node::Group(..) => true,
            Node::Compose(_, project) => self.compose().iter().any(|(name, _)| name == project),
            Node::Service(_, project, service) => self.compose().iter().any(|(name, services)| {
                name == project && services.iter().any(|(name, _)| name == service)
            }),
            Node::Container(_, id) => self.container(id).is_some(),
            Node::Image(_, id) => self.image(id).is_some(),
            Node::Network(_, id) => self.networks.iter().any(|network| &network.id == id),
            Node::Volume(_, name) => self.volumes.iter().any(|volume| &volume.name == name),
        }
    }
}

/// An image by its first tag, or its id while it has none.
fn image_title(image: &Image) -> String {
    match image.tags().first() {
        Some(tag) => (*tag).to_owned(),
        None => image.short_id().to_owned(),
    }
}

/// A stream read on a thread of its own, closed when dropped.
struct Follow {
    closer: Option<Socket>,
    _task: Task<()>,
}

impl Drop for Follow {
    fn drop(&mut self) {
        if let Some(closer) = &self.closer {
            closer.shutdown();
        }
    }
}

/// A connection, and what its daemon holds once connected.
struct Server {
    connection: Connection,
    state: State,
    snapshot: Snapshot,
    /// The daemon's events, each one a reason to look again.
    events: Option<Follow>,
    loading: Task<()>,
}

/// A tab of the selected node's details, with IntelliJ's titles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Dashboard,
    Log,
    Files,
    AttachedConsole,
    Inspection,
    Terminal,
    Layers,
    Labels,
}

impl Tab {
    fn title(self) -> &'static str {
        match self {
            Tab::Dashboard => "Dashboard",
            Tab::Log => "Log",
            Tab::Files => "Files",
            Tab::AttachedConsole => "Attached Console",
            Tab::Inspection => "Inspection",
            Tab::Terminal => "Terminal",
            Tab::Layers => "Layers",
            Tab::Labels => "Labels",
        }
    }
}

/// A read-only console: a terminal the daemon's output is written into.
struct Console {
    terminal: Entity<Terminal>,
    view: Entity<TerminalView>,
    _follow: Option<Follow>,
}

struct Files {
    path: String,
    entries: Option<Result<Vec<String>, SharedString>>,
    _loading: Task<()>,
}

/// The selected node's tabs and what they show.
struct Details {
    node: Node,
    tab: Tab,
    inspect: Option<Result<Value, SharedString>>,
    log: Option<Console>,
    attached: Option<Console>,
    inspection: Option<Console>,
    terminal: Option<(Entity<Terminal>, Entity<TerminalView>)>,
    files: Option<Files>,
    layers: Option<Result<Vec<Layer>, SharedString>>,
    labels: bool,
    /// Inspect was asked for before the inspection came: it shows once it does.
    inspect_asked: bool,
    _loading: Task<()>,
}

impl Details {
    fn new(node: Node) -> Self {
        Details {
            node,
            tab: Tab::Dashboard,
            inspect: None,
            log: None,
            attached: None,
            inspection: None,
            terminal: None,
            files: None,
            layers: None,
            labels: false,
            inspect_asked: false,
            _loading: Task::ready(()),
        }
    }

    fn tabs(&self) -> Vec<Tab> {
        let mut tabs = vec![Tab::Dashboard];
        match &self.node {
            Node::Container(..) => {
                tabs.extend([Tab::Log, Tab::Files]);
                tabs.extend(self.attached.as_ref().map(|_| Tab::AttachedConsole));
                tabs.extend(self.inspection.as_ref().map(|_| Tab::Inspection));
                tabs.extend(self.terminal.as_ref().map(|_| Tab::Terminal));
            }
            Node::Image(..) => {
                tabs.extend(self.layers.as_ref().map(|_| Tab::Layers));
                tabs.extend(self.labels.then_some(Tab::Labels));
                tabs.extend(self.inspection.as_ref().map(|_| Tab::Inspection));
            }
            _ => tabs.extend(self.inspection.as_ref().map(|_| Tab::Inspection)),
        }
        tabs
    }
}

/// The Images group's pull console: "Image to pull:" and the pulls' progress.
struct Pull {
    server: SharedString,
    field: Entity<InputState>,
    console: Console,
    pulling: Option<Follow>,
    _subscription: Subscription,
}

/// What the nodes' toolbar and context menu do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Do {
    Connect,
    Disconnect,
    EditConfiguration,
    DeleteConnection,
    PullImage,
    CleanUp,
    Start,
    Restart,
    Pause,
    Unpause,
    Stop,
    DeleteContainer,
    CreateContainer,
    DeleteImage,
    ShowLayers,
    ShowLabels,
    DeleteNetwork,
    DeleteVolume,
    StartAll,
    ComposeStop,
    ComposeDown,
    ShowFiles,
    CopyImageId,
    CopyContainerId,
    Inspect,
    Attach,
    TerminalAsUser,
    TerminalAsRoot,
}

impl Do {
    /// IntelliJ's text and icon.
    fn look(self) -> (&'static str, &'static str) {
        match self {
            Do::Connect => ("Connect", "run/run"),
            Do::Disconnect => ("Disconnect", "run/stop"),
            Do::EditConfiguration => ("Edit Configuration", "general/edit"),
            Do::DeleteConnection => ("Delete Connections", "general/remove"),
            Do::PullImage => ("Pull Image…", "general/download"),
            Do::CleanUp => ("Clean Up", "general/delete"),
            Do::Start => ("Start Container", "run/run"),
            Do::Restart => ("Restart Container", "run/restart"),
            Do::Pause => ("Pause Container", "run/pause"),
            Do::Unpause => ("Unpause Container", "run/run"),
            Do::Stop => ("Stop Container", "run/stop"),
            Do::DeleteContainer => ("Delete Container", "general/remove"),
            Do::CreateContainer => ("Create Container", "general/add"),
            Do::DeleteImage => ("Delete Image", "general/remove"),
            Do::ShowLayers => ("Show Layers", "run/showCurrentFrame"),
            Do::ShowLabels => ("Show Labels", ""),
            Do::DeleteNetwork => ("Delete Network", "general/remove"),
            Do::DeleteVolume => ("Delete Volume", "general/remove"),
            Do::StartAll => ("Start All", "actions/runAll"),
            Do::ComposeStop => ("Stop", "run/stop"),
            Do::ComposeDown => ("Down", "actions/undeploy"),
            Do::ShowFiles => ("Show Files", ""),
            Do::CopyImageId => ("Copy Image ID", ""),
            Do::CopyContainerId => ("Copy Container ID", ""),
            Do::Inspect => ("Inspect", ""),
            Do::Attach => ("Attach", ""),
            Do::TerminalAsUser => ("As Container User", ""),
            Do::TerminalAsRoot => ("As Root User", ""),
        }
    }

    fn red(self) -> bool {
        matches!(self, Do::Stop | Do::Disconnect | Do::ComposeStop)
    }
}

pub(crate) enum DockerEvent {
    /// A node was selected: the run configurations' selection goes.
    Selected,
}

pub(crate) struct Docker {
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    store: Entity<ConnectionStore>,
    servers: Vec<Server>,
    selected: Option<Node>,
    details: Option<Details>,
    pull: Option<Pull>,
    /// The `docker` CLI, when it has Compose: `None` until looked for.
    cli: Option<Option<PathBuf>>,
    finding_cli: Task<()>,
    /// The last error an action got, for the end-to-end tests (the notification shows it).
    last_error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DockerEvent> for Docker {}

impl Docker {
    pub(crate) fn new(
        workspace: WeakEntity<Workspace>,
        project: Entity<Project>,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = ConnectionStore::global(cx);
        let mut docker = Self {
            workspace,
            project,
            _subscriptions: vec![cx.observe(&store, |this, _, cx| this.sync(cx))],
            store,
            servers: Vec::new(),
            selected: None,
            details: None,
            pull: None,
            cli: None,
            finding_cli: Task::ready(()),
            last_error: None,
        };
        docker.sync(cx);
        docker
    }

    /// Looks for a local daemon again, as opening Services does in IntelliJ.
    pub(crate) fn detect(&mut self, cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| store.detect(cx));
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    pub(crate) fn has_selection(&self) -> bool {
        self.selected.is_some()
    }

    pub(crate) fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if self.selected.take().is_some() {
            self.details = None;
            cx.notify();
        }
    }

    /// The servers follow the saved connections: one gone is disconnected, one changed starts over.
    pub(crate) fn sync(&mut self, cx: &mut Context<Self>) {
        let list = self.store.read(cx).list().to_vec();
        let mut servers = Vec::new();
        for connection in list {
            let kept = self
                .servers
                .iter()
                .position(|server| server.connection == connection)
                .map(|ix| self.servers.remove(ix));
            servers.push(kept.unwrap_or(Server {
                connection,
                state: State::Disconnected,
                snapshot: Snapshot::default(),
                events: None,
                loading: Task::ready(()),
            }));
        }
        self.servers = servers;
        if let Some(node) = &self.selected
            && self.server(node.server()).is_none()
        {
            self.selected = None;
            self.details = None;
        }
        cx.notify();
    }

    fn server(&self, name: &str) -> Option<&Server> {
        self.servers
            .iter()
            .find(|server| server.connection.name == name)
    }

    fn server_mut(&mut self, name: &str) -> Option<&mut Server> {
        self.servers
            .iter_mut()
            .find(|server| server.connection.name == name)
    }

    fn engine(&self, name: &str) -> Option<Engine> {
        match &self.server(name)?.state {
            State::Connected(engine) => Some(engine.clone()),
            _ => None,
        }
    }

    fn show_error(&mut self, error: anyhow::Error, cx: &mut Context<Self>) {
        self.last_error = Some(format!("{error:#}").into());
        self.workspace
            .update(cx, |workspace, cx| workspace.show_error(error, cx))
            .log_err();
        cx.notify();
    }

    pub(crate) fn connect(&mut self, name: &SharedString, cx: &mut Context<Self>) {
        let Some(server) = self.server_mut(name) else {
            return;
        };
        if matches!(server.state, State::Connecting | State::Connected(_)) {
            return;
        }
        server.state = State::Connecting;
        let url = connections::api_url(&server.connection);
        let name = name.clone();
        server.loading = cx.spawn(async move |this, cx| {
            let connected = cx
                .background_spawn(async move {
                    let engine = Engine::connect(&url?)?;
                    let snapshot = Snapshot::load(&engine)?;
                    anyhow::Ok((engine, snapshot))
                })
                .await;
            this.update(cx, |this, cx| {
                match connected {
                    Ok((engine, snapshot)) => {
                        let events = this.follow_events(&name, &engine, cx);
                        if let Some(server) = this.server_mut(&name) {
                            server.state = State::Connected(engine);
                            server.snapshot = snapshot;
                            server.events = events;
                        }
                        this.find_cli(cx);
                    }
                    Err(error) => {
                        if let Some(server) = this.server_mut(&name) {
                            server.state = State::Failed(format!("{error:#}").into());
                        }
                        this.show_error(error.context(format!("Cannot connect to {name}")), cx);
                    }
                }
                cx.notify();
            })
            .log_err();
        });
        cx.notify();
    }

    fn disconnect(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(server) = self.server_mut(name) {
            server.state = State::Disconnected;
            server.snapshot = Snapshot::default();
            server.events = None;
            server.loading = Task::ready(());
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|node| node.server() == name && !matches!(node, Node::Server(_)))
        {
            self.selected = Some(Node::Server(name.to_owned().into()));
            self.details = None;
        }
        cx.notify();
    }

    /// Follows the daemon's events, each a reason to list again.
    fn follow_events(
        &mut self,
        name: &SharedString,
        engine: &Engine,
        cx: &mut Context<Self>,
    ) -> Option<Follow> {
        let stream = engine.events().log_err()?;
        let closer = stream.closer().log_err();
        let (sender, mut receiver) = futures::channel::mpsc::unbounded::<()>();
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stream);
            let mut line = String::new();
            while std::io::BufRead::read_line(&mut reader, &mut line).is_ok_and(|read| read > 0) {
                line.clear();
                if sender.unbounded_send(()).is_err() {
                    break;
                }
            }
        });
        let name = name.clone();
        let task = cx.spawn(async move |this, cx| {
            while receiver.next().await.is_some() {
                // a burst of events, one look
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                while receiver.try_recv().is_ok() {}
                if this.update(cx, |this, cx| this.refresh(&name, cx)).is_err() {
                    break;
                }
            }
        });
        Some(Follow {
            closer,
            _task: task,
        })
    }

    /// Lists what the daemon holds again.
    fn refresh(&mut self, name: &SharedString, cx: &mut Context<Self>) {
        let Some(engine) = self.engine(name) else {
            return;
        };
        let task = cx.spawn({
            let name = name.clone();
            async move |this, cx| {
                let snapshot = cx
                    .background_spawn(async move { Snapshot::load(&engine) })
                    .await;
                this.update(cx, |this, cx| match snapshot {
                    Ok(snapshot) => {
                        if let Some(selected) = &this.selected
                            && selected.server() == &name
                            && !snapshot.exists(selected)
                        {
                            this.selected = None;
                            this.details = None;
                        }
                        if let Some(server) = this.server_mut(&name) {
                            server.snapshot = snapshot;
                        }
                        cx.notify();
                    }
                    Err(error) => this.show_error(error, cx),
                })
                .log_err();
            }
        });
        if let Some(server) = self.server_mut(name) {
            server.loading = task;
        }
    }

    /// Runs `operation` on the daemon in the background; its error becomes a notification, and
    /// the daemon is listed again after it.
    fn run(
        &mut self,
        name: &SharedString,
        operation: impl FnOnce(&Engine) -> Result<()> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(engine) = self.engine(name) else {
            return;
        };
        let name = name.clone();
        cx.spawn(async move |this, cx| {
            let done = cx.background_spawn(async move { operation(&engine) }).await;
            this.update(cx, |this, cx| {
                if let Err(error) = done {
                    this.show_error(error, cx);
                }
                this.refresh(&name, cx);
            })
            .log_err();
        })
        .detach();
    }

    /// Looks once for the `docker` CLI with Compose: IntelliJ's default place, then the PATH.
    fn find_cli(&mut self, cx: &mut Context<Self>) {
        if self.cli.is_some() {
            return;
        }
        self.finding_cli = cx.spawn(async move |this, cx| {
            let found = match docker_cli() {
                Some(cli) => {
                    let version = util::command::new_command(&cli)
                        .args(["compose", "version"])
                        .output()
                        .await;
                    match version {
                        Ok(output) if output.status.success() => Some(cli),
                        _ => None,
                    }
                }
                None => None,
            };
            this.update(cx, |this, cx| {
                this.cli = Some(found);
                cx.notify();
            })
            .log_err();
        });
    }

    /// Runs `docker compose` on a project with the connection's daemon as its host.
    fn compose(
        &mut self,
        name: &SharedString,
        project: &str,
        command: &[&str],
        cx: &mut Context<Self>,
    ) {
        let (Some(Some(cli)), Some(server)) = (self.cli.clone(), self.server(name)) else {
            return;
        };
        let url = connections::api_url(&server.connection);
        let mut args = vec![
            "compose".to_owned(),
            "--project-name".to_owned(),
            project.to_owned(),
        ];
        if let Some(container) = server
            .snapshot
            .containers
            .iter()
            .find(|container| container.label(COMPOSE_PROJECT) == Some(project))
        {
            if let Some(folder) = container.label(COMPOSE_DIRECTORY) {
                args.extend(["--project-directory".to_owned(), folder.to_owned()]);
            }
            for file in container
                .label(COMPOSE_FILES)
                .unwrap_or_default()
                .split(',')
                .filter(|file| !file.is_empty())
            {
                args.extend(["--file".to_owned(), file.to_owned()]);
            }
        }
        args.extend(command.iter().map(|arg| (*arg).to_owned()));
        let name = name.clone();
        cx.spawn(async move |this, cx| {
            let done: Result<()> = async {
                let output = util::command::new_command(&cli)
                    .args(&args)
                    .env("DOCKER_HOST", url?)
                    .output()
                    .await?;
                if !output.status.success() {
                    anyhow::bail!(
                        "Docker Compose execution failed: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    );
                }
                Ok(())
            }
            .await;
            this.update(cx, |this, cx| {
                if let Err(error) = done {
                    this.show_error(error, cx);
                }
                this.refresh(&name, cx);
            })
            .log_err();
        })
        .detach();
    }

    pub(crate) fn select(&mut self, node: Node, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DockerEvent::Selected);
        if self.selected.as_ref() == Some(&node) {
            return;
        }
        self.selected = Some(node.clone());
        self.details = None;
        match &node {
            Node::Container(..) | Node::Image(..) | Node::Network(..) | Node::Volume(..) => {
                self.details = Some(Details::new(node.clone()));
                self.load_inspect(window, cx);
            }
            Node::Group(server, Group::Images) => self.show_pull(server.clone(), window, cx),
            _ => {}
        }
        cx.notify();
    }

    /// Loads the selected object's inspection, which its Dashboard shows; a container's log
    /// follows once it is known whether the container has a terminal.
    fn load_inspect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(details) = &self.details else {
            return;
        };
        let node = details.node.clone();
        let Some(engine) = self.engine(node.server()) else {
            return;
        };
        let task = cx.spawn_in(window, async move |this, cx| {
            let inspected = cx
                .background_spawn({
                    let node = node.clone();
                    async move {
                        match &node {
                            Node::Container(_, id) => engine.inspect_container(id),
                            Node::Image(_, id) => engine.inspect_image(id),
                            Node::Network(_, id) => engine.inspect_network(id),
                            Node::Volume(_, name) => engine.inspect_volume(name),
                            _ => Ok(Value::Null),
                        }
                    }
                })
                .await
                .map_err(|error| SharedString::from(format!("{error:#}")));
            this.update_in(cx, |this, window, cx| {
                let tty = inspected
                    .as_ref()
                    .is_ok_and(|value| value["Config"]["Tty"].as_bool() == Some(true));
                let Some(details) = this.details.as_mut().filter(|d| d.node == node) else {
                    return;
                };
                details.inspect = Some(inspected);
                let asked = details.inspect_asked;
                if let Node::Container(server, id) = &node {
                    let log = this.log_console(server, id, tty, window, cx);
                    if let Some(details) = this.details.as_mut() {
                        details.log = log;
                    }
                }
                if asked {
                    this.inspect(window, cx);
                }
                cx.notify();
            })
            .log_err();
        });
        if let Some(details) = &mut self.details {
            details._loading = task;
        }
    }

    fn console(&self, window: &mut Window, cx: &mut Context<Self>) -> Console {
        let builder = terminal::TerminalBuilder::new_display_only(
            terminal::terminal_settings::CursorShape::default(),
            terminal::terminal_settings::AlternateScroll::On,
            None,
            0,
            cx.background_executor(),
            self.project.read(cx).path_style(cx),
        );
        let terminal = cx.new(|cx| builder.subscribe(cx));
        let view = cx.new(|cx| {
            TerminalView::new(
                terminal.clone(),
                self.workspace.clone(),
                None,
                self.project.downgrade(),
                window,
                cx,
            )
        });
        Console {
            terminal,
            view,
            _follow: None,
        }
    }

    /// A console fed by a stream on a thread of its own: a container's log, its attached output.
    fn follow_into(
        &self,
        console: &mut Console,
        open: impl FnOnce() -> Result<(Box<dyn std::io::Read + Send>, Option<Socket>)>,
        tty: bool,
        cx: &mut Context<Self>,
    ) {
        let (reader, closer) = match open() {
            Ok(opened) => opened,
            Err(error) => {
                let message = format!("{error:#}\n");
                console.terminal.update(cx, |terminal, cx| {
                    terminal.write_output(message.as_bytes(), cx)
                });
                return;
            }
        };
        let (sender, mut receiver) = futures::channel::mpsc::unbounded::<Vec<u8>>();
        let error = foreground(Tokens::get(cx).console_error);
        std::thread::spawn(move || {
            let mut reader = reader;
            if tty {
                let mut buffer = [0u8; 8192];
                while let Ok(read @ 1..) = reader.read(&mut buffer) {
                    if sender.unbounded_send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
                return;
            }
            while let Ok(Some((output, payload))) = engine::read_frame(&mut reader) {
                let bytes = match output {
                    Output::Stdout => payload,
                    Output::Stderr => {
                        let mut colored = error.clone().into_bytes();
                        colored.extend(payload);
                        colored.extend(b"\x1b[0m");
                        colored
                    }
                };
                if sender.unbounded_send(bytes).is_err() {
                    break;
                }
            }
        });
        let terminal = console.terminal.downgrade();
        let task = cx.spawn(async move |_, cx| {
            while let Some(bytes) = receiver.next().await {
                if terminal
                    .update(cx, |terminal, cx| terminal.write_output(&bytes, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        console._follow = Some(Follow {
            closer,
            _task: task,
        });
    }

    fn log_console(
        &self,
        server: &SharedString,
        id: &str,
        tty: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Console> {
        let engine = self.engine(server)?;
        let mut console = self.console(window, cx);
        let id = id.to_owned();
        self.follow_into(
            &mut console,
            move || {
                let stream = engine.logs(&id, true)?;
                let closer = stream.closer().log_err();
                Ok((Box::new(stream) as Box<dyn std::io::Read + Send>, closer))
            },
            tty,
            cx,
        );
        Some(console)
    }

    fn show_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if let Some(details) = &mut self.details {
            details.tab = tab;
            cx.notify();
        }
    }

    fn inspect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = match self.details.as_ref().and_then(|d| d.inspect.as_ref()) {
            Some(Ok(value)) => serde_json::to_string_pretty(value).unwrap_or_default(),
            Some(Err(error)) => error.to_string(),
            None => {
                if let Some(details) = &mut self.details {
                    details.inspect_asked = true;
                }
                return;
            }
        };
        let console = self.console(window, cx);
        console.terminal.update(cx, |terminal, cx| {
            terminal.write_output(text.as_bytes(), cx)
        });
        if let Some(details) = &mut self.details {
            details.inspection = Some(console);
            details.tab = Tab::Inspection;
        }
        cx.notify();
    }

    fn attach(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Node::Container(server, id)) = self.selected.clone() else {
            return;
        };
        let Some(engine) = self.engine(&server) else {
            return;
        };
        let tty = matches!(
            self.details.as_ref().and_then(|d| d.inspect.as_ref()),
            Some(Ok(value)) if value["Config"]["Tty"].as_bool() == Some(true)
        );
        let mut console = self.console(window, cx);
        self.follow_into(
            &mut console,
            move || {
                let hijacked = engine.attach(&id)?;
                let closer = hijacked.input.try_clone().log_err();
                Ok((
                    Box::new(hijacked.output) as Box<dyn std::io::Read + Send>,
                    closer,
                ))
            },
            tty,
            cx,
        );
        if let Some(details) = &mut self.details {
            details.attached = Some(console);
            details.tab = Tab::AttachedConsole;
        }
        cx.notify();
    }

    /// Create Terminal: Zed's terminal running Spork IDE's bridge to a shell in the container.
    fn create_terminal(&mut self, root: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Node::Container(server, id)) = self.selected.clone() else {
            return;
        };
        let (Some(engine), Some(connection)) = (
            self.engine(&server),
            self.server(&server).map(|server| server.connection.clone()),
        ) else {
            return;
        };
        let spawn: Result<task::SpawnInTerminal> = (|| {
            let request = ExecRequest {
                url: connections::api_url(&connection)?,
                version: engine.version().to_owned(),
                container: id.clone(),
                user: if root {
                    "root".to_owned()
                } else {
                    String::new()
                },
            };
            let exe = std::env::current_exe().context("the IDE's binary")?;
            Ok(task::SpawnInTerminal {
                label: "Terminal".to_owned(),
                full_label: "Terminal".to_owned(),
                command: Some(exe.to_string_lossy().into_owned()),
                command_label: format!("Terminal ({id})"),
                env: [(EXEC_VAR.to_owned(), serde_json::to_string(&request)?)]
                    .into_iter()
                    .collect(),
                show_summary: false,
                show_command: false,
                ..task::SpawnInTerminal::default()
            })
        })();
        let spawn = match spawn {
            Ok(spawn) => spawn,
            Err(error) => return self.show_error(error, cx),
        };
        let create = self
            .project
            .update(cx, |project, cx| project.create_terminal_task(spawn, cx));
        cx.spawn_in(window, async move |this, cx: &mut AsyncWindowContext| {
            let terminal = create.await;
            this.update_in(cx, |this, window, cx| match terminal {
                Ok(terminal) => {
                    let view = cx.new(|cx| {
                        TerminalView::new(
                            terminal.clone(),
                            this.workspace.clone(),
                            None,
                            this.project.downgrade(),
                            window,
                            cx,
                        )
                    });
                    view.read(cx).focus_handle(cx).focus(window, cx);
                    if let Some(details) = this
                        .details
                        .as_mut()
                        .filter(|d| d.node == Node::Container(server.clone(), id.clone()))
                    {
                        details.terminal = Some((terminal, view));
                        details.tab = Tab::Terminal;
                    }
                    cx.notify();
                }
                Err(error) => this.show_error(error, cx),
            })
            .log_err();
        })
        .detach();
    }

    fn show_files(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(Node::Container(server, id)) = self.selected.clone() else {
            return;
        };
        let running = self
            .server(&server)
            .and_then(|server| server.snapshot.container(&id))
            .is_some_and(Container::running);
        let engine = self.engine(&server).filter(|_| running);
        let loading = match engine {
            Some(engine) => {
                let (folder, node) = (path.clone(), Node::Container(server, id.clone()));
                cx.spawn(async move |this, cx| {
                    let entries = cx
                        .background_spawn(async move { engine.list_folder(&id, &folder) })
                        .await
                        .map_err(|error| SharedString::from(format!("{error:#}")));
                    this.update(cx, |this, cx| {
                        if let Some(files) = this
                            .details
                            .as_mut()
                            .filter(|d| d.node == node)
                            .and_then(|d| d.files.as_mut())
                        {
                            files.entries = Some(entries);
                        }
                        cx.notify();
                    })
                    .log_err();
                })
            }
            None => Task::ready(()),
        };
        if let Some(details) = &mut self.details {
            details.files = Some(Files {
                path,
                entries: None,
                _loading: loading,
            });
            details.tab = Tab::Files;
        }
        cx.notify();
    }

    fn show_layers(&mut self, cx: &mut Context<Self>) {
        let Some(Node::Image(server, id)) = self.selected.clone() else {
            return;
        };
        let Some(engine) = self.engine(&server) else {
            return;
        };
        let node = Node::Image(server, id.clone());
        let task = cx.spawn(async move |this, cx| {
            let layers = cx
                .background_spawn(async move { engine.image_history(&id) })
                .await
                .map_err(|error| SharedString::from(format!("{error:#}")));
            this.update(cx, |this, cx| {
                if let Some(details) = this.details.as_mut().filter(|d| d.node == node) {
                    details.layers = Some(layers);
                    details.tab = Tab::Layers;
                }
                cx.notify();
            })
            .log_err();
        });
        if let Some(details) = &mut self.details {
            details._loading = task;
        }
    }

    fn show_pull(&mut self, server: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        if self.pull.as_ref().is_some_and(|pull| pull.server == server) {
            return;
        }
        let field = cx.new(|cx| InputState::new(window, cx));
        let subscription = cx.subscribe(&field, |this, _, event: &InputEvent, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.pull_image(cx);
            }
        });
        self.pull = Some(Pull {
            server,
            field,
            console: self.console(window, cx),
            pulling: None,
            _subscription: subscription,
        });
    }

    /// Pulls the image the Images group's field names, its progress in the console under it.
    fn pull_image(&mut self, cx: &mut Context<Self>) {
        let Some(pull) = &self.pull else {
            return;
        };
        let image = pull.field.read(cx).value().trim().to_owned();
        let Some(engine) = self.engine(&pull.server).filter(|_| !image.is_empty()) else {
            return;
        };
        let (name, tag) = engine::split_tag(&image);
        let heading = format!("{}:{tag}\n", name);
        pull.console.terminal.update(cx, |terminal, cx| {
            terminal.write_output(heading.as_bytes(), cx)
        });
        let stream = match engine.pull(&image) {
            Ok(stream) => stream,
            Err(error) => {
                let message = format!("{error:#}\n");
                pull.console.terminal.update(cx, |terminal, cx| {
                    terminal.write_output(message.as_bytes(), cx)
                });
                return;
            }
        };
        let closer = stream.closer().log_err();
        let (sender, mut receiver) = futures::channel::mpsc::unbounded::<Option<String>>();
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stream);
            let mut line = String::new();
            while std::io::BufRead::read_line(&mut reader, &mut line).is_ok_and(|read| read > 0) {
                if let Some(text) = pull_line(&line)
                    && sender.unbounded_send(Some(text)).is_err()
                {
                    return;
                }
                line.clear();
            }
            sender.unbounded_send(None).ok();
        });
        let terminal = pull.console.terminal.downgrade();
        let server = pull.server.clone();
        let task = cx.spawn(async move |this, cx| {
            while let Some(line) = receiver.next().await {
                let Some(line) = line else {
                    this.update(cx, |this, cx| this.refresh(&server, cx))
                        .log_err();
                    break;
                };
                if terminal
                    .update(cx, |terminal, cx| {
                        terminal.write_output(line.as_bytes(), cx)
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        if let Some(pull) = &mut self.pull {
            pull.pulling = Some(Follow {
                closer,
                _task: task,
            });
        }
    }

    /// What a node's toolbar and context menu offer, in IntelliJ's order; `None` separates.
    fn actions(&self, node: &Node) -> Vec<Option<(Do, bool)>> {
        let Some(server) = self.server(node.server()) else {
            return Vec::new();
        };
        let connected = matches!(server.state, State::Connected(_));
        let compose = matches!(&self.cli, Some(Some(_)));
        match node {
            Node::Server(_) => vec![
                Some(match connected {
                    true => (Do::Disconnect, true),
                    false => (Do::Connect, !matches!(server.state, State::Connecting)),
                }),
                None,
                Some((Do::EditConfiguration, true)),
                Some((Do::DeleteConnection, true)),
                None,
                Some((Do::PullImage, connected)),
                Some((Do::CleanUp, connected)),
            ],
            Node::Group(_, Group::Images) => vec![
                Some((Do::PullImage, connected)),
                Some((Do::CleanUp, connected)),
            ],
            Node::Group(..) => vec![Some((Do::CleanUp, connected))],
            Node::Compose(..) => vec![
                Some((Do::StartAll, compose)),
                Some((Do::ComposeStop, compose)),
                Some((Do::ComposeDown, compose)),
            ],
            Node::Service(..) => Vec::new(),
            Node::Container(_, id) => {
                let Some(container) = server.snapshot.container(id) else {
                    return Vec::new();
                };
                let mut actions = match (container.running(), container.paused()) {
                    (true, _) => vec![
                        Some((Do::Restart, true)),
                        Some((Do::Pause, true)),
                        Some((Do::Stop, true)),
                    ],
                    (false, true) => vec![Some((Do::Unpause, true)), Some((Do::Stop, true))],
                    (false, false) => vec![Some((Do::Start, true))],
                };
                actions.extend([Some((Do::DeleteContainer, true)), None]);
                actions.extend([
                    Some((Do::ShowFiles, true)),
                    Some((Do::CopyImageId, true)),
                    Some((Do::CopyContainerId, true)),
                    Some((Do::Inspect, true)),
                    Some((Do::Attach, container.running())),
                    Some((Do::TerminalAsUser, container.running())),
                    Some((Do::TerminalAsRoot, container.running())),
                ]);
                actions
            }
            Node::Image(..) => vec![
                Some((Do::CreateContainer, true)),
                Some((Do::DeleteImage, true)),
                Some((Do::ShowLayers, true)),
                None,
                Some((Do::CopyImageId, true)),
                Some((Do::Inspect, true)),
                Some((Do::ShowLabels, true)),
            ],
            Node::Network(..) => vec![
                Some((Do::DeleteNetwork, true)),
                None,
                Some((Do::Inspect, true)),
            ],
            Node::Volume(..) => vec![
                Some((Do::DeleteVolume, true)),
                None,
                Some((Do::Inspect, true)),
            ],
        }
    }

    fn perform(&mut self, node: Node, action: Do, window: &mut Window, cx: &mut Context<Self>) {
        self.select(node.clone(), window, cx);
        let server = node.server().clone();
        match (action, &node) {
            (Do::Connect, _) => self.connect(&server, cx),
            (Do::Disconnect, _) => self.disconnect(&server, cx),
            (Do::EditConfiguration, _) => {
                let workspace = self.workspace.clone();
                window.defer(cx, move |window, cx| {
                    workspace
                        .update(cx, |workspace, cx| {
                            spork_settings::open(
                                Some(workspace),
                                Some(spork_settings::Target::Page(SETTINGS_PAGE.to_owned())),
                                window,
                                cx,
                            )
                        })
                        .log_err();
                });
            }
            (Do::DeleteConnection, _) => {
                self.store
                    .update(cx, |store, cx| store.remove(&server, cx));
            }
            (Do::PullImage, _) => {
                self.select(Node::Group(server.clone(), Group::Images), window, cx);
                if let Some(pull) = &self.pull {
                    pull.field.update(cx, |field, cx| field.focus(window, cx));
                }
            }
            (Do::CleanUp, Node::Group(_, group)) => {
                let (message, prunes) = match group {
                    Group::Containers => ("Remove all stopped containers?", vec![Prune::Containers]),
                    Group::Images => ("Remove all unused images?", vec![Prune::UnusedImages]),
                    Group::Networks => ("Remove all unused networks?", vec![Prune::Networks]),
                    Group::Volumes => ("Remove all unused volumes?", vec![Prune::Volumes]),
                };
                self.confirm_prune(server, message, prunes, window, cx);
            }
            (Do::CleanUp, _) => self.confirm_prune(
                server,
                "Remove all stopped containers, unused volumes, networks, dangling images, and all build caches?",
                vec![
                    Prune::Containers,
                    Prune::Volumes,
                    Prune::Networks,
                    Prune::DanglingImages,
                    Prune::BuildCache,
                ],
                window,
                cx,
            ),
            (Do::Start | Do::Restart | Do::Pause | Do::Unpause | Do::Stop, Node::Container(_, id)) => {
                let verb = match action {
                    Do::Start => "start",
                    Do::Restart => "restart",
                    Do::Pause => "pause",
                    Do::Unpause => "unpause",
                    _ => "stop",
                };
                let id = id.clone();
                self.run(&server, move |engine| engine.container_action(&id, verb), cx);
            }
            (Do::DeleteContainer, Node::Container(_, id)) => {
                let id = id.clone();
                self.confirm_delete(server, "Delete 1 container?", move |engine| engine.remove_container(&id), window, cx);
            }
            (Do::DeleteImage, Node::Image(_, id)) => {
                let id = id.clone();
                self.confirm_delete(server, "Delete 1 image?", move |engine| engine.remove_image(&id), window, cx);
            }
            (Do::DeleteNetwork, Node::Network(_, id)) => {
                let id = id.clone();
                self.confirm_delete(server, "Delete item?", move |engine| engine.remove_network(&id), window, cx);
            }
            (Do::DeleteVolume, Node::Volume(_, name)) => {
                let name = name.clone();
                self.confirm_delete(server, "Delete item?", move |engine| engine.remove_volume(&name), window, cx);
            }
            (Do::CreateContainer, Node::Image(_, id)) => {
                let image = self
                    .server(&server)
                    .and_then(|s| s.snapshot.image(id))
                    .map(image_title)
                    .unwrap_or_else(|| id.clone());
                CreateContainer::open(cx.entity().downgrade(), server, image, cx);
            }
            (Do::ShowLayers, _) => self.show_layers(cx),
            (Do::ShowLabels, _) => {
                if let Some(details) = &mut self.details {
                    details.labels = true;
                    details.tab = Tab::Labels;
                }
            }
            (Do::StartAll, Node::Compose(_, project)) => {
                let project = project.clone();
                self.compose(&server, &project, &["up", "--detach"], cx)
            }
            (Do::ComposeStop, Node::Compose(_, project)) => {
                let project = project.clone();
                self.compose(&server, &project, &["stop"], cx)
            }
            (Do::ComposeDown, Node::Compose(_, project)) => {
                let project = project.clone();
                self.compose(&server, &project, &["down"], cx)
            }
            (Do::ShowFiles, _) => self.show_files("/".to_owned(), cx),
            (Do::CopyContainerId, Node::Container(_, id)) => {
                cx.write_to_clipboard(ClipboardItem::new_string(id.clone()))
            }
            (Do::CopyImageId, Node::Container(_, id)) => {
                let image = self
                    .server(&server)
                    .and_then(|s| s.snapshot.container(id))
                    .map(|container| container.image_id.clone());
                if let Some(image) = image {
                    cx.write_to_clipboard(ClipboardItem::new_string(image));
                }
            }
            (Do::CopyImageId, Node::Image(_, id)) => {
                cx.write_to_clipboard(ClipboardItem::new_string(id.clone()))
            }
            (Do::Inspect, _) => self.inspect(window, cx),
            (Do::Attach, _) => self.attach(window, cx),
            (Do::TerminalAsUser, _) => self.create_terminal(false, window, cx),
            (Do::TerminalAsRoot, _) => self.create_terminal(true, window, cx),
            _ => {}
        }
        cx.notify();
    }

    fn confirm_delete(
        &mut self,
        server: SharedString,
        message: &'static str,
        delete: impl FnOnce(&Engine) -> Result<()> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity().downgrade();
        Confirm::show(
            &self.workspace,
            "Confirm Deletion",
            message,
            "Delete",
            move |cx| {
                this.update(cx, |this, cx| this.run(&server, delete, cx))
                    .log_err();
            },
            window,
            cx,
        );
    }

    fn confirm_prune(
        &mut self,
        server: SharedString,
        message: &'static str,
        prunes: Vec<Prune>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity().downgrade();
        Confirm::show(
            &self.workspace,
            "Clean Up",
            message,
            "Yes",
            move |cx| {
                this.update(cx, |this, cx| {
                    this.run(
                        &server,
                        move |engine| prunes.into_iter().try_for_each(|prune| engine.prune(prune)),
                        cx,
                    )
                })
                .log_err();
            },
            window,
            cx,
        );
    }

    /// Creates the container the Create Container dialog describes, then shows it.
    fn create_container(
        &mut self,
        server: SharedString,
        new: NewContainer,
        cx: &mut Context<Self>,
    ) {
        let Some(engine) = self.engine(&server) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let created = cx
                .background_spawn(async move { engine.create_container(&new) })
                .await;
            this.update(cx, |this, cx| {
                if let Err(error) = created {
                    this.show_error(error, cx);
                }
                this.refresh(&server, cx);
            })
            .log_err();
        })
        .detach();
    }

    /// The menu of a node: its actions, Create Terminal's in a submenu.
    fn menu(
        this: WeakEntity<Self>,
        node: Node,
        actions: Vec<Option<(Do, bool)>>,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let item =
            |action: Do, enabled: bool| menu_item(this.clone(), node.clone(), action, enabled);
        let terminal: Vec<(Do, bool)> = actions
            .iter()
            .flatten()
            .filter(|(action, _)| matches!(action, Do::TerminalAsUser | Do::TerminalAsRoot))
            .copied()
            .collect();
        let mut menu = menu;
        let mut separated = true;
        for action in actions {
            match action {
                None if !separated => {
                    menu = menu.separator();
                    separated = true;
                }
                None => {}
                Some((Do::TerminalAsUser | Do::TerminalAsRoot, _)) => {}
                Some((action, enabled)) => {
                    menu = menu.item(item(action, enabled));
                    separated = false;
                }
            }
        }
        if !terminal.is_empty() {
            let enabled = terminal.iter().any(|(_, enabled)| *enabled);
            menu = match enabled {
                true => menu.submenu("Create Terminal", window, cx, move |mut submenu, _, _| {
                    for (action, enabled) in &terminal {
                        submenu =
                            submenu.item(menu_item(this.clone(), node.clone(), *action, *enabled));
                    }
                    submenu
                }),
                false => menu.item(PopupMenuItem::new("Create Terminal").disabled(true)),
            };
        }
        menu
    }

    /// The connections' nodes for Services' tree: a connection's open, the nodes under it closed
    /// until Expand All (`expand`).
    pub(crate) fn tree_items(&self, expand: bool, cx: &mut Context<Self>) -> Vec<TreeItem> {
        let this = cx.entity().downgrade();
        let item = |label: SharedString, node: Node, icon: &'static str| {
            let (select, menu, connect) = (this.clone(), this.clone(), this.clone());
            let actions = self.actions(&node);
            let (picked, menu_node) = (node.clone(), node.clone());
            let is_server = matches!(node, Node::Server(_));
            TreeItem::new(label)
                .colored_icon(icon)
                .active(self.selected.as_ref() == Some(&node))
                .on_click(move |event, window, cx| {
                    let node = picked.clone();
                    if is_server && event.click_count() == 2 {
                        let name = node.server().clone();
                        connect
                            .update(cx, |this, cx| this.connect(&name, cx))
                            .log_err();
                    }
                    select
                        .update(cx, |this, cx| this.select(node, window, cx))
                        .log_err();
                })
                .context_menu(move |popup, window, cx| {
                    Self::menu(
                        menu.clone(),
                        menu_node.clone(),
                        actions.clone(),
                        popup,
                        window,
                        cx,
                    )
                })
        };
        self.servers
            .iter()
            .map(|server| {
                let name = SharedString::from(server.connection.name.clone());
                let node = item(
                    name.clone(),
                    Node::Server(name.clone()),
                    "webReferences/server",
                );
                let node = match &server.state {
                    State::Connecting => node.hint("Connecting"),
                    State::Failed(_) => node.hint("Error"),
                    State::Disconnected | State::Connected(_) => node,
                };
                if !matches!(server.state, State::Connected(_)) {
                    return node;
                }
                let snapshot = &server.snapshot;
                let container_item = |container: &Container| {
                    item(
                        container.name().to_owned().into(),
                        Node::Container(name.clone(), container.id.clone()),
                        "nodes/artifact",
                    )
                    .hint(container.status.clone())
                };
                let compose = snapshot.compose().into_iter().map(|(project, services)| {
                    let of = project.clone();
                    let services = services.into_iter().map(|(service, containers)| {
                        item(
                            service.clone().into(),
                            Node::Service(name.clone(), of.clone(), service),
                            "nodes/services",
                        )
                        .default_open(expand)
                        .children(containers.into_iter().map(container_item))
                    });
                    item(
                        project.clone().into(),
                        Node::Compose(name.clone(), project),
                        "nodes/moduleGroup",
                    )
                    .default_open(expand)
                    .children(services.collect::<Vec<_>>())
                });
                let groups = Group::ALL.map(|group| {
                    let children: Vec<TreeItem> = match group {
                        Group::Containers => snapshot
                            .containers
                            .iter()
                            .filter(|container| container.label(COMPOSE_PROJECT).is_none())
                            .map(container_item)
                            .collect(),
                        Group::Images => snapshot
                            .images
                            .iter()
                            .map(|image| {
                                item(
                                    image_title(image).into(),
                                    Node::Image(name.clone(), image.id.clone()),
                                    "nodes/library",
                                )
                            })
                            .collect(),
                        Group::Networks => snapshot
                            .networks
                            .iter()
                            .map(|network| {
                                item(
                                    network.name.clone().into(),
                                    Node::Network(name.clone(), network.id.clone()),
                                    "webReferences/webSocket",
                                )
                            })
                            .collect(),
                        Group::Volumes => snapshot
                            .volumes
                            .iter()
                            .map(|volume| {
                                item(
                                    volume.name.clone().into(),
                                    Node::Volume(name.clone(), volume.name.clone()),
                                    "nodes/dataTables",
                                )
                            })
                            .collect(),
                    };
                    item(
                        group.title().into(),
                        Node::Group(name.clone(), group),
                        "nodes/folder",
                    )
                    .default_open(expand)
                    .children(children)
                });
                node.default_open(true)
                    .children(compose.chain(groups).collect::<Vec<_>>())
            })
            .collect()
    }

    /// The selected node's toolbar (`ServiceViewItemToolbar`) on the left of its details.
    fn item_toolbar(&self, node: &Node, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let this = cx.entity().downgrade();
        // the actions with an icon are the toolbar's; the others are the menu's alone
        let buttons = self
            .actions(node)
            .into_iter()
            .flatten()
            .filter(|(action, _)| !action.look().1.is_empty())
            .enumerate()
            .map(|(ix, (action, enabled))| {
                let (label, icon) = action.look();
                let (this, node) = (this.clone(), node.clone());
                let button = ActionButton::new(("docker-action", ix), icon)
                    .tooltip(label)
                    .disabled(!enabled)
                    .on_click(move |_, window, cx| {
                        let node = node.clone();
                        this.update(cx, |this, cx| this.perform(node, action, window, cx))
                            .log_err();
                    });
                match action.red() || matches!(action, Do::Connect | Do::Start | Do::StartAll) {
                    true if enabled => button.colored(),
                    _ => button,
                }
                .into_any_element()
            })
            .collect::<Vec<_>>();
        v_flex()
            .id("docker-item-toolbar")
            .role(Role::Toolbar)
            .aria_label("ServiceViewItemToolbar")
            .flex_none()
            .w(px(32.))
            .h_full()
            .pt(px(4.))
            .items_center()
            .gap(px(4.))
            .border_r_1()
            .border_color(tokens.separator)
            .children(buttons)
            .into_any_element()
    }
}

fn menu_item(this: WeakEntity<Docker>, node: Node, action: Do, enabled: bool) -> PopupMenuItem {
    PopupMenuItem::new(action.look().0)
        .disabled(!enabled)
        .on_click(move |_, window, cx| {
            let node = node.clone();
            this.update(cx, |this, cx| this.perform(node, action, window, cx))
                .log_err();
        })
}

/// The colour escape IntelliJ's console writes stderr in.
fn foreground(color: gpui::Hsla) -> String {
    let color = color.to_rgb();
    let [r, g, b] = [color.r, color.g, color.b].map(|c| (c * 255.).round() as u8);
    format!("\x1b[38;2;{r};{g};{b}m")
}

/// A line of a pull's progress as `docker pull` writes it: `id: status progress`.
fn pull_line(line: &str) -> Option<String> {
    let event: Value = serde_json::from_str(line.trim()).ok()?;
    if let Some(error) = event["error"].as_str() {
        return Some(format!("{error}\n"));
    }
    let status = event["status"].as_str()?;
    Some(match (event["id"].as_str(), event["progress"].as_str()) {
        (Some(id), Some(progress)) => format!("{id}: {status} {progress}\n"),
        (Some(id), None) => format!("{id}: {status}\n"),
        (None, _) => format!("{status}\n"),
    })
}

/// The `docker` CLI: IntelliJ's default `/usr/local/bin/docker`, Docker Desktop's, then the
/// PATH's.
fn docker_cli() -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|folder| folder.join("docker"))
        .chain([
            PathBuf::from("/usr/local/bin/docker"),
            PathBuf::from("/Applications/Docker.app/Contents/Resources/bin/docker"),
            paths::home_dir().join(".docker/bin/docker"),
            paths::home_dir().join(".orbstack/bin/docker"),
        ])
        .find(|cli| cli.is_file())
}

/// Bytes as Docker writes sizes: in units of 1000.
fn size(bytes: i64) -> String {
    let bytes = bytes.max(0) as f64;
    match bytes {
        b if b >= 1e9 => format!("{:.2} GB", b / 1e9),
        b if b >= 1e6 => format!("{:.1} MB", b / 1e6),
        b if b >= 1e3 => format!("{:.1} kB", b / 1e3),
        b => format!("{b} B"),
    }
}

fn date(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|date| date.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}

/// A Dashboard's segment: its title over rows of a name and a value.
fn segment(title: &'static str, rows: Vec<(String, String)>, cx: &App) -> AnyElement {
    let tokens = Tokens::get(cx);
    v_flex()
        .id(title)
        .role(Role::Group)
        .aria_label(title)
        .gap(px(2.))
        .child(
            div()
                .pb(px(4.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title),
        )
        .when(rows.is_empty(), |segment| {
            segment.child(
                div()
                    .text_color(tokens.text_secondary)
                    .child("Nothing to show"),
            )
        })
        .children(rows.into_iter().map(|(name, value)| {
            h_flex()
                .gap(px(12.))
                .child(div().w(px(220.)).flex_none().truncate().child(name))
                .child(div().text_color(tokens.text_secondary).child(value))
        }))
        .into_any_element()
}

/// `{"80/tcp": [{"HostIp": "0.0.0.0", "HostPort": "8080"}]}` as rows.
fn ports(value: &Value) -> Vec<(String, String)> {
    value
        .as_object()
        .map(|ports| {
            ports
                .iter()
                .map(|(port, bindings)| {
                    let hosts: Vec<String> = bindings
                        .as_array()
                        .map(|bindings| {
                            bindings
                                .iter()
                                .map(|binding| {
                                    format!(
                                        "{}:{}",
                                        binding["HostIp"].as_str().unwrap_or_default(),
                                        binding["HostPort"].as_str().unwrap_or_default()
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    (port.clone(), hosts.join(", "))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn strings(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn labels(value: &Value) -> Vec<(String, String)> {
    value
        .as_object()
        .map(|labels| {
            labels
                .iter()
                .map(|(key, value)| (key.clone(), value.as_str().unwrap_or_default().to_owned()))
                .collect()
        })
        .unwrap_or_default()
}

impl Docker {
    fn dashboard(&self, details: &Details, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let inspect = match &details.inspect {
            Some(Ok(inspect)) => inspect,
            Some(Err(error)) => return centered(error.clone(), cx),
            None => return centered("Loading…".into(), cx),
        };
        let snapshot = self.server(details.node.server()).map(|s| &s.snapshot);
        let title = |text: String, id: &str| {
            h_flex()
                .gap(px(8.))
                .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(text))
                .child(
                    div()
                        .text_color(tokens.text_secondary)
                        .child(engine::short_id(id).to_owned()),
                )
        };
        let content: Vec<AnyElement> = match &details.node {
            Node::Container(_, id) => {
                let container = snapshot.and_then(|s| s.container(id));
                let running = container.is_some_and(Container::running);
                let this = cx.entity().downgrade();
                let button = |label: &'static str, action: Do, enabled: bool| {
                    let (this, node) = (this.clone(), details.node.clone());
                    Button::new(
                        SharedString::from(format!("docker-dashboard-{label}")),
                        label,
                    )
                    .disabled(!enabled)
                    .on_click(move |_, window, cx| {
                        let node = node.clone();
                        this.update(cx, |this, cx| this.perform(node, action, window, cx))
                            .log_err();
                    })
                };
                let env = strings(&inspect["Config"]["Env"])
                    .into_iter()
                    .map(|pair| match pair.split_once('=') {
                        Some((name, value)) => (name.to_owned(), value.to_owned()),
                        None => (pair.to_owned(), String::new()),
                    })
                    .collect();
                let mounts = inspect["Mounts"]
                    .as_array()
                    .map(|mounts| {
                        mounts
                            .iter()
                            .map(|mount| {
                                let source = mount["Name"]
                                    .as_str()
                                    .or(mount["Source"].as_str())
                                    .unwrap_or_default();
                                let access = match mount["RW"].as_bool() {
                                    Some(false) => "Read only",
                                    _ => "Read/write",
                                };
                                (
                                    mount["Destination"].as_str().unwrap_or_default().to_owned(),
                                    format!("{source} ({access})"),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                vec![
                    title(
                        container.map(|c| c.name().to_owned()).unwrap_or_default(),
                        id,
                    )
                    .into_any_element(),
                    h_flex()
                        .gap(px(8.))
                        .child(button("Start", Do::Start, !running))
                        .child(button("Restart", Do::Restart, running))
                        .child(button("Stop", Do::Stop, running))
                        .child(button("Terminal", Do::TerminalAsUser, running))
                        .into_any_element(),
                    segment("Environment variables", env, cx),
                    segment("Ports", ports(&inspect["NetworkSettings"]["Ports"]), cx),
                    segment("Volumes", mounts, cx),
                ]
            }
            Node::Image(_, id) => {
                let image = snapshot.and_then(|s| s.image(id));
                let containers = snapshot
                    .map(|s| {
                        s.containers
                            .iter()
                            .filter(|container| &container.image_id == id)
                            .map(|container| {
                                (container.name().to_owned(), container.status.clone())
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let tags = image
                    .map(|image| {
                        image
                            .tags()
                            .into_iter()
                            .map(|tag| (tag.to_owned(), String::new()))
                            .collect()
                    })
                    .unwrap_or_default();
                let this = cx.entity().downgrade();
                let node = details.node.clone();
                vec![
                    title(image.map(image_title).unwrap_or_default(), id).into_any_element(),
                    h_flex()
                        .child(
                            Button::new("docker-dashboard-create-container", "Create Container")
                                .on_click(move |_, window, cx| {
                                    let node = node.clone();
                                    this.update(cx, |this, cx| {
                                        this.perform(node, Do::CreateContainer, window, cx)
                                    })
                                    .log_err();
                                }),
                        )
                        .into_any_element(),
                    segment(
                        "Properties",
                        vec![
                            ("ID".to_owned(), id.clone()),
                            (
                                "Created".to_owned(),
                                image.map(|i| date(i.created)).unwrap_or_default(),
                            ),
                            (
                                "Size".to_owned(),
                                image.map(|i| size(i.size)).unwrap_or_default(),
                            ),
                        ],
                        cx,
                    ),
                    segment("Tags", tags, cx),
                    segment("Containers", containers, cx),
                ]
            }
            Node::Network(_, id) => {
                let flag = |key: &str| match inspect[key].as_bool() {
                    Some(true) => "true".to_owned(),
                    _ => "false".to_owned(),
                };
                let containers = inspect["Containers"]
                    .as_object()
                    .map(|containers| {
                        containers
                            .values()
                            .map(|container| {
                                (
                                    container["Name"].as_str().unwrap_or_default().to_owned(),
                                    container["IPv4Address"]
                                        .as_str()
                                        .unwrap_or_default()
                                        .to_owned(),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let ipam = inspect["IPAM"]["Config"]
                    .as_array()
                    .map(|configs| {
                        configs
                            .iter()
                            .flat_map(|config| {
                                [("subnet", "Subnet"), ("gateway", "Gateway")]
                                    .into_iter()
                                    .filter_map(|(name, key)| {
                                        Some((name.to_owned(), config[key].as_str()?.to_owned()))
                                    })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                vec![
                    title(inspect["Name"].as_str().unwrap_or_default().to_owned(), id)
                        .into_any_element(),
                    segment(
                        "Properties",
                        vec![
                            ("Network ID".to_owned(), id.clone()),
                            (
                                "Scope".to_owned(),
                                inspect["Scope"].as_str().unwrap_or_default().to_owned(),
                            ),
                            (
                                "Driver".to_owned(),
                                inspect["Driver"].as_str().unwrap_or_default().to_owned(),
                            ),
                            ("Internal".to_owned(), flag("Internal")),
                            ("Attachable".to_owned(), flag("Attachable")),
                            ("IPv6 enabled".to_owned(), flag("EnableIPv6")),
                        ],
                        cx,
                    ),
                    segment("Containers", containers, cx),
                    segment("IPAM", ipam, cx),
                ]
            }
            Node::Volume(_, name) => {
                let users: Vec<(String, String)> = snapshot
                    .map(|s| {
                        s.containers
                            .iter()
                            .filter(|container| {
                                container.mounts.iter().any(|mount| &mount.name == name)
                            })
                            .map(|container| {
                                (container.name().to_owned(), container.status.clone())
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                vec![
                    title(name.clone(), "").into_any_element(),
                    segment(
                        "Properties",
                        vec![
                            (
                                "Driver".to_owned(),
                                inspect["Driver"].as_str().unwrap_or_default().to_owned(),
                            ),
                            (
                                "Mountpoint".to_owned(),
                                inspect["Mountpoint"]
                                    .as_str()
                                    .unwrap_or_default()
                                    .to_owned(),
                            ),
                        ],
                        cx,
                    ),
                    segment("In Use", users, cx),
                ]
            }
            _ => Vec::new(),
        };
        v_flex()
            .id("docker-dashboard")
            .size_full()
            .p(px(12.))
            .gap(px(16.))
            .overflow_y_scroll()
            .children(content)
            .into_any_element()
    }

    fn files_tab(&self, details: &Details, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let running = match &details.node {
            Node::Container(server, id) => self
                .server(server)
                .and_then(|s| s.snapshot.container(id))
                .is_some_and(Container::running),
            _ => false,
        };
        let Some(files) = details.files.as_ref().filter(|_| running) else {
            return centered("Start container to browse files".into(), cx);
        };
        let this = cx.entity().downgrade();
        let path = files.path.clone();
        let parent = match path.trim_end_matches('/').rsplit_once('/') {
            Some(("", _)) => Some("/".to_owned()),
            Some((parent, _)) => Some(format!("{parent}/")),
            None => None,
        };
        let row = |ix: usize, text: String, folder: Option<String>| {
            let this = this.clone();
            h_flex()
                .id(("docker-file", ix))
                .role(Role::ListBoxOption)
                .aria_label(SharedString::from(text.clone()))
                .h(px(spork_workbench::tokens::metrics::ROW_HEIGHT))
                .px(px(8.))
                .gap(px(4.))
                .child(spork_workbench::image(
                    match folder.is_some() {
                        true => "nodes/folder",
                        false => "fileTypes/text",
                    },
                    cx,
                ))
                .child(text)
                .when_some(folder, |row, folder| {
                    row.cursor_pointer().on_click(move |event, _, cx| {
                        if event.click_count() == 2 {
                            this.update(cx, |this, cx| this.show_files(folder.clone(), cx))
                                .log_err();
                        }
                    })
                })
                .into_any_element()
        };
        let rows: Vec<AnyElement> = match &files.entries {
            None => vec![div().p(px(8.)).child("Loading…").into_any_element()],
            Some(Err(error)) => vec![
                div()
                    .p(px(8.))
                    .text_color(tokens.text_error)
                    .child(error.clone())
                    .into_any_element(),
            ],
            Some(Ok(entries)) => parent
                .into_iter()
                .map(|parent| ("..".to_owned(), Some(parent)))
                .chain(entries.iter().map(|entry| {
                    let folder = entry
                        .strip_suffix('/')
                        .map(|_| format!("{}{entry}", files.path));
                    (entry.trim_end_matches('/').to_owned(), folder)
                }))
                .enumerate()
                .map(|(ix, (text, folder))| row(ix, text, folder))
                .collect(),
        };
        v_flex()
            .size_full()
            .child(
                div()
                    .id("docker-files-path")
                    .role(Role::Label)
                    .aria_label(SharedString::from(files.path.clone()))
                    .px(px(8.))
                    .py(px(4.))
                    .border_b_1()
                    .border_color(tokens.separator)
                    .child(files.path.clone()),
            )
            .child(
                v_flex()
                    .id("docker-files")
                    .role(Role::List)
                    .aria_label("Files")
                    .flex_1()
                    .overflow_y_scroll()
                    .children(rows),
            )
            .into_any_element()
    }

    fn layers_tab(&self, details: &Details, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let layers = match &details.layers {
            Some(Ok(layers)) => layers,
            Some(Err(error)) => return centered(error.clone(), cx),
            None => return centered("Analyzing…".into(), cx),
        };
        let header = h_flex()
            .px(px(8.))
            .py(px(4.))
            .border_b_1()
            .border_color(tokens.separator)
            .text_color(tokens.text_secondary)
            .child(div().flex_1().child("Command"))
            .child(div().w(px(100.)).child("Size"));
        v_flex()
            .id("docker-layers")
            .role(Role::List)
            .aria_label("Layers")
            .size_full()
            .overflow_y_scroll()
            .child(header)
            .children(layers.iter().enumerate().map(|(ix, layer)| {
                h_flex()
                    .id(("docker-layer", ix))
                    .role(Role::ListBoxOption)
                    .aria_label(SharedString::from(layer.created_by.clone()))
                    .px(px(8.))
                    .h(px(spork_workbench::tokens::metrics::ROW_HEIGHT))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(layer.created_by.clone()),
                    )
                    .child(div().w(px(100.)).child(size(layer.size)))
            }))
            .into_any_element()
    }

    fn pull_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(pull) = &self.pull else {
            return centered("Nothing to show".into(), cx);
        };
        let this = cx.entity().downgrade();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .p(px(8.))
                    .gap(px(8.))
                    .child("Image to pull:")
                    .child(div().w(px(320.)).child(
                        spork_workbench::TextField::new(&pull.field).aria_label("Image to pull"),
                    ))
                    .child(
                        Button::new("docker-pull", "Pull").on_click(move |_, _, cx| {
                            this.update(cx, |this, cx| this.pull_image(cx)).log_err();
                        }),
                    ),
            )
            .child(
                div()
                    .id("docker-pull-console")
                    .role(Role::Group)
                    .aria_label("Images Console")
                    .flex_1()
                    .min_h_0()
                    .child(pull.console.view.clone()),
            )
            .into_any_element()
    }

    /// The text of what the details show, for the end-to-end tests.
    fn shown_text(&self, cx: &App) -> Option<String> {
        if let Some(Node::Group(_, Group::Images)) = &self.selected {
            return self
                .pull
                .as_ref()
                .map(|pull| pull.console.terminal.read(cx).get_content());
        }
        let details = self.details.as_ref()?;
        let console = |console: &Option<Console>| {
            console
                .as_ref()
                .map(|console| console.terminal.read(cx).get_content())
        };
        match details.tab {
            Tab::Log => console(&details.log),
            Tab::AttachedConsole => console(&details.attached),
            Tab::Inspection => console(&details.inspection),
            Tab::Terminal => details
                .terminal
                .as_ref()
                .map(|(terminal, _)| terminal.read(cx).get_content()),
            Tab::Files => details
                .files
                .as_ref()
                .and_then(|files| match &files.entries {
                    Some(Ok(entries)) => Some(entries.join("\n")),
                    Some(Err(error)) => Some(error.to_string()),
                    None => None,
                }),
            Tab::Layers => details.layers.as_ref().and_then(|layers| {
                Some(
                    layers
                        .as_ref()
                        .ok()?
                        .iter()
                        .map(|layer| layer.created_by.clone())
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            }),
            Tab::Labels => match &details.inspect {
                Some(Ok(inspect)) => Some(
                    labels(&inspect["Config"]["Labels"])
                        .into_iter()
                        .map(|(name, value)| format!("{name}={value}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
                _ => None,
            },
            Tab::Dashboard => None,
        }
    }

    pub(crate) fn e2e_state(&self, cx: &App) -> Value {
        let servers: serde_json::Map<String, Value> = self
            .servers
            .iter()
            .map(|server| {
                let status = match &server.state {
                    State::Disconnected => "Disconnected".to_owned(),
                    State::Connecting => "Connecting".to_owned(),
                    State::Connected(_) => "Connected".to_owned(),
                    State::Failed(error) => format!("Failed: {error}"),
                };
                let snapshot = &server.snapshot;
                let compose: serde_json::Map<String, Value> = snapshot
                    .compose()
                    .into_iter()
                    .map(|(project, services)| {
                        let services: serde_json::Map<String, Value> = services
                            .into_iter()
                            .map(|(service, containers)| {
                                (service, json!(containers.iter().map(|c| c.name()).collect::<Vec<_>>()))
                            })
                            .collect();
                        (project, Value::Object(services))
                    })
                    .collect();
                (
                    server.connection.name.clone(),
                    json!({
                        "status": status,
                        "containers": snapshot.containers.iter()
                            .filter(|c| c.label(COMPOSE_PROJECT).is_none())
                            .map(|c| json!({"name": c.name(), "state": c.state}))
                            .collect::<Vec<_>>(),
                        "compose": compose,
                        "images": snapshot.images.iter().map(image_title).collect::<Vec<_>>(),
                        "networks": snapshot.networks.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
                        "volumes": snapshot.volumes.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(),
                    }),
                )
            })
            .collect();
        let tabs = self.details.as_ref().map(|details| {
            details
                .tabs()
                .into_iter()
                .map(Tab::title)
                .collect::<Vec<_>>()
        });
        json!({
            "servers": servers,
            "selected": self.selected.as_ref().map(|node| format!("{node:?}")),
            "tabs": tabs,
            "tab": self.details.as_ref().map(|details| details.tab.title()),
            "text": self.shown_text(cx).map(|text| text.trim_end().to_owned()),
            "compose_cli": matches!(&self.cli, Some(Some(_))),
            "last_error": self.last_error.as_ref().map(|e| e.as_ref()),
        })
    }
}

fn centered(text: SharedString, cx: &App) -> AnyElement {
    div()
        .id(SharedString::from(format!("docker-centered-{text}")))
        .role(Role::Label)
        .aria_label(text.clone())
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(Tokens::get(cx).text_secondary)
        .child(text)
        .into_any_element()
}

impl Render for Docker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let Some(node) = self.selected.clone() else {
            return div().into_any_element();
        };
        let toolbar = self.item_toolbar(&node, cx);
        let content = match (&node, &self.details) {
            (Node::Server(name), _) => {
                let text = match self.server(name).map(|server| &server.state) {
                    Some(State::Disconnected) => {
                        "Double-click on the server node to connect".into()
                    }
                    Some(State::Connecting) => "Connecting".into(),
                    Some(State::Connected(_)) => "Connected".into(),
                    Some(State::Failed(error)) => format!("Cannot connect: {error}").into(),
                    None => SharedString::default(),
                };
                centered(text, cx)
            }
            (Node::Group(_, Group::Images), _) => self.pull_panel(cx),
            (Node::Compose(server, project) | Node::Service(server, project, _), _) => {
                let service = match &node {
                    Node::Service(_, _, service) => Some(service.as_str()),
                    _ => None,
                };
                let containers: Vec<&Container> = self
                    .server(server)
                    .map(|s| {
                        s.snapshot
                            .containers
                            .iter()
                            .filter(|c| c.label(COMPOSE_PROJECT) == Some(project.as_str()))
                            .filter(|c| service.is_none() || c.label(COMPOSE_SERVICE) == service)
                            .collect()
                    })
                    .unwrap_or_default();
                let running = containers.iter().filter(|c| c.running()).count();
                centered(
                    format!("Running: ({running}/{})", containers.len()).into(),
                    cx,
                )
            }
            (_, Some(details)) => {
                let tabs = details.tabs();
                let selected = details.tab;
                let tab_row = h_flex()
                    .id("docker-tabs")
                    .role(Role::TabList)
                    .aria_label("Docker details")
                    .flex_none()
                    .border_b_1()
                    .border_color(tokens.separator)
                    .children(tabs.into_iter().map(|tab| {
                        let on = tab == selected;
                        v_flex()
                            .id(tab.title())
                            .role(Role::Tab)
                            .aria_label(tab.title())
                            .aria_selected(on)
                            .h(px(32.))
                            .px(px(12.))
                            .justify_center()
                            .cursor_pointer()
                            .child(div().flex_1().flex().items_center().child(tab.title()))
                            .child(
                                div()
                                    .h(px(3.))
                                    .rounded_t(px(2.))
                                    .when(on, |line| line.bg(tokens.accent)),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if tab == Tab::Files
                                    && this.details.as_ref().is_some_and(|d| d.files.is_none())
                                {
                                    this.show_files("/".to_owned(), cx);
                                }
                                this.show_tab(tab, cx)
                            }))
                    }));
                let console = |console: &Option<Console>| match console {
                    Some(console) => console.view.clone().into_any_element(),
                    None => centered("Loading…".into(), cx),
                };
                let body = match selected {
                    Tab::Dashboard => self.dashboard(details, cx),
                    Tab::Log => console(&details.log),
                    Tab::AttachedConsole => console(&details.attached),
                    Tab::Inspection => console(&details.inspection),
                    Tab::Terminal => match &details.terminal {
                        Some((_, view)) => view.clone().into_any_element(),
                        None => centered("Loading…".into(), cx),
                    },
                    Tab::Files => self.files_tab(details, cx),
                    Tab::Layers => self.layers_tab(details, cx),
                    Tab::Labels => match &details.inspect {
                        Some(Ok(inspect)) => v_flex()
                            .id("docker-labels")
                            .p(px(12.))
                            .child(segment("Labels", labels(&inspect["Config"]["Labels"]), cx))
                            .into_any_element(),
                        _ => centered("Loading…".into(), cx),
                    },
                };
                v_flex()
                    .size_full()
                    .child(tab_row)
                    .child(div().flex_1().min_h_0().child(body))
                    .into_any_element()
            }
            _ => centered("Select service to view details".into(), cx),
        };
        h_flex()
            .size_full()
            .items_start()
            .child(toolbar)
            .child(div().flex_1().min_w_0().h_full().child(content))
            .into_any_element()
    }
}

/// IntelliJ's yes/no question over the IDE window (`Messages.showYesNoDialog`): Docker's Delete
/// "Confirm Deletion" and Clean Up, the debugger's "Breakpoint Condition Error".
pub struct Confirm {
    title: SharedString,
    message: SharedString,
    yes: SharedString,
    no: SharedString,
    on_answer: Option<Box<dyn FnOnce(bool, &mut App)>>,
    focus: FocusHandle,
}

impl Confirm {
    fn show(
        workspace: &WeakEntity<Workspace>,
        title: &'static str,
        message: &'static str,
        yes: &'static str,
        on_yes: impl FnOnce(&mut App) + 'static,
        window: &mut Window,
        cx: &mut App,
    ) {
        Self::ask(
            workspace,
            [title, message, yes, "Cancel"].map(SharedString::from),
            move |yes, cx| {
                if yes {
                    on_yes(cx)
                }
            },
            window,
            cx,
        );
    }

    /// Asks `[title, message, yes, no]`; `on_answer` takes whether the answer is yes (⎋ is no).
    pub fn ask(
        workspace: &WeakEntity<Workspace>,
        [title, message, yes, no]: [SharedString; 4],
        on_answer: impl FnOnce(bool, &mut App) + 'static,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(workspace) = workspace.upgrade() else {
            return;
        };
        let on_answer: Box<dyn FnOnce(bool, &mut App)> = Box::new(on_answer);
        // a click in Services' details runs inside the Docker view's update: the modal opens after
        window.defer(cx, move |window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.toggle_modal(window, cx, move |window, cx| {
                    let focus = cx.focus_handle();
                    window.focus(&focus, cx);
                    Confirm {
                        title,
                        message,
                        yes,
                        no,
                        on_answer: Some(on_answer),
                        focus,
                    }
                })
            });
        });
    }

    fn answer(&mut self, yes: bool, cx: &mut Context<Self>) {
        if let Some(on_answer) = self.on_answer.take() {
            on_answer(yes, cx);
        }
        cx.emit(DismissEvent);
    }
}

impl EventEmitter<DismissEvent> for Confirm {}

impl ModalView for Confirm {}

impl Focusable for Confirm {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Confirm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        spork_vcs::spork_look(cx).child(
            popup::surface(cx)
                .id("docker-confirm")
                .role(Role::Dialog)
                .aria_label(self.title.clone())
                .w(px(400.))
                .track_focus(&self.focus)
                .key_context("DockerConfirm menu")
                .on_action(cx.listener(|this, _: &menu::Confirm, _, cx| this.answer(true, cx)))
                .on_action(cx.listener(|this, _: &menu::Cancel, _, cx| this.answer(false, cx)))
                .child(popup::title(self.title.clone(), cx))
                .child(
                    v_flex()
                        .px(px(16.))
                        .pb(px(14.))
                        .gap(px(12.))
                        .child(
                            div()
                                .id("docker-confirm-message")
                                .role(Role::Label)
                                .aria_label(self.message.clone())
                                .child(self.message.clone()),
                        )
                        .child(
                            h_flex()
                                .justify_end()
                                .gap(px(8.))
                                .child(
                                    Button::new("docker-confirm-no", self.no.clone()).on_click(
                                        cx.listener(|this, _, _, cx| this.answer(false, cx)),
                                    ),
                                )
                                .child(
                                    Button::new("docker-confirm-yes", self.yes.clone())
                                        .primary(true)
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.answer(true, cx)),
                                        ),
                                ),
                        ),
                ),
        )
    }
}

/// A dialog window of its own with a form and OK and Cancel, as IntelliJ's `DialogWrapper`.
pub(crate) fn open_dialog<V: Render + 'static>(
    title: &'static str,
    width: f32,
    height: f32,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V + 'static,
) {
    let options = gpui::WindowOptions {
        window_bounds: Some(gpui::WindowBounds::centered(
            None,
            gpui::size(px(width), px(height)),
            cx,
        )),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some(title.into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(12.), px(8.))),
        }),
        ..Default::default()
    };
    cx.defer(move |cx| {
        if let Err(error) = gpui_kit::open_window(options, cx, move |window, cx| {
            crate::configurations_dialog::follow_ide_theme(window, cx);
            window.activate_window();
            cx.new(|cx| build(window, cx))
        }) {
            log::error!("failed to open {title}: {error:#}");
        }
    });
}

pub(crate) fn dialog_frame(
    title: &'static str,
    body: AnyElement,
    buttons: AnyElement,
    cx: &App,
) -> AnyElement {
    let tokens = Tokens::get(cx);
    v_flex()
        .id(title)
        .role(Role::Dialog)
        .aria_label(title)
        .size_full()
        .bg(tokens.panel)
        .text_color(tokens.text)
        .child(
            div()
                .h(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .child(title),
        )
        .child(div().flex_1().min_h_0().p(px(16.)).child(body))
        .child(h_flex().justify_end().gap(px(8.)).p(px(12.)).child(buttons))
        .into_any_element()
}

/// Add Service ▸ Docker Connection: IntelliJ's "New Docker Connection" dialog, the Docker page's
/// form; OK adds the connection and connects to it.
pub(crate) struct NewConnection {
    form: Entity<spork_docker::ConnectionForm>,
    docker: WeakEntity<Docker>,
}

impl NewConnection {
    pub(crate) fn open(docker: WeakEntity<Docker>, cx: &mut App) {
        let store = ConnectionStore::global(cx);
        let taken: Vec<String> = store
            .read(cx)
            .list()
            .iter()
            .map(|connection| connection.name.clone())
            .collect();
        let taken: Vec<&str> = taken.iter().map(String::as_str).collect();
        let connection = Connection::new(connections::unique_name(&taken));
        open_dialog(
            "New Docker Connection",
            640.,
            380.,
            cx,
            move |window, cx| {
                let form = cx.new(|cx| spork_docker::ConnectionForm::new(connection, window, cx));
                NewConnection { form, docker }
            },
        );
    }

    fn ok(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let connection = self.form.read(cx).connection().clone();
        let name: SharedString = connection.name.clone().into();
        let store = ConnectionStore::global(cx);
        store.update(cx, |store, cx| store.add(connection, cx));
        self.docker
            .update(cx, |docker, cx| {
                // the store tells of the new connection later: Services learns of it now
                docker.sync(cx);
                docker.connect(&name, cx)
            })
            .log_err();
        window.remove_window();
    }
}

impl Render for NewConnection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name_taken = {
            let name = self.form.read(cx).connection().name.clone();
            name.trim().is_empty()
                || ConnectionStore::global(cx)
                    .read(cx)
                    .list()
                    .iter()
                    .any(|connection| connection.name == name)
        };
        let buttons = h_flex()
            .gap(px(8.))
            .child(
                Button::new("docker-new-cancel", "Cancel")
                    .on_click(|_, window, _| window.remove_window()),
            )
            .child(
                Button::new("docker-new-ok", "OK")
                    .primary(true)
                    .disabled(name_taken)
                    .on_click(cx.listener(|this, _, window, cx| this.ok(window, cx))),
            )
            .into_any_element();
        dialog_frame(
            "New Docker Connection",
            self.form.clone().into_any_element(),
            buttons,
            cx,
        )
    }
}

/// Create Container (IntelliJ's run configuration form for an image, its basic fields): the
/// image, the container's name, its ports, environment variables and command; Run creates and
/// starts it.
struct CreateContainer {
    docker: WeakEntity<Docker>,
    server: SharedString,
    image: Entity<InputState>,
    name: Entity<InputState>,
    ports: Entity<InputState>,
    env: Entity<InputState>,
    command: Entity<InputState>,
}

impl CreateContainer {
    fn open(docker: WeakEntity<Docker>, server: SharedString, image: String, cx: &mut App) {
        open_dialog("Create Container", 600., 360., cx, move |window, cx| {
            let field = |text: &str,
                         placeholder: &'static str,
                         window: &mut Window,
                         cx: &mut Context<Self>| {
                let text = text.to_owned();
                cx.new(|cx| {
                    let mut input = InputState::new(window, cx).placeholder(placeholder);
                    input.set_value(text, window, cx);
                    input
                })
            };
            let image = field(&image, "", window, cx);
            let name = field("", "", window, cx);
            name.update(cx, |name, cx| name.focus(window, cx));
            CreateContainer {
                docker,
                server,
                image,
                name,
                ports: field("", "8080:80", window, cx),
                env: field("", "NAME=value;OTHER=value", window, cx),
                command: field("", "", window, cx),
            }
        });
    }

    fn new_container(&self, cx: &App) -> NewContainer {
        let text = |input: &Entity<InputState>| input.read(cx).value().trim().to_owned();
        NewContainer {
            image: text(&self.image),
            name: text(&self.name),
            command: text(&self.command)
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
            env: text(&self.env)
                .split(';')
                .map(str::trim)
                .filter(|pair| !pair.is_empty())
                .map(str::to_owned)
                .collect(),
            ports: text(&self.ports)
                .split([',', ' '])
                .filter_map(|pair| {
                    let (host, container) = pair.trim().split_once(':')?;
                    Some((host.to_owned(), container.to_owned()))
                })
                .collect(),
        }
    }

    fn run(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let new = self.new_container(cx);
        let server = self.server.clone();
        self.docker
            .update(cx, |docker, cx| docker.create_container(server, new, cx))
            .log_err();
        window.remove_window();
    }
}

impl Render for CreateContainer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row = |label: &'static str, input: &Entity<InputState>| {
            h_flex()
                .gap(px(8.))
                .child(
                    div()
                        .w(px(180.))
                        .flex_none()
                        .whitespace_nowrap()
                        .child(label),
                )
                .child(div().flex_1().child(
                    spork_workbench::TextField::new(input).aria_label(label.trim_end_matches(':')),
                ))
        };
        let empty_image = self.image.read(cx).value().trim().is_empty();
        let body = v_flex()
            .gap(px(10.))
            .child(row("Image ID or name:", &self.image))
            .child(row("Container name:", &self.name))
            .child(row("Bind ports:", &self.ports))
            .child(row("Environment variables:", &self.env))
            .child(row("Command:", &self.command))
            .into_any_element();
        let buttons = h_flex()
            .gap(px(8.))
            .child(
                Button::new("docker-create-cancel", "Cancel")
                    .on_click(|_, window, _| window.remove_window()),
            )
            .child(
                Button::new("docker-create-run", "Run")
                    .primary(true)
                    .disabled(empty_image)
                    .on_click(cx.listener(|this, _, window, cx| this.run(window, cx))),
            )
            .into_any_element();
        dialog_frame("Create Container", body, buttons, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::{pull_line, size};

    #[test]
    fn writes_pull_progress_and_sizes() {
        assert_eq!(
            pull_line(r#"{"status":"Pulling from library/nginx","id":"latest"}"#).as_deref(),
            Some("latest: Pulling from library/nginx\n")
        );
        assert_eq!(
            pull_line(r#"{"status":"Downloading","id":"a1","progress":"[=>  ] 1MB/2MB"}"#)
                .as_deref(),
            Some("a1: Downloading [=>  ] 1MB/2MB\n")
        );
        assert_eq!(
            pull_line(r#"{"error":"manifest unknown"}"#).as_deref(),
            Some("manifest unknown\n")
        );
        assert_eq!(pull_line("not json"), None);
        assert_eq!(size(187_654_321), "187.7 MB");
        assert_eq!(size(512), "512 B");
        assert_eq!(size(2_500_000_000), "2.50 GB");
    }
}
