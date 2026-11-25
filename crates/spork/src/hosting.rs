//! GitHub and GitLab: connect with a token, list and check out pull / merge requests, create one
//! from a branch (docs/specs/M15).

use std::collections::HashSet;

use crate::notices::Notification;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::command::{CommandItem, CommandState};
use gpui_kit::component::diff::{Diff, DiffState};
use gpui_kit::component::input::{InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Selectable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_git::{GitError, credential, remote};
use spork_hosting::{Checks, Client, Namespace, PullRequest, Review, Service, parse_web_url};

use crate::ai::AiTask;
use crate::{LeftTool, RepoView};

actions!(
    spork,
    [
        CreatePullRequest,
        ViewPullRequests,
        SyncFork,
        ShareOnGitHub,
        CloneFromGitHub,
        ManageGitHubAccounts,
        CreateMergeRequest,
        ViewMergeRequests,
        ShareOnGitLab,
        CloneFromGitLab,
        ManageGitLabAccounts,
        ActivatePullRequestsToolWindow,
        ActivateMergeRequestsToolWindow,
    ]
);

thread_local! {
    /// Whether the active project is on GitHub and on GitLab, as the menu bar shows it now.
    static MENU_HOSTING: std::cell::Cell<(bool, bool)> = const { std::cell::Cell::new((false, false)) };
}

/// Git ▸ GitHub, as the reference app's: what works on a GitHub repository shows only on one; Create Gist…
/// and the browser link need a file, which the menu bar never has.
pub(crate) fn github_menu() -> Menu {
    let on_github = MENU_HOSTING.get().0;
    let mut items = Vec::new();
    if on_github {
        items.extend([
            MenuItem::action("Create Pull Request…", CreatePullRequest),
            MenuItem::action("View Pull Requests", ViewPullRequests),
            MenuItem::action("Sync Fork", SyncFork),
            MenuItem::separator(),
        ]);
    }
    items.extend([
        MenuItem::action("Share Project on GitHub", ShareOnGitHub),
        MenuItem::action("Clone Repository from GitHub…", CloneFromGitHub),
        MenuItem::separator(),
        MenuItem::action("Manage GitHub Accounts…", ManageGitHubAccounts),
    ]);
    Menu::new("GitHub").items(items)
}

/// Git ▸ GitLab, as the reference app's; Create Snippet… needs a file, as Create Gist….
pub(crate) fn gitlab_menu() -> Menu {
    let on_gitlab = MENU_HOSTING.get().1;
    let mut items = Vec::new();
    if on_gitlab {
        items.extend([
            MenuItem::action("Create Merge Request…", CreateMergeRequest),
            MenuItem::action("View Merge Requests", ViewMergeRequests),
            MenuItem::separator(),
        ]);
    }
    items.extend([
        MenuItem::action("Share Project on GitLab", ShareOnGitLab),
        MenuItem::action("Clone Repository from GitLab…", CloneFromGitLab),
        MenuItem::separator(),
        MenuItem::action("Manage GitLab Accounts…", ManageGitLabAccounts),
    ]);
    Menu::new("GitLab").items(items)
}

/// Find Action's entries for the hosting tool windows, as the reference app's, and Spork's own account
/// commands, which the reference app's menus don't have.
pub(crate) fn palette_commands() -> Vec<(SharedString, Box<dyn Action>)> {
    vec![
        (
            "Pull Requests".into(),
            Box::new(ActivatePullRequestsToolWindow),
        ),
        (
            "Merge Requests".into(),
            Box::new(ActivateMergeRequestsToolWindow),
        ),
        ("Accounts…".into(), Box::new(crate::shell::Accounts)),
        ("Log In…".into(), Box::new(crate::shell::ConnectAccount)),
        (
            "Notifications…".into(),
            Box::new(crate::shell::Notifications),
        ),
    ]
}

/// Where the current repository is hosted and what we know about its pull / merge requests.
pub(crate) struct Hosted {
    pub(crate) service: Service,
    host: String,
    /// `owner/name`, or a GitLab namespace path.
    project: String,
    /// The remote that points at the service.
    remote: String,
    /// `None` while loading; `Some(Err)` explains why there is no list (not connected, offline…).
    pulls: Option<Result<Vec<PullRequest>, String>>,
    /// CI results and reviewers' verdicts by pull request number, loaded after the list.
    checks: Vec<(u64, Checks)>,
    reviews: Vec<(u64, Review)>,
    /// The list's selected request, by number.
    selected: Option<u64>,
    /// The request View Pull Request opened, and the tab the tool window shows.
    details: Option<PullDetails>,
    tab: PullsTab,
    /// GitLab's Show Merge Request Events: the timeline shows state and assignee changes too.
    show_events: bool,
    /// Its changes alone by name, not under their folders (Group By ▸ Directory off).
    flat: bool,
}

impl Hosted {
    fn find(remotes: &[remote::Remote], default: Option<&str>) -> Option<Self> {
        let mut ordered = remotes
            .iter()
            .filter(|r| Some(r.name.as_str()) == default)
            .chain(remotes.iter());
        ordered.find_map(|r| {
            let (host, project) = parse_web_url(&remote::web_url(&r.url)?)?;
            let service = service_for(&host)?;
            Some(Hosted {
                service,
                host: service.canonical_host(&host),
                project,
                remote: r.name.clone(),
                pulls: None,
                checks: Vec::new(),
                reviews: Vec::new(),
                selected: None,
                details: None,
                tab: PullsTab::List,
                show_events: false,
                flat: false,
            })
        })
    }
}

thread_local! {
    /// The pull / merge requests the end-to-end tests list in place of the service's (e2e.rs).
    static TEST_PULLS: std::cell::RefCell<Option<Vec<PullRequest>>> =
        const { std::cell::RefCell::new(None) };
    /// The page Spork opened last under the end-to-end tests, which open no browser.
    static OPENED: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    /// Requests Open in Worktree shows in the worktree's window, by the worktree's folder.
    static SHOW_IN_WINDOW: std::cell::RefCell<Vec<(std::path::PathBuf, u64)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Opens `url` in the browser; under the end-to-end tests only remembers it (`opened_url`).
fn open_in_browser(url: &str, cx: &mut App) {
    match std::env::var_os("SPORK_E2E") {
        Some(_) => OPENED.set(Some(url.to_owned())),
        None => cx.open_url(url),
    }
}

/// The page `open_in_browser` opened last under the end-to-end tests.
pub(crate) fn opened_url() -> Option<String> {
    OPENED.with_borrow(Clone::clone)
}

/// "Pull Request" or "Merge Request", as the reference app's menus name them.
fn request_name(service: Service) -> &'static str {
    match service {
        Service::GitLab => "Merge Request",
        _ => "Pull Request",
    }
}

/// How the service refers to a request: GitLab's `!7`, GitHub's `#7`.
fn request_ref(service: Service, number: u64) -> String {
    match service {
        Service::GitLab => format!("!{number}"),
        _ => format!("#{number}"),
    }
}

/// The service on `host`: known by its name, or as the user said when connecting it.
pub(crate) fn service_for(host: &str) -> Option<Service> {
    Service::detect(host).or_else(|| {
        Service::from_id(&spork_git::config::get_global(&format!(
            "spork.{host}.service"
        ))?)
    })
}

/// Every account connected on `service`, as `(host, user)`.
pub(crate) fn service_accounts(service: Service) -> Vec<(String, String)> {
    spork_git::config::global_matching(r"^spork\..*\.account$")
        .into_iter()
        .filter_map(|(key, user)| {
            let host = key.strip_prefix("spork.")?.strip_suffix(".account")?;
            (service_for(host) == Some(service)).then(|| (host.to_owned(), user))
        })
        .collect()
}

/// The users a Clone dialog source lists: GitHub's on github.com, GitHub Enterprise's (`host`
/// empty) on any other GitHub server, GitLab's on any GitLab server.
pub(crate) fn clone_source_users(service: Service, host: &str) -> Vec<String> {
    service_accounts(service)
        .into_iter()
        .filter(|(on, _)| match (service, host) {
            (Service::GitLab, _) => true,
            (_, "") => on != service.default_host(),
            _ => on == host,
        })
        .map(|(_, user)| user)
        .collect()
}

/// `(username, token)` for `host`: this repository's chosen account (`spork.account`), else the
/// host's default one, else whichever the credential helper has.
pub(crate) fn account_token(
    root: Option<&std::path::Path>,
    host: &str,
) -> Option<(String, String)> {
    let user = root
        .and_then(|r| spork_git::config::get_local(r, "spork.account"))
        .or_else(|| spork_git::config::get_global(&format!("spork.{host}.default")));
    credential::get_as(host, user.as_deref())
}

/// Checks `token`, keeps it in the credential helper, and records the account (and the service
/// of an unknown host). `root`: the repository to use it for. Returns the account's name.
fn save_account(
    root: Option<&std::path::Path>,
    service: Service,
    host: &str,
    token: &str,
) -> Result<String, GitError> {
    let login = Client::new(service, host, token).login().map_err(other)?;
    credential::store(host, &login, token)?;
    // the helper may have refused to save it (none configured): say so now, not later
    if credential::get_as(host, Some(&login)).is_none() {
        return Err(other(
            "The token works, but no git credential helper saved it. Set one up (git config --global credential.helper …) and connect again.",
        ));
    }
    let config = |key: &str| format!("spork.{host}.{key}");
    spork_git::config::add_global(&config("account"), &login)?;
    if Service::detect(host) != Some(service) {
        spork_git::config::set_global(&config("service"), service.id())?;
    }
    if spork_git::config::get_global(&config("default")).is_none() {
        spork_git::config::set_global(&config("default"), &login)?;
    }
    if let Some(root) = root {
        spork_git::config::set_local(root, "spork.account", Some(&login))?;
    }
    Ok(login)
}

/// The GitHub OAuth app Spork signs in as in the browser: built in with `SPORK_GITHUB_CLIENT_ID`
/// at build time, or set with `git config --global spork.githubClientId <id>`.
fn github_client_id() -> Option<String> {
    option_env!("SPORK_GITHUB_CLIENT_ID")
        .map(str::to_owned)
        .or_else(|| spork_git::config::get_global("spork.githubClientId"))
        .filter(|id| !id.trim().is_empty())
}

/// The repository's pull request template: GitHub's and Gitea's `pull_request_template.md` in the
/// usual places, or GitLab's default merge request template.
fn pr_template(root: &std::path::Path) -> Option<String> {
    [
        ".github/pull_request_template.md",
        ".github/PULL_REQUEST_TEMPLATE.md",
        "pull_request_template.md",
        "PULL_REQUEST_TEMPLATE.md",
        "docs/pull_request_template.md",
        "docs/PULL_REQUEST_TEMPLATE.md",
        ".gitea/pull_request_template.md",
        ".gitlab/merge_request_templates/Default.md",
        ".gitlab/merge_request_templates/default.md",
    ]
    .iter()
    .find_map(|p| std::fs::read_to_string(root.join(p)).ok())
    .filter(|t| !t.trim().is_empty())
}

fn other(e: impl ToString) -> GitError {
    GitError::Other {
        code: None,
        stderr: e.to_string(),
    }
}

impl RepoView {
    /// The GitHub and GitLab menus' actions, for `on_actions`.
    pub(crate) fn on_hosting_actions(&self, el: Div, cx: &mut Context<Self>) -> Div {
        el.on_action(cx.listener(|this, _: &CreatePullRequest, w, cx| this.create_pull(w, cx)))
            .on_action(cx.listener(|this, _: &CreateMergeRequest, w, cx| this.create_pull(w, cx)))
            .on_action(cx.listener(|this, _: &ViewPullRequests, _, cx| {
                this.open_left(LeftTool::PullRequests, cx)
            }))
            .on_action(cx.listener(|this, _: &ViewMergeRequests, _, cx| {
                this.open_left(LeftTool::PullRequests, cx)
            }))
            .on_action(cx.listener(|this, _: &SyncFork, w, cx| this.sync_fork(w, cx)))
            // a tool window shows only for its service: GitLab's is Merge Requests
            .on_action(
                cx.listener(|this, _: &ActivatePullRequestsToolWindow, w, cx| {
                    this.pulls_activate(false, w, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &ActivateMergeRequestsToolWindow, w, cx| {
                    this.pulls_activate(true, w, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ShareOnGitHub, w, cx| {
                this.share_project(Service::GitHub, w, cx)
            }))
            .on_action(cx.listener(|this, _: &ShareOnGitLab, w, cx| {
                this.share_project(Service::GitLab, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CloneFromGitHub, w, cx| {
                this.clone_dialog_on(crate::git_menu::CLONE_GITHUB, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CloneFromGitLab, w, cx| {
                this.clone_dialog_on(crate::git_menu::CLONE_GITLAB, w, cx)
            }))
            .on_action(cx.listener(|this, _: &ManageGitHubAccounts, w, cx| {
                this.settings_page(Some("Version Control/GitHub".into()), w, cx)
            }))
            .on_action(cx.listener(|this, _: &ManageGitLabAccounts, w, cx| {
                this.settings_page(Some("Version Control/GitLab".into()), w, cx)
            }))
    }

    /// Shows the GitHub and GitLab menus' items for this project while its window is the
    /// active one: the menu bar is the app's, so it follows the project in front. With Spork in
    /// the background any project may set it; activating a window draws it again.
    pub(crate) fn sync_hosting_menus(&self, cx: &mut App) {
        if !self.window_active && cx.active_window().is_some() {
            return;
        }
        let service = self.hosted.as_ref().map(|h| h.service);
        let shown = (
            service == Some(Service::GitHub),
            service == Some(Service::GitLab),
        );
        if MENU_HOSTING.replace(shown) != shown {
            cx.defer(crate::shell::refresh_menus);
        }
    }

    /// Find Action's Pull Requests (`gitlab`: Merge Requests): shows the tool window, or hides
    /// it when it shows, as the reference app's tool window actions do; nothing on another service's project.
    fn pulls_activate(&mut self, gitlab: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .hosted
            .as_ref()
            .is_some_and(|h| (h.service == Service::GitLab) == gitlab)
        {
            self.toggle_left(LeftTool::PullRequests, window, cx)
        }
    }

    /// Git ▸ GitHub ▸ Create Pull Request… (GitLab: Create Merge Request…): from the current
    /// branch.
    fn create_pull(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let Some(branch) = repo.head.clone() else {
            return window.push_notification(
                Notification::error(format!(
                    "Check out a branch to create a {} from.",
                    self.hosted
                        .as_ref()
                        .map_or("pull request", |h| h.service.noun())
                )),
                cx,
            );
        };
        self.pull_request_dialog(branch, window, cx)
    }

    /// Git ▸ GitHub ▸ Sync Fork: rebases the current branch onto the default branch of the
    /// repository this one was forked from, its `upstream` remote (added when missing).
    fn sync_fork(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(h) = &self.hosted else { return };
        let (service, host, project) = (h.service, h.host.clone(), h.project.clone());
        self.run_outcome(
            window,
            cx,
            "Successfully rebased GitHub fork".into(),
            move |root| {
                const UPSTREAM: &str = "upstream";
                let added = match remote::remotes(root).iter().any(|r| r.name == UPSTREAM) {
                    true => None,
                    false => {
                        let (_, token) = account_token(Some(root), &host)
                            .ok_or_else(|| other("No suitable GitHub account found"))?;
                        let (url, branch) = Client::new(service, &host, &token)
                            .fork_parent(&project)
                            .map_err(other)?
                            .ok_or_else(|| other(format!("Remote {UPSTREAM} was not added")))?;
                        remote::add(root, UPSTREAM, &url)?;
                        Some(branch)
                    }
                };
                remote::fetch(root, UPSTREAM)?;
                let branch = added
                    .or_else(|| remote::default_branch(root, UPSTREAM))
                    .ok_or_else(|| {
                        other("Unable to determine upstream branch - no default branch set for repository")
                    })?;
                spork_git::rebase(root, &format!("{UPSTREAM}/{branch}"))
            },
        )
    }

    /// Finds the repository's hosting service and loads its open pull / merge requests in the
    /// background. Runs when a repository opens and after fetches, not on every file change.
    pub(crate) fn load_pulls(&mut self, cx: &mut Context<Self>) {
        let default = self.default_remote();
        let Some(Ok(repo)) = &self.repo else { return };
        let before = self.hosted.take();
        self.hosted = Hosted::find(&repo.remotes, default.as_deref());
        let Some(h) = &mut self.hosted else { return };
        // a reload keeps the selection
        if let Some(before) = before.filter(|b| b.service == h.service && b.project == h.project) {
            h.selected = before.selected;
            h.details = before.details;
            h.tab = before.tab;
            h.show_events = before.show_events;
            h.flat = before.flat;
        }
        if let Some(pulls) = TEST_PULLS.with_borrow(Clone::clone) {
            h.pulls = Some(Ok(pulls));
            self.pulls_show_pending(cx);
            return cx.notify();
        }
        let (service, host, project) = (h.service, h.host.clone(), h.project.clone());
        let (ci_host, ci_project) = (host.clone(), project.clone());
        let root = match &self.repo {
            Some(Ok(repo)) => Some(repo.root.clone()),
            _ => None,
        };
        let ci_root = root.clone();
        self._pulls_load = Some(cx.spawn(async move |this, cx| {
            let pulls = cx
                .background_spawn(async move {
                    let (_, token) = account_token(root.as_deref(), &host)
                        .ok_or_else(|| format!("Connect {} to see them.", service.name()))?;
                    Client::new(service, &host, &token)
                        .pull_requests(&project)
                        .map_err(|e| e.to_string())
                })
                .await;
            let listed = pulls.clone().unwrap_or_default();
            this.update(cx, |this, cx| {
                if let Some(h) = &mut this.hosted {
                    h.pulls = Some(pulls);
                }
                this.pulls_show_pending(cx);
                cx.notify();
            })
            .ok();
            // shortcut: CI and reviews for the first 30 only (two or three requests each); page if lists grow.
            let (checks, reviews) = cx
                .background_spawn(async move {
                    let Some((_, token)) = account_token(ci_root.as_deref(), &ci_host) else {
                        return (Vec::new(), Vec::new());
                    };
                    let client = Client::new(service, &ci_host, &token);
                    let first = || listed.iter().take(30);
                    let checks = first()
                        .filter_map(|pr| Some((pr.number, client.checks(&ci_project, pr).ok()??)))
                        .collect();
                    let reviews = first()
                        .filter_map(|pr| Some((pr.number, client.review(&ci_project, pr).ok()??)))
                        .collect();
                    (checks, reviews)
                })
                .await;
            this.update(cx, |this, cx| {
                if let Some(h) = &mut this.hosted {
                    h.checks = checks;
                    h.reviews = reviews;
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// The Pull Requests (GitLab: Merge Requests) tool window: the remote's open requests.
    pub(crate) fn pull_requests_window(&mut self, cx: &mut Context<Self>) -> Div {
        let Some(h) = &self.hosted else {
            return v_flex();
        };
        let title = match h.service {
            Service::GitLab => "Merge Requests",
            _ => "Pull Requests",
        };
        let header =
            spork_workbench::shell::tool_window_header(title).children(self.pulls_tabs(cx));
        let open = match h.tab {
            PullsTab::List => None,
            PullsTab::Details => self.pulls_details_pane(cx),
            PullsTab::Timeline => self.pulls_timeline_pane(cx),
        };
        if let Some(open) = open {
            return v_flex().size_full().child(header).child(open);
        }
        let rows = self.pulls_rows(cx);
        v_flex().size_full().child(header).child(
            div()
                .id("pulls-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .pb_2()
                .child(
                    v_flex()
                        .id("pulls-list")
                        .role(Role::List)
                        .aria_label(format!("{title} List"))
                        .children(rows),
                ),
        )
    }

    /// The list's rows: each request with its number, author, review and CI; or why there are
    /// none (loading, not connected), a click on which connects an account.
    fn pulls_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(h) = &self.hosted else {
            return Vec::new();
        };
        let tokens = spork_workbench::Tokens::get(cx);
        let theme = cx.theme();
        let (green, warning, danger) = (theme.green, theme.warning, theme.danger);
        let note = |text: String| {
            div()
                .id("pulls-note")
                .role(Role::Label)
                .aria_label(text.clone())
                .px_3()
                .py_1()
                .text_color(tokens.text_secondary)
                .child(text)
        };
        let pulls = match &h.pulls {
            None => return vec![note("Loading…".into()).into_any_element()],
            Some(Err(e)) => {
                return vec![
                    note(e.clone())
                        .cursor_pointer()
                        .on_click(
                            cx.listener(|this, _, window, cx| this.connect_dialog(window, cx)),
                        )
                        .into_any_element(),
                ];
            }
            Some(Ok(pulls)) => pulls,
        };
        let view = cx.entity();
        pulls
            .iter()
            .map(|pr| {
                let selected = h.selected == Some(pr.number);
                let ci = h
                    .checks
                    .iter()
                    .find(|(n, _)| *n == pr.number)
                    .map(|(_, c)| match c {
                        Checks::Passed => ("✓", green),
                        Checks::Running => ("●", warning),
                        Checks::Failed => ("✗", danger),
                    });
                let review =
                    h.reviews
                        .iter()
                        .find(|(n, _)| *n == pr.number)
                        .map(|(_, r)| match r {
                            Review::Approved => ("approved", green),
                            Review::ChangesRequested => ("changes asked", danger),
                        });
                let number = pr.number;
                let reference = request_ref(h.service, number);
                h_flex()
                    .id(("pull", number as usize))
                    .role(Role::Row)
                    .aria_label(pr.title.clone())
                    .aria_selected(selected)
                    .px_3()
                    .py_1()
                    .gap_2()
                    .cursor_pointer()
                    .when(selected, |row| row.bg(tokens.selection_inactive))
                    .when(!selected, |row| {
                        row.hover(|row| row.bg(tokens.selection_hover))
                    })
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().truncate().child(pr.title.clone()))
                            .child(div().text_color(tokens.text_secondary).truncate().child(
                                match (pr.draft, pr.author.is_empty()) {
                                    (true, _) => format!("{reference} · draft, by {}", pr.author),
                                    (false, true) => reference,
                                    (false, false) => format!("{reference} · by {}", pr.author),
                                },
                            )),
                    )
                    .children(review.map(|(text, color)| div().text_color(color).child(text)))
                    .children(ci.map(|(mark, color)| div().text_color(color).child(mark)))
                    // a right click selects the row it opens the menu on, as the reference app's list
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, _, cx| this.pulls_select(number, cx)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.pulls_select(number, cx)))
                    .context_menu(pulls_list_menu(view.clone(), h.service, pr.clone()))
                    .into_any_element()
            })
            .collect()
    }

    /// The request Open in Worktree opened this window for, in the tool window.
    fn pulls_show_pending(&mut self, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let root = repo
            .root
            .canonicalize()
            .unwrap_or_else(|_| repo.root.clone());
        let number = SHOW_IN_WINDOW.with_borrow_mut(|pending| {
            let ix = pending.iter().position(|(path, _)| *path == root)?;
            Some(pending.remove(ix).1)
        });
        if let Some(number) = number {
            self.open_left(LeftTool::PullRequests, cx);
            self.pulls_show(number, cx);
        }
    }

    /// View Pull Request (GitLab: Show Merge Request): the request's details in the tool window,
    /// its changes fetched from the remote.
    fn pulls_show(&mut self, number: u64, cx: &mut Context<Self>) {
        let Some(h) = &mut self.hosted else { return };
        h.selected = Some(number);
        h.tab = PullsTab::Details;
        let keep = h.details.as_ref().is_some_and(|d| d.number == number);
        if !keep {
            h.details = Some(PullDetails {
                number,
                changes: None,
                file: None,
                collapsed: HashSet::new(),
                timeline: false,
                _load: None,
            });
            self.pulls_load_details(cx);
        }
        cx.notify();
    }

    /// Refresh Pull Request (GitLab: Refresh Merge Request): fetches the request's changes again.
    fn pulls_load_details(&mut self, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let root = repo.root.clone();
        let Some(h) = &mut self.hosted else { return };
        let (service, remote) = (h.service, h.remote.clone());
        let Some(number) = h.details.as_ref().map(|d| d.number) else {
            return;
        };
        let pr = match &h.pulls {
            Some(Ok(pulls)) => pulls.iter().find(|pr| pr.number == number).cloned(),
            _ => None,
        };
        let Some(details) = &mut h.details else {
            return;
        };
        let Some(pr) = pr else {
            details.changes = Some(Err(format!(
                "{} isn't open any more",
                request_ref(service, number)
            )));
            return cx.notify();
        };
        details.changes = None;
        details._load = Some(cx.spawn(async move |this, cx| {
            let changes = cx
                .background_spawn(async move {
                    fetch_request_changes(&root, &remote, service, &pr).map_err(|e| e.to_string())
                })
                .await;
            this.update(cx, |this, cx| {
                if let Some(d) = this.hosted.as_mut().and_then(|h| h.details.as_mut()) {
                    d.changes = Some(changes);
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// The tool window's header tabs while a request is open: the list, the request, and its
    /// timeline once opened.
    fn pulls_tabs(&self, cx: &mut Context<Self>) -> Option<Div> {
        let h = self.hosted.as_ref()?;
        let d = h.details.as_ref()?;
        let tab = |id: &'static str, label: String, which: PullsTab| {
            let selected = h.tab == which;
            spork_workbench::shell::header_tab(id, label.clone(), selected, cx)
                .role(Role::Tab)
                .aria_label(label)
                .aria_selected(selected)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(h) = &mut this.hosted {
                        h.tab = which;
                    }
                    cx.notify();
                }))
        };
        let timeline = match h.service {
            Service::GitLab => format!("Timeline for !{}", d.number),
            _ => format!("#{} Timeline", d.number),
        };
        Some(
            h_flex()
                .gap_1()
                .child(tab("pulls-tab-list", h.project.clone(), PullsTab::List))
                .child(tab(
                    "pulls-tab-open",
                    request_ref(h.service, d.number),
                    PullsTab::Details,
                ))
                .when(d.timeline, |tabs| {
                    tabs.child(tab("pulls-tab-timeline", timeline, PullsTab::Timeline))
                }),
        )
    }

    /// View Timeline: the open request's description and what happened since.
    fn pulls_view_timeline(&mut self, cx: &mut Context<Self>) {
        if let Some(h) = &mut self.hosted
            && let Some(d) = &mut h.details
        {
            d.timeline = true;
            h.tab = PullsTab::Timeline;
        }
        cx.notify();
    }

    /// The open request's timeline: its description, then its commits.
    // shortcut: the commits as git fetched them; comments, reviews and events come from the
    // service's timeline API, which Spork's client doesn't read yet
    fn pulls_timeline_pane(&self, cx: &mut Context<Self>) -> Option<Div> {
        let h = self.hosted.as_ref()?;
        let d = h.details.as_ref()?;
        let pr = match &h.pulls {
            Some(Ok(pulls)) => pulls.iter().find(|pr| pr.number == d.number)?.clone(),
            _ => return None,
        };
        let tokens = spork_workbench::Tokens::get(cx);
        let gitlab = h.service == Service::GitLab;
        let failed = matches!(d.changes, Some(Err(_)));
        let description = match pr.description.trim().is_empty() {
            true => "No description provided".to_owned(),
            false => pr.description.clone(),
        };
        let item = |id: &'static str, who: &str, text: String| {
            v_flex()
                .id(id)
                .role(Role::Group)
                .aria_label(text.clone())
                .gap_1()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(who.to_owned()),
                )
                .child(text)
        };
        let items: Vec<AnyElement> = match &d.changes {
            None => vec![pulls_note("Loading…", tokens)],
            Some(Err(_)) => vec![pulls_note(
                match gitlab {
                    true => "Failed to load timeline",
                    false => "Can't load timeline items",
                },
                tokens,
            )],
            Some(Ok(changes)) => {
                let mut items =
                    vec![item("timeline-description", &pr.author, description).into_any_element()];
                // GitLab tells of commits in its own notes, which come from its API
                if !gitlab && !changes.log.is_empty() {
                    let added = match changes.log.len() {
                        1 => "added a commit to this pull request".to_owned(),
                        n => format!("added {n} commits to this pull request"),
                    };
                    items.push(
                        item("timeline-commits", &pr.author, added)
                            .children(changes.log.iter().map(|(hash, subject)| {
                                h_flex()
                                    .gap_2()
                                    .child(
                                        div().text_color(tokens.text_secondary).child(hash.clone()),
                                    )
                                    .child(subject.clone())
                            }))
                            .into_any_element(),
                    );
                }
                items
            }
        };
        let view = cx.entity();
        let title = format!("{} {}", pr.title, request_ref(h.service, pr.number));
        let menu = pulls_timeline_menu(view, h.service, pr, failed, h.show_events);
        Some(
            v_flex().size_full().child(
                v_flex()
                    .id("pull-timeline")
                    .role(Role::Group)
                    .aria_label(format!("{} Timeline", request_name(h.service)))
                    .size_full()
                    .px_3()
                    .py_2()
                    .gap_3()
                    .child(div().font_weight(FontWeight::BOLD).child(title))
                    .children(items)
                    .context_menu(menu),
            ),
        )
    }

    /// The open request: its title, number and branches, and the files it changes under their
    /// toolbar (Github.PullRequest.Changes.Toolbar).
    fn pulls_details_pane(&self, cx: &mut Context<Self>) -> Option<Div> {
        let h = self.hosted.as_ref()?;
        let d = h.details.as_ref()?;
        let pr = match &h.pulls {
            Some(Ok(pulls)) => pulls.iter().find(|pr| pr.number == d.number)?.clone(),
            _ => return None,
        };
        let tokens = spork_workbench::Tokens::get(cx);
        let view = cx.entity();
        let name = request_name(h.service);
        let summary = v_flex()
            .id("pull-summary")
            .role(Role::Group)
            .aria_label(format!("{name} Details"))
            .gap_1()
            .px_3()
            .py_2()
            .child(div().font_weight(FontWeight::BOLD).child(pr.title.clone()))
            .child(
                div()
                    .text_color(tokens.text_secondary)
                    .child(match pr.author.is_empty() {
                        true => request_ref(h.service, pr.number),
                        false => {
                            format!("{} · by {}", request_ref(h.service, pr.number), pr.author)
                        }
                    }),
            )
            .child(
                spork_workbench::controls::link("pull-view-timeline", "View Timeline", cx)
                    .on_click(cx.listener(|this, _, _, cx| this.pulls_view_timeline(cx))),
            )
            .child(
                div()
                    .id("pull-branches")
                    .role(Role::Label)
                    .aria_label(format!("{} ← {}", pr.base, pr.head))
                    .child(format!("{} ← {}", pr.base, pr.head)),
            )
            .context_menu(pulls_details_menu(view.clone(), h.service, pr.clone()));
        let picked = d.file.clone();
        let group = {
            let flat = h.flat;
            spork_workbench::ActionButton::new("pull-changes-group", "actions/groupBy")
                .tooltip("Group By")
                .popup_marker()
                .dropdown_menu(move |menu, _, _| pulls_group_by(menu, &view, flat))
        };
        let toolbar = h_flex()
            .id("pull-changes-toolbar")
            .role(Role::Toolbar)
            .aria_label(format!("{name} Changes Toolbar"))
            .px_2()
            .gap_0p5()
            .child(
                spork_workbench::ActionButton::new("pull-changes-diff", "vcs/diff")
                    .tooltip("Show Diff (⌘D)")
                    .disabled(picked.is_none())
                    .on_click(cx.listener(|this, _, window, cx| this.pulls_show_diff(window, cx))),
            )
            .child(div().w(px(1.)).h(px(16.)).mx_1().bg(tokens.separator))
            .child(group);
        let body = match &d.changes {
            None => vec![pulls_note("Loading…", tokens)],
            Some(Err(e)) => vec![pulls_note(e, tokens)],
            Some(Ok(changes)) if changes.files.is_empty() => {
                vec![pulls_note(
                    "Pull request does not contain any changes",
                    tokens,
                )]
            }
            Some(Ok(changes)) => self.pulls_change_rows(&pr, changes, cx),
        };
        let commits = match &d.changes {
            Some(Ok(changes)) => format!("All commits ({})", changes.log.len()),
            _ => "All commits".to_owned(),
        };
        Some(
            v_flex()
                .size_full()
                .child(summary)
                .child(div().px_3().pb_1().child(commits))
                .child(toolbar)
                .child(
                    div()
                        .id("pull-changes-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .child(
                            v_flex()
                                .id("pull-changes")
                                .role(Role::Tree)
                                .aria_label(format!("{name} Changes"))
                                .children(body),
                        ),
                ),
        )
    }

    /// The changed files: under their folders (Group By ▸ Directory), or alone by name.
    fn pulls_change_rows(
        &self,
        pr: &PullRequest,
        changes: &PullChanges,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(h) = &self.hosted else {
            return Vec::new();
        };
        let Some(d) = &h.details else {
            return Vec::new();
        };
        let tokens = spork_workbench::Tokens::get(cx);
        let view = cx.entity();
        let mut files: Vec<&(char, String)> = changes.files.iter().collect();
        files.sort_by(|a, b| a.1.cmp(&b.1));
        let mut rows = Vec::new();
        let mut folder: Option<&str> = None;
        for (status, path) in files {
            let (dir, file_name) = match path.rsplit_once('/') {
                Some((dir, file_name)) => (dir, file_name),
                None => ("", path.as_str()),
            };
            if !h.flat && !dir.is_empty() && folder != Some(dir) {
                folder = Some(dir);
                let open = !d.collapsed.contains(dir);
                let toggle = dir.to_owned();
                rows.push(
                    h_flex()
                        .id(SharedString::from(format!("pull-dir-{dir}")))
                        .role(Role::Row)
                        .aria_label(dir.to_owned())
                        .h(px(spork_workbench::tokens::metrics::ROW_HEIGHT))
                        .px_2()
                        .gap_1()
                        .cursor_pointer()
                        .child(
                            spork_workbench::icon(match open {
                                true => "general/chevronDown",
                                false => "general/chevronRight",
                            })
                            .size(px(16.)),
                        )
                        .child(spork_workbench::icon("nodes/folder").size(px(16.)))
                        .child(dir.to_owned())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(d) = this.hosted.as_mut().and_then(|h| h.details.as_mut())
                                && !d.collapsed.remove(&toggle)
                            {
                                d.collapsed.insert(toggle.clone());
                            }
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
            if !h.flat && d.collapsed.contains(dir) {
                continue;
            }
            let selected = d.file.as_deref() == Some(path.as_str());
            use spork_workbench::FileStatus;
            let color = spork_workbench::file_status_color(
                match status {
                    'A' => FileStatus::Added,
                    'D' => FileStatus::Deleted,
                    'R' => FileStatus::Renamed,
                    _ => FileStatus::Modified,
                },
                cx,
            );
            let (pick, menu_pick) = (path.clone(), path.clone());
            rows.push(
                h_flex()
                    .id(SharedString::from(format!("pull-file-{path}")))
                    .role(Role::Row)
                    .aria_label(file_name.to_owned())
                    .aria_selected(selected)
                    .h(px(spork_workbench::tokens::metrics::ROW_HEIGHT))
                    .pl(match h.flat || dir.is_empty() {
                        true => px(8.),
                        false => px(28.),
                    })
                    .pr_2()
                    .gap_1()
                    .cursor_pointer()
                    .when(selected, |row| row.bg(tokens.selection_inactive))
                    .child(spork_workbench::image(
                        spork_workbench::file_icon(file_name),
                        cx,
                    ))
                    .child(div().text_color(color).child(file_name.to_owned()))
                    .when(h.flat && !dir.is_empty(), |row| {
                        row.child(
                            div()
                                .text_color(tokens.text_secondary)
                                .child(dir.to_owned()),
                        )
                    })
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, _, cx| this.pulls_pick_file(&menu_pick, cx)),
                    )
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        this.pulls_pick_file(&pick, cx);
                        if event.click_count() == 2 {
                            this.pulls_show_diff(window, cx)
                        }
                    }))
                    .context_menu(pulls_changes_menu(
                        view.clone(),
                        h.service,
                        pr.clone(),
                        h.flat,
                    ))
                    .into_any_element(),
            );
        }
        rows
    }

    fn pulls_pick_file(&mut self, path: &str, cx: &mut Context<Self>) {
        if let Some(d) = self.hosted.as_mut().and_then(|h| h.details.as_mut()) {
            d.file = Some(path.to_owned());
        }
        cx.notify();
    }

    /// The open request's changes and selected file: `(base, head, path)`.
    fn pulls_picked_change(&self) -> Option<(String, String, String)> {
        let d = self.hosted.as_ref()?.details.as_ref()?;
        let Some(Ok(changes)) = &d.changes else {
            return None;
        };
        Some((changes.base.clone(), changes.head.clone(), d.file.clone()?))
    }

    /// Show Diff: the selected file's changes in the request.
    // shortcut: in a dialog, as Spork's other two-revision diffs; an editor tab once Spork's
    // main area can show one besides the log's commit
    fn pulls_show_diff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((base, head, path)) = self.pulls_picked_change() else {
            return;
        };
        let title = path.clone();
        self.run_op(
            window,
            cx,
            move |root| {
                let out = spork_git::run(
                    root,
                    &[
                        "diff",
                        "--no-color",
                        "--no-ext-diff",
                        "--find-renames",
                        &base,
                        &head,
                        "--",
                        &path,
                    ],
                )?;
                Ok(String::from_utf8_lossy(&out).into_owned())
            },
            move |this, patch, window, cx| {
                let diff = cx.new(|cx| DiffState::new([], cx));
                this.set_patch(&diff, &patch, cx);
                let height = (window.viewport_size().height * 0.75).max(px(300.));
                window.open_dialog(cx, move |dialog, _, _| {
                    dialog
                        .width(px(1100.))
                        .title(title.clone())
                        .child(div().h(height).child(Diff::new(&diff).size_full()))
                });
            },
        );
    }

    /// Jump to Source: the selected file in the editor.
    fn pulls_jump_to_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some((_, _, path)), Some(Ok(repo))) = (self.pulls_picked_change(), &self.repo) else {
            return;
        };
        let path = repo.root.join(path);
        self.open_in_editor(Some(path), window, cx)
    }

    /// Expand All / Collapse All of the request's changes.
    fn pulls_fold_all(&mut self, collapse: bool, cx: &mut Context<Self>) {
        let Some(d) = self.hosted.as_mut().and_then(|h| h.details.as_mut()) else {
            return;
        };
        d.collapsed = match (&d.changes, collapse) {
            (Some(Ok(changes)), true) => changes
                .files
                .iter()
                .filter_map(|(_, path)| Some(path.rsplit_once('/')?.0.to_owned()))
                .collect(),
            _ => HashSet::new(),
        };
        cx.notify();
    }

    fn pulls_select(&mut self, number: u64, cx: &mut Context<Self>) {
        if let Some(h) = &mut self.hosted {
            h.selected = Some(number);
        }
        cx.notify();
    }

    /// The list's Refresh List: the service's requests again.
    fn pulls_reload(&mut self, cx: &mut Context<Self>) {
        if let Some(h) = &mut self.hosted {
            h.pulls = None;
        }
        self.load_pulls(cx);
    }

    /// Checkout Branch: fetches the request's branch and checks it out, as a local branch of
    /// the same name tracking it. A fork's branch isn't on this remote: its published ref
    /// (`refs/pull/<n>/head`) comes in as `pr/<n>`.
    // shortcut: a fork's branch comes from the service's ref, not from a remote added for the
    // fork as the reference app's does; add one when Spork knows the fork's URL (the API's head repo)
    fn pulls_checkout(&mut self, pr: PullRequest, window: &mut Window, cx: &mut Context<Self>) {
        let Some(h) = &self.hosted else { return };
        let (service, remote) = (h.service, h.remote.clone());
        self.run_op(
            window,
            cx,
            move |root| {
                let local = match pr.fork {
                    true => {
                        let (source, branch) = service.head_ref(pr.number, &pr.head);
                        remote::checkout_pull_request(root, &remote, &source, &branch)?;
                        branch
                    }
                    false => {
                        fetch_request_branch(root, &remote, &pr.head)?;
                        spork_git::checkout_remote(root, &format!("{remote}/{}", pr.head))?
                    }
                };
                Ok(format!("Switched to {local}"))
            },
            |_, message, window, cx| window.push_notification(Notification::success(message), cx),
        );
    }

    /// Open in Worktree: fetches the request's branch, checks it out in a new worktree in
    /// Spork's `reviewWorktrees` folder and opens that in a new window, as the reference app does (its
    /// folder is in its system folder); a worktree already on the branch opens instead.
    fn pulls_open_in_worktree(
        &mut self,
        pr: PullRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(h) = &self.hosted else { return };
        let (service, remote, number) = (h.service, h.remote.clone(), pr.number);
        self.run_op(
            window,
            cx,
            move |root| {
                let branch = match pr.fork {
                    true => {
                        let (source, branch) = service.head_ref(pr.number, &pr.head);
                        spork_git::run(
                            root,
                            &["fetch", &remote, &format!("+{source}:refs/heads/{branch}")],
                        )?;
                        branch
                    }
                    false => {
                        fetch_request_branch(root, &remote, &pr.head)?;
                        let tracked = format!("{remote}/{}", pr.head);
                        let local = format!("refs/heads/{}", pr.head);
                        // a local branch to check out there, tracking the request's
                        if spork_git::run(root, &["rev-parse", "--verify", "--quiet", &local])
                            .is_err()
                        {
                            spork_git::run(root, &["branch", "--track", &pr.head, &tracked])?;
                        }
                        pr.head
                    }
                };
                let worktrees = spork_git::worktrees(root)?;
                if let Some(open) = worktrees
                    .iter()
                    .find(|w| w.branch.as_deref() == Some(branch.as_str()))
                {
                    return Ok(open.path.clone());
                }
                let parent = crate::repos::config_dir()
                    .ok_or_else(|| other("No folder for review worktrees"))?
                    .join("reviewWorktrees");
                std::fs::create_dir_all(&parent).map_err(other)?;
                let name = root
                    .file_name()
                    .map_or_else(|| "repo".into(), |n| n.to_string_lossy().into_owned());
                let leaf = branch.rsplit('/').next().unwrap_or(&branch);
                let path = parent.join(format!("{name}-{leaf}"));
                spork_git::add_worktree(root, &path, &branch)?;
                Ok(path)
            },
            move |_, path, _, cx| {
                // its window shows the request once its list has loaded, as the reference app's does
                let key = path.canonicalize().unwrap_or_else(|_| path.clone());
                SHOW_IN_WINDOW.with_borrow_mut(|pending| pending.push((key, number)));
                crate::open_repo_window(vec![path], None, cx);
            },
        );
    }
    /// Asks for a personal access token, checks it with the service and saves it in the git
    /// credential helper, where git's own HTTPS pushes and fetches find it too.
    pub(crate) fn connect_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (service, host) = self.hosted.as_ref().map_or_else(
            || (Service::GitHub, "github.com".to_owned()),
            |h| (h.service, h.host.clone()),
        );
        self.connect_service(service, host, window, cx)
    }

    /// Connect dialog: the service, its address (any host, so self-hosted servers work) and a
    /// token. Connecting again with another token adds another account on that host.
    pub(crate) fn connect_service(
        &mut self,
        service: Service,
        host: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.connect_as = service;
        let client_id = github_client_id();
        let token = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("Token")
        });
        let address = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Server, e.g. github.com or git.example.com")
                .default_value(host)
        });
        let view = cx.entity();
        let muted = cx.theme().muted_foreground;
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let service = view.read(cx).connect_as;
            let host = address.read(cx).value().to_string();
            let (page, scope) = service.token_page(host.trim());
            let pick = |s: Service| {
                let (view, address) = (view.clone(), address.clone());
                Button::new(s.id())
                    .small()
                    .label(s.name())
                    .selected(s == service)
                    .on_click(move |_, window, cx| {
                        // a public address follows the service; a typed one stays
                        let typed = address.read(cx).value().trim().to_owned();
                        if Service::ALL.iter().any(|o| o.default_host() == typed) {
                            address.update(cx, |a, cx| a.set_value(s.default_host(), window, cx));
                        }
                        view.update(cx, |this, cx| {
                            this.connect_as = s;
                            cx.notify();
                        })
                    })
            };
            let browser = (service == Service::GitHub)
                .then(|| client_id.clone())
                .flatten()
                .map(|id| {
                    let (view, host) = (view.clone(), host.trim().to_owned());
                    Button::new("connect-browser")
                        .small()
                        .label("Sign in with the browser instead…")
                        .on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            let (host, id) = (host.clone(), id.clone());
                            view.update(cx, |this, cx| this.github_sign_in(host, id, window, cx))
                        })
                });
            let (view, token, address) = (view.clone(), token.clone(), address.clone());
            dialog
                .title("Connect an account")
                .description(
                    v_flex()
                        .gap_2()
                        .child(h_flex().flex_wrap().gap_1().children(Service::ALL.map(pick)))
                        .child(spork_workbench::TextField::new(&address))
                        .child(div().text_sm().text_color(muted).child(format!(
                            "Paste a personal access token with the {scope} scope, created at {page}. \
                             It is kept in your git credential helper, never in Spork's files."
                        )))
                        .child(spork_workbench::TextField::new(&token))
                        .children(browser.map(|b| h_flex().child(b))),
                )
                .confirm()
                .ok_text("Connect")
                .on_ok(move |_, window, cx| {
                    let token = token.read(cx).value().trim().to_owned();
                    let host = address
                        .read(cx)
                        .value()
                        .trim()
                        .trim_start_matches("https://")
                        .trim_end_matches('/')
                        .to_owned();
                    if host.is_empty() || token.is_empty() {
                        return false;
                    }
                    view.update(cx, |this, cx| {
                        let here = this.hosted.as_ref().is_some_and(|h| h.host == host);
                        this.run_op(
                            window,
                            cx,
                            move |root| {
                                save_account(here.then_some(root), service, &host, &token)
                            },
                            |this, login, window, cx| {
                                window.push_notification(Notification::success(format!("Connected as {login}")), cx);
                                this.load_pulls(cx);
                            },
                        )
                    });
                    true
                })
        });
    }

    /// The Clone dialog's "Log In via GitHub…": in the browser when Spork has a GitHub app to
    /// sign in as, else with a token.
    pub(crate) fn log_in_via_github(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let host = Service::GitHub.default_host().to_owned();
        match github_client_id() {
            Some(id) => self.github_sign_in(host, id, window, cx),
            None => self.connect_service(Service::GitHub, host, window, cx),
        }
    }

    /// Sign in to GitHub in the browser (device flow): shows a code to enter on GitHub, then waits
    /// for the approval and keeps the token like a pasted one.
    fn github_sign_in(
        &mut self,
        host: String,
        client_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let here = self.hosted.as_ref().is_some_and(|h| h.host == host);
        let root = match &self.repo {
            Some(Ok(repo)) if here => Some(repo.root.clone()),
            _ => None,
        };
        let (start_host, start_id) = (host.clone(), client_id.clone());
        self.run_op(
            window,
            cx,
            move |_| spork_hosting::github_device_code(&start_host, &start_id).map_err(other),
            move |this, code, window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(code.user_code.clone()));
                let (view, url, shown) =
                    (cx.entity(), code.verification_uri.clone(), code.user_code.clone());
                let muted = cx.theme().muted_foreground;
                window.open_dialog(cx, move |dialog, _, _| {
                    let (url, view) = (url.clone(), view.clone());
                    dialog
                        .title("Sign in to GitHub")
                        .child(
                            v_flex()
                                .gap_3()
                                .child(div().text_sm().text_color(muted).child(
                                    "Enter this code on GitHub (it is copied already), then approve Spork:",
                                ))
                                .child(
                                    div()
                                        .text_2xl()
                                        .font_weight(FontWeight::BOLD)
                                        .child(shown.clone()),
                                )
                                .child(h_flex().child(
                                    Button::new("open-github")
                                        .small()
                                        .label("Open GitHub")
                                        .on_click(move |_, _, cx| cx.open_url(&url)),
                                ))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(muted)
                                        .child("Waiting for your approval…"),
                                ),
                        )
                        // closing it stops waiting
                        .on_close(move |_, _, cx| view.update(cx, |this, _| this._sign_in = None))
                });
                cx.open_url(&code.verification_uri);
                let interval = std::time::Duration::from_secs(code.interval.max(1));
                this._sign_in = Some(cx.spawn_in(window, async move |this, cx| {
                    let answer = loop {
                        cx.background_executor().timer(interval).await;
                        let (h, id, device) =
                            (host.clone(), client_id.clone(), code.device_code.clone());
                        let poll = cx
                            .background_spawn(async move {
                                spork_hosting::github_device_token(&h, &id, &device)
                            })
                            .await;
                        match poll {
                            Ok(None) => continue,
                            Ok(Some(token)) => {
                                let (host, root) = (host.clone(), root.clone());
                                break cx
                                    .background_spawn(async move {
                                        save_account(root.as_deref(), Service::GitHub, &host, &token)
                                    })
                                    .await
                                    .map_err(|e| e.to_string());
                            }
                            Err(e) => break Err(e.to_string()),
                        }
                    };
                    this.update_in(cx, |this, window, cx| {
                        window.close_dialog(cx);
                        let note = match answer {
                            Ok(login) => Notification::success(format!("Connected as {login}")),
                            Err(e) => Notification::error(e),
                        };
                        window.push_notification(note, cx);
                        this.load_pulls(cx);
                    })
                    .ok();
                }));
            },
        )
    }

    /// Spork ▸ Accounts…: every connected account by server; use one for this repository, make
    /// one the server's default, or remove one (its token leaves the credential helper).
    pub(crate) fn accounts_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let accounts: Vec<(String, String)> =
            spork_git::config::global_matching(r"^spork\..*\.account$")
                .into_iter()
                .filter_map(|(key, user)| {
                    let host = key.strip_prefix("spork.")?.strip_suffix(".account")?;
                    Some((host.to_owned(), user))
                })
                .collect();
        let (here_host, root) = match (&self.hosted, &self.repo) {
            (Some(h), Some(Ok(repo))) => (Some(h.host.clone()), Some(repo.root.clone())),
            _ => (None, None),
        };
        let used = root
            .as_deref()
            .and_then(|r| spork_git::config::get_local(r, "spork.account"));
        // read once: the dialog redraws often and git config is a process each time
        let defaults: Vec<Option<String>> = accounts
            .iter()
            .map(|(host, _)| spork_git::config::get_global(&format!("spork.{host}.default")))
            .collect();
        let view = cx.entity();
        let muted = cx.theme().muted_foreground;
        window.open_dialog(cx, move |dialog, _, _| {
            let rows = accounts.iter().enumerate().map(|(i, (host, user))| {
                let is_default = defaults[i].as_deref() == Some(user.as_str());
                let here = here_host.as_deref() == Some(host.as_str());
                let action = |id: &'static str,
                              label: &'static str,
                              f: fn(
                    &mut RepoView,
                    String,
                    String,
                    &mut Window,
                    &mut Context<RepoView>,
                )| {
                    let (view, host, user) = (view.clone(), host.clone(), user.clone());
                    Button::new((id, i)).ghost().xsmall().label(label).on_click(
                        move |_, window, cx| {
                            let (host, user) = (host.clone(), user.clone());
                            window.close_dialog(cx);
                            view.update(cx, |this, cx| f(this, host, user, window, cx))
                        },
                    )
                };
                h_flex()
                    .gap_2()
                    .child(div().flex_1().min_w_0().truncate().text_sm().child(format!(
                        "{user} · {host}{}",
                        if is_default { " (default)" } else { "" }
                    )))
                    .when(here && used.as_deref() != Some(user.as_str()), |row| {
                        row.child(action(
                            "account-use",
                            "Use here",
                            |this, _, user, window, cx| {
                                this.run_op(
                                    window,
                                    cx,
                                    move |root| {
                                        spork_git::config::set_local(
                                            root,
                                            "spork.account",
                                            Some(&user),
                                        )
                                    },
                                    |this, (), _, cx| this.load_pulls(cx),
                                )
                            },
                        ))
                    })
                    .when(!is_default, |row| {
                        row.child(action(
                            "account-default",
                            "Make default",
                            |this, host, user, window, cx| {
                                this.run_op(
                                    window,
                                    cx,
                                    move |_| {
                                        spork_git::config::set_global(
                                            &format!("spork.{host}.default"),
                                            &user,
                                        )
                                    },
                                    |this, (), _, cx| this.load_pulls(cx),
                                )
                            },
                        ))
                    })
                    .child(action(
                        "account-remove",
                        "Remove",
                        |this, host, user, window, cx| {
                            this.run_op(
                                window,
                                cx,
                                move |_| {
                                    credential::erase(&host, &user)?;
                                    spork_git::config::remove_global(
                                        &format!("spork.{host}.account"),
                                        Some(&user),
                                    )?;
                                    spork_git::config::remove_global(
                                        &format!("spork.{host}.default"),
                                        Some(&user),
                                    )
                                },
                                |this, (), _, cx| this.load_pulls(cx),
                            )
                        },
                    ))
            });
            let connect = view.clone();
            dialog.title("Accounts").child(
                v_flex()
                    .gap_2()
                    .when(accounts.is_empty(), |d| {
                        d.child(
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child("No accounts connected yet."),
                        )
                    })
                    .children(rows)
                    .child(
                        h_flex().child(
                            Button::new("account-connect")
                                .small()
                                .label("Connect an account…")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    connect.update(cx, |this, cx| this.connect_dialog(window, cx))
                                }),
                        ),
                    ),
            )
        });
    }

    /// Branch menu: push `branch` if needed, then create a pull / merge request (connected) or open
    /// the service's own page for it (not connected).
    pub(crate) fn pull_request_dialog(
        &mut self,
        branch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(h) = &self.hosted else { return };
        let (service, host, project, remote) = (
            h.service,
            h.host.clone(),
            h.project.clone(),
            h.remote.clone(),
        );
        let Some(Ok(repo)) = &self.repo else { return };
        let base = remote::default_branch(&repo.root, &remote).unwrap_or_else(|| "main".into());
        let compare = service.compare_url(&host, &project, &base, &branch);
        let title = repo
            .refs
            .iter()
            .find(|r| r.kind == spork_git::RefKind::Local && r.name == branch)
            .and_then(|r| repo.history.commits.iter().find(|c| c.id == r.commit))
            .map_or_else(|| branch.clone(), |c| c.subject.clone());
        let title_input = cx.new(|cx| InputState::new(window, cx).placeholder("Title"));
        title_input.update(cx, |s, cx| s.set_value(title, window, cx));
        let body_input =
            cx.new(|cx| TextareaState::new(window, cx).placeholder("Description (optional)"));
        // the repository's own pull / merge request template starts the description
        if let Some(template) = pr_template(&repo.root) {
            body_input.update(cx, |s, cx| s.set_value(template, window, cx));
        }
        self.pr_form = Some((title_input.clone(), body_input.clone()));
        let (view, noun) = (cx.entity(), service.noun());
        let muted = cx.theme().muted_foreground;
        // what the AI draft compares against: the remote's copy of the base branch
        let base_ref = format!("{remote}/{base}");
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, title_input, body_input) =
                (view.clone(), title_input.clone(), body_input.clone());
            let (ai_view, base_ref, ai_branch) = (view.clone(), base_ref.clone(), branch.clone());
            let (host, project, remote, branch, base, compare) = (
                host.clone(),
                project.clone(),
                remote.clone(),
                branch.clone(),
                base.clone(),
                compare.clone(),
            );
            dialog
                .title(format!("Create {noun} from {branch}"))
                .description(
                    v_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child(format!("Into {base}. Spork pushes {branch} first.")),
                        )
                        .child(spork_workbench::TextField::new(&title_input))
                        .child(Textarea::new(&body_input).h(px(140.)))
                        .child(
                            Button::new("pr-ai")
                                .ghost()
                                .small()
                                .label("✦ Write with AI")
                                .on_click(move |_, window, cx| {
                                    let task =
                                        AiTask::PullRequest(base_ref.clone(), ai_branch.clone());
                                    ai_view.update(cx, |this, cx| this.ai_start(task, window, cx))
                                }),
                        ),
                )
                .confirm()
                .ok_text(format!("Create {noun}"))
                .on_ok(move |_, window, cx| {
                    let title = title_input.read(cx).value().trim().to_owned();
                    let body = body_input.read(cx).value().trim().to_owned();
                    let (host, project, remote, branch, base, compare) = (
                        host.clone(),
                        project.clone(),
                        remote.clone(),
                        branch.clone(),
                        base.clone(),
                        compare.clone(),
                    );
                    view.update(cx, |this, cx| {
                        this.pr_form = None;
                        this.run_op(
                            window,
                            cx,
                            move |root| {
                                remote::push_branch(root, &remote, &branch)?;
                                let Some((_, token)) = account_token(Some(root), &host) else {
                                    // not connected: the service's own page finishes the job in the browser
                                    return Ok(compare);
                                };
                                Client::new(service, &host, &token)
                                    .create_pull_request(&project, &branch, &base, &title, &body)
                                    .map(|pr| pr.url)
                                    .map_err(other)
                            },
                            |_, url, window, cx| {
                                cx.open_url(&url);
                                window.push_notification(Notification::success(url), cx)
                            },
                        )
                    });
                    true
                })
        });
    }

    /// Unread notifications of the account for this repository's service (GitHub by default);
    /// a click opens one in the browser (M15 §4).
    pub(crate) fn notifications_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (service, host) = self.hosted.as_ref().map_or_else(
            || (Service::GitHub, "github.com".to_owned()),
            |h| (h.service, h.host.clone()),
        );
        let lookup = host.clone();
        self.run_op(
            window,
            cx,
            move |root| {
                let Some((_, token)) = account_token(Some(root), &lookup) else {
                    return Ok(None);
                };
                Client::new(service, &lookup, &token)
                    .notifications()
                    .map(Some)
                    .map_err(other)
            },
            move |this, notices, window, cx| {
                let Some(notices) = notices else {
                    return this.connect_service(service, host, window, cx);
                };
                let theme = cx.theme();
                let (muted, hover) = (theme.muted_foreground, theme.list_hover);
                window.open_dialog(cx, move |dialog, _, _| {
                    let rows = notices.iter().enumerate().map(|(ix, n)| {
                        let url = n.url.clone();
                        v_flex()
                            .id(("notice", ix))
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .hover(move |r| r.bg(hover))
                            .on_click(move |_, _, cx| cx.open_url(&url))
                            .child(div().text_sm().truncate().child(n.title.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("{} · {}", n.repository, n.kind)),
                            )
                    });
                    dialog
                        .title(format!(
                            "{} notifications ({})",
                            service.name(),
                            notices.len()
                        ))
                        .w(px(560.))
                        .child(
                            v_flex()
                                .id("notices")
                                .max_h(px(440.))
                                .children(rows)
                                .when(notices.is_empty(), |d| {
                                    d.child(
                                        div().text_sm().text_color(muted).child("Nothing unread."),
                                    )
                                })
                                .overflow_y_scrollbar(),
                        )
                });
            },
        );
    }

    /// Clone dialog: pick one of the account's repositories; connects first when needed (M15 §2).
    pub(crate) fn account_repos(
        &mut self,
        service: Service,
        host: &'static str,
        url: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_op(
            window,
            cx,
            move |root| {
                let Some((_, token)) = account_token(Some(root), host) else {
                    return Ok(None);
                };
                Client::new(service, host, &token)
                    .repositories()
                    .map(Some)
                    .map_err(other)
            },
            move |this, repos, window, cx| {
                let Some(repos) = repos else {
                    return this.connect_service(service, host.into(), window, cx);
                };
                let items: Vec<CommandItem> = repos
                    .iter()
                    .map(|r| {
                        let private = if r.private { "  (private)" } else { "" };
                        CommandItem::new().label(format!("{}{private}", r.full_name))
                    })
                    .collect();
                let state = cx.new(|cx| CommandState::new(window, cx));
                crate::shell::palette(
                    window,
                    cx,
                    state,
                    items,
                    "Pick a repository…",
                    move |ix, window, cx| {
                        window.close_dialog(cx);
                        if let Some(r) = repos.get(ix) {
                            let clone_url = r.clone_url.clone();
                            url.update(cx, |s, cx| s.set_value(clone_url, window, cx));
                        }
                    },
                );
            },
        );
    }

    /// Commit text with `#123` turned into Markdown links to the service's issues (M16 §2);
    /// the text as-is when the repository isn't on a known service.
    /// The service's page for commit `id`, and the service's name.
    /// The project's page on its service, and the service's name.
    pub(crate) fn project_web_url(&self) -> Option<(String, &'static str)> {
        let h = self.hosted.as_ref()?;
        Some((h.service.project_url(&h.host, &h.project), h.service.name()))
    }

    pub(crate) fn commit_web_url(&self, id: &str) -> Option<(String, &'static str)> {
        let h = self.hosted.as_ref()?;
        Some((
            h.service.commit_url(&h.host, &h.project, id),
            h.service.name(),
        ))
    }

    pub(crate) fn link_issues(&self, text: &str) -> String {
        let text = match self
            .hosted
            .as_ref()
            .and_then(|h| h.service.issues_url(&h.host, &h.project))
        {
            Some(issues) => link_issues(text, &issues),
            None => text.to_owned(),
        };
        match &self.repo {
            Some(Ok(repo)) => match &repo.tracker {
                Some(url) => link_keys(&text, url),
                None => text,
            },
            _ => text,
        }
    }

    /// Share Project on GitHub / GitLab: a new repository on the service with this one's
    /// commits. A project with remotes on the service already asks first, as the reference app's does.
    fn share_project(&mut self, service: Service, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let on_service: Vec<String> = repo
            .remotes
            .iter()
            .filter(|r| {
                remote::web_url(&r.url)
                    .and_then(|web| parse_web_url(&web))
                    .and_then(|(host, _)| service_for(&host))
                    == Some(service)
            })
            .map(|r| r.url.clone())
            .collect();
        if on_service.is_empty() {
            return self.share_dialog(service, window, cx);
        }
        let view = cx.entity();
        window.open_dialog(cx, move |modal, _, cx| {
            let name = service.name();
            let text = match on_service.len() {
                1 => format!("Remote is already on {name}:"),
                _ => format!("Following remotes are already on {name}:"),
            };
            let links = on_service.iter().enumerate().map(|(ix, url)| {
                let page = remote::web_url(url).unwrap_or_else(|| url.clone());
                spork_workbench::controls::link(("share-remote", ix), url.clone(), cx)
                    .on_click(move |_, _, cx| open_in_browser(&page, cx))
            });
            let anyway = view.clone();
            let footer = h_flex()
                .gap_2()
                .justify_end()
                .child(
                    spork_workbench::Button::new("share-existing-cancel", "Cancel")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .child(
                    spork_workbench::Button::new("share-anyway", "Share Anyway")
                        .primary(true)
                        .on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            anyway.update(cx, |this, cx| this.share_dialog(service, window, cx))
                        }),
                );
            modal
                .title(crate::git_menu::dialog_title(&format!(
                    "Project Is Already on {name}"
                )))
                .w(px(480.))
                .child(
                    v_flex()
                        .gap_4()
                        .child(
                            h_flex()
                                .gap_3()
                                .items_start()
                                .child(
                                    spork_workbench::image("general/questionDialog", cx)
                                        .size(px(32.)),
                                )
                                .child(
                                    v_flex()
                                        .gap_1()
                                        .child(
                                            div()
                                                .id("share-existing-text")
                                                .role(Role::Label)
                                                .aria_label(text.clone())
                                                .child(text),
                                        )
                                        .children(links),
                                ),
                        )
                        .child(footer),
                )
        });
    }

    /// The reference app's Share Project dialog: the new repository's name and visibility, the remote
    /// that will point at it, a description and the account (GitLab: and namespace) it goes to.
    // shortcut: an account added from the dialog shows once it opens again, as in the Clone
    // dialog; GitLab's namespace hint (ⓘ) is left out until the workbench has contextHelp
    fn share_dialog(&mut self, service: Service, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let project_name = repo
            .root
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let taken: Vec<String> = repo.remotes.iter().map(|r| r.name.clone()).collect();
        let default_remote = match taken.iter().any(|r| r == "origin") {
            true => service.id(),
            false => "origin",
        };
        let accounts = service_accounts(service);
        let form = cx.new(|cx| ShareForm {
            name: cx.new(|cx| InputState::new(window, cx).default_value(project_name)),
            remote: cx.new(|cx| InputState::new(window, cx).default_value(default_remote)),
            description: cx.new(|cx| TextareaState::new(window, cx)),
            private: false,
            account: accounts.first().cloned(),
            namespaces: None,
            namespace: 0,
            error: None,
        });
        if service == Service::GitLab {
            Self::share_load_namespaces(&form, cx);
        }
        let view = cx.entity();
        window.open_dialog(cx, move |modal, _, cx| {
            let gitlab = service == Service::GitLab;
            let state = form.read(cx);
            let label = |text: &'static str| {
                crate::git_menu::form_label(text).w(px(if gitlab { 140. } else { 120. }))
            };
            let account_name = state.account.as_ref().map(|(_, user)| user.clone());
            let account_combo = {
                let (form, accounts) = (form.clone(), accounts.clone());
                spork_workbench::controls::combo(
                    "share-account",
                    match (&account_name, gitlab) {
                        (Some(user), _) => user.clone(),
                        (None, true) => "No accounts".to_owned(),
                        (None, false) => String::new(),
                    },
                    accounts.is_empty(),
                    move |menu, _, _| {
                        accounts.iter().fold(menu, |menu, account| {
                            let (form, account) = (form.clone(), account.clone());
                            menu.item(PopupMenuItem::new(account.1.clone()).on_click(
                                move |_, _, cx| {
                                    form.update(cx, |form, cx| {
                                        form.account = Some(account.clone());
                                        form.namespaces = None;
                                        cx.notify();
                                    });
                                    if gitlab {
                                        RepoView::share_load_namespaces(&form, cx);
                                    }
                                },
                            ))
                        })
                    },
                )
            };
            let add_account = share_add_account(&view, service);
            let account_row = |name: &'static str| {
                h_flex()
                    .gap_2()
                    .child(label(name))
                    .child(div().flex_1().child(account_combo))
                    .when(gitlab || accounts.is_empty(), |row| row.child(add_account))
            };
            let namespace_row = gitlab.then(|| {
                let (shown, choices) = match &state.namespaces {
                    Some(Ok(namespaces)) if !namespaces.is_empty() => (
                        namespaces
                            .get(state.namespace)
                            .map_or_else(String::new, namespace_label),
                        namespaces.clone(),
                    ),
                    _ => ("No namespaces loaded".to_owned(), Vec::new()),
                };
                let form = form.clone();
                h_flex().gap_2().child(label("Namespace / Group:")).child(
                    div().flex_1().child(spork_workbench::controls::combo(
                        "share-namespace",
                        shown,
                        choices.is_empty(),
                        move |menu, _, _| {
                            choices.iter().enumerate().fold(menu, |menu, (ix, ns)| {
                                let form = form.clone();
                                menu.item(PopupMenuItem::new(namespace_label(ns)).on_click(
                                    move |_, _, cx| {
                                        form.update(cx, |form, cx| {
                                            form.namespace = ix;
                                            cx.notify();
                                        })
                                    },
                                ))
                            })
                        },
                    )),
                )
            });
            let private = {
                let form = form.clone();
                spork_workbench::controls::labeled_check(
                    "share-private",
                    "Private",
                    state.private,
                    cx,
                )
                .on_click(move |_, _, cx| {
                    form.update(cx, |form, cx| {
                        form.private = !form.private;
                        cx.notify();
                    })
                })
            };
            let name_row = h_flex()
                .gap_2()
                .child(label("Repository name:"))
                .child(
                    spork_workbench::TextField::new(&state.name)
                        .aria_label("Repository name:")
                        .flex_1(),
                )
                .child(private);
            let remote_row = h_flex()
                .gap_2()
                .child(label(if gitlab { "Remote name:" } else { "Remote:" }))
                .child(
                    spork_workbench::TextField::new(&state.remote)
                        .aria_label(if gitlab { "Remote name:" } else { "Remote:" })
                        .flex_1(),
                );
            let description_row = h_flex()
                .gap_2()
                .items_start()
                .child(label("Description:"))
                .child(
                    div()
                        .flex_1()
                        .child(Textarea::new(&state.description).h(px(100.))),
                );
            let error = state.error.clone().map(|text| {
                div()
                    .id("share-error")
                    .role(Role::Label)
                    .aria_label(text.clone())
                    .text_color(cx.theme().danger)
                    .child(text)
            });
            let fields = match gitlab {
                true => v_flex()
                    .gap_3()
                    .child(account_row("Account:"))
                    .children(namespace_row)
                    .child(name_row)
                    .child(remote_row)
                    .child(description_row),
                false => v_flex()
                    .gap_3()
                    .child(name_row)
                    .child(remote_row)
                    .child(description_row)
                    .child(account_row("Share by:")),
            };
            let (share_view, share_form, share_taken) = (view.clone(), form.clone(), taken.clone());
            let footer = h_flex()
                .gap_2()
                .when(!gitlab, |footer| {
                    footer.child(
                        spork_workbench::ActionButton::new("share-help", "general/questionMark")
                            .tooltip("Show help contents")
                            .on_click(|_, _, cx| {
                                cx.open_url(
                                    "https://docs.github.com/repositories/creating-and-managing-repositories/creating-a-new-repository",
                                )
                            }),
                    )
                })
                .child(div().flex_1())
                .child(
                    spork_workbench::Button::new("share-cancel", "Cancel")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .child(
                    spork_workbench::Button::new("share-ok", "Share")
                        .primary(true)
                        .on_click(move |_, window, cx| {
                            let checked = share_form.read(cx).checked(service, &share_taken, cx);
                            match checked {
                                Err(error) => share_form.update(cx, |form, cx| {
                                    form.error = Some(error);
                                    cx.notify();
                                }),
                                Ok(share) => {
                                    window.close_dialog(cx);
                                    share_view.update(cx, |this, cx| {
                                        this.share_run(service, share, window, cx)
                                    })
                                }
                            }
                        }),
                );
            modal
                .title(crate::git_menu::dialog_title(match gitlab {
                    true => "Share Project on GitLab",
                    false => "Share Project On GitHub",
                }))
                .w(px(560.))
                .child(
                    v_flex()
                        .gap_4()
                        .child(fields)
                        .children(error)
                        .child(footer),
                )
        });
    }

    /// The chosen GitLab account's namespaces, for the Share dialog.
    fn share_load_namespaces(form: &Entity<ShareForm>, cx: &mut App) {
        let Some((host, user)) = form.read(cx).account.clone() else {
            return;
        };
        let task = cx.spawn({
            let form = form.downgrade();
            async move |cx| {
                let namespaces = cx
                    .background_spawn(async move {
                        let (_, token) = credential::get_as(&host, Some(&user))
                            .ok_or_else(|| format!("Token for account {user} is invalid"))?;
                        Client::new(Service::GitLab, &host, &token)
                            .namespaces()
                            .map_err(|_| format!("Cannot load namespaces for account {user}"))
                    })
                    .await;
                form.update(cx, |form, cx| {
                    form.namespaces = Some(namespaces);
                    form.namespace = 0;
                    cx.notify();
                })
                .ok();
            }
        });
        task.detach();
    }

    /// Creates the repository, adds the remote and pushes the current branch to it.
    fn share_run(
        &mut self,
        service: Service,
        share: ShareRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(repo)) = &self.repo else { return };
        let branch = repo.head.clone();
        let ssh = crate::settings_window::Store::load().get("github.cloneSsh") == Some("true");
        let name = service.name();
        self.run_outcome(
            window,
            cx,
            format!("Successfully shared project on {name}"),
            move |root| {
                let (_, token) = credential::get_as(&share.host, Some(&share.user))
                    .ok_or_else(|| other(format!("Token for account {} is invalid", share.user)))?;
                let created = Client::new(service, &share.host, &token)
                    .create_repository(&share.name, &share.description, share.private, share.namespace)
                    .map_err(|e| other(format!("Failed to create repository: {e}")))?;
                let url = match ssh && service == Service::GitHub {
                    true => created.ssh_url,
                    false => created.clone_url,
                };
                remote::add(root, &share.remote, &url)?;
                let Some(branch) = branch else {
                    return Err(other(format!(
                        "Successfully created project '{}' on {name}, but initial push failed: no current branch",
                        share.name
                    )));
                };
                remote::push_branch(root, &share.remote, &branch).map_err(|e| {
                    other(format!(
                        "Successfully created project '{}' on {name}, but initial push failed: {e}",
                        share.name
                    ))
                })?;
                Ok(spork_git::Outcome::Done)
            },
        );
    }

    /// The end-to-end tests' requests, as the service's API answers them, listed in place of
    /// the service's from now on (e2e.rs); `listed`: shown at once, else on the next load.
    pub(crate) fn e2e_pulls(
        &mut self,
        list: &serde_json::Value,
        listed: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let service = self
            .hosted
            .as_ref()
            .map(|h| h.service)
            .ok_or("no GitHub or GitLab remote")?;
        let pulls = list
            .as_array()
            .ok_or("pulls: a list of the service's requests")?
            .iter()
            .map(|v| {
                spork_hosting::parse_pull_request(service, v)
                    .ok_or_else(|| format!("not a {}: {v}", service.noun()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        TEST_PULLS.set(Some(pulls));
        if listed {
            self.load_pulls(cx);
        }
        Ok(())
    }
}

/// The Share dialog's fields between its redraws.
pub(crate) struct ShareForm {
    name: Entity<InputState>,
    remote: Entity<InputState>,
    description: Entity<TextareaState>,
    private: bool,
    /// The account it goes to, as `(host, user)`.
    account: Option<(String, String)>,
    /// GitLab: the account's namespaces, `None` while they load; the chosen one's index.
    namespaces: Option<Result<Vec<Namespace>, String>>,
    namespace: usize,
    /// Why Share didn't go: the reference app's validation message.
    error: Option<String>,
}

/// What Share creates, checked.
pub(crate) struct ShareRequest {
    host: String,
    user: String,
    name: String,
    description: String,
    private: bool,
    remote: String,
    namespace: Option<u64>,
}

impl ShareForm {
    /// The form as a request, or the first of the reference app's validation messages that applies.
    fn checked(
        &self,
        service: Service,
        taken: &[String],
        cx: &App,
    ) -> Result<ShareRequest, String> {
        let gitlab = service == Service::GitLab;
        let name = self.name.read(cx).value().trim().to_owned();
        let remote = self.remote.read(cx).value().trim().to_owned();
        let account = self.account.clone();
        let namespace = match &self.namespaces {
            Some(Ok(namespaces)) => namespaces.get(self.namespace).map(|n| n.id),
            _ => None,
        };
        let valid_name = name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
        let error = match () {
            _ if gitlab && account.is_none() => "No account selected",
            _ if gitlab && namespace.is_none() => "No namespace selected",
            _ if name.is_empty() && gitlab => "No repository name entered",
            _ if name.is_empty() => "No repository name selected",
            _ if !valid_name => {
                "Invalid repository name. Name should consist of letters, numbers, dashes, dots and underscores"
            }
            _ if remote.is_empty() && gitlab => "No remote name entered",
            _ if remote.is_empty() => "No remote name selected",
            _ if taken.contains(&remote) => "Remote with selected name already exists",
            _ if account.is_none() => "Account cannot be empty",
            _ => "",
        };
        let Some((host, user)) = account.filter(|_| error.is_empty()) else {
            return Err(error.to_owned());
        };
        Ok(ShareRequest {
            host,
            user,
            name,
            description: self.description.read(cx).value().trim().to_owned(),
            private: self.private,
            remote,
            namespace,
        })
    }
}

/// A GitLab namespace as the Share dialog lists it: "Personal: ann", or the group's path.
fn namespace_label(namespace: &Namespace) -> String {
    match namespace.personal {
        true => format!("Personal: {}", namespace.path),
        false => namespace.path.clone(),
    }
}

/// The Share dialog's way to add an account: GitHub's "Add account" link, GitLab's button.
fn share_add_account(view: &Entity<RepoView>, service: Service) -> AnyElement {
    let view = view.clone();
    let items = move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
        let log_in =
            |label: &'static str, how: fn(&mut RepoView, &mut Window, &mut Context<RepoView>)| {
                let view = view.clone();
                PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    view.update(cx, |this, cx| how(this, window, cx))
                })
            };
        match service {
            Service::GitLab => menu
                .item(log_in("Log In to gitlab.com…", |this, window, cx| {
                    this.connect_service(Service::GitLab, "gitlab.com".into(), window, cx)
                }))
                .item(log_in("Log In to Custom Server…", |this, window, cx| {
                    this.connect_service(Service::GitLab, String::new(), window, cx)
                })),
            _ => menu
                .item(log_in("Log In via GitHub…", RepoView::log_in_via_github))
                .item(log_in("Log in with Token…", |this, window, cx| {
                    this.connect_service(Service::GitHub, "github.com".into(), window, cx)
                }))
                .item(log_in(
                    "Log In to GitHub Enterprise…",
                    |this, window, cx| {
                        this.connect_service(Service::GitHub, String::new(), window, cx)
                    },
                )),
        }
    };
    match service {
        Service::GitLab => spork_workbench::Button::new("share-add-account", "Add account")
            .dropdown_menu(items)
            .into_any_element(),
        _ => spork_workbench::controls::DropDownLink::new("share-add-account", "Add account")
            .dropdown_menu(items)
            .into_any_element(),
    }
}

