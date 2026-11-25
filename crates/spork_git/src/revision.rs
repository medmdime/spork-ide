//! Actions on one commit from the history (docs/specs/M06 §2, M04 §7).

use std::path::{Path, PathBuf};

use crate::{GitError, run, run_with_input};

/// How `reset` treats the changes between the old and the new position (M04 §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    /// Keep them, staged.
    Soft,
    /// Keep them, unstaged.
    Mixed,
    /// Throw them away, uncommitted changes included.
    Hard,
    /// Throw them away, but keep uncommitted changes (git refuses if they'd be overwritten).
    Keep,
}

/// The commit `expr` names (a branch, tag, hash, `HEAD~2`), and its subject; an error when it
/// names none.
pub fn resolve_commit(repo: &Path, expr: &str) -> Result<(String, String), GitError> {
    let invalid = || GitError::Other {
        code: None,
        stderr: "The specified commit expression did not pass validation.".into(),
    };
    let spec = format!("{expr}^{{commit}}");
    let out = run(
        repo,
        &[
            "log",
            "-1",
            "--format=%H%x00%s",
            "--end-of-options",
            &spec,
            "--",
        ],
    )
    .map_err(|_| invalid())?;
    let out = String::from_utf8_lossy(&out);
    let (id, subject) = out.trim_end().split_once('\0').ok_or_else(invalid)?;
    Ok((id.to_owned(), subject.to_owned()))
}

/// Moves the current branch to `target`.
pub fn reset(repo: &Path, target: &str, mode: ResetMode) -> Result<(), GitError> {
    let flag = match mode {
        ResetMode::Soft => "--soft",
        ResetMode::Mixed => "--mixed",
        ResetMode::Hard => "--hard",
        ResetMode::Keep => "--keep",
    };
    run(repo, &["reset", flag, target]).map(drop)
}

/// Replaces HEAD's message; staged changes stay out of the commit (`--only`).
pub fn reword_head(repo: &Path, message: &str) -> Result<(), GitError> {
    run_with_input(
        repo,
        // an empty commit stays empty: only its message changes
        &[
            "commit",
            "--amend",
            "--only",
            "--allow-empty",
            "--cleanup=strip",
            "-F",
            "-",
        ],
        Some(message.as_bytes()),
    )
    .map(drop)
}

/// What differs between `commit` and the working tree, as a patch (Compare with Local).
pub fn diff_with_local(repo: &Path, commit: &str) -> Result<String, GitError> {
    let out = run(
        repo,
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--find-renames",
            commit,
        ],
    )?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Writes `commit` as a mailbox patch into `dir`; returns the file.
