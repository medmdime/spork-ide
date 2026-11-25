//! Find the commit that broke something by halving the history (docs/specs/M05 §8).

use std::path::Path;

use crate::{GitError, run};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Good,
    Bad,
    Skip,
}

/// Where a bisect stands after a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    /// About `steps` more commits to test; the one to test is checked out.
    Testing { steps: u32 },
    /// The first bad commit.
    Found(String),
}

/// Whether a bisect is running (git keeps `BISECT_LOG` meanwhile).
pub fn is_running(repo: &Path) -> bool {
    let Ok(out) = run(repo, &["rev-parse", "--git-path", "BISECT_LOG"]) else {
        return false;
    };
    repo.join(String::from_utf8_lossy(&out).trim()).exists()
}

/// Starts between a `bad` and a `good` commit and checks out the first one to test.
pub fn start(repo: &Path, bad: &str, good: &str) -> Result<Progress, GitError> {
    progress(run(repo, &["bisect", "start", bad, good])?)
}

/// Marks the checked-out commit and checks out the next one, or names the culprit.
pub fn mark(repo: &Path, verdict: Verdict) -> Result<Progress, GitError> {
    let word = match verdict {
        Verdict::Good => "good",
        Verdict::Bad => "bad",
        Verdict::Skip => "skip",
    };
    progress(run(repo, &["bisect", word])?)
}

/// Ends the bisect and goes back to where it started.
pub fn reset(repo: &Path) -> Result<(), GitError> {
    run(repo, &["bisect", "reset"]).map(drop)
}

fn progress(out: Vec<u8>) -> Result<Progress, GitError> {
    let text = String::from_utf8_lossy(&out).into_owned();
    parse_progress(&text).ok_or(GitError::Other {
        code: None,
        stderr: text,
    })
}

/// Parses git's bisect output.
pub fn parse_progress(out: &str) -> Option<Progress> {
    // newer git quotes the term: "is the first 'bad' commit"
    let culprit = |l: &&str| {
        l.split_once(" is the first ")
            .is_some_and(|(_, t)| t.replace('\'', "") == "bad commit")
    };
    if let Some(line) = out.lines().find(culprit) {
        return Some(Progress::Found(line.split(' ').next()?.to_owned()));
    }
    let (_, rest) = out.split_once("(roughly ")?;
    let steps = rest.split(' ').next()?.parse().ok()?;
    Some(Progress::Testing { steps })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::repo;

    #[test]
    fn parses_progress() {
        let p = |s| parse_progress(s);
        assert_eq!(
            p("Bisecting: 3 revisions left to test after this (roughly 2 steps)\n[abc] msg\n"),
            Some(Progress::Testing { steps: 2 })
        );
        assert_eq!(
            p("Bisecting: 0 revisions left to test after this (roughly 0 steps)\n"),
            Some(Progress::Testing { steps: 0 })
        );
        assert_eq!(
            p("0123abcd is the first bad commit\ncommit 0123abcd\n"),
            Some(Progress::Found("0123abcd".into()))
        );
        assert_eq!(
            p("0123abcd is the first 'bad' commit\n"),
            Some(Progress::Found("0123abcd".into()))
        );
        assert_eq!(p("nonsense"), None);
    }

    #[test]
    fn finds_the_first_bad_commit() {
        let tmp = repo();
        let d = tmp.path();
        for i in 1..=8 {
            std::fs::write(d.join("n.txt"), i.to_string()).unwrap();
            crate::stage(d, &["n.txt"]).unwrap();
            crate::commit(d, &format!("c{i}"), false).unwrap();
        }
        let good = String::from_utf8_lossy(&run(d, &["rev-parse", "HEAD~7"]).unwrap())
            .trim()
            .to_owned();
        let mut step = start(d, "HEAD", &good).unwrap();
        for _ in 0..10 {
            let Progress::Testing { .. } = step else {
                break;
            };
            let n: u32 = std::fs::read_to_string(d.join("n.txt"))
                .unwrap()
                .parse()
                .unwrap();
            step = mark(d, if n >= 5 { Verdict::Bad } else { Verdict::Good }).unwrap();
        }
        let Progress::Found(id) = step else {
            panic!("no culprit: {step:?}")
        };
        let subject = run(d, &["log", "-1", "--format=%s", &id]).unwrap();
        assert_eq!(String::from_utf8_lossy(&subject).trim(), "c5");
        assert!(is_running(d));
        reset(d).unwrap();
        assert!(!is_running(d));
    }
}
