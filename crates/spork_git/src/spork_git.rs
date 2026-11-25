//! The one place Spork runs git (docs/00-overview.md §3.4) plus the ref reader behind the sidebar (docs/specs/M03).
#![allow(
    clippy::disallowed_methods,
    reason = "the git runner blocks on purpose: its callers run it on background threads"
)]

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub mod bisect;
pub mod branch;
pub mod config;
pub mod credential;
pub mod file;
pub mod gitflow;
pub mod ignore;
pub mod lean;
pub mod lfs;
pub mod log;
pub mod remote;
pub mod revision;
pub mod shelf;
pub mod stash;
pub mod stats;
pub mod submodule;

#[cfg(test)]
mod testutil;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitError {
    /// git couldn't be started at all (not installed, bad path).
    GitMissing(String),
    NotARepo,
    IndexLocked,
    AuthFailed,
    NonFastForward,
    Other {
        code: Option<i32>,
        stderr: String,
    },
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitError::GitMissing(e) => write!(f, "git could not be started: {e}"),
            GitError::NotARepo => f.write_str("This folder is not a git repository."),
            GitError::IndexLocked => {
                f.write_str("Another git process is running (index.lock exists).")
            }
            GitError::AuthFailed => f.write_str("Authentication failed."),
            GitError::NonFastForward => f.write_str("The remote has commits you don't have yet."),
            GitError::Other { stderr, .. } => f.write_str(stderr.trim()),
        }
    }
}

impl std::error::Error for GitError {}

/// Map a failed git run to a friendly error. Relies on `LC_ALL=C`, which `run` always sets.
pub fn classify(code: Option<i32>, stderr: &str) -> GitError {
    let s = stderr;
    if s.contains("not a git repository") {
        GitError::NotARepo
    } else if s.contains("index.lock") && s.contains("File exists") {
        GitError::IndexLocked
    } else if s.contains("Authentication failed")
        || s.contains("could not read Username")
        || s.contains("Permission denied (publickey")
    {
        GitError::AuthFailed
    } else if s.contains("[rejected]")
        && (s.contains("non-fast-forward") || s.contains("fetch first"))
    {
        GitError::NonFastForward
    } else {
        GitError::Other {
            code,
            stderr: stderr.to_owned(),
        }
    }
}

/// Path of the git binary: `SPORK_GIT` if set, else `git` from PATH.
pub fn git_binary() -> PathBuf {
    std::env::var_os("SPORK_GIT")
        .map(PathBuf::from)
        .unwrap_or_else(|| "git".into())
}

/// Run git in `repo` and return stdout. Never prompts on a terminal. Network commands (fetch,
/// pull, push, clone) report progress through [`job_progress`] and stop on [`cancel_job`].
pub fn run(repo: &Path, args: &[&str]) -> Result<Vec<u8>, GitError> {
    run_with_input(repo, args, None)
}

/// Like [`run`], feeding `input` to git's stdin (commit messages, pathspec lists).
pub fn run_with_input(
    repo: &Path,
    args: &[&str],
    input: Option<&[u8]>,
) -> Result<Vec<u8>, GitError> {
    run_with_env(repo, args, input, &[])
}

/// Like [`run_with_input`], with extra environment variables for this one command.
fn run_with_env(
    repo: &Path,
    args: &[&str],
    input: Option<&[u8]>,
    env: &[(&str, &str)],
) -> Result<Vec<u8>, GitError> {
    // network commands are jobs (M09): progress is forced on so it can be shown
    let network = matches!(args.first(), Some(&("fetch" | "pull" | "push" | "clone")));
    let mut cmd = Command::new(git_binary());
    cmd.args(["-c", "core.quotepath=false", "-c", "color.ui=false"])
        .args(&args[..args.len().min(1)])
        .args(network.then_some("--progress"))
        .args(&args[args.len().min(1)..])
        .current_dir(repo)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        // reads must not take the index lock behind the user's back
        .env("GIT_OPTIONAL_LOCKS", "0")
        // never wait for an editor nobody sees: Spork always passes messages explicitly
        .env("GIT_EDITOR", "true")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    askpass::configure(&mut cmd);
    cmd.envs(env.iter().copied());
    let mut child = cmd
        .spawn()
        .map_err(|e| GitError::GitMissing(e.to_string()))?;
    if let Some(input) = input {
        // write from a thread so a large stdin can't deadlock against a full stdout pipe
        let mut stdin = child.stdin.take().expect("stdin is piped");
        let input = input.to_vec();
        std::thread::spawn(move || std::io::Write::write_all(&mut stdin, &input));
    }
    let out = if network {
        wait_job(child)?
    } else {
        child
            .wait_with_output()
            .map_err(|e| GitError::GitMissing(e.to_string()))?
    };
    record(repo, args, out.status.code(), &out.stderr);
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(classify(
            out.status.code(),
            &String::from_utf8_lossy(&out.stderr),
        ))
    }
}

/// The running network command's latest progress line ("Receiving objects:  45% (450/1000)"),
/// `Some("")` before git reports any, `None` when no network command runs.
static JOB_PROGRESS: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
static JOB_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What the running fetch / pull / push / clone last reported, if one runs.
pub fn job_progress() -> Option<String> {
    JOB_PROGRESS.lock().ok()?.clone()
}

/// Stops the running fetch / pull / push / clone; it then fails with "Cancelled."
pub fn cancel_job() {
    JOB_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// The newest non-empty line of git's stderr; progress lines are rewritten in place with `\r`.
fn last_progress_line(stderr: &str) -> Option<&str> {
    stderr
        .rsplit(['\r', '\n'])
        .map(str::trim)
        .find(|l| !l.is_empty())
}

/// Waits for a network command while publishing its progress and watching for a cancel.
fn wait_job(mut child: std::process::Child) -> Result<std::process::Output, GitError> {
    use std::io::Read;
    use std::sync::atomic::Ordering;
    let set = |p: Option<String>| {
        if let Ok(mut slot) = JOB_PROGRESS.lock() {
            *slot = p;
        }
    };
    JOB_CANCEL.store(false, Ordering::SeqCst);
    set(Some(String::new()));
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).ok();
        buf
    });
    // git rewrites progress lines with \r; the newest non-empty piece is the one to show
    let err_reader = std::thread::spawn(move || {
        let (mut all, mut chunk) = (Vec::new(), [0u8; 4096]);
        while let Ok(n @ 1..) = stderr.read(&mut chunk) {
            all.extend_from_slice(&chunk[..n]);
            if let (Some(line), Ok(mut slot)) = (
                last_progress_line(&String::from_utf8_lossy(&all)),
                JOB_PROGRESS.lock(),
            ) {
                *slot = Some(line.to_owned());
            }
        }
        all
    });
    let status = loop {
        if JOB_CANCEL.swap(false, Ordering::SeqCst) {
            child.kill().ok();
            child.wait().ok();
            set(None);
            // helpers git started (ssh, remote-https) may still hold the pipes: don't wait on them
            return Err(GitError::Other {
                code: None,
                stderr: "Cancelled.".into(),
            });
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(e) => {
                set(None);
                return Err(GitError::GitMissing(e.to_string()));
            }
        }
    };
    set(None);
    Ok(std::process::Output {
        status,
        stdout: out_reader.join().unwrap_or_default(),
        stderr: err_reader.join().unwrap_or_default(),
    })
}

/// One finished git command that changed something or failed, for the activity log (M01 §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLog {
    pub time: std::time::SystemTime,
    /// The repository it ran in.
    pub dir: PathBuf,
    /// The arguments after `git`.
    pub args: String,
    /// `Some(0)` on success.
    pub code: Option<i32>,
    /// What git printed on stderr: progress, hints, errors.
    pub stderr: String,
}

const LOG_SIZE: usize = 200;
static LOG: std::sync::Mutex<std::collections::VecDeque<CommandLog>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());

/// The latest logged commands, oldest first.
pub fn activity() -> Vec<CommandLog> {
    LOG.lock()
        .map(|l| l.iter().cloned().collect())
        .unwrap_or_default()
}

/// Empties the activity log (the Git console's Clear All).
pub fn clear_activity() {
    if let Ok(mut log) = LOG.lock() {
        log.clear();
    }
}

/// Whether a command only reads; reads run on every refresh and would flood the log.
fn is_read(args: &[&str]) -> bool {
    let mut words = args.iter().copied();
    // skip leading global options (`-c key=value`, `--literal-pathspecs`)
    let sub = loop {
        match words.next() {
            Some("-c") => {
                words.next();
            }
            Some(a) if a.starts_with('-') => {}
            other => break other.unwrap_or(""),
        }
    };
    let next = words.next().unwrap_or("");
    matches!(
        sub,
        "status"
            | "log"
            | "for-each-ref"
            | "rev-parse"
            | "rev-list"
            | "show"
            | "diff"
            | "blame"
            | "ls-files"
            | "ls-tree"
            | "diff-tree"
            | "grep"
            | "reflog"
            | "ls-remote"
            | "merge-base"
            | "symbolic-ref"
            | "cat-file"
            | "check-ref-format"
            | "version"
            | "credential"
    ) || (sub == "config" && args.iter().any(|a| a.starts_with("--get")))
        || matches!(
            (sub, next),
            ("stash", "list")
                | ("worktree", "list")
                | ("submodule", "status")
                | ("lfs", "ls-files" | "version")
        )
}

/// A read that succeeded, or answered "no" (exit 1 and nothing on stderr: an unset config key,
/// not an ancestor), is no news; everything else goes in the log.
fn worth_logging(args: &[&str], code: Option<i32>, stderr: &[u8]) -> bool {
    // a credential lookup that finds nothing answers "none stored" too
    let answered = code == Some(0)
        || (code == Some(1) && stderr.trim_ascii().is_empty())
        || args.contains(&"credential");
    !(answered && is_read(args))
}

fn record(repo: &Path, args: &[&str], code: Option<i32>, stderr: &[u8]) {
    if !worth_logging(args, code, stderr) {
        return;
    }
    let Ok(mut log) = LOG.lock() else { return };
    if log.len() == LOG_SIZE {
        log.pop_front();
    }
    log.push_back(CommandLog {
        time: std::time::SystemTime::now(),
        dir: repo.to_owned(),
        args: args.join(" "),
        code,
        stderr: String::from_utf8_lossy(stderr).trim().to_owned(),
    });
}

/// Top-level directory of the repository containing `path`.
pub fn toplevel(path: &Path) -> Result<PathBuf, GitError> {
    let out = run(path, &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(String::from_utf8_lossy(&out).trim_end()))
}