/// The request View Pull Request opened in the tool window, and its changes.
pub(crate) struct PullDetails {
    number: u64,
    /// `None` while they are fetched; the changes, or why they couldn't be.
    changes: Option<Result<PullChanges, String>>,
    /// The changed file selected in the tree.
    file: Option<String>,
    /// The tree's folded folders.
    collapsed: HashSet<String>,
    /// View Timeline opened its tab.
    timeline: bool,
    _load: Option<Task<()>>,
}

/// What the Pull Requests tool window shows: the list, the open request or its timeline.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PullsTab {
    List,
    Details,
    Timeline,
}

/// What a request changes: from where its branch left the base to its head.
struct PullChanges {
    base: String,
    head: String,
    /// Its commits, newest first: short hash and subject.
    log: Vec<(String, String)>,
    files: Vec<(char, String)>,
}

/// The request's changes: its head fetched from the service's ref, its base branch from the
/// remote, and the files changed since the two parted.
fn fetch_request_changes(
    root: &std::path::Path,
    remote: &str,
    service: Service,
    pr: &PullRequest,
) -> Result<PullChanges, GitError> {
    let text = |out: Vec<u8>| String::from_utf8_lossy(&out).trim().to_owned();
    let (source, _) = service.head_ref(pr.number, &pr.head);
    spork_git::run(root, &["fetch", remote, &source])?;
    let head = text(spork_git::run(root, &["rev-parse", "FETCH_HEAD"])?);
    fetch_request_branch(root, remote, &pr.base)?;
    let base_ref = format!("{remote}/{}", pr.base);
    let base = text(spork_git::run(root, &["merge-base", &base_ref, &head])?);
    let range = format!("{base}..{head}");
    let log = text(spork_git::run(root, &["log", "--format=%h%x1f%s", &range])?)
        .lines()
        .filter_map(|line| line.split_once('\x1f'))
        .map(|(hash, subject)| (hash.to_owned(), subject.to_owned()))
        .collect();
    let files = spork_git::files_between(root, &base, &head)?;
    Ok(PullChanges {
        base,
        head,
        log,
        files,
    })
}

