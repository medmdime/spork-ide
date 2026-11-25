//! Ignore files: suggested patterns and where they are written (docs/specs/M07 §6).

use std::path::{Path, PathBuf};

use crate::{GitError, run};

/// Patterns offered for ignoring `path` (repository-relative, `/`-separated): the file itself
/// (`/dir/name.ext`, anchored at the root), every file with its extension (`*.ext`), and its folder (`/dir/`).
pub fn suggestions(path: &str) -> Vec<String> {
    let mut out = vec![format!("/{path}")];
    let name = path.rsplit('/').next().unwrap_or(path);
    // an extension: a dot in the file name that isn't its first character (so `.env` has none)
    if let Some((stem, ext)) = name.rsplit_once('.')
        && !stem.is_empty()
        && !ext.is_empty()
    {
        out.push(format!("*.{ext}"));
    }
    if let Some((dir, _)) = path.rsplit_once('/')
        && !dir.is_empty()
    {
        out.push(format!("/{dir}/"));
    }
    out
}

/// Appends `pattern` to the root `.gitignore` (shared with everyone), or with `private` to
/// `.git/info/exclude` (this clone only). Returns the file written.
pub fn add(repo: &Path, pattern: &str, private: bool) -> Result<PathBuf, GitError> {
    let fail = |stderr: String| GitError::Other { code: None, stderr };
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return Err(fail("Enter a pattern.".into()));
    }
    let target = if private {
        // `--git-path` answers relative to the repository, not to Spork's working directory
        let out = run(repo, &["rev-parse", "--git-path", "info/exclude"])?;
        let exclude = repo.join(String::from_utf8_lossy(&out).trim());
        if let Some(dir) = exclude.parent() {
            std::fs::create_dir_all(dir).map_err(|e| fail(e.to_string()))?;
        }
        exclude
    } else {
        repo.join(".gitignore")
    };
    let mut content = std::fs::read_to_string(&target).unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(pattern);
    content.push('\n');
    std::fs::write(&target, content).map_err(|e| fail(e.to_string()))?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::repo;

    #[test]
    fn suggests_patterns() {
        assert_eq!(
            suggestions("src/app/main.rs"),
            vec!["/src/app/main.rs", "*.rs", "/src/app/"]
        );
        assert_eq!(suggestions("Makefile"), vec!["/Makefile"]);
        assert_eq!(suggestions(".env"), vec!["/.env"]);
        assert_eq!(suggestions("é.txt"), vec!["/é.txt", "*.txt"]);
    }

    #[test]
    fn adds_patterns() {
        let tmp = repo();
        let d = tmp.path();
        std::fs::write(d.join(".gitignore"), "target").unwrap();
        add(d, "*.log", false).unwrap();
        assert_eq!(
            std::fs::read_to_string(d.join(".gitignore")).unwrap(),
            "target\n*.log\n"
        );
        let p = add(d, " secret.txt ", true).unwrap();
        assert!(p.ends_with("info/exclude"), "{p:?}");
        assert!(p.starts_with(d), "written inside the repository: {p:?}");
        assert!(
            std::fs::read_to_string(&p)
                .unwrap()
                .ends_with("secret.txt\n")
        );
        assert!(add(d, "  ", false).is_err());
    }
}
