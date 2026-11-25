//! Optional AI features with the user's own provider (docs/10-ai.md): off until configured, the
//! user sees what will be sent, and nothing here ever runs git.
//!
//! Providers: Anthropic's Messages API, and any OpenAI-compatible chat endpoint (Ollama,
//! LM Studio, vLLM…) for local models. Blocking calls, meant for background threads.
// shortcut: commit messages, code review and PR descriptions, no streaming; the rest of
// docs/10-ai.md §3 reuses `complete` with its own prompts.

use std::fmt;

use serde_json::{Value, json};

pub const DEFAULT_MODEL: &str = "claude-opus-5-5";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provider {
    /// `api.anthropic.com` with an API key.
    Anthropic { key: String, model: String },
    /// `<endpoint>/chat/completions`, e.g. `http://localhost:11434/v1` for Ollama; the key is
    /// optional for local servers.
    OpenAiCompatible {
        endpoint: String,
        key: String,
        model: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Http(u16, String),
    Network(String),
    /// The model declined; its answer is not used.
    Refused,
    Unexpected,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Http(401, _) => write!(f, "The API key was rejected."),
            Error::Http(status, msg) => write!(f, "The AI provider answered {status}: {msg}"),
            Error::Network(e) => write!(f, "Couldn't reach the AI provider: {e}"),
            Error::Refused => write!(f, "The model declined this request."),
            Error::Unexpected => write!(f, "The AI provider sent an answer Spork can't read."),
        }
    }
}

/// What a request would send, shown to the user before anything leaves the machine (§2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    /// The diff text that would be sent.
    pub diff: String,
    /// Files left out: excluded by name, or cut because of the size cap.
    pub omitted: Vec<String>,
    /// Files whose changes look like they contain a secret; the user must confirm or remove them.
    pub secrets: Vec<String>,
}

// keep requests small: commit messages need the gist, not every line
const PER_FILE_CAP: usize = 8_000;
const TOTAL_CAP: usize = 60_000;

/// Lockfiles, generated and binary-ish files, and anything that usually holds secrets.
fn excluded(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
    matches!(
        name,
        "Cargo.lock"
            | "package-lock.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "poetry.lock"
            | "Gemfile.lock"
            | "composer.lock"
            | "go.sum"
    ) || name.starts_with(".env")
        || name.ends_with(".min.js")
        || matches!(
            ext,
            "map" | "pem" | "key" | "p12" | "pfx" | "jks" | "keystore"
        )
}

/// Strings that mark a likely secret on an added line.
const SECRET_MARKERS: &[&str] = &[
    "-----BEGIN",
    "AKIA",
    "ghp_",
    "github_pat_",
    "glpat-",
    "sk-ant-",
    "xoxb-",
    "xoxp-",
    "AIza",
];

/// Whether `text` contains something that looks like a key or token.
pub fn looks_secret(text: &str) -> bool {
    SECRET_MARKERS.iter().any(|m| text.contains(m))
}

/// Splits a `git diff` into files, drops excluded ones, caps sizes and scans for secrets.
pub fn prepare(diff: &str) -> Prepared {
    let mut out = Prepared {
        diff: String::new(),
        omitted: Vec::new(),
        secrets: Vec::new(),
    };
    for chunk in diff.split("\ndiff --git ").filter(|c| !c.trim().is_empty()) {
        let chunk = chunk.strip_prefix("diff --git ").unwrap_or(chunk);
        // header: "a/<path> b/<path>"
        let header = chunk.lines().next().unwrap_or("");
        let path = header
            .rsplit_once(" b/")
            .map_or(header, |(_, p)| p)
            .to_owned();
        if excluded(&path) || chunk.contains("\nBinary files ") {
            out.omitted.push(path);
            continue;
        }
        if chunk
            .lines()
            .filter(|l| l.starts_with('+'))
            .any(|l| SECRET_MARKERS.iter().any(|m| l.contains(m)))
        {
            out.secrets.push(path.clone());
        }
        let mut text = format!("diff --git {chunk}");
        if text.len() > PER_FILE_CAP {
            let cut = (0..=PER_FILE_CAP)
                .rev()
                .find(|&i| text.is_char_boundary(i))
                .unwrap_or(0);
            text.truncate(cut);
            text.push_str("\n[… rest of this file's changes left out]");
        }
        if out.diff.len() + text.len() > TOTAL_CAP {
            out.omitted.push(path);
            continue;
        }
        if !out.diff.is_empty() {
            out.diff.push('\n');
        }
        out.diff.push_str(&text);
    }
    out
}