/// Current branch name, or `None` when HEAD is detached. Works on an unborn branch.
pub fn head_branch(repo: &Path) -> Result<Option<String>, GitError> {
    match run(repo, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
        Ok(out) => Ok(Some(String::from_utf8_lossy(&out).trim_end().to_owned())),
        Err(GitError::Other { code: Some(1), .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Local,
    Remote,
    Tag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    pub kind: RefKind,
    /// Full name, e.g. `refs/remotes/origin/main`.
    pub full: String,
    /// Display name, e.g. `origin/main`.
    pub name: String,
    pub target: String,
    /// Commit the ref points at (annotated tags peeled to their commit).
    pub commit: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Upstream is configured but its remote branch no longer exists.
    pub upstream_gone: bool,
    pub is_head: bool,
}

const REF_FORMAT: &str = "--format=%(refname)%00%(objectname)%00%(upstream:short)%00%(upstream:track,nobracket)%00%(HEAD)%00%(*objectname)";

pub fn refs(repo: &Path) -> Result<Vec<Ref>, GitError> {
    let out = run(
        repo,
        &[
            "for-each-ref",
            REF_FORMAT,
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    )?;
    Ok(parse_refs(&String::from_utf8_lossy(&out)))
}

/// Parse `for-each-ref` output produced with `REF_FORMAT`.
pub fn parse_refs(out: &str) -> Vec<Ref> {
    out.lines()
        .filter_map(|line| {
            let mut f = line.split('\0');
            let full = f.next()?;
            let target = f.next()?;
            let upstream = f.next().unwrap_or("");
            let track = f.next().unwrap_or("");
            let head = f.next().unwrap_or("");
            let peeled = f.next().unwrap_or("");
            let (kind, name) = if let Some(n) = full.strip_prefix("refs/heads/") {
                (RefKind::Local, n)
            } else if let Some(n) = full.strip_prefix("refs/remotes/") {
                (RefKind::Remote, n)
            } else {
                (RefKind::Tag, full.strip_prefix("refs/tags/")?)
            };
            // `origin/HEAD` is a pointer, not a branch
            if kind == RefKind::Remote && name.ends_with("/HEAD") {
                return None;
            }
            let (ahead, behind, upstream_gone) = parse_track(track);
            Some(Ref {
                kind,
                full: full.to_owned(),
                name: name.to_owned(),
                target: target.to_owned(),
                commit: if peeled.is_empty() { target } else { peeled }.to_owned(),
                upstream: (!upstream.is_empty()).then(|| upstream.to_owned()),
                ahead,
                behind,
                upstream_gone,
                is_head: head == "*",
            })
        })
        .collect()
}

/// `"ahead 2, behind 1"` / `"ahead 2"` / `"gone"` / `""` → (ahead, behind, gone).
fn parse_track(track: &str) -> (u32, u32, bool) {
    if track == "gone" {
        return (0, 0, true);
    }
    let (mut ahead, mut behind) = (0, 0);
    for part in track.split(", ") {
        if let Some(n) = part.strip_prefix("ahead ") {
            ahead = n.parse().unwrap_or(0);
        } else if let Some(n) = part.strip_prefix("behind ") {
            behind = n.parse().unwrap_or(0);
        }
    }
    (ahead, behind, false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub id: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    /// Author time, Unix seconds.
    pub time: i64,
    /// Commit time (the committer's), Unix seconds.
    pub commit_time: i64,
    pub subject: String,
}

const LOG_FORMAT: &str = "--format=%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%ct%x1f%s";

/// Commits reachable from branches, remote branches, tags and HEAD, newest first (date order), at most `max`.
// shortcut: CLI log; moves to an incremental gix revwalk when repos beyond ~1M commits matter (M05 §1).
pub fn log(repo: &Path, max: usize) -> Result<Vec<Commit>, GitError> {
    let max = format!("--max-count={max}");
    let args = [
        "log",
        "-z",
        "--date-order",
        "--ignore-missing",
        &max,
        LOG_FORMAT,
    ];
    // HEAD may not exist yet (new repo); --ignore-missing skips it instead of failing
    let out = run(
        repo,
        &[
            &args[..],
            &["--branches", "--remotes", "--tags", "HEAD", "--"],
        ]
        .concat(),
    )?;
    Ok(parse_log(&String::from_utf8_lossy(&out)))
}

/// Parse `git log -z` output produced with `LOG_FORMAT`.
pub fn parse_log(out: &str) -> Vec<Commit> {
    out.split('\0')
        .filter_map(|record| {
            let mut f = record.split('\x1f');
            Some(Commit {
                id: f.next().filter(|id| !id.is_empty())?.to_owned(),
                parents: f.next()?.split_whitespace().map(str::to_owned).collect(),
                author: f.next()?.to_owned(),
                email: f.next()?.to_owned(),
                time: f.next()?.parse().ok()?,
                commit_time: f.next()?.parse().ok()?,
                subject: f.next()?.to_owned(),
            })
        })
        .collect()
}

/// Git's well-known empty tree: the "parent" of a root commit.
pub const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitDetails {
    /// Full message: subject, blank line, body.
    pub message: String,
    pub author_email: String,
    pub author_date: String,
    pub committer: String,
    pub committer_email: String,
    pub committer_date: String,
    /// Unified patch against the first parent (or the empty tree for a root commit).
    pub patch: String,
}

const DETAILS_FORMAT: &str = "--format=%ae%x1f%ad%x1f%cn%x1f%ce%x1f%cd%x1f%B";

/// Details panel data for one commit (docs/specs/M06). Merges are shown against their first parent.
pub fn commit_details(
    repo: &Path,
    id: &str,
    first_parent: Option<&str>,
    ignore_whitespace: bool,
) -> Result<CommitDetails, GitError> {
    let meta = run(
        repo,
        &[
            "show",
            "-s",
            "--date=format:%Y-%m-%d %H:%M",
            DETAILS_FORMAT,
            id,
            "--",
        ],
    )?;
    let meta = String::from_utf8_lossy(&meta);
    let mut f = meta.splitn(6, '\x1f');
    let mut next = || f.next().unwrap_or("").to_owned();
    let (author_email, author_date, committer, committer_email, committer_date) =
        (next(), next(), next(), next(), next());
    let message = next().trim_end().to_owned();
    let base = first_parent.unwrap_or(EMPTY_TREE);
    // the whole file as context: the viewer collapses it to a few lines and can expand it all
    let mut args = vec![
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--find-renames",
        "--unified=100000",
    ];
    if ignore_whitespace {
        args.push("--ignore-all-space");
    }
    args.extend([base, id, "--"]);
    let patch = run(repo, &args)?;
    Ok(CommitDetails {
        message,
        author_email,
        author_date,
        committer,
        committer_email,
        committer_date,
        patch: String::from_utf8_lossy(&patch).into_owned(),
    })
}

/// One changed path from `git status --porcelain=v2` (docs/specs/M07 §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub path: String,
    /// Previous path for renames/copies.
    pub orig_path: Option<String>,
    /// Index (staged) state: `.` unchanged, `M`, `A`, `D`, `R`, `C`, `T`; `?` untracked; `!`
    /// ignored (only listed by [`status_with`] asking for them).
    pub index: char,
    /// Work-tree (unstaged) state, same letters.
    pub worktree: char,
    pub conflicted: bool,
}

impl StatusEntry {
    /// Not tracked: new files, and ignored ones when listed.
    pub fn untracked(&self) -> bool {
        self.index == '?' || self.index == '!'
    }
    pub fn ignored(&self) -> bool {
        self.index == '!'
    }
    pub fn has_staged(&self) -> bool {
        !self.conflicted && !self.untracked() && self.index != '.'
    }
    pub fn has_unstaged(&self) -> bool {
        self.conflicted || self.untracked() || self.worktree != '.'
    }
}

pub fn status(repo: &Path) -> Result<Vec<StatusEntry>, GitError> {
    status_with(repo, false)
}

/// [`status`], with `ignored` also the ignored files (a wholly ignored folder as one `dir/` entry).
pub fn status_with(repo: &Path, ignored: bool) -> Result<Vec<StatusEntry>, GitError> {
    let out = run(
        repo,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
            "--find-renames",
        ],
    )?;
    let mut entries = parse_status(&String::from_utf8_lossy(&out));
    if ignored {
        // asked apart: with every untracked file listed, git would list ignored folders file by file
        let out = run(
            repo,
            &[
                "status",
                "--porcelain=v2",
                "-z",
                "--untracked-files=normal",
                "--ignored",
            ],
        )?;
        let listed = parse_status(&String::from_utf8_lossy(&out));
        entries.extend(listed.into_iter().filter(StatusEntry::ignored));
    }
    Ok(entries)
}

/// Parse `git status --porcelain=v2 -z` output.
pub fn parse_status(out: &str) -> Vec<StatusEntry> {
    let mut entries = Vec::new();
    let mut records = out.split('\0');
    while let Some(record) = records.next() {
        let entry = |xy: &str, path: &str, orig: Option<&str>, conflicted| {
            let mut c = xy.chars();
            StatusEntry {
                path: path.to_owned(),
                orig_path: orig.map(str::to_owned),
                index: c.next().unwrap_or('.'),
                worktree: c.next().unwrap_or('.'),
                conflicted,
            }
        };
        match record.split_once(' ') {
            // ordinary: 1 XY sub mH mI mW hH hI path
            Some(("1", rest)) => {
                if let [xy, .., path] = rest.splitn(8, ' ').collect::<Vec<_>>()[..] {
                    entries.push(entry(xy, path, None, false));
                }
            }
            // rename/copy: 2 XY sub mH mI mW hH hI score path, then origPath as its own record
            Some(("2", rest)) => {
                if let [xy, .., path] = rest.splitn(9, ' ').collect::<Vec<_>>()[..] {
                    entries.push(entry(xy, path, records.next(), false));
                }
            }
            // unmerged: u XY sub m1 m2 m3 mW h1 h2 h3 path
            Some(("u", rest)) => {
                if let [xy, .., path] = rest.splitn(10, ' ').collect::<Vec<_>>()[..] {
                    entries.push(entry(xy, path, None, true));
                }
            }
            Some(("?", path)) => entries.push(entry("??", path, None, false)),
            Some(("!", path)) => entries.push(entry("!!", path, None, false)),
            _ => {} // `#` headers, trailing empty record
        }
    }
    entries
}

/// What git ignores in the work tree, folders as one `dir/` entry each (`target/`,
/// `node_modules/`): changes there are no news to a git client.
pub fn ignored_paths(repo: &Path) -> Vec<String> {
    run(
        repo,
        &[
            "ls-files",
            "-z",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
        ],
    )
    .map(|out| {
        String::from_utf8_lossy(&out)
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect()
    })
    .unwrap_or_default()
}

/// `git add` the paths (also records deletions).
pub fn stage(repo: &Path, paths: &[&str]) -> Result<(), GitError> {
    with_pathspecs(repo, &["add", "--all"], paths)
}

/// `git add --force`: stages ignored paths the user picked on purpose.
pub fn stage_ignored(repo: &Path, paths: &[&str]) -> Result<(), GitError> {
    with_pathspecs(repo, &["add", "--force"], paths)
}

/// Remove the paths from the index, keeping work-tree changes. Works before the first commit too.
pub fn unstage(repo: &Path, paths: &[&str]) -> Result<(), GitError> {
    if run(repo, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_ok() {
        with_pathspecs(repo, &["restore", "--staged"], paths)
    } else {
        with_pathspecs(repo, &["rm", "--cached", "-r", "--quiet"], paths)
    }
}

/// Pathspec that matches exactly `path`: a file named `*.rs` or `:x` is never a pattern.
/// (Per path, not `GIT_LITERAL_PATHSPECS`, which breaks `git stash --include-untracked`.)
pub(crate) fn literal(path: &str) -> String {
    format!(":(literal){path}")
}

/// Runs `git <args> --pathspec-from-file=-` with NUL-separated paths on stdin (no argv length limits).
fn with_pathspecs(repo: &Path, args: &[&str], paths: &[&str]) -> Result<(), GitError> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut input = Vec::new();
    for p in paths {
        input.extend_from_slice(literal(p).as_bytes());
        input.push(0);
    }
    let args = [args, &["--pathspec-from-file=-", "--pathspec-file-nul"]].concat();
    run_with_input(repo, &args, Some(&input)).map(drop)
}

/// Throws away unstaged changes (docs/specs/M07 §3): tracked files go back to their staged/committed
/// content; untracked files are moved to the system Trash, never deleted outright.
pub fn discard(repo: &Path, entries: &[StatusEntry]) -> Result<(), GitError> {
    let tracked: Vec<&str> = entries
        .iter()
        .filter(|e| !e.untracked() && !e.conflicted && e.worktree != '.')
        .map(|e| e.path.as_str())
        .collect();
    with_pathspecs(repo, &["restore", "--worktree"], &tracked)?;
    let untracked: Vec<PathBuf> = entries
        .iter()
        .filter(|e| e.untracked())
        .map(|e| repo.join(&e.path))
        .collect();
    if !untracked.is_empty() {
        trash::delete_all(&untracked).map_err(|e| GitError::Other {
            code: None,
            stderr: format!("Could not move untracked files to the Trash: {e}"),
        })?;
    }
    Ok(())
}

/// Puts the tracked `entries` back as HEAD has them, staged and unstaged (IntelliJ's Rollback).
/// A file HEAD doesn't have (added, a rename's new path) is unstaged, and moved to the Trash when
/// `delete_added`; a rename's old path comes back.
pub fn rollback(repo: &Path, entries: &[StatusEntry], delete_added: bool) -> Result<(), GitError> {
    let tracked: Vec<&StatusEntry> = entries.iter().filter(|e| !e.untracked()).collect();
    let added = |e: &&&StatusEntry| matches!(e.index, 'A' | 'C' | 'R');
    let in_head: Vec<&str> = tracked
        .iter()
        .filter_map(|e| match e.index {
            'R' => e.orig_path.as_deref(),
            'A' | 'C' => None,
            _ => Some(e.path.as_str()),
        })
        .collect();
    let new: Vec<&str> = tracked
        .iter()
        .filter(added)
        .map(|e| e.path.as_str())
        .collect();
    with_pathspecs(
        repo,
        &["restore", "--source=HEAD", "--staged", "--worktree"],
        &in_head,
    )?;
    with_pathspecs(repo, &["restore", "--staged"], &new)?;
    let delete: Vec<PathBuf> = new
        .iter()
        .map(|path| repo.join(path))
        .filter(|file| delete_added && file.exists())
        .collect();
    to_trash(&delete)
}

/// Moves `files` to the system Trash through NSFileManager on macOS: unlike the Finder (the trash
/// crate's default) it asks for no permission to control another app.
fn to_trash(files: &[PathBuf]) -> Result<(), GitError> {
    if files.is_empty() {
        return Ok(());
    }
    #[allow(unused_mut)]
    let mut trash = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos as _};
        trash.set_delete_method(DeleteMethod::NsFileManager);
    }
    trash.delete_all(files).map_err(|e| GitError::Other {
        code: None,
        stderr: format!("Could not move the files to the Trash: {e}"),
    })
}

/// A multi-step operation git has paused, usually on conflicts (docs/specs/M11 §0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpState {
    None,
    Merging,
    Rebasing {
        step: usize,
        total: usize,
    },
    CherryPicking,
    Reverting,
    /// `git am` applying mailbox patches (M06 §3).
    Applying {
        step: usize,
        total: usize,
    },
}

pub fn op_state(repo: &Path) -> Result<OpState, GitError> {
    let git_dir = PathBuf::from(
        String::from_utf8_lossy(&run(repo, &["rev-parse", "--absolute-git-dir"])?).trim_end(),
    );
    let num = |dir: &str, file: &str| {
        std::fs::read_to_string(git_dir.join(dir).join(file))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    };
    Ok(if git_dir.join("rebase-merge").is_dir() {
        OpState::Rebasing {
            step: num("rebase-merge", "msgnum"),
            total: num("rebase-merge", "end"),
        }
    } else if git_dir.join("rebase-apply/applying").is_file() {
        OpState::Applying {
            step: num("rebase-apply", "next"),
            total: num("rebase-apply", "last"),
        }
    } else if git_dir.join("rebase-apply").is_dir() {
        OpState::Rebasing {
            step: num("rebase-apply", "next"),
            total: num("rebase-apply", "last"),
        }
    } else if git_dir.join("MERGE_HEAD").is_file() {
        OpState::Merging
    } else if git_dir.join("CHERRY_PICK_HEAD").is_file() {
        OpState::CherryPicking
    } else if git_dir.join("REVERT_HEAD").is_file() {
        OpState::Reverting
    } else {
        OpState::None
    })
}

/// How an operation that can stop on conflicts ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// Paused with conflicts to resolve; then continue or abort.
    Conflicts,
}

/// Turns "git stopped because of conflicts" into `Outcome::Conflicts` instead of an error.
pub(crate) fn allow_conflicts(
    repo: &Path,
    result: Result<Vec<u8>, GitError>,
) -> Result<Outcome, GitError> {
    match result {
        Ok(_) => Ok(Outcome::Done),
        Err(e) => {
            let conflicted = status(repo).is_ok_and(|st| st.iter().any(|e| e.conflicted));
            if conflicted {
                Ok(Outcome::Conflicts)
            } else {
                Err(e)
            }
        }
    }
}

pub(crate) fn is_merge_commit(repo: &Path, commit: &str) -> bool {
    run(
        repo,
        &["rev-parse", "--verify", "--quiet", &format!("{commit}^2")],
    )
    .is_ok()
}

/// How a merge brings a branch in (M11 §1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MergeKind {
    /// Fast-forward when possible, else a merge commit (git's default, or `merge.ff`).
    #[default]
    Auto,
    /// Always a merge commit (`--no-ff`).
    Commit,
    /// The branch's changes staged as one change for the user to commit (`--squash`).
    Squash,
}

/// Merges `branch` into the current branch (M11 §1). Fast-forwards when possible.
pub fn merge(repo: &Path, branch: &str) -> Result<Outcome, GitError> {
    merge_with(repo, branch, MergeKind::Auto, true)
}

/// Merges `branch` as `kind`; `commit: false` stops before committing so the result can be
/// reviewed (a squash never commits). A fast-forward can't be stopped, so that implies `--no-ff`.
pub fn merge_with(
    repo: &Path,
    branch: &str,
    kind: MergeKind,
    commit: bool,
) -> Result<Outcome, GitError> {
    let mut args = vec!["merge", "--no-edit"];
    match (kind, commit) {
        (MergeKind::Squash, _) => args.push("--squash"),
        (MergeKind::Commit, _) | (MergeKind::Auto, false) => args.push("--no-ff"),
        (MergeKind::Auto, true) => {}
    }
    if !commit && kind != MergeKind::Squash {
        args.push("--no-commit");
    }
    args.push(branch);
    allow_conflicts(repo, run(repo, &args))
}

/// Rebases the current branch onto `onto`, stashing and restoring local changes (M11 §2).
pub fn rebase(repo: &Path, onto: &str) -> Result<Outcome, GitError> {
    allow_conflicts(repo, run(repo, &["rebase", "--autostash", onto]))
}

/// Applies one commit on top of HEAD (M06). Merge commits are picked against their first parent.
pub fn cherry_pick(repo: &Path, commit: &str) -> Result<Outcome, GitError> {
    let mut args = vec!["cherry-pick"];
    if is_merge_commit(repo, commit) {
        args.extend(["-m", "1"]);
    }
    args.push(commit);
    allow_conflicts(repo, run(repo, &args))
}

/// Creates a commit undoing `commit` (M06). Merge commits are reverted against their first parent.
pub fn revert(repo: &Path, commit: &str) -> Result<Outcome, GitError> {
    let mut args = vec!["revert", "--no-edit"];
    if is_merge_commit(repo, commit) {
        args.extend(["-m", "1"]);
    }
    args.push(commit);
    allow_conflicts(repo, run(repo, &args))
}

fn op_command(state: OpState) -> Option<&'static str> {
    match state {
        OpState::None => None,
        OpState::Merging => Some("merge"),
        OpState::Rebasing { .. } => Some("rebase"),
        OpState::CherryPicking => Some("cherry-pick"),
        OpState::Reverting => Some("revert"),
        OpState::Applying { .. } => Some("am"),
    }
}

/// Applies a patch file made by `git format-patch` (M06 §3): as commits with `commit`
/// (`git am`), else as changes in the index and working copy (`git apply`). Both fall back to a
/// 3-way merge and stop on conflicts.
pub fn apply_mailbox(repo: &Path, file: &Path, commit: bool) -> Result<Outcome, GitError> {
    let file = file.to_string_lossy();
    let cmd = if commit { "am" } else { "apply" };
    allow_conflicts(repo, run(repo, &[cmd, "--3way", "--", &file]))
}

/// Continues the paused operation once conflicts are resolved and staged.
pub fn continue_op(repo: &Path) -> Result<Outcome, GitError> {
    let Some(cmd) = op_command(op_state(repo)?) else {
        return Ok(Outcome::Done);
    };
    allow_conflicts(repo, run(repo, &[cmd, "--continue"]))
}

/// Skips the current commit of a rebase / cherry-pick / revert.
pub fn skip_op(repo: &Path) -> Result<Outcome, GitError> {
    let Some(cmd) = op_command(op_state(repo)?) else {
        return Ok(Outcome::Done);
    };
    allow_conflicts(repo, run(repo, &[cmd, "--skip"]))
}

/// Abandons the paused operation and restores the state from before it started.
pub fn abort_op(repo: &Path) -> Result<(), GitError> {
    let Some(cmd) = op_command(op_state(repo)?) else {
        return Ok(());
    };
    run(repo, &[cmd, "--abort"]).map(drop)
}

/// Resolves a conflicted file by taking one side whole (M11 §4), then marks it resolved.
/// `ours` is the current branch's side (during a rebase: the branch being rebased onto).
pub fn resolve_with(repo: &Path, path: &str, ours: bool) -> Result<(), GitError> {
    let side = if ours { "--ours" } else { "--theirs" };
    let spec = &literal(path);
    match run(repo, &["checkout", side, "--", spec]) {
        Ok(_) => stage(repo, &[path]),
        // the chosen side deleted the file
        Err(_) => run(repo, &["rm", "--quiet", "--", spec]).map(drop),
    }
}

/// Stages every change to tracked files (`git add -u`): IntelliJ's Stage All Tracked.
pub fn stage_tracked(repo: &Path) -> Result<(), GitError> {
    run(repo, &["add", "--update"]).map(drop)
}

/// Stages every change and untracked file (`git add -A`): IntelliJ's Stage All.
pub fn stage_all(repo: &Path) -> Result<(), GitError> {
    run(repo, &["add", "--all"]).map(drop)
}

/// Records untracked `paths` in the index without their content (`git add -N`): IntelliJ's Stage
/// Without Content, so their content shows as an unstaged change.
pub fn stage_without_content(repo: &Path, paths: &[&str]) -> Result<(), GitError> {
    with_pathspecs(repo, &["add", "--intent-to-add"], paths)
}

/// The conflicts resolved since the merge stopped, which git can still put back (its index's
/// resolve-undo record): what IntelliJ's Revert Resolved acts on.
pub fn resolved_conflicts(repo: &Path) -> Result<Vec<String>, GitError> {
    let out = run(repo, &["ls-files", "--resolve-undo", "-z"])?;
    let mut paths: Vec<String> = String::from_utf8_lossy(&out)
        .split('\0')
        .filter_map(|entry| entry.split_once('\t').map(|(_, path)| path.to_owned()))
        .collect();
    paths.dedup();
    Ok(paths)
}

/// Puts `paths`' conflicts back, as they were when the merge stopped (Revert Resolved).
pub fn revert_resolved(repo: &Path, paths: &[String]) -> Result<(), GitError> {
    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
    with_pathspecs(repo, &["checkout", "--merge"], &paths)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    /// Position in `git stash list`, as in `stash@{index}`.
    pub index: usize,
    pub id: String,
    pub message: String,
    pub time: i64,
    /// The commit it was made on (its first parent).
    pub base: String,
    pub author: String,
}

/// Saved stashes, newest first (docs/specs/M12 §1).
pub fn stashes(repo: &Path) -> Result<Vec<Stash>, GitError> {
    let out = run(
        repo,
        &["stash", "list", "--format=%H%x1f%gs%x1f%ct%x1f%P%x1f%an"],
    )?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let mut f = line.split('\x1f');
            Some(Stash {
                index,
                id: f.next()?.to_owned(),
                message: f.next()?.to_owned(),
                time: f.next()?.parse().ok()?,
                base: f.next()?.split(' ').next()?.to_owned(),
                author: f.next()?.to_owned(),
            })
        })
        .collect())
}

/// Stashes local changes (untracked files too when asked). Empty message = git's default.
pub fn stash_save(repo: &Path, message: &str, include_untracked: bool) -> Result<(), GitError> {
    let mut args = vec!["stash", "push"];
    if include_untracked {
        args.push("--include-untracked");
    }
    if !message.trim().is_empty() {
        args.extend(["--message", message.trim()]);
    }
    run(repo, &args).map(drop)
}

/// Applies a stash; `pop` also drops it, except when it conflicts (git keeps it then).
pub fn stash_apply(repo: &Path, index: usize, pop: bool) -> Result<Outcome, GitError> {
    let name = format!("stash@{{{index}}}");
    allow_conflicts(
        repo,
        run(repo, &["stash", if pop { "pop" } else { "apply" }, &name]),
    )
}

pub fn stash_drop(repo: &Path, index: usize) -> Result<(), GitError> {
    run(repo, &["stash", "drop", &format!("stash@{{{index}}}")]).map(drop)
}

