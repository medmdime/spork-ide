//! Git-flow (docs/specs/M13), compatible with the git-flow CLI's `gitflow.*` config keys.

use crate::{GitError, Outcome};
use std::path::Path;

/// A repository's git-flow setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gitflow {
    /// Production branch (`gitflow.branch.master`).
    pub master: String,
    /// Development branch (`gitflow.branch.develop`).
    pub develop: String,
    pub feature: String,
    pub release: String,
    pub hotfix: String,
    pub bugfix: String,
    /// Prefix of release / hotfix tags (`gitflow.prefix.versiontag`), often empty.
    pub tag: String,
}

/// Parses `git config --get-regexp ^gitflow\.` output (one `key value` per line).
/// `None` until git-flow is initialized, i.e. when `gitflow.branch.develop` is missing.
pub fn parse_config(out: &str) -> Option<Gitflow> {
    let mut g = Gitflow {
        master: "main".into(),
        develop: String::new(),
        feature: "feature/".into(),
        release: "release/".into(),
        hotfix: "hotfix/".into(),
        bugfix: "bugfix/".into(),
        tag: String::new(),
    };
    for line in out.lines() {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "gitflow.branch.master" => g.master = value.to_owned(),
            "gitflow.branch.develop" => g.develop = value.to_owned(),
            "gitflow.prefix.feature" => g.feature = value.to_owned(),
            "gitflow.prefix.release" => g.release = value.to_owned(),
            "gitflow.prefix.hotfix" => g.hotfix = value.to_owned(),
            "gitflow.prefix.bugfix" => g.bugfix = value.to_owned(),
            "gitflow.prefix.versiontag" => g.tag = value.to_owned(),
            _ => {}
        }
    }
    (!g.develop.is_empty()).then_some(g)
}

/// The repository's git-flow setup; `None` when git-flow isn't initialized.
pub fn read(repo: &Path) -> Option<Gitflow> {
    let out = crate::run(repo, &["config", "--get-regexp", r"^gitflow\."]).ok()?;
    parse_config(&String::from_utf8_lossy(&out))
}

/// True when the local branch `name` exists.
fn branch_exists(repo: &Path, name: &str) -> bool {
    crate::run(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{name}"),
        ],
    )
    .is_ok()
}

/// Initializes git-flow with the CLI's defaults (M13 §1) and returns the setup.
pub fn init(repo: &Path) -> Result<Gitflow, GitError> {
    // an unborn repository has no commit for develop to start at
    if crate::run(repo, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_err() {
        crate::run(
            repo,
            &["commit", "--allow-empty", "--message", "Initial commit"],
        )?;
    }
    let master = if branch_exists(repo, "main") {
        "main".to_owned()
    } else if branch_exists(repo, "master") {
        "master".to_owned()
    } else {
        crate::head_branch(repo)?.ok_or_else(|| GitError::Other {
            code: None,
            stderr: "Check out a branch before initializing git-flow.".into(),
        })?
    };
    if !branch_exists(repo, "develop") {
        crate::run(repo, &["branch", "develop", &master])?;
    }
    let flow = Gitflow {
        master: master.clone(),
        develop: "develop".into(),
        feature: "feature/".into(),
        release: "release/".into(),
        hotfix: "hotfix/".into(),
        bugfix: "bugfix/".into(),
        tag: String::new(),
    };
    // the same keys the git-flow CLI writes, so both tools agree
    for (key, value) in [
        ("gitflow.branch.master", master.as_str()),
        ("gitflow.branch.develop", "develop"),
        ("gitflow.prefix.feature", "feature/"),
        ("gitflow.prefix.release", "release/"),
        ("gitflow.prefix.hotfix", "hotfix/"),
        ("gitflow.prefix.bugfix", "bugfix/"),
        ("gitflow.prefix.support", "support/"),
        ("gitflow.prefix.versiontag", ""),
    ] {
        crate::run(repo, &["config", key, value])?;
    }
    Ok(flow)
}

/// The git-flow branch kinds Spork supports.
// shortcut: no support/ branches (long-lived, never finished); add them when someone asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Feature,
    /// A fix for develop: started and finished like a feature.
    Bugfix,
    Release,
    Hotfix,
}

