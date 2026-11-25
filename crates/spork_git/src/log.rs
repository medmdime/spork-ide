//! The commit list with the View menu's filters (docs/specs/M05 §4).

use std::path::Path;

use crate::{Commit, GitError, run};

/// The View menu's filters; `Default` is what `crate::log` shows (every branch, remote and tag).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Filters {
    /// Only HEAD's history instead of all branches, remotes and tags.
    pub current_branch: bool,
    /// Follow only first parents, which collapses merged branches (`--first-parent`).
    pub first_parent: bool,
    /// Also commits only the reflog still knows, e.g. after a reset (`--reflog`).
    pub lost_commits: bool,
    /// Each branch's commits together, parents after all their children (`--topo-order`),
    /// instead of newest first across branches (`--date-order`).
    pub topo_order: bool,
    /// Leave remote branches out (their commits show only when a local branch or tag reaches them).
    pub hide_remotes: bool,
}

/// `crate::log` with `filters` applied, leaving out branches named in `hidden` (`main`,
/// `origin/old`) unless other refs reach their commits. HEAD always shows, unless `only` names
/// the branches to show (missing ones are skipped).
pub fn filtered(
    repo: &Path,
    max: usize,
    filters: Filters,
    hidden: &[String],
    only: &[String],
) -> Result<Vec<Commit>, GitError> {
    let max = format!("--max-count={max}");
    // an --exclude applies to the next --branches / --remotes only, so it is given to both
    let excludes: Vec<String> = hidden.iter().map(|h| format!("--exclude={h}")).collect();
    let excludes: Vec<&str> = excludes.iter().map(String::as_str).collect();
    let mut args = [
        "log",
        "-z",
        if filters.topo_order {
            "--topo-order"
        } else {
            "--date-order"
        },
        "--ignore-missing",
        &max,
        crate::LOG_FORMAT,
    ]
    .to_vec();
    if filters.first_parent {
        args.push("--first-parent");
    }
    if !only.is_empty() {
        args.extend(only.iter().map(String::as_str));
    } else if filters.current_branch {
        args.push("HEAD");
    } else {
        args.extend(&excludes);
        args.push("--branches");
        if !filters.hide_remotes {
            args.extend(&excludes);
            args.push("--remotes");
        }
        args.extend(["--tags", "HEAD"]);
    }
    if filters.lost_commits {
        args.push("--reflog");
    }
    args.push("--");
    let out = run(repo, &args)?;
    Ok(crate::parse_log(&String::from_utf8_lossy(&out)))
}

/// Folds the branch `merge` brought in: drops from `commits` those only its second parent
/// reaches, and keeps only its first parent, so the graph draws it as a plain commit. Returns how
/// many commits were folded (0 when `merge` isn't a listed merge).
pub fn fold_merge(repo: &Path, commits: &mut Vec<Commit>, merge: &str) -> usize {
    let Some(at) = commits
        .iter()
        .position(|c| c.id == merge && c.parents.len() > 1)
    else {
        return 0;
    };
    let range = format!("{merge}^1..{merge}^2");
    let Ok(out) = run(repo, &["rev-list", &range]) else {
        return 0;
    };
    let out = String::from_utf8_lossy(&out);
    let folded: std::collections::HashSet<&str> = out.lines().collect();
    commits[at].parents.truncate(1);
    let before = commits.len();
    commits.retain(|c| !folded.contains(c.id.as_str()));
    before - commits.len()
}

/// Adds each stash to `commits` as a commit of its own on the commit it was made on (its first
/// parent only), among the others by time. Stashes made on a commit that isn't listed stay out.
pub fn insert_stashes(commits: &mut Vec<Commit>, stashes: &[crate::Stash]) {
    for s in stashes {
        let Some(base) = commits.iter().position(|c| c.id == s.base) else {
            continue;
        };
        // above its base, and above older commits so it sits where it happened
        let at = commits[..base]
            .iter()
            .position(|c| c.time <= s.time)
            .unwrap_or(base);
        commits.insert(
            at,
            Commit {
                id: s.id.clone(),
                parents: vec![s.base.clone()],
                author: s.author.clone(),
                email: String::new(),
                time: s.time,
                commit_time: s.time,
                subject: s.message.clone(),
            },
        );
    }
}

/// Where [`search`] looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchIn {
    /// Lines added or removed: commits that change how often `text` appears (`git log -S`).
    Changes,
    /// Paths of the changed files, ignoring case (`text` anywhere in the path).
    Paths,
}

/// Ids of the commits on any branch, remote branch or tag (at most `max`) that match `text`.
pub fn search(
    repo: &Path,
    text: &str,
    within: SearchIn,
    max: usize,
) -> Result<std::collections::HashSet<String>, GitError> {
    let max = format!("--max-count={max}");
    let mut args = vec![
        "log",
        "--format=%H",
        &max,
        "--branches",
        "--remotes",
        "--tags",
        "HEAD",
    ];
    let pickaxe = format!("-S{text}");
    // `*` matches across folders here; `icase` ignores case
    let path = format!(":(icase)*{}*", text.replace(['*', '?', '['], ""));
    match within {
        SearchIn::Changes => args.extend([pickaxe.as_str(), "--"]),
        SearchIn::Paths => args.extend(["--", path.as_str()]),
    }
    let out = run(repo, &args)?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .map(str::to_owned)
        .collect())
}

