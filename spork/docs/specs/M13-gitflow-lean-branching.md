# M13 — Git-flow & lean branching

Fork surfaces: GitflowInit, StartFeature, FinishFeature, StartRelease, FinishRelease, StartHotfix, FinishHotfix, LeanBranchingStart, LeanBranchingFinish.
Phase: P4. Components: `Dialog`, `Form`, `Input`, `Checkbox`, `Menu` (Repository ▸ Git Flow).

Fork bundles **git-flow-avh 1.11.0** (BSD-licensed; its shFlags helper is LGPL). Unlike Fork's code, **that source may be read directly**
(`Resources/gitflow-avh/git-flow-*`). We re-implement the subset below in Rust (`spork-ops::gitflow`) so it works on Windows without bash,
and we stay **compatible with the git-flow CLI's config keys**, so teams can mix Spork and the CLI.

## 1. Init
Fields: Production branch (`master`/`main`) · Development branch (`develop`) · Feature / Release / Hotfix prefixes (`feature/`, `release/`, `hotfix/`) · (avh extras, Spork: Bugfix `bugfix/`, Support `support/`) · Version tag prefix (empty).
Writes `git config gitflow.branch.master|develop`, `gitflow.prefix.feature|release|hotfix|bugfix|support|versiontag`.
Creates `develop` from production if missing (`git branch develop <master>`); creates production with an empty initial commit in an unborn repo.
Menu shows "Init" until `gitflow.branch.develop` exists, then Start/Finish entries.

## 2. Start
| Kind | Fields | Git |
|---|---|---|
| Feature | name · start at (default develop) | `git switch -c feature/<n> <base>` |
| Release | version · start at (default develop) | `git switch -c release/<v> <base>` |
| Hotfix | version · start at (default production) | `git switch -c hotfix/<v> <base>` |
Validate that base is up to date with its remote (warn, offer fetch), and that no other release/hotfix branch exists (git-flow rule; warn only).

## 3. Finish
**Feature** — options: Rebase before merging · No fast-forward · Delete branch (default on).
1. optional `git rebase develop feature/<n>`; 2. `git switch develop`; 3. `git merge [--no-ff] feature/<n>`; 4. `git branch -d feature/<n>` (+ remote delete if it was pushed and option set).
**Release** — options: Tag message · Delete branch · (Spork adv.) Push after finish, No back-merge, Sign tag.
1. `git switch <master>` → `git merge --no-ff release/<v>`; 2. `git tag -a <prefix><v> -m <msg>`; 3. `git switch develop` → `git merge --no-ff <prefix><v>` (back-merge the tag, avh default); 4. delete branch; 5. optional push master, develop, tag.
**Hotfix** — same as release; if a release branch exists, back-merge into it instead of develop (git-flow rule).
Each step is a separate git call; a conflict mid-way stops with the M11 banner and a "Resume git-flow finish" button that continues from the saved step (state in `.git/spork/gitflow-finish.json`).

## 4. Lean branching (GitHub-flow)
Main branch from repo settings (M02). **Start branch**: name (`/` folders allowed) · create at (default main) · local-changes control (M04 §0) → `git switch -c <name> <base>`.
**Finish branch**: merge current into main → `git switch <main>` → `git merge --no-ff <branch>` → optionally delete branch. (Spork) "Open PR instead" when an account is linked (M15).

## Tests
Run the same scenario through Spork and the git-flow-avh CLI on identical fixtures; compare resulting refs/tags/merge parents.

## Open questions
1. Which finish options Fork exposes for release/hotfix (labels showed only "Delete branches" + message; record the dialogs). 2. Does Fork's finish push automatically?
