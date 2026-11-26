//! IntelliJ's HTTP Client (its `restClient` plugin), first slice: `.http` and `.rest` files as
//! the "HTTP Request" language (highlighted on tree-sitter-http's grammar, completed with the
//! methods and headers), each request's run icon in the gutter, the editor's toolbar (Run All
//! Requests in File, the "Run with:" environment chooser), `http-client.env.json` and
//! `http-client.private.env.json` environments and `{{variables}}`, requests sent with the
//! workspace's HTTP client and their responses in the Services tool window's "HTTP Request" node,
//! each saved and linked under its request (`<> 2026-11-26T013102.200.txt`).

mod ui;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use futures::{AsyncReadExt as _, FutureExt as _};
use gpui::{App, SharedString};
use http_client::{AsyncBody, HttpClient, HttpRequestExt as _, RedirectPolicy};
use language::{LanguageConfig, LanguageMatcher, LanguageRegistry, LoadedLanguage};
use language::{LanguageQueries, QueryFile, QueryFileContents};

pub use ui::{e2e_state, open_in_http_client, submit_requests};

unsafe extern "C" {
    fn tree_sitter_http() -> *const ();
}

/// The HTTP Request grammar (`grammar/`).
const GRAMMAR: tree_sitter_language::LanguageFn =
    unsafe { tree_sitter_language::LanguageFn::from_raw(tree_sitter_http) };

/// The language's name, IntelliJ's `http.request.element.name`.
pub const LANGUAGE: &str = "HTTP Request";

/// IntelliJ's environment files: the public one and the private one, whose values win.
pub const PUBLIC_ENV: &str = "http-client.env.json";
pub const PRIVATE_ENV: &str = "http-client.private.env.json";

/// The methods IntelliJ's HTTP Client knows, as completion offers them.
pub(crate) const METHODS: [&str; 9] = [
    "GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "TRACE", "CONNECT",
];

/// Registers the HTTP Request language, and the HTTP Client's editor, gutter and Services parts.
// The gutter's markers wrap the ones set before (`spork_java`'s): call this after its `init`.
pub fn init(languages: Arc<LanguageRegistry>, cx: &mut App) {
    languages.register_native_grammars([("http", GRAMMAR)]);
    let config = LanguageConfig {
        name: LANGUAGE.into(),
        grammar: Some("http".into()),
        matcher: Arc::new(LanguageMatcher {
            path_suffixes: vec!["http".into(), "rest".into()],
            ..Default::default()
        }),
        // shortcut: no line comment, as the editor would continue `# ` on the line after a `###`
        // separator; give it `# ` once that continuation can be turned off per language
        ..Default::default()
    };
    languages.register_language(
        config.name.clone(),
        config.grammar.clone(),
        config.matcher.clone(),
        config.hidden,
        None,
        Arc::new(move || {
            let config = config.clone();
            async move {
                Ok(LoadedLanguage {
                    config,
                    queries: LanguageQueries::from_files([QueryFileContents::new(
                        QueryFile::Highlights,
                        std::borrow::Cow::Borrowed(include_str!("../queries/highlights.scm")),
                    )]),
                    context_provider: None,
                    toolchain_provider: None,
                    manifest_name: None,
                })
            }
            .boxed()
        }),
    );
    ui::init(cx);
}

/// A request of a `.http` file, as IntelliJ's HTTP Request format writes it: a `###` line
/// before it, its `# @name`, its request line (`GET https://…`, the method `GET` when left out,
/// a query continued on the indented lines after it), its headers, a blank line and its body,
/// then the responses saved for it (`<> file`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Request {
    /// The request line's row, where its run icon goes.
    pub row: u32,
    pub name: Option<String>,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// The responses saved for it, newest first: each `<>` line's row and file.
    pub responses: Vec<(u32, String)>,
    /// The row after which a new response's link goes: the request's last row of content.
    pub last_row: u32,
}