/// Creates tag `name` at `target`; annotated when `message` isn't empty (docs/specs/M12 §2).
pub fn create_tag(repo: &Path, name: &str, target: &str, message: &str) -> Result<(), GitError> {
    create_tag_with(repo, name, target, message, false)
}

/// [`create_tag`], signed with the key git is set up with (`user.signingkey`, `gpg.format`)
/// when `sign`; a signed tag is annotated, with the name as message if none is given.
pub fn create_tag_with(
    repo: &Path,
    name: &str,
    target: &str,
    message: &str,
    sign: bool,
) -> Result<(), GitError> {
    if run(repo, &["check-ref-format", &format!("refs/tags/{name}")]).is_err() {
        return Err(GitError::Other {
            code: None,
            stderr: format!("`{name}` is not a valid tag name."),
        });
    }
    if sign {
        let message = Some(message.trim())
            .filter(|m| !m.is_empty())
            .unwrap_or(name);
        run(repo, &["tag", "--sign", name, "--message", message, target]).map(drop)
    } else if message.trim().is_empty() {
        run(repo, &["tag", name, target]).map(drop)
    } else {
        run(
            repo,
            &[
                "tag",
                "--annotate",
                name,
                "--message",
                message.trim(),
                target,
            ],
        )
        .map(drop)
    }
}

pub fn delete_tag(repo: &Path, name: &str) -> Result<(), GitError> {
    run(repo, &["tag", "--delete", name]).map(drop)
}

/// Pushes one tag to `remote`.
pub fn push_tag(repo: &Path, remote: &str, name: &str) -> Result<(), GitError> {
    run(repo, &["push", remote, &format!("refs/tags/{name}")]).map(drop)
}

/// What to do with one commit in an interactive rebase (docs/specs/M11 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoAction {
    Pick,
    /// Stop after applying it, so it can be changed and amended; then continue.
    Edit,
    /// Meld into the previous commit, keeping both messages.
    Squash,
    /// Meld into the previous commit, dropping this message.
    Fixup,
    Drop,
}

/// Commits after `base` up to HEAD, oldest first: the rows of the interactive rebase editor.
pub fn rebase_range(repo: &Path, base: &str) -> Result<Vec<Commit>, GitError> {
    let range = format!("{base}..HEAD");
    let out = run(
        repo,
        &[
            "log",
            "-z",
            "--reverse",
            "--topo-order",
            LOG_FORMAT,
            &range,
            "--",
        ],
    )?;
    let commits = parse_log(&String::from_utf8_lossy(&out));
    if commits.iter().any(|c| c.parents.len() > 1) {
        return Err(GitError::Other {
            code: None,
            stderr: "This range contains merge commits; interactive rebase would flatten them."
                .into(),
        });
    }
    Ok(commits)
}

/// Rewrites the commits after `base` following `plan` (in the new order). Git does the work:
/// the plan is written as git's todo list and copied in by `GIT_SEQUENCE_EDITOR`.
pub fn interactive_rebase(
    repo: &Path,
    base: &str,
    plan: &[(String, TodoAction)],
) -> Result<Outcome, GitError> {
    interactive_rebase_reworded(repo, base, plan, &[])
}

/// [`interactive_rebase`], the commits in `messages` (by id) given a new message: the commit
/// and the ones squashed or fixed up into it are amended with it.
pub fn interactive_rebase_reworded(
    repo: &Path,
    base: &str,
    plan: &[(String, TodoAction)],
    messages: &[(String, String)],
) -> Result<Outcome, GitError> {
    let fail = |msg: &str| GitError::Other {
        code: None,
        stderr: msg.to_owned(),
    };
    match plan.iter().find(|(_, a)| *a != TodoAction::Drop) {
        None => return Err(fail("Every commit would be dropped; use Reset instead.")),
        Some((_, TodoAction::Squash | TodoAction::Fixup)) => {
            return Err(fail(
                "The first kept commit can't be squashed or fixed up: there is nothing before it.",
            ));
        }
        _ => {}
    }
    let dir = std::env::temp_dir().join(format!("spork-reword-{}", askpass::random_token()));
    let mut todo = String::new();
    // the new message waits for the commits melded into the reworded one
    let mut amend: Option<String> = None;
    for (ix, (id, action)) in plan.iter().enumerate() {
        let verb = match action {
            TodoAction::Pick => "pick",
            TodoAction::Edit => "edit",
            TodoAction::Squash => "squash",
            TodoAction::Fixup => "fixup",
            TodoAction::Drop => "drop",
        };
        todo.push_str(&format!("{verb} {id}\n"));
        if let Some((_, message)) = messages.iter().find(|(m, _)| m == id) {
            std::fs::create_dir_all(&dir)
                .and_then(|()| std::fs::write(dir.join(format!("{ix}")), message))
                .map_err(|e| fail(&e.to_string()))?;
            amend = Some(format!(
                "exec git commit --amend --allow-empty -q --cleanup=strip -F {}\n",
                shell_quote(&dir.join(format!("{ix}")))
            ));
        }
        let melded_next = matches!(
            plan.get(ix + 1),
            Some((_, TodoAction::Squash | TodoAction::Fixup))
        );
        if !melded_next && let Some(line) = amend.take() {
            todo.push_str(&line);
        }
    }
    let result = run_todo(repo, base, &todo);
    std::fs::remove_dir_all(&dir).ok();
    result
}

/// Melds `commits` (on the current branch) together without the rebase editor (M06 §2): one
/// commit melds into its parent, several into the oldest of them. `keep_messages`: squash (both
/// messages) rather than fixup (the earlier message only). The commits after them are replayed.
pub fn meld(repo: &Path, commits: &[String], keep_messages: bool) -> Result<Outcome, GitError> {
    let action = if keep_messages {
        TodoAction::Squash
    } else {
        TodoAction::Fixup
    };
    let (ids, oldest) = oldest_first(repo, commits)?;
    // one commit melds into its parent, which stays a pick
    let (melded, base) = if ids.len() == 1 {
        (ids, format!("{oldest}^^"))
    } else {
        (ids[1..].to_vec(), format!("{oldest}^"))
    };
    rewrite(repo, &base, &melded, action)
}

/// Squash Commits…: melds `commits` (on the current branch) as [`meld`] does, then gives the
/// commit they became `message`.
pub fn squash(repo: &Path, commits: &[String], message: &str) -> Result<Outcome, GitError> {
    let (ids, _) = oldest_first(repo, commits)?;
    let newest = ids.last().cloned().unwrap_or_default();
    // the commits above the newest one stay above the squashed commit
    let range = format!("{newest}..HEAD");
    let above = String::from_utf8_lossy(&run(repo, &["rev-list", "--count", &range])?)
        .trim()
        .to_owned();
    if meld(repo, commits, true)? == Outcome::Conflicts {
        return Ok(Outcome::Conflicts);
    }
    match above.as_str() {
        "0" => revision::reword_head(repo, message).map(|()| Outcome::Done),
        n => {
            let squashed =
                String::from_utf8_lossy(&run(repo, &["rev-parse", &format!("HEAD~{n}")])?)
                    .trim()
                    .to_owned();
            reword(repo, &squashed, message)
        }
    }
}

/// Removes `commits` (on the current branch) from its history; the commits after them are replayed.
pub fn drop_commits(repo: &Path, commits: &[String]) -> Result<Outcome, GitError> {
    let (ids, oldest) = oldest_first(repo, commits)?;
    rewrite(repo, &format!("{oldest}^"), &ids, TodoAction::Drop)
}

/// Takes the changes `commit` (on the current branch) made to `paths` out of it, replaying the
/// commits after it (IntelliJ's Drop Selected Changes).
pub fn drop_changes(repo: &Path, commit: &str, paths: &[String]) -> Result<Outcome, GitError> {
    split_changes(repo, commit, paths, None)
}

/// Moves the changes `commit` (on the current branch) made to `paths` into a new commit right
/// after it, with `message` and its author (IntelliJ's Extract Selected Changes to Separate
/// Commit); the commits after it are replayed.
pub fn extract_changes(
    repo: &Path,
    commit: &str,
    paths: &[String],
    message: &str,
) -> Result<Outcome, GitError> {
    split_changes(repo, commit, paths, Some(message))
}

fn split_changes(
    repo: &Path,
    commit: &str,
    paths: &[String],
    message: Option<&str>,
) -> Result<Outcome, GitError> {
    let fail = |msg: &str| GitError::Other {
        code: None,
        stderr: msg.to_owned(),
    };
    let full = String::from_utf8_lossy(&run(repo, &["rev-parse", commit])?)
        .trim()
        .to_owned();
    let base = format!("{full}^");
    if run(repo, &["rev-parse", "--verify", "--quiet", &base]).is_err() {
        return Err(fail(
            "The first commit of the history can't be changed here yet.",
        ));
    }
    let changed: Vec<String> = commit_files(repo, &full)?
        .into_iter()
        .map(|(_, path)| path)
        .collect();
    if changed.iter().all(|p| paths.contains(p)) {
        return Err(fail(match message {
            Some(_) => "Unable to extract all changes from the commit",
            None => "Unable to drop all changes from the commit",
        }));
    }
    let range = rebase_range(repo, &base)?;
    if range.first().is_none_or(|c| c.id != full) {
        return Err(fail("That commit isn't on the current branch."));
    }
    let pathspecs: Vec<String> = paths
        .iter()
        .map(|p| shell_quote(Path::new(&literal(p))))
        .collect();
    let pathspecs = pathspecs.join(" ");
    let mut exec = format!(
        "git restore --source={full}^ --staged --worktree -- {pathspecs} && git commit --amend --no-edit --allow-empty --no-verify"
    );
    let message_file =
        std::env::temp_dir().join(format!("spork-extract-message-{}", askpass::random_token()));
    if let Some(message) = message {
        std::fs::write(&message_file, message).map_err(|e| fail(&e.to_string()))?;
        let author =
            String::from_utf8_lossy(&run(repo, &["log", "-1", "--format=%an <%ae>", &full])?)
                .trim()
                .to_owned();
        exec.push_str(&format!(
            " && git restore --source={full} --staged --worktree -- {pathspecs} && git commit --no-verify --author={} -F {}",
            shell_quote(Path::new(&author)),
            shell_quote(&message_file),
        ));
    }
    let todo: String = range
        .iter()
        .enumerate()
        .map(|(ix, c)| match ix {
            0 => format!("pick {}\nexec {exec}\n", c.id),
            _ => format!("pick {}\n", c.id),
        })
        .collect();
    let result = run_todo(repo, &base, &todo);
    std::fs::remove_file(&message_file).ok();
    result
}

/// Moves `commit` to sit right after `after` (newer than it) on the current branch, replaying
/// the commits in between: drag and drop in the commit list (M06 §2).
pub fn move_commit(repo: &Path, commit: &str, after: &str) -> Result<Outcome, GitError> {
    let fail = |msg: &str| GitError::Other {
        code: None,
        stderr: msg.to_owned(),
    };
    let (ids, oldest) = oldest_first(repo, &[commit.to_owned(), after.to_owned()])?;
    let full = |rev: &str| -> Result<String, GitError> {
        Ok(String::from_utf8_lossy(&run(repo, &["rev-parse", rev])?)
            .trim()
            .to_owned())
    };
    let (moved, after) = (full(commit)?, full(after)?);
    if moved == after || ids.len() < 2 {
        return Ok(Outcome::Done);
    }
    let base = format!("{oldest}^");
    if run(repo, &["rev-parse", "--verify", "--quiet", &base]).is_err() {
        return Err(fail("The first commit of the history can't be moved yet."));
    }
    let mut order: Vec<String> = rebase_range(repo, &base)?
        .into_iter()
        .map(|c| c.id)
        .collect();
    let before = order.clone();
    let from = order
        .iter()
        .position(|c| *c == moved)
        .ok_or_else(|| fail("Only commits of the current branch can be moved."))?;
    order.remove(from);
    let to = order
        .iter()
        .position(|c| *c == after)
        .ok_or_else(|| fail("Only commits of the current branch can be moved."))?;
    order.insert(to + 1, moved);
    if order == before {
        return Ok(Outcome::Done);
    }
    let plan: Vec<(String, TodoAction)> =
        order.into_iter().map(|c| (c, TodoAction::Pick)).collect();
    interactive_rebase(repo, &base, &plan)
}

/// Full ids of `commits`, oldest first, and the oldest one.
fn oldest_first(repo: &Path, commits: &[String]) -> Result<(Vec<String>, String), GitError> {
    let mut ids: Vec<(usize, String)> = commits
        .iter()
        .map(|c| {
            let id = String::from_utf8_lossy(&run(repo, &["rev-parse", c])?)
                .trim()
                .to_owned();
            // how far from the start of history: the smaller, the older
            let depth = String::from_utf8_lossy(&run(repo, &["rev-list", "--count", &id])?)
                .trim()
                .parse()
                .unwrap_or(0);
            Ok((depth, id))
        })
        .collect::<Result<_, GitError>>()?;
    ids.sort();
    let ids: Vec<String> = ids.into_iter().map(|(_, id)| id).collect();
    let oldest = ids.first().cloned().ok_or_else(|| GitError::Other {
        code: None,
        stderr: "No commits given.".into(),
    })?;
    Ok((ids, oldest))
}

/// Replays the commits after `base` with `action` on `marked` and pick on the rest.
fn rewrite(
    repo: &Path,
    base: &str,
    marked: &[String],
    action: TodoAction,
) -> Result<Outcome, GitError> {
    let fail = |msg: &str| GitError::Other {
        code: None,
        stderr: msg.to_owned(),
    };
    if run(repo, &["rev-parse", "--verify", "--quiet", base]).is_err() {
        return Err(fail(
            "The first commits of the history can't be changed here yet.",
        ));
    }
    let range = rebase_range(repo, base)?;
    if !marked.iter().all(|m| range.iter().any(|c| c.id == *m)) {
        return Err(fail("Those commits aren't all on the current branch."));
    }
    // dropping every commit after `base` is moving the branch back; --keep refuses rather
    // than lose local changes
    if action == TodoAction::Drop && range.len() == marked.len() {
        return run(repo, &["reset", "--keep", base]).map(|_| Outcome::Done);
    }
    let plan: Vec<(String, TodoAction)> = range
        .iter()
        .map(|c| {
            let a = if marked.contains(&c.id) {
                action
            } else {
                TodoAction::Pick
            };
            (c.id.clone(), a)
        })
        .collect();
    interactive_rebase(repo, base, &plan)
}

/// Changes the message of `commit`, an ancestor of HEAD, by replaying the commits after it with
/// an amend right after it (M06 §2). HEAD itself: see `revision::reword_head`.
// shortcut: not the root commit (that needs `rebase --root`).
pub fn reword(repo: &Path, commit: &str, message: &str) -> Result<Outcome, GitError> {
    let fail = |msg: &str| GitError::Other {
        code: None,
        stderr: msg.to_owned(),
    };
    let base = format!("{commit}^");
    if run(repo, &["rev-parse", "--verify", "--quiet", &base]).is_err() {
        return Err(fail(
            "The first commit's message can't be changed here yet.",
        ));
    }
    let full = String::from_utf8_lossy(&run(repo, &["rev-parse", commit])?)
        .trim()
        .to_owned();
    let commits = rebase_range(repo, &base)?;
    if !commits.iter().any(|c| c.id == full) {
        return Err(fail("That commit isn't on the current branch."));
    }
    let file = std::env::temp_dir().join(format!("spork-message-{}", askpass::random_token()));
    std::fs::write(&file, message).map_err(|e| fail(&e.to_string()))?;
    let amend = format!(
        "exec git commit --amend --only --allow-empty --no-verify --cleanup=strip -F {}\n",
        shell_quote(&file)
    );
    let todo: String = commits
        .iter()
        .map(|c| {
            let pick = format!("pick {}\n", c.id);
            if c.id == full { pick + &amend } else { pick }
        })
        .collect();
    let result = run_todo(repo, &base, &todo);
    std::fs::remove_file(&file).ok();
    result
}

/// `path` single-quoted for the shell git runs editors and `exec` lines in.
fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

/// Runs `git rebase -i base` with `todo` as its todo list, copied in by `GIT_SEQUENCE_EDITOR`.
fn run_todo(repo: &Path, base: &str, todo: &str) -> Result<Outcome, GitError> {
    let file = std::env::temp_dir().join(format!("spork-rebase-todo-{}", askpass::random_token()));
    std::fs::write(&file, todo).map_err(|e| GitError::Other {
        code: None,
        stderr: e.to_string(),
    })?;
    // the editor command runs in git's shell with the todo path appended: `cp <ours> <git's>`
    let editor = format!("cp {}", shell_quote(&file));
    let result = run_with_env(
        repo,
        &["rebase", "--interactive", "--autostash", base],
        None,
        &[("GIT_SEQUENCE_EDITOR", &editor)],
    );
    std::fs::remove_file(&file).ok();
    allow_conflicts(repo, result)
}

/// Default folder name for a clone: `https://host/org/spork.git` → `spork`, `git@host:org/x` → `x`.
pub fn repo_name_from_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    let last = trimmed.rsplit(['/', ':', '\\']).next().unwrap_or(trimmed);
    let name = last.strip_suffix(".git").unwrap_or(last);
    if name.is_empty() {
        "repository".into()
    } else {
        name.into()
    }
}

/// Clones `url` into `parent/name` with submodules (docs/specs/M02 §1), checking out `branch`
/// (empty: the remote's default) with only the last `depth` commits when given. Refuses to touch
/// an existing non-empty folder. Returns the new repository's path.
pub fn clone(
    url: &str,
    parent: &Path,
    name: &str,
    branch: &str,
    depth: Option<u32>,
) -> Result<PathBuf, GitError> {
    let dest = parent.join(name);
    let busy = std::fs::read_dir(&dest).is_ok_and(|mut entries| entries.next().is_some());
    if busy {
        return Err(GitError::Other {
            code: None,
            stderr: format!("{} already exists and isn't empty.", dest.display()),
        });
    }
    let depth = depth.map(|d| format!("--depth={d}"));
    let mut args = vec!["clone", "--recurse-submodules"];
    args.extend(depth.as_deref());
    if !branch.trim().is_empty() {
        args.extend(["--branch", branch.trim()]);
    }
    args.extend(["--", url.trim(), name]);
    run(parent, &args)?;
    Ok(dest)
}