/// A line in place of a list: loading, or why there is nothing.
fn pulls_note(text: &str, tokens: &spork_workbench::Tokens) -> AnyElement {
    div()
        .id(SharedString::from(format!("pulls-note-{text}")))
        .role(Role::Label)
        .aria_label(text.to_owned())
        .px_3()
        .py_1()
        .text_color(tokens.text_secondary)
        .child(text.to_owned())
        .into_any_element()
}

/// View Pull Request in Browser and Copy Pull Request URL, which every request menu ends with.
fn pulls_link_items(menu: PopupMenu, service: Service, url: &str) -> PopupMenu {
    let name = request_name(service);
    let (open, copy) = (url.to_owned(), url.to_owned());
    menu.item(
        PopupMenuItem::new(format!("View {name} in Browser"))
            .on_click(move |_, _, cx| open_in_browser(&open, cx)),
    )
    .item(
        PopupMenuItem::new(format!("Copy {name} URL")).on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
        }),
    )
}

/// Refresh Pull Request (GitLab: Refresh Merge Request), ⌘R as the reference app's Refresh.
fn pulls_refresh_item(view: &Entity<RepoView>, service: Service) -> PopupMenuItem {
    let view = view.clone();
    PopupMenuItem::new(format!("Refresh {}", request_name(service)))
        .action(Box::new(crate::shell::Refresh))
        .on_click(move |_, _, cx| view.update(cx, |this, cx| this.pulls_load_details(cx)))
}