impl Request {
    /// How IntelliJ names its run: `# @name`, else `<file> | #<number>`.
    pub fn run_name(&self, file: &str, number: usize) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("{file} | #{number}"))
    }
}

/// The requests of a file and its variables (`@name = value`), in order.
pub fn parse(text: &str) -> (Vec<Request>, Vec<(String, String)>) {
    let mut requests = Vec::new();
    let mut variables = Vec::new();
    let mut current: Option<Request> = None;
    let mut name = None;
    // where the request is: its headers, then its body
    let mut in_body = false;
    let mut in_script = false;
    for (row, line) in text.lines().enumerate() {
        let row = row as u32;
        let trimmed = line.trim();
        if trimmed.starts_with("###") {
            requests.extend(current.take());
            name = None;
            in_body = false;
            in_script = false;
            continue;
        }
        if in_script {
            in_script = !trimmed.ends_with("%}");
            continue;
        }
        if let Some(request) = current.as_mut() {
            if let Some(file) = trimmed.strip_prefix("<>") {
                request.responses.push((row, file.trim().to_owned()));
                continue;
            }
            if trimmed.starts_with("> {%") {
                in_script = !trimmed.ends_with("%}");
                continue;
            }
            if trimmed.starts_with('>') {
                continue;
            }
            if !in_body {
                if trimmed.is_empty() {
                    in_body = true;
                } else if is_comment(trimmed) {
                } else if line.starts_with(char::is_whitespace) && request.headers.is_empty() {
                    // `    ?a=1` and `    &b=2` continue the URL
                    request.url.push_str(trimmed);
                    request.last_row = row;
                } else if let Some((header, value)) = trimmed.split_once(':') {
                    request
                        .headers
                        .push((header.trim().to_owned(), value.trim().to_owned()));
                    request.last_row = row;
                }
                continue;
            }
            if request.responses.is_empty() {
                if !request.body.is_empty() || !trimmed.is_empty() {
                    request.body.push_str(line);
                    request.body.push('\n');
                }
                if !trimmed.is_empty() {
                    request.last_row = row;
                }
            }
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        if is_comment(trimmed) {
            let tag = trimmed.trim_start_matches(['#', '/']).trim();
            if let Some(found) = tag.strip_prefix("@name") {
                name = Some(found.trim().trim_start_matches('=').trim().to_owned());
            }
            continue;
        }
        if let Some(declaration) = trimmed.strip_prefix('@')
            && let Some((variable, value)) = declaration.split_once('=')
        {
            variables.push((variable.trim().to_owned(), value.trim().to_owned()));
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let first = parts.next().unwrap_or_default();
        let (method, url) = match METHODS.contains(&first) {
            true => (
                first.to_owned(),
                parts.next().unwrap_or_default().to_owned(),
            ),
            false => ("GET".to_owned(), first.to_owned()),
        };
        current = Some(Request {
            row,
            name: name.take(),
            method,
            url,
            last_row: row,
            ..Default::default()
        });
    }
    requests.extend(current);
    for request in &mut requests {
        let body = request.body.trim_end().to_owned();
        request.body = body;
    }
    (requests, variables)
}

fn is_comment(line: &str) -> bool {
    line.starts_with('#') || line.starts_with("//")
}

/// The environments of the `.http` file at `file`: those of the nearest folder up to `root` with
/// an environment file, each name's variables, the private file's over the public one's.
// shortcut: the nearest folder's files, where IntelliJ merges every environment file of the
// project
pub fn environments(file: &Path, root: &Path) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut environments = BTreeMap::new();
    let folder = file
        .ancestors()
        .skip(1)
        .take_while(|folder| folder.starts_with(root))
        .find(|folder| folder.join(PUBLIC_ENV).is_file() || folder.join(PRIVATE_ENV).is_file());
    let Some(folder) = folder else {
        return environments;
    };
    for name in [PUBLIC_ENV, PRIVATE_ENV] {
        let Ok(text) = std::fs::read_to_string(folder.join(name)) else {
            continue;
        };
        let Ok(serde_json::Value::Object(found)) = serde_json::from_str(&text) else {
            log::warn!("{name} isn't a JSON object of environments");
            continue;
        };
        for (environment, values) in found {
            let Some(values) = values.as_object() else {
                continue;
            };
            let variables: &mut BTreeMap<String, String> =
                environments.entry(environment).or_default();
            for (key, value) in values {
                let value = match value {
                    serde_json::Value::String(text) => text.clone(),
                    other => other.to_string(),
                };
                variables.insert(key.clone(), value);
            }
        }
    }
    environments
}

/// `text` with its `{{name}}` variables replaced: the environment's first, then the file's; an
/// unknown one stays as written.
pub fn substitute(text: &str, variables: &BTreeMap<String, String>) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        result.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            result.push_str(&rest[start..]);
            return result;
        };
        let name = after[..end].trim();
        match variables.get(name) {
            Some(value) => result.push_str(value),
            None => result.push_str(&rest[start..start + 2 + end + 2]),
        }
        rest = &after[end + 2..];
    }
    result.push_str(rest);
    result
}

