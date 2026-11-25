//! Git LFS through the `git lfs` command (docs/specs/M14).

use std::path::Path;

use crate::{GitError, run};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LfsFile {
    pub path: String,
    pub oid: String,
    /// The real content is in the working copy, not just the pointer file.
    pub downloaded: bool,
}

/// Whether the repository tracks anything with LFS (`filter=lfs` in its root `.gitattributes`).
pub fn is_used(repo: &Path) -> bool {
    std::fs::read_to_string(repo.join(".gitattributes")).is_ok_and(|s| s.contains("filter=lfs"))
}

pub fn installed(repo: &Path) -> bool {
    run(repo, &["lfs", "version"]).is_ok()
}

/// Downloads the current checkout's LFS objects from `remote` into the working copy.
pub fn pull(repo: &Path, remote: &str) -> Result<(), GitError> {
    run(repo, &["lfs", "pull", remote]).map(drop)
}

/// Downloads LFS objects from `remote` without touching the working copy.
pub fn fetch(repo: &Path, remote: &str) -> Result<(), GitError> {
    run(repo, &["lfs", "fetch", remote]).map(drop)
}

/// Stores files matching `pattern` with LFS from now on; stages `.gitattributes`.
pub fn track(repo: &Path, pattern: &str) -> Result<(), GitError> {
    run(repo, &["lfs", "track", "--", pattern])?;
    crate::stage(repo, &[".gitattributes"])
}

/// Stops storing files matching `pattern` with LFS; stages `.gitattributes`.
pub fn untrack(repo: &Path, pattern: &str) -> Result<(), GitError> {
    run(repo, &["lfs", "untrack", "--", pattern])?;
    crate::stage(repo, &[".gitattributes"])
}

/// Parses `git lfs ls-files --long`: `<oid> <*|-> <path>`, `*` = downloaded, `-` = pointer only.
pub fn parse_files(out: &str) -> Vec<LfsFile> {
    out.lines()
        .filter_map(|line| {
            let (oid, rest) = line.split_once(' ')?;
            let (mark, path) = rest.split_once(' ')?;
            Some(LfsFile {
                path: path.to_owned(),
                oid: oid.to_owned(),
                downloaded: match mark {
                    "*" => true,
                    "-" => false,
                    _ => return None,
                },
            })
        })
        .collect()
}

/// Files stored with LFS in the current checkout.
pub fn files(repo: &Path) -> Result<Vec<LfsFile>, GitError> {
    Ok(parse_files(&String::from_utf8_lossy(&run(
        repo,
        &["lfs", "ls-files", "--long"],
    )?)))
}

/// Locks `path` on the LFS server so others can't push changes to it.
pub fn lock(repo: &Path, path: &str) -> Result<(), GitError> {
    run(repo, &["lfs", "lock", "--", path]).map(drop)
}

/// Releases a lock; `force` releases someone else's.
pub fn unlock(repo: &Path, path: &str, force: bool) -> Result<(), GitError> {
    let mut args = vec!["lfs", "unlock"];
    if force {
        args.push("--force");
    }
    args.extend(["--", path]);
    run(repo, &args).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::repo;

    #[test]
    fn parses_ls_files() {
        let files = parse_files("abc123 * assets/big file.psd\ndef456 - video.mp4\nbad\n");
        assert_eq!(files.len(), 2);
        assert_eq!(
            (
                files[0].path.as_str(),
                files[0].oid.as_str(),
                files[0].downloaded
            ),
            ("assets/big file.psd", "abc123", true)
        );
        assert_eq!(
            (files[1].path.as_str(), files[1].downloaded),
            ("video.mp4", false)
        );
    }

    #[test]
    fn track_and_untrack() {
        let tmp = repo();
        let d = tmp.path();
        if !installed(d) {
            eprintln!("git-lfs not installed, skipping");
            return;
        }
        assert!(!is_used(d));
        track(d, "*.psd").unwrap();
        assert!(is_used(d));
        assert!(
            crate::status(d)
                .unwrap()
                .iter()
                .any(|e| e.path == ".gitattributes" && e.has_staged())
        );
        untrack(d, "*.psd").unwrap();
        assert!(!is_used(d));
    }
}