/// The open request's menu (Github.PullRequest.Details.Popup, GitLab's
/// GitLab.Merge.Request.Details.Popup).
fn pulls_details_menu(
    view: Entity<RepoView>,
    service: Service,
    pr: PullRequest,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, _, _| {
        pulls_link_items(
            menu.item(pulls_refresh_item(&view, service)),
            service,
            &pr.url,
        )
    }
}

/// The timeline's menu (Github.PullRequest.Timeline.Popup, GitLab's
/// GitLab.Merge.Request.Timeline.Popup), or GitLab's on a timeline that failed to load
/// (GitLab.Merge.Request.Timeline.Error.Popup).
fn pulls_timeline_menu(
    view: Entity<RepoView>,
    service: Service,
    pr: PullRequest,
    failed: bool,
    events: bool,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, _, _| {
        let menu = match (service, failed) {
            (Service::GitLab, true) => menu,
            (Service::GitLab, false) => {
                let toggle = view.clone();
                menu.item(
                    PopupMenuItem::new("Show Merge Request Events")
                        .checked(events)
                        .on_click(move |_, _, cx| {
                            toggle.update(cx, |this, cx| {
                                if let Some(h) = &mut this.hosted {
                                    h.show_events = !h.show_events;
                                }
                                cx.notify();
                            })
                        }),
                )
                .separator()
                .item(pulls_refresh_item(&view, service))
            }
            _ => {
                let view = view.clone();
                menu.item(
                    PopupMenuItem::new("Refresh Timeline")
                        .action(Box::new(crate::shell::Refresh))
                        .on_click(move |_, _, cx| {
                            view.update(cx, |this, cx| this.pulls_load_details(cx))
                        }),
                )
            }
        };
        pulls_link_items(menu, service, &pr.url)
    }
}

