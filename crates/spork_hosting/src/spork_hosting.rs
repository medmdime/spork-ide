//! GitHub, GitLab, Gitea / Forgejo, Bitbucket Cloud, Bitbucket Data Center and Azure DevOps over
//! their REST APIs: the signed-in user, pull / merge requests, repositories (docs/specs/M15).
//!
//! Blocking calls, meant for background threads like the git runner. The token comes from the
//! caller (Spork keeps it in the user's git credential helper, see `spork_git::credential`).

use std::fmt;

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    GitHub,
    GitLab,
    /// Gitea and its fork Forgejo (Codeberg runs Forgejo).
    Gitea,
    /// Bitbucket Cloud, with an Atlassian API token (sent as a bearer token, so no email needed;
    /// git over HTTPS takes it with the Bitbucket username `/user` returns).
    Bitbucket,
    /// Bitbucket Server / Data Center, with an HTTP access token.
    BitbucketServer,
    /// Azure DevOps (dev.azure.com, *.visualstudio.com), with a personal access token.
    AzureDevOps,
}

impl Service {
    /// Recognises a host by name: `github.com` / `github.*`, `gitlab.com` / `gitlab.*`.
    pub fn detect(host: &str) -> Option<Self> {
        let is =
            |name: &str| host == format!("{name}.com") || host.starts_with(&format!("{name}."));
        if is("github") {
            Some(Service::GitHub)
        } else if is("gitlab") {
            Some(Service::GitLab)
        } else if host == "codeberg.org" || is("gitea") || is("forgejo") {
            Some(Service::Gitea)
        } else if host == "bitbucket.org" {
            Some(Service::Bitbucket)
        } else if is("bitbucket") {
            Some(Service::BitbucketServer)
        } else if host.ends_with("dev.azure.com") || host.ends_with(".visualstudio.com") {
            Some(Service::AzureDevOps)
        } else {
            None
        }
    }

    /// Every service, in the order a picker shows them.
    pub const ALL: [Service; 6] = [
        Service::GitHub,
        Service::GitLab,
        Service::Gitea,
        Service::Bitbucket,
        Service::BitbucketServer,
        Service::AzureDevOps,
    ];

    /// A stable name for settings (`spork.<host>.service`).
    pub fn id(self) -> &'static str {
        match self {
            Service::GitHub => "github",
            Service::GitLab => "gitlab",
            Service::Gitea => "gitea",
            Service::Bitbucket => "bitbucket",
            Service::BitbucketServer => "bitbucket-server",
            Service::AzureDevOps => "azure-devops",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    /// The public host users of the service connect to first.
    pub fn default_host(self) -> &'static str {
        match self {
            Service::GitHub => "github.com",
            Service::GitLab => "gitlab.com",
            Service::Gitea => "codeberg.org",
            Service::Bitbucket => "bitbucket.org",
            // always the company's own server
            Service::BitbucketServer => "",
            Service::AzureDevOps => "dev.azure.com",
        }
    }

    /// The host Spork talks to and keeps the token under: Azure DevOps' SSH host is its web host.
    pub fn canonical_host(self, host: &str) -> String {
        match (self, host) {
            (Service::AzureDevOps, "ssh.dev.azure.com") => "dev.azure.com".to_owned(),
            _ => host.to_owned(),
        }
    }

    /// The project's web page.
    pub fn project_url(self, host: &str, project: &str) -> String {
        let host = self.canonical_host(host);
        match self {
            Service::BitbucketServer => {
                let (key, repo) = bbs_parts(project);
                format!("https://{host}/projects/{key}/repos/{repo}")
            }
            Service::AzureDevOps => {
                let (base, repo) = azure_parts(project);
                format!("https://{host}/{base}/_git/{repo}")
            }
            _ => format!("https://{host}/{project}"),
        }
    }

    /// The web page of commit `id`.
    pub fn commit_url(self, host: &str, project: &str, id: &str) -> String {
        let base = self.project_url(host, project);
        match self {
            Service::GitLab => format!("{base}/-/commit/{id}"),
            Service::Bitbucket | Service::BitbucketServer => format!("{base}/commits/{id}"),
            Service::GitHub | Service::Gitea | Service::AzureDevOps => {
                format!("{base}/commit/{id}")
            }
        }
    }

    /// Where `#123` in messages points; `None` where issues live elsewhere (Jira, work items).
    pub fn issues_url(self, host: &str, project: &str) -> Option<String> {
        let base = self.project_url(host, project);
        match self {
            Service::GitLab => Some(format!("{base}/-/issues")),
            Service::GitHub | Service::Gitea | Service::Bitbucket => Some(format!("{base}/issues")),
            Service::BitbucketServer | Service::AzureDevOps => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Service::GitHub => "GitHub",
            Service::GitLab => "GitLab",
            Service::Gitea => "Gitea",
            Service::Bitbucket => "Bitbucket",
            Service::BitbucketServer => "Bitbucket Server",
            Service::AzureDevOps => "Azure DevOps",
        }
    }