const COMMIT_SYSTEM: &str = "You write git commit messages for the staged changes the user sends. \
Reply with the message only: a subject line in the imperative mood of at most 72 characters, \
then, only if the change needs explaining, a blank line and a short body wrapped at 72 characters \
that says what changed and why. Match the style of the recent subjects you are given \
(for example Conventional Commits prefixes) when they follow one. No Markdown, no code fences, \
no quotes around the message.";

/// A commit message for `prepared.diff`, in the style of `recent_subjects`.
pub fn commit_message(
    provider: &Provider,
    prepared: &Prepared,
    recent_subjects: &[String],
) -> Result<String, Error> {
    let mut user = String::new();
    if !recent_subjects.is_empty() {
        user.push_str("Recent subjects in this repository:\n");
        for s in recent_subjects {
            user.push_str(&format!("- {s}\n"));
        }
        user.push('\n');
    }
    if !prepared.omitted.is_empty() {
        user.push_str(&format!(
            "Also changed (not shown): {}\n\n",
            prepared.omitted.join(", ")
        ));
    }
    user.push_str("Staged changes:\n");
    user.push_str(&prepared.diff);
    let text = complete(provider, COMMIT_SYSTEM, &user)?;
    Ok(clean_message(&text))
}

const REVIEW_SYSTEM: &str = "You review a change to a codebase, given as a git diff. \
Report only problems worth fixing before this ships: bugs, security issues, data loss, \
unhandled errors and risky behaviour changes. For each, name the file and line, say what goes \
wrong and when, and suggest the fix in a sentence. Skip style and naming remarks. \
If nothing needs fixing, say so in one sentence. Answer in Markdown, as a short bullet list.";

/// Review notes in Markdown for `prepared.diff` (docs/10-ai.md §3, feature 2).
pub fn review(provider: &Provider, prepared: &Prepared, title: &str) -> Result<String, Error> {
    let mut user = format!("Change: {title}\n\n");
    if !prepared.omitted.is_empty() {
        user.push_str(&format!(
            "Also changed (not shown): {}\n\n",
            prepared.omitted.join(", ")
        ));
    }
    user.push_str(&prepared.diff);
    complete(provider, REVIEW_SYSTEM, &user)
}

const PR_SYSTEM: &str = "You write pull request descriptions from a branch's commits and diff. \
Reply with a title line of at most 72 characters, a blank line, then a body in Markdown for the \
reviewers: what changes and why, anything they should look at closely, and how to test it. \
Keep it short and factual; don't repeat the diff line by line.";

/// A pull request title and body (`title\n\nbody`) for a branch (docs/10-ai.md §3, feature 3).
pub fn pr_description(
    provider: &Provider,
    prepared: &Prepared,
    subjects: &[String],
) -> Result<String, Error> {
    let mut user = String::from("Commits on the branch, oldest first:\n");
    for s in subjects {
        user.push_str(&format!("- {s}\n"));
    }
    if !prepared.omitted.is_empty() {
        user.push_str(&format!(
            "\nAlso changed (not shown): {}\n",
            prepared.omitted.join(", ")
        ));
    }
    user.push_str("\nDiff:\n");
    user.push_str(&prepared.diff);
    Ok(clean_message(&complete(provider, PR_SYSTEM, &user)?))
}

const EXPLAIN_SYSTEM: &str = "You explain a git commit to a developer who didn't write it. \
In a few short paragraphs of Markdown: what it changes, why it was most likely made, and what \
it could affect elsewhere. Refer to files by name. Say when the diff doesn't show enough to tell.";

