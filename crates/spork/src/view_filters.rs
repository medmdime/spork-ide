//! The commit list's View filters (docs/specs/M05 §4): current branch only, first parent,
//! lost commits from the reflog, hide tags; and the quick filter (M05 §5), which selects
//! matching commits and dims the rest, keeping the graph whole.
// shortcut: lost commits show like the others; drawing them faded needs reachability per row.

use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use spork_git::Commit;
use spork_git::log::{Filters, SearchIn};

use crate::RepoView;

#[derive(Clone, Copy)]
pub(crate) enum Toggle {
    CurrentBranch,
    FirstParent,
    LostCommits,
    HideTags,
    TopoOrder,
    HideRemotes,
    HideStashes,
}

/// `query` is lowercase: a SHA prefix, or part of the subject, author name or email.
pub(crate) fn matches(c: &Commit, query: &str) -> bool {
    let has = |s: &str| s.to_lowercase().contains(query);
    c.id.starts_with(query) || has(&c.subject) || has(&c.author) || has(&c.email)
}

/// The log toolbar's filters beside the text (log_toolbar.rs): an author, a time since, a path.
#[derive(Default)]
pub(crate) struct LogFilters {
    /// The text is a regular expression (`.*`); its case counts (`Cc`).
    pub(crate) regex: bool,
    pub(crate) match_case: bool,
    pub(crate) user: Option<String>,
    /// Commits made since this Unix time, with how the chip names it ("Last 7 days").
    pub(crate) since: Option<(i64, &'static str)>,
    pub(crate) path: Option<String>,
    /// The commits git says touch `path`; `None` while it is looking.
    pub(crate) path_hits: Option<std::collections::HashSet<String>>,
    /// The text's regular expression, built once per text and case.
    compiled: std::cell::RefCell<Option<(String, bool, Option<regex::Regex>)>>,
}

impl LogFilters {
    /// `c`'s message, author or SHA holds `query`, as the `.*` and `Cc` toggles say.
    fn text_matches(&self, c: &Commit, query: &str) -> bool {
        if self.regex {
            let mut compiled = self.compiled.borrow_mut();
            if compiled
                .as_ref()
                .is_none_or(|(q, case, _)| q != query || *case != self.match_case)
            {
                let re = regex::RegexBuilder::new(query)
                    .case_insensitive(!self.match_case)
                    .build()
                    .ok();
                *compiled = Some((query.to_owned(), self.match_case, re));
            }
            let Some((_, _, Some(re))) = compiled.as_ref() else {
                return false;
            };
            return c.id.starts_with(query) || re.is_match(&c.subject) || re.is_match(&c.author);
        }
        match self.match_case {
            true => {
                c.id.starts_with(query)
                    || c.subject.contains(query)
                    || c.author.contains(query)
                    || c.email.contains(query)
            }
            false => matches(c, &query.to_lowercase()),
        }
    }
}

impl RepoView {
    /// The quick filter's text; empty when not filtering by text.
    pub(crate) fn commit_query(&self, cx: &App) -> String {
        self.commit_filter.read(cx).value().trim().to_owned()
    }

    /// Some filter is on: text, user, date or path.
    pub(crate) fn filtering(&self, cx: &App) -> bool {
        let f = &self.log_filters;
        !self.commit_query(cx).is_empty()
            || f.user.is_some()
            || f.since.is_some()
            || f.path.is_some()
    }

    /// Whether `c` passes every filter: the text (its message, author or SHA, or git's answer
    /// when searching changes or paths; everything counts while that is still coming), the
    /// user, the date and the path.
    pub(crate) fn is_match(&self, c: &Commit, query: &str) -> bool {
        let f = &self.log_filters;
        let text = query.is_empty()
            || match (self.search_in, &self.search_hits) {
                (None, _) => f.text_matches(c, query),
                (Some(_), Some(hits)) => hits.contains(&c.id),
                (Some(_), None) => true,
            };
        let user = f
            .user
            .as_ref()
            .is_none_or(|u| c.author == *u || c.email == *u);
        let since = f.since.is_none_or(|(t, _)| c.time >= t);
        let path = f.path.is_none() || f.path_hits.as_ref().is_none_or(|h| h.contains(&c.id));
        text && user && since && path
    }

    /// After a filter changed: count the matches again and select the first.
    pub(crate) fn refilter(&mut self, cx: &mut Context<Self>) {
        self.jump_to_match(0, cx)
    }

    /// The Paths chip: commits touching `path` (git answers), or none.
    pub(crate) fn set_path_filter(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        self.log_filters.path = path.clone();
        self.log_filters.path_hits = None;
        let (Some(path), Some(Ok(repo))) = (path, &self.repo) else {
            self._path_search = None;
            return self.refilter(cx);
        };
        let root = repo.root.clone();
        self._path_search = Some(cx.spawn(async move |this, cx| {
            let hits = cx
                .background_spawn(async move {
                    spork_git::log::search(&root, &path, SearchIn::Paths, 20000)
                })
                .await
                .unwrap_or_default();
            this.update(cx, |this, cx| {
                this.log_filters.path_hits = Some(hits);
                this.refilter(cx)
            })
            .ok();
        }));
        cx.notify();
    }