/// Whether the log is indexed: git keeps a commit graph (with changed-path filters once
/// [`write_index`] wrote it), which walks and path filters read instead of every commit.
pub fn is_indexed(repo: &Path) -> Result<bool, GitError> {
    let out = run(
        repo,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "objects/info/commit-graph",
            "--git-path",
            "objects/info/commit-graphs/commit-graph-chain",
        ],
    )?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .any(|path| Path::new(path).is_file()))
}

/// Indexes the log (IntelliJ's Enable Log Indexing): writes the commit graph with changed-path
/// filters, and has fetches keep it up to date.
pub fn write_index(repo: &Path) -> Result<(), GitError> {
    run(
        repo,
        &["config", "--local", "fetch.writeCommitGraph", "true"],
    )?;
    run(
        repo,
        &["commit-graph", "write", "--reachable", "--changed-paths"],
    )
    .map(drop)
}

/// The commits on `branch` the current branch has no equivalent of, by their changes: not
/// merged in nor cherry-picked (`git log --cherry-pick --right-only HEAD...branch`).
pub fn not_cherry_picked(
    repo: &Path,
    branch: &str,
) -> Result<std::collections::HashSet<String>, GitError> {
    let range = format!("HEAD...{branch}");
    let out = run(
        repo,
        &[
            "log",
            "--format=%H",
            "--cherry-pick",
            "--right-only",
            "--no-merges",
            &range,
            "--",
        ],
    )?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .map(str::to_owned)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::repo;

    #[test]
    fn indexes_the_log() {
        let tmp = repo();
        let d = tmp.path();
        run(d, &["commit", "--allow-empty", "-m", "base"]).unwrap();
        assert!(!is_indexed(d).unwrap());
        write_index(d).unwrap();
        assert!(is_indexed(d).unwrap());
    }

    #[test]
    fn finds_commits_not_cherry_picked() {
        let tmp = repo();
        let d = tmp.path();
        std::fs::write(d.join("a"), "a").unwrap();
        run(d, &["add", "a"]).unwrap();
        run(d, &["commit", "-m", "base"]).unwrap();
        run(d, &["switch", "-c", "topic"]).unwrap();
        for name in ["picked", "left"] {
            std::fs::write(d.join(name), name).unwrap();
            run(d, &["add", name]).unwrap();
            run(d, &["commit", "-m", name]).unwrap();
        }
        run(d, &["switch", "main"]).unwrap();
        run(d, &["cherry-pick", "topic~1"]).unwrap();
        let left = not_cherry_picked(d, "topic").unwrap();
        let topic = String::from_utf8(run(d, &["rev-parse", "topic"]).unwrap()).unwrap();
        assert_eq!(left.into_iter().collect::<Vec<_>>(), [topic.trim()]);
    }

    #[test]
    fn filters_change_the_list() {
        let tmp = repo();
        let d = tmp.path();
        run(d, &["commit", "--allow-empty", "-m", "base"]).unwrap();
        run(d, &["switch", "-c", "side"]).unwrap();
        run(d, &["commit", "--allow-empty", "-m", "side work"]).unwrap();
        run(d, &["switch", "main"]).unwrap();
        run(d, &["commit", "--allow-empty", "-m", "main work"]).unwrap();
        run(d, &["merge", "--no-ff", "-m", "merge side", "side"]).unwrap();
        run(d, &["commit", "--allow-empty", "-m", "doomed"]).unwrap();
        run(d, &["reset", "--hard", "HEAD~1"]).unwrap();

        let subjects = |f: Filters| {
            filtered(d, 100, f, &[], &[])
                .unwrap()
                .into_iter()
                .map(|c| c.subject)
                .collect::<Vec<_>>()
        };

        let all = subjects(Filters::default());
        assert!(all.contains(&"side work".to_owned()));
        assert!(all.contains(&"merge side".to_owned()));
        assert!(!all.contains(&"doomed".to_owned()));

        let first = subjects(Filters {
            current_branch: true,
            first_parent: true,
            ..Default::default()
        });
        assert!(first.contains(&"merge side".to_owned()));
        assert!(first.contains(&"main work".to_owned()));
        assert!(!first.contains(&"side work".to_owned()));

        let lost = subjects(Filters {
            lost_commits: true,
            ..Default::default()
        });
        assert!(lost.contains(&"doomed".to_owned()));

        let mut topo = subjects(Filters {
            topo_order: true,
            ..Default::default()
        });
        assert_eq!(topo[0], "merge side");
        let mut by_date = all;
        topo.sort();
        by_date.sort();
        assert_eq!(topo, by_date); // the same commits, maybe in another order

        assert_eq!(
            crate::log(d, 100).unwrap().len(),
            filtered(d, 100, Filters::default(), &[], &[])
                .unwrap()
                .len()
        );
    }

    #[test]
    fn searches_changes_and_paths() {
        let tmp = repo();
        let d = tmp.path();
        let commit = |file: &str, text: &str, msg: &str| {
            if let Some(dir) = std::path::Path::new(file).parent() {
                std::fs::create_dir_all(d.join(dir)).unwrap();
            }
            std::fs::write(d.join(file), text).unwrap();
            run(d, &["add", "."]).unwrap();
            run(d, &["commit", "-q", "-m", msg]).unwrap();
        };
        commit("src/Login.rs", "fn login() {}\n", "add login");
        commit("README.md", "hello\n", "readme");
        commit("src/Login.rs", "fn login() { check() }\n", "check");
        let id = |rev: &str| {
            String::from_utf8_lossy(&run(d, &["rev-parse", rev]).unwrap())
                .trim()
                .to_owned()
        };
        let changes = search(d, "check()", SearchIn::Changes, 100).unwrap();
        assert_eq!(changes, [id("HEAD")].into());
        let paths = search(d, "login", SearchIn::Paths, 100).unwrap();
        assert_eq!(paths, [id("HEAD"), id("HEAD~2")].into());
        assert!(
            search(d, "nothing-like-this", SearchIn::Paths, 100)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn stashes_sit_on_their_base() {
        let commit = |id: &str, parent: &str, time: i64| Commit {
            id: id.into(),
            parents: vec![parent.into()],
            author: "a".into(),
            email: String::new(),
            time,
            commit_time: time,
            subject: id.into(),
        };
        let mut commits = vec![
            commit("c3", "c2", 30),
            commit("c2", "c1", 20),
            commit("c1", "", 10),
        ];
        let stash = |id: &str, base: &str, time: i64| crate::Stash {
            index: 0,
            id: id.into(),
            message: "WIP".into(),
            time,
            base: base.into(),
            author: "a".into(),
        };
        // newer than everything, made on c2: still right above c2's newer neighbours by time
        insert_stashes(
            &mut commits,
            &[
                stash("s1", "c2", 25),
                stash("s2", "c1", 40),
                stash("s3", "gone", 50),
            ],
        );
        let ids: Vec<&str> = commits.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["s2", "c3", "s1", "c2", "c1"]);
        assert_eq!(commits[0].parents, ["c1"]);
    }

    #[test]
    fn folds_a_merged_branch() {
        let tmp = repo();
        let d = tmp.path();
        run(d, &["commit", "--allow-empty", "-m", "base"]).unwrap();
        run(d, &["switch", "-c", "topic"]).unwrap();
        run(d, &["commit", "--allow-empty", "-m", "t1"]).unwrap();
        run(d, &["commit", "--allow-empty", "-m", "t2"]).unwrap();
        run(d, &["switch", "main"]).unwrap();
        run(d, &["commit", "--allow-empty", "-m", "m1"]).unwrap();
        run(d, &["merge", "--no-ff", "-q", "-m", "merge topic", "topic"]).unwrap();
        run(d, &["branch", "-D", "topic"]).unwrap();
        let mut commits = filtered(d, 100, Filters::default(), &[], &[]).unwrap();
        let merge = commits[0].id.clone();
        assert_eq!(fold_merge(d, &mut commits, &merge), 2);
        let subjects: Vec<&str> = commits.iter().map(|c| c.subject.as_str()).collect();
        assert_eq!(subjects, ["merge topic", "m1", "base"]);
        assert_eq!(commits[0].parents.len(), 1);
        let base = commits[2].id.clone();
        assert_eq!(fold_merge(d, &mut commits, &base), 0); // not a merge
    }

    #[test]
    fn hides_branches_but_not_head() {
        let tmp = repo();
        let d = tmp.path();
        run(d, &["commit", "--allow-empty", "-m", "base"]).unwrap();
        run(d, &["switch", "-c", "experiment"]).unwrap();
        run(d, &["commit", "--allow-empty", "-m", "try"]).unwrap();
        run(d, &["switch", "main"]).unwrap();
        let has_try = |hidden: &[String]| {
            filtered(d, 100, Filters::default(), hidden, &[])
                .unwrap()
                .iter()
                .any(|c| c.subject == "try")
        };
        assert!(has_try(&[]));
        assert!(!has_try(&["experiment".to_owned()]));
        run(d, &["switch", "experiment"]).unwrap();
        assert!(has_try(&["experiment".to_owned()])); // the current branch always shows
        // only the named branches; a missing name is skipped, not an error
        run(d, &["switch", "main"]).unwrap();
        run(d, &["commit", "--allow-empty", "-m", "on main"]).unwrap();
        let only = |names: &[&str]| {
            let names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
            filtered(d, 100, Filters::default(), &[], &names)
                .unwrap()
                .into_iter()
                .map(|c| c.subject)
                .collect::<Vec<_>>()
        };
        assert_eq!(only(&["experiment", "gone"]), ["try", "base"]);
    }
}
