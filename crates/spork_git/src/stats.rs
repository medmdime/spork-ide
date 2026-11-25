//! Repository statistics: who committed how much, and when (docs/specs/M16 §3).

use std::path::Path;

use crate::{GitError, run};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Author {
    pub name: String,
    pub email: String,
    pub commits: usize,
    pub added: usize,
    pub deleted: usize,
    /// Unix seconds of their first and latest commit.
    pub first: i64,
    pub last: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    /// Most commits first; ties by name.
    pub authors: Vec<Author>,
    /// Commit time (Unix seconds) of every commit, newest first, for activity charts.
    pub times: Vec<i64>,
}

/// Parses `git log --no-merges --numstat --format=%x01%aN%x00%aE%x00%at` output.
pub fn parse(out: &str) -> Stats {
    use std::collections::HashMap;

    let mut authors: HashMap<String, Author> = HashMap::new();
    let mut times = Vec::new();
    for part in out.split('\u{1}') {
        // the first line is `name\0email\0time`; anything else means a malformed part
        let mut lines = part.lines();
        let Some(header) = lines.next() else { continue };
        let mut f = header.split('\0');
        let name = f.next();
        let email = f.next();
        let time = f.next().and_then(|t| t.parse::<i64>().ok());
        let (Some(name), Some(email), Some(time)) = (name, email, time) else {
            continue;
        };
        let author = authors
            .entry(email.to_lowercase())
            .or_insert_with(|| Author {
                name: name.to_owned(),
                email: email.to_owned(),
                commits: 0,
                added: 0,
                deleted: 0,
                first: time,
                last: time,
            });
        author.commits += 1;
        author.first = author.first.min(time);
        author.last = author.last.max(time);
        // the other non-empty lines: `added<TAB>deleted<TAB>path`, `-` for binary files
        for line in lines.filter(|l| !l.trim().is_empty()) {
            let mut n = line.split('\t');
            let (Some(added), Some(deleted)) = (n.next(), n.next()) else {
                continue;
            };
            author.added += count(added);
            author.deleted += count(deleted);
        }
        times.push(time);
    }
    let mut authors: Vec<Author> = authors.into_values().collect();
    authors.sort_by(|a, b| b.commits.cmp(&a.commits).then_with(|| a.name.cmp(&b.name)));
    Stats { authors, times }
}

/// A numstat count: `-` (binary file) counts as 0, and so does anything unparsable.
fn count(s: &str) -> usize {
    s.parse().unwrap_or(0)
}

/// Statistics of the current branch's history (merges left out; `.mailmap` applied by git).
pub fn stats(repo: &Path) -> Result<Stats, GitError> {
    let out = match run(
        repo,
        &[
            "log",
            "--no-merges",
            "--numstat",
            "--format=%x01%aN%x00%aE%x00%at",
            "HEAD",
            "--",
        ],
    ) {
        Ok(out) => out,
        // a repository without commits: `git log HEAD` fails, and so does verifying HEAD
        Err(e) => {
            if run(repo, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_err() {
                return Ok(Stats::default());
            }
            return Err(e);
        }
    };
    Ok(parse(&String::from_utf8_lossy(&out)))
}

/// Facts about a repository for its details panel (M02 §3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Details {
    /// Commits reachable from HEAD.
    pub commits: usize,
    /// Size of git's object database (loose and packed), in bytes.
    pub size: u64,
    /// The first commit's date (`YYYY-MM-DD`), if there is one.
    pub first_commit: Option<String>,
    pub branches: usize,
    pub tags: usize,
}

pub fn details(repo: &Path) -> Result<Details, GitError> {
    let text =
        |args: &[&str]| run(repo, args).map(|o| String::from_utf8_lossy(&o).trim().to_owned());
    let has_head = run(repo, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_ok();
    // `count-objects -v` reports sizes in KiB
    let objects = text(&["count-objects", "-v"])?;
    let kib = |key: &str| {
        objects
            .lines()
            .find_map(|l| l.strip_prefix(key)?.trim().parse::<u64>().ok())
            .unwrap_or(0)
    };
    let count = |pattern: &str| {
        text(&["for-each-ref", "--format=x", pattern]).map_or(0, |s| s.lines().count())
    };
    Ok(Details {
        commits: if has_head {
            text(&["rev-list", "--count", "HEAD"])?.parse().unwrap_or(0)
        } else {
            0
        },
        size: (kib("size:") + kib("size-pack:")) * 1024,
        first_commit: if has_head {
            text(&["log", "--max-parents=0", "--format=%as", "HEAD"])?
                .lines()
                .last()
                .map(str::to_owned)
        } else {
            None
        },
        branches: count("refs/heads"),
        tags: count("refs/tags"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::repo;

    #[test]
    fn parses_numstat() {
        let out = concat!(
            "\u{1}Ann\0a@x\0200\n\n3\t1\tsrc/a.rs\n-\t-\tlogo.png\n\n",
            "\u{1}Bob\0b@x\0150\n\n10\t0\tREADME.md\n\n",
            "\u{1}Ann B\0A@X\0100\n\n1\t1\tsrc/a.rs\n\n"
        );
        let s = parse(out);
        let [ann, bob] = s.authors.as_slice() else {
            panic!("expected two authors: {s:?}")
        };
        assert_eq!(ann.name, "Ann");
        assert_eq!(ann.email, "a@x"); // first name and email seen win, email matches case-insensitively
        assert_eq!(ann.commits, 2);
        assert_eq!(ann.added, 4);
        assert_eq!(ann.deleted, 2);
        assert_eq!(ann.first, 100);
        assert_eq!(ann.last, 200);
        assert_eq!(bob.name, "Bob");
        assert_eq!(bob.commits, 1);
        assert_eq!(bob.added, 10);
        assert_eq!(bob.deleted, 0);
        assert_eq!(bob.first, 150);
        assert_eq!(bob.last, 150);
        assert_eq!(s.times, [200, 150, 100]);
    }

    #[test]
    fn counts_a_real_repository() {
        let tmp = repo();
        let d = tmp.path();
        assert_eq!(stats(d).unwrap(), Stats::default()); // no commits yet
        std::fs::write(d.join("a.txt"), "1\n2\n").unwrap();
        crate::stage(d, &["a.txt"]).unwrap();
        crate::commit(d, "one", false).unwrap();
        std::fs::write(d.join("a.txt"), "1\n").unwrap();
        crate::stage(d, &["a.txt"]).unwrap();
        crate::commit(d, "two", false).unwrap();
        let s = stats(d).unwrap();
        assert_eq!(s.authors.len(), 1);
        let a = &s.authors[0];
        assert_eq!(a.name, "Spork Test");
        assert_eq!(a.commits, 2);
        assert_eq!(a.added, 2);
        assert_eq!(a.deleted, 1);
        assert_eq!(s.times.len(), 2);
    }

    #[test]
    fn repository_details() {
        let tmp = repo();
        let d = tmp.path();
        assert_eq!(details(d).unwrap().commits, 0);
        std::fs::write(d.join("a.txt"), "x".repeat(10_000)).unwrap();
        crate::stage(d, &["a.txt"]).unwrap();
        crate::commit(d, "one", false).unwrap();
        run(d, &["tag", "v1"]).unwrap();
        let info = details(d).unwrap();
        assert_eq!((info.commits, info.branches, info.tags), (1, 1, 1));
        assert!(info.size > 0 && info.first_commit.is_some_and(|date| date.len() == 10));
    }
}