    /// What the service calls a pull request.
    pub fn noun(self) -> &'static str {
        match self {
            Service::GitLab => "merge request",
            _ => "pull request",
        }
    }

    /// Where users create a token, and the scope to tick.
    pub fn token_page(self, host: &str) -> (String, &'static str) {
        match self {
            Service::GitHub => (format!("https://{host}/settings/tokens"), "repo"),
            Service::GitLab => (
                format!("https://{host}/-/user_settings/personal_access_tokens"),
                "api",
            ),
            Service::Gitea => (
                format!("https://{host}/user/settings/applications"),
                "repository (read and write)",
            ),
            Service::Bitbucket => (
                "https://id.atlassian.com/manage-profile/security/api-tokens".to_owned(),
                "read:user, read:repository, write:repository, read:pullrequest and write:pullrequest (Bitbucket)",
            ),
            Service::BitbucketServer => (
                format!("https://{host}/plugins/servlet/access-tokens/manage"),
                "Project read and Repository write",
            ),
            Service::AzureDevOps => (
                format!("https://{host} ▸ User settings ▸ Personal access tokens"),
                "Code (Read & write)",
            ),
        }
    }

    /// The ref a pull / merge request's commits are published under, and the local branch for it.
    /// `head` is its source branch, which Bitbucket fetches directly (it publishes no PR refs).
    // shortcut: Bitbucket pull requests from forks aren't fetched (their branch is in the fork).
    pub fn head_ref(self, number: u64, head: &str) -> (String, String) {
        match self {
            Service::Bitbucket | Service::BitbucketServer | Service::AzureDevOps => {
                (format!("refs/heads/{head}"), format!("pr/{number}"))
            }
            Service::GitHub | Service::Gitea => {
                (format!("refs/pull/{number}/head"), format!("pr/{number}"))
            }
            Service::GitLab => (
                format!("refs/merge-requests/{number}/head"),
                format!("mr/{number}"),
            ),
        }
    }

    /// The web page that creates a pull / merge request from `branch` into `base`.
    pub fn compare_url(self, host: &str, project: &str, base: &str, branch: &str) -> String {
        match self {
            Service::GitHub => {
                format!("https://{host}/{project}/compare/{base}...{branch}?expand=1")
            }
            Service::Gitea => format!("https://{host}/{project}/compare/{base}...{branch}"),
            Service::Bitbucket => {
                format!("https://{host}/{project}/pull-requests/new?source={branch}&dest={base}")
            }
            Service::GitLab => format!(
                "https://{host}/{project}/-/merge_requests/new?merge_request[source_branch]={branch}&merge_request[target_branch]={base}"
            ),
            Service::BitbucketServer => format!(
                "{}/pull-requests?create&sourceBranch=refs/heads/{branch}&targetBranch=refs/heads/{base}",
                self.project_url(host, project)
            ),
            Service::AzureDevOps => format!(
                "{}/pullrequestcreate?sourceRef={branch}&targetRef={base}",
                self.project_url(host, project)
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The server answered with an error status, with its message.
    Http(u16, String),
    /// No answer: offline, DNS, TLS, timeout.
    Network(String),
    /// An answer we couldn't read.
    Unexpected,
    /// The service doesn't offer this, with a sentence saying so.
    Unsupported(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Http(401, _) => {
                write!(
                    f,
                    "The token was rejected: create a new one and connect again."
                )
            }
            Error::Http(status, msg) => write!(f, "The server answered {status}: {msg}"),
            Error::Network(e) => write!(f, "Couldn't reach the server: {e}"),
            Error::Unexpected => write!(f, "The server sent an answer Spork can't read."),
            Error::Unsupported(why) => f.write_str(why),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    /// The number shown to users (GitHub `number`, GitLab `iid`).
    pub number: u64,
    pub title: String,
    pub author: String,
    /// Branch the changes come from.
    pub head: String,
    /// Branch they go into.
    pub base: String,
    /// Web page of the pull request.
    pub url: String,
    pub draft: bool,
    /// What its author wrote about it (Markdown); empty when they wrote nothing.
    pub description: String,
    /// The head branch's latest commit (Bitbucket: abbreviated); empty when not given.
    pub sha: String,
    /// The head branch is in another repository (a fork), not on this one's remote.
    pub fork: bool,
}

/// A pull / merge request as `service`'s API answers it.
pub fn parse_pull_request(service: Service, v: &Value) -> Option<PullRequest> {
    Client::new(service, "", "").parse_pull(v)
}

/// Where reviewers stand on a pull request: anyone asking for changes outweighs approvals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Review {
    Approved,
    ChangesRequested,
}

/// What CI says about a pull request's latest commit, all its checks taken together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checks {
    Passed,
    Running,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    /// `owner/name` (GitLab: the full namespace path).
    pub full_name: String,
    pub clone_url: String,
    pub private: bool,
}

/// A repository Share Project created: where to push to (HTTPS, SSH) and its page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRepository {
    pub clone_url: String,
    pub ssh_url: String,
    pub web_url: String,
}

/// Where GitLab can create a project: the user's own namespace or a group's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    pub id: u64,
    pub path: String,
    /// The user's own (GitLab's `kind` "user"), not a group's.
    pub personal: bool,
}

/// An unread notification (GitHub, Gitea) or pending to-do (GitLab) of the signed-in user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    /// `owner/name` of the repository it's about.
    pub repository: String,
    /// What it is about: "PullRequest", "Issue", "merge_request", …
    pub kind: String,
    /// Its web page.
    pub url: String,
}

/// `(host, project)` of a web URL such as `https://github.com/o/r` or
/// `https://gitlab.com/group/sub/r` (see `spork_git::remote::web_url`).
pub fn parse_web_url(url: &str) -> Option<(String, String)> {
    let (host, project) = url.strip_prefix("https://")?.split_once('/')?;
    let parts: Vec<&str> = project.split('/').collect();
    (!host.is_empty() && parts.len() >= 2 && parts.iter().all(|p| !p.is_empty()))
        .then(|| (host.to_owned(), project.to_owned()))
}

pub struct Client {
    service: Service,
    host: String,
    api: String,
    token: String,
}

impl Client {
    /// `github.com` uses `api.github.com`, other GitHub hosts are Enterprise (`/api/v3`);
    /// GitLab is `/api/v4` and Gitea `/api/v1` on their host.
    pub fn new(service: Service, host: &str, token: &str) -> Self {
        let api = match service {
            Service::GitHub if host == "github.com" => "https://api.github.com".to_owned(),
            Service::GitHub => format!("https://{host}/api/v3"),
            Service::GitLab => format!("https://{host}/api/v4"),
            Service::Gitea => format!("https://{host}/api/v1"),
            Service::Bitbucket => "https://api.bitbucket.org/2.0".to_owned(),
            Service::BitbucketServer => format!("https://{host}/rest/api/1.0"),
            Service::AzureDevOps => format!("https://{}", service.canonical_host(host)),
        };
        Self {
            service,
            host: service.canonical_host(host),
            api,
            token: token.trim().to_owned(),
        }
    }

    fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, Error> {
        // a full URL is a service endpoint off the API's base (logins, build statuses)
        let url = if path.starts_with("https://") {
            path.to_owned()
        } else {
            format!("{}{path}", self.api)
        };
        let auth = match self.service {
            // Azure DevOps takes a personal access token as the password of basic auth
            Service::AzureDevOps => {
                format!("Basic {}", base64(format!(":{}", self.token).as_bytes()))
            }
            _ => format!("Bearer {}", self.token),
        };
        let accept = match self.service {
            Service::GitHub => "application/vnd.github+json",
            _ => "application/json",
        };
        let response = match (method, body) {
            ("POST", Some(body)) => ureq::post(&url)
                .header("Authorization", &auth)
                .header("Content-Type", "application/json")
                .header("Accept", accept)
                .header("User-Agent", "Spork")
                .config()
                .http_status_as_error(false)
                .build()
                .send(body.to_string()),
            _ => ureq::get(&url)
                .header("Authorization", &auth)
                .header("Accept", accept)
                .header("User-Agent", "Spork")
                .config()
                .http_status_as_error(false)
                .build()
                .call(),
        };
        let mut response = response.map_err(|e| Error::Network(e.to_string()))?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| Error::Network(e.to_string()))?;
        // a plain-text answer (Bitbucket Server's whoami) comes back as a string
        let value: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) if status < 400 => Value::String(text.trim().to_owned()),
            Err(_) => Value::String(text.chars().take(200).collect()),
        };
        if status >= 400 {
            // GitHub: {"message": "…"}; GitLab: {"message": …} or {"error": "…"};
            // Bitbucket: {"error": {"message": "…"}}
            let msg = match (&value["message"], &value["error"]) {
                (Value::String(m), _) | (_, Value::String(m)) => m.clone(),
                (_, e) if e["message"].is_string() => {
                    e["message"].as_str().unwrap_or("").to_owned()
                }
                (m, _) => m.to_string(),
            };
            return Err(Error::Http(status, msg));
        }
        Ok(value)
    }

    /// GitLab addresses projects by their URL-encoded path.
    fn project(&self, project: &str) -> String {
        match self.service {
            Service::GitHub | Service::Gitea => format!("/repos/{project}"),
            Service::GitLab => format!("/projects/{}", project.replace('/', "%2F")),
            Service::Bitbucket => format!("/repositories/{project}"),
            Service::BitbucketServer => {
                let (key, repo) = bbs_parts(project);
                format!("/projects/{key}/repos/{repo}")
            }
            Service::AzureDevOps => {
                let (base, repo) = azure_parts(project);
                format!("/{base}/_apis/git/repositories/{repo}")
            }
        }
    }

    /// The token's user name; also how a token is checked before it is saved.
    pub fn login(&self) -> Result<String, Error> {
        let (path, key) = match self.service {
            Service::GitHub | Service::Gitea => ("/user".to_owned(), "login"),
            Service::GitLab | Service::Bitbucket => ("/user".to_owned(), "username"),
            // answers the user name as plain text
            Service::BitbucketServer => (
                format!("https://{}/plugins/servlet/applinks/whoami", self.host),
                "",
            ),
            Service::AzureDevOps => (
                "https://app.vssps.visualstudio.com/_apis/profile/profiles/me?api-version=7.1"
                    .to_owned(),
                "emailAddress",
            ),
        };
        let user = self.call("GET", &path, None)?;
        let name = if key.is_empty() { &user } else { &user[key] };
        name.as_str()
            .filter(|n| !n.is_empty() && !n.contains('<'))
            .map(str::to_owned)
            .ok_or(Error::Unexpected)
    }

    // shortcut: first 100 open ones; page through the APIs' paging headers when repositories have more.
    pub fn pull_requests(&self, project: &str) -> Result<Vec<PullRequest>, Error> {
        let path = match self.service {
            Service::GitHub => "/pulls?state=open&per_page=100",
            Service::Gitea => "/pulls?state=open&limit=50",
            Service::GitLab => "/merge_requests?state=opened&per_page=100",
            Service::Bitbucket => "/pullrequests?state=OPEN&pagelen=50",
            Service::BitbucketServer => "/pull-requests?state=OPEN&limit=50",
            Service::AzureDevOps => "/pullrequests?searchCriteria.status=active&api-version=7.1",
        };
        let list = self.call("GET", &format!("{}{path}", self.project(project)), None)?;
        items(&list)
            .ok_or(Error::Unexpected)?
            .iter()
            .map(|v| self.parse_pull(v).ok_or(Error::Unexpected))
            .collect()
    }

    pub fn create_pull_request(
        &self,
        project: &str,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Result<PullRequest, Error> {
        let (path, body) = match self.service {
            Service::GitHub | Service::Gitea => (
                "/pulls",
                json!({"head": head, "base": base, "title": title, "body": body}),
            ),
            Service::GitLab => (
                "/merge_requests",
                json!({"source_branch": head, "target_branch": base, "title": title, "description": body}),
            ),
            Service::Bitbucket => (
                "/pullrequests",
                json!({"title": title, "description": body,
                       "source": {"branch": {"name": head}}, "destination": {"branch": {"name": base}}}),
            ),
            Service::BitbucketServer => (
                "/pull-requests",
                json!({"title": title, "description": body,
                       "fromRef": {"id": format!("refs/heads/{head}")},
                       "toRef": {"id": format!("refs/heads/{base}")}}),
            ),
            Service::AzureDevOps => (
                "/pullrequests?api-version=7.1",
                json!({"title": title, "description": body,
                       "sourceRefName": format!("refs/heads/{head}"),
                       "targetRefName": format!("refs/heads/{base}")}),
            ),
        };
        let pr = self.call(
            "POST",
            &format!("{}{path}", self.project(project)),
            Some(body),
        )?;
        self.parse_pull(&pr).ok_or(Error::Unexpected)
    }

    /// CI's verdict on `pr`'s latest commit; `None` when nothing ran on it.
    pub fn checks(&self, project: &str, pr: &PullRequest) -> Result<Option<Checks>, Error> {
        let (base, sha) = (self.project(project), &pr.sha);
        if sha.is_empty() && self.service != Service::GitLab {
            return Ok(None);
        }
        let answers = match self.service {
            // commit statuses, and the check runs GitHub Actions reports through
            Service::GitHub => vec![
                self.call("GET", &format!("{base}/commits/{sha}/status"), None)?,
                self.call("GET", &format!("{base}/commits/{sha}/check-runs"), None)?,
            ],
            Service::Gitea => {
                vec![self.call("GET", &format!("{base}/commits/{sha}/status"), None)?]
            }
            Service::GitLab => {
                vec![self.call("GET", &format!("{base}/merge_requests/{}", pr.number), None)?]
            }
            Service::Bitbucket => {
                vec![self.call("GET", &format!("{base}/commit/{sha}/statuses"), None)?]
            }
            Service::BitbucketServer => vec![self.call(
                "GET",
                &format!("https://{}/rest/build-status/1.0/commits/{sha}", self.host),
                None,
            )?],
            Service::AzureDevOps => vec![self.call(
                "GET",
                &format!("{base}/pullrequests/{}/statuses?api-version=7.1", pr.number),
                None,
            )?],
        };
        Ok(fold_checks(answers.iter().flat_map(check_states)))
    }

    /// Reviewers' verdict on `pr`; `None` while nobody approved or asked for changes.
    pub fn review(&self, project: &str, pr: &PullRequest) -> Result<Option<Review>, Error> {
        let (base, n) = (self.project(project), pr.number);
        let path = match self.service {
            Service::GitHub | Service::Gitea => format!("{base}/pulls/{n}/reviews"),
            Service::GitLab => format!("{base}/merge_requests/{n}/approvals"),
            Service::Bitbucket => format!("{base}/pullrequests/{n}"),
            Service::BitbucketServer => format!("{base}/pull-requests/{n}"),
            Service::AzureDevOps => format!("{base}/pullrequests/{n}?api-version=7.1"),
        };
        Ok(fold_review(&self.call("GET", &path, None)?))
    }

    // shortcut: the 100 most recently active repositories; page when users ask for more.
    /// The repository `project` was forked from: its clone URL and default branch; None when it
    /// isn't a fork (GitHub's and Gitea's `parent`).
    pub fn fork_parent(&self, project: &str) -> Result<Option<(String, String)>, Error> {
        let repo = self.call("GET", &self.project(project), None)?;
        let parent = &repo["parent"];
        Ok(parent["clone_url"]
            .as_str()
            .zip(parent["default_branch"].as_str())
            .map(|(url, branch)| (url.to_owned(), branch.to_owned())))
    }

    /// Creates a repository called `name` for the token's user (GitLab: in `namespace`, by id).
    pub fn create_repository(
        &self,
        name: &str,
        description: &str,
        private: bool,
        namespace: Option<u64>,
    ) -> Result<NewRepository, Error> {
        let (path, body, keys) = match self.service {
            Service::GitHub => (
                "/user/repos",
                json!({"name": name, "description": description, "private": private}),
                ["clone_url", "ssh_url", "html_url"],
            ),
            Service::GitLab => (
                "/projects",
                json!({"name": name, "description": description, "namespace_id": namespace,
                       "visibility": if private { "private" } else { "public" }}),
                ["http_url_to_repo", "ssh_url_to_repo", "web_url"],
            ),
            _ => {
                return Err(Error::Unsupported(
                    "Spork shares projects on GitHub and GitLab only.",
                ));
            }
        };
        let repo = self.call("POST", path, Some(body))?;
        let field = |key: &str| {
            repo[key]
                .as_str()
                .map(str::to_owned)
                .ok_or(Error::Unexpected)
        };
        let [clone, ssh, web] = keys;
        Ok(NewRepository {
            clone_url: field(clone)?,
            ssh_url: field(ssh)?,
            web_url: field(web)?,
        })
    }

    /// GitLab's namespaces the user can create projects in, theirs first.
    pub fn namespaces(&self) -> Result<Vec<Namespace>, Error> {
        let list = self.call("GET", "/namespaces?per_page=100", None)?;
        let mut namespaces: Vec<Namespace> = items(&list)
            .ok_or(Error::Unexpected)?
            .iter()
            .filter_map(parse_namespace)
            .collect();
        namespaces.sort_by_key(|n| !n.personal);
        Ok(namespaces)
    }

    pub fn repositories(&self) -> Result<Vec<Repository>, Error> {
        let path = match self.service {
            Service::GitHub => "/user/repos?per_page=100&sort=pushed",
            Service::Gitea => "/user/repos?limit=50",
            Service::GitLab => "/projects?membership=true&order_by=last_activity_at&per_page=100",
            Service::Bitbucket => "/repositories?role=member&sort=-updated_on&pagelen=100",
            Service::BitbucketServer => "/repos?limit=100",
            Service::AzureDevOps => {
                return Err(Error::Unsupported(
                    "Azure DevOps repositories aren't listed here yet: paste the clone URL.",
                ));
            }
        };
        let list = self.call("GET", path, None)?;
        Ok(items(&list)
            .ok_or(Error::Unexpected)?
            .iter()
            .filter_map(|v| self.parse_repository(v))
            .collect())
    }

    /// Unread notifications (GitLab: pending to-dos), newest first (M15 §4).
    pub fn notifications(&self) -> Result<Vec<Notice>, Error> {
        let path = match self.service {
            Service::GitHub | Service::Gitea => "/notifications",
            Service::GitLab => "/todos?state=pending",
            Service::Bitbucket | Service::BitbucketServer | Service::AzureDevOps => {
                return Err(Error::Unsupported("This service has no notifications API."));
            }
        };
        let list = self.call("GET", path, None)?;
        Ok(items(&list)
            .ok_or(Error::Unexpected)?
            .iter()
            .filter_map(|v| self.parse_notice(v))
            .collect())
    }

    fn parse_notice(&self, v: &Value) -> Option<Notice> {
        Some(match self.service {
            Service::GitHub => Notice {
                title: v["subject"]["title"].as_str()?.to_owned(),
                repository: v["repository"]["full_name"].as_str()?.to_owned(),
                kind: v["subject"]["type"].as_str().unwrap_or("").to_owned(),
                // the API gives an API URL; its web page has the same path on github.com
                url: v["subject"]["url"].as_str().map_or_else(
                    || {
                        format!(
                            "https://github.com/{}",
                            v["repository"]["full_name"].as_str().unwrap_or("")
                        )
                    },
                    |u| {
                        u.replacen("https://api.github.com/repos/", "https://github.com/", 1)
                            .replacen("/pulls/", "/pull/", 1)
                    },
                ),
            },
            Service::Gitea => Notice {
                title: v["subject"]["title"].as_str()?.to_owned(),
                repository: v["repository"]["full_name"].as_str()?.to_owned(),
                kind: v["subject"]["type"].as_str().unwrap_or("").to_owned(),
                url: v["subject"]["html_url"].as_str()?.to_owned(),
            },
            Service::GitLab => Notice {
                title: v["target"]["title"]
                    .as_str()
                    .or_else(|| v["body"].as_str())?
                    .to_owned(),
                repository: v["project"]["path_with_namespace"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned(),
                kind: v["target_type"].as_str().unwrap_or("").to_owned(),
                url: v["target_url"].as_str()?.to_owned(),
            },
            Service::Bitbucket | Service::BitbucketServer | Service::AzureDevOps => return None,
        })
    }

    fn parse_pull(&self, v: &Value) -> Option<PullRequest> {
        Some(match self.service {
            // Gitea's pull request objects follow GitHub's shape
            Service::GitHub | Service::Gitea => PullRequest {
                number: v["number"].as_u64()?,
                title: v["title"].as_str()?.to_owned(),
                description: v["body"].as_str().unwrap_or("").to_owned(),
                author: v["user"]["login"].as_str().unwrap_or("").to_owned(),
                head: v["head"]["ref"].as_str()?.to_owned(),
                base: v["base"]["ref"].as_str()?.to_owned(),
                url: v["html_url"].as_str()?.to_owned(),
                draft: v["draft"].as_bool().unwrap_or(false),
                sha: v["head"]["sha"].as_str().unwrap_or("").to_owned(),
                // a deleted fork leaves `head.repo` null
                fork: v["head"]["repo"]["full_name"] != v["base"]["repo"]["full_name"],
            },
            Service::GitLab => PullRequest {
                number: v["iid"].as_u64()?,
                title: v["title"].as_str()?.to_owned(),
                description: v["description"].as_str().unwrap_or("").to_owned(),
                author: v["author"]["username"].as_str().unwrap_or("").to_owned(),
                head: v["source_branch"].as_str()?.to_owned(),
                base: v["target_branch"].as_str()?.to_owned(),
                url: v["web_url"].as_str()?.to_owned(),
                draft: v["draft"].as_bool().unwrap_or(false),
                sha: v["sha"].as_str().unwrap_or("").to_owned(),
                fork: v["source_project_id"] != v["target_project_id"],
            },
            Service::Bitbucket => PullRequest {
                number: v["id"].as_u64()?,
                title: v["title"].as_str()?.to_owned(),
                description: v["description"].as_str().unwrap_or("").to_owned(),
                author: v["author"]["nickname"]
                    .as_str()
                    .or_else(|| v["author"]["display_name"].as_str())
                    .unwrap_or("")
                    .to_owned(),
                head: v["source"]["branch"]["name"].as_str()?.to_owned(),
                base: v["destination"]["branch"]["name"].as_str()?.to_owned(),
                url: v["links"]["html"]["href"].as_str()?.to_owned(),
                draft: v["draft"].as_bool().unwrap_or(false),
                sha: v["source"]["commit"]["hash"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned(),
                fork: v["source"]["repository"]["full_name"]
                    != v["destination"]["repository"]["full_name"],
            },
            Service::BitbucketServer => PullRequest {
                number: v["id"].as_u64()?,
                title: v["title"].as_str()?.to_owned(),
                description: v["description"].as_str().unwrap_or("").to_owned(),
                author: v["author"]["user"]["name"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned(),
                head: v["fromRef"]["displayId"].as_str()?.to_owned(),
                base: v["toRef"]["displayId"].as_str()?.to_owned(),
                url: v["links"]["self"][0]["href"].as_str()?.to_owned(),
                draft: v["draft"].as_bool().unwrap_or(false),
                sha: v["fromRef"]["latestCommit"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned(),
                fork: v["fromRef"]["repository"]["id"] != v["toRef"]["repository"]["id"],
            },
            Service::AzureDevOps => {
                let number = v["pullRequestId"].as_u64()?;
                let branch = |key: &str| {
                    v[key]
                        .as_str()
                        .map(|r| r.strip_prefix("refs/heads/").unwrap_or(r).to_owned())
                };
                PullRequest {
                    number,
                    title: v["title"].as_str()?.to_owned(),
                    description: v["description"].as_str().unwrap_or("").to_owned(),
                    author: v["createdBy"]["displayName"]
                        .as_str()
                        .unwrap_or("")
                        .to_owned(),
                    head: branch("sourceRefName")?,
                    base: branch("targetRefName")?,
                    url: format!(
                        "{}/pullrequest/{number}",
                        v["repository"]["webUrl"].as_str()?
                    ),
                    draft: v["isDraft"].as_bool().unwrap_or(false),
                    sha: v["lastMergeSourceCommit"]["commitId"]
                        .as_str()
                        .unwrap_or("")
                        .to_owned(),
                    fork: v["forkSource"].is_object(),
                }
            }
        })
    }

    fn parse_repository(&self, v: &Value) -> Option<Repository> {
        Some(match self.service {
            Service::GitHub | Service::Gitea => Repository {
                full_name: v["full_name"].as_str()?.to_owned(),
                clone_url: v["clone_url"].as_str()?.to_owned(),
                private: v["private"].as_bool().unwrap_or(false),
            },
            Service::GitLab => Repository {
                full_name: v["path_with_namespace"].as_str()?.to_owned(),
                clone_url: v["http_url_to_repo"].as_str()?.to_owned(),
                private: v["visibility"].as_str() != Some("public"),
            },
            Service::Bitbucket => {
                let href = v["links"]["clone"]
                    .as_array()?
                    .iter()
                    .find(|l| l["name"] == "https")?["href"]
                    .as_str()?;
                // drop the `user@` Bitbucket puts in, so git asks the helper by host alone
                let (scheme, rest) = href.split_once("://")?;
                let rest = rest
                    .split_once('@')
                    .filter(|(user, _)| !user.contains('/'))
                    .map_or(rest, |(_, after)| after);
                Repository {
                    full_name: v["full_name"].as_str()?.to_owned(),
                    clone_url: format!("{scheme}://{rest}"),
                    private: v["is_private"].as_bool().unwrap_or(true),
                }
            }
            Service::BitbucketServer => Repository {
                full_name: format!("{}/{}", v["project"]["key"].as_str()?, v["slug"].as_str()?),
                clone_url: v["links"]["clone"]
                    .as_array()?
                    .iter()
                    .find(|l| l["name"] == "http")?["href"]
                    .as_str()?
                    .to_owned(),
                private: !v["public"].as_bool().unwrap_or(false),
            },
            Service::AzureDevOps => return None,
        })
    }
}

/// The results in a status answer, whichever service's shape: GitHub / Gitea `statuses`, GitHub
/// `check_runs`, a GitLab merge request's `head_pipeline`, Bitbucket's `values`.
fn check_states(v: &Value) -> Vec<Checks> {
    let state = |s: &str| match s.to_ascii_lowercase().as_str() {
        "success" | "successful" | "succeeded" | "neutral" | "skipped" | "warning" => {
            Some(Checks::Passed)
        }
        "failure" | "failed" | "error" | "stopped" | "canceled" | "cancelled" | "timed_out"
        | "action_required" => Some(Checks::Failed),
        // manual jobs and anything unknown say nothing
        "manual" | "notapplicable" | "notset" | "" => None,
        _ => Some(Checks::Running), // pending, queued, in_progress, running, created, …
    };
    let list = |key: &str| v[key].as_array().into_iter().flatten();
    let statuses = list("statuses")
        .chain(list("values"))
        .chain(list("value"))
        .filter_map(|s| state(s["state"].as_str()?));
    let runs = list("check_runs").filter_map(|r| match r["status"].as_str()? {
        "completed" => state(r["conclusion"].as_str()?),
        _ => Some(Checks::Running),
    });
    let pipeline = v["head_pipeline"]["status"].as_str().and_then(state);
    statuses.chain(runs).chain(pipeline).collect()
}

/// The verdict in a review answer, whichever service's shape: GitHub / Gitea review lists (each
/// reviewer's latest counts), GitLab approvals, a Bitbucket pull request's participants.
fn fold_review(v: &Value) -> Option<Review> {
    let mut latest: Vec<(String, String)> = Vec::new();
    if let Some(reviews) = v.as_array() {
        for r in reviews {
            let who = r["user"]["login"].as_str().unwrap_or("").to_owned();
            let state = r["state"].as_str().unwrap_or("").to_uppercase();
            // comments don't change where someone stands
            if matches!(state.as_str(), "COMMENTED" | "COMMENT" | "PENDING" | "") {
                continue;
            }
            latest.retain(|(w, _)| *w != who);
            latest.push((who, state));
        }
    } else if let Some(by) = v["approved_by"].as_array() {
        return (!by.is_empty()).then_some(Review::Approved);
    } else if let Some(people) = v["reviewers"].as_array() {
        // Bitbucket Server: status; Azure DevOps: vote (10 approved … -10 rejected)
        for p in people {
            let state = match (p["status"].as_str(), p["vote"].as_i64()) {
                (Some("NEEDS_WORK"), _) => "CHANGES_REQUESTED",
                (Some("APPROVED"), _) => "APPROVED",
                (_, Some(v)) if v <= -5 => "CHANGES_REQUESTED",
                (_, Some(v)) if v >= 5 => "APPROVED",
                _ => continue,
            };
            latest.push((String::new(), state.to_owned()));
        }
    } else if let Some(people) = v["participants"].as_array() {
        for p in people {
            let state = match (p["state"].as_str(), p["approved"].as_bool()) {
                (Some("changes_requested"), _) => "CHANGES_REQUESTED",
                (_, Some(true)) => "APPROVED",
                _ => continue,
            };
            latest.push((String::new(), state.to_owned()));
        }
    }
    let has = |s: &[&str]| latest.iter().any(|(_, st)| s.contains(&st.as_str()));
    if has(&["CHANGES_REQUESTED", "REQUEST_CHANGES"]) {
        Some(Review::ChangesRequested)
    } else if has(&["APPROVED"]) {
        Some(Review::Approved)
    } else {
        None
    }
}

/// Any failure fails; otherwise anything still running makes it running.
fn fold_checks(states: impl Iterator<Item = Checks>) -> Option<Checks> {
    states.reduce(|a, b| match (a, b) {
        (Checks::Failed, _) | (_, Checks::Failed) => Checks::Failed,
        (Checks::Running, _) | (_, Checks::Running) => Checks::Running,
        _ => Checks::Passed,
    })
}

/// GitHub's device flow, to sign in in the browser instead of pasting a token: the code the user
/// types at `verification_uri`, and what to poll with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    /// Seconds between polls.
    pub interval: u64,
}

fn post_form(url: &str, form: &str) -> Result<Value, Error> {
    let mut response = ureq::post(url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("User-Agent", "Spork")
        .send(form)
        .map_err(|e| Error::Network(e.to_string()))?;
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| Error::Network(e.to_string()))?;
    serde_json::from_str(&text).map_err(|_| Error::Unexpected)
}

/// Starts signing in to GitHub (`host`: github.com or an Enterprise server) as the OAuth app
/// `client_id`, asking for the scopes Spork uses.
pub fn github_device_code(host: &str, client_id: &str) -> Result<DeviceCode, Error> {
    let v = post_form(
        &format!("https://{host}/login/device/code"),
        &format!("client_id={client_id}&scope=repo%20notifications%20read:user"),
    )?;
    parse_device_code(&v)
}

fn parse_device_code(v: &Value) -> Result<DeviceCode, Error> {
    if let Some(e) = v["error_description"].as_str() {
        return Err(Error::Http(400, e.to_owned()));
    }
    Ok(DeviceCode {
        device_code: v["device_code"]
            .as_str()
            .ok_or(Error::Unexpected)?
            .to_owned(),
        user_code: v["user_code"].as_str().ok_or(Error::Unexpected)?.to_owned(),
        verification_uri: v["verification_uri"]
            .as_str()
            .ok_or(Error::Unexpected)?
            .to_owned(),
        interval: v["interval"].as_u64().unwrap_or(5),
    })
}

/// One poll of the device flow: the token once the user approved, `None` while still waiting,
/// an error when they declined or the code expired.
pub fn github_device_token(
    host: &str,
    client_id: &str,
    device_code: &str,
) -> Result<Option<String>, Error> {
    let v = post_form(
        &format!("https://{host}/login/oauth/access_token"),
        &format!(
            "client_id={client_id}&device_code={device_code}&grant_type=urn:ietf:params:oauth:grant-type:device_code"
        ),
    )?;
    parse_token_answer(&v)
}

fn parse_token_answer(v: &Value) -> Result<Option<String>, Error> {
    match (v["access_token"].as_str(), v["error"].as_str()) {
        (Some(token), _) => Ok(Some(token.to_owned())),
        (_, Some("authorization_pending" | "slow_down")) => Ok(None),
        (_, Some(_)) => Err(Error::Http(
            400,
            v["error_description"]
                .as_str()
                .unwrap_or("Sign-in failed")
                .to_owned(),
        )),
        _ => Err(Error::Unexpected),
    }
}

/// A list answer: a bare array, Bitbucket's page object `{"values": [...]}`, or Azure DevOps'
/// `{"value": [...]}`.
fn parse_namespace(v: &Value) -> Option<Namespace> {
    Some(Namespace {
        id: v["id"].as_u64()?,
        path: v["full_path"].as_str()?.to_owned(),
        personal: v["kind"] == "user",
    })
}

fn items(list: &Value) -> Option<&Vec<Value>> {
    list.as_array()
        .or_else(|| list["values"].as_array())
        .or_else(|| list["value"].as_array())
}

/// Bitbucket Server's project key and repository slug in a clone path: `scm/KEY/repo` (HTTPS)
/// or `key/repo` (SSH).
fn bbs_parts(project: &str) -> (&str, &str) {
    let path = project.strip_prefix("scm/").unwrap_or(project);
    path.rsplit_once('/').unwrap_or(("", path))
}

/// Azure DevOps' `org/project` (or `project` on *.visualstudio.com) and repository in a clone
/// path: `org/project/_git/repo` (HTTPS) or `v3/org/project/repo` (SSH).
fn azure_parts(project: &str) -> (String, String) {
    let path = project.strip_prefix("v3/").unwrap_or(project);
    let parts: Vec<&str> = path.split('/').filter(|p| *p != "_git").collect();
    match parts.split_last() {
        Some((repo, base)) => (base.join("/"), (*repo).to_owned()),
        None => (String::new(), path.to_owned()),
    }
}

/// Standard base64, for Azure DevOps' basic auth (no dependency for a few lines).
fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, b| n << 8 | u32::from(*b)) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                ABC[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_services_and_parses_web_urls() {
        assert_eq!(Service::detect("github.com"), Some(Service::GitHub));
        assert_eq!(
            Service::detect("github.corp.example"),
            Some(Service::GitHub)
        );
        assert_eq!(Service::detect("gitlab.com"), Some(Service::GitLab));
        assert_eq!(Service::detect("codeberg.org"), Some(Service::Gitea));
        assert_eq!(Service::detect("gitea.example.com"), Some(Service::Gitea));
        assert_eq!(Service::detect("bitbucket.org"), Some(Service::Bitbucket));
        assert_eq!(
            parse_web_url("https://github.com/o/r"),
            Some(("github.com".into(), "o/r".into()))
        );
        assert_eq!(
            parse_web_url("https://gitlab.com/group/sub/r"),
            Some(("gitlab.com".into(), "group/sub/r".into()))
        );
        assert_eq!(parse_web_url("https://github.com/o"), None);
        assert_eq!(parse_web_url("http://github.com/o/r"), None);
    }

    #[test]
    fn parses_pull_and_merge_requests() {
        // trimmed from the services' documented responses
        let gh = Client::new(Service::GitHub, "github.com", "t");
        let v: Value = serde_json::from_str(
            r#"{"number": 42, "title": "Add login", "draft": true,
                "html_url": "https://github.com/o/r/pull/42", "user": {"login": "ann"},
                "head": {"ref": "feature/login"}, "base": {"ref": "main"}}"#,
        )
        .unwrap();
        let pr = gh.parse_pull(&v).unwrap();
        assert_eq!(
            (pr.number, pr.author.as_str(), pr.head.as_str(), pr.draft),
            (42, "ann", "feature/login", true)
        );

        let gl = Client::new(Service::GitLab, "gitlab.com", "t");
        let v: Value = serde_json::from_str(
            r#"{"id": 9001, "iid": 7, "title": "Fix", "draft": false,
                "web_url": "https://gitlab.com/g/r/-/merge_requests/7", "author": {"username": "bob"},
                "source_branch": "fix", "target_branch": "main"}"#,
        )
        .unwrap();
        let mr = gl.parse_pull(&v).unwrap();
        assert_eq!(
            (mr.number, mr.author.as_str(), mr.base.as_str()),
            (7, "bob", "main")
        );
        assert_eq!(gl.parse_pull(&json!({"iid": 1})), None);
        assert!(!pr.fork && !mr.fork);
        let from_fork = json!({"number": 3, "title": "T", "html_url": "u",
            "head": {"ref": "main", "repo": {"full_name": "ann/r"}},
            "base": {"ref": "main", "repo": {"full_name": "o/r"}}});
        assert!(
            parse_pull_request(Service::GitHub, &from_fork)
                .unwrap()
                .fork
        );

        // Gitea: GitHub's shape, without a draft flag
        let gt = Client::new(Service::Gitea, "codeberg.org", "t");
        let v: Value = serde_json::from_str(
            r#"{"number": 3, "title": "Docs", "html_url": "https://codeberg.org/o/r/pulls/3",
                "user": {"login": "cy"}, "head": {"ref": "docs"}, "base": {"ref": "main"}}"#,
        )
        .unwrap();
        let pr = gt.parse_pull(&v).unwrap();
        assert_eq!((pr.number, pr.author.as_str(), pr.draft), (3, "cy", false));
        assert_eq!(gt.api, "https://codeberg.org/api/v1");
    }

    #[test]
    fn parses_notifications() {
        let gh = Client::new(Service::GitHub, "github.com", "t");
        let v: Value = serde_json::from_str(
            r#"{"subject": {"title": "Add login", "type": "PullRequest",
                "url": "https://api.github.com/repos/o/r/pulls/42"},
                "repository": {"full_name": "o/r"}}"#,
        )
        .unwrap();
        let n = gh.parse_notice(&v).unwrap();
        assert_eq!(
            (n.url.as_str(), n.repository.as_str()),
            ("https://github.com/o/r/pull/42", "o/r")
        );

        let gl = Client::new(Service::GitLab, "gitlab.com", "t");
        let v: Value = serde_json::from_str(
            r#"{"target_type": "MergeRequest", "target": {"title": "Fix"},
                "target_url": "https://gitlab.com/g/r/-/merge_requests/7",
                "project": {"path_with_namespace": "g/r"}}"#,
        )
        .unwrap();
        assert_eq!(gl.parse_notice(&v).unwrap().title, "Fix");
    }

    #[test]
    fn endpoints_per_service() {
        assert_eq!(
            Client::new(Service::GitHub, "github.com", "t").api,
            "https://api.github.com"
        );
        assert_eq!(
            Client::new(Service::GitHub, "git.corp", " t\n").api,
            "https://git.corp/api/v3"
        );
        assert_eq!(Client::new(Service::GitHub, "git.corp", " t\n").token, "t");
        let gl = Client::new(Service::GitLab, "gitlab.com", "t");
        assert_eq!(gl.api, "https://gitlab.com/api/v4");
        assert_eq!(gl.project("group/sub/r"), "/projects/group%2Fsub%2Fr");
        assert_eq!(
            Service::GitLab.head_ref(7, "fix"),
            ("refs/merge-requests/7/head".into(), "mr/7".into())
        );
    }

    #[test]
    fn bitbucket_server_and_azure_devops() {
        assert_eq!(base64(b":t0k"), "OnQwaw==");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(bbs_parts("scm/PROJ/repo"), ("PROJ", "repo"));
        assert_eq!(bbs_parts("proj/repo"), ("proj", "repo"));
        assert_eq!(
            azure_parts("org/proj/_git/repo"),
            ("org/proj".into(), "repo".into())
        );
        assert_eq!(
            azure_parts("v3/org/proj/repo"),
            ("org/proj".into(), "repo".into())
        );
        assert_eq!(
            Service::detect("ssh.dev.azure.com"),
            Some(Service::AzureDevOps)
        );
        assert_eq!(
            Service::detect("acme.visualstudio.com"),
            Some(Service::AzureDevOps)
        );
        assert_eq!(
            Service::detect("bitbucket.acme.io"),
            Some(Service::BitbucketServer)
        );
        let az = Service::AzureDevOps;
        assert_eq!(
            az.commit_url("ssh.dev.azure.com", "v3/org/proj/repo", "abc"),
            "https://dev.azure.com/org/proj/_git/repo/commit/abc"
        );
        assert_eq!(
            Service::BitbucketServer.project_url("git.acme.io", "scm/PROJ/repo"),
            "https://git.acme.io/projects/PROJ/repos/repo"
        );
        assert_eq!(Service::BitbucketServer.issues_url("h", "a/b"), None);

        let bbs = Client::new(Service::BitbucketServer, "git.acme.io", "t");
        assert_eq!(bbs.api, "https://git.acme.io/rest/api/1.0");
        assert_eq!(bbs.project("scm/PROJ/repo"), "/projects/PROJ/repos/repo");
        let pr = bbs
            .parse_pull(&json!({"id": 4, "title": "Fix", "author": {"user": {"name": "ann"}},
                "fromRef": {"displayId": "fix", "latestCommit": "abc"},
                "toRef": {"displayId": "main"},
                "links": {"self": [{"href": "https://git.acme.io/projects/PROJ/repos/repo/pull-requests/4"}]}}))
            .unwrap();
        assert_eq!(
            (pr.number, pr.head.as_str(), pr.sha.as_str()),
            (4, "fix", "abc")
        );

        let azure = Client::new(az, "dev.azure.com", "t");
        assert_eq!(
            azure.project("org/proj/_git/repo"),
            "/org/proj/_apis/git/repositories/repo"
        );
        let page = json!({"count": 1, "value": [{"pullRequestId": 7, "title": "Add",
            "createdBy": {"displayName": "Bob"}, "sourceRefName": "refs/heads/feature/x",
            "targetRefName": "refs/heads/main", "isDraft": true,
            "lastMergeSourceCommit": {"commitId": "def"},
            "repository": {"webUrl": "https://dev.azure.com/org/proj/_git/repo"}}]});
        let pr = azure.parse_pull(&items(&page).unwrap()[0]).unwrap();
        assert_eq!(
            (pr.head.as_str(), pr.base.as_str(), pr.draft),
            ("feature/x", "main", true)
        );
        assert_eq!(
            pr.url,
            "https://dev.azure.com/org/proj/_git/repo/pullrequest/7"
        );

        // their CI and review answers
        let states = |v: Value| fold_checks(check_states(&v).into_iter());
        assert_eq!(
            states(json!({"value": [{"state": "succeeded"}, {"state": "pending"}]})),
            Some(Checks::Running)
        );
        assert_eq!(
            states(json!({"values": [{"state": "FAILED"}]})),
            Some(Checks::Failed)
        );
        assert_eq!(
            fold_review(&json!({"reviewers": [{"vote": 10}]})),
            Some(Review::Approved)
        );
        assert_eq!(
            fold_review(&json!({"reviewers": [{"status": "APPROVED"}, {"status": "NEEDS_WORK"}]})),
            Some(Review::ChangesRequested)
        );
    }

    #[test]
    fn reads_the_device_flow() {
        let code = parse_device_code(&json!({"device_code": "dc", "user_code": "WDJB-MJHT",
            "verification_uri": "https://github.com/login/device", "expires_in": 900, "interval": 5}))
        .unwrap();
        assert_eq!((code.user_code.as_str(), code.interval), ("WDJB-MJHT", 5));
        assert!(
            parse_device_code(&json!({"error": "unauthorized_client",
            "error_description": "Device flow is disabled"}))
            .is_err()
        );
        assert_eq!(
            parse_token_answer(&json!({"error": "authorization_pending"})),
            Ok(None)
        );
        assert_eq!(
            parse_token_answer(&json!({"access_token": "gho_x", "token_type": "bearer"})),
            Ok(Some("gho_x".into()))
        );
        assert!(parse_token_answer(&json!({"error": "access_denied"})).is_err());
    }

    #[test]
    fn folds_reviews() {
        let review = |s: &str| fold_review(&serde_json::from_str(s).unwrap());
        // GitHub: ann asked for changes, then approved; bob only commented
        assert_eq!(
            review(
                r#"[{"user":{"login":"ann"},"state":"CHANGES_REQUESTED"},
                    {"user":{"login":"bob"},"state":"COMMENTED"},
                    {"user":{"login":"ann"},"state":"APPROVED"}]"#
            ),
            Some(Review::Approved)
        );
        // Gitea's spelling
        assert_eq!(
            review(r#"[{"user":{"login":"c"},"state":"REQUEST_CHANGES"}]"#),
            Some(Review::ChangesRequested)
        );
        assert_eq!(review("[]"), None);
        // GitLab approvals, Bitbucket participants
        assert_eq!(
            review(r#"{"approved_by":[{"user":{}}]}"#),
            Some(Review::Approved)
        );
        assert_eq!(review(r#"{"approved_by":[]}"#), None);
        assert_eq!(
            review(r#"{"participants":[{"approved":true},{"state":"changes_requested"}]}"#),
            Some(Review::ChangesRequested)
        );
    }

    #[test]
    fn folds_ci_checks() {
        let states = |s: &str| check_states(&serde_json::from_str(s).unwrap());
        let fold = |s: &str| fold_checks(states(s).into_iter());
        // GitHub: a status and Actions check runs
        assert_eq!(
            fold(r#"{"state": "success", "statuses": [{"state": "success"}]}"#),
            Some(Checks::Passed)
        );
        assert_eq!(
            fold(
                r#"{"check_runs": [{"status": "completed", "conclusion": "success"},
                                   {"status": "in_progress", "conclusion": null}]}"#
            ),
            Some(Checks::Running)
        );
        assert_eq!(
            fold(r#"{"check_runs": [{"status": "completed", "conclusion": "failure"}]}"#),
            Some(Checks::Failed)
        );
        assert_eq!(fold(r#"{"statuses": [], "total_count": 0}"#), None);
        // GitLab merge request, Bitbucket statuses
        assert_eq!(
            fold(r#"{"iid": 7, "head_pipeline": {"status": "running"}}"#),
            Some(Checks::Running)
        );
        assert_eq!(fold(r#"{"iid": 7, "head_pipeline": null}"#), None);
        assert_eq!(
            fold(r#"{"values": [{"state": "SUCCESSFUL"}, {"state": "FAILED"}]}"#),
            Some(Checks::Failed)
        );
    }

    #[test]
    fn bitbucket() {
        let bb = Client::new(Service::Bitbucket, "bitbucket.org", "t");
        assert_eq!(bb.api, "https://api.bitbucket.org/2.0");
        assert_eq!(bb.project("ws/repo"), "/repositories/ws/repo");
        assert_eq!(
            Service::Bitbucket.head_ref(3, "fix"),
            ("refs/heads/fix".into(), "pr/3".into())
        );
        let page: Value = serde_json::from_str(
            r#"{"pagelen": 50, "values": [{"id": 3, "title": "Fix", "draft": false,
                "author": {"nickname": "ann"},
                "source": {"branch": {"name": "fix"}}, "destination": {"branch": {"name": "main"}},
                "links": {"html": {"href": "https://bitbucket.org/ws/repo/pull-requests/3"}}}]}"#,
        )
        .unwrap();
        let pr = bb.parse_pull(&items(&page).unwrap()[0]).unwrap();
        assert_eq!(
            (pr.number, pr.head.as_str(), pr.base.as_str()),
            (3, "fix", "main")
        );
        assert_eq!(pr.author, "ann");
        let repo: Value = serde_json::from_str(
            r#"{"full_name": "ws/repo", "is_private": true, "links": {"clone": [
                {"name": "https", "href": "https://ann@bitbucket.org/ws/repo.git"},
                {"name": "ssh", "href": "git@bitbucket.org:ws/repo.git"}]}}"#,
        )
        .unwrap();
        let r = bb.parse_repository(&repo).unwrap();
        assert_eq!(r.clone_url, "https://bitbucket.org/ws/repo.git");
        assert!(r.private);
        assert!(matches!(bb.notifications(), Err(Error::Unsupported(_))));
    }

    #[test]
    fn parses_gitlab_namespaces() {
        let ns = parse_namespace(&json!({"id": 4, "full_path": "ann", "kind": "user"}));
        assert_eq!(
            ns,
            Some(Namespace {
                id: 4,
                path: "ann".into(),
                personal: true
            })
        );
        let group = parse_namespace(&json!({"id": 9, "full_path": "corp/web", "kind": "group"}));
        assert_eq!(group.map(|g| g.personal), Some(false));
        assert_eq!(parse_namespace(&json!({"full_path": "x"})), None);
        let bb = Client::new(Service::Bitbucket, "bitbucket.org", "t");
        assert!(matches!(
            bb.create_repository("r", "", false, None),
            Err(Error::Unsupported(_))
        ));
    }
}
