//! The Docker connections, saved per IDE (IntelliJ's Settings › Build, Execution, Deployment ›
//! Docker), and the local daemon found as IntelliJ finds it: Docker Desktop's socket, Colima's,
//! OrbStack's, Podman's, or the system's `/var/run/docker.sock`.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use gpui::{App, AppContext as _, Context, Entity, Global, TaskExt as _};
use serde::{Deserialize, Serialize};
use util::ResultExt as _;

/// How a connection reaches its daemon: IntelliJ's "Connect to Docker daemon with:" choices.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// Docker Desktop's socket: `~/.docker/run/docker.sock`, `/var/run/docker.sock` before it.
    #[default]
    DockerForMac,
    /// Colima's default profile's socket.
    Colima,
    /// A socket path of the user's.
    UnixSocket,
    /// An Engine API URL of the user's (`tcp://`, or a `unix://` path).
    Tcp,
}

impl Kind {
    pub const ALL: [Kind; 4] = [
        Kind::DockerForMac,
        Kind::Colima,
        Kind::UnixSocket,
        Kind::Tcp,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Kind::DockerForMac => "Docker for Mac",
            Kind::Colima => "Colima",
            Kind::UnixSocket => "Unix socket:",
            Kind::Tcp => "TCP socket",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub name: String,
    pub kind: Kind,
    /// The Unix socket's path, for `Kind::UnixSocket`.
    #[serde(default)]
    pub socket: String,
    /// The Engine API URL, for `Kind::Tcp`.
    #[serde(default)]
    pub url: String,
}

/// The socket IntelliJ's Unix socket choice starts with, and its TCP socket's URL.
pub const DEFAULT_SOCKET: &str = "/var/run/docker.sock";
pub const DEFAULT_URL: &str = "tcp://localhost:2375";

impl Connection {
    /// A new connection as Add makes it: Docker for Mac, the socket fields at IntelliJ's defaults.
    pub fn new(name: String) -> Self {
        Connection {
            name,
            kind: Kind::DockerForMac,
            socket: DEFAULT_SOCKET.to_owned(),
            url: DEFAULT_URL.to_owned(),
        }
    }

    /// The Engine API URL this connection reaches, its socket found under `home`.
    pub fn api_url(&self, home: &Path) -> Result<String> {
        let url = match self.kind {
            Kind::DockerForMac => socket_url(&docker_desktop_socket(home, SYSTEM_SOCKET)),
            Kind::Colima => socket_url(&colima_socket(home)),
            Kind::UnixSocket => match self.socket.trim() {
                "" => bail!("Specify the socket path"),
                socket if socket.starts_with("unix://") => socket.to_owned(),
                socket => format!("unix://{socket}"),
            },
            Kind::Tcp => match self.url.trim() {
                "" => bail!("API URL required"),
                url => url.to_owned(),
            },
        };
        Ok(url)
    }
}

const SYSTEM_SOCKET: &str = "/var/run/docker.sock";

fn socket_url(path: &Path) -> String {
    format!("unix://{}", path.display())
}

/// Docker Desktop's socket: the user's (`~/.docker/run/docker.sock`) when there is one, the
/// system's otherwise, as IntelliJ's Docker for Mac picks it.
pub fn docker_desktop_socket(home: &Path, system: &str) -> PathBuf {
    let user = home.join(".docker/run/docker.sock");
    match user.exists() {
        true => user,
        false => PathBuf::from(system),
    }
}

/// Colima's default profile's socket: under `$COLIMA_HOME`, `~/.colima` or `~/.config/colima`.
pub fn colima_socket(home: &Path) -> PathBuf {
    let candidates = colima_homes(home)
        .into_iter()
        .map(|folder| folder.join("default/docker.sock"))
        .collect::<Vec<_>>();
    candidates
        .iter()
        .find(|socket| socket.exists())
        .or(candidates.first())
        .cloned()
        .unwrap_or_else(|| home.join(".colima/default/docker.sock"))
}

fn colima_homes(home: &Path) -> Vec<PathBuf> {
    let mut homes = Vec::new();
    if let Some(colima) = std::env::var_os("COLIMA_HOME") {
        homes.push(PathBuf::from(colima));
    }
    homes.push(home.join(".colima"));
    homes.push(home.join(".config/colima"));
    homes
}

/// The local daemons' sockets, in the order a default connection is picked from: Docker
/// Desktop's, Colima's, OrbStack's, Podman's machine's, then the system's.
fn local_sockets(home: &Path, system: &str) -> Vec<(Kind, PathBuf)> {
    let mut sockets = vec![(Kind::DockerForMac, home.join(".docker/run/docker.sock"))];
    sockets.extend(
        colima_homes(home)
            .into_iter()
            .map(|folder| (Kind::Colima, folder.join("default/docker.sock"))),
    );
    sockets.push((Kind::UnixSocket, home.join(".orbstack/run/docker.sock")));
    let podman = home.join(".local/share/containers/podman/machine");
    sockets.push((Kind::UnixSocket, podman.join("podman.sock")));
    sockets.push((
        Kind::UnixSocket,
        podman.join("podman-machine-default/podman.sock"),
    ));
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        sockets.push((
            Kind::UnixSocket,
            PathBuf::from(runtime).join("podman/podman.sock"),
        ));
    }
    sockets.push((Kind::DockerForMac, PathBuf::from(system)));
    sockets
}

