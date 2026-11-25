//! IntelliJ's Local History (ROADMAP, phase 6): every file's saved and external changes, kept in
//! the IDE's data folder (`LocalHistory/`) as the content before and after each change, with the
//! labels put on a project. As IntelliJ's, it keeps 5 days (`localHistory.daysToKeep`, Settings ›
//! Advanced Settings › IDE's "Duration of storing changes in Local History", kept in the config
//! folder's `local_history.json`) and leaves out what is too large to be worth keeping. Show History (File ▸ Local History, the editor's and
//! the Project tool window's context menus) lists a file's or folder's changes beside a diff with
//! the current content, to revert one; Put Label… names the project's state.

mod history_dialog;

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use collections::HashMap;
use gpui::{App, BorrowAppContext as _, Entity, Global};
use language::{Buffer, BufferEvent};
use project::Project;
use project::buffer_store::BufferStoreEvent;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use util::ResultExt as _;
use workspace::Workspace;

pub use history_dialog::{put_label, show_history};

pub use zed_actions::local_history::{PutLabel, ShowHistory};

/// IntelliJ's time window: changes older than this many days go (the `localHistory.daysToKeep`
/// advanced setting, 5 by default).
const DEFAULT_DAYS_TO_KEEP: i64 = 5;
/// The largest content kept: IntelliJ's file system keeps contents up to 1 MB
/// (`PersistentFSConstants.MAX_FILE_LENGTH_TO_CACHE`), and so does this history.
const MAX_CONTENT: usize = 1024 * 1024;
const DAY: i64 = 24 * 60 * 60 * 1000;

/// What happened to a file, or to the project for a label. Contents are named by their hash.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Change {
    /// The file's content when first seen, the base of its next change; not listed.
    Seen { path: PathBuf, content: String },
    /// Saved from the IDE.
    Saved {
        path: PathBuf,
        before: String,
        after: String,
    },
    /// Changed on the disk by something else.
    External {
        path: PathBuf,
        before: String,
        after: String,
    },
    /// Put Label… on the project in `root`.
    Label { root: PathBuf, name: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Record {
    /// Milliseconds since the Unix epoch.
    time: i64,
    #[serde(flatten)]
    change: Change,
}

/// A row of Show History: when, what IntelliJ calls it, the file it shows and its content then
/// (the content's hash; `None` when it is the current one).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Revision {
    pub time: i64,
    pub title: Option<String>,
    pub path: PathBuf,
    pub content: Option<String>,
}

/// The history on the disk: `changes.jsonl`, a record a line, oldest first, and `contents/`, each
/// content under its hash.
struct Store {
    dir: PathBuf,
    records: Vec<Record>,
    /// Each file's content as last recorded.
    known: HashMap<PathBuf, String>,
}

impl Store {
    /// Reads the history in `dir`, leaving out what is older than `days` before `now`.
    fn open(dir: PathBuf, now: i64, days: i64) -> Self {
        let log = dir.join("changes.jsonl");
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        let records: Vec<Record> = text
            .lines()
            .filter_map(|line| serde_json::from_str(line).log_err())
            .collect();
        let mut store = Self {
            dir,
            records,
            known: HashMap::default(),
        };
        store.keep_since(now - days * DAY);
        for record in &store.records {
            match &record.change {
                Change::Seen { path, content } => {
                    store.known.insert(path.clone(), content.clone());
                }
                Change::Saved { path, after, .. } | Change::External { path, after, .. } => {
                    store.known.insert(path.clone(), after.clone());
                }
                Change::Label { .. } => {}
            }
        }
        store
    }

    /// Forgets the changes made before `since`, on the disk too; each file's last content stays,
    /// the base of its next change.
    fn keep_since(&mut self, since: i64) {
        let before = self.records.len();
        self.records.retain(|record| record.time >= since);
        if self.records.len() != before {
            self.rewrite().log_err();
            self.collect_garbage().log_err();
        }
    }

    fn contents(&self) -> PathBuf {
        self.dir.join("contents")
    }

