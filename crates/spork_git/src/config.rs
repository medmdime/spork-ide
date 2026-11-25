//! Repository-local git config, for the repository settings (docs/specs/M02 §2), and the user's
//! global identity (M01 §1).

use std::path::Path;

use crate::{GitError, run};

/// The value set in this repository's own config, if any.
pub fn get_local(repo: &Path, key: &str) -> Option<String> {
    let out = run(repo, &["config", "--local", "--get", key]).ok()?;
    Some(String::from_utf8_lossy(&out).trim_end().to_owned())
}

/// The value git uses: local, else global, else system.
pub fn get(repo: &Path, key: &str) -> Option<String> {
    let out = run(repo, &["config", "--get", key]).ok()?;
    Some(String::from_utf8_lossy(&out).trim_end().to_owned())
}

/// The user's own (global) value, e.g. `user.name`.
pub fn get_global(key: &str) -> Option<String> {
    let out = run(&std::env::temp_dir(), &["config", "--global", "--get", key]).ok()?;
    Some(String::from_utf8_lossy(&out).trim_end().to_owned())
}

/// Sets `key` in the user's global config (`~/.gitconfig`).
pub fn set_global(key: &str, value: &str) -> Result<(), GitError> {
    run(&std::env::temp_dir(), &["config", "--global", key, value]).map(drop)
}

/// Adds `value` to multi-valued global `key`, once.
pub fn add_global(key: &str, value: &str) -> Result<(), GitError> {
    let dir = std::env::temp_dir();
    if get_all_global(key).iter().any(|v| v == value) {
        return Ok(());
    }
    run(&dir, &["config", "--global", "--add", key, value]).map(drop)
}

/// Every value of global `key`.
pub fn get_all_global(key: &str) -> Vec<String> {
    run(
        &std::env::temp_dir(),
        &["config", "--global", "--get-all", key],
    )
    .map(|out| {
        String::from_utf8_lossy(&out)
            .lines()
            .map(str::to_owned)
            .collect()
    })
    .unwrap_or_default()
}

/// Removes `value` from global `key` (every value with `None`); fine when it isn't there.
pub fn remove_global(key: &str, value: Option<&str>) -> Result<(), GitError> {
    let args = match value {
        Some(v) => vec!["config", "--global", "--fixed-value", "--unset-all", key, v],
        None => vec!["config", "--global", "--unset-all", key],
    };
    match run(&std::env::temp_dir(), &args) {
        Err(GitError::Other { code: Some(5), .. }) => Ok(()),
        other => other.map(drop),
    }
}

/// `(key, value)` of every global setting whose key matches `pattern` (a regex).
pub fn global_matching(pattern: &str) -> Vec<(String, String)> {
    run(
        &std::env::temp_dir(),
        &["config", "--global", "--get-regexp", pattern],
    )
    .map(|out| {
        String::from_utf8_lossy(&out)
            .lines()
            .filter_map(|l| l.split_once(' '))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    })
    .unwrap_or_default()
}

/// Sets `key` in this repository's config, or removes it with `None`.
pub fn set_local(repo: &Path, key: &str, value: Option<&str>) -> Result<(), GitError> {
    match value {
        Some(v) => run(repo, &["config", "--local", key, v]).map(drop),
        // exit code 5: the key wasn't set, which is what we want anyway
        None => match run(repo, &["config", "--local", "--unset", key]) {
            Err(GitError::Other { code: Some(5), .. }) => Ok(()),
            other => other.map(drop),
        },
    }
}

/// Every value of a multi-valued key, e.g. the starred branches (`spork.favorite`).
pub fn get_all(repo: &Path, key: &str) -> Vec<String> {
    run(repo, &["config", "--get-all", key])
        .map(|out| {
            String::from_utf8_lossy(&out)
                .lines()
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Adds `value` to multi-valued `key` in this repository's config, once.
pub fn add_local(repo: &Path, key: &str, value: &str) -> Result<(), GitError> {
    if get_all(repo, key).iter().any(|v| v == value) {
        return Ok(());
    }
    run(repo, &["config", "--local", "--add", key, value]).map(drop)
}

/// Removes `value` from multi-valued `key`, matched literally (branch names may hold regex characters).
pub fn remove_local(repo: &Path, key: &str, value: &str) -> Result<(), GitError> {
    match run(
        repo,
        &[
            "config",
            "--local",
            "--fixed-value",
            "--unset-all",
            key,
            value,
        ],
    ) {
        Err(GitError::Other { code: Some(5), .. }) => Ok(()), // wasn't there
        other => other.map(drop),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::repo;

    #[test]
    fn local_config_round_trip() {
        let tmp = repo();
        let d = tmp.path();
        assert_eq!(get_local(d, "merge.ff"), None);
        set_local(d, "merge.ff", Some("false")).unwrap();
        assert_eq!(get_local(d, "merge.ff").as_deref(), Some("false"));
        assert_eq!(get(d, "merge.ff").as_deref(), Some("false"));
        set_local(d, "merge.ff", None).unwrap();
        assert_eq!(get_local(d, "merge.ff"), None);
        set_local(d, "merge.ff", None).unwrap();
        assert_eq!(get(d, "user.name").as_deref(), Some("Spork Test"));
    }

    #[test]
    fn multi_valued_keys() {
        let tmp = repo();
        let d = tmp.path();
        add_local(d, "spork.favorite", "main").unwrap();
        add_local(d, "spork.favorite", "feature/a.b+c").unwrap();
        add_local(d, "spork.favorite", "main").unwrap(); // once only
        assert_eq!(get_all(d, "spork.favorite"), ["main", "feature/a.b+c"]);
        remove_local(d, "spork.favorite", "feature/a.b+c").unwrap();
        remove_local(d, "spork.favorite", "nope").unwrap();
        assert_eq!(get_all(d, "spork.favorite"), ["main"]);
    }
}