impl Gitflow {
    /// Branch-name prefix of `kind`, e.g. `feature/`.
    pub fn prefix(&self, kind: Kind) -> &str {
        match kind {
            Kind::Feature => &self.feature,
            Kind::Release => &self.release,
            Kind::Hotfix => &self.hotfix,
            Kind::Bugfix => &self.bugfix,
        }
    }

    /// Where `kind` branches start: develop, except hotfixes, which start at production.
    pub fn base(&self, kind: Kind) -> &str {
        match kind {
            Kind::Hotfix => &self.master,
            Kind::Feature | Kind::Bugfix | Kind::Release => &self.develop,
        }
    }

    /// `(kind, name)` when `branch` is a flow branch: `feature/login` → `(Feature, "login")`.
    /// Empty prefixes never match, and the name after the prefix must not be empty.
    pub fn kind_of<'a>(&self, branch: &'a str) -> Option<(Kind, &'a str)> {
        for kind in [Kind::Feature, Kind::Bugfix, Kind::Release, Kind::Hotfix] {
            let prefix = self.prefix(kind);
            if prefix.is_empty() {
                continue;
            }
            let Some(name) = branch.strip_prefix(prefix) else {
                continue;
            };
            if !name.is_empty() {
                return Some((kind, name));
            }
        }
        None
    }
}

/// Creates `<prefix><name>` at the kind's base and switches to it (M13 §2). Returns the branch.
pub fn start(repo: &Path, flow: &Gitflow, kind: Kind, name: &str) -> Result<String, GitError> {
    let branch = format!("{}{}", flow.prefix(kind), name.trim());
    crate::create_branch(repo, &branch, Some(flow.base(kind)), true)?;
    Ok(branch)
}

/// Finishes `<prefix><name>` (M13 §3). A feature or bugfix merges into develop; a release or
/// hotfix merges into production, gets tagged, and the tag merges back into develop (a hotfix into
/// the open release branch instead, if there is one). Then the branch is deleted unless `keep`.
/// Steps already done are skipped, so after a conflict the user resolves, continues, and calls
/// this again.
pub fn finish(
    repo: &Path,
    flow: &Gitflow,
    kind: Kind,
    name: &str,
    message: &str,
    keep: bool,
) -> Result<Outcome, GitError> {
    if crate::status(repo)?.iter().any(|e| !e.untracked()) {
        return Err(GitError::Other {
            code: None,
            stderr: "Commit or stash your changes before finishing.".into(),
        });
    }
    let branch = format!("{}{name}", flow.prefix(kind));
    if matches!(kind, Kind::Feature | Kind::Bugfix) {
        if merge_into(repo, &flow.develop, &branch)? == Outcome::Conflicts {
            return Ok(Outcome::Conflicts);
        }
    } else {
        if merge_into(repo, &flow.master, &branch)? == Outcome::Conflicts {
            return Ok(Outcome::Conflicts);
        }
        let tag = format!("{}{name}", flow.tag);
        // a tag that exists but isn't on production is someone else's: create_tag then refuses
        if !is_ancestor(repo, &tag, &flow.master) {
            crate::create_tag(repo, &tag, &flow.master, message)?;
        }
        let release = crate::refs(repo)?.into_iter().find(|r| {
            r.kind == crate::RefKind::Local
                && flow.kind_of(&r.name).map(|(k, _)| k) == Some(Kind::Release)
        });
        let back = match release {
            Some(r) if kind == Kind::Hotfix => r.name,
            _ => flow.develop.clone(),
        };
        if merge_into(repo, &back, &tag)? == Outcome::Conflicts {
            return Ok(Outcome::Conflicts);
        }
    }
    if !keep {
        crate::delete_branch(repo, &branch)?;
    }
    Ok(Outcome::Done)
}