/// Group By's choices for the request's changes (ChangesView.GroupBy).
fn pulls_group_by(menu: PopupMenu, view: &Entity<RepoView>, flat: bool) -> PopupMenu {
    let choice = |label: &'static str, checked: bool, directory: Option<bool>| {
        let view = view.clone();
        PopupMenuItem::new(label)
            .checked(checked)
            .on_click(move |_, _, cx| {
                view.update(cx, |this, cx| {
                    if let (Some(h), Some(by_directory)) = (&mut this.hosted, directory) {
                        h.flat = !by_directory;
                    }
                    cx.notify();
                })
            })
    };
    // one repository: grouping by it changes nothing, as in the reference app's
    menu.item(choice("Directory", !flat, Some(flat)))
        .item(choice("Repository", false, None))
}

/// The menu on a request's changed file (Github.PullRequest.Changes.Popup, GitLab's
/// GitLab.Merge.Request.Changes.Popup), in the reference app's order.
// shortcut: the viewed state is the service's (GitHub's GraphQL, GitLab's discussions API),
// which Spork's client doesn't reach yet: those items show greyed
fn pulls_changes_menu(
    view: Entity<RepoView>,
    service: Service,
    pr: PullRequest,
    flat: bool,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, window, cx| {
        let run = |label: &'static str,
                   f: fn(&mut RepoView, &mut Window, &mut Context<RepoView>)| {
            let view = view.clone();
            PopupMenuItem::new(label)
                .on_click(move |_, window, cx| view.update(cx, |this, cx| f(this, window, cx)))
        };
        let fold = |label: &'static str, collapse: bool| {
            let view = view.clone();
            PopupMenuItem::new(label).on_click(move |_, _, cx| {
                view.update(cx, |this, cx| this.pulls_fold_all(collapse, cx))
            })
        };
        let options = {
            let view = view.clone();
            move |menu: PopupMenu, window: &mut Window, cx: &mut Context<PopupMenu>| {
                let view = view.clone();
                menu.submenu("Group By", window, cx, move |menu, _, _| {
                    pulls_group_by(menu, &view, flat)
                })
            }
        };
        let menu = menu
            .item(
                run("Show Diff", RepoView::pulls_show_diff)
                    .action(Box::new(crate::changes::ShowDiff)),
            )
            .item(run(
                "Show Diff in a New Tab or Window",
                RepoView::pulls_show_diff,
            ))
            .item(
                run("Jump to Source", RepoView::pulls_jump_to_source)
                    .action(Box::new(crate::changes::JumpToSource)),
            );
        let menu = match service {
            Service::GitLab => menu
                .item(PopupMenuItem::new("Mark as Viewed").disabled(true))
                .item(PopupMenuItem::new("Mark as Not Viewed").disabled(true))
                .submenu("View Options", window, cx, options)
                .separator(),
            _ => menu
                .item(PopupMenuItem::new("Toggle viewed/not viewed state").disabled(true))
                .separator()
                .submenu("View Options", window, cx, options),
        };
        let menu = menu
            .item(fold("Expand All", false).action(Box::new(crate::changes::ExpandAll)))
            .item(fold("Collapse All", true).action(Box::new(crate::changes::CollapseAll)))
            .separator()
            .item(pulls_refresh_item(&view, service));
        pulls_link_items(menu, service, &pr.url)
    }
}

