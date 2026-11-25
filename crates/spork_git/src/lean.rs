//! Lean branching (GitHub flow): branches start from the main branch and finish by merging back
//! into it with a merge commit (docs/specs/M13 §4).

use std::path::Path;

use crate::{GitError, MergeKind, Outcome, run};

/// The branch work starts from and finishes into: the remote's default branch (`origin/HEAD`),
/// else a local `main`, else `master`.
pub fn main_branch(repo: &Path) -> Option<String> {
    let remote_head = run(
        repo,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    );
    if let Ok(out) = remote_head
        && let Some(branch) = String::from_utf8_lossy(&out).trim().strip_prefix("origin/")
    {
        return Some(branch.to_owned());
    }
    ["main", "master"]
        .into_iter()
        .find(|b| {
            run(
                repo,
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("refs/heads/{b}"),
                ],
            )
            .is_ok()
        })
        .map(str::to_owned)
}

/// Switches to `main`, merges `branch` with a merge commit, and deletes `branch` when asked and
/// the merge went through. Conflicts pause the merge on `main` for resolving.
pub fn finish(repo: &Path, branch: &str, main: &str, delete: bool) -> Result<Outcome, GitError> {
    crate::checkout(repo, main)?;
    let outcome = crate::merge_with(repo, branch, MergeKind::Commit, true)?;
    if outcome == Outcome::Done && delete {
        crate::delete_branch(repo, branch)?;
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::repo;

    #[test]
    fn finishes_into_main() {
        let tmp = repo();
        let d = tmp.path();
        run(d, &["commit", "--allow-empty", "-m", "base"]).unwrap();
        assert_eq!(main_branch(d).as_deref(), Some("main"));
        run(d, &["switch", "-c", "feature/x"]).unwrap();
        run(d, &["commit", "--allow-empty", "-m", "work"]).unwrap();

        assert_eq!(finish(d, "feature/x", "main", true).unwrap(), Outcome::Done);
        assert_eq!(crate::head_branch(d).unwrap().as_deref(), Some("main"));
        // a merge commit even though main could have fast-forwarded, and the branch is gone
        assert!(run(d, &["rev-parse", "--verify", "--quiet", "HEAD^2"]).is_ok());
        assert!(
            run(
                d,
                &["rev-parse", "--verify", "--quiet", "refs/heads/feature/x"]
            )
            .is_err()
        );
    }
}
