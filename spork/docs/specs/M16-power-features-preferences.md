# M16 — Custom commands, bug-tracker rules, statistics, preferences

Fork surfaces: CustomCommandsPreferences, EditCustomCommandActionWindow, EditCustomCommandUIControlsWindow, CustomCommandUIWindow,
CustomCommandResult, RunSharedCustomCommandConfirmation, BugtrackerRulesViewController, RepositoryOverview, RepositoryOverviewAuthorsTab, RepositoryOverviewCommitsTab,
RepositoryStatistics(+View), StatisticsDateRangePopover, GeneralPreferences, GitPreferences, CommitPreferences, IntegrationPreferences,
UpdatesPreferences, (AI: AiCodeReview, EditAiRequest → 10-ai.md). Phase: P5.
Components: `Settings`, `List`, `Form`, `Select`, `Input`, `Checkbox`, `Dialog`, `Chart`, `DatePicker`, `DataTable`, `Tabs`, `TextView`.

## 1. Custom commands
**Targets** (where the command appears in context menus): Repository · Commit · File · Branch (local / remote) · Submodule.
**Scope**: Local (user settings) or Shared (`.spork/custom-commands.json`, committed with the repo).
**Definition**:
```jsonc
{ "version": 1, "commands": [{
  "name": "Deploy branch", "target": "branch", "refspec_filter": "refs/heads/release/",
  "ui": { "title": "Deploy", "description": "Deploy ${ref:short} to…", "buttons": [{"title":"Deploy","action":0}],
          "controls": [
            {"type":"text", "id":"env", "title":"Environment", "placeholder":"staging", "default":"staging"},
            {"type":"path", "id":"out", "dialog":"folder|file|save", "default_dir":"./dist", "filename":"", "filter":"*.zip"},
            {"type":"ref", "id":"base", "kind":"local|remote|tag|any", "filter":"refs/heads/"},
            {"type":"checkbox", "id":"dry", "title":"Dry run", "checked_value":"--dry-run", "unchecked_value":"", "checked":true}
          ]},
  "actions": [{ "type":"process|bash|url", "path":"/usr/bin/env", "args":"…", "script":"…", "url":"https://…",
                "show_output": true, "wait": true }]
}]}
```
**Variables** (substituted in args/script/url, shell-escaped for process args): `${repo:path}`, `${repo:name}`, `${ref}`, `${ref:short}`,
`${sha}`, `${sha:short}`, `${file}`, `${file:abs}`, `${submodule:path}`, `${remote}`, `${input:<id>}`, `${git}` (path of the git in use).
**Actions**: Start process (path + args) · Bash command (on Windows: Git Bash from MinGit, or PowerShell when `shell:"pwsh"`) · Open URL.
**Execution**: as a job (Activity Manager); "Show output" opens a result window with streamed stdout/stderr and exit code; repo refresh afterwards.
**Security**: shared commands from a repo never run until the user clicks **Trust this repository** (dialog shows every command's full action text); trust is stored per repo path + a hash of the file — any change to the file asks again.

## 2. Bug-tracker rules
Rule = `{regex, url_template, scope: local|shared}`; samples: GitHub (`#(\d+)` → `<repo issues URL>/$1`), Jira (`([A-Z]+-\d+)` → `https://<site>/browse/$1`), Jira multi-project.
Applied in commit subjects/bodies (M05 list, M06 details) as links. Shared in `.spork/bugtracker.json`. Pref to disable highlighting. Regex engine: `regex` crate (linear time; reject invalid with inline error).

## 3. Repository overview & statistics
Data: one streamed pass `git log --no-merges --numstat --format=%x00%H%x00%an%x00%ae%x00%at -z [--since --until]` (gix variant later), aggregated in background.
- **Commits tab**: commits per day/week/month (`Chart` bar), by weekday × hour heatmap, totals.
- **Authors tab**: `DataTable` author · commits · additions · deletions · first/last commit; author aliasing via `.mailmap` (git applies it with `%aN %aE`).
- **Treemap**: current tree file sizes by directory (gix tree walk + blob sizes), click to drill down, colour by extension.
- Date range popover (`DatePicker`): last 7/30/90 days, year, all, custom.

## 4. Preferences (one `Settings` window; keys in overview §5)
| Page | Options |
|---|---|
| General | Appearance (system/light/dark) · Compact branch labels · Sort commits (by date / topologically) · Disable syntax highlighting · Always include tags into fetch · Fetch remotes automatically (+ interval) · Push automatically on commit · Remember checkout state in Create Branch · Update submodules on checkout · Diff font · Space character in branch names · Source code folders |
| Git | Git instance (system / bundled / custom path; shows version) · Global user name & email · Verbose output toggles: trace, curl, packet, performance |
| Commit | Spellcheck · Page guide column · Subject low/high limits |
| Integration | Terminal client (macOS: Terminal, iTerm2, Warp, Ghostty, Kitty, Hyper, WezTerm, Alacritty · Windows: Windows Terminal, PowerShell, Git Bash, cmd · Linux: detect `x-terminal-emulator`, GNOME Terminal, Konsole, Kitty, Alacritty, WezTerm, Ghostty) · External editor · Merge tool · Diff tool (M11 §5) · Default host tool for PRs (browser / in-app) · Command line tool install/uninstall · Highlight bug-tracker links · Avatar service (Gravatar / GitHub / off) |
| AI | provider, model, endpoint, key, editable prompts for commit message / code review / PR description, data limits (10-ai.md) |
| Accounts | M15 |
| Keyboard | keymap viewer/editor (Spork) |
| Updates | Auto-check · Auto-download · Channel (stable / beta) · Interval · Last check time |

"Open in Terminal" launches the chosen terminal at the repo root (per-OS command templates in `spork-ui/terminals.toml`).

## Tests
Custom command variable substitution table (incl. quoting with spaces/quotes/unicode on each OS); trust-hash invalidation; bug-tracker regex rendering snapshots; stats aggregation on a fixture with known numbers; preferences round-trip.

## Open questions
1. Fork's custom command file name/location and variable syntax (look at a shared-commands file Fork writes in a test repo; we define ours independently but an importer helps migrating users). 2. Which terminals Fork supports on macOS (labels show Terminal, Hyper, Kitty, Warp, Ghostty + presumably iTerm).
