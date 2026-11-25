//! Remotes: list, add, edit, remove, fetch one, tags and pull requests on them (docs/specs/M09).

use std::path::Path;

use crate::{GitError, run};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    /// Fetch URL.
    pub url: String,
}

/// Parses `git config --get-regexp ^remote\..*\.url$` output (`remote.<name>.url <url>` lines;
/// names may contain dots).
pub fn parse_remotes(out: &str) -> Vec<Remote> {
    out.lines()
        .filter_map(|line| {
            let (key, url) = line.split_once(' ')?;
            let name = key.strip_prefix("remote.")?.strip_suffix(".url")?;
            Some(Remote {
                name: name.to_owned(),
                url: url.to_owned(),
            })
        })
        .collect()
}

/// The repository's remotes in config order; empty when there are none.
pub fn remotes(repo: &Path) -> Vec<Remote> {
    run(repo, &["config", "--get-regexp", r"^remote\..*\.url$"])
        .map(|out| parse_remotes(&String::from_utf8_lossy(&out)))
        .unwrap_or_default()
}

pub fn add(repo: &Path, name: &str, url: &str) -> Result<(), GitError> {
    let (name, url) = (name.trim(), url.trim());
    if name.is_empty() || url.is_empty() {
        return Err(GitError::Other {
            code: None,
            stderr: "Enter a name and a URL.".into(),
        });
    }
    run(repo, &["remote", "add", name, url]).map(drop)
}

pub fn set_url(repo: &Path, name: &str, url: &str) -> Result<(), GitError> {
    run(repo, &["remote", "set-url", name, url.trim()]).map(drop)
}

/// The push URL set apart from the fetch URL; `None` when pushes go to the fetch URL.
pub fn push_url(repo: &Path, name: &str) -> Option<String> {
    let out = run(
        repo,
        &["config", "--get", &format!("remote.{name}.pushurl")],
    )
    .ok()?;
    Some(String::from_utf8_lossy(&out).trim().to_owned()).filter(|u| !u.is_empty())
}

/// Saves Edit Remote: a new name (its remote branches follow), the fetch URL, and a push URL
/// (empty: push to the fetch URL).
pub fn edit(
    repo: &Path,
    name: &str,
    new_name: &str,
    url: &str,
    push: &str,
) -> Result<(), GitError> {
    let (new_name, push) = (new_name.trim(), push.trim());
    if new_name.is_empty() || url.trim().is_empty() {
        return Err(GitError::Other {
            code: None,
            stderr: "Enter a name and a URL.".into(),
        });
    }
    if new_name != name {
        run(repo, &["remote", "rename", name, new_name])?;
    }
    set_url(repo, new_name, url)?;
    if !push.is_empty() {
        run(repo, &["remote", "set-url", "--push", new_name, push])?;
    } else if push_url(repo, new_name).is_some() {
        run(
            repo,
            &[
                "config",
                "--unset-all",
                &format!("remote.{new_name}.pushurl"),
            ],
        )?;
    }
    Ok(())
}

/// Asks the remote for its branches: whether its URL and the credentials work. Returns how many
/// branches it has.
pub fn test_connection(repo: &Path, name: &str) -> Result<usize, GitError> {
    let out = run(repo, &["ls-remote", "--heads", name])?;
    Ok(String::from_utf8_lossy(&out).lines().count())
}

/// Forgets the remote and its remote branches; nothing changes on the server.
pub fn remove(repo: &Path, name: &str) -> Result<(), GitError> {
    run(repo, &["remote", "remove", name]).map(drop)
}

pub fn fetch(repo: &Path, name: &str) -> Result<(), GitError> {
    run(repo, &["fetch", "--prune", name]).map(drop)
}

/// Deletes `tag` on remote `name`; the local tag stays (M12 §2).
pub fn delete_tag(repo: &Path, name: &str, tag: &str) -> Result<(), GitError> {
    run(
        repo,
        &["push", name, "--delete", &format!("refs/tags/{tag}")],
    )
    .map(drop)
}