/// Fetches `branch` of `remote` into its remote-tracking ref.
fn fetch_request_branch(
    root: &std::path::Path,
    remote: &str,
    branch: &str,
) -> Result<(), GitError> {
    let refspec = format!("+refs/heads/{branch}:refs/remotes/{remote}/{branch}");
    spork_git::run(root, &["fetch", remote, &refspec]).map(drop)
}

/// The list's menu on a request (Github.PullRequest.ToolWindow.List.Popup, GitLab's
/// GitLab.Merge.Request.List.Actions), in the reference app's order.
fn pulls_list_menu(
    view: Entity<RepoView>,
    service: Service,
    pr: PullRequest,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, _, _| {
        let run =
            |label: &str,
             f: fn(&mut RepoView, PullRequest, &mut Window, &mut Context<RepoView>)| {
                let (view, pr) = (view.clone(), pr.clone());
                PopupMenuItem::new(label.to_owned()).on_click(move |_, window, cx| {
                    let pr = pr.clone();
                    view.update(cx, |this, cx| f(this, pr, window, cx))
                })
            };
        let show = match service {
            Service::GitLab => "Show Merge Request",
            _ => "View Pull Request",
        };
        let reload = view.clone();
        let menu = menu
            .item(run(show, |this, pr, _, cx| this.pulls_show(pr.number, cx)))
            .item(run("Checkout Branch", RepoView::pulls_checkout))
            .item(run("Open in Worktree", RepoView::pulls_open_in_worktree));
        pulls_link_items(menu, service, &pr.url).separator().item(
            PopupMenuItem::new("Refresh List")
                .action(Box::new(crate::shell::Refresh))
                .on_click(move |_, _, cx| reload.update(cx, |this, cx| this.pulls_reload(cx))),
        )
    }
}

