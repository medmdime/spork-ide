//! The Docker Engine API, the versioned REST API a Docker (or Podman, Colima, OrbStack) daemon
//! serves on its socket: HTTP/1.1 over a Unix socket or TCP, one connection a request, blocking on
//! the caller's thread (the IDE calls it from background threads). Streams (logs, events, a pull's
//! progress) and hijacked connections (an exec's terminal) keep their connection open.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Value, json};

// shortcut: no Unix sockets on Windows, so `Socket::Unix` is never made there and a daemon is
// reached over tcp://; Docker Desktop's named pipe comes when Windows is supported.
#[cfg(not(unix))]
type UnixStream = TcpStream;

/// The newest API version Spork IDE asks for; an older daemon's own is used instead.
const MAX_API_VERSION: (u32, u32) = (1, 45);
/// How long a request (not a stream) may wait for the daemon.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Where a daemon listens: `unix:///var/run/docker.sock` or `tcp://localhost:2375`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    Unix(PathBuf),
    Tcp(String),
}

impl Endpoint {
    /// The endpoint of an Engine API URL; a bare path is a Unix socket.
    pub fn parse(url: &str) -> Result<Self> {
        let url = url.trim();
        if let Some(path) = url.strip_prefix("unix://") {
            if path.is_empty() {
                bail!("the socket path is empty");
            }
            return Ok(Endpoint::Unix(PathBuf::from(path)));
        }
        if url.starts_with('/') {
            return Ok(Endpoint::Unix(PathBuf::from(url)));
        }
        let address = url
            .strip_prefix("tcp://")
            .or_else(|| url.strip_prefix("http://"))
            .ok_or_else(|| anyhow!("unsupported URL: {url}"))?
            .trim_end_matches('/');
        if address.is_empty() {
            bail!("the host is empty");
        }
        let address = match address.rsplit_once(':') {
            Some((_, port)) if port.parse::<u16>().is_ok() => address.to_owned(),
            _ => format!("{address}:2375"),
        };
        Ok(Endpoint::Tcp(address))
    }

    fn connect(&self, timeout: Option<Duration>) -> Result<Socket> {
        let socket = match self {
            #[cfg(unix)]
            Endpoint::Unix(path) => Socket::Unix(
                UnixStream::connect(path)
                    .with_context(|| format!("cannot connect to {}", path.display()))?,
            ),
            #[cfg(not(unix))]
            Endpoint::Unix(path) => bail!(
                "cannot connect to {}: Unix sockets aren't supported on this system",
                path.display()
            ),
            Endpoint::Tcp(address) => Socket::Tcp(
                TcpStream::connect(address)
                    .with_context(|| format!("cannot connect to {address}"))?,
            ),
        };
        socket.set_timeout(timeout)?;
        Ok(socket)
    }
}

/// A connection to the daemon.
pub enum Socket {
    Unix(UnixStream),
    Tcp(TcpStream),
}

impl Socket {
    pub fn try_clone(&self) -> Result<Socket> {
        Ok(match self {
            Socket::Unix(stream) => Socket::Unix(stream.try_clone()?),
            Socket::Tcp(stream) => Socket::Tcp(stream.try_clone()?),
        })
    }

    /// Ends the connection, waking a thread blocked reading it.
    pub fn shutdown(&self) {
        let shut = match self {
            Socket::Unix(stream) => stream.shutdown(Shutdown::Both),
            Socket::Tcp(stream) => stream.shutdown(Shutdown::Both),
        };
        // the daemon may have closed it first
        if let Err(error) = shut {
            log::debug!("closing a Docker connection: {error}");
        }
    }

    /// No timeout from now on; macOS refuses it once the daemon has closed its end, and then the
    /// next read ends the stream anyway.
    fn clear_timeout(&self) {
        if let Err(error) = self.set_timeout(None) {
            log::debug!("clearing a Docker connection's timeout: {error}");
        }
    }

    fn set_timeout(&self, timeout: Option<Duration>) -> Result<()> {
        match self {
            Socket::Unix(stream) => {
                stream.set_read_timeout(timeout)?;
                stream.set_write_timeout(timeout)?;
            }
            Socket::Tcp(stream) => {
                stream.set_read_timeout(timeout)?;
                stream.set_write_timeout(timeout)?;
            }
        }
        Ok(())
    }
}

impl Read for Socket {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Socket::Unix(stream) => stream.read(buf),
            Socket::Tcp(stream) => stream.read(buf),
        }
    }
}

impl Write for Socket {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Socket::Unix(stream) => stream.write(buf),
            Socket::Tcp(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Socket::Unix(stream) => stream.flush(),
            Socket::Tcp(stream) => stream.flush(),
        }
    }
}

