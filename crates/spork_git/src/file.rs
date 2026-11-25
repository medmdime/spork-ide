//! One file through time: history and blame (docs/specs/M08 §5–6).

use std::path::Path;

use crate::{Commit, GitError, run};

/// Every file in `rev`'s tree, sorted by path (the commit's File Tree, M06 §1).
pub fn tree(repo: &Path, rev: &str) -> Result<Vec<String>, GitError> {
    let out = run(repo, &["ls-tree", "-r", "-z", "--name-only", rev])?;
    Ok(String::from_utf8_lossy(&out)
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect())
}

/// A line that contains what was searched for: its file, its number (from 1), its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub path: String,
    pub line: u32,
    pub text: String,
}

/// The lines of the work tree's files (tracked, and untracked but not ignored) that contain
/// `text`, ignoring case, at most `max`; binary files are skipped.
pub fn grep(repo: &Path, text: &str, max: usize) -> Result<Vec<Match>, GitError> {
    let out = match run(
        repo,
        &[
            "grep",
            "-z",
            "-n",
            "-I",
            "-i",
            "-F",
            "--untracked",
            "-e",
            text,
        ],
    ) {
        Ok(out) => out,
        // nothing found
        Err(GitError::Other { code: Some(1), .. }) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    // `-z`: path NUL line NUL text, one match a line
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|l| {
            let mut parts = l.splitn(3, '\0');
            let (path, line, text) = (parts.next()?, parts.next()?, parts.next()?);
            Some(Match {
                path: path.to_owned(),
                line: line.parse().ok()?,
                text: text.to_owned(),
            })
        })
        .take(max)
        .collect())
}

/// Puts `path` back as it was in `rev`, in the working copy only; the change then shows in
/// Local Changes to commit or discard (M08 §5).
pub fn restore(repo: &Path, path: &str, rev: &str) -> Result<(), GitError> {
    run(
        repo,
        &["restore", "--source", rev, "--", &crate::literal(path)],
    )
    .map(drop)
}

/// Commits that changed `path`, newest first, following renames.
pub fn history(repo: &Path, path: &str, max: usize) -> Result<Vec<Commit>, GitError> {
    history_from(repo, "HEAD", path, max)
}

/// [`history`] back from `rev` rather than HEAD.
pub fn history_from(
    repo: &Path,
    rev: &str,
    path: &str,
    max: usize,
) -> Result<Vec<Commit>, GitError> {
    let max = format!("--max-count={max}");
    let out = run(
        repo,
        &[
            "log",
            "-z",
            "--follow",
            &max,
            crate::LOG_FORMAT,
            rev,
            "--",
            path,
        ],
    )?;
    Ok(crate::parse_log(&String::from_utf8_lossy(&out)))
}

