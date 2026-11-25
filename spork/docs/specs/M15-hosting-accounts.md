# M15 — Hosting accounts, repositories, pull requests, notifications

Fork surfaces: AccountsWindow, AccountDetails, AccountRepositories, AddServiceWindow, GitHubLogin, GitHubEnterpriseLogin, GitLabLogin,
BitbucketLogin, BitbucketServerLogin, GiteaLogin, OAuthWebApplicationFlow, NotificationView, RepositoryDetails links, PullRequest icon.
Phase: P5. Components: `Settings`, `List`, `Avatar`, `Dialog`, `Input`, `Select`, `TextView`, `Badge`, `Notification`.

## 1. Services and login methods
| Service | Server field | Login (Spork) | Token scopes we request |
|---|---|---|---|
| GitHub | — | **OAuth device flow** (no client secret needed — fits open source) or Personal Access Token | `repo`, `read:user`, `notifications` |
| GitHub Enterprise | URL | PAT (device flow if enabled on server) | same |
| GitLab (.com or self-hosted) | URL | PAT; OAuth device flow on GitLab ≥ 17.2 | `api` or `read_api`+`write_repository` |
| Bitbucket Cloud | — | email + API token (app passwords are deprecated by Atlassian) | repository read/write, pullrequest read/write |
| Bitbucket Server / Data Center | URL | HTTP access token (TFA users need it) | project/repo read/write |
| Gitea / Forgejo | URL | access token | `read:user`, `write:repository` |
Account record in `accounts.json` (overview §5); token in keyring. "Status" = last validation result (`GET /user` equivalent), "Update token" re-runs login. Avatar cached on disk.

## 2. Using accounts
- Clone dialog: account selector → list repositories (paged, filter box) → fills URL (HTTPS or SSH per account preference).
- Credentials: askpass auto-answers for matching hosts (M10 §1.2); for HTTPS the username is `x-access-token` (GitHub) / `oauth2` (GitLab) / account username (others).
- Remote ↔ account match by host (+ path prefix for enterprise).
- Host URL patterns per service: commit URL, branch URL, compare URL, new-PR URL, issues URL, PR list URL — used by "Open on host", repository details links, bug-tracker defaults.

## 3. Pull requests (Spork scope for 1.0)
- Sidebar section "Pull Requests" (open PRs of the current repo's remote): title, number, author avatar, CI state, review state.
- Actions: Checkout PR (`git fetch <remote> pull/<n>/head:pr/<n>` on GitHub; `merge-requests/<n>/head` on GitLab; fork PRs via the fork's clone URL) · Open in browser · Copy link.
- Create PR from branch: title/body (AI-generated draft optional), base branch, draft toggle → REST API; or "Open compare page in browser" (Fork's "Default GitHub/Bitbucket Tool" pref = which of these to use).
- Out of scope 1.0: in-app code review comments (see AI review instead), merging PRs in-app.

## 4. Notifications
Per account toggle "Enable notifications" → poll GitHub notifications API (respect `X-Poll-Interval`), GitLab todos; show tray/menu-bar badge + list (issue/PR title, repo) → open in browser. Desktop notifications via `notify-rust`.

## 5. Deep links
`x-github-client://openRepo/<url>` ("Open with GitHub Desktop" buttons) → clone or open; register at install time per OS.

## Edge cases
Token expired → status red + banner on next network op; rate limiting (show reset time); SSO-enforced orgs (GitHub returns `X-GitHub-SSO` → link to authorize); self-signed enterprise certs (explicit per-account "trust this certificate" storing the fingerprint).

## Tests
Recorded HTTP fixtures (no live network in CI) per service for: validate token, list repos, list PRs, create PR. Device-flow state machine unit tests.

## Open questions
1. Fork's OAuth client registration per service (we register our own OAuth apps for Spork — needs an org account). 2. What the menu-bar notifications show and the poll interval.