/// Replaces `#123` that starts a word with `[#123](<issues>/123)`. GitHub and Gitea send an
/// issue URL that is really a pull request on to it.
fn link_issues(text: &str, issues: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('#') {
        let (before, after) = rest.split_at(i);
        out.push_str(before);
        let digits = after[1..].chars().take_while(char::is_ascii_digit).count();
        let starts_word = out
            .chars()
            .last()
            .is_none_or(|c| c.is_whitespace() || "([{,".contains(c));
        if digits > 0 && starts_word {
            let n = &after[1..=digits];
            out.push_str(&format!("[#{n}]({issues}/{n})"));
            rest = &after[1 + digits..];
        } else {
            out.push('#');
            rest = &after[1..];
        }
    }
    out.push_str(rest);
    out
}

/// Links Jira-style keys (`PROJ-123`: capitals, a dash, digits, as a whole word) to `tracker`,
/// e.g. `https://jira.example.com/browse/` (M16 §2).
fn link_keys(text: &str, tracker: &str) -> String {
    let base = tracker.trim_end_matches('/');
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let starts_word = i == 0 || !chars[i - 1].is_alphanumeric();
        let letters = chars[i..]
            .iter()
            .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            .count();
        let key_end = i + letters;
        let digits = chars.get(key_end).filter(|c| **c == '-').map(|_| {
            chars[key_end + 1..]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .count()
        });
        match digits {
            Some(n)
                if starts_word
                    && letters >= 2
                    && chars[i].is_ascii_uppercase()
                    && n > 0
                    && chars
                        .get(key_end + 1 + n)
                        .is_none_or(|c| !c.is_alphanumeric()) =>
            {
                let key: String = chars[i..key_end + 1 + n].iter().collect();
                out.push_str(&format!("[{key}]({base}/{key})"));
                i = key_end + 1 + n;
            }
            _ => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    // explicit: `super::*` would pull in gpui's `test` attribute, which shadows #[test]
    use super::{link_issues, link_keys};
    use spork_hosting::Service;

    #[test]
    fn finds_pull_request_templates() {
        let dir = std::env::temp_dir().join(format!("spork-pr-template-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".github")).unwrap();
        assert_eq!(super::pr_template(&dir), None);
        std::fs::write(dir.join(".github/pull_request_template.md"), "## What\n").unwrap();
        assert_eq!(super::pr_template(&dir).as_deref(), Some("## What\n"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn links_issue_numbers() {
        let base = Service::GitHub.issues_url("github.com", "o/r").unwrap();
        assert_eq!(
            link_issues("Fix crash (#12), see #7 and #x or a#3", &base),
            "Fix crash ([#12](https://github.com/o/r/issues/12)), see [#7](https://github.com/o/r/issues/7) and #x or a#3"
        );
        assert_eq!(
            Service::GitLab.issues_url("gitlab.com", "g/r").as_deref(),
            Some("https://gitlab.com/g/r/-/issues")
        );
        assert_eq!(link_issues("no refs", &base), "no refs");
        assert_eq!(
            link_issues("#1#2", &base),
            "[#1](https://github.com/o/r/issues/1)#2"
        );
    }

    #[test]
    fn links_tracker_keys() {
        let base = "https://jira.example.com/browse/";
        assert_eq!(
            link_keys("PROJ-12: fix, see AB2-7 and x-1, ABC-, MY-PROJ-3", base),
            "[PROJ-12](https://jira.example.com/browse/PROJ-12): fix, see [AB2-7](https://jira.example.com/browse/AB2-7) and x-1, ABC-, MY-[PROJ-3](https://jira.example.com/browse/PROJ-3)"
        );
        assert_eq!(link_keys("élan", base), "élan");
    }
}