/// Creates a new repository in `path` (made if missing). Uses `init.defaultBranch`, else `main`.
pub fn init(path: &Path) -> Result<(), GitError> {
    std::fs::create_dir_all(path).map_err(|e| GitError::Other {
        code: None,
        stderr: e.to_string(),
    })?;
    let has_default = run(path, &["config", "--get", "init.defaultBranch"]).is_ok();
    let args: &[&str] = if has_default {
        &["init", "--quiet"]
    } else {
        &["init", "--quiet", "--initial-branch=main"]
    };
    run(path, args).map(drop)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// Checked-out branch (short name); `None` when detached.
    pub branch: Option<String>,
    /// The repository's main working tree (can't be removed).
    pub is_main: bool,
    /// `git worktree lock`ed: prune and remove leave it alone.
    pub locked: bool,
    /// Its folder is gone: `git worktree prune` would drop it.
    pub prunable: bool,
}

/// All working trees of the repository, main one first (docs/specs/M12 §4).
pub fn worktrees(repo: &Path) -> Result<Vec<Worktree>, GitError> {
    let out = run(repo, &["worktree", "list", "--porcelain", "-z"])?;
    let mut list: Vec<Worktree> = Vec::new();
    for field in String::from_utf8_lossy(&out).split('\0') {
        if let Some(path) = field.strip_prefix("worktree ") {
            list.push(Worktree {
                path: PathBuf::from(path),
                branch: None,
                is_main: list.is_empty(),
                locked: false,
                prunable: false,
            });
            continue;
        }
        let Some(w) = list.last_mut() else { continue };
        // `locked` and `prunable` come bare or followed by a reason
        let (key, value) = field.split_once(' ').unwrap_or((field, ""));
        match key {
            "branch" => w.branch = Some(value.trim_start_matches("refs/heads/").to_owned()),
            "locked" => w.locked = true,
            "prunable" => w.prunable = true,
            _ => {}
        }
    }
    Ok(list)
}

/// Checks out an existing local `branch` in a new working tree at `path`.
pub fn add_worktree(repo: &Path, path: &Path, branch: &str) -> Result<(), GitError> {
    run(repo, &["worktree", "add", &path.to_string_lossy(), branch]).map(drop)
}

/// The files a new worktree copies from this one, which `.worktreeinclude` lists in
/// `.gitignore` syntax: the untracked ones its patterns match, from the root.
pub fn worktree_include_files(repo: &Path) -> Result<Vec<String>, GitError> {
    let out = run(
        repo,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-from=.worktreeinclude",
            "-z",
        ],
    )?;
    Ok(String::from_utf8_lossy(&out)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Locks a working tree so prune and remove leave it alone, or unlocks it.
pub fn lock_worktree(repo: &Path, path: &Path, lock: bool) -> Result<(), GitError> {
    let verb = if lock { "lock" } else { "unlock" };
    run(repo, &["worktree", verb, &path.to_string_lossy()]).map(drop)
}

/// Drops git's records of working trees whose folders are gone.
pub fn prune_worktrees(repo: &Path) -> Result<(), GitError> {
    run(repo, &["worktree", "prune"]).map(drop)
}

/// Removes a working tree; git refuses if it has uncommitted changes.
pub fn remove_worktree(repo: &Path, path: &Path) -> Result<(), GitError> {
    run(repo, &["worktree", "remove", &path.to_string_lossy()]).map(drop)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submodule {
    pub path: String,
    pub commit: String,
    /// `' '` up to date, `'-'` not initialized, `'+'` different commit checked out, `'U'` conflict.
    pub state: char,
}

/// Submodules, recursively (M12 §3).
pub fn submodules(repo: &Path) -> Result<Vec<Submodule>, GitError> {
    let out = run(repo, &["submodule", "status", "--recursive"])?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|line| {
            let state = line.chars().next()?;
            let mut f = line[state.len_utf8()..].split(' ');
            let commit = f.next()?.to_owned();
            let path = f.next()?.to_owned();
            Some(Submodule {
                path,
                commit,
                state,
            })
        })
        .collect())
}

/// Initializes and checks out every submodule at the commit the repository records.
pub fn update_submodules(repo: &Path) -> Result<(), GitError> {
    run(repo, &["submodule", "update", "--init", "--recursive"]).map(drop)
}

/// Where a partial patch from `spork_patch` is applied (docs/specs/M07 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchTarget {
    /// Forward patch into the index.
    Stage,
    /// Reverse patch out of the index.
    Unstage,
    /// Reverse patch out of the work tree (throws the selected changes away).
    Discard,
}

/// Applies a patch to the index or work tree. Git rejects patches that don't fit, so a failure
/// changes nothing.
pub fn apply_patch(repo: &Path, patch: &str, target: PatchTarget) -> Result<(), GitError> {
    let mut args = vec!["apply", "--recount", "--whitespace=nowarn"];
    match target {
        PatchTarget::Stage => args.push("--cached"),
        PatchTarget::Unstage => args.extend(["--cached", "--reverse"]),
        PatchTarget::Discard => args.push("--reverse"),
    }
    args.push("-");
    run_with_input(repo, &args, Some(patch.as_bytes())).map(drop)
}

/// Git refused to commit because it doesn't know the user's name and email yet.
pub fn is_identity_missing(e: &GitError) -> bool {
    matches!(e, GitError::Other { stderr, .. } if stderr.contains("Please tell me who you are"))
}

/// Commits the index with `message` (hooks run as usual). `amend` replaces HEAD.
pub fn commit(repo: &Path, message: &str, amend: bool) -> Result<(), GitError> {
    commit_with(
        repo,
        message,
        CommitOptions {
            amend,
            ..Default::default()
        },
    )
}

/// How a commit is made (M07 §5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommitOptions {
    /// Replace HEAD instead of adding a commit.
    pub amend: bool,
    /// Add a `Signed-off-by:` trailer with the committer (`--signoff`).
    pub signoff: bool,
    /// Skip the pre-commit and commit-msg hooks (`--no-verify`).
    pub no_verify: bool,
}

pub fn commit_with(repo: &Path, message: &str, opts: CommitOptions) -> Result<(), GitError> {
    let mut args = vec!["commit", "--cleanup=strip", "-F", "-"];
    for (on, flag) in [
        (opts.amend, "--amend"),
        (opts.signoff, "--signoff"),
        (opts.no_verify, "--no-verify"),
    ] {
        if on {
            args.push(flag);
        }
    }
    run_with_input(repo, &args, Some(message.as_bytes())).map(drop)
}

/// A hook runs on commit here, so a refused commit may be the hook's doing.
pub fn has_commit_hooks(repo: &Path) -> bool {
    // `--git-path` follows core.hooksPath and worktrees
    let Ok(out) = run(repo, &["rev-parse", "--git-path", "hooks"]) else {
        return false;
    };
    let hooks = repo.join(String::from_utf8_lossy(&out).trim());
    ["pre-commit", "prepare-commit-msg", "commit-msg"]
        .iter()
        .any(|h| hooks.join(h).is_file())
}

/// The commit message template (`commit.template`) without its `#` comment lines, if one is set.
pub fn commit_template(repo: &Path) -> Option<String> {
    let path = config::get(repo, "commit.template")?;
    let path = match path.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var_os("HOME")?).join(rest),
        None => repo.join(path),
    };
    let text = std::fs::read_to_string(path).ok()?;
    let kept: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    let text = kept.join("\n").trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// HEAD is already on a remote branch: amending it rewrites history others may have.
pub fn head_is_pushed(repo: &Path) -> bool {
    run(repo, &["branch", "--remotes", "--contains", "HEAD"])
        .is_ok_and(|out| !out.trim_ascii().is_empty())
}

/// Full message of HEAD (prefill for amend), or `None` before the first commit.
pub fn head_message(repo: &Path) -> Option<String> {
    commit_message(repo, "HEAD")
}

/// The files `rev` changed, with their status letter (`A`, `M`, `D`, `R`…); a rename names the
/// new path. A root commit lists everything it added.
pub fn commit_files(repo: &Path, rev: &str) -> Result<Vec<(char, String)>, GitError> {
    let out = run(
        repo,
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-status",
            "-r",
            "-z",
            "--root",
            rev,
            "--",
        ],
    )?;
    Ok(name_status(&String::from_utf8_lossy(&out)))
}

/// The files changed from `from` to `to`, as [`commit_files`] lists them.
pub fn files_between(repo: &Path, from: &str, to: &str) -> Result<Vec<(char, String)>, GitError> {
    let out = run(
        repo,
        &[
            "diff-tree",
            "-r",
            "--name-status",
            "-z",
            "--find-renames",
            from,
            to,
            "--",
        ],
    )?;
    Ok(name_status(&String::from_utf8_lossy(&out)))
}

/// A merge's changes as IntelliJ's log shows them: the files it changed from every parent (how
/// it resolved conflicts), and each parent with the files changed from it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeChanges {
    pub resolved: Vec<String>,
    pub parents: Vec<(String, Vec<(char, String)>)>,
}

/// `merge`'s [`MergeChanges`] against `parents`.
pub fn merge_changes(
    repo: &Path,
    merge: &str,
    parents: &[String],
) -> Result<MergeChanges, GitError> {
    // -c lists the paths changed from all parents
    let out = run(
        repo,
        &[
            "diff-tree",
            "-r",
            "-c",
            "--no-commit-id",
            "--name-only",
            "-z",
            merge,
        ],
    )?;
    let resolved = String::from_utf8_lossy(&out)
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect();
    let parents = parents
        .iter()
        .map(|parent| {
            let out = run(
                repo,
                &[
                    "diff-tree",
                    "-r",
                    "--no-commit-id",
                    "--name-status",
                    "-z",
                    "--find-renames",
                    parent,
                    merge,
                    "--",
                ],
            )?;
            Ok((parent.clone(), name_status(&String::from_utf8_lossy(&out))))
        })
        .collect::<Result<_, GitError>>()?;
    Ok(MergeChanges { resolved, parents })
}

/// `git … --name-status -z` output: a status, then one path (two for renames and copies).
fn name_status(out: &str) -> Vec<(char, String)> {
    let mut fields = out.split('\0').filter(|f| !f.is_empty());
    let mut files = Vec::new();
    while let Some(status) = fields.next() {
        let Some(letter) = status.chars().next() else {
            continue;
        };
        let renamed = matches!(letter, 'R' | 'C');
        let path = match renamed {
            true => fields.nth(1),
            false => fields.next(),
        };
        if let Some(path) = path {
            files.push((letter, path.to_owned()));
        }
    }
    files
}

/// The full message of `rev`: subject, blank line, body.
pub fn commit_message(repo: &Path, rev: &str) -> Option<String> {
    let out = run(repo, &["log", "-1", "--format=%B", rev, "--"]).ok()?;
    Some(String::from_utf8_lossy(&out).trim_end().to_owned())
}

/// Patch for one changed file: staged (HEAD→index) or unstaged (index→work tree; untracked shown as added).
pub fn file_diff(repo: &Path, entry: &StatusEntry, staged: bool) -> Result<String, GitError> {
    let base = ["diff", "--no-color", "--no-ext-diff", "--find-renames"];
    let out = if staged {
        let mut paths = vec![literal(&entry.path)];
        paths.extend(entry.orig_path.as_deref().map(literal));
        let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
        run(repo, &[&base[..], &["--cached", "--"], &paths].concat())?
    } else if entry.untracked() {
        run_allowing_diff(
            repo,
            &[&base[..], &["--no-index", "--", NULL_DEVICE, &entry.path]].concat(),
        )?
    } else {
        run(repo, &[&base[..], &["--", &literal(&entry.path)]].concat())?
    };
    Ok(String::from_utf8_lossy(&out).into_owned())
}

#[cfg(windows)]
const NULL_DEVICE: &str = "NUL";
#[cfg(not(windows))]
const NULL_DEVICE: &str = "/dev/null";

/// For `diff --no-index`, exit code 1 means "files differ" and stdout holds the patch.
fn run_allowing_diff(repo: &Path, args: &[&str]) -> Result<Vec<u8>, GitError> {
    let out = Command::new(git_binary())
        .args(["-c", "core.quotepath=false", "-c", "color.ui=false"])
        .args(args)
        .current_dir(repo)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| GitError::GitMissing(e.to_string()))?;
    match out.status.code() {
        Some(0 | 1) => Ok(out.stdout),
        code => Err(classify(code, &String::from_utf8_lossy(&out.stderr))),
    }
}

/// The files that merging `other` into HEAD would leave in conflict, worked out without touching
/// the work tree (`git merge-tree`, git 2.38+); empty when it merges cleanly, `None` when git can't
/// tell (older git, unrelated histories). Rebasing onto `other` meets the same conflicts, give or
/// take the order of the commits.
pub fn predict_conflicts(repo: &Path, other: &str) -> Option<Vec<String>> {
    let out = run_allowing_diff(
        repo,
        &[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            "HEAD",
            other,
        ],
    )
    .ok()?;
    let out = String::from_utf8_lossy(&out);
    let mut lines = out.lines();
    lines.next()?; // the merged tree
    Some(lines.filter(|l| !l.is_empty()).map(str::to_owned).collect())
}

/// Fetches every remote, pruning remote branches deleted upstream (docs/specs/M09 §1).
pub fn fetch(repo: &Path) -> Result<(), GitError> {
    run(repo, &["fetch", "--all", "--prune"]).map(drop)
}

/// `git <command> <flags…> -- <revisions…>` for the Git menu's Pull, Merge and Rebase dialogs,
/// whose Modify options pick the flags; conflicts pause for resolving. What the user typed comes
/// after `--end-of-options`, so a name starting with `-` can't pass for an option.
pub fn run_command(
    repo: &Path,
    command: &str,
    flags: &[&str],
    revisions: &[&str],
) -> Result<Outcome, GitError> {
    let mut args = vec![command];
    match command {
        // local changes are put aside and back, as the reference app saves them around a rebase
        "rebase" => args.push("--autostash"),
        // pull and merge would otherwise open an editor for the merge commit's message
        _ => args.push("--no-edit"),
    }
    args.extend(flags);
    args.push("--end-of-options");
    args.extend(revisions);
    allow_conflicts(repo, run(repo, &args))
}

/// A clone made with `--depth`, whose history is cut short.
pub fn is_shallow(repo: &Path) -> bool {
    run(repo, &["rev-parse", "--is-shallow-repository"])
        .is_ok_and(|out| out.trim_ascii() == b"true")
}

/// Fetches the history a shallow clone left out, making it a complete repository.
pub fn unshallow(repo: &Path) -> Result<(), GitError> {
    run(repo, &["fetch", "--unshallow"]).map(drop)
}

/// Pulls the current branch's upstream; merge vs rebase follows the user's git config. Never opens an editor.
pub fn pull(repo: &Path) -> Result<(), GitError> {
    run(repo, &["pull", "--no-edit"]).map(drop)
}

/// How the Pull dialog pulls (M09 §2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PullOptions {
    /// Rebase local commits onto the pulled ones instead of merging.
    pub rebase: bool,
    /// Stash local changes first and put them back after (`--autostash`).
    pub autostash: bool,
}

/// Pulls `branch` from `remote` into the current branch; conflicts pause for resolving.
pub fn pull_from(
    repo: &Path,
    remote: &str,
    branch: &str,
    opts: PullOptions,
) -> Result<Outcome, GitError> {
    let args = [
        "pull",
        "--no-edit",
        if opts.rebase {
            "--rebase"
        } else {
            "--no-rebase"
        },
        if opts.autostash {
            "--autostash"
        } else {
            "--no-autostash"
        },
        remote,
        branch,
    ];
    allow_conflicts(repo, run(repo, &args))
}

