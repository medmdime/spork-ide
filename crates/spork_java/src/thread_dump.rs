//! Java's threads beyond what java-debug tells over the protocol: their groups and states, and
//! IntelliJ's thread dump (`ThreadDumpAction`) of them. The protocol lists the threads and their
//! frames; a thread's group, state, priority, id and daemon flag come from evaluating Java in the
//! stopped frame (`Thread.getAllStackTraces()`, which reads them without touching the other
//! threads, all suspended). The locks a thread holds or waits for aren't told: JDI's monitor
//! information isn't reachable through java-debug.

use anyhow::{Context as _, Result};
use gpui::{App, AppContext as _, Entity, Task};
use project::debugger::session::Session;
use serde_json::{Value, json};
use spork_run::{DumpThread, ThreadInfo};

use crate::debugger::{ProxyHandle, proxy_of, source_package};

/// The threads as Java sees them, one `name|state|group|daemon|priority|id` per `;`.
// shortcut: a thread named with `|` or `;` reads wrong; a structured answer needs java-debug's
const THREADS: &str = "StringBuilder b = new StringBuilder(); \
    for (Thread t : Thread.getAllStackTraces().keySet()) { ThreadGroup g = t.getThreadGroup(); \
    b.append(t.getName()).append('|').append(t.getState()).append('|') \
    .append(g == null ? \"\" : g.getName()).append('|').append(t.isDaemon()).append('|') \
    .append(t.getPriority()).append('|').append(t.getId()).append(';'); } return b.toString();";

/// A thread as Java sees it.
#[derive(Clone, Debug, PartialEq)]
struct JavaThread {
    name: String,
    state: String,
    group: String,
    daemon: bool,
    priority: String,
    id: u64,
}

fn parse_threads(result: &str) -> Vec<JavaThread> {
    let text = result
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .unwrap_or(result);
    text.split(';')
        .filter_map(|thread| {
            let mut parts = thread.split('|');
            Some(JavaThread {
                name: parts.next()?.to_owned(),
                state: parts.next()?.to_owned(),
                group: parts.next()?.to_owned(),
                daemon: parts.next()? == "true",
                priority: parts.next()?.to_owned(),
                id: parts.next()?.parse().ok()?,
            })
        })
        .collect()
}

async fn java_threads(proxy: &ProxyHandle, frame: u64) -> Result<Vec<JavaThread>> {
    let answer = proxy
        .request(
            "evaluate",
            json!({"expression": THREADS, "frameId": frame, "context": "repl"}),
        )
        .await?;
    let result = answer["result"].as_str().context("no threads")?;
    Ok(parse_threads(result))
}

/// The groups and states of a stopped Java session's threads, for Show thread groups.
pub(crate) fn thread_infos(
    session: &Entity<Session>,
    frame: u64,
    cx: &mut App,
) -> Task<Result<Vec<ThreadInfo>>> {
    let Some(proxy) = proxy_of(session.read(cx), cx) else {
        return Task::ready(Ok(Vec::new()));
    };
    cx.background_spawn(async move {
        Ok(java_threads(&proxy, frame)
            .await?
            .into_iter()
            .map(|thread| ThreadInfo {
                name: thread.name,
                group: thread.group,
                state: thread.state,
            })
            .collect())
    })
}

/// IntelliJ's state of a thread in a dump (`javaThreadStateToState`).
fn dump_state(state: &str) -> &'static str {
    match state {
        "BLOCKED" => "waiting for monitor entry",
        "NEW" => "not started",
        "RUNNABLE" => "runnable",
        "TIMED_WAITING" => "sleeping",
        "WAITING" => "waiting",
        "TERMINATED" => "zombie",
        _ => "unknown",
    }
}