    /// Typing selects the first match; Enter the next one, wrapping around.
    pub(crate) fn on_commit_filter(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => match self.search_in {
                Some(within) => self.deep_search(within, cx),
                None => self.jump_to_match(0, cx),
            },
            InputEvent::PressEnter { .. } => {
                self.jump_to_match(self.selected.map_or(0, |i| i + 1), cx)
            }
            _ => {}
        }
    }

    /// Asks git which commits change the text, or touch a matching path, a moment after typing
    /// stops; a newer search replaces (cancels) the one under way.
    pub(crate) fn deep_search(&mut self, within: SearchIn, cx: &mut Context<Self>) {
        let text = self.commit_filter.read(cx).value().trim().to_owned();
        self.search_hits = None;
        let Some(Ok(repo)) = &self.repo else { return };
        if text.is_empty() {
            self._search = None;
            self.commit_matches = 0;
            return cx.notify();
        }
        let root = repo.root.clone();
        self._search = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            let hits = cx
                .background_spawn(async move { spork_git::log::search(&root, &text, within, 5000) })
                .await
                .unwrap_or_default();
            this.update(cx, |this, cx| {
                this.search_hits = Some(hits);
                this.jump_to_match(0, cx)
            })
            .ok();
        }));
        cx.notify();
    }

    pub(crate) fn jump_to_match(&mut self, from: usize, cx: &mut Context<Self>) {
        let query = self.commit_query(cx);
        let filtering = self.filtering(cx);
        let Some(Ok(repo)) = &self.repo else { return };
        let commits = &repo.history.commits;
        self.commit_matches = if !filtering {
            0
        } else {
            commits.iter().filter(|c| self.is_match(c, &query)).count()
        };
        let hit = (filtering && self.commit_matches > 0)
            .then(|| {
                (from..commits.len())
                    .chain(0..from.min(commits.len()))
                    .find(|&i| self.is_match(&commits[i], &query))
            })
            .flatten();
        if let Some(ix) = hit {
            self.select(ix, cx);
            self.history_scroll
                .scroll_to_item(ix, ScrollStrategy::Center);
        }
        cx.notify();
    }

    /// Cmd/Ctrl-F: the commit list, with the cursor in the quick filter.
    pub(crate) fn focus_commit_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.view = crate::View::History;
        self.commit_filter.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    pub(crate) fn toggle_filter(&mut self, toggle: Toggle, cx: &mut Context<Self>) {
        let f: &mut Filters = &mut self.filters;
        match toggle {
            Toggle::CurrentBranch => f.current_branch = !f.current_branch,
            Toggle::FirstParent => f.first_parent = !f.first_parent,
            Toggle::LostCommits => f.lost_commits = !f.lost_commits,
            Toggle::HideTags => self.hide_tags = !self.hide_tags,
            Toggle::TopoOrder => f.topo_order = !f.topo_order,
            Toggle::HideRemotes => f.hide_remotes = !f.hide_remotes,
            Toggle::HideStashes => self.hide_stashes = !self.hide_stashes,
        }
        self.reload(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    // explicit: `super::*` would pull in gpui's `test` attribute, which shadows #[test]
    use super::{LogFilters, matches};
    use spork_git::Commit;

    #[test]
    fn regex_and_match_case_toggles() {
        let c = Commit {
            id: "9c4169f2ab".into(),
            parents: vec![],
            author: "Ada".into(),
            email: "ada@x".into(),
            time: 0,
            commit_time: 0,
            subject: "Fix Login bug #12".into(),
        };
        let mut f = LogFilters::default();
        assert!(f.text_matches(&c, "login"));
        f.match_case = true;
        assert!(!f.text_matches(&c, "login") && f.text_matches(&c, "Login"));
        f.regex = true;
        assert!(f.text_matches(&c, r"L\w+ bug #\d+"));
        assert!(!f.text_matches(&c, r"l\w+ bug"));
        f.match_case = false;
        assert!(f.text_matches(&c, r"l\w+ bug"));
        assert!(!f.text_matches(&c, "(unclosed")); // a broken pattern matches nothing
        assert!(f.text_matches(&c, "9c41")); // hashes still match by prefix
    }

    #[test]
    fn matches_sha_prefix_subject_and_author() {
        let c = Commit {
            id: "9c4169f2ab".into(),
            parents: Vec::new(),
            author: "Ann Lee".into(),
            email: "ann@x.org".into(),
            time: 0,
            commit_time: 0,
            subject: "Fix Login".into(),
        };
        for q in ["9c41", "login", "ann", "x.org"] {
            assert!(matches(&c, q), "{q}");
        }
        assert!(!matches(&c, "169f")); // SHAs match from the start only
        assert!(!matches(&c, "logout"));
    }
}