/// What a request got: the status line, headers and body as received, and how long it took.
#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub elapsed: Duration,
}

impl Response {
    /// The response file's extension, by its content type, as IntelliJ names it.
    pub fn extension(&self) -> &'static str {
        let content_type = self
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
            .map(|(_, value)| value.to_lowercase())
            .unwrap_or_default();
        if content_type.contains("json") {
            "json"
        } else if content_type.contains("html") {
            "html"
        } else if content_type.contains("xml") {
            "xml"
        } else {
            "txt"
        }
    }
}

/// Sends `request`, its variables replaced, following redirects as IntelliJ's client does.
pub async fn send(
    http: Arc<dyn HttpClient>,
    request: &Request,
    variables: &BTreeMap<String, String>,
) -> Result<Response> {
    let url = substitute(&request.url, variables);
    let mut builder = http_client::Request::builder()
        .method(request.method.as_str())
        .uri(url.as_str())
        .follow_redirects(RedirectPolicy::FollowAll);
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), substitute(value, variables));
    }
    let body = substitute(&request.body, variables);
    let started = Instant::now();
    let mut response = http
        .send(builder.body(AsyncBody::from(body))?)
        .await
        .with_context(|| format!("{} {url}", request.method))?;
    let mut body = Vec::new();
    response.body_mut().read_to_end(&mut body).await?;
    let status = response.status();
    Ok(Response {
        status: status.as_u16(),
        reason: status.canonical_reason().unwrap_or_default().to_owned(),
        headers: response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_owned(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect(),
        body,
        elapsed: started.elapsed(),
    })
}

/// The console text IntelliJ writes for a run: the request, the response's status line, headers
/// and body, where it was saved, and its summary line.
pub fn console_text(
    request_line: &str,
    response: &Result<Response, SharedString>,
    saved: Option<&str>,
) -> String {
    let mut text = format!("{request_line}\n\n");
    match response {
        Ok(response) => {
            text.push_str(&format!(
                "HTTP/1.1 {} {}\n",
                response.status, response.reason
            ));
            for (name, value) in &response.headers {
                text.push_str(&format!("{name}: {value}\n"));
            }
            text.push('\n');
            match response.body.is_empty() {
                true => text.push_str("<Response body is empty>"),
                false => text.push_str(&String::from_utf8_lossy(&response.body)),
            }
            text.push_str("\n\n");
            if let Some(saved) = saved {
                text.push_str(&format!("Response file saved.\n> {saved}\n\n"));
            }
            let millis = response.elapsed.as_millis();
            let length = response.body.len();
            text.push_str(&format!(
                "Response code: {}; Time: {millis}ms ({millis} ms); Content length: {length} bytes ({})",
                response.status,
                byte_size(length)
            ));
        }
        Err(error) => text.push_str(error),
    }
    text
}