/// The change `commit` made to `path`, as a patch.
// shortcut: commits from before a rename show no diff; record each commit's old path (--name-status) when that matters.
pub fn diff_at(
    repo: &Path,
    commit: &str,
    path: &str,
    ignore_whitespace: bool,
) -> Result<String, GitError> {
    let mut args = vec!["show", "--format=", "--find-renames", "--no-color"];
    if ignore_whitespace {
        args.push("--ignore-all-space");
    }
    args.extend([commit, "--", path]);
    let out = run(repo, &args)?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// How `path` changed from `rev` to the work tree, as a patch (IntelliJ's Compare with Local).
pub fn diff_with_local(repo: &Path, rev: &str, path: &str) -> Result<String, GitError> {
    let out = run(
        repo,
        &[
            "diff",
            "--find-renames",
            "--no-color",
            rev,
            "--",
            &crate::literal(path),
        ],
    )?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// `text` (the clipboard's) against `path`'s working copy, as a patch (Compare with Clipboard).
pub fn diff_text_with_file(repo: &Path, text: &str, path: &str) -> Result<String, GitError> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let dir = std::env::temp_dir().join(format!("spork-clipboard-{}", std::process::id()));
    let copy = dir.join(name);
    std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(&copy, text))
        .map_err(|e| GitError::Other {
            code: None,
            stderr: e.to_string(),
        })?;
    let copy = copy.to_string_lossy();
    let out = crate::run_allowing_diff(
        repo,
        &["diff", "--no-color", "--no-index", "--", &copy, path],
    )?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// A file's bytes at `spec`: `HEAD:path`, `:path` (the index) or `<commit>:path` (image diffs, M08 §3).
pub fn blob(repo: &Path, spec: &str) -> Result<Vec<u8>, GitError> {
    run(repo, &["cat-file", "blob", spec])
}

/// Opens the user's configured diff tool (`diff.tool`) on `path`'s local changes, staged or not;
/// returns when the tool closes (M08 §7).
pub fn external_diff(repo: &Path, path: &str, staged: bool) -> Result<(), GitError> {
    let mut args = vec!["difftool", "--no-prompt"];
    if staged {
        args.push("--cached");
    }
    args.extend(["--", path]);
    run(repo, &args).map(drop)
}

/// Opens the user's configured merge tool (`merge.tool`) on conflicted `path` (M11 §5); git
/// marks it resolved when the tool reports success.
pub fn merge_tool(repo: &Path, path: &str) -> Result<(), GitError> {
    run(repo, &["mergetool", "--no-prompt", "--", path]).map(drop)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    pub commit: String,
    pub author: String,
    /// Author time, Unix seconds.
    pub time: i64,
    pub summary: String,
    /// 1-based line number in the blamed file.
    pub line: usize,
    pub text: String,
}

/// Parses `git blame --line-porcelain` (every line repeats its commit's header).
pub fn parse_blame(out: &str) -> Vec<BlameLine> {
    let mut lines = Vec::new();
    let mut cur: Option<BlameLine> = None;
    for l in out.lines() {
        if let Some(text) = l.strip_prefix('\t') {
            if let Some(c) = &mut cur {
                c.text = text.to_owned();
                lines.push(c.clone());
            }
        } else if let Some(v) = l.strip_prefix("author ") {
            cur.iter_mut().for_each(|c| c.author = v.to_owned());
        } else if let Some(v) = l.strip_prefix("author-time ") {
            cur.iter_mut().for_each(|c| c.time = v.parse().unwrap_or(0));
        } else if let Some(v) = l.strip_prefix("summary ") {
            cur.iter_mut().for_each(|c| c.summary = v.to_owned());
        } else {
            let mut f = l.split(' ');
            let (hash, line) = (
                f.next().unwrap_or(""),
                f.nth(1).and_then(|n| n.parse().ok()),
            );
            if let (true, Some(line)) = (
                matches!(hash.len(), 40 | 64) && hash.bytes().all(|b| b.is_ascii_hexdigit()),
                line,
            ) {
                cur = Some(BlameLine {
                    commit: hash.to_owned(),
                    author: String::new(),
                    time: 0,
                    summary: String::new(),
                    line,
                    text: String::new(),
                });
            }
        }
    }
    lines
}

/// Who last changed each line of `path` at `rev` (default: the working copy); moves and copies detected.
pub fn blame(repo: &Path, rev: Option<&str>, path: &str) -> Result<Vec<BlameLine>, GitError> {
    let mut args = vec!["blame", "--line-porcelain", "-M", "-C"];
    args.extend(rev);
    args.extend(["--", path]);
    Ok(parse_blame(&String::from_utf8_lossy(&run(repo, &args)?)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::repo;

    #[test]
    fn greps_tracked_and_new_files_but_not_ignored_ones() {
        let tmp = repo();
        let d = tmp.path();
        std::fs::write(d.join("a.txt"), "one\nNeedle here\n").unwrap();
        std::fs::write(d.join(".gitignore"), "skip.txt\n").unwrap();
        crate::run(d, &["add", "a.txt", ".gitignore"]).unwrap();
        std::fs::write(d.join("new.txt"), "a needle\n").unwrap();
        std::fs::write(d.join("skip.txt"), "needle\n").unwrap();
        let mut found = grep(d, "needle", 10).unwrap();
        found.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(
            found,
            [
                Match {
                    path: "a.txt".into(),
                    line: 2,
                    text: "Needle here".into()
                },
                Match {
                    path: "new.txt".into(),
                    line: 1,
                    text: "a needle".into()
                },
            ]
        );
        assert!(grep(d, "absent", 10).unwrap().is_empty());
    }

    #[test]
    fn parses_line_porcelain() {
        let a = "a".repeat(40);
        let header = |n: usize| {
            format!(
                "{a} {n} {n}\nauthor Ann\nauthor-mail <a@x>\nauthor-time 100\nauthor-tz +0000\n\
                 committer Ann\nsummary first\nfilename f.txt\n"
            )
        };
        let out = format!("{}\tone\n{}\ttwo\n", header(1), header(2));
        let lines = parse_blame(&out);
        assert_eq!(lines.len(), 2);
        assert_eq!((lines[0].line, lines[0].text.as_str()), (1, "one"));
        assert_eq!((lines[1].line, lines[1].text.as_str()), (2, "two"));
        assert_eq!(
            (
                lines[1].author.as_str(),
                lines[1].time,
                lines[1].summary.as_str()
            ),
            ("Ann", 100, "first")
        );
    }

    #[test]
    fn history_follows_renames_and_blames() {
        let tmp = repo();
        let d = tmp.path();
        let commit = |file: &str, text: &str, msg: &str| {
            std::fs::write(d.join(file), text).unwrap();
            crate::stage(d, &[file]).unwrap();
            crate::commit(d, msg, false).unwrap();
        };
        commit("a.txt", "one\n", "add a");
        run(d, &["mv", "a.txt", "b.txt"]).unwrap();
        crate::commit(d, "rename", false).unwrap();
        commit("b.txt", "one\ntwo\n", "edit b");

        let h = history(d, "b.txt", 100).unwrap();
        let subjects: Vec<&str> = h.iter().map(|c| c.subject.as_str()).collect();
        assert_eq!(subjects, ["edit b", "rename", "add a"]);
        assert!(
            diff_at(d, &h[0].id, "b.txt", false)
                .unwrap()
                .contains("+two")
        );
        assert_eq!(blob(d, "HEAD:b.txt").unwrap(), b"one\ntwo\n");
        assert!(blob(d, "HEAD:a.txt").is_err()); // renamed away
        let b = blame(d, None, "b.txt").unwrap();
        let got: Vec<(&str, &str)> = b
            .iter()
            .map(|l| (l.summary.as_str(), l.text.as_str()))
            .collect();
        assert_eq!(got, [("add a", "one"), ("edit b", "two")]);
        // an indentation-only change disappears when whitespace is ignored
        commit("b.txt", "  one\ntwo\n", "indent");
        let indent = history(d, "b.txt", 1).unwrap().remove(0);
        assert!(
            diff_at(d, &indent.id, "b.txt", false)
                .unwrap()
                .contains("+  one")
        );
        assert!(
            !diff_at(d, &indent.id, "b.txt", true)
                .unwrap()
                .contains("+  one")
        );

        // a configured tool runs on the change: `true` stands in for one that exits cleanly
        run(d, &["config", "diff.tool", "fake"]).unwrap();
        run(d, &["config", "difftool.fake.cmd", "true"]).unwrap();
        std::fs::write(d.join("b.txt"), "changed\n").unwrap();
        external_diff(d, "b.txt", false).unwrap();
    }

    #[test]
    fn restores_an_older_version() {
        let tmp = repo();
        let d = tmp.path();
        for v in ["one\n", "two\n"] {
            std::fs::write(d.join("f.txt"), v).unwrap();
            crate::stage(d, &["f.txt"]).unwrap();
            crate::commit(d, v.trim(), false).unwrap();
        }
        restore(d, "f.txt", "HEAD~1").unwrap();
        assert_eq!(std::fs::read_to_string(d.join("f.txt")).unwrap(), "one\n");
        assert_eq!(blob(d, "HEAD~1:f.txt").unwrap(), b"one\n");
    }

    #[test]
    fn lists_the_tree_at_a_commit() {
        let tmp = repo();
        let d = tmp.path();
        std::fs::create_dir(d.join("src")).unwrap();
        std::fs::write(d.join("src/a b.rs"), "x").unwrap();
        std::fs::write(d.join("README"), "x").unwrap();
        crate::stage(d, &["src/a b.rs", "README"]).unwrap();
        crate::commit(d, "files", false).unwrap();
        std::fs::remove_file(d.join("README")).unwrap();
        crate::stage(d, &["README"]).unwrap();
        crate::commit(d, "drop readme", false).unwrap();
        assert_eq!(tree(d, "HEAD~1").unwrap(), ["README", "src/a b.rs"]);
        assert_eq!(tree(d, "HEAD").unwrap(), ["src/a b.rs"]);
    }
}
