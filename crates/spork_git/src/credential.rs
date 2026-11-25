//! Hosting tokens kept in the user's own git credential helper (docs/specs/M10 §1, M15 §1).
//!
//! `git credential approve` puts them in the keychain the user already configured (osxkeychain,
//! Git Credential Manager, libsecret…), so they never touch Spork's files, and git's own HTTPS
//! fetches and pushes to that host use them too.

use crate::{GitError, run_with_env};

// a lookup must never ask anyone: no askpass dialog, no terminal, no helper login window
const QUIET: &[(&str, &str)] = &[
    ("GIT_ASKPASS", ""),
    ("SSH_ASKPASS", ""),
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GCM_INTERACTIVE", "never"),
];

fn request(host: &str, username: Option<&str>, password: Option<&str>) -> String {
    let mut s = format!("protocol=https\nhost={host}\n");
    if let Some(u) = username {
        s.push_str(&format!("username={u}\n"));
    }
    if let Some(p) = password {
        s.push_str(&format!("password={p}\n"));
    }
    s + "\n"
}

fn credential(action: &str, input: &str) -> Result<Vec<u8>, GitError> {
    // run outside any repository: accounts are per user, not per repository config
    run_with_env(
        &std::env::temp_dir(),
        &["-c", "credential.interactive=false", "credential", action],
        Some(input.as_bytes()),
        QUIET,
    )
}

/// Saves `token` for `https://<host>` under `username`.
pub fn store(host: &str, username: &str, token: &str) -> Result<(), GitError> {
    if [host, username, token].iter().any(|v| v.contains('\n')) {
        return Err(GitError::Other {
            code: None,
            stderr: "Line breaks aren't allowed here.".into(),
        });
    }
    credential("approve", &request(host, Some(username), Some(token))).map(drop)
}

/// `(username, token)` saved for `https://<host>`; `None` when there is none.
pub fn get(host: &str) -> Option<(String, String)> {
    get_as(host, None)
}

/// [`get`] for one of several accounts on the host: `username`'s token.
pub fn get_as(host: &str, username: Option<&str>) -> Option<(String, String)> {
    let out = credential("fill", &request(host, username, None)).ok()?;
    parse_fill(&String::from_utf8_lossy(&out))
}

/// Forgets the token saved for `https://<host>` as `username`.
pub fn erase(host: &str, username: &str) -> Result<(), GitError> {
    credential("reject", &request(host, Some(username), None)).map(drop)
}

fn parse_fill(out: &str) -> Option<(String, String)> {
    let field = |key: &str| {
        out.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    Some((field("username")?, field("password")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    // the helper itself isn't exercised: tests must never write to the developer's keychain
    #[test]
    fn formats_requests_and_parses_answers() {
        assert_eq!(
            request("github.com", Some("ann"), Some("t0k")),
            "protocol=https\nhost=github.com\nusername=ann\npassword=t0k\n\n"
        );
        assert_eq!(
            parse_fill("protocol=https\nhost=github.com\nusername=ann\npassword=t0k\n"),
            Some(("ann".into(), "t0k".into()))
        );
        assert_eq!(parse_fill("protocol=https\nhost=github.com\n"), None);
        assert!(store("github.com", "ann", "a\nhost=evil").is_err());
    }
}