/// A response's status line and headers.
#[derive(Debug, PartialEq)]
struct Head {
    status: u16,
    headers: Vec<(String, String)>,
}

impl Head {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn body_kind(&self) -> BodyKind {
        if self
            .header("transfer-encoding")
            .is_some_and(|value| value.eq_ignore_ascii_case("chunked"))
        {
            return BodyKind::Chunked;
        }
        match self
            .header("content-length")
            .and_then(|value| value.trim().parse().ok())
        {
            Some(length) => BodyKind::Length(length),
            // 204 and 304 have no body; others last until the daemon closes the connection
            None if matches!(self.status, 204 | 304) => BodyKind::Length(0),
            None => BodyKind::ToEnd,
        }
    }
}

fn read_head(reader: &mut impl BufRead) -> Result<Head> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let status = match (parts.next(), parts.next()) {
        (Some(version), Some(status)) if version.starts_with("HTTP/") => status
            .parse()
            .with_context(|| format!("bad status line: {}", line.trim()))?,
        _ if line.is_empty() => bail!("the daemon closed the connection"),
        _ => bail!("bad status line: {}", line.trim()),
    };
    let mut headers = Vec::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            bail!("the daemon closed the connection in the headers");
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.push((name.trim().to_owned(), value.trim().to_owned()));
        }
    }
    Ok(Head { status, headers })
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum BodyKind {
    Length(usize),
    Chunked,
    ToEnd,
}

/// A response's body as its bytes come, its chunks decoded.
pub struct Body<R> {
    reader: R,
    kind: BodyKind,
    /// What is left of the current chunk, or of the body for `Length`.
    left: usize,
    done: bool,
}

impl<R: BufRead> Body<R> {
    fn new(reader: R, kind: BodyKind) -> Self {
        let left = match kind {
            BodyKind::Length(length) => length,
            _ => 0,
        };
        Self {
            reader,
            kind,
            left,
            done: kind == BodyKind::Length(0),
        }
    }

    fn next_chunk(&mut self) -> std::io::Result<()> {
        let mut line = String::new();
        // a chunk's data ends in CRLF before the next size line
        loop {
            line.clear();
            if self.reader.read_line(&mut line)? == 0 {
                self.done = true;
                return Ok(());
            }
            if !line.trim().is_empty() {
                break;
            }
        }
        let size = line.trim().split(';').next().unwrap_or_default();
        self.left = usize::from_str_radix(size, 16).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("bad chunk size: {size:?}"),
            )
        })?;
        if self.left == 0 {
            self.done = true;
        }
        Ok(())
    }
}

impl<R: BufRead> Read for Body<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.done || buf.is_empty() {
            return Ok(0);
        }
        match self.kind {
            BodyKind::ToEnd => self.reader.read(buf),
            BodyKind::Length(_) => {
                let max = buf.len().min(self.left);
                let read = self.reader.read(&mut buf[..max])?;
                self.left -= read;
                if read == 0 || self.left == 0 {
                    self.done = true;
                }
                Ok(read)
            }
            BodyKind::Chunked => {
                if self.left == 0 {
                    self.next_chunk()?;
                    if self.done {
                        return Ok(0);
                    }
                }
                let max = buf.len().min(self.left);
                let read = self.reader.read(&mut buf[..max])?;
                if read == 0 {
                    self.done = true;
                }
                self.left -= read;
                Ok(read)
            }
        }
    }
}

/// An open response: its body read as it comes, and the connection to end it early.
pub struct Stream {
    pub body: Body<BufReader<Socket>>,
    closer: Socket,
}

impl Stream {
    /// A handle that ends the stream from another thread.
    pub fn closer(&self) -> Result<Socket> {
        self.closer.try_clone()
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.body.read(buf)
    }
}

/// A connection the daemon handed over to a process's input and output (an exec, an attach).
pub struct Hijacked {
    /// The process's output, the bytes the daemon read ahead included.
    pub output: BufReader<Socket>,
    /// The process's input.
    pub input: Socket,
}

/// Which of a process's streams a frame of a multiplexed log is from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    Stdout,
    Stderr,
}