/// Pushes the current branch. The first push sets the upstream on `origin` (or the only remote).
/// Returns a short summary for the notification.
pub fn push(repo: &Path) -> Result<String, GitError> {
    let fail = |msg: &str| GitError::Other {
        code: None,
        stderr: msg.to_owned(),
    };
    let branch = head_branch(repo)?
        .ok_or_else(|| fail("HEAD is detached: check out a branch to push it."))?;
    if run(
        repo,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
    .is_ok()
    {
        run(repo, &["push"])?;
        return Ok(format!("Pushed {branch}"));
    }
    let remotes = String::from_utf8_lossy(&run(repo, &["remote"])?).into_owned();
    let remotes: Vec<&str> = remotes.lines().collect();
    let remote = match remotes[..] {
        _ if remotes.contains(&"origin") => "origin",
        [only] => only,
        [] => return Err(fail("This repository has no remote to push to.")),
        _ => {
            return Err(fail(
                "Several remotes and none is `origin`: set an upstream for this branch first.",
            ));
        }
    };
    let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
    run(repo, &["push", "--set-upstream", remote, &refspec])?;
    Ok(format!("Pushed {branch} to {remote}"))
}

/// Everything staged for the next commit, as one patch (input of the AI commit message).
pub fn staged_diff(repo: &Path) -> Result<String, GitError> {
    let args = [
        "diff",
        "--cached",
        "--no-color",
        "--no-ext-diff",
        "--find-renames",
    ];
    Ok(String::from_utf8_lossy(&run(repo, &args)?).into_owned())
}

/// The local changes of `entries` against HEAD, untracked files as new ones, as one patch that
/// `git apply` takes (IntelliJ's Copy as Patch to Clipboard).
pub fn local_patch(repo: &Path, entries: &[StatusEntry]) -> Result<String, GitError> {
    local_patch_for(repo, entries, false, "")
}

/// [`local_patch`], `reverse`d (undoing the changes) and its paths under `prefix` (`sub/dir/`,
/// for a patch based above the repository): IntelliJ's Patch File Settings.
pub fn local_patch_for(
    repo: &Path,
    entries: &[StatusEntry],
    reverse: bool,
    prefix: &str,
) -> Result<String, GitError> {
    // -R swaps the sides, prefixes too: given swapped, they read a/ and b/ again
    let (old, new) = if reverse { ("b", "a") } else { ("a", "b") };
    let (source, destination) = (
        format!("--src-prefix={old}/{prefix}"),
        format!("--dst-prefix={new}/{prefix}"),
    );
    let mut base = vec![
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--binary",
        "--find-renames",
        &source,
        &destination,
    ];
    if reverse {
        base.push("-R");
    }
    let (untracked, tracked): (Vec<&StatusEntry>, Vec<&StatusEntry>) =
        entries.iter().partition(|e| e.untracked());
    let mut patch = Vec::new();
    if !tracked.is_empty() {
        let paths: Vec<String> = tracked
            .iter()
            .flat_map(|e| std::iter::once(&e.path).chain(e.orig_path.as_ref()))
            .map(|path| literal(path))
            .collect();
        let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
        patch.extend(run(repo, &[&base[..], &["HEAD", "--"], &paths].concat())?);
    }
    for entry in untracked {
        patch.extend(run_allowing_diff(
            repo,
            &[&base[..], &["--no-index", "--", NULL_DEVICE, &entry.path]].concat(),
        )?);
    }
    Ok(String::from_utf8_lossy(&patch).into_owned())
}

/// Moves `paths` (relative to the repository) to the Trash: IntelliJ's Delete in the changes view.
pub fn trash_files(repo: &Path, paths: &[String]) -> Result<(), GitError> {
    let files: Vec<PathBuf> = paths.iter().map(|path| repo.join(path)).collect();
    to_trash(&files)
}

/// Overwrites the current branch's upstream with it, but only if the remote branch is still where
/// our last fetch saw it (`--force-with-lease`), so nobody else's new commits are lost (M09 §3).
pub fn push_force_with_lease(repo: &Path) -> Result<String, GitError> {
    let branch = head_branch(repo)?.ok_or_else(|| GitError::Other {
        code: None,
        stderr: "HEAD is detached: check out a branch to push it.".into(),
    })?;
    run(repo, &["push", "--force-with-lease"])?;
    Ok(format!("Force-pushed {branch}"))
}

/// Switches to a local branch (docs/specs/M04 §2). Git refuses if local changes would be overwritten.
pub fn checkout(repo: &Path, branch: &str) -> Result<(), GitError> {
    run(repo, &["switch", "--no-guess", branch]).map(drop)
}

/// Checks out a remote branch such as `origin/feature/x`: switches to the local branch of the same
/// name if it exists, otherwise creates it tracking the remote one (M04 §3). Returns the local name.
pub fn checkout_remote(repo: &Path, remote_branch: &str) -> Result<String, GitError> {
    let local = remote_branch
        .split_once('/')
        .map_or(remote_branch, |(_, b)| b)
        .to_owned();
    let exists = run(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{local}"),
        ],
    )
    .is_ok();
    if exists {
        checkout(repo, &local)?;
    } else {
        run(repo, &["switch", "--track", remote_branch])?;
    }
    Ok(local)
}

/// Creates `name` at `start` (default HEAD), optionally switching to it (M04 §1).
pub fn create_branch(
    repo: &Path,
    name: &str,
    start: Option<&str>,
    switch: bool,
) -> Result<(), GitError> {
    if run(repo, &["check-ref-format", "--branch", name]).is_err() {
        return Err(GitError::Other {
            code: None,
            stderr: format!("`{name}` is not a valid branch name."),
        });
    }
    let mut args = if switch {
        vec!["switch", "-c", name]
    } else {
        vec!["branch", name]
    };
    args.extend(start);
    run(repo, &args).map(drop)
}

/// Deletes a local branch; git refuses if it isn't merged anywhere (no data loss).
pub fn delete_branch(repo: &Path, name: &str) -> Result<(), GitError> {
    run(repo, &["branch", "--delete", name]).map(drop)
}