/// A plain-language explanation of a commit (docs/10-ai.md §3, feature 5).
pub fn explain(provider: &Provider, prepared: &Prepared, message: &str) -> Result<String, Error> {
    let mut user = format!("Commit message:\n{message}\n\n");
    if !prepared.omitted.is_empty() {
        user.push_str(&format!(
            "Also changed (not shown): {}\n\n",
            prepared.omitted.join(", ")
        ));
    }
    user.push_str(&prepared.diff);
    complete(provider, EXPLAIN_SYSTEM, &user)
}

const RELEASE_SYSTEM: &str = "You write release notes from a list of commit subjects. \
Group them under Markdown headings (Features, Fixes, Other; leave out empty groups), one short \
user-facing bullet per change, merging duplicates and leaving out purely internal chores. \
No preamble.";

/// Release notes in Markdown for a tag's commits (docs/10-ai.md §3, feature 8).
pub fn release_notes(provider: &Provider, tag: &str, subjects: &[String]) -> Result<String, Error> {
    let mut user = format!("Release {tag}. Commits, oldest first:\n");
    for s in subjects {
        user.push_str(&format!("- {s}\n"));
    }
    complete(provider, RELEASE_SYSTEM, &user)
}

const CONFLICT_SYSTEM: &str = "You resolve git merge conflicts. You get a whole file in which \
conflicting hunks sit between <<<<<<<, ======= and >>>>>>> markers. Combine both sides so the \
intent of each survives; when they truly contradict, prefer the incoming side and keep the code \
compiling. Reply with the complete resolved file only: no conflict markers, no explanations, \
no code fences.";

/// A proposed resolution of every conflict in `text` (docs/10-ai.md §3, feature 4); the caller
/// shows it and applies it only if the user says so.
pub fn resolve_conflicts(provider: &Provider, path: &str, text: &str) -> Result<String, Error> {
    let user = format!("File: {path}\n\n{text}");
    resolved_file(&complete(provider, CONFLICT_SYSTEM, &user)?).ok_or(Error::Unexpected)
}

/// The file in a model's answer: unfenced when it fenced it anyway; `None` if conflict markers remain.
fn resolved_file(answer: &str) -> Option<String> {
    let fenced = answer.trim().strip_prefix("```").and_then(|rest| {
        let (_, body) = rest.split_once('\n')?; // drop the language tag line
        Some(body.trim_end().strip_suffix("```")?.to_owned())
    });
    let file = fenced.unwrap_or_else(|| answer.to_owned());
    (!file.contains("<<<<<<<") && !file.contains(">>>>>>>")).then_some(file)
}

/// What the rebase planner proposes for one commit (docs/10-ai.md §3, feature 6). It never
/// proposes dropping or moving commits: that stays the user's call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Pick,
    /// Meld into the previous commit, keeping both messages.
    Squash,
    /// Meld into the previous commit, dropping this message.
    Fixup,
}

const REBASE_SYSTEM: &str = "You tidy a branch's history before it is shared. You get its \
commits oldest first, one per line as `<id> <subject>`. Reply with one line per commit, in the \
same order: `<id> pick`, `<id> squash` (belongs with the commit before it, both messages kept) \
or `<id> fixup` (only fixes the commit before it, e.g. typos, review fixes, 'wip'). The first \
commit is always pick. When unsure, pick. Nothing else in the reply.";

/// One step per commit of `commits` (`(id, subject)`, oldest first).
pub fn plan_rebase(provider: &Provider, commits: &[(String, String)]) -> Result<Vec<Step>, Error> {
    let user: String = commits
        .iter()
        .map(|(id, subject)| format!("{id} {subject}\n"))
        .collect();
    let answer = complete(provider, REBASE_SYSTEM, &user)?;
    Ok(parse_plan(&answer, commits))
}

/// Reads `<id> <step>` lines; commits the answer skips or garbles stay `Pick`, and the first
/// commit is always `Pick` (there is nothing before it to meld into).
fn parse_plan(answer: &str, commits: &[(String, String)]) -> Vec<Step> {
    commits
        .iter()
        .enumerate()
        .map(|(i, (id, _))| {
            let step = answer.lines().find_map(|l| {
                let mut words = l.split_whitespace();
                (words.next()? == id).then(|| words.next()).flatten()
            });
            match step {
                Some("squash") if i > 0 => Step::Squash,
                Some("fixup") if i > 0 => Step::Fixup,
                _ => Step::Pick,
            }
        })
        .collect()
}