/// Reads the next frame of a stream the daemon multiplexes (a container without a terminal): an
/// 8-byte header, its stream and the payload's length, then the payload. `None` at its end.
pub fn read_frame(reader: &mut impl Read) -> Result<Option<(Output, Vec<u8>)>> {
    let mut header = [0u8; 8];
    let mut filled = 0;
    while filled < header.len() {
        let read = reader.read(&mut header[filled..])?;
        if read == 0 {
            return match filled {
                0 => Ok(None),
                _ => Err(anyhow!("the stream ended inside a frame's header")),
            };
        }
        filled += read;
    }
    let [kind, _, _, _, a, b, c, d] = header;
    let length = u32::from_be_bytes([a, b, c, d]) as usize;
    let mut payload = vec![0; length];
    reader
        .read_exact(&mut payload)
        .context("the stream ended inside a frame")?;
    let output = match kind {
        2 => Output::Stderr,
        _ => Output::Stdout,
    };
    Ok(Some((output, payload)))
}

/// A container, as the list gives it (`GET /containers/json`).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct Container {
    pub id: String,
    pub names: Vec<String>,
    pub image: String,
    #[serde(rename = "ImageID")]
    pub image_id: String,
    pub state: String,
    pub status: String,
    pub labels: Option<BTreeMap<String, String>>,
    pub created: i64,
    pub mounts: Vec<Mount>,
}

/// A volume or a folder of the host a container has mounted.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct Mount {
    #[serde(rename = "Type")]
    pub kind: String,
    pub name: String,
    pub source: String,
    pub destination: String,
}

pub const COMPOSE_PROJECT: &str = "com.docker.compose.project";
pub const COMPOSE_SERVICE: &str = "com.docker.compose.service";
pub const COMPOSE_FILES: &str = "com.docker.compose.project.config_files";
pub const COMPOSE_DIRECTORY: &str = "com.docker.compose.project.working_dir";

impl Container {
    /// Its name, without the leading slash the daemon writes.
    pub fn name(&self) -> &str {
        self.names
            .first()
            .map(|name| name.trim_start_matches('/'))
            .unwrap_or(&self.id)
    }

    pub fn label(&self, key: &str) -> Option<&str> {
        self.labels.as_ref()?.get(key).map(String::as_str)
    }

    pub fn running(&self) -> bool {
        self.state == "running"
    }

    pub fn paused(&self) -> bool {
        self.state == "paused"
    }
}

/// An image, as the list gives it (`GET /images/json`).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct Image {
    pub id: String,
    pub repo_tags: Option<Vec<String>>,
    pub size: i64,
    pub created: i64,
}

impl Image {
    /// Its tags, `<none>:<none>` left out.
    pub fn tags(&self) -> Vec<&str> {
        self.repo_tags
            .iter()
            .flatten()
            .map(String::as_str)
            .filter(|tag| *tag != "<none>:<none>")
            .collect()
    }

    /// Its id without the digest's algorithm, 12 characters, as `docker images` shows it.
    pub fn short_id(&self) -> &str {
        short_id(&self.id)
    }
}

pub fn short_id(id: &str) -> &str {
    let id = id.strip_prefix("sha256:").unwrap_or(id);
    id.get(..12).unwrap_or(id)
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct Network {
    pub name: String,
    pub id: String,
    pub driver: String,
    pub scope: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct Volume {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
}

/// A layer of an image, as its history gives it (`GET /images/{id}/history`).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct Layer {
    pub id: String,
    pub created: i64,
    pub created_by: String,
    pub size: i64,
    pub comment: String,
}

/// What `prune` removes: IntelliJ's Clean Up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prune {
    Containers,
    /// The untagged images no container uses.
    DanglingImages,
    /// Every image no container uses.
    UnusedImages,
    Networks,
    Volumes,
    BuildCache,
}

/// What Create Container sends: the image, its name and the basic fields.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NewContainer {
    pub image: String,
    pub name: String,
    pub command: Vec<String>,
    pub env: Vec<String>,
    /// `host:container` port pairs, `container/udp` for UDP.
    pub ports: Vec<(String, String)>,
}

impl NewContainer {
    fn body(&self) -> Value {
        let mut exposed = serde_json::Map::new();
        let mut bindings = serde_json::Map::new();
        for (host, container) in &self.ports {
            let port = match container.contains('/') {
                true => container.clone(),
                false => format!("{container}/tcp"),
            };
            exposed.insert(port.clone(), json!({}));
            bindings.insert(port, json!([{"HostPort": host}]));
        }
        let mut body = json!({
            "Image": self.image,
            "Env": self.env,
            "ExposedPorts": exposed,
            "HostConfig": {"PortBindings": bindings},
        });
        if !self.command.is_empty()
            && let Some(object) = body.as_object_mut()
        {
            object.insert("Cmd".into(), json!(self.command));
        }
        body
    }
}

/// A daemon reached at its endpoint, with the API version both speak.
#[derive(Clone, Debug)]
pub struct Engine {
    endpoint: Endpoint,
    /// `v1.45`.
    version: String,
}

