//! Branch operations beyond create / checkout / delete (docs/specs/M04).

use std::path::Path;

use crate::{GitError, run};

/// Renames local branch `old` to `new` (M04 §4). With `on_remote`, also renames it on its
/// upstream's remote (push `new`, delete the old remote branch, track the new one); those steps
/// run only after the local rename succeeded, and are skipped when there is no upstream.
pub fn rename(repo: &Path, old: &str, new: &str, on_remote: bool) -> Result<(), GitError> {
    if run(repo, &["check-ref-format", "--branch", new]).is_err() {
        return Err(GitError::Other {
            code: None,
            stderr: format!("`{new}` is not a valid branch name."),
        });
    }
    // read before renaming; a remote of `.` is a local upstream
    let get = |key: &str| {
        run(repo, &["config", "--get", key])
            .ok()
            .map(|out| String::from_utf8_lossy(&out).trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    let remote = get(&format!("branch.{old}.remote")).filter(|r| r != ".");
    let remote_branch = get(&format!("branch.{old}.merge"))
        .and_then(|m| m.strip_prefix("refs/heads/").map(str::to_owned));
    run(repo, &["branch", "--move", old, new])?;
    if on_remote {
        let Some(remote) = remote else {
            return Ok(());
        };
        let Some(old_remote_branch) = remote_branch else {
            return Ok(());
        };
        run(
            repo,
            &[
                "push",
                &remote,
                &format!("refs/heads/{new}:refs/heads/{new}"),
            ],
        )?;
        run(repo, &["push", &remote, "--delete", &old_remote_branch])?;
        run(
            repo,
            &["branch", &format!("--set-upstream-to={remote}/{new}"), new],
        )?;
    }
    Ok(())
}

/// Deletes local `name` even though its commits aren't merged anywhere (`branch -D`, M04 §5);
/// they stay findable through the reflog for a while.
pub fn force_delete(repo: &Path, name: &str) -> Result<(), GitError> {
    run(repo, &["branch", "-D", name]).map(drop)
}

/// Git refused [`crate::delete_branch`] because the branch has commits merged nowhere else.
pub fn is_unmerged_refusal(e: &GitError) -> bool {
    matches!(e, GitError::Other { stderr, .. } if stderr.contains("not fully merged"))
}

/// Deletes a branch on its remote, e.g. `origin/feature/x` (M04 §6).
pub fn delete_remote(repo: &Path, remote_branch: &str) -> Result<(), GitError> {
    let Some((remote, branch)) = remote_branch.split_once('/') else {
        return Err(GitError::Other {
            code: None,
            stderr: format!("`{remote_branch}` is not a remote branch."),
        });
    };
    run(repo, &["push", remote, "--delete", branch]).map(drop)
}

/// Makes `branch` track `upstream`, e.g. `origin/main` (M03 §4).
pub fn set_upstream(repo: &Path, branch: &str, upstream: &str) -> Result<(), GitError> {
    run(
        repo,
        &["branch", &format!("--set-upstream-to={upstream}"), branch],
    )
    .map(drop)
}

/// The branches checked out lately, newest first, read from the last 50 checkouts in HEAD's
/// reflog as IntelliJ's "Recent" branches are; the caller keeps those that still exist.
pub fn recent_checkouts(repo: &Path) -> Vec<String> {
    let Ok(out) = run(
        repo,
        &[
            "reflog",
            "--max-count",
            "50",
            "--grep-reflog",
            "checkout:",
            "--format=%gs",
        ],
    ) else {
        // a repository without commits has no reflog yet
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    for line in String::from_utf8_lossy(&out).lines() {
        if let Some((_, name)) = line.rsplit_once(" to ")
            && !names.iter().any(|n| n == name)
        {
            names.push(name.to_owned());
        }
    }
    names
}

/// Checks out a commit or tag without a branch (detached HEAD, M06 §2).
pub fn checkout_detached(repo: &Path, rev: &str) -> Result<(), GitError> {
    run(repo, &["switch", "--detach", rev]).map(drop)
}

/// Switches to `branch` carrying local changes along (M04 §0 "Stash and reapply"): stash them
/// (untracked files too), switch, pop. `Conflicts` when they clash with the branch; git then
/// keeps the stash, so nothing is lost.
pub fn checkout_with_stash(repo: &Path, branch: &str) -> Result<crate::Outcome, GitError> {
    let message = format!("Spork: auto-stash before checkout {branch}");
    let before = crate::stashes(repo)?.len();
    run(
        repo,
        &[
            "stash",
            "push",
            "--include-untracked",
            "--message",
            &message,
        ],
    )?;
    let stashed = crate::stashes(repo)?.len() > before;
    if let Err(e) = crate::checkout(repo, branch) {
        if stashed {
            run(repo, &["stash", "pop"])?; // put everything back as it was
        }
        return Err(e);
    }
    if !stashed {
        return Ok(crate::Outcome::Done);
    }
    crate::allow_conflicts(repo, run(repo, &["stash", "pop"]))
}

/// Local branches whose commits are all in HEAD, so deleting them loses nothing; the current
/// branch and the usual long-lived ones (main, master, develop) are never offered.
pub fn merged(repo: &Path) -> Result<Vec<String>, GitError> {
    let out = run(
        repo,
        &[
            "branch",
            "--merged",
            "HEAD",
            "--format=%(refname:short)%00%(HEAD)",
        ],
    )?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|l| l.split_once('\0'))
        .filter(|(name, head)| *head != "*" && !matches!(*name, "main" | "master" | "develop"))
        .map(|(name, _)| name.to_owned())
        .collect())
}

/// Moves `branch` forward to its upstream when it only lacks commits (M04 §2 "Fast-forward to").
/// The current branch merges `--ff-only`; any other is updated without a checkout by pushing into
/// this repository, which git refuses unless it's a fast-forward.
pub fn fast_forward(repo: &Path, branch: &str) -> Result<(), GitError> {
    if crate::head_branch(repo)?.as_deref() == Some(branch) {
        return run(repo, &["merge", "--ff-only", "@{upstream}"]).map(drop);
    }
    let upstream = run(
        repo,
        &[
            "rev-parse",
            "--symbolic-full-name",
            &format!("{branch}@{{upstream}}"),
        ],
    )?;
    let upstream = String::from_utf8_lossy(&upstream).trim().to_owned();
    run(
        repo,
        &["push", ".", &format!("{upstream}:refs/heads/{branch}")],
    )
    .map(drop)
}

/// Where git keeps the index lock; it stays behind when a git command crashes (M01 §6).
pub fn index_lock(repo: &Path) -> Result<std::path::PathBuf, GitError> {
    let out = run(repo, &["rev-parse", "--git-path", "index.lock"])?;
    Ok(repo.join(String::from_utf8_lossy(&out).trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{bare_origin, repo};

    #[test]
    fn recent_checkouts_newest_first_once_each() {
        let tmp = repo();
        let d = tmp.path();
        run(d, &["commit", "--allow-empty", "-m", "one"]).unwrap();
        run(d, &["branch", "a"]).unwrap();
        run(d, &["branch", "fix/b"]).unwrap();
        for branch in ["a", "fix/b", "a", "main"] {
            run(d, &["checkout", "--quiet", branch]).unwrap();
        }
        assert_eq!(recent_checkouts(d), ["main", "a", "fix/b"]);
    }

    #[test]
    fn rename_local_and_remote() {
        let tmp = repo();
        let d = tmp.path();
        let remote_dir = tempfile::tempdir().unwrap();
        bare_origin(remote_dir.path(), d);

        run(d, &["commit", "--allow-empty", "-m", "one"]).unwrap();
        run(d, &["branch", "topic"]).unwrap();
        run(d, &["push", "--set-upstream", "origin", "topic"]).unwrap();

        assert!(rename(d, "topic", "bad name", false).is_err());

        rename(d, "topic", "topic2", true).unwrap();

        assert!(
            run(
                d,
                &["rev-parse", "--verify", "--quiet", "refs/heads/topic2"]
            )
            .is_ok()
        );
        assert!(run(d, &["rev-parse", "--verify", "--quiet", "refs/heads/topic"]).is_err());
        assert!(
            run(
                d,
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    "refs/remotes/origin/topic2"
                ]
            )
            .is_ok()
        );
        // git removes the tracking ref when push --delete succeeds
        assert!(
            run(
                d,
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    "refs/remotes/origin/topic"
                ]
            )
            .is_err()
        );
        let merge =
            String::from_utf8_lossy(&run(d, &["config", "--get", "branch.topic2.merge"]).unwrap())
                .trim()
                .to_owned();
        assert_eq!(merge, "refs/heads/topic2");

        // local-only: no upstream means no remote steps and no error
        run(d, &["branch", "solo"]).unwrap();
        rename(d, "solo", "solo2", true).unwrap();
    }

    #[test]
    fn delete_remote_upstream_detached() {
        let tmp = repo();
        let d = tmp.path();
        let remote_dir = tempfile::tempdir().unwrap();
        bare_origin(remote_dir.path(), d);

        run(d, &["commit", "--allow-empty", "-m", "one"]).unwrap();
        run(d, &["push", "origin", "main"]).unwrap();
        run(d, &["branch", "x"]).unwrap();
        run(d, &["push", "origin", "x"]).unwrap();

        set_upstream(d, "x", "origin/main").unwrap();
        let merge =
            String::from_utf8_lossy(&run(d, &["config", "--get", "branch.x.merge"]).unwrap())
                .trim()
                .to_owned();
        assert_eq!(merge, "refs/heads/main");

        assert!(delete_remote(d, "nonsense").is_err());
        delete_remote(d, "origin/x").unwrap();
        assert!(
            run(
                d,
                &["rev-parse", "--verify", "--quiet", "refs/remotes/origin/x"]
            )
            .is_err()
        );

        run(d, &["tag", "v1"]).unwrap();
        checkout_detached(d, "v1").unwrap();
        assert!(crate::head_branch(d).unwrap().is_none());
    }

    #[test]
    fn checkout_carries_local_changes() {
        let tmp = repo();
        let d = tmp.path();
        let write = |s: &str| std::fs::write(d.join("f.txt"), s).unwrap();
        let read = || std::fs::read_to_string(d.join("f.txt")).unwrap();
        write("base\n");
        crate::stage(d, &["f.txt"]).unwrap();
        crate::commit(d, "base", false).unwrap();
        run(d, &["switch", "-c", "other"]).unwrap();
        write("other\n");
        crate::stage(d, &["f.txt"]).unwrap();
        crate::commit(d, "other", false).unwrap();
        run(d, &["switch", "main"]).unwrap();

        // an untracked file travels along cleanly
        std::fs::write(d.join("new.txt"), "n").unwrap();
        assert_eq!(
            checkout_with_stash(d, "other").unwrap(),
            crate::Outcome::Done
        );
        assert!(d.join("new.txt").exists() && crate::stashes(d).unwrap().is_empty());

        // a clashing change stops with conflicts and the stash is kept
        run(d, &["switch", "main"]).unwrap();
        write("mine\n");
        assert!(crate::checkout(d, "other").is_err()); // plain checkout refuses
        assert_eq!(
            checkout_with_stash(d, "other").unwrap(),
            crate::Outcome::Conflicts
        );
        assert_eq!(crate::head_branch(d).unwrap().as_deref(), Some("other"));
        assert_eq!(crate::stashes(d).unwrap().len(), 1);
        assert!(read().contains("<<<<<<<"));

        assert!(index_lock(d).unwrap().ends_with(".git/index.lock"));
    }

    #[test]
    fn lists_merged_branches() {
        let tmp = repo();
        let d = tmp.path();
        run(d, &["commit", "--allow-empty", "-m", "base"]).unwrap();
        run(d, &["branch", "done"]).unwrap();
        run(d, &["branch", "develop"]).unwrap();
        run(d, &["switch", "-c", "wip"]).unwrap();
        run(d, &["commit", "--allow-empty", "-m", "unfinished"]).unwrap();
        run(d, &["switch", "main"]).unwrap();
        assert_eq!(merged(d).unwrap(), ["done"]);
    }

    #[test]
    fn fast_forwards_current_and_other_branches() {
        let (tmp, r) = (repo(), tempfile::tempdir().unwrap());
        let d = tmp.path();
        bare_origin(r.path(), d);
        run(d, &["commit", "--allow-empty", "-m", "one"]).unwrap();
        run(d, &["branch", "side"]).unwrap();
        run(d, &["push", "--set-upstream", "origin", "main", "side"]).unwrap();
        run(d, &["branch", "--set-upstream-to=origin/side", "side"]).unwrap();
        // the remote moves on; locally both branches are behind
        run(d, &["commit", "--allow-empty", "-m", "two"]).unwrap();
        run(d, &["push", "origin", "main", "HEAD:side"]).unwrap();
        run(d, &["reset", "--hard", "HEAD~1"]).unwrap();
        let at = |rev: &str| {
            String::from_utf8_lossy(&run(d, &["rev-parse", rev]).unwrap())
                .trim()
                .to_owned()
        };
        fast_forward(d, "main").unwrap();
        fast_forward(d, "side").unwrap();
        assert_eq!(at("main"), at("origin/main"));
        assert_eq!(at("side"), at("origin/side"));
        // a branch with its own commit can't be fast-forwarded
        run(d, &["switch", "side"]).unwrap();
        run(d, &["commit", "--allow-empty", "-m", "mine"]).unwrap();
        run(d, &["switch", "main"]).unwrap();
        run(d, &["push", "origin", "main:side", "--force"]).unwrap();
        run(d, &["fetch", "origin"]).unwrap();
        assert!(fast_forward(d, "side").is_err());
    }

    #[test]
    fn force_deletes_an_unmerged_branch() {
        let tmp = repo();
        let d = tmp.path();
        run(d, &["commit", "--allow-empty", "-m", "base"]).unwrap();
        run(d, &["switch", "-c", "wip"]).unwrap();
        run(d, &["commit", "--allow-empty", "-m", "only here"]).unwrap();
        run(d, &["switch", "main"]).unwrap();
        let refused = crate::delete_branch(d, "wip").unwrap_err();
        assert!(is_unmerged_refusal(&refused), "{refused:?}");
        force_delete(d, "wip").unwrap();
        assert!(run(d, &["rev-parse", "--verify", "--quiet", "refs/heads/wip"]).is_err());
    }
}
