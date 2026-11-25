//! Stash extras: snapshots, rename, branch from a stash, stash some files (docs/specs/M12 §1).

use std::path::Path;

use crate::{GitError, Outcome, allow_conflicts, run, run_with_input};

/// Saves local changes to tracked files as a stash without touching the working copy;
/// `false` when there was nothing to save.
pub fn snapshot(repo: &Path, message: &str) -> Result<bool, GitError> {
    let out = run(repo, &["stash", "create", message])?;
    let oid = String::from_utf8_lossy(&out).trim().to_owned();
    if oid.is_empty() {
        return Ok(false);
    }
    run(repo, &["stash", "store", "--message", message, &oid])?;
    Ok(true)
}

/// Renames stash `index`; it moves to the top of the list.
pub fn rename(repo: &Path, index: usize, message: &str) -> Result<(), GitError> {
    let at = format!("stash@{{{index}}}");
    let out = run(repo, &["rev-parse", &at])?;
    let oid = String::from_utf8_lossy(&out).trim().to_owned();
    run(repo, &["stash", "drop", &at])?;
    run(repo, &["stash", "store", "--message", message, &oid]).map(drop)
}

/// Creates `name` at the commit stash `index` was made on, checks it out and applies the stash there.
pub fn branch_from(repo: &Path, index: usize, name: &str) -> Result<(), GitError> {
    run(
        repo,
        &["stash", "branch", name, &format!("stash@{{{index}}}")],
    )
    .map(drop)
}

/// Stashes the staged and unstaged changes of `paths` only.
pub fn save_paths(repo: &Path, message: &str, paths: &[&str]) -> Result<(), GitError> {
    let mut args = vec!["--literal-pathspecs", "stash", "push"];
    if !message.trim().is_empty() {
        args.extend(["--message", message]);
    }
    args.push("--");
    args.extend(paths);
    run(repo, &args).map(drop)
}

/// Stashes the local changes as the reference app's Stash dialog does: `keep_index` leaves what is staged
/// in place, `untracked` takes new files too.
pub fn save(repo: &Path, message: &str, keep_index: bool, untracked: bool) -> Result<(), GitError> {
    let mut args = vec!["stash", "push"];
    if keep_index {
        args.push("--keep-index");
    }
    if untracked {
        args.push("--include-untracked");
    }
    if !message.trim().is_empty() {
        args.extend(["--message", message.trim()]);
    }
    run(repo, &args).map(drop)
}

/// Applies stash `index` as the Unstash dialog asks: `reinstate_index` stages again what was
/// staged; `pop` drops it after, unless it conflicts (git keeps it then).
pub fn unstash(
    repo: &Path,
    index: usize,
    pop: bool,
    reinstate_index: bool,
) -> Result<Outcome, GitError> {
    let name = format!("stash@{{{index}}}");
    let mut args = vec!["stash", if pop { "pop" } else { "apply" }];
    if reinstate_index {
        args.push("--index");
    }
    args.push(&name);
    allow_conflicts(repo, run(repo, &args))
}

/// Drops every stash.
pub fn clear(repo: &Path) -> Result<(), GitError> {
    run(repo, &["stash", "clear"]).map(drop)
}

/// The changes stash `id` holds for `path`, against the commit it was made on.
pub fn file_patch(repo: &Path, id: &str, path: &str) -> Result<String, GitError> {
    let base = format!("{id}^1");
    let out = run(repo, &["diff", "--binary", &base, id, "--", path])?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Brings the changes stash `id` holds for `path` into the working tree.
pub fn apply_file(repo: &Path, id: &str, path: &str) -> Result<(), GitError> {
    let patch = file_patch(repo, id, path)?;
    run_with_input(repo, &["apply", "-"], Some(patch.as_bytes())).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::repo;

    #[test]
    fn save_unstash_clear() {
        let tmp = repo();
        let d = tmp.path();
        let write = |f: &str, s: &str| std::fs::write(d.join(f), s).unwrap();
        let read = |f: &str| std::fs::read_to_string(d.join(f)).unwrap();
        write("a.txt", "1\n");
        crate::stage(d, &["a.txt"]).unwrap();
        crate::commit(d, "base", false).unwrap();
        write("a.txt", "2\n");
        crate::stage(d, &["a.txt"]).unwrap();
        write("new.txt", "n\n");
        save(d, "kept", true, false).unwrap();
        // the staged change stays, the untracked file too
        assert_eq!(read("a.txt"), "2\n");
        assert!(d.join("new.txt").exists());
        run(d, &["reset", "--hard", "--quiet"]).unwrap();
        let id = crate::stashes(d).unwrap()[0].id.clone();
        assert!(file_patch(d, &id, "a.txt").unwrap().contains("+2"));
        apply_file(d, &id, "a.txt").unwrap();
        assert_eq!(read("a.txt"), "2\n");
        run(d, &["checkout", "--", "a.txt"]).unwrap();

        assert_eq!(unstash(d, 0, true, true).unwrap(), Outcome::Done);
        assert!(
            crate::status(d)
                .unwrap()
                .iter()
                .any(|e| e.path == "a.txt" && e.has_staged())
        );
        save(d, "", false, true).unwrap();
        assert!(!d.join("new.txt").exists());
        clear(d).unwrap();
        assert!(crate::stashes(d).unwrap().is_empty());
    }

    #[test]
    fn snapshot_rename_branch_paths() {
        let tmp = repo();
        let d = tmp.path();
        let write = |f: &str, s: &str| std::fs::write(d.join(f), s).unwrap();
        let read = |f: &str| std::fs::read_to_string(d.join(f)).unwrap();
        let commit = |f: &str, s: &str, msg: &str| {
            write(f, s);
            crate::stage(d, &[f]).unwrap();
            crate::commit(d, msg, false).unwrap();
        };
        commit("a.txt", "1", "base");
        write("a.txt", "2");
        assert!(snapshot(d, "snap").unwrap());
        assert_eq!(
            (crate::stashes(d).unwrap().len(), read("a.txt").as_str()),
            (1, "2")
        );
        rename(d, 0, "renamed").unwrap();
        assert!(crate::stashes(d).unwrap()[0].message.contains("renamed"));
        run(d, &["checkout", "--", "a.txt"]).unwrap();
        assert!(!snapshot(d, "empty").unwrap());

        commit("b.txt", "b", "add b");
        write("a.txt", "3");
        write("b.txt", "bb");
        save_paths(d, "only b", &["b.txt"]).unwrap();
        assert_eq!((read("a.txt").as_str(), read("b.txt").as_str()), ("3", "b"));
        assert_eq!(crate::stashes(d).unwrap().len(), 2);

        run(d, &["checkout", "--", "a.txt"]).unwrap();
        branch_from(d, 0, "from-stash").unwrap();
        assert_eq!(
            crate::head_branch(d).unwrap().as_deref(),
            Some("from-stash")
        );
        assert_eq!(
            (read("b.txt").as_str(), crate::stashes(d).unwrap().len()),
            ("bb", 1)
        );
    }
}