impl Engine {
    /// Reaches the daemon at `url` and settles on the API version: the daemon's, or Spork IDE's
    /// newest when the daemon's is newer.
    pub fn connect(url: &str) -> Result<Engine> {
        let endpoint = Endpoint::parse(url)?;
        let unversioned = Engine {
            endpoint,
            version: String::new(),
        };
        unversioned.ping()?;
        let version = unversioned.get("/version")?;
        let theirs = version["ApiVersion"]
            .as_str()
            .and_then(parse_version)
            .context("the daemon didn't tell its API version")?;
        let (major, minor) = theirs.min(MAX_API_VERSION);
        Ok(Engine {
            version: format!("v{major}.{minor}"),
            ..unversioned
        })
    }

    /// The engine at `url` speaking `version` (`v1.45`), as a terminal's bridge is handed it.
    pub fn with_version(url: &str, version: &str) -> Result<Engine> {
        Ok(Engine {
            endpoint: Endpoint::parse(url)?,
            version: version.to_owned(),
        })
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    fn path(&self, path: &str) -> String {
        match self.version.is_empty() {
            true => path.to_owned(),
            false => format!("/{}{path}", self.version),
        }
    }

    fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        upgrade: bool,
        timeout: Option<Duration>,
    ) -> Result<(Head, BufReader<Socket>)> {
        let mut socket = self.endpoint.connect(timeout)?;
        let request = request_bytes(method, &self.path(path), body, upgrade)?;
        socket.write_all(&request)?;
        socket.flush()?;
        let mut reader = BufReader::new(socket);
        let head = read_head(&mut reader)?;
        Ok((head, reader))
    }

    /// A request answered with its whole body; an error status becomes the daemon's message.
    pub fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Vec<u8>> {
        let (head, reader) = self.send(method, path, body, false, Some(REQUEST_TIMEOUT))?;
        let kind = head.body_kind();
        let mut bytes = Vec::new();
        Body::new(reader, kind).read_to_end(&mut bytes)?;
        check_status(head.status, &bytes)?;
        Ok(bytes)
    }