    /// Writes the records again: after the time window dropped some.
    fn rewrite(&self) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let mut text = String::new();
        for record in &self.records {
            text.push_str(&serde_json::to_string(record)?);
            text.push('\n');
        }
        std::fs::write(self.dir.join("changes.jsonl"), text)?;
        Ok(())
    }

    /// Removes the contents no record names any more.
    fn collect_garbage(&self) -> Result<()> {
        // each file's last content too: it is the base of the file's next change
        let mut used: collections::HashSet<String> = self.known.values().cloned().collect();
        for record in &self.records {
            match &record.change {
                Change::Seen { content, .. } => {
                    used.insert(content.clone());
                }
                Change::Saved { before, after, .. } | Change::External { before, after, .. } => {
                    used.insert(before.clone());
                    used.insert(after.clone());
                }
                Change::Label { .. } => {}
            }
        }
        let Ok(entries) = std::fs::read_dir(self.contents()) else {
            return Ok(());
        };
        for entry in entries {
            let entry = entry?;
            if !used.contains(entry.file_name().to_string_lossy().as_ref()) {
                std::fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }

    fn append(&mut self, record: Record) -> Result<()> {
        use std::io::Write as _;
        std::fs::create_dir_all(&self.dir)?;
        let mut log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("changes.jsonl"))
            .context("opening the local history")?;
        writeln!(log, "{}", serde_json::to_string(&record)?)?;
        self.records.push(record);
        Ok(())
    }

    /// Keeps `text` under its hash, and answers the hash.
    fn put(&self, text: &str) -> Result<String> {
        let hash = format!("{:x}", Sha256::digest(text.as_bytes()));
        let path = self.contents().join(&hash);
        if !path.exists() {
            std::fs::create_dir_all(self.contents())?;
            std::fs::write(&path, text)?;
        }
        Ok(hash)
    }

    fn content(&self, hash: &str) -> Option<String> {
        std::fs::read_to_string(self.contents().join(hash)).ok()
    }

    /// A file was opened with `text`: its base, or an external change if it differs from what
    /// was last recorded.
    fn seen(&mut self, path: &Path, text: &str, now: i64) -> Result<()> {
        if text.len() > MAX_CONTENT {
            return Ok(());
        }
        let hash = self.put(text)?;
        let change = match self.known.get(path) {
            Some(known) if *known == hash => return Ok(()),
            Some(known) => Change::External {
                path: path.to_path_buf(),
                before: known.clone(),
                after: hash.clone(),
            },
            None => Change::Seen {
                path: path.to_path_buf(),
                content: hash.clone(),
            },
        };
        self.append(Record { time: now, change })?;
        self.known.insert(path.to_path_buf(), hash);
        Ok(())
    }

    /// A file's content became `text`, saved from the IDE or changed on the disk (`external`).
    fn changed(&mut self, path: &Path, text: &str, external: bool, now: i64) -> Result<()> {
        if text.len() > MAX_CONTENT {
            return Ok(());
        }
        let Some(before) = self.known.get(path).cloned() else {
            return self.seen(path, text, now);
        };
        let after = self.put(text)?;
        if before == after {
            return Ok(());
        }
        let path = path.to_path_buf();
        let change = match external {
            true => Change::External {
                path: path.clone(),
                before,
                after: after.clone(),
            },
            false => Change::Saved {
                path: path.clone(),
                before,
                after: after.clone(),
            },
        };
        self.append(Record { time: now, change })?;
        self.known.insert(path, after);
        Ok(())
    }

    fn label(&mut self, root: &Path, name: &str, now: i64) -> Result<()> {
        self.append(Record {
            time: now,
            change: Change::Label {
                root: root.to_path_buf(),
                name: name.to_owned(),
            },
        })
    }

    /// `path`'s content at `time`: after the last change by then, else before the first one
    /// after; `None` when nothing was recorded (the current content).
    fn content_at(&self, path: &Path, time: i64) -> Option<String> {
        let mut last = None;
        for record in &self.records {
            let (before, after) = match &record.change {
                Change::Seen { path: p, content } if p == path => (content, content),
                Change::Saved {
                    path: p,
                    before,
                    after,
                }
                | Change::External {
                    path: p,
                    before,
                    after,
                } if p == path => (before, after),
                _ => continue,
            };
            if record.time > time {
                return Some(last.unwrap_or(before).clone());
            }
            last = Some(after);
        }
        last.cloned()
    }

    /// Show History's rows for a file, or for every file under a folder, newest first: each
    /// change with the content before it, and for a file the labels with its content then.
    fn revisions(&self, path: &Path) -> Vec<Revision> {
        let mut revisions = Vec::new();
        for record in self.records.iter().rev() {
            match &record.change {
                Change::Saved {
                    path: changed,
                    before,
                    ..
                }
                | Change::External {
                    path: changed,
                    before,
                    ..
                } if changed.starts_with(path) => revisions.push(Revision {
                    time: record.time,
                    title: matches!(record.change, Change::External { .. })
                        .then(|| "External change".to_owned()),
                    path: changed.clone(),
                    content: Some(before.clone()),
                }),
                Change::Label { root, name } if path.starts_with(root) && path != root => revisions
                    .push(Revision {
                        time: record.time,
                        title: Some(name.clone()),
                        path: path.to_path_buf(),
                        content: self.content_at(path, record.time),
                    }),
                _ => {}
            }
        }
        revisions
    }
}