pub fn save_patch(repo: &Path, commit: &str, dir: &Path) -> Result<PathBuf, GitError> {
    let out = run(
        repo,
        &["format-patch", "-1", commit, "-o", &dir.to_string_lossy()],
    )?;
    Ok(PathBuf::from(String::from_utf8_lossy(&out).trim()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagInfo {
    /// Annotated tags carry a tagger, date and message; lightweight ones are just a name.
    pub annotated: bool,
    pub tagger: String,
    pub date: String,
    /// The commit the tag points at.
    pub target: String,
    pub message: String,
}

/// Who made tag `name`, when, on which commit, and its message (M12 §2 Details).
pub fn tag_info(repo: &Path, name: &str) -> Result<TagInfo, GitError> {
    let format = "--format=%(objecttype)%00%(taggername) %(taggeremail)%00%(taggerdate:iso)%00%(*objectname)%00%(objectname)%00%(contents)";
    let out = run(
        repo,
        &["for-each-ref", format, &format!("refs/tags/{name}")],
    )?;
    let text = String::from_utf8_lossy(&out);
    let f: Vec<&str> = text.splitn(6, '\0').collect();
    let [kind, tagger, date, peeled, object, message] = f[..] else {
        return Err(GitError::Other {
            code: None,
            stderr: format!("No tag named {name}."),
        });
    };
    let annotated = kind == "tag";
    Ok(TagInfo {
        annotated,
        tagger: tagger.trim().to_owned(),
        date: date.to_owned(),
        // an annotated tag is its own object; its commit is the peeled one
        target: if peeled.is_empty() { object } else { peeled }.to_owned(),
        // for a lightweight tag, `contents` is the commit's message, not the tag's
        message: if annotated {
            message.trim().to_owned()
        } else {
            String::new()
        },
    })
}

/// The latest `n` commit messages on HEAD, newest first (the commit box's recent messages, M07 §5).
pub fn recent_messages(repo: &Path, n: usize) -> Vec<String> {
    let max = format!("--max-count={n}");
    let Ok(out) = run(repo, &["log", &max, "--format=%B%x00", "HEAD", "--"]) else {
        return Vec::new(); // no commits yet
    };
    String::from_utf8_lossy(&out)
        .split('\0')
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Cherry-picks `commit` with options (M06 §2): `record_origin` adds "(cherry picked from …)"
/// to the message (`-x`); `no_commit` leaves the changes staged instead of committing them.
pub fn cherry_pick_with(
    repo: &Path,
    commit: &str,
    record_origin: bool,
    no_commit: bool,
) -> Result<crate::Outcome, GitError> {
    let mut args = vec!["cherry-pick"];
    if record_origin {
        args.push("-x");
    }
    if no_commit {
        args.push("--no-commit");
    }
    let parents = run(repo, &["rev-list", "--parents", "-n", "1", commit])?;
    // merge commits are picked against their first parent
    if String::from_utf8_lossy(&parents).split_whitespace().count() > 2 {
        args.extend(["-m", "1"]);
    }
    args.push(commit);
    crate::allow_conflicts(repo, run(repo, &args))
}

/// The commits from `oldest` to `newest`, both included, oldest first: what two selected commits
/// span (M06 §1). `None` unless `oldest` is an ancestor of `newest`, so the run is one line.
pub fn run_between(repo: &Path, oldest: &str, newest: &str) -> Option<Vec<String>> {
    run(repo, &["merge-base", "--is-ancestor", oldest, newest]).ok()?;
    let range = format!("{oldest}..{newest}");
    let out = run(repo, &["rev-list", "--reverse", "--ancestry-path", &range]).ok()?;
    let ids = String::from_utf8_lossy(&out);
    let oldest = String::from_utf8_lossy(&run(repo, &["rev-parse", oldest]).ok()?)
        .trim()
        .to_owned();
    Some(
        std::iter::once(oldest)
            .chain(ids.lines().map(str::to_owned))
            .collect(),
    )
}

/// Cherry-picks `commits` in order onto HEAD; merges among them against their first parent.
pub fn cherry_pick_many(repo: &Path, commits: &[String]) -> Result<crate::Outcome, GitError> {
    let mut args = vec!["cherry-pick"];
    if commits.iter().any(|c| crate::is_merge_commit(repo, c)) {
        // git accepts -m on the plain commits too (2.21+)
        args.extend(["-m", "1"]);
    }
    args.extend(commits.iter().map(String::as_str));
    crate::allow_conflicts(repo, run(repo, &args))
}

/// Reverts `commits` (oldest first), newest first so each revert applies cleanly.
pub fn revert_many(repo: &Path, commits: &[String]) -> Result<crate::Outcome, GitError> {
    let mut args = vec!["revert", "--no-edit"];
    if commits.iter().any(|c| crate::is_merge_commit(repo, c)) {
        args.extend(["-m", "1"]);
    }
    args.extend(commits.iter().rev().map(String::as_str));
    crate::allow_conflicts(repo, run(repo, &args))
}

/// Writes `commits` (a [`run_between`] run, oldest first) as numbered mailbox patches into `dir`.
pub fn save_patches(repo: &Path, commits: &[String], dir: &Path) -> Result<usize, GitError> {
    let (Some(oldest), Some(newest)) = (commits.first(), commits.last()) else {
        return Ok(0);
    };
    let range = format!("{oldest}^..{newest}");
    let dir = dir.to_string_lossy();
    // a root commit has no parent to start from: `--root` takes history from the start
    let args = if run(
        repo,
        &["rev-parse", "--verify", "--quiet", &format!("{oldest}^")],
    )
    .is_ok()
    {
        vec!["format-patch", "-o", &dir, range.as_str()]
    } else {
        vec!["format-patch", "-o", &dir, "--root", newest.as_str()]
    };
    let out = run(repo, &args)?;
    Ok(String::from_utf8_lossy(&out).lines().count())
}

/// Points local `branch` (not the current one) at `commit` (M04 §7 "move"). Its old commits
/// stay reachable from the reflog.
pub fn move_branch(repo: &Path, branch: &str, commit: &str) -> Result<(), GitError> {
    run(repo, &["branch", "--force", branch, commit]).map(drop)
}

/// Subjects of the commits a release `tag` adds since the tag before it (all of history when it
/// is the first), oldest first: the input of release notes.
pub fn release_subjects(repo: &Path, tag: &str) -> Result<Vec<String>, GitError> {
    let previous = run(
        repo,
        &["describe", "--tags", "--abbrev=0", &format!("{tag}^")],
    )
    .ok();
    let range = match previous {
        Some(out) => format!("{}..{tag}", String::from_utf8_lossy(&out).trim()),
        None => tag.to_owned(),
    };
    let out = run(
        repo,
        &[
            "log",
            "--reverse",
            "--no-merges",
            "--format=%s",
            &range,
            "--",
        ],
    )?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .map(str::to_owned)
        .collect())
}

/// The last change to undo, as git's reflog describes it ("commit: Fix login", "rebase (finish):
/// …", "checkout: moving from a to b"), and whether Spork can undo it. A checkout is HEAD's own
/// last move; anything else is read from the current branch's reflog, which has one entry per
/// rebase or merge where HEAD's has one per step.
pub fn last_change(repo: &Path) -> Option<(String, bool)> {
    let last = |args: &[&str]| {
        run(repo, args)
            .ok()
            .map(|o| String::from_utf8_lossy(&o).trim().to_owned())
            .filter(|s| !s.is_empty())
    };
    let head = last(&["reflog", "-1", "--format=%gs", "HEAD"])?;
    if head.starts_with("checkout:") {
        return Some((head, true));
    }
    let Ok(Some(branch)) = crate::head_branch(repo) else {
        return Some((head, false)); // detached: no branch to move back
    };
    let what = last(&[
        "reflog",
        "-1",
        "--format=%gs",
        &format!("refs/heads/{branch}"),
    ])?;
    let kind = what.split(':').next().unwrap_or("");
    // the actions a branch move can take back; others (branch created, renamed…) aren't offered
    let undoable = [
        "commit",
        "reset",
        "merge",
        "rebase",
        "cherry-pick",
        "revert",
        "pull",
    ]
    .iter()
    .any(|k| kind == *k || kind.starts_with(&format!("{k} ")));
    Some((what, undoable))
}

/// Takes back [`last_change`]: switches back after a checkout, else moves the current branch to
/// where it was before (`reset --keep @{1}`, which refuses rather than lose uncommitted changes).
pub fn undo_last(repo: &Path) -> Result<String, GitError> {
    let fail = |msg: &str| GitError::Other {
        code: None,
        stderr: msg.to_owned(),
    };
    let (what, undoable) = last_change(repo).ok_or_else(|| fail("Nothing to undo yet."))?;
    if !undoable {
        return Err(fail(&format!("Spork can't undo \u{201c}{what}\u{201d}.")));
    }
    if what.starts_with("checkout:") {
        run(repo, &["switch", "-"])?;
    } else {
        run(repo, &["reset", "--keep", "@{1}"])?;
    }
    Ok(format!("Undid \u{201c}{what}\u{201d}"))
}

/// What `head` changes since it left `base` (`git diff base...head`), for PR descriptions.
pub fn branch_diff(repo: &Path, base: &str, head: &str) -> Result<String, GitError> {
    let range = format!("{base}...{head}");
    let out = run(
        repo,
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--find-renames",
            &range,
        ],
    )?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Subjects of the commits on `head` that `base` doesn't have, oldest first.
pub fn branch_subjects(repo: &Path, base: &str, head: &str) -> Result<Vec<String>, GitError> {
    let out = run(
        repo,
        &[
            "log",
            "--reverse",
            "--format=%s",
            &format!("{base}..{head}"),
            "--",
        ],
    )?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .map(str::to_owned)
        .collect())
}

/// Takes the changes `commit` made to `paths` back out of the work tree (IntelliJ's Revert
/// Selected Changes); a merge is read against its first parent. Git refuses, changing nothing,
/// when the files have moved on since.
pub fn revert_paths(repo: &Path, commit: &str, paths: &[String]) -> Result<(), GitError> {
    let patch = paths_patch(repo, commit, paths)?;
    run_with_input(
        repo,
        &["apply", "--reverse", "--whitespace=nowarn", "-"],
        Some(&patch),
    )
    .map(drop)
}

/// Applies the changes `commit` made to `paths` to the work tree (IntelliJ's Cherry-Pick
/// Selected Changes); like [`revert_paths`], git refuses, changing nothing, when they don't fit.
pub fn apply_paths(repo: &Path, commit: &str, paths: &[String]) -> Result<(), GitError> {
    let patch = paths_patch(repo, commit, paths)?;
    run_with_input(repo, &["apply", "--whitespace=nowarn", "-"], Some(&patch)).map(drop)
}

/// The changes `commit` made to `paths`, as a binary patch; a merge's against its first parent.
pub fn paths_patch(repo: &Path, commit: &str, paths: &[String]) -> Result<Vec<u8>, GitError> {
    let literal: Vec<String> = paths.iter().map(|p| crate::literal(p)).collect();
    let mut args = vec![
        "diff-tree",
        "-p",
        "--binary",
        "--root",
        "-m",
        "--first-parent",
        "--no-commit-id",
        commit,
        "--",
    ];
    args.extend(literal.iter().map(String::as_str));
    run(repo, &args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::repo;

    #[test]
    fn resolves_a_commit_and_refuses_an_option() {
        let tmp = repo();
        let d = tmp.path();
        std::fs::write(d.join("a"), "one\n").unwrap();
        run(d, &["add", "."]).unwrap();
        run(d, &["commit", "-m", "first"]).unwrap();
        let (id, subject) = resolve_commit(d, "HEAD").unwrap();
        assert_eq!((id.len(), subject.as_str()), (40, "first"));
        assert!(resolve_commit(d, "--all").is_err());
        assert!(resolve_commit(d, "nothing-here").is_err());
    }

    #[test]
    fn reverts_one_file_of_a_commit() {
        let tmp = repo();
        let d = tmp.path();
        std::fs::write(d.join("a"), "one\n").unwrap();
        std::fs::write(d.join("b"), "one\n").unwrap();
        run(d, &["add", "."]).unwrap();
        run(d, &["commit", "-m", "first"]).unwrap();
        std::fs::write(d.join("a"), "two\n").unwrap();
        std::fs::write(d.join("b"), "two\n").unwrap();
        run(d, &["commit", "-am", "second"]).unwrap();
        revert_paths(d, "HEAD", &["a".into()]).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a")).unwrap(), "one\n");
        assert_eq!(std::fs::read_to_string(d.join("b")).unwrap(), "two\n");
        // and applied again: Cherry-Pick Selected Changes
        apply_paths(d, "HEAD", &["a".into()]).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a")).unwrap(), "two\n");
        assert!(apply_paths(d, "HEAD", &["a".into()]).is_err()); // already there
    }

    #[test]
    fn reset_reword_save_patch() {
        let tmp = repo();
        let d = tmp.path();
        let write = |f: &str, s: &str| std::fs::write(d.join(f), s).unwrap();
        let only = |staged: bool| {
            let st = crate::status(d).unwrap();
            st.len() == 1 && st[0].has_staged() == staged && st[0].has_unstaged() != staged
        };
        write("a.txt", "one\n");
        crate::stage(d, &["a.txt"]).unwrap();
        crate::commit(d, "first", false).unwrap();
        write("a.txt", "two\n");
        crate::stage(d, &["a.txt"]).unwrap();
        crate::commit(d, "second", false).unwrap();

        reset(d, "HEAD~1", ResetMode::Soft).unwrap();
        assert!(only(true));
        assert_eq!(crate::head_message(d).as_deref(), Some("first"));
        crate::commit(d, "second again", false).unwrap();
        reset(d, "HEAD~1", ResetMode::Mixed).unwrap();
        assert!(only(false));
        reset(d, "HEAD", ResetMode::Hard).unwrap();
        assert!(crate::status(d).unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "one\n");

        write("b.txt", "x");
        crate::stage(d, &["b.txt"]).unwrap();
        reword_head(d, "renamed").unwrap();
        assert_eq!(crate::head_message(d).as_deref(), Some("renamed"));
        assert!(only(true)); // b.txt is still staged, not in the commit

        assert_eq!(recent_messages(d, 5), ["renamed"]); // "first" was reworded in place

        let head = String::from_utf8_lossy(&run(d, &["rev-parse", "HEAD"]).unwrap())
            .trim()
            .to_owned();
        run(d, &["tag", "light"]).unwrap();
        run(d, &["tag", "-a", "v1", "-m", "First release"]).unwrap();
        let light = tag_info(d, "light").unwrap();
        assert!(!light.annotated && light.target == head && light.message.is_empty());
        let v1 = tag_info(d, "v1").unwrap();
        assert!(v1.annotated && v1.target == head && v1.message == "First release");
        assert!(v1.tagger.starts_with("Spork Test <test@spork.invalid>"));
        assert!(tag_info(d, "nope").is_err());

        run(d, &["switch", "-c", "topic"]).unwrap();
        write("c.txt", "c");
        crate::stage(d, &["c.txt"]).unwrap();
        crate::commit(d, "add c", false).unwrap();
        assert_eq!(branch_subjects(d, "main", "topic").unwrap(), ["add c"]);
        assert!(branch_diff(d, "main", "topic").unwrap().contains("+c"));
        run(d, &["switch", "main"]).unwrap();

        // cherry-pick with -x, and without committing; move a branch
        let topic = String::from_utf8_lossy(&run(d, &["rev-parse", "topic"]).unwrap())
            .trim()
            .to_owned();
        assert_eq!(
            cherry_pick_with(d, &topic, true, false).unwrap(),
            crate::Outcome::Done
        );
        assert!(
            crate::head_message(d)
                .unwrap()
                .contains("(cherry picked from commit")
        );
        reset(d, "HEAD~1", ResetMode::Hard).unwrap();
        assert_eq!(
            cherry_pick_with(d, &topic, false, true).unwrap(),
            crate::Outcome::Done
        );
        assert!(
            crate::status(d)
                .unwrap()
                .iter()
                .any(|e| e.path == "c.txt" && e.has_staged())
        );
        reset(d, "HEAD", ResetMode::Hard).unwrap();
        move_branch(d, "topic", "HEAD").unwrap();
        assert_eq!(
            recent_messages(d, 1),
            [
                String::from_utf8_lossy(&run(d, &["log", "-1", "--format=%B", "topic"]).unwrap())
                    .trim()
                    .to_owned()
            ]
        );
        assert!(move_branch(d, "main", "topic").is_err()); // the current branch can't be moved

        let out = tempfile::tempdir().unwrap();
        let file = save_patch(d, "HEAD", out.path()).unwrap();
        assert!(
            std::fs::read_to_string(file)
                .unwrap()
                .contains("Subject: [PATCH] renamed")
        );

        // release notes cover the commits since the previous tag
        run(d, &["tag", "r1"]).unwrap();
        run(d, &["commit", "--allow-empty", "-m", "feature one"]).unwrap();
        run(d, &["commit", "--allow-empty", "-m", "fix two"]).unwrap();
        run(d, &["tag", "r2"]).unwrap();
        assert_eq!(
            release_subjects(d, "r2").unwrap(),
            ["feature one", "fix two"]
        );
        assert!(!release_subjects(d, "r1").unwrap().is_empty()); // first tag: all of history
    }

    #[test]
    fn undoes_the_last_change() {
        let tmp = repo();
        let d = tmp.path();
        let head = || {
            String::from_utf8_lossy(&run(d, &["rev-parse", "HEAD"]).unwrap())
                .trim()
                .to_owned()
        };
        std::fs::write(d.join("a.txt"), "1").unwrap();
        crate::stage(d, &["a.txt"]).unwrap();
        crate::commit(d, "one", false).unwrap();
        let one = head();
        crate::commit(d, "amended", true).unwrap();
        assert!(last_change(d).unwrap().0.starts_with("commit (amend)"));
        undo_last(d).unwrap();
        assert_eq!(head(), one); // the amend is taken back

        // a checkout is undone by switching back
        run(d, &["switch", "-c", "other"]).unwrap();
        undo_last(d).unwrap();
        assert_eq!(crate::head_branch(d).unwrap().as_deref(), Some("main"));

        // a rebase is undone in one step: the branch goes back to before it
        run(d, &["switch", "-c", "topic"]).unwrap();
        run(d, &["commit", "--allow-empty", "-m", "on topic"]).unwrap();
        run(d, &["switch", "main"]).unwrap();
        run(d, &["commit", "--allow-empty", "-m", "on main"]).unwrap();
        run(d, &["switch", "topic"]).unwrap();
        let before = head();
        run(d, &["rebase", "main"]).unwrap();
        assert_ne!(head(), before);
        assert!(last_change(d).unwrap().0.starts_with("rebase"));
        undo_last(d).unwrap();
        assert_eq!(head(), before);
        run(d, &["switch", "main"]).unwrap();

        // uncommitted changes in the way make it refuse instead of losing them
        std::fs::write(d.join("a.txt"), "2").unwrap();
        crate::stage(d, &["a.txt"]).unwrap();
        crate::commit(d, "two", false).unwrap();
        std::fs::write(d.join("a.txt"), "dirty").unwrap();
        assert!(undo_last(d).is_err());
        assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "dirty");
    }

    #[test]
    fn several_commits() {
        let tmp = repo();
        let d = tmp.path();
        let id = |rev: &str| {
            String::from_utf8_lossy(&run(d, &["rev-parse", rev]).unwrap())
                .trim()
                .to_owned()
        };
        let commit = |f: &str| {
            std::fs::write(d.join(f), f).unwrap();
            crate::stage(d, &[f]).unwrap();
            crate::commit(d, f, false).unwrap();
        };
        commit("base");
        run(d, &["switch", "-c", "topic"]).unwrap();
        commit("a");
        commit("b");
        commit("c");
        let (a, c) = (id("topic~2"), id("topic"));
        let picked = run_between(d, &a, &c).unwrap();
        assert_eq!(picked, [a.clone(), id("topic~1"), c.clone()]);
        assert!(run_between(d, &c, &a).is_none()); // newest first is not a run

        let patches = tmp.path().join("patches");
        assert_eq!(save_patches(d, &picked, &patches).unwrap(), 3);
        // the root commit has no parent: the run starts from history's beginning
        assert_eq!(
            save_patches(d, &run_between(d, &id("main"), &a).unwrap(), &patches).unwrap(),
            2
        );

        run(d, &["switch", "main"]).unwrap();
        assert_eq!(cherry_pick_many(d, &picked).unwrap(), crate::Outcome::Done);
        assert_eq!(recent_messages(d, 3), ["c", "b", "a"]);
        assert_eq!(revert_many(d, &picked).unwrap(), crate::Outcome::Done);
        assert!(!d.join("a").exists() && !d.join("c").exists());
    }
}