    fn get(&self, path: &str) -> Result<Value> {
        let bytes = self.request("GET", path, None)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// A request whose body comes as the daemon writes it, until it ends or is closed.
    pub fn stream(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Stream> {
        let (head, reader) = self.send(method, path, body, false, Some(REQUEST_TIMEOUT))?;
        let kind = head.body_kind();
        if !(200..300).contains(&head.status) {
            let mut bytes = Vec::new();
            Body::new(reader, kind).read_to_end(&mut bytes)?;
            check_status(head.status, &bytes)?;
            bail!("the daemon answered {}", head.status);
        }
        // a log or the events wait as long as the daemon has nothing to say
        reader.get_ref().clear_timeout();
        let closer = reader.get_ref().try_clone()?;
        Ok(Stream {
            body: Body::new(reader, kind),
            closer,
        })
    }

    /// A request after which the connection carries a process's input and output.
    pub fn hijack(&self, path: &str, body: &Value) -> Result<Hijacked> {
        let (head, mut reader) =
            self.send("POST", path, Some(body), true, Some(REQUEST_TIMEOUT))?;
        if !matches!(head.status, 101 | 200) {
            let mut bytes = Vec::new();
            Body::new(&mut reader, head.body_kind()).read_to_end(&mut bytes)?;
            check_status(head.status, &bytes)?;
            bail!("the daemon answered {}", head.status);
        }
        reader.get_ref().clear_timeout();
        let input = reader.get_ref().try_clone()?;
        Ok(Hijacked {
            output: reader,
            input,
        })
    }

    pub fn ping(&self) -> Result<()> {
        let bytes = self.request("GET", "/_ping", None)?;
        match bytes.trim_ascii() {
            b"OK" => Ok(()),
            other => bail!(
                "the daemon answered the ping with {:?}",
                String::from_utf8_lossy(other)
            ),
        }
    }

    pub fn containers(&self) -> Result<Vec<Container>> {
        Ok(serde_json::from_value(self.get("/containers/json?all=1")?)?)
    }

    pub fn images(&self) -> Result<Vec<Image>> {
        Ok(serde_json::from_value(self.get("/images/json")?)?)
    }

    pub fn networks(&self) -> Result<Vec<Network>> {
        Ok(serde_json::from_value(self.get("/networks")?)?)
    }

    pub fn volumes(&self) -> Result<Vec<Volume>> {
        #[derive(Deserialize)]
        struct List {
            #[serde(rename = "Volumes", default)]
            volumes: Option<Vec<Volume>>,
        }
        let list: List = serde_json::from_value(self.get("/volumes")?)?;
        Ok(list.volumes.unwrap_or_default())
    }

    pub fn inspect_container(&self, id: &str) -> Result<Value> {
        self.get(&format!("/containers/{}/json", encode(id)))
    }

    pub fn inspect_image(&self, id: &str) -> Result<Value> {
        self.get(&format!("/images/{}/json", encode(id)))
    }

    pub fn inspect_network(&self, id: &str) -> Result<Value> {
        self.get(&format!("/networks/{}", encode(id)))
    }

    pub fn inspect_volume(&self, name: &str) -> Result<Value> {
        self.get(&format!("/volumes/{}", encode(name)))
    }

    pub fn image_history(&self, id: &str) -> Result<Vec<Layer>> {
        Ok(serde_json::from_value(
            self.get(&format!("/images/{}/history", encode(id)))?,
        )?)
    }

    /// `start`, `stop`, `restart`, `pause` or `unpause`.
    pub fn container_action(&self, id: &str, action: &str) -> Result<()> {
        self.request(
            "POST",
            &format!("/containers/{}/{action}", encode(id)),
            None,
        )?;
        Ok(())
    }

    pub fn remove_container(&self, id: &str) -> Result<()> {
        self.request(
            "DELETE",
            &format!("/containers/{}?force=true", encode(id)),
            None,
        )?;
        Ok(())
    }

    pub fn remove_image(&self, id: &str) -> Result<()> {
        self.request("DELETE", &format!("/images/{}", encode(id)), None)?;
        Ok(())
    }

    pub fn remove_network(&self, id: &str) -> Result<()> {
        self.request("DELETE", &format!("/networks/{}", encode(id)), None)?;
        Ok(())
    }

    pub fn remove_volume(&self, name: &str) -> Result<()> {
        self.request("DELETE", &format!("/volumes/{}", encode(name)), None)?;
        Ok(())
    }

    pub fn prune(&self, what: Prune) -> Result<()> {
        let path = match what {
            Prune::Containers => "/containers/prune",
            Prune::DanglingImages => "/images/prune",
            Prune::UnusedImages => "/images/prune?filters=%7B%22dangling%22%3A%5B%22false%22%5D%7D",
            Prune::Networks => "/networks/prune",
            Prune::Volumes => "/volumes/prune",
            Prune::BuildCache => "/build/prune",
        };
        self.request("POST", path, None)?;
        Ok(())
    }

    /// Creates the container and starts it, as IntelliJ's Create Container runs it; its id.
    pub fn create_container(&self, new: &NewContainer) -> Result<String> {
        let path = match new.name.trim() {
            "" => "/containers/create".to_owned(),
            name => format!("/containers/create?name={}", encode(name)),
        };
        let created: Value =
            serde_json::from_slice(&self.request("POST", &path, Some(&new.body()))?)?;
        let id = created["Id"]
            .as_str()
            .context("the daemon didn't tell the new container's id")?
            .to_owned();
        self.container_action(&id, "start")?;
        Ok(id)
    }

    /// The container's log, everything so far, then (`follow`) what it writes next.
    pub fn logs(&self, id: &str, follow: bool) -> Result<Stream> {
        self.stream(
            "GET",
            &format!(
                "/containers/{}/logs?stdout=1&stderr=1&follow={}",
                encode(id),
                follow as u8
            ),
            None,
        )
    }

    /// The container's main process's output from now on, after what it wrote so far.
    pub fn attach(&self, id: &str) -> Result<Hijacked> {
        self.hijack(
            &format!(
                "/containers/{}/attach?stream=1&logs=1&stdout=1&stderr=1",
                encode(id)
            ),
            &json!({}),
        )
    }

    /// A pull's progress, a JSON object a line (`status`, `progress`, `error`).
    pub fn pull(&self, image: &str) -> Result<Stream> {
        let (name, tag) = split_tag(image);
        self.stream(
            "POST",
            &format!(
                "/images/create?fromImage={}&tag={}",
                encode(name),
                encode(tag)
            ),
            None,
        )
    }

    /// The daemon's events, a JSON object a line, as they happen.
    pub fn events(&self) -> Result<Stream> {
        self.stream("GET", "/events", None)
    }

    pub fn exec_create(
        &self,
        container: &str,
        command: &[&str],
        tty: bool,
        user: &str,
    ) -> Result<String> {
        let body = json!({
            "AttachStdin": tty,
            "AttachStdout": true,
            "AttachStderr": true,
            "Tty": tty,
            "Env": ["TERM=xterm-256color"],
            "Cmd": command,
            "User": user,
        });
        let created: Value = serde_json::from_slice(&self.request(
            "POST",
            &format!("/containers/{}/exec", encode(container)),
            Some(&body),
        )?)?;
        Ok(created["Id"]
            .as_str()
            .context("the daemon didn't tell the exec's id")?
            .to_owned())
    }

    pub fn exec_start(&self, exec: &str, tty: bool) -> Result<Hijacked> {
        self.hijack(
            &format!("/exec/{}/start", encode(exec)),
            &json!({"Detach": false, "Tty": tty}),
        )
    }

    pub fn exec_resize(&self, exec: &str, rows: u16, columns: u16) -> Result<()> {
        self.request(
            "POST",
            &format!("/exec/{}/resize?h={rows}&w={columns}", encode(exec)),
            None,
        )?;
        Ok(())
    }

    pub fn exec_exit_code(&self, exec: &str) -> Result<Option<i64>> {
        Ok(self.get(&format!("/exec/{}/json", encode(exec)))?["ExitCode"].as_i64())
    }

    /// The entries of a folder of a running container, folders ending in `/`, by `ls` run in it.
    pub fn list_folder(&self, container: &str, path: &str) -> Result<Vec<String>> {
        let exec = self.exec_create(container, &["ls", "-1Ap", "--", path], false, "")?;
        let mut hijacked = self.exec_start(&exec, false)?;
        let (mut out, mut err) = (Vec::new(), Vec::new());
        while let Some((output, payload)) = read_frame(&mut hijacked.output)? {
            match output {
                Output::Stdout => out.extend(payload),
                Output::Stderr => err.extend(payload),
            }
        }
        if self.exec_exit_code(&exec)?.unwrap_or(0) != 0 {
            bail!("{}", String::from_utf8_lossy(&err).trim());
        }
        Ok(String::from_utf8_lossy(&out)
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect())
    }
}

fn request_bytes(method: &str, path: &str, body: Option<&Value>, upgrade: bool) -> Result<Vec<u8>> {
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: docker\r\nUser-Agent: Spork-IDE\r\nAccept: */*\r\n"
    );
    let body = body.map(serde_json::to_vec).transpose()?;
    if let Some(body) = &body {
        request.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    } else if method != "GET" {
        request.push_str("Content-Length: 0\r\n");
    }
    match upgrade {
        true => request.push_str("Connection: Upgrade\r\nUpgrade: tcp\r\n"),
        false => request.push_str("Connection: close\r\n"),
    }
    request.push_str("\r\n");
    let mut bytes = request.into_bytes();
    bytes.extend(body.unwrap_or_default());
    Ok(bytes)
}

/// A non-2xx status as the daemon's message (`{"message": …}`); 304, already done, is fine.
fn check_status(status: u16, body: &[u8]) -> Result<()> {
    if (200..300).contains(&status) || status == 304 {
        return Ok(());
    }
    let message = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| value["message"].as_str().map(str::to_owned))
        .unwrap_or_else(|| String::from_utf8_lossy(body).trim().to_owned());
    match message.is_empty() {
        true => bail!("the daemon answered {status}"),
        false => bail!("{message}"),
    }
}

fn parse_version(version: &str) -> Option<(u32, u32)> {
    let (major, minor) = version.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// `nginx:1.27` as its name and tag, `latest` when it has none (a registry's port isn't a tag).
pub fn split_tag(image: &str) -> (&str, &str) {
    let image = image.trim();
    match image.rsplit_once(':') {
        Some((name, tag)) if !tag.contains('/') && !name.is_empty() => (name, tag),
        _ => (image, "latest"),
    }
}

/// A path segment or query value percent-encoded.
fn encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn parses_endpoints() {
        assert_eq!(
            Endpoint::parse("unix:///var/run/docker.sock").ok(),
            Some(Endpoint::Unix("/var/run/docker.sock".into()))
        );
        assert_eq!(
            Endpoint::parse("/tmp/d.sock").ok(),
            Some(Endpoint::Unix("/tmp/d.sock".into()))
        );
        assert_eq!(
            Endpoint::parse("tcp://localhost:2375").ok(),
            Some(Endpoint::Tcp("localhost:2375".into()))
        );
        assert_eq!(
            Endpoint::parse("tcp://192.168.1.2").ok(),
            Some(Endpoint::Tcp("192.168.1.2:2375".into()))
        );
        assert!(Endpoint::parse("ssh://host").is_err());
        assert!(Endpoint::parse("unix://").is_err());
    }

    #[test]
    fn reads_bodies() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello trailing";
        let mut reader = &response[..];
        let head = read_head(&mut reader).unwrap();
        assert_eq!(head.status, 200);
        let mut body = String::new();
        Body::new(reader, head.body_kind())
            .read_to_string(&mut body)
            .unwrap();
        assert_eq!(body, "hello");

        let chunked =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n6;x=y\r\npedia \r\n0\r\n\r\n";
        let mut reader = &chunked[..];
        let head = read_head(&mut reader).unwrap();
        assert_eq!(head.body_kind(), BodyKind::Chunked);
        let mut body = String::new();
        Body::new(reader, head.body_kind())
            .read_to_string(&mut body)
            .unwrap();
        assert_eq!(body, "Wikipedia ");

        let no_content = b"HTTP/1.1 204 No Content\r\n\r\n";
        let mut reader = &no_content[..];
        let head = read_head(&mut reader).unwrap();
        assert_eq!(head.body_kind(), BodyKind::Length(0));
        assert!(read_head(&mut &b""[..]).is_err());
        assert!(read_head(&mut &b"garbage\r\n\r\n"[..]).is_err());
    }

    #[test]
    fn writes_requests() {
        let get = request_bytes("GET", "/v1.45/containers/json?all=1", None, false).unwrap();
        assert_eq!(
            String::from_utf8(get).unwrap(),
            "GET /v1.45/containers/json?all=1 HTTP/1.1\r\nHost: docker\r\nUser-Agent: Spork-IDE\r\nAccept: */*\r\nConnection: close\r\n\r\n"
        );
        let post =
            request_bytes("POST", "/exec/1/start", Some(&json!({"Tty": true})), true).unwrap();
        let post = String::from_utf8(post).unwrap();
        assert!(post.contains("Content-Length: 12\r\n"));
        assert!(post.contains("Connection: Upgrade\r\nUpgrade: tcp\r\n"));
        assert!(post.ends_with("\r\n\r\n{\"Tty\":true}"));
        let empty = String::from_utf8(request_bytes("POST", "/x", None, false).unwrap()).unwrap();
        assert!(empty.contains("Content-Length: 0\r\n"));
    }