const ASK_SYSTEM: &str = "You map a git user's request to one command of a git client. You \
get the client's commands as a numbered list, then the request. Answer with the number of the \
one command that does what was asked, or 0 when none does. Only the number.";

/// Which of `commands` does what `question` asks (docs/10-ai.md §3, feature 9): its index, or
/// `None`. The caller names it and runs it only when the user confirms; nothing else can run.
pub fn ask(
    provider: &Provider,
    question: &str,
    commands: &[String],
) -> Result<Option<usize>, Error> {
    let mut user = String::new();
    for (i, c) in commands.iter().enumerate() {
        user.push_str(&format!("{}. {c}\n", i + 1));
    }
    user.push_str(&format!("\nRequest: {question}"));
    Ok(pick(
        &complete(provider, ASK_SYSTEM, &user)?,
        commands.len(),
    ))
}

/// The 1-based number at the start of an answer, as an index into `count` commands.
fn pick(answer: &str, count: usize) -> Option<usize> {
    let n: usize = answer
        .trim()
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    (1..=count).contains(&n).then(|| n - 1)
}

/// Trims what models sometimes add anyway: code fences and surrounding quotes.
fn clean_message(text: &str) -> String {
    let t = text.trim();
    let t = t
        .strip_prefix("```")
        .map(|rest| rest.split_once('\n').map_or("", |(_, body)| body))
        .unwrap_or(t);
    let t = t.trim().strip_suffix("```").unwrap_or(t).trim();
    t.trim_matches('"').trim().to_owned()
}

/// One request, one text answer.
pub fn complete(provider: &Provider, system: &str, user: &str) -> Result<String, Error> {
    match provider {
        Provider::Anthropic { key, model } => {
            let body = json!({
                "model": model,
                // room for adaptive thinking plus the answer; non-streaming stays well under timeouts
                "max_tokens": 16000,
                // a commit message is a simple task
                "output_config": {"effort": "low"},
                // if a safety classifier declines, retry server-side on the recommended model
                "fallbacks": "default",
                "system": system,
                "messages": [{"role": "user", "content": user}],
            });
            let v = post(
                "https://api.anthropic.com/v1/messages",
                &[
                    ("x-api-key", key),
                    ("anthropic-version", "2026-10-21"),
                    ("anthropic-beta", "server-side-fallback-2026-07-01"),
                ],
                &body,
            )?;
            anthropic_text(&v)
        }
        Provider::OpenAiCompatible {
            endpoint,
            key,
            model,
        } => {
            let body = json!({
                "model": model,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ],
            });
            let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));
            let auth = format!("Bearer {key}");
            let headers: Vec<(&str, &str)> = if key.is_empty() {
                Vec::new()
            } else {
                vec![("Authorization", auth.as_str())]
            };
            let v = post(&url, &headers, &body)?;
            v["choices"][0]["message"]["content"]
                .as_str()
                .map(str::to_owned)
                .ok_or(Error::Unexpected)
        }
    }
}

/// The text of a Messages API response; a refusal is an error, never text to use.
fn anthropic_text(v: &Value) -> Result<String, Error> {
    if v["stop_reason"] == "refusal" {
        return Err(Error::Refused);
    }
    // thinking blocks may come first; only text blocks are the answer
    let text: String = v["content"]
        .as_array()
        .ok_or(Error::Unexpected)?
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect();
    if text.trim().is_empty() {
        return Err(Error::Unexpected);
    }
    Ok(text)
}