/// The connection IntelliJ adds on its own when a local daemon's socket exists: "Docker".
pub fn detect(home: &Path, system: &str) -> Option<Connection> {
    let (kind, socket) = local_sockets(home, system)
        .into_iter()
        .find(|(_, socket)| socket.exists())?;
    let mut connection = Connection::new("Docker".to_owned());
    connection.kind = kind;
    if kind == Kind::UnixSocket {
        connection.socket = socket.display().to_string();
    }
    Some(connection)
}

/// A name no connection has: "Docker", then "Docker2", "Docker3"… (IntelliJ's
/// `UniqueNameGenerator`).
pub fn unique_name(taken: &[&str]) -> String {
    (1..)
        .map(|n| match n {
            1 => "Docker".to_owned(),
            n => format!("Docker{n}"),
        })
        .find(|name| !taken.contains(&name.as_str()))
        .unwrap_or_else(|| "Docker".to_owned())
}

/// The key the connections are kept under, for the whole IDE.
const KEY: &str = "spork_docker_connections";

/// The IDE's Docker connections: the saved ones, or the local daemon's until some are saved.
pub struct ConnectionStore {
    list: Vec<Connection>,
    saved: bool,
}

struct GlobalStore(Entity<ConnectionStore>);

impl Global for GlobalStore {}

impl ConnectionStore {
    pub fn global(cx: &mut App) -> Entity<ConnectionStore> {
        if let Some(store) = cx.try_global::<GlobalStore>() {
            return store.0.clone();
        }
        let saved = db::kvp::KeyValueStore::global(cx)
            .read_kvp(KEY)
            .log_err()
            .flatten()
            .and_then(|value| serde_json::from_str::<Vec<Connection>>(&value).log_err());
        let store = cx.new(|_| ConnectionStore {
            saved: saved.is_some(),
            list: saved.unwrap_or_else(|| detected().into_iter().collect()),
        });
        cx.set_global(GlobalStore(store.clone()));
        store
    }

    pub fn list(&self) -> &[Connection] {
        &self.list
    }

    /// Looks for a local daemon again while no connection was saved: one may have started.
    pub fn detect(&mut self, cx: &mut Context<Self>) {
        if self.saved {
            return;
        }
        let list: Vec<Connection> = detected().into_iter().collect();
        if list != self.list {
            self.list = list;
            cx.notify();
        }
    }

    pub fn set(&mut self, list: Vec<Connection>, cx: &mut Context<Self>) {
        self.list = list;
        self.saved = true;
        cx.notify();
        let value = serde_json::to_string(&self.list);
        let store = db::kvp::KeyValueStore::global(cx);
        cx.background_spawn(async move { store.write_kvp(KEY.to_owned(), value?).await })
            .detach_and_log_err(cx);
    }

    pub fn add(&mut self, connection: Connection, cx: &mut Context<Self>) {
        let mut list = self.list.clone();
        list.push(connection);
        self.set(list, cx);
    }

    pub fn remove(&mut self, name: &str, cx: &mut Context<Self>) {
        let list = self
            .list
            .iter()
            .filter(|connection| connection.name != name)
            .cloned()
            .collect();
        self.set(list, cx);
    }
}

fn detected() -> Option<Connection> {
    detect(paths::home_dir(), SYSTEM_SOCKET)
}

/// The Engine API URL of a connection, its socket found under the user's home.
pub fn api_url(connection: &Connection) -> Result<String> {
    connection.api_url(paths::home_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    #[test]
    fn detects_the_local_daemon() {
        let home = tempfile::tempdir().unwrap();
        let none = home.path().join("no-system.sock");
        let none = none.to_str().unwrap();
        assert_eq!(detect(home.path(), none), None);

        let orbstack = home.path().join(".orbstack/run/docker.sock");
        touch(&orbstack);
        let found = detect(home.path(), none).unwrap();
        assert_eq!(found.name, "Docker");
        assert_eq!(found.kind, Kind::UnixSocket);
        assert_eq!(found.socket, orbstack.display().to_string());

        touch(&home.path().join(".colima/default/docker.sock"));
        assert_eq!(detect(home.path(), none).unwrap().kind, Kind::Colima);
        assert_eq!(
            Connection {
                kind: Kind::Colima,
                ..Connection::new("c".into())
            }
            .api_url(home.path())
            .unwrap(),
            format!(
                "unix://{}",
                home.path().join(".colima/default/docker.sock").display()
            )
        );

        let desktop = home.path().join(".docker/run/docker.sock");
        assert_eq!(
            docker_desktop_socket(home.path(), "/var/run/docker.sock"),
            PathBuf::from("/var/run/docker.sock")
        );
        touch(&desktop);
        assert_eq!(detect(home.path(), none).unwrap().kind, Kind::DockerForMac);
        assert_eq!(docker_desktop_socket(home.path(), none), desktop);
    }

    #[test]
    fn builds_urls_and_names() {
        let home = Path::new("/nowhere");
        let mut connection = Connection::new("Docker".into());
        connection.kind = Kind::UnixSocket;
        assert_eq!(
            connection.api_url(home).unwrap(),
            "unix:///var/run/docker.sock"
        );
        connection.socket = "unix:///tmp/d.sock".into();
        assert_eq!(connection.api_url(home).unwrap(), "unix:///tmp/d.sock");
        connection.socket = " ".into();
        assert!(connection.api_url(home).is_err());
        connection.kind = Kind::Tcp;
        assert_eq!(connection.api_url(home).unwrap(), "tcp://localhost:2375");
        assert_eq!(unique_name(&[]), "Docker");
        assert_eq!(unique_name(&["Docker"]), "Docker2");
        assert_eq!(unique_name(&["Docker", "Docker2"]), "Docker3");
    }
}