/// Password / passphrase prompts from git and ssh, answered by Spork's UI (docs/specs/M10).
///
/// Git runs the Spork binary itself as `GIT_ASKPASS` / `SSH_ASKPASS` (with `SPORK_ASKPASS=1`).
/// That helper sends the prompt to the running app over a localhost connection guarded by a
/// random per-launch token, and prints the answer for git. Standard library only, same on
/// every OS.
pub mod askpass {
    use std::io::{self, BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::OnceLock;
    use std::time::Duration;

    /// Marks a process started by git as the askpass helper.
    pub const HELPER_ENV: &str = "SPORK_ASKPASS";
    const ADDR_ENV: &str = "SPORK_IPC";
    const TOKEN_ENV: &str = "SPORK_IPC_TOKEN";

    #[derive(Debug, Clone)]
    pub struct Config {
        /// Program git runs for prompts (the Spork executable).
        pub program: PathBuf,
        pub addr: String,
        pub token: String,
    }

    static CONFIG: OnceLock<Config> = OnceLock::new();

    /// Routes prompts of every git command started after this call to the app.
    pub fn install(config: Config) {
        CONFIG.set(config).ok();
    }

    pub(crate) fn configure(cmd: &mut Command) {
        let Some(c) = CONFIG.get() else { return };
        cmd.env("GIT_ASKPASS", &c.program)
            .env("SSH_ASKPASS", &c.program)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env(HELPER_ENV, "1")
            .env(ADDR_ENV, &c.addr)
            .env(TOKEN_ENV, &c.token);
        // older OpenSSH only uses SSH_ASKPASS when DISPLAY is set
        #[cfg(target_os = "linux")]
        if std::env::var_os("DISPLAY").is_none() {
            cmd.env("DISPLAY", ":0");
        }
    }

    /// Adds private key `key` to the SSH agent so connections stop asking for its passphrase; on
    /// macOS the passphrase also goes into the Keychain. ssh-add's prompt reaches the app the
    /// same way git's do.
    pub fn add_to_agent(key: &std::path::Path) -> Result<(), crate::GitError> {
        let mut cmd = Command::new("ssh-add");
        if cfg!(target_os = "macos") {
            cmd.arg("--apple-use-keychain");
        }
        cmd.arg(key).stdin(std::process::Stdio::null());
        configure(&mut cmd);
        let out = cmd.output().map_err(|e| crate::GitError::Other {
            code: None,
            stderr: format!("ssh-add: {e}"),
        })?;
        if out.status.success() {
            return Ok(());
        }
        Err(crate::GitError::Other {
            code: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        })
    }

    /// 128 random bits as hex, from the OS-seeded hasher keys (no extra dependency).
    pub fn random_token() -> String {
        use std::hash::{BuildHasher, Hasher};
        (0..2u64)
            .map(|i| {
                let mut h = std::collections::hash_map::RandomState::new().build_hasher();
                h.write_u64(i);
                format!("{:016x}", h.finish())
            })
            .collect()
    }

    fn escape(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('\n', "\\n")
            .replace('\t', "\\t")
    }

    fn unescape(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            match (c, c == '\\') {
                (_, true) => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(other) => out.push(other),
                    None => {}
                },
                (c, false) => out.push(c),
            }
        }
        out
    }

    /// App side: answers prompts arriving on `listener`; `ask` returns the answer, `None` cancels.
    /// Connections without the right token are dropped.
    pub fn serve(listener: TcpListener, token: &str, ask: impl Fn(String) -> Option<String>) {
        for stream in listener.incoming().flatten() {
            handle(stream, token, &ask).ok();
        }
    }

    fn handle(
        stream: TcpStream,
        token: &str,
        ask: &impl Fn(String) -> Option<String>,
    ) -> io::Result<()> {
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line)?;
        let Some((sent, prompt)) = line.trim_end_matches('\n').split_once('\t') else {
            return Ok(());
        };
        if sent != token {
            return Ok(());
        }
        let reply = match ask(unescape(prompt)) {
            Some(answer) => format!("OK\t{}\n", escape(&answer)),
            None => "CANCEL\n".to_owned(),
        };
        (&stream).write_all(reply.as_bytes())
    }

    /// Helper side (run by git): asks the app. `None` when cancelled or the app is unreachable.
    pub fn request(addr: &str, token: &str, prompt: &str) -> Option<String> {
        let mut stream = TcpStream::connect(addr).ok()?;
        stream
            .write_all(format!("{token}\t{}\n", escape(prompt)).as_bytes())
            .ok()?;
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).ok()?;
        line.trim_end_matches('\n')
            .strip_prefix("OK\t")
            .map(unescape)
    }

    /// Entry point when git runs Spork as askpass: prints the answer, exit code 1 on cancel.
    pub fn helper_main() -> i32 {
        let prompt = std::env::args().nth(1).unwrap_or_default();
        let (Ok(addr), Ok(token)) = (std::env::var(ADDR_ENV), std::env::var(TOKEN_ENV)) else {
            return 1;
        };
        match request(&addr, &token, &prompt) {
            Some(answer) => {
                println!("{answer}");
                0
            }
            None => 1,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn prompt_round_trip_cancel_and_wrong_token() {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap().to_string();
            let token = random_token();
            assert_eq!(token.len(), 32);
            let server_token = token.clone();
            std::thread::spawn(move || {
                serve(listener, &server_token, |prompt| match prompt.as_str() {
                    "Password for 'https://u@host':" => Some("s3cr\tet\\x".into()),
                    _ => None,
                })
            });
            assert_eq!(
                request(&addr, &token, "Password for 'https://u@host':").as_deref(),
                Some("s3cr\tet\\x")
            );
            assert_eq!(request(&addr, &token, "Username for 'https://host':"), None); // cancelled
            assert_eq!(
                request(&addr, "wrong-token", "Password for 'https://u@host':"),
                None
            );
            assert_ne!(random_token(), token);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_merge_s_changes_to_each_parent() {
        let tmp = crate::testutil::repo();
        let d = tmp.path();
        std::fs::write(d.join("a"), "a\n").unwrap();
        run(d, &["add", "."]).unwrap();
        run(d, &["commit", "-m", "base"]).unwrap();
        run(d, &["switch", "-c", "topic"]).unwrap();
        std::fs::write(d.join("t"), "t\n").unwrap();
        run(d, &["add", "."]).unwrap();
        run(d, &["commit", "-m", "topic"]).unwrap();
        run(d, &["switch", "main"]).unwrap();
        std::fs::write(d.join("m"), "m\n").unwrap();
        run(d, &["add", "."]).unwrap();
        run(d, &["commit", "-m", "main"]).unwrap();
        run(d, &["merge", "--no-ff", "-q", "-m", "merge", "topic"]).unwrap();
        let parents: Vec<String> = ["HEAD^1", "HEAD^2"]
            .iter()
            .map(|r| {
                String::from_utf8(run(d, &["rev-parse", r]).unwrap())
                    .unwrap()
                    .trim()
                    .to_owned()
            })
            .collect();
        let changes = merge_changes(d, "HEAD", &parents).unwrap();
        // a clean merge resolved nothing; each parent lacks the other side's file
        assert!(changes.resolved.is_empty());
        assert_eq!(changes.parents[0].1, [('A', "t".to_owned())]);
        assert_eq!(changes.parents[1].1, [('A', "m".to_owned())]);
    }

    #[test]
    fn drops_and_extracts_selected_changes() {
        let tmp = crate::testutil::repo();
        let d = tmp.path();
        let write = |f: &str, s: &str| std::fs::write(d.join(f), s).unwrap();
        let subjects = || {
            crate::log(d, 10)
                .unwrap()
                .into_iter()
                .map(|c| c.subject)
                .collect::<Vec<_>>()
        };
        write("a", "a\n");
        run(d, &["add", "."]).unwrap();
        run(d, &["commit", "-m", "base"]).unwrap();
        write("a", "a2\n");
        write("b", "b\n");
        run(d, &["add", "."]).unwrap();
        run(d, &["commit", "-m", "both"]).unwrap();
        write("c", "c\n");
        run(d, &["add", "."]).unwrap();
        run(d, &["commit", "-m", "after"]).unwrap();
        let both = String::from_utf8(run(d, &["rev-parse", "HEAD~1"]).unwrap()).unwrap();
        let files = |rev: &str| {
            commit_files(d, rev)
                .unwrap()
                .into_iter()
                .map(|(_, p)| p)
                .collect::<Vec<_>>()
        };
        // all of a commit's changes can't go
        assert!(drop_changes(d, both.trim(), &["a".into(), "b".into()]).is_err());
        extract_changes(d, both.trim(), &["b".into()], "add b").unwrap();
        assert_eq!(subjects(), ["after", "add b", "both", "base"]);
        assert_eq!(files("HEAD~2"), ["a"]);
        assert_eq!(files("HEAD~1"), ["b"]);
        // a changed file and a new one in the middle of the branch: the new one dropped
        write("a", "a3\n");
        write("e", "e\n");
        run(d, &["add", "."]).unwrap();
        run(d, &["commit", "-m", "two"]).unwrap();
        write("c", "c2\n");
        run(d, &["commit", "-am", "last"]).unwrap();
        drop_changes(d, "HEAD~1", &["e".into()]).unwrap();
        assert_eq!(subjects()[..2], ["last", "two"]);
        assert_eq!(files("HEAD~1"), ["a"]);
        assert!(!d.join("e").exists());
        assert_eq!(std::fs::read_to_string(d.join("c")).unwrap(), "c2\n");
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args([
                "-c",
                "user.name=Spork Test",
                "-c",
                "user.email=test@spork.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?} failed");
    }

    #[test]
    fn parses_track_and_skips_remote_head() {
        let out = "refs/heads/main\0aaa\0origin/main\0ahead 2, behind 1\0*\n\
                   refs/heads/old\0bbb\0origin/old\0gone\0 \n\
                   refs/remotes/origin/HEAD\0aaa\0\0\0 \n\
                   refs/remotes/origin/main\0ccc\0\0\0 \n\
                   refs/tags/v1.0\0ddd\0\0\0 \n";
        let r = parse_refs(out);
        assert_eq!(r.len(), 4);
        assert_eq!(
            (r[0].name.as_str(), r[0].ahead, r[0].behind, r[0].is_head),
            ("main", 2, 1, true)
        );
        assert_eq!(r[0].upstream.as_deref(), Some("origin/main"));
        assert!(r[1].upstream_gone && !r[1].is_head);
        assert_eq!(
            (r[2].kind, r[2].name.as_str()),
            (RefKind::Remote, "origin/main")
        );
        assert_eq!(
            (r[3].kind, r[3].name.as_str(), r[3].upstream.as_deref()),
            (RefKind::Tag, "v1.0", None)
        );
    }

    #[test]
    fn classifies_common_failures() {
        let c = |s| classify(Some(128), s);
        assert_eq!(
            c("fatal: not a git repository (or any of the parent directories): .git"),
            GitError::NotARepo
        );
        assert_eq!(
            c("fatal: Unable to create '/r/.git/index.lock': File exists."),
            GitError::IndexLocked
        );
        assert_eq!(
            c("fatal: Authentication failed for 'https://x/'"),
            GitError::AuthFailed
        );
        assert_eq!(
            c(" ! [rejected]        main -> main (non-fast-forward)"),
            GitError::NonFastForward
        );
        assert!(matches!(c("boom"), GitError::Other { .. }));
    }

    #[test]
    fn reads_a_real_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        git(d, &["init", "-q", "-b", "main"]);
        assert_eq!(head_branch(d).unwrap().as_deref(), Some("main")); // unborn branch
        assert_eq!(log(d, 100).unwrap(), vec![]); // no commits yet is not an error
        git(d, &["commit", "-q", "--allow-empty", "-m", "one"]);
        git(d, &["branch", "feature/x"]);
        git(d, &["tag", "v1"]);
        git(d, &["tag", "-a", "v1-annotated", "-m", "release"]);
        // fake an upstream one commit behind, without any network (the remote URL is never contacted)
        git(
            d,
            &["remote", "add", "origin", "https://example.invalid/r.git"],
        );
        git(d, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        git(
            d,
            &["branch", "-q", "--set-upstream-to=origin/main", "main"],
        );
        git(d, &["commit", "-q", "--allow-empty", "-m", "two"]);

        let r = refs(d).unwrap();
        let main = r.iter().find(|r| r.full == "refs/heads/main").unwrap();
        assert!(main.is_head);
        assert_eq!((main.ahead, main.behind), (1, 0));
        assert!(
            r.iter()
                .any(|r| r.kind == RefKind::Local && r.name == "feature/x")
        );
        assert!(
            r.iter()
                .any(|r| r.kind == RefKind::Remote && r.name == "origin/main")
        );
        assert!(r.iter().any(|r| r.kind == RefKind::Tag && r.name == "v1"));

        let annotated = r.iter().find(|r| r.name == "v1-annotated").unwrap();
        assert_ne!(annotated.target, annotated.commit); // tag object vs the commit it points at
        assert_eq!(
            annotated.commit,
            r.iter().find(|r| r.name == "v1").unwrap().commit
        );

        git(d, &["switch", "-q", "feature/x"]);
        git(d, &["commit", "-q", "--allow-empty", "-m", "feature work"]);
        git(d, &["switch", "-q", "main"]);
        git(
            d,
            &["merge", "-q", "--no-ff", "-m", "merge feature", "feature/x"],
        );
        let commits = log(d, 100).unwrap();
        assert_eq!(commits[0].subject, "merge feature");
        assert_eq!(commits[0].parents.len(), 2);
        assert_eq!(commits[0].author, "Spork Test");
        assert_eq!(log(d, 2).unwrap().len(), 2);

        // details: root commit diffs against the empty tree, merges against their first parent
        let root = commits.last().unwrap();
        assert!(root.parents.is_empty());
        let details = commit_details(d, &root.id, None, false).unwrap();
        assert_eq!(details.message, "one");
        assert_eq!(details.committer, "Spork Test");
        assert_eq!(details.author_date.len(), "2026-01-01 00:00".len());

        std::fs::write(d.join("hello.txt"), "hi\n").unwrap();
        git(d, &["add", "hello.txt"]);
        git(d, &["commit", "-q", "-m", "add hello", "-m", "with a body"]);
        let c = &log(d, 1).unwrap()[0];
        let details =
            commit_details(d, &c.id, c.parents.first().map(String::as_str), false).unwrap();
        assert_eq!(details.message, "add hello\n\nwith a body");
        assert!(details.patch.contains("+++ b/hello.txt"));
        assert!(details.patch.contains("+hi"));

        git(d, &["switch", "-q", "--detach", "HEAD"]);
        assert_eq!(head_branch(d).unwrap(), None);
        std::fs::create_dir(d.join("sub")).unwrap();
        let top = toplevel(&d.join("sub")).unwrap();
        assert_eq!(top.canonicalize().unwrap(), d.canonicalize().unwrap());
    }

    #[test]
    fn lists_ignored_folders_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        git(d, &["init", "-q", "-b", "main"]);
        std::fs::write(d.join(".gitignore"), "target/\n*.log\n").unwrap();
        std::fs::create_dir_all(d.join("target/debug")).unwrap();
        std::fs::write(d.join("target/debug/a.o"), "x").unwrap();
        std::fs::write(d.join("x.log"), "x").unwrap();
        std::fs::write(d.join("kept.rs"), "x").unwrap();
        assert_eq!(ignored_paths(d), ["target/", "x.log"]);
    }

    #[test]
    fn local_patches_reversed_and_prefixed() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        git(d, &["init", "-q", "-b", "main"]);
        git(d, &["config", "user.name", "Spork Test"]);
        git(d, &["config", "user.email", "test@spork.invalid"]);
        std::fs::write(d.join("a.txt"), "one\n").unwrap();
        git(d, &["add", "a.txt"]);
        git(d, &["commit", "-q", "-m", "a"]);
        std::fs::write(d.join("a.txt"), "two\n").unwrap();
        std::fs::write(d.join("new.txt"), "new\n").unwrap();
        let entries = status(d).unwrap();
        let patch = local_patch(d, &entries).unwrap();
        assert!(patch.contains("--- a/a.txt") && patch.contains("+two") && patch.contains("+new"));
        let reversed = local_patch_for(d, &entries, true, "repo/").unwrap();
        assert!(reversed.contains("--- a/repo/a.txt") && reversed.contains("+++ b/repo/a.txt"));
        assert!(
            reversed.contains("-two") && reversed.contains("+one") && reversed.contains("-new")
        );
    }

    #[test]
    fn ignored_files_listed_and_force_staged() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        git(d, &["init", "-q", "-b", "main"]);
        std::fs::write(d.join(".gitignore"), "*.log\nbuild/\n").unwrap();
        std::fs::write(d.join("app.log"), "x").unwrap();
        std::fs::create_dir(d.join("build")).unwrap();
        std::fs::write(d.join("build/out.o"), "x").unwrap();
        assert!(status(d).unwrap().iter().all(|e| !e.ignored()));
        let all = status_with(d, true).unwrap();
        let ignored: Vec<&str> = all
            .iter()
            .filter(|e| e.ignored())
            .map(|e| e.path.as_str())
            .collect();
        assert_eq!(ignored, ["app.log", "build/"]);
        assert!(all.iter().all(|e| !e.has_staged()));
        assert!(stage(d, &["app.log"]).is_err()); // git refuses ignored paths without --force
        stage_ignored(d, &["app.log"]).unwrap();
        let staged = status(d).unwrap();
        assert!(staged.iter().any(|e| e.path == "app.log" && e.has_staged()));
    }

    #[test]
    fn working_copy_status_stage_unstage_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        let write = |p: &str, c: &str| std::fs::write(d.join(p), c).unwrap();
        git(d, &["init", "-q", "-b", "main"]);
        git(d, &["config", "user.name", "Spork Test"]);
        git(d, &["config", "user.email", "test@spork.invalid"]);
        git(d, &["config", "commit.gpgsign", "false"]);

        // before the first commit: stage + unstage must work on an unborn HEAD
        write("a.txt", "one\n");
        stage(d, &["a.txt"]).unwrap();
        assert!(status(d).unwrap()[0].has_staged());
        unstage(d, &["a.txt"]).unwrap();
        assert!(status(d).unwrap()[0].untracked());
        stage(d, &["a.txt"]).unwrap();
        assert_eq!(head_message(d), None);
        commit(d, "first\n\nbody line", false).unwrap();
        assert_eq!(head_message(d).as_deref(), Some("first\n\nbody line"));

        write("a.txt", "one\ntwo\n"); // modified, unstaged
        write("star*.txt", "x\n"); // untracked, name with a glob char
        write("gone.txt", "bye\n");
        stage(d, &["gone.txt"]).unwrap();
        commit(d, "add gone", false).unwrap();
        std::fs::remove_file(d.join("gone.txt")).unwrap(); // deleted
        git(d, &["mv", "a.txt", "b.txt"]); // staged rename (with an unstaged edit inside)

        let st = status(d).unwrap();
        let find = |p: &str| {
            st.iter()
                .find(|e| e.path == p)
                .unwrap_or_else(|| panic!("{p} in {st:?}"))
        };
        assert!(find("star*.txt").untracked());
        assert_eq!(
            (find("gone.txt").index, find("gone.txt").worktree),
            ('.', 'D')
        );
        let renamed = find("b.txt");
        assert_eq!(
            (renamed.index, renamed.orig_path.as_deref()),
            ('R', Some("a.txt"))
        );

        // literal pathspec: staging `star*.txt` must not touch other files
        stage(d, &["star*.txt", "gone.txt"]).unwrap();
        let st = status(d).unwrap();
        assert!(st.iter().any(|e| e.path == "star*.txt" && e.index == 'A'));
        assert!(st.iter().any(|e| e.path == "gone.txt" && e.index == 'D'));
        unstage(d, &["star*.txt"]).unwrap();
        let st = status(d).unwrap();
        assert!(st.iter().any(|e| e.path == "star*.txt" && e.untracked()));

        // per-file diffs: untracked shows as added, staged rename shows as rename
        let star = st.iter().find(|e| e.path == "star*.txt").unwrap();
        assert!(file_diff(d, star, false).unwrap().contains("+x"));
        let renamed = st.iter().find(|e| e.path == "b.txt").unwrap();
        assert!(
            file_diff(d, renamed, true)
                .unwrap()
                .contains("rename to b.txt")
        );
        assert!(file_diff(d, renamed, false).unwrap().contains("+two"));

        // discard restores a tracked file to its index content (untracked → Trash isn't run in tests)
        write("gone.txt", "edited\n");
        git(d, &["add", "gone.txt"]);
        write("gone.txt", "edited again\n");
        let entry = status(d)
            .unwrap()
            .into_iter()
            .find(|e| e.path == "gone.txt")
            .unwrap();
        discard(d, &[entry]).unwrap();
        assert_eq!(
            std::fs::read_to_string(d.join("gone.txt")).unwrap(),
            "edited\n"
        );
        unstage(d, &["gone.txt"]).unwrap();
        let entry = status(d)
            .unwrap()
            .into_iter()
            .find(|e| e.path == "gone.txt")
            .unwrap();
        discard(d, &[entry]).unwrap();
        // unstaged → the index holds HEAD's version again, so discard restores that
        assert_eq!(
            std::fs::read_to_string(d.join("gone.txt")).unwrap(),
            "bye\n"
        );

        // amend rewrites HEAD's message without adding a commit
        commit(d, "add gone (amended)", true).unwrap();
        assert_eq!(head_message(d).as_deref(), Some("add gone (amended)"));
        assert_eq!(log(d, 10).unwrap().len(), 2);
    }

    #[test]
    fn fetch_pull_push_against_a_bare_remote() {
        let tmp = tempfile::tempdir().unwrap();
        let (server, a, b) = (
            tmp.path().join("server.git"),
            tmp.path().join("a"),
            tmp.path().join("b"),
        );
        git(
            tmp.path(),
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                server.to_str().unwrap(),
            ],
        );
        git(
            tmp.path(),
            &["init", "-q", "-b", "main", a.to_str().unwrap()],
        );
        git(&a, &["commit", "-q", "--allow-empty", "-m", "one"]);

        // no remote yet: a clear error instead of a git usage message
        assert!(push(&a).unwrap_err().to_string().contains("no remote"));
        git(&a, &["remote", "add", "origin", server.to_str().unwrap()]);
        // first push sets the upstream
        assert_eq!(push(&a).unwrap(), "Pushed main to origin");
        assert_eq!(
            refs(&a)
                .unwrap()
                .iter()
                .find(|r| r.name == "main")
                .unwrap()
                .upstream
                .as_deref(),
            Some("origin/main")
        );

        // someone else pushes a commit
        git(
            tmp.path(),
            &["clone", "-q", server.to_str().unwrap(), b.to_str().unwrap()],
        );
        git(&b, &["commit", "-q", "--allow-empty", "-m", "two"]);
        assert_eq!(push(&b).unwrap(), "Pushed main");

        // fetch shows we're behind, pull fast-forwards
        fetch(&a).unwrap();
        assert_eq!(
            refs(&a)
                .unwrap()
                .iter()
                .find(|r| r.name == "main")
                .unwrap()
                .behind,
            1
        );
        pull(&a).unwrap();
        assert_eq!(head_message(&a).as_deref(), Some("two"));

        // diverged: push is rejected with the friendly error
        git(&b, &["commit", "-q", "--allow-empty", "-m", "three"]);
        push(&b).unwrap();
        git(&a, &["commit", "-q", "--allow-empty", "-m", "local"]);
        assert_eq!(push(&a).unwrap_err(), GitError::NonFastForward);
    }

    #[test]
    fn branch_checkout_create_delete() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        git(d, &["init", "-q", "-b", "main"]);
        git(d, &["commit", "-q", "--allow-empty", "-m", "one"]);

        create_branch(d, "feature/x", None, false).unwrap();
        assert_eq!(head_branch(d).unwrap().as_deref(), Some("main"));
        create_branch(d, "feature/y", Some("main"), true).unwrap();
        assert_eq!(head_branch(d).unwrap().as_deref(), Some("feature/y"));
        assert!(
            create_branch(d, "bad..name", None, false)
                .unwrap_err()
                .to_string()
                .contains("not a valid")
        );

        checkout(d, "feature/x").unwrap();
        assert_eq!(head_branch(d).unwrap().as_deref(), Some("feature/x"));

        // unmerged branch: git refuses to delete it
        checkout(d, "feature/y").unwrap();
        git(d, &["commit", "-q", "--allow-empty", "-m", "only on y"]);
        checkout(d, "main").unwrap();
        assert!(delete_branch(d, "feature/y").is_err());
        delete_branch(d, "feature/x").unwrap();
        assert!(!refs(d).unwrap().iter().any(|r| r.name == "feature/x"));

        // remote branch: creates a tracking branch, then reuses it
        git(
            d,
            &["remote", "add", "origin", "https://example.invalid/r.git"],
        );
        git(d, &["update-ref", "refs/remotes/origin/feature/z", "HEAD"]);
        assert_eq!(checkout_remote(d, "origin/feature/z").unwrap(), "feature/z");
        let z = refs(d)
            .unwrap()
            .into_iter()
            .find(|r| r.name == "feature/z")
            .unwrap();
        assert_eq!(
            (z.is_head, z.upstream.as_deref()),
            (true, Some("origin/feature/z"))
        );
        checkout(d, "main").unwrap();
        assert_eq!(checkout_remote(d, "origin/feature/z").unwrap(), "feature/z");
    }

    /// Randomised check of line-level stage / unstage / discard against real git: every
    /// result must equal the file computed independently from a full-context diff.
    #[test]
    fn partial_patches_apply_exactly() {
        use spork_patch::{FilePatch, Kind};

        let mut seed: u64 = 0x5eed;
        let mut rand = move |n: u64| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) % n
        };
        // selection is a function of the line text, so it is identical in -U3 and full diffs
        let pick = |salt: u64, raw: &str| {
            let h = raw
                .bytes()
                .fold(salt, |h, b| h.wrapping_mul(31).wrapping_add(b as u64));
            h % 3 != 0
        };
        // expected file content after keeping `keep(kind, picked)` lines of a full-context diff
        let expected = |full: &str, salt: u64, keep: &dyn Fn(Kind, bool) -> bool| -> String {
            let patch = FilePatch::parse(full).unwrap();
            let lines: Vec<_> = patch.lines().collect();
            let mut out = String::new();
            for (i, l) in lines.iter().enumerate() {
                if l.kind == Kind::NoNewline || !keep(l.kind, pick(salt, &l.raw)) {
                    continue;
                }
                let text = &l.raw[1..];
                // a following "\ No newline" marker means this file line has no line ending
                let no_eol = lines.get(i + 1).is_some_and(|n| n.kind == Kind::NoNewline);
                out.push_str(if no_eol {
                    text.trim_end_matches('\n')
                } else {
                    text
                });
            }
            out
        };
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        git(d, &["init", "-q", "-b", "main"]);
        let read_index =
            |d: &Path| String::from_utf8(run(d, &["show", ":f.txt"]).unwrap()).unwrap();
        let unstaged =
            |d: &Path| String::from_utf8(run(d, &["diff", "--", "f.txt"]).unwrap()).unwrap();
        let staged = |d: &Path| {
            String::from_utf8(run(d, &["diff", "--cached", "--", "f.txt"]).unwrap()).unwrap()
        };
        let full = |d: &Path, cached: bool| {
            let args: &[&str] = if cached {
                &["diff", "--cached", "-U100000", "--", "f.txt"]
            } else {
                &["diff", "-U100000", "--", "f.txt"]
            };
            String::from_utf8(run(d, args).unwrap()).unwrap()
        };

        let mut applied = 0;
        for round in 0..150u64 {
            // random base and random edit of it
            let lines = 5 + rand(40) as usize;
            let mut base: String = (0..lines).map(|i| format!("line {i} r{round}\n")).collect();
            let mut new = String::new();
            for (i, l) in base.lines().enumerate() {
                match rand(10) {
                    0 => {}
                    1 => new.push_str(&format!("line {i} changed r{round}\n")),
                    2 => new.push_str(&format!("{l}\ninserted {i} r{round}\n")),
                    _ => new.push_str(&format!("{l}\n")),
                }
            }
            if rand(4) == 0 {
                new.push_str(&format!("tail r{round}"));
            } // no trailing newline
            if rand(6) == 0 {
                base.pop();
            } // base without trailing newline
            if new == base {
                continue;
            }
            std::fs::write(d.join("f.txt"), &base).unwrap();
            git(d, &["add", "f.txt"]);
            git(d, &["commit", "-q", "--allow-empty", "-m", "base"]);
            std::fs::write(d.join("f.txt"), &new).unwrap();
            let edge_case = !base.ends_with('\n') || !new.ends_with('\n');
            let salt = round;

            // 1. stage a selection: index = base + picked changes
            let patch = FilePatch::parse(&unstaged(d)).unwrap();
            let want = expected(&full(d, false), salt, &|k, p| match k {
                Kind::Removed => !p,
                Kind::Added => p,
                _ => true,
            });
            if let Some(partial) = patch.partial(|_, l| pick(salt, &l.raw), false) {
                let before = read_index(d);
                match apply_patch(d, &partial, PatchTarget::Stage) {
                    Ok(()) => {
                        assert_eq!(read_index(d), want, "stage, round {round}\n{partial}");
                        applied += 1;
                    }
                    Err(e) => {
                        assert!(edge_case, "stage rejected, round {round}: {e}\n{partial}");
                        assert_eq!(read_index(d), before, "rejected patch changed the index");
                    }
                }
            }

            // 2. stage everything, then unstage a selection: index = new minus picked changes
            stage(d, &["f.txt"]).unwrap();
            let patch = FilePatch::parse(&staged(d)).unwrap();
            let want = expected(&full(d, true), salt, &|k, p| match k {
                Kind::Removed => p,
                Kind::Added => !p,
                _ => true,
            });
            if let Some(partial) = patch.partial(|_, l| pick(salt, &l.raw), true) {
                let before = read_index(d);
                match apply_patch(d, &partial, PatchTarget::Unstage) {
                    Ok(()) => assert_eq!(read_index(d), want, "unstage, round {round}\n{partial}"),
                    Err(e) => {
                        assert!(edge_case, "unstage rejected, round {round}: {e}\n{partial}");
                        assert_eq!(read_index(d), before);
                    }
                }
            }

            // 3. reset the index to base, then discard a selection from the work tree
            unstage(d, &["f.txt"]).unwrap();
            let patch = FilePatch::parse(&unstaged(d)).unwrap();
            let want = expected(&full(d, false), salt, &|k, p| match k {
                Kind::Removed => p,
                Kind::Added => !p,
                _ => true,
            });
            if let Some(partial) = patch.partial(|_, l| pick(salt, &l.raw), true) {
                match apply_patch(d, &partial, PatchTarget::Discard) {
                    Ok(()) => assert_eq!(
                        std::fs::read_to_string(d.join("f.txt")).unwrap(),
                        want,
                        "discard, round {round}"
                    ),
                    Err(e) => {
                        assert!(edge_case, "discard rejected, round {round}: {e}\n{partial}");
                        assert_eq!(std::fs::read_to_string(d.join("f.txt")).unwrap(), new);
                    }
                }
            }
        }
        assert!(applied > 100, "too few rounds exercised: {applied}");
    }

    #[test]
    fn merge_rebase_pick_revert_with_conflicts() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        let write = |c: &str| std::fs::write(d.join("f.txt"), c).unwrap();
        let read = || std::fs::read_to_string(d.join("f.txt")).unwrap();
        git(d, &["init", "-q", "-b", "main"]);
        write("base\n");
        git(d, &["add", "f.txt"]);
        git(d, &["commit", "-q", "-m", "base"]);
        git(d, &["switch", "-q", "-c", "feature"]);
        write("feature\n");
        git(d, &["commit", "-q", "-am", "feature edit"]);
        git(d, &["switch", "-q", "main"]);
        write("main\n");
        git(d, &["commit", "-q", "-am", "main edit"]);
        assert_eq!(op_state(d).unwrap(), OpState::None);

        // merge → conflict → abort restores main
        assert_eq!(merge(d, "feature").unwrap(), Outcome::Conflicts);
        assert_eq!(op_state(d).unwrap(), OpState::Merging);
        abort_op(d).unwrap();
        assert_eq!(
            (op_state(d).unwrap(), read()),
            (OpState::None, "main\n".into())
        );

        // merge → conflict → take theirs → continue creates the merge commit
        merge(d, "feature").unwrap();
        resolve_with(d, "f.txt", false).unwrap();
        assert!(!status(d).unwrap().iter().any(|e| e.conflicted));
        assert_eq!(continue_op(d).unwrap(), Outcome::Done);
        assert_eq!(
            (op_state(d).unwrap(), read()),
            (OpState::None, "feature\n".into())
        );
        assert_eq!(log(d, 1).unwrap()[0].parents.len(), 2);

        // rebase with conflicts reports progress; resolve with ours and continue
        git(d, &["switch", "-q", "-c", "topic", "HEAD~1"]); // main before the merge
        write("topic\n");
        git(d, &["commit", "-q", "-am", "topic edit"]);
        assert_eq!(rebase(d, "feature").unwrap(), Outcome::Conflicts);
        assert_eq!(
            op_state(d).unwrap(),
            OpState::Rebasing { step: 1, total: 2 }
        );
        resolve_with(d, "f.txt", true).unwrap(); // during a rebase "ours" = feature
        assert_eq!(continue_op(d).unwrap(), Outcome::Conflicts); // next commit conflicts too
        assert_eq!(skip_op(d).unwrap(), Outcome::Done);
        assert_eq!(op_state(d).unwrap(), OpState::None);

        // cherry-pick cleanly, then revert it
        git(d, &["switch", "-q", "main"]);
        std::fs::write(d.join("g.txt"), "g\n").unwrap();
        git(d, &["add", "g.txt"]);
        git(d, &["commit", "-q", "-m", "add g"]);
        let g = log(d, 1).unwrap()[0].id.clone();
        git(d, &["switch", "-q", "topic"]);
        assert_eq!(cherry_pick(d, &g).unwrap(), Outcome::Done);
        assert!(d.join("g.txt").exists());
        assert_eq!(revert(d, "HEAD").unwrap(), Outcome::Done);
        assert!(!d.join("g.txt").exists());
        // a merge commit is picked against its first parent instead of failing
        git(d, &["switch", "-q", "main"]);
        let merge_commit = log(d, 10)
            .unwrap()
            .into_iter()
            .find(|c| c.parents.len() == 2)
            .unwrap()
            .id;
        git(d, &["switch", "-q", "-c", "picker", "HEAD~3"]);
        assert!(cherry_pick(d, &merge_commit).is_ok());
    }

    #[test]
    fn stash_save_apply_pop_drop() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        git(d, &["init", "-q", "-b", "main"]);
        std::fs::write(d.join("f.txt"), "one\n").unwrap();
        git(d, &["add", "f.txt"]);
        git(d, &["commit", "-q", "-m", "base"]);
        assert!(stashes(d).unwrap().is_empty());

        std::fs::write(d.join("f.txt"), "two\n").unwrap();
        std::fs::write(d.join("new.txt"), "n\n").unwrap();
        stash_save(d, "wip: two", true).unwrap();
        let st = status(d).unwrap();
        assert!(st.is_empty(), "{st:?}"); // untracked file went into the stash too
        std::fs::write(d.join("f.txt"), "three\n").unwrap();
        stash_save(d, "", false).unwrap();
        let list = stashes(d).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].message, "On main: wip: two");
        assert!(list[0].message.starts_with("WIP on main"));

        // apply keeps the stash, pop removes it
        assert_eq!(stash_apply(d, 1, false).unwrap(), Outcome::Done);
        assert_eq!(std::fs::read_to_string(d.join("f.txt")).unwrap(), "two\n");
        assert_eq!(stashes(d).unwrap().len(), 2);
        // uncommitted changes to the same file block applying the other stash; nothing is lost
        assert!(stash_apply(d, 0, true).is_err());
        assert_eq!(stashes(d).unwrap().len(), 2);
        git(d, &["checkout", "--", "f.txt"]);
        std::fs::remove_file(d.join("new.txt")).unwrap();
        assert_eq!(stash_apply(d, 0, true).unwrap(), Outcome::Done);
        assert_eq!(stashes(d).unwrap().len(), 1);
        stash_drop(d, 0).unwrap();
        assert!(stashes(d).unwrap().is_empty());
    }

    #[test]
    fn tags_create_delete_push() {
        let tmp = tempfile::tempdir().unwrap();
        let (server, d) = (tmp.path().join("server.git"), tmp.path().join("repo"));
        git(
            tmp.path(),
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                server.to_str().unwrap(),
            ],
        );
        git(
            tmp.path(),
            &["init", "-q", "-b", "main", d.to_str().unwrap()],
        );
        git(&d, &["commit", "-q", "--allow-empty", "-m", "one"]);
        git(&d, &["commit", "-q", "--allow-empty", "-m", "two"]);

        create_tag(&d, "v1.0", "HEAD~1", "").unwrap();
        create_tag(&d, "v2.0", "HEAD", "Second release").unwrap();
        assert!(
            create_tag(&d, "bad tag", "HEAD", "")
                .unwrap_err()
                .to_string()
                .contains("not a valid")
        );
        let tags: Vec<Ref> = refs(&d)
            .unwrap()
            .into_iter()
            .filter(|r| r.kind == RefKind::Tag)
            .collect();
        let v1 = tags.iter().find(|t| t.name == "v1.0").unwrap();
        let v2 = tags.iter().find(|t| t.name == "v2.0").unwrap();
        assert_eq!(v1.target, v1.commit); // lightweight
        assert_ne!(v2.target, v2.commit); // annotated: a tag object
        assert_eq!(v2.commit, log(&d, 1).unwrap()[0].id);

        assert!(push_tag(&d, "origin", "v2.0").is_err()); // no remote yet
        git(&d, &["remote", "add", "origin", server.to_str().unwrap()]);
        push_tag(&d, "origin", "v2.0").unwrap();
        assert!(
            !String::from_utf8(run(&server, &["tag"]).unwrap())
                .unwrap()
                .contains("v1.0")
        );
        assert!(
            String::from_utf8(run(&server, &["tag"]).unwrap())
                .unwrap()
                .contains("v2.0")
        );

        delete_tag(&d, "v1.0").unwrap();
        assert!(!refs(&d).unwrap().iter().any(|r| r.name == "v1.0"));
    }

    #[test]
    fn signed_tag_with_an_ssh_key() {
        let tmp = tempfile::tempdir().unwrap();
        let (d, key) = (tmp.path().join("repo"), tmp.path().join("key"));
        let made = std::process::Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-f"])
            .arg(&key)
            .status();
        if !made.is_ok_and(|s| s.success()) {
            return; // no ssh-keygen here (some Windows setups): nothing to sign with
        }
        git(
            tmp.path(),
            &["init", "-q", "-b", "main", d.to_str().unwrap()],
        );
        git(&d, &["config", "gpg.format", "ssh"]);
        git(&d, &["config", "user.signingkey", key.to_str().unwrap()]);
        git(&d, &["commit", "-q", "--allow-empty", "-m", "one"]);
        create_tag_with(&d, "v1", "HEAD", "", true).unwrap();
        let object = String::from_utf8(run(&d, &["cat-file", "tag", "v1"]).unwrap()).unwrap();
        assert!(object.contains("BEGIN SSH SIGNATURE"));
        assert!(object.contains("\nv1\n")); // the name stands in for the missing message
    }

    #[test]
    fn predicts_conflicts() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        git(d, &["init", "-q", "-b", "main"]);
        std::fs::write(d.join("f.txt"), "base\n").unwrap();
        git(d, &["add", "."]);
        git(d, &["commit", "-q", "-m", "base"]);
        git(d, &["switch", "-q", "-c", "other"]);
        std::fs::write(d.join("f.txt"), "theirs\n").unwrap();
        std::fs::write(d.join("new.txt"), "x").unwrap();
        git(d, &["add", "."]);
        git(d, &["commit", "-q", "-m", "theirs"]);
        git(d, &["switch", "-q", "-c", "clean", "main"]);
        std::fs::write(d.join("g.txt"), "g").unwrap();
        git(d, &["add", "."]);
        git(d, &["commit", "-q", "-m", "clean"]);
        assert_eq!(predict_conflicts(d, "other"), Some(Vec::new()));
        git(d, &["switch", "-q", "main"]);
        std::fs::write(d.join("f.txt"), "ours\n").unwrap();
        git(d, &["commit", "-q", "-am", "ours"]);
        assert_eq!(
            predict_conflicts(d, "other"),
            Some(vec!["f.txt".to_owned()])
        );
        assert!(status(d).unwrap().is_empty()); // nothing touched
        assert_eq!(predict_conflicts(d, "no-such-branch"), None);
    }

    #[test]
    fn meld_and_drop_without_the_editor() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        git(d, &["init", "-q", "-b", "main"]);
        for name in ["a", "b", "c", "d", "e"] {
            std::fs::write(d.join(format!("{name}.txt")), name).unwrap();
            git(d, &["add", "."]);
            git(d, &["commit", "-q", "-m", name]);
        }
        let subjects = || {
            String::from_utf8(run(d, &["log", "--format=%s", "HEAD"]).unwrap())
                .unwrap()
                .lines()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let id = |rev: &str| {
            String::from_utf8(run(d, &["rev-parse", rev]).unwrap())
                .unwrap()
                .trim()
                .to_owned()
        };
        // fixup d into c: c keeps its message and gains d.txt
        assert_eq!(meld(d, &[id("HEAD~1")], false).unwrap(), Outcome::Done);
        assert_eq!(subjects(), ["e", "c", "b", "a"]);
        assert!(
            String::from_utf8(run(d, &["show", "--name-only", "--format=", "HEAD~1"]).unwrap())
                .unwrap()
                .contains("d.txt")
        );
        // squash a run (b, c) into the oldest: both messages kept
        assert_eq!(
            meld(d, &[id("HEAD~1"), id("HEAD~2")], true).unwrap(),
            Outcome::Done
        );
        assert_eq!(subjects().len(), 3);
        let message =
            String::from_utf8(run(d, &["log", "-1", "--format=%B", "HEAD~1"]).unwrap()).unwrap();
        assert!(message.contains('b') && message.contains('c'));
        // move b (squashed with c) above e, then e back on top; not the first commit
        let order = || subjects().join(",");
        assert_eq!(order(), "e,b,a");
        assert_eq!(
            move_commit(d, &id("HEAD~1"), &id("HEAD")).unwrap(),
            Outcome::Done
        );
        assert_eq!(order(), "b,e,a");
        assert_eq!(
            move_commit(d, &id("HEAD~1"), &id("HEAD")).unwrap(),
            Outcome::Done
        );
        assert_eq!(order(), "e,b,a");
        assert!(move_commit(d, &id("HEAD~2"), &id("HEAD")).is_err());
        // drop e; the first commit can't be melded into a parent it doesn't have
        assert_eq!(drop_commits(d, &[id("HEAD")]).unwrap(), Outcome::Done);
        assert!(!d.join("e.txt").exists());
        assert!(meld(d, &[id("HEAD~1")], false).is_err());
    }

    #[test]
    fn squash_gives_the_squashed_commit_its_message() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        git(d, &["init", "-q", "-b", "main"]);
        for name in ["a", "b", "c", "d"] {
            std::fs::write(d.join(format!("{name}.txt")), name).unwrap();
            git(d, &["add", "."]);
            git(d, &["commit", "-q", "-m", name]);
        }
        let id = |rev: &str| {
            String::from_utf8(run(d, &["rev-parse", rev]).unwrap())
                .unwrap()
                .trim()
                .to_owned()
        };
        let log = || String::from_utf8(run(d, &["log", "--format=%s", "HEAD"]).unwrap()).unwrap();
        // b and c, with d above them
        let squashed = squash(d, &[id("HEAD~1"), id("HEAD~2")], "b and c").unwrap();
        assert_eq!(squashed, Outcome::Done);
        assert_eq!(log(), "d\nb and c\na\n");
        // the newest two: HEAD itself is reworded
        assert_eq!(
            squash(d, &[id("HEAD"), id("HEAD~1")], "all but a").unwrap(),
            Outcome::Done
        );
        assert_eq!(log(), "all but a\na\n");
    }

    #[test]
    fn interactive_rebase_rewords_after_the_melded_commits() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        git(d, &["init", "-q", "-b", "main"]);
        for name in ["base", "a", "b", "c"] {
            std::fs::write(d.join(format!("{name}.txt")), name).unwrap();
            git(d, &["add", "."]);
            git(d, &["commit", "-q", "-m", name]);
        }
        let range = rebase_range(d, "HEAD~3").unwrap();
        let id = |s: &str| range.iter().find(|c| c.subject == s).unwrap().id.clone();
        let plan = vec![
            (id("a"), TodoAction::Pick),
            (id("b"), TodoAction::Fixup),
            (id("c"), TodoAction::Pick),
        ];
        let messages = [
            (id("a"), "a and b\n\nwith a body".to_owned()),
            (id("c"), "c's new message".to_owned()),
        ];
        assert_eq!(
            interactive_rebase_reworded(d, "HEAD~3", &plan, &messages).unwrap(),
            Outcome::Done
        );
        let messages = String::from_utf8(run(d, &["log", "--format=%B%x00"]).unwrap()).unwrap();
        let messages: Vec<&str> = messages.split('\0').map(str::trim).collect();
        assert_eq!(
            messages,
            ["c's new message", "a and b\n\nwith a body", "base", ""]
        );
    }

    #[test]
    fn interactive_rebase_reorders_squashes_drops() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        git(d, &["init", "-q", "-b", "main"]);
        for (i, name) in ["base", "a", "b", "c", "d"].iter().enumerate() {
            std::fs::write(d.join(format!("{name}.txt")), format!("{i}\n")).unwrap();
            git(d, &["add", "."]);
            git(d, &["commit", "-q", "-m", name]);
        }
        let subjects = |d: &Path| {
            log(d, 20)
                .unwrap()
                .into_iter()
                .map(|c| c.subject)
                .collect::<Vec<_>>()
        };
        let range = rebase_range(d, "HEAD~4").unwrap();
        assert_eq!(
            range.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(),
            ["a", "b", "c", "d"]
        );
        let id = |s: &str| range.iter().find(|c| c.subject == s).unwrap().id.clone();

        // reorder d first, squash b into it... keep a, fixup c into a, drop nothing
        let plan = vec![
            (id("d"), TodoAction::Pick),
            (id("b"), TodoAction::Squash),
            (id("a"), TodoAction::Pick),
            (id("c"), TodoAction::Fixup),
        ];
        assert_eq!(
            interactive_rebase(d, "HEAD~4", &plan).unwrap(),
            Outcome::Done
        );
        assert_eq!(subjects(d), ["a", "d", "base"]);
        let squashed = &log(d, 20).unwrap()[1];
        assert!(
            commit_details(
                d,
                &squashed.id,
                squashed.parents.first().map(String::as_str),
                false
            )
            .unwrap()
            .message
            .contains("b")
        );
        for f in ["a", "b", "c", "d"] {
            assert!(d.join(format!("{f}.txt")).exists(), "{f}.txt kept");
        }

        // drop removes the commit and its file
        let range = rebase_range(d, "HEAD~2").unwrap();
        let plan = vec![
            (range[0].id.clone(), TodoAction::Drop),
            (range[1].id.clone(), TodoAction::Pick),
        ];
        interactive_rebase(d, "HEAD~2", &plan).unwrap();
        assert_eq!(subjects(d), ["a", "base"]);

        // guard rails
        let only = vec![(log(d, 1).unwrap()[0].id.clone(), TodoAction::Squash)];
        assert!(interactive_rebase(d, "HEAD~1", &only).is_err());
        let all_dropped = vec![(log(d, 1).unwrap()[0].id.clone(), TodoAction::Drop)];
        assert!(interactive_rebase(d, "HEAD~1", &all_dropped).is_err());
        assert_eq!(op_state(d).unwrap(), OpState::None);
    }

    #[test]
    fn clone_init_and_names() {
        assert_eq!(
            repo_name_from_url("https://github.com/medmdime/Spork.git"),
            "Spork"
        );
        assert_eq!(repo_name_from_url("git@github.com:org/tool"), "tool");
        assert_eq!(repo_name_from_url("/srv/git/project.git/"), "project");
        assert_eq!(repo_name_from_url(""), "repository");

        let tmp = tempfile::tempdir().unwrap();
        let origin = tmp.path().join("origin");
        init(&origin).unwrap();
        git(&origin, &["commit", "-q", "--allow-empty", "-m", "hello"]);
        assert!(head_branch(&origin).unwrap().is_some());

        let dest = clone(origin.to_str().unwrap(), tmp.path(), "copy", "", None).unwrap();
        assert_eq!(head_message(&dest).as_deref(), Some("hello"));
        // never clone over an existing non-empty folder
        let err = clone(origin.to_str().unwrap(), tmp.path(), "copy", "", None).unwrap_err();
        assert!(err.to_string().contains("isn't empty"));

        // one branch, shallow (depth needs a file:// URL for a local origin)
        git(&origin, &["commit", "-q", "--allow-empty", "-m", "two"]);
        git(&origin, &["branch", "side", "HEAD~1"]);
        let url = format!("file://{}", origin.display());
        let side = clone(&url, tmp.path(), "side", "side", None).unwrap();
        assert_eq!(head_branch(&side).unwrap().as_deref(), Some("side"));
        let shallow = clone(&url, tmp.path(), "shallow", "", Some(1)).unwrap();
        assert_eq!(log(&shallow, 10).unwrap().len(), 1);
    }

    #[test]
    fn worktrees_and_submodules() {
        let tmp = tempfile::tempdir().unwrap();
        let (lib, d) = (tmp.path().join("lib"), tmp.path().join("app"));
        init(&lib).unwrap();
        git(&lib, &["commit", "-q", "--allow-empty", "-m", "lib"]);
        init(&d).unwrap();
        git(&d, &["commit", "-q", "--allow-empty", "-m", "app"]);
        git(&d, &["branch", "feature"]);

        let wt = tmp.path().join("app-feature");
        add_worktree(&d, &wt, "feature").unwrap();
        let list = worktrees(&d).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list[0].is_main && !list[1].is_main);
        assert_eq!(list[1].branch.as_deref(), Some("feature"));
        assert_eq!(head_branch(&wt).unwrap().as_deref(), Some("feature"));
        std::fs::write(wt.join("dirty.txt"), "x").unwrap();
        assert!(remove_worktree(&d, &wt).is_err()); // uncommitted work is protected
        std::fs::remove_file(wt.join("dirty.txt")).unwrap();
        remove_worktree(&d, &wt).unwrap();
        assert_eq!(worktrees(&d).unwrap().len(), 1);

        assert!(submodules(&d).unwrap().is_empty());
        git(
            &d,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                lib.to_str().unwrap(),
                "vendor/lib",
            ],
        );
        git(&d, &["commit", "-q", "-m", "add lib"]);
        let subs = submodules(&d).unwrap();
        assert_eq!(
            (subs.len(), subs[0].path.as_str(), subs[0].state),
            (1, "vendor/lib", ' ')
        );
    }

    #[test]
    fn apply_patches_and_force_with_lease() {
        let (tmp, out) = (crate::testutil::repo(), tempfile::tempdir().unwrap());
        let d = tmp.path();
        let write = |s: &str| std::fs::write(d.join("f.txt"), s).unwrap();
        write("base\n");
        stage(d, &["f.txt"]).unwrap();
        commit(d, "base", false).unwrap();
        write("patched\n");
        stage(d, &["f.txt"]).unwrap();
        commit(d, "change", false).unwrap();
        let patch = crate::revision::save_patch(d, "HEAD", out.path()).unwrap();

        // as a commit (git am), then as plain changes (git apply)
        crate::revision::reset(d, "HEAD~1", crate::revision::ResetMode::Hard).unwrap();
        assert_eq!(apply_mailbox(d, &patch, true).unwrap(), Outcome::Done);
        assert_eq!(head_message(d).as_deref(), Some("change"));
        crate::revision::reset(d, "HEAD~1", crate::revision::ResetMode::Hard).unwrap();
        assert_eq!(apply_mailbox(d, &patch, false).unwrap(), Outcome::Done);
        assert!(status(d).unwrap().iter().any(|e| e.path == "f.txt"));

        // a conflicting `am` pauses as Applying and can be aborted
        crate::revision::reset(d, "HEAD", crate::revision::ResetMode::Hard).unwrap();
        write("other\n");
        stage(d, &["f.txt"]).unwrap();
        commit(d, "other", false).unwrap();
        assert_eq!(apply_mailbox(d, &patch, true).unwrap(), Outcome::Conflicts);
        assert!(matches!(op_state(d).unwrap(), OpState::Applying { .. }));
        abort_op(d).unwrap();
        assert_eq!(op_state(d).unwrap(), OpState::None);

        // force-with-lease refuses to overwrite commits we haven't fetched, then works once fetched
        let r = tempfile::tempdir().unwrap();
        crate::testutil::bare_origin(r.path(), d);
        run(d, &["push", "--set-upstream", "origin", "main"]).unwrap();
        let other = r.path().join("other");
        git(
            r.path(),
            &["clone", "-q", "--branch", "main", "origin.git", "other"],
        );
        git(&other, &["commit", "-q", "--allow-empty", "-m", "theirs"]);
        git(&other, &["push", "-q", "origin", "main"]);
        commit(d, "mine, amended", true).unwrap();
        assert!(push_force_with_lease(d).is_err());
        fetch(d).unwrap();
        assert_eq!(push_force_with_lease(d).unwrap(), "Force-pushed main");
    }

    #[test]
    fn logs_writes_and_failures_not_reads() {
        assert!(is_read(&["status", "--porcelain=v2"]));
        assert!(is_read(&[
            "-c",
            "credential.interactive=false",
            "credential",
            "fill"
        ]));
        assert!(is_read(&["config", "--local", "--get", "merge.ff"]));
        assert!(is_read(&["stash", "list"]));
        assert!(!is_read(&["--literal-pathspecs", "stash", "push"]));
        assert!(!is_read(&["config", "--local", "merge.ff", "false"]));
        assert!(!is_read(&["push", "--force-with-lease"]));
        let unset = ["config", "--get", "spork.issueTracker"];
        assert!(!worth_logging(&unset, Some(1), b""));
        assert!(worth_logging(
            &unset,
            Some(128),
            b"fatal: not a git repository"
        ));
        assert!(worth_logging(&["fetch"], Some(1), b""));
        let global = ["config", "--global", "--get", "spork.github.com.default"];
        assert!(!worth_logging(&global, Some(1), b""));
        let probe = ["-c", "credential.interactive=false", "credential", "fill"];
        assert!(!worth_logging(
            &probe,
            Some(128),
            b"fatal: unable to get password from user"
        ));
        let tmp = crate::testutil::repo();
        let _ = run(tmp.path(), &["switch", "no-such-branch"]);
        let last = activity()
            .into_iter()
            .rev()
            .find(|c| c.args == "switch no-such-branch");
        assert!(last.is_some_and(|c| c.code != Some(0) && !c.stderr.is_empty()));
    }

    #[test]
    fn worktrees_show_their_lock() {
        let tmp = crate::testutil::repo();
        run(tmp.path(), &["commit", "--allow-empty", "-m", "root"]).unwrap();
        run(tmp.path(), &["branch", "side"]).unwrap();
        let linked = tmp.path().join("linked");
        add_worktree(tmp.path(), &linked, "side").unwrap();
        lock_worktree(tmp.path(), &linked, true).unwrap();
        let list = worktrees(tmp.path()).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list[0].is_main && !list[0].locked);
        assert_eq!(list[1].branch.as_deref(), Some("side"));
        assert!(list[1].locked && !list[1].prunable);
        lock_worktree(tmp.path(), &linked, false).unwrap();
        std::fs::remove_dir_all(&linked).unwrap();
        assert!(worktrees(tmp.path()).unwrap()[1].prunable);
        prune_worktrees(tmp.path()).unwrap();
        assert_eq!(worktrees(tmp.path()).unwrap().len(), 1);
    }

    #[test]
    fn worktree_include_lists_the_untracked_files_it_matches() {
        let tmp = crate::testutil::repo();
        let write = |name: &str| std::fs::write(tmp.path().join(name), "x").unwrap();
        write(".worktreeinclude");
        std::fs::write(tmp.path().join(".worktreeinclude"), "*.env\n").unwrap();
        write("tracked.env");
        run(tmp.path(), &["add", "tracked.env"]).unwrap();
        write("local.env");
        write("other.txt");
        assert_eq!(worktree_include_files(tmp.path()).unwrap(), ["local.env"]);
    }

    #[test]
    fn lists_a_commits_files_and_renames() {
        assert_eq!(
            name_status("M\0a.txt\0R100\0old.rs\0new.rs\0A\0b c.md\0"),
            [
                ('M', "a.txt".to_owned()),
                ('R', "new.rs".to_owned()),
                ('A', "b c.md".to_owned())
            ]
        );
        let tmp = crate::testutil::repo();
        std::fs::write(tmp.path().join("first.txt"), "x").unwrap();
        run(tmp.path(), &["add", "."]).unwrap();
        run(tmp.path(), &["commit", "-m", "root"]).unwrap();
        assert_eq!(
            commit_files(tmp.path(), "HEAD").unwrap(),
            [('A', "first.txt".to_owned())]
        );
    }

    #[test]
    fn parses_porcelain_v2_records() {
        let out = "1 .M N... 100644 100644 100644 aaa bbb src/main.rs\0\
                   2 R. N... 100644 100644 100644 aaa bbb R100 new name.rs\0old name.rs\0\
                   u UU N... 100644 100644 100644 100644 a b c conflict.rs\0\
                   ? notes/todo.md\0";
        let st = parse_status(out);
        assert_eq!(st.len(), 4);
        assert_eq!(
            (st[0].path.as_str(), st[0].index, st[0].worktree),
            ("src/main.rs", '.', 'M')
        );
        assert!(st[0].has_unstaged() && !st[0].has_staged());
        assert_eq!(
            (st[1].path.as_str(), st[1].orig_path.as_deref()),
            ("new name.rs", Some("old name.rs"))
        );
        assert!(st[2].conflicted && st[2].has_unstaged() && !st[2].has_staged());
        assert!(st[3].untracked());
    }

    #[test]
    fn plain_folder_is_not_a_repo() {
        let tmp = tempfile::tempdir().unwrap();
        // stop git from discovering a repo in a parent of the temp dir
        let err = Command::new(git_binary())
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(tmp.path())
            .env("GIT_CEILING_DIRECTORIES", tmp.path().parent().unwrap())
            .env("LC_ALL", "C")
            .output()
            .unwrap();
        assert_eq!(
            classify(err.status.code(), &String::from_utf8_lossy(&err.stderr)),
            GitError::NotARepo
        );
    }

    #[test]
    fn merge_kinds() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        git(d, &["init", "-q", "-b", "main"]);
        git(d, &["commit", "-q", "--allow-empty", "-m", "base"]);
        git(d, &["switch", "-q", "-c", "side"]);
        std::fs::write(d.join("s.txt"), "side\n").unwrap();
        git(d, &["add", "s.txt"]);
        git(d, &["commit", "-q", "-m", "side"]);
        git(d, &["switch", "-q", "main"]);
        let head = || {
            String::from_utf8_lossy(&run(d, &["rev-parse", "HEAD"]).unwrap())
                .trim()
                .to_owned()
        };
        let base = head();
        let back = || git(d, &["reset", "-q", "--hard", &base]);

        // could fast-forward, but a merge commit was asked for
        merge_with(d, "side", MergeKind::Commit, true).unwrap();
        assert!(is_merge_commit(d, "HEAD"));
        back();

        // squash: HEAD stays, the change is staged, no merge in progress
        merge_with(d, "side", MergeKind::Squash, true).unwrap();
        assert_eq!(head(), base);
        let st = status(d).unwrap();
        assert!(
            st.iter().any(|e| e.path == "s.txt" && e.index == 'A'),
            "{st:?}"
        );
        assert_eq!(op_state(d).unwrap(), OpState::None);
        back();

        // no commit: stops with the merge in progress, even where it could fast-forward
        merge_with(d, "side", MergeKind::Auto, false).unwrap();
        assert_eq!(head(), base);
        assert_eq!(op_state(d).unwrap(), OpState::Merging);
    }

    #[test]
    fn rewords_and_edits_older_commits() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        git(d, &["init", "-q", "-b", "main"]);
        for (file, msg) in [("a", "one"), ("b", "two"), ("c", "three")] {
            std::fs::write(d.join(file), msg).unwrap();
            git(d, &["add", file]);
            git(d, &["commit", "-q", "-m", msg]);
        }
        let subjects = || {
            log(d, 10)
                .unwrap()
                .into_iter()
                .map(|c| c.subject)
                .collect::<Vec<_>>()
        };
        let rev = |r: &str| {
            String::from_utf8_lossy(&run(d, &["rev-parse", r]).unwrap())
                .trim()
                .to_owned()
        };
        let tree = rev("HEAD^{tree}");

        assert_eq!(
            reword(d, "HEAD~1", "two, said better\n\nwith a body").unwrap(),
            Outcome::Done
        );
        assert_eq!(subjects(), ["three", "two, said better", "one"]);
        assert_eq!(rev("HEAD^{tree}"), tree); // only the message changed
        assert!(reword(d, "HEAD~2", "x").is_err()); // the root commit

        // edit stops after the commit; continuing finishes the rebase
        let base = rev("HEAD~2");
        let plan = [
            (rev("HEAD~1"), TodoAction::Edit),
            (rev("HEAD"), TodoAction::Pick),
        ];
        interactive_rebase(d, &base, &plan).unwrap();
        assert!(matches!(op_state(d).unwrap(), OpState::Rebasing { .. }));
        assert_eq!(continue_op(d).unwrap(), Outcome::Done);
        assert_eq!(op_state(d).unwrap(), OpState::None);
    }

    #[test]
    fn commit_options_template_and_pushed_head() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        git(d, &["init", "-q", "-b", "main"]);
        for (k, v) in [
            ("user.name", "Spork Test"),
            ("user.email", "test@spork.invalid"),
            ("commit.gpgsign", "false"),
        ] {
            git(d, &["config", k, v]);
        }
        std::fs::write(d.join("a"), "1").unwrap();
        git(d, &["add", "a"]);
        let opts = CommitOptions {
            signoff: true,
            ..Default::default()
        };
        commit_with(d, "signed", opts).unwrap();
        assert!(
            commit_message(d, "HEAD")
                .unwrap()
                .contains("Signed-off-by: Spork Test <test@spork.invalid>")
        );

        // a hook that refuses every commit; --no-verify skips it
        assert!(!has_commit_hooks(d));
        let hook = d.join(".git/hooks/pre-commit");
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(&hook, "#!/bin/sh\necho no >&2\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert!(has_commit_hooks(d));
        std::fs::write(d.join("a"), "2").unwrap();
        git(d, &["add", "a"]);
        assert!(commit(d, "blocked", false).is_err());
        commit_with(
            d,
            "unblocked",
            CommitOptions {
                no_verify: true,
                ..Default::default()
            },
        )
        .unwrap();

        // the template, comment lines dropped
        std::fs::write(d.join("tpl.txt"), "Subject\n# a hint\n\nWhy:\n").unwrap();
        git(d, &["config", "commit.template", "tpl.txt"]);
        assert_eq!(commit_template(d).as_deref(), Some("Subject\n\nWhy:"));

        // pushed once HEAD is on a remote branch
        assert!(!head_is_pushed(d));
        let remote = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "-q", "--bare"]);
        git(
            d,
            &[
                "remote",
                "add",
                "origin",
                &remote.path().display().to_string(),
            ],
        );
        git(d, &["push", "-q", "origin", "main"]);
        assert!(head_is_pushed(d));
    }

    #[test]
    fn pull_rebases_with_local_changes() {
        let (tmp, origin) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let d = tmp.path();
        git(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
        git(d, &["init", "-q", "-b", "main"]);
        for (k, v) in [
            ("user.name", "Spork Test"),
            ("user.email", "test@spork.invalid"),
            ("commit.gpgsign", "false"),
        ] {
            git(d, &["config", k, v]);
        }
        git(
            d,
            &[
                "remote",
                "add",
                "origin",
                &origin.path().display().to_string(),
            ],
        );
        std::fs::write(d.join("a"), "1").unwrap();
        git(d, &["add", "a"]);
        git(d, &["commit", "-q", "-m", "base"]);
        git(d, &["push", "-q", "origin", "main"]);
        // someone else's commit on the remote; a local commit and an uncommitted edit here
        git(d, &["commit", "-q", "--allow-empty", "-m", "theirs"]);
        git(d, &["push", "-q", "origin", "main"]);
        git(d, &["reset", "-q", "--hard", "HEAD~1"]);
        git(d, &["commit", "-q", "--allow-empty", "-m", "mine"]);
        std::fs::write(d.join("a"), "dirty").unwrap();

        let opts = PullOptions {
            rebase: true,
            autostash: true,
        };
        assert_eq!(pull_from(d, "origin", "main", opts).unwrap(), Outcome::Done);
        let subjects: Vec<String> = log(d, 10).unwrap().into_iter().map(|c| c.subject).collect();
        assert_eq!(subjects, ["mine", "theirs", "base"]); // linear: mine on top of theirs
        assert_eq!(std::fs::read_to_string(d.join("a")).unwrap(), "dirty");
    }

    #[test]
    fn network_commands_are_jobs() {
        let (tmp, origin) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let d = tmp.path();
        git(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
        git(d, &["init", "-q", "-b", "main"]);
        git(
            d,
            &[
                "remote",
                "add",
                "origin",
                &origin.path().display().to_string(),
            ],
        );
        // a fetch runs through the job path (`--progress` after the subcommand is accepted)
        run(d, &["fetch", "origin"]).unwrap();
        let stderr = "Counting objects: 1\rReceiving objects:  45% (45/100)\rReceiving objects: 100% (100/100), done.\n";
        assert_eq!(
            last_progress_line(stderr),
            Some("Receiving objects: 100% (100/100), done.")
        );
        assert_eq!(last_progress_line("remote: x\r\n\n"), Some("remote: x"));
        assert_eq!(last_progress_line(""), None);
    }

    #[test]
    fn reverts_a_resolved_conflict() {
        let tmp = testutil::repo();
        let dir = tmp.path();
        std::fs::write(dir.join("c.txt"), "base\n").unwrap();
        git(dir, &["add", "c.txt"]);
        git(dir, &["commit", "-q", "-m", "base"]);
        git(dir, &["checkout", "-q", "-b", "other"]);
        std::fs::write(dir.join("c.txt"), "theirs\n").unwrap();
        git(dir, &["commit", "-q", "-am", "theirs"]);
        git(dir, &["checkout", "-q", "main"]);
        std::fs::write(dir.join("c.txt"), "ours\n").unwrap();
        git(dir, &["commit", "-q", "-am", "ours"]);
        assert!(merge(dir, "other").is_ok());
        resolve_with(dir, "c.txt", false).unwrap();
        assert_eq!(resolved_conflicts(dir).unwrap(), vec!["c.txt"]);
        revert_resolved(dir, &["c.txt".to_owned()]).unwrap();
        assert!(status(dir).unwrap().iter().any(|e| e.conflicted));
    }

    #[test]
    fn rolls_back_keeping_added_files() {
        let tmp = testutil::repo();
        let dir = tmp.path();
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        git(dir, &["add", "a.txt"]);
        git(dir, &["commit", "-q", "-m", "a"]);
        std::fs::write(dir.join("a.txt"), "two\n").unwrap();
        // rolling back keeps an added file's copy unless asked
        std::fs::write(dir.join("c.txt"), "new\n").unwrap();
        git(dir, &["add", "c.txt"]);
        rollback(dir, &status(dir).unwrap(), false).unwrap();
        let left = status(dir).unwrap();
        assert_eq!(left.len(), 1);
        assert!(left[0].untracked() && left[0].path == "c.txt");
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\n");
    }
}