/// Pushes every local tag to remote `name` (M09 §5).
pub fn push_tags(repo: &Path, name: &str) -> Result<(), GitError> {
    run(repo, &["push", name, "--tags"]).map(drop)
}

/// Pushes local `branch` to remote `name` under the same name and tracks it (M09 §3).
pub fn push_branch(repo: &Path, name: &str, branch: &str) -> Result<(), GitError> {
    let spec = format!("refs/heads/{branch}:refs/heads/{branch}");
    run(repo, &["push", "--set-upstream", name, &spec]).map(drop)
}

/// How "Push to…" pushes (M09 §3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PushOptions {
    /// Make the pushed branch the local branch's upstream (`--set-upstream`).
    pub set_upstream: bool,
    /// Also push all tags (`--tags`).
    pub tags: bool,
    /// Overwrite the remote branch, but only if it is still where our last fetch saw it.
    pub force_with_lease: bool,
    /// Overwrite the remote branch whatever it holds (`--force`).
    pub force: bool,
    /// Also push the annotated tags the pushed commits reach (`--follow-tags`).
    pub follow_tags: bool,
}

/// Pushes local `branch` to `dest` on remote `name`.
pub fn push_to(
    repo: &Path,
    name: &str,
    branch: &str,
    dest: &str,
    opts: PushOptions,
) -> Result<(), GitError> {
    let mut args = vec!["push"];
    if opts.set_upstream {
        args.push("--set-upstream");
    }
    if opts.tags {
        args.push("--tags");
    }
    if opts.follow_tags {
        args.push("--follow-tags");
    }
    if opts.force {
        args.push("--force");
    }
    // the lease names the destination ref, which may differ from the local branch's name
    let lease = format!("--force-with-lease=refs/heads/{dest}");
    if opts.force_with_lease {
        args.push(&lease);
    }
    let spec = format!("refs/heads/{branch}:refs/heads/{}", dest.trim());
    args.extend([name, spec.as_str()]);
    run(repo, &args).map(drop)
}

/// What a push of local `branch` to `dest` on remote `name` sends, newest first, and whether
/// `dest` is there already (as the last fetch saw it): the commits it lacks, or for a new branch
/// those on none of the remote's branches.
pub fn outgoing(
    repo: &Path,
    name: &str,
    branch: &str,
    dest: &str,
) -> Result<(Vec<crate::Commit>, bool), GitError> {
    let tracking = format!("refs/remotes/{name}/{}", dest.trim());
    let exists = run(repo, &["rev-parse", "--verify", "--quiet", &tracking]).is_ok();
    let local = format!("refs/heads/{branch}");
    let range = format!("{tracking}..{local}");
    let not_on_remote = format!("--remotes={name}");
    let revs: &[&str] = match exists {
        true => &[&range],
        false => &[&local, "--not", &not_on_remote],
    };
    let out = run(
        repo,
        &[&["log", "-z", crate::LOG_FORMAT][..], revs, &["--"]].concat(),
    )?;
    Ok((crate::parse_log(&String::from_utf8_lossy(&out)), exists))
}

/// Pushes `commit` and everything before it to branch `dest` on remote `name` (Push All up to
/// Here).
pub fn push_commit(repo: &Path, name: &str, commit: &str, dest: &str) -> Result<(), GitError> {
    let spec = format!("{commit}:refs/heads/{}", dest.trim());
    run(repo, &["push", name, spec.as_str()]).map(drop)
}

/// Fetches a pull / merge request's `source` ref (e.g. `refs/pull/7/head`) from remote `name`
/// into local `branch` and checks it out (M15 §3).
pub fn checkout_pull_request(
    repo: &Path,
    name: &str,
    source: &str,
    branch: &str,
) -> Result<(), GitError> {
    // git won't move the checked-out branch from a fetch, so update that one by fast-forward
    if crate::head_branch(repo)?.as_deref() == Some(branch) {
        run(repo, &["fetch", name, source])?;
        return run(repo, &["merge", "--ff-only", "FETCH_HEAD"]).map(drop);
    }
    // `+`: the author may have force-pushed since the last checkout
    run(
        repo,
        &["fetch", name, &format!("+{source}:refs/heads/{branch}")],
    )?;
    crate::checkout(repo, branch)
}