struct LocalHistory(Store);

impl Global for LocalHistory {}

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// How many days of changes the history keeps.
struct DaysToKeep(i64);

impl Global for DaysToKeep {}

#[derive(Serialize, Deserialize)]
struct Retention {
    days_to_keep: i64,
}

fn retention_file() -> PathBuf {
    paths::config_dir().join("local_history.json")
}

/// The days kept as `file` says, IntelliJ's 5 without it.
fn read_days(file: &Path) -> i64 {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str::<Retention>(&text).log_err())
        .map_or(DEFAULT_DAYS_TO_KEEP, |retention| retention.days_to_keep)
        .max(1)
}

fn write_days(file: &Path, days: i64) -> Result<()> {
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let retention = Retention { days_to_keep: days };
    std::fs::write(file, serde_json::to_string_pretty(&retention)?)
        .with_context(|| format!("writing {}", file.display()))
}

/// Advanced Settings › IDE › "Duration of storing changes in Local History", in days.
pub fn days_to_keep(cx: &App) -> i64 {
    cx.try_global::<DaysToKeep>()
        .map_or(DEFAULT_DAYS_TO_KEEP, |days| days.0)
}

/// Keeps `days` of changes from now on, forgetting the older ones at once.
pub fn set_days_to_keep(days: i64, cx: &mut App) -> Result<()> {
    let days = days.max(1);
    write_days(&retention_file(), days)?;
    cx.set_global(DaysToKeep(days));
    if cx.has_global::<LocalHistory>() {
        cx.update_global::<LocalHistory, _>(|history, _| history.0.keep_since(now() - days * DAY));
    }
    Ok(())
}

/// Opens the history in the IDE's data folder and records the changes of every local file the
/// IDE opens.
pub fn init(cx: &mut App) {
    let days = read_days(&retention_file());
    cx.set_global(DaysToKeep(days));
    let store = Store::open(paths::data_dir().join("LocalHistory"), now(), days);
    cx.set_global(LocalHistory(store));
    // every buffer a project opens, through its buffer store: buffers loaded from the disk are
    // built in place, so `observe_new` doesn't see them
    cx.observe_new(|project: &mut Project, _, cx| {
        let buffer_store = project.buffer_store().clone();
        cx.subscribe(&buffer_store, |_, _, event, cx| {
            if let BufferStoreEvent::BufferAdded(buffer) = event {
                track(buffer, cx);
            }
        })
        .detach();
    })
    .detach();
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action(|workspace, _: &ShowHistory, window, cx| {
            if let Some(path) = open_file(workspace, cx) {
                show_history(path, workspace, window, cx);
            }
        });
        workspace.register_action(|workspace, _: &PutLabel, window, cx| {
            put_label(workspace, window, cx)
        });
    })
    .detach();
}

/// Records `buffer`'s file as it is now, then each time it is saved or reloaded from the disk.
fn track(buffer: &Entity<Buffer>, cx: &mut App) {
    let path = local_path(buffer.read(cx), cx);
    if let Some(path) = path {
        let text = buffer.read(cx).text();
        cx.update_global::<LocalHistory, _>(|history, _| history.0.seen(&path, &text, now()))
            .log_err();
    }
    cx.subscribe(buffer, |buffer, event: &BufferEvent, cx| {
        let external = match event {
            BufferEvent::Saved => false,
            BufferEvent::Reloaded => true,
            _ => return,
        };
        let buffer = buffer.read(cx);
        if let Some(path) = local_path(buffer, cx) {
            let text = buffer.text();
            cx.update_global::<LocalHistory, _>(|history, _| {
                history.0.changed(&path, &text, external, now())
            })
            .log_err();
        }
    })
    .detach();
}