fn is_ancestor(repo: &Path, commit: &str, of: &str) -> bool {
    crate::run(repo, &["merge-base", "--is-ancestor", commit, of]).is_ok()
}

/// Switches to `into` and merges `what` with a merge commit, unless `into` already contains it.
fn merge_into(repo: &Path, into: &str, what: &str) -> Result<Outcome, GitError> {
    crate::checkout(repo, into)?;
    if is_ancestor(repo, what, into) {
        return Ok(Outcome::Done);
    }
    crate::allow_conflicts(
        repo,
        crate::run(repo, &["merge", "--no-ff", "--no-edit", what]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testutil::repo;

    #[test]
    fn init_with_defaults() {
        let tmp = repo();
        let d = tmp.path();
        assert_eq!(read(d), None);
        let flow = init(d).unwrap(); // unborn repo: init must create the first commit
        assert_eq!(read(d), Some(flow.clone()));
        assert_eq!(flow.master, "main");
        assert_eq!(flow.develop, "develop");
        assert!(branch_exists(d, "develop"));
        assert!(init(d).is_ok()); // idempotent
    }

    #[test]
    fn parses_cli_config() {
        assert_eq!(parse_config(""), None);
        assert_eq!(parse_config("gitflow.branch.master main\n"), None);
        let out = "gitflow.branch.master master\n\
                   gitflow.branch.develop dev\n\
                   gitflow.prefix.feature f/\n\
                   gitflow.prefix.versiontag\n\
                   gitflow.prefix.bugfix bugfix/\n";
        assert_eq!(
            parse_config(out).unwrap(),
            Gitflow {
                master: "master".into(),
                develop: "dev".into(),
                feature: "f/".into(),
                release: "release/".into(),
                hotfix: "hotfix/".into(),
                bugfix: "bugfix/".into(),
                tag: "".into(),
            }
        );
    }

    #[test]
    fn kind_of_matches_prefixes() {
        let mut flow = Gitflow {
            master: "main".into(),
            develop: "develop".into(),
            feature: "feature/".into(),
            release: "release/".into(),
            hotfix: "hotfix/".into(),
            bugfix: "bugfix/".into(),
            tag: String::new(),
        };
        assert_eq!(
            flow.kind_of("feature/login"),
            Some((Kind::Feature, "login"))
        );
        assert_eq!(flow.kind_of("hotfix/1.0.1"), Some((Kind::Hotfix, "1.0.1")));
        assert_eq!(flow.kind_of("feature/"), None);
        assert_eq!(flow.kind_of("main"), None);
        // an empty prefix never matches
        flow.feature = String::new();
        assert_eq!(flow.kind_of("login"), None);
    }

    #[test]
    fn start_branches_at_their_base() {
        let tmp = repo();
        let d = tmp.path();
        let flow = init(d).unwrap();
        crate::run(d, &["switch", "develop"]).unwrap();
        crate::run(d, &["commit", "--allow-empty", "-m", "dev work"]).unwrap();
        assert_eq!(
            start(d, &flow, Kind::Feature, "login").unwrap(),
            "feature/login"
        );
        assert_eq!(
            crate::head_branch(d).unwrap().as_deref(),
            Some("feature/login")
        );
        start(d, &flow, Kind::Hotfix, "1.0.1").unwrap();
        assert_eq!(
            crate::run(d, &["rev-parse", "HEAD"]).unwrap(),
            crate::run(d, &["rev-parse", "main"]).unwrap()
        );
    }

    #[test]
    fn finish_feature_release_hotfix() {
        let tmp = repo();
        let d = tmp.path();
        let flow = init(d).unwrap();
        let commit_file = |file: &str, text: &str| {
            std::fs::write(d.join(file), text).unwrap();
            crate::stage(d, &[file]).unwrap();
            crate::commit(d, &format!("edit {file}"), false).unwrap();
        };
        let has = |commit: &str, of: &str| {
            crate::run(d, &["merge-base", "--is-ancestor", commit, of]).is_ok()
        };
        let exists = |r: &str| crate::run(d, &["rev-parse", "--verify", "--quiet", r]).is_ok();
        let head = || crate::head_branch(d).unwrap().unwrap();

        // feature: merged into develop as a merge commit, branch deleted
        start(d, &flow, Kind::Feature, "login").unwrap();
        commit_file("login.txt", "x");
        assert_eq!(
            finish(d, &flow, Kind::Feature, "login", "", false).unwrap(),
            crate::Outcome::Done
        );
        assert_eq!(head(), "develop");
        assert!(!exists("refs/heads/feature/login"));
        assert!(d.join("login.txt").exists());
        assert_eq!(crate::log(d, 1).unwrap()[0].parents.len(), 2); // merge commit

        // release: tagged on main, tag merged back into develop, branch deleted
        start(d, &flow, Kind::Release, "1.0").unwrap();
        commit_file("version.txt", "1.0");
        assert_eq!(
            finish(d, &flow, Kind::Release, "1.0", "Release 1.0", false).unwrap(),
            crate::Outcome::Done
        );
        assert!(exists("refs/tags/1.0"));
        assert!(has("1.0", "main"));
        assert!(has("1.0", "develop"));
        assert!(!exists("refs/heads/release/1.0"));
        assert_eq!(head(), "develop");

        // hotfix: into main and, a release being open, back into the release branch
        start(d, &flow, Kind::Release, "1.1").unwrap();
        start(d, &flow, Kind::Hotfix, "1.0.1").unwrap();
        commit_file("fix.txt", "fix");
        assert_eq!(
            finish(d, &flow, Kind::Hotfix, "1.0.1", "Hotfix 1.0.1", false).unwrap(),
            crate::Outcome::Done
        );
        assert!(has("1.0.1", "main"));
        assert!(has("1.0.1", "release/1.1"));
        assert!(!has("1.0.1", "develop"));
        assert_eq!(head(), "release/1.1");

        // a dirty work tree is refused
        std::fs::write(d.join("version.txt"), "dirty").unwrap();
        assert!(finish(d, &flow, Kind::Release, "1.1", "", false).is_err());
        crate::run(d, &["checkout", "--", "version.txt"]).unwrap();

        // conflict: finish pauses, the user resolves and continues, then finish completes
        crate::run(d, &["switch", "develop"]).unwrap();
        start(d, &flow, Kind::Feature, "clash").unwrap();
        commit_file("clash.txt", "feature");
        crate::run(d, &["switch", "develop"]).unwrap();
        commit_file("clash.txt", "develop");
        assert_eq!(
            finish(d, &flow, Kind::Feature, "clash", "", false).unwrap(),
            crate::Outcome::Conflicts
        );
        crate::resolve_with(d, "clash.txt", false).unwrap();
        assert_eq!(crate::continue_op(d).unwrap(), crate::Outcome::Done);
        assert_eq!(
            finish(d, &flow, Kind::Feature, "clash", "", false).unwrap(),
            crate::Outcome::Done
        );
        assert!(!exists("refs/heads/feature/clash"));
        assert_eq!(head(), "develop");

        // bugfix: into develop like a feature; kept when asked
        start(d, &flow, Kind::Bugfix, "typo").unwrap();
        commit_file("typo.txt", "fixed");
        assert_eq!(
            finish(d, &flow, Kind::Bugfix, "typo", "", true).unwrap(),
            crate::Outcome::Done
        );
        assert!(has("bugfix/typo", "develop"));
        assert!(exists("refs/heads/bugfix/typo"));
    }
}
