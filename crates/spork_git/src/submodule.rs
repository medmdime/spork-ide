//! Add, sync and remove submodules; worktrees on a new branch (docs/specs/M12 §3–4).

use std::path::Path;

use crate::{GitError, run};

/// Adds the repository at `url` as a submodule in `path` and checks it out, recursively.
pub fn add(repo: &Path, url: &str, path: &str) -> Result<(), GitError> {
    run(repo, &["submodule", "add", "--", url.trim(), path.trim()])?;
    run(
        repo,
        &[
            "submodule",
            "update",
            "--init",
            "--recursive",
            "--",
            path.trim(),
        ],
    )
    .map(drop)
}

/// Checks out the commit this repository records for submodule `path` (initializing it if needed).
pub fn update(repo: &Path, path: &str) -> Result<(), GitError> {
    run(repo, &["submodule", "update", "--init", "--", path]).map(drop)
}

/// Copies submodule URLs from `.gitmodules` into the repository's config.
pub fn sync(repo: &Path) -> Result<(), GitError> {
    run(repo, &["submodule", "sync", "--recursive"]).map(drop)
}

/// Removes submodule `path`: its folder, its entry (staged) and its data under `.git/modules`.
pub fn remove(repo: &Path, path: &str) -> Result<(), GitError> {
    run(repo, &["submodule", "deinit", "--force", "--", path])?;
    run(repo, &["rm", "--force", "--", path])?;
    let out = run(repo, &["rev-parse", "--absolute-git-dir"])?;
    let git_dir = String::from_utf8_lossy(&out).trim().to_owned();
    match std::fs::remove_dir_all(Path::new(&git_dir).join("modules").join(path)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(GitError::Other {
            code: None,
            stderr: e.to_string(),
        }),
        _ => Ok(()),
    }
}

/// What moving submodule `path` changes: git's `--submodule=log` summary, one line per commit
/// gained (`>`) or lost (`<`), unstaged or staged (M08 §4).
pub fn change_log(repo: &Path, path: &str, staged: bool) -> Result<String, GitError> {
    let mut args = vec!["diff", "--submodule=log"];
    if staged {
        args.push("--cached");
    }
    args.extend(["--", path]);
    Ok(String::from_utf8_lossy(&run(repo, &args)?).into_owned())
}

/// Creates `branch` at `start` and checks it out in a new worktree at `path`.
pub fn add_worktree_new_branch(
    repo: &Path,
    path: &Path,
    branch: &str,
    start: &str,
) -> Result<(), GitError> {
    run(
        repo,
        &[
            "worktree",
            "add",
            "-b",
            branch,
            &path.to_string_lossy(),
            start,
        ],
    )
    .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::repo;

    #[test]
    fn sync_remove_and_worktree() {
        let (lib_tmp, app_tmp) = (repo(), repo());
        let (lib, d) = (lib_tmp.path(), app_tmp.path());
        run(lib, &["commit", "--allow-empty", "-m", "lib"]).unwrap();
        run(d, &["commit", "--allow-empty", "-m", "app"]).unwrap();
        // local file:// submodules need an explicit opt-in; `add` itself is these same two commands
        let lib_path = lib.to_str().unwrap();
        run(
            d,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "--",
                lib_path,
                "vendor/lib",
            ],
        )
        .unwrap();
        assert_eq!(crate::submodules(d).unwrap()[0].path, "vendor/lib");
        crate::commit(d, "add lib", false).unwrap();
        // the submodule moves on: the parent sees the commits it gained
        let sub = d.join("vendor/lib");
        // the clone doesn't inherit the test identity; CI machines may have no global one
        for (k, v) in [
            ("user.name", "Spork Test"),
            ("user.email", "test@spork.invalid"),
            ("commit.gpgsign", "false"),
        ] {
            run(&sub, &["config", k, v]).unwrap();
        }
        run(&sub, &["commit", "--allow-empty", "-m", "lib two"]).unwrap();
        assert!(
            change_log(d, "vendor/lib", false)
                .unwrap()
                .contains("> lib two")
        );
        update(d, "vendor/lib").unwrap(); // back to the recorded commit
        assert!(change_log(d, "vendor/lib", false).unwrap().is_empty());
        sync(d).unwrap();
        remove(d, "vendor/lib").unwrap();
        crate::commit(d, "remove lib", false).unwrap();
        assert!(crate::submodules(d).unwrap().is_empty());
        assert!(!d.join(".git/modules/vendor/lib").exists());

        let wt_tmp = tempfile::tempdir().unwrap();
        let wt = wt_tmp.path().join("wt");
        add_worktree_new_branch(d, &wt, "topic", "main").unwrap();
        assert_eq!(crate::head_branch(&wt).unwrap().as_deref(), Some("topic"));
    }
}