/// The file the editor shows, on the disk.
fn open_file(workspace: &Workspace, cx: &App) -> Option<PathBuf> {
    let project_path = workspace.active_item(cx)?.project_path(cx)?;
    workspace
        .project()
        .read(cx)
        .absolute_path(&project_path, cx)
}

fn local_path(buffer: &Buffer, cx: &App) -> Option<PathBuf> {
    Some(buffer.file()?.as_local()?.abs_path(cx))
}

fn with_store<R>(cx: &App, read: impl FnOnce(&Store) -> R) -> Option<R> {
    cx.try_global::<LocalHistory>()
        .map(|history| read(&history.0))
}

fn put_label_now(root: &Path, name: &str, cx: &mut App) {
    if cx.has_global::<LocalHistory>() {
        cx.update_global::<LocalHistory, _>(|history, _| history.0.label(root, name, now()))
            .log_err();
    }
}

/// What Show History needs to label its diff: IntelliJ's date and time, `21/11/2026, 19:52`.
fn shown_time(time: i64) -> String {
    use chrono::TimeZone as _;
    chrono::Local
        .timestamp_millis_opt(time)
        .single()
        .map(|time| time.format("%d/%m/%Y, %H:%M").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{DAY, DEFAULT_DAYS_TO_KEEP, Revision, Store, read_days, write_days};

    const DAYS: i64 = DEFAULT_DAYS_TO_KEEP;

    #[test]
    fn records_changes_and_answers_the_content_before_each() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let file = dir.path().join("project/README.md");
        let mut store = Store::open(dir.path().join("history"), 0, DAYS);
        store.seen(&file, "one", 10).expect("seen");
        store.changed(&file, "two", false, 20).expect("saved");
        store
            .changed(&file, "two", false, 25)
            .expect("saved, unchanged");
        store
            .label(&dir.path().join("project"), "before three", 30)
            .expect("label");
        store.changed(&file, "three", true, 40).expect("external");

        let revisions = store.revisions(&file);
        let shown: Vec<_> = revisions
            .iter()
            .map(|revision| {
                (
                    revision.time,
                    revision.title.clone(),
                    revision
                        .content
                        .as_deref()
                        .and_then(|hash| store.content(hash)),
                )
            })
            .collect();
        assert_eq!(
            shown,
            vec![
                (40, Some("External change".into()), Some("two".into())),
                (30, Some("before three".into()), Some("two".into())),
                (20, None, Some("one".into())),
            ]
        );
        // the folder's history lists its files' changes, not the labels
        let folder = store.revisions(&dir.path().join("project"));
        assert_eq!(folder.len(), 2);
        assert!(folder.iter().all(|Revision { path, .. }| path == &file));

        // read again, later: the same history; past the time window, nothing
        let reopened = Store::open(dir.path().join("history"), 41, DAYS);
        assert_eq!(reopened.revisions(&file), revisions);
        let purged = Store::open(dir.path().join("history"), 41 + 5 * DAY, DAYS);
        assert!(purged.revisions(&file).is_empty());
        assert!(
            std::fs::read_dir(dir.path().join("history/contents"))
                .expect("the contents folder")
                .next()
                .is_none()
        );
    }

    #[test]
    fn leaves_out_what_is_too_large() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let file = dir.path().join("big.txt");
        let mut store = Store::open(dir.path().join("history"), 0, DAYS);
        let big = "x".repeat(super::MAX_CONTENT + 1);
        store.seen(&file, "small", 1).expect("seen");
        store.changed(&file, &big, false, 2).expect("too large");
        assert!(store.revisions(&file).is_empty());
    }

    #[test]
    fn keeps_the_days_set_and_five_without_a_setting() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let file = dir.path().join("config/local_history.json");
        assert_eq!(read_days(&file), 5);
        write_days(&file, 12).expect("written");
        assert_eq!(read_days(&file), 12);

        // a shorter window forgets the changes before it
        let mut store = Store::open(dir.path().join("history"), 0, 12);
        let path = dir.path().join("a.txt");
        store.seen(&path, "one", 0).expect("seen");
        store.changed(&path, "two", false, 3 * DAY).expect("saved");
        store.keep_since(2 * DAY);
        assert_eq!(store.records.len(), 1);
    }
}