    #[test]
    fn reads_frames() {
        let mut stream = Vec::new();
        stream.extend([1, 0, 0, 0, 0, 0, 0, 3]);
        stream.extend(b"out");
        stream.extend([2, 0, 0, 0, 0, 0, 0, 4]);
        stream.extend(b"err\n");
        let mut reader = &stream[..];
        assert_eq!(
            read_frame(&mut reader).unwrap(),
            Some((Output::Stdout, b"out".to_vec()))
        );
        assert_eq!(
            read_frame(&mut reader).unwrap(),
            Some((Output::Stderr, b"err\n".to_vec()))
        );
        assert_eq!(read_frame(&mut reader).unwrap(), None);
        assert!(read_frame(&mut &[1u8, 0, 0][..]).is_err());
        assert!(read_frame(&mut &[1u8, 0, 0, 0, 0, 0, 0, 9, b'x'][..]).is_err());
    }

    #[test]
    fn reports_the_daemons_message() {
        assert!(check_status(204, b"").is_ok());
        assert!(check_status(304, b"").is_ok());
        let error = check_status(404, br#"{"message":"No such container: x"}"#).unwrap_err();
        assert_eq!(error.to_string(), "No such container: x");
        let error = check_status(500, b"").unwrap_err();
        assert_eq!(error.to_string(), "the daemon answered 500");
    }

    #[test]
    fn splits_tags_and_encodes() {
        assert_eq!(split_tag("nginx"), ("nginx", "latest"));
        assert_eq!(split_tag("nginx:1.27"), ("nginx", "1.27"));
        assert_eq!(
            split_tag("localhost:5000/app"),
            ("localhost:5000/app", "latest")
        );
        assert_eq!(
            split_tag("localhost:5000/app:v2"),
            ("localhost:5000/app", "v2")
        );
        assert_eq!(encode("a b/c"), "a%20b%2Fc");
        assert_eq!(short_id("sha256:0123456789abcdef"), "0123456789ab");
        assert_eq!(short_id("abc"), "abc");
        assert_eq!(parse_version("1.47"), Some((1, 47)));
        assert_eq!(parse_version("x"), None);
    }

    #[test]
    fn decodes_the_lists() {
        let containers: Vec<Container> = serde_json::from_value(json!([{
            "Id": "c1", "Names": ["/web"], "Image": "nginx:latest", "ImageID": "sha256:i1",
            "State": "running", "Status": "Up 2 minutes", "Created": 1,
            "Labels": {"com.docker.compose.project": "shop"}
        }, {"Id": "c2", "Names": [], "Labels": null}]))
        .unwrap();
        assert_eq!(containers[0].name(), "web");
        assert!(containers[0].running());
        assert_eq!(containers[0].label(COMPOSE_PROJECT), Some("shop"));
        assert_eq!(containers[1].name(), "c2");
        assert_eq!(containers[1].label(COMPOSE_PROJECT), None);
        let images: Vec<Image> = serde_json::from_value(json!([
            {"Id": "sha256:aaaabbbbccccdddd", "RepoTags": ["<none>:<none>"]},
            {"Id": "i2", "RepoTags": null}
        ]))
        .unwrap();
        assert!(images[0].tags().is_empty());
        assert!(images[1].tags().is_empty());
        assert_eq!(images[0].short_id(), "aaaabbbbcccc");
    }

    #[test]
    fn builds_create_bodies() {
        let body = NewContainer {
            image: "nginx".into(),
            name: "web".into(),
            command: vec![],
            env: vec!["A=1".into()],
            ports: vec![("8080".into(), "80".into()), ("53".into(), "53/udp".into())],
        }
        .body();
        assert_eq!(body["Image"], "nginx");
        assert_eq!(body["Env"], json!(["A=1"]));
        assert_eq!(
            body["HostConfig"]["PortBindings"]["80/tcp"],
            json!([{"HostPort": "8080"}])
        );
        assert!(body["ExposedPorts"]["53/udp"].is_object());
        assert!(body.get("Cmd").is_none());
    }

    /// A daemon on a Unix socket answering each request with the next canned response, the
    /// requests it read sent back.
    fn daemon(
        responses: Vec<&'static [u8]>,
    ) -> (tempfile::TempDir, std::sync::mpsc::Receiver<String>) {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("d.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if let Some(value) = line.strip_prefix("Content-Length: ") {
                        length = value.trim().parse().unwrap();
                    }
                    request.push_str(&line);
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                request.push_str(&String::from_utf8(body).unwrap());
                sender.send(request).unwrap();
                stream.write_all(response).unwrap();
            }
        });
        (folder, receiver)
    }

    #[test]
    fn negotiates_and_lists() {
        let (folder, requests) = daemon(vec![
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK",
            b"HTTP/1.1 200 OK\r\nContent-Length: 21\r\n\r\n{\"ApiVersion\":\"1.41\"}",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n[]\r\n0\r\n\r\n",
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 30\r\n\r\n{\"message\":\"No such image: x\"}",
        ]);
        let url = format!("unix://{}", folder.path().join("d.sock").display());
        let engine = Engine::connect(&url).unwrap();
        assert_eq!(engine.version(), "v1.41");
        assert!(engine.containers().unwrap().is_empty());
        let error = engine.remove_image("x").unwrap_err();
        assert_eq!(error.to_string(), "No such image: x");
        let requests: Vec<String> = requests.try_iter().collect();
        assert!(requests[0].starts_with("GET /_ping HTTP/1.1\r\n"));
        assert!(requests[1].starts_with("GET /version HTTP/1.1\r\n"));
        assert!(requests[2].starts_with("GET /v1.41/containers/json?all=1 HTTP/1.1\r\n"));
        assert!(requests[3].starts_with("DELETE /v1.41/images/x HTTP/1.1\r\n"));
    }

    #[test]
    fn hijacks_and_streams() {
        let (folder, requests) = daemon(vec![
            b"HTTP/1.1 101 UPGRADED\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\nraw output",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n6\r\nline1\n\r\n6\r\nline2\n\r\n0\r\n\r\n",
        ]);
        let url = format!("unix://{}", folder.path().join("d.sock").display());
        let engine = Engine::with_version(&url, "v1.45").unwrap();
        let mut hijacked = engine.exec_start("e1", true).unwrap();
        let mut output = String::new();
        hijacked.output.read_to_string(&mut output).unwrap();
        assert_eq!(output, "raw output");
        let mut log = String::new();
        engine
            .logs("c1", true)
            .unwrap()
            .read_to_string(&mut log)
            .unwrap();
        assert_eq!(log, "line1\nline2\n");
        let requests: Vec<String> = requests.try_iter().collect();
        assert!(requests[0].starts_with("POST /v1.45/exec/e1/start HTTP/1.1\r\n"));
        assert!(requests[0].ends_with("{\"Detach\":false,\"Tty\":true}"));
        assert!(
            requests[1].starts_with(
                "GET /v1.45/containers/c1/logs?stdout=1&stderr=1&follow=1 HTTP/1.1\r\n"
            )
        );
    }
}
