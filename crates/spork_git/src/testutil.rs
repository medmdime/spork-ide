//! Scratch repositories for tests.

use std::path::Path;

/// A scratch repository on `main` with the commit identity tests need (no signing).
pub fn repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    crate::init(tmp.path()).unwrap();
    for (k, v) in [
        ("user.name", "Spork Test"),
        ("user.email", "test@spork.invalid"),
        ("commit.gpgsign", "false"),
        ("tag.gpgsign", "false"),
    ] {
        crate::run(tmp.path(), &["config", k, v]).unwrap();
    }
    // `crate::init` honours `init.defaultBranch`; force `main` regardless
    crate::run(tmp.path(), &["symbolic-ref", "HEAD", "refs/heads/main"]).unwrap();
    tmp
}

/// A bare repository next to `repo`, added to it as `origin`.
pub fn bare_origin(tmp: &Path, repo: &Path) {
    let bare = tmp.join("origin.git");
    crate::run(tmp, &["init", "--quiet", "--bare", bare.to_str().unwrap()]).unwrap();
    crate::run(repo, &["remote", "add", "origin", bare.to_str().unwrap()]).unwrap();
}