/// The branch remote `name`'s HEAD points at (its default branch), if git knows it.
pub fn default_branch(repo: &Path, name: &str) -> Option<String> {
    let out = run(
        repo,
        &[
            "symbolic-ref",
            "--short",
            &format!("refs/remotes/{name}/HEAD"),
        ],
    )
    .ok()?;
    let full = String::from_utf8_lossy(&out).trim().to_owned();
    Some(full.strip_prefix(&format!("{name}/"))?.to_owned())
}

/// The web page of a remote URL, for "Open in browser": `git@host:o/r.git`,
/// `ssh://git@host:22/o/r.git` and `https://user@host/o/r.git` all give `https://host/o/r`.
/// `None` for local paths and `file://` URLs.
pub fn web_url(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    let without_user = |authority_and_path: &str| -> String {
        match authority_and_path.split_once('/') {
            Some((auth, path)) => {
                let host = auth.rsplit_once('@').map_or(auth, |(_, h)| h);
                format!("{host}/{path}")
            }
            None => authority_and_path.to_owned(),
        }
    };
    for scheme in ["https://", "http://"] {
        if let Some(rest) = url.strip_prefix(scheme) {
            return Some(format!("{scheme}{}", without_user(rest)));
        }
    }
    if let Some(rest) = url.strip_prefix("ssh://") {
        let rest = without_user(rest);
        let (host, path) = rest.split_once('/')?;
        let host = host.split_once(':').map_or(host, |(h, _)| h);
        return Some(format!("https://{host}/{path}"));
    }
    // scp-like `[user@]host:path`; a drive letter (`C:\…`) or `/` before `:` means a local path
    let (host, path) = url.split_once(':')?;
    if host.contains('/') || host.contains('\\') || host.len() < 2 || path.starts_with('/') {
        return None;
    }
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    Some(format!("https://{host}/{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{bare_origin, repo};

    #[test]
    fn web_urls() {
        for (url, web) in [
            ("git@github.com:o/r.git", Some("https://github.com/o/r")),
            ("ssh://git@host:22/o/r.git", Some("https://host/o/r")),
            ("https://user@host/o/r.git", Some("https://host/o/r")),
            ("https://github.com/o/r", Some("https://github.com/o/r")),
            ("/tmp/x.git", None),
            ("file:///tmp/x.git", None),
            ("C:\\repos\\x", None),
        ] {
            assert_eq!(web_url(url).as_deref(), web, "{url}");
        }
    }

    #[test]
    fn parses_names_with_dots() {
        let r =
            parse_remotes("remote.origin.url https://a/b.git\nremote.my.fork.url git@h:o/r.git\n");
        assert_eq!(r.len(), 2);
        assert_eq!(
            (r[1].name.as_str(), r[1].url.as_str()),
            ("my.fork", "git@h:o/r.git")
        );
    }

    #[test]
    fn remotes_tags_branches_and_pull_requests() {
        let (tmp, r) = (repo(), tempfile::tempdir().unwrap());
        let d = tmp.path();
        bare_origin(r.path(), d);
        let bare = r.path().join("origin.git");
        let exists = |rev: &str| run(d, &["rev-parse", "--verify", "--quiet", rev]).is_ok();
        run(d, &["commit", "--allow-empty", "-m", "one"]).unwrap();
        run(d, &["push", "origin", "main"]).unwrap();

        // add / fetch / edit / remove
        add(d, "backup", bare.to_str().unwrap()).unwrap();
        assert!(add(d, " ", "x").is_err());
        let names: Vec<String> = remotes(d).into_iter().map(|r| r.name).collect();
        assert_eq!(names, ["origin", "backup"]);
        fetch(d, "backup").unwrap();
        assert!(exists("refs/remotes/backup/main"));
        set_url(d, "backup", "https://example.invalid/x.git").unwrap();
        assert_eq!(remotes(d)[1].url, "https://example.invalid/x.git");
        set_url(d, "backup", bare.to_str().unwrap()).unwrap();
        assert_eq!(test_connection(d, "backup").unwrap(), 1);
        edit(
            d,
            "backup",
            "mirror",
            bare.to_str().unwrap(),
            "https://p.invalid/x.git",
        )
        .unwrap();
        assert!(exists("refs/remotes/mirror/main") && !exists("refs/remotes/backup/main"));
        assert_eq!(
            push_url(d, "mirror").as_deref(),
            Some("https://p.invalid/x.git")
        );
        edit(d, "mirror", "mirror", bare.to_str().unwrap(), " ").unwrap();
        assert_eq!(push_url(d, "mirror"), None);
        assert!(edit(d, "mirror", "", "x", "").is_err());
        remove(d, "mirror").unwrap();
        assert_eq!(remotes(d).len(), 1);
        assert!(!exists("refs/remotes/mirror/main"));

        // tags
        run(d, &["tag", "v1"]).unwrap();
        run(d, &["tag", "v2"]).unwrap();
        push_tags(d, "origin").unwrap();
        let remote_tags = || {
            String::from_utf8_lossy(&run(d, &["ls-remote", "--tags", "origin"]).unwrap())
                .into_owned()
        };
        assert!(remote_tags().contains("refs/tags/v1") && remote_tags().contains("refs/tags/v2"));
        delete_tag(d, "origin", "v1").unwrap();
        assert!(!remote_tags().contains("refs/tags/v1"));
        assert!(exists("refs/tags/v1"));

        // push a branch, default branch
        run(d, &["branch", "topic"]).unwrap();
        push_branch(d, "origin", "topic").unwrap();
        assert!(exists("refs/remotes/origin/topic"));
        run(d, &["remote", "set-head", "origin", "main"]).unwrap();
        assert_eq!(default_branch(d, "origin").as_deref(), Some("main"));
        assert_eq!(default_branch(d, "nope"), None);

        // push to another name, then overwrite it only with the lease
        let opts = PushOptions {
            set_upstream: true,
            ..Default::default()
        };
        push_to(d, "origin", "topic", "renamed", opts).unwrap();
        assert!(exists("refs/remotes/origin/renamed"));
        run(d, &["switch", "topic"]).unwrap();
        run(
            d,
            &["commit", "--allow-empty", "--amend", "-m", "rewritten"],
        )
        .unwrap();
        assert!(push_to(d, "origin", "topic", "renamed", PushOptions::default()).is_err());
        let force = PushOptions {
            force_with_lease: true,
            ..Default::default()
        };
        push_to(d, "origin", "topic", "renamed", force).unwrap();
        run(d, &["switch", "main"]).unwrap();

        // what a push sends: the commits the remote branch lacks, all of a new branch's own
        run(d, &["commit", "--allow-empty", "-m", "two"]).unwrap();
        let (commits, exists) = outgoing(d, "origin", "main", "main").unwrap();
        let subjects: Vec<&str> = commits.iter().map(|c| c.subject.as_str()).collect();
        assert_eq!((subjects, exists), (vec!["two"], true));
        let (commits, exists) = outgoing(d, "origin", "main", "fresh").unwrap();
        assert_eq!((commits.len(), exists), (1, false));

        // GitHub keeps pull requests under refs/pull/<n>/head
        run(
            &bare,
            &["update-ref", "refs/pull/7/head", "refs/heads/main"],
        )
        .unwrap();
        checkout_pull_request(d, "origin", "refs/pull/7/head", "pr/7").unwrap();
        assert_eq!(crate::head_branch(d).unwrap().as_deref(), Some("pr/7"));
        checkout_pull_request(d, "origin", "refs/pull/7/head", "pr/7").unwrap(); // again, while on it
    }
}