/// A thread's text in IntelliJ's dump: its header, its Java state, then its frames.
fn thread_text(name: &str, thread: Option<&JavaThread>, frames: &[String]) -> String {
    let mut text = format!("\"{name}\"");
    if let Some(thread) = thread {
        if thread.daemon {
            text.push_str(" daemon");
        }
        text.push_str(&format!(
            " prio={} tid=0x{:x} nid=NA {}\n  java.lang.Thread.State: {}",
            thread.priority,
            thread.id,
            dump_state(&thread.state),
            thread.state
        ));
    }
    text.push('\n');
    text.push_str(
        &frames
            .iter()
            .map(|frame| format!("\t{frame}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    text
}

/// A java-debug frame as a dump writes it: `at com.example.Debuggee.measure(Debuggee.java:44)`.
async fn frame_line(frame: &Value) -> String {
    let name = frame["name"].as_str().unwrap_or_default();
    let name = name.trim_end_matches("[native method]");
    let method = name.split('(').next().unwrap_or(name);
    let package = match frame["source"]["path"].as_str() {
        Some(path) => source_package(path).await,
        None => None,
    };
    let method = match package {
        Some(package) if !package.is_empty() => format!("{package}.{method}"),
        _ => method.to_owned(),
    };
    let source = frame["source"]["name"].as_str().unwrap_or("Unknown Source");
    let line = frame["line"].as_i64().unwrap_or(-1);
    format!("at {method}({source}:{line})")
}

/// Get Thread Dump of a stopped Java session: each thread with its state and frames.
pub(crate) fn thread_dump(
    session: &Entity<Session>,
    frame: u64,
    cx: &mut App,
) -> Task<Result<Vec<DumpThread>>> {
    let Some(proxy) = proxy_of(session.read(cx), cx) else {
        return Task::ready(Ok(Vec::new()));
    };
    cx.background_spawn(async move {
        let java = java_threads(&proxy, frame).await?;
        let threads = proxy.request("threads", json!({})).await?;
        let mut dump = Vec::new();
        for thread in threads["threads"].as_array().into_iter().flatten() {
            let id = thread["id"].as_i64().unwrap_or_default();
            let raw = thread["name"].as_str().unwrap_or_default();
            let name = raw
                .strip_prefix("Thread [")
                .and_then(|name| name.strip_suffix(']'))
                .unwrap_or(raw);
            let trace = proxy
                .request(
                    "stackTrace",
                    json!({"threadId": id, "startFrame": 0, "levels": 2000}),
                )
                .await?;
            let mut frames = Vec::new();
            for frame in trace["stackFrames"].as_array().into_iter().flatten() {
                frames.push(frame_line(frame).await);
            }
            let info = java.iter().find(|java| java.name == name);
            // IntelliJ's thread name in a dump: the name and the JDI id
            let named = format!("{name}@{id}");
            dump.push(DumpThread {
                name: named.clone(),
                state: info
                    .map(|info| dump_state(&info.state).to_owned())
                    .unwrap_or_default(),
                text: thread_text(&named, info, &frames),
            });
        }
        Ok(dump)
    })
}

#[cfg(test)]
mod tests {
    use super::{JavaThread, parse_threads, thread_text};

    #[test]
    fn writes_a_thread_as_intellijs_dump() {
        let threads =
            parse_threads("\"main|WAITING|main|false|5|1;worker|RUNNABLE|main|false|5|31;\"");
        assert_eq!(threads.len(), 2);
        let main = JavaThread {
            name: "main".into(),
            state: "WAITING".into(),
            group: "main".into(),
            daemon: false,
            priority: "5".into(),
            id: 1,
        };
        assert_eq!(threads[0], main);
        assert_eq!(
            thread_text(
                "main@1",
                Some(&main),
                &["at java.lang.Object.wait(Object.java:-1)".into()]
            ),
            "\"main@1\" prio=5 tid=0x1 nid=NA waiting\n  java.lang.Thread.State: WAITING\n\tat java.lang.Object.wait(Object.java:-1)"
        );
    }
}