fn post(url: &str, headers: &[(&str, &str)], body: &Value) -> Result<Value, Error> {
    let mut req = ureq::post(url).header("Content-Type", "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let mut response = req
        .config()
        .http_status_as_error(false)
        .build()
        .send(body.to_string())
        .map_err(|e| Error::Network(e.to_string()))?;
    let status = response.status().as_u16();
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| Error::Network(e.to_string()))?;
    let v: Value = serde_json::from_str(&text).map_err(|_| Error::Unexpected)?;
    if status >= 400 {
        // Anthropic: {"error": {"message"}}; OpenAI-compatible: {"error": {"message"}} or {"error": "…"}
        let msg = v["error"]["message"]
            .as_str()
            .or_else(|| v["error"].as_str())
            .unwrap_or("")
            .to_owned();
        return Err(Error::Http(status, msg));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n\
diff --git a/Cargo.lock b/Cargo.lock\n--- a/Cargo.lock\n+++ b/Cargo.lock\n@@ -1 +1 @@\n-a\n+b\n\
diff --git a/config/prod.rs b/config/prod.rs\n--- a/config/prod.rs\n+++ b/config/prod.rs\n@@ -0,0 +1 @@\n+let key = \"sk-ant-api03-xyz\";\n\
diff --git a/logo.png b/logo.png\nBinary files a/logo.png and b/logo.png differ\n";

    #[test]
    fn prepares_what_would_be_sent() {
        let p = prepare(DIFF);
        assert!(p.diff.contains("+new") && p.diff.contains("config/prod.rs"));
        assert!(!p.diff.contains("Cargo.lock") && !p.diff.contains("logo.png"));
        assert_eq!(p.omitted, ["Cargo.lock", "logo.png"]);
        assert_eq!(p.secrets, ["config/prod.rs"]);
        assert!(excluded(".env.local") && excluded("app/bundle.min.js") && excluded("id.pem"));
        assert!(!excluded("src/key_handler.rs"));
    }

    #[test]
    fn caps_large_files() {
        let big = format!(
            "diff --git a/a.txt b/a.txt\n@@ -0,0 +1 @@\n+{}\n",
            "é".repeat(PER_FILE_CAP)
        );
        let p = prepare(&big);
        assert!(p.diff.len() <= PER_FILE_CAP + 64 && p.diff.ends_with("left out]"));
    }

    #[test]
    fn parses_a_rebase_plan() {
        let commits: Vec<(String, String)> =
            ["a1 add login", "b2 fix typo", "c3 more login", "d4 docs"]
                .iter()
                .map(|l| {
                    let (id, s) = l.split_once(' ').unwrap();
                    (id.into(), s.into())
                })
                .collect();
        let answer = "a1 fixup\nb2 fixup\nc3 squash\nd4 drop\n";
        assert_eq!(
            parse_plan(answer, &commits),
            [Step::Pick, Step::Fixup, Step::Squash, Step::Pick]
        );
        assert_eq!(parse_plan("nonsense", &commits), [Step::Pick; 4]);
    }

    #[test]
    fn picks_a_command_number() {
        assert_eq!(pick("3", 5), Some(2));
        assert_eq!(pick(" 1. Fetch", 5), Some(0));
        assert_eq!(pick("0", 5), None);
        assert_eq!(pick("9", 5), None);
        assert_eq!(pick("none", 5), None);
    }

    #[test]
    fn takes_the_resolved_file() {
        assert_eq!(resolved_file("a\nb\n").as_deref(), Some("a\nb\n"));
        assert_eq!(
            resolved_file("```rust\nfn a() {}\n```\n").as_deref(),
            Some("fn a() {}\n")
        );
        assert_eq!(
            resolved_file("<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> b\n"),
            None
        );
        assert!(looks_secret("token = ghp_abc") && !looks_secret("let x = 1;"));
    }

    #[test]
    fn reads_answers() {
        let ok = json!({"stop_reason": "end_turn", "content": [
            {"type": "thinking", "thinking": ""},
            {"type": "text", "text": "Fix login redirect\n\nThe callback lost the return URL."}]});
        assert_eq!(
            anthropic_text(&ok).unwrap(),
            "Fix login redirect\n\nThe callback lost the return URL."
        );
        assert_eq!(
            anthropic_text(&json!({"stop_reason": "refusal", "content": []})),
            Err(Error::Refused)
        );
        assert_eq!(clean_message("```\nAdd tests\n```"), "Add tests");
        assert_eq!(clean_message("\"Add tests\""), "Add tests");
    }
}