/// A size as IntelliJ's `StringUtil.formatFileSize` writes it: `13 B`, `1.21 kB`.
fn byte_size(bytes: usize) -> String {
    const UNITS: [&str; 4] = ["B", "kB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1000. && unit < UNITS.len() - 1 {
        size /= 1000.;
        unit += 1;
    }
    match unit {
        0 => format!("{bytes} B"),
        _ => format!("{:.2} {}", size, UNITS[unit]),
    }
}

/// The name IntelliJ saves a response under: its time, status and type
/// (`2026-11-26T013102.200.json`), made unique in `folder`.
pub fn response_file_name(folder: &Path, response: &Response) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y-%m-%dT%H%M%S");
    let name = format!("{stamp}.{}.{}", response.status, response.extension());
    let path = folder.join(&name);
    if !path.exists() {
        return path;
    }
    (1..)
        .map(|ix| {
            folder.join(format!(
                "{stamp}-{ix}.{}.{}",
                response.status,
                response.extension()
            ))
        })
        .find(|path| !path.exists())
        .unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{GRAMMAR, environments, parse, substitute};

    const FILE: &str = "@host = localhost:8080\n\n### Hello\n# @name hello\nGET http://{{host}}/hello\n    ?name=world\nAccept: application/json\n\n<> 2026-11-26T013102.200.json\n\n###\nPOST http://localhost/items\nContent-Type: application/json\n\n{\"a\": 1}\n\n> {%\nclient.log(1);\n%}\n\n###\nhttp://localhost/plain\n";

    #[test]
    fn requests_as_intellij_writes_them() {
        let (requests, variables) = parse(FILE);
        assert_eq!(
            variables,
            [("host".to_owned(), "localhost:8080".to_owned())]
        );
        assert_eq!(requests.len(), 3);
        let hello = &requests[0];
        assert_eq!(
            (hello.row, hello.name.as_deref(), hello.method.as_str()),
            (4, Some("hello"), "GET")
        );
        assert_eq!(hello.url, "http://{{host}}/hello?name=world");
        assert_eq!(
            hello.headers,
            [("Accept".into(), "application/json".into())]
        );
        assert_eq!(hello.responses, [(8, "2026-11-26T013102.200.json".into())]);
        assert_eq!(hello.last_row, 6);
        let post = &requests[1];
        assert_eq!(
            (post.method.as_str(), post.body.as_str()),
            ("POST", "{\"a\": 1}")
        );
        assert_eq!(post.last_row, 14);
        assert_eq!(requests[2].method, "GET");
        assert_eq!(requests[2].run_name("api", 3), "api | #3");
    }

    #[test]
    fn variables_from_the_environment() {
        let variables = BTreeMap::from([("host".to_owned(), "localhost:9090".to_owned())]);
        assert_eq!(
            substitute("http://{{host}}/a/{{ id }}", &variables),
            "http://localhost:9090/a/{{ id }}"
        );
    }

    #[test]
    fn the_private_environment_wins() -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        let folder = root.path().join("api");
        std::fs::create_dir_all(&folder)?;
        std::fs::write(
            root.path().join(super::PUBLIC_ENV),
            r#"{"dev": {"host": "localhost:8080", "token": "public"}, "prod": {"host": "example.org"}}"#,
        )?;
        std::fs::write(
            root.path().join(super::PRIVATE_ENV),
            r#"{"dev": {"token": "secret"}}"#,
        )?;
        let found = environments(&folder.join("requests.http"), root.path());
        assert_eq!(found.keys().collect::<Vec<_>>(), ["dev", "prod"]);
        assert_eq!(found["dev"]["token"], "secret");
        assert_eq!(found["dev"]["host"], "localhost:8080");
        Ok(())
    }

    #[test]
    fn the_highlights_query_fits_the_grammar() {
        let language =
            language::Language::new(language::LanguageConfig::default(), Some(GRAMMAR.into()))
                .with_highlights_query(include_str!("../queries/highlights.scm"));
        assert!(language.is_ok());
    }
}
