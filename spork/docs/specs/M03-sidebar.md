# M03 — Sidebar (refs browser)

Fork surfaces: SidebarController (+ star, branch filter, active/warning branch icons). Phase: P1.
Components: `Sidebar`, `Tree`, `Badge`, `Input` (filter), `Menu` (context), `Tooltip`.

## 1. Sections (collapsible; collapsed state persisted per repo)
| Section | Source (one call each, all cached in `Entity<Repo>`) |
|---|---|
| Favorites (starred branches) | `repositories.toml → starred_branches` |
| Branches (local) | `git for-each-ref refs/heads --format='%(refname)%00%(objectname)%00%(upstream)%00%(upstream:track,nobracket)%00%(HEAD)%00%(worktreepath)%00%(committerdate:unix)'` |
| Remotes → branches | `for-each-ref refs/remotes …` grouped by remote; `<remote>/HEAD` hidden |
| Tags | `for-each-ref refs/tags --sort=-creatordate --format=…%(objecttype)%00%(*objectname)` |
| Stashes | `git stash list -z --format=%gd%x00%H%x00%gs%x00%ct` |
| Submodules | `git submodule status --recursive` (+ `-` uninit, `+` changed, `U` conflict) |
| Worktrees | `git worktree list --porcelain -z` |
| Pull Requests (if account) | M15 |

Branch names containing `/` render as folders (`feature/login` → `feature ▸ login`).

## 2. Row content
Branch: name, HEAD marker (current), ahead/behind badges `↑2 ↓1` from upstream track, "gone" upstream warning icon, worktree indicator (checked out elsewhere → disabled checkout), star.
Tag: name; annotated vs lightweight icon. Stash: message + relative date. Submodule: path + state.

## 3. Interactions
- Double-click local branch → checkout (M04 dialog only if local changes conflict; otherwise direct).
- Double-click remote branch → Track Remote Branch dialog (M04) or checkout existing tracking branch.
- Single click → select + scroll graph to that commit (M05).
- **Branch filter** toggle on rows: show only checked branches in the graph (multi-select); "Filtered by: … Clear filter" in the list status bar.
- **Drag & drop**: drop branch A onto branch B → menu: Merge A into B / Rebase B onto A (current must be B; otherwise "checkout B first" option). Drop onto remote → Push to.
- Filter box: substring across all sections, auto-expands matches.
- Hide/unhide branch (local only) → `hidden_branches`.

## 4. Context menus (each entry → module op)
Local branch: Checkout · Merge into current · Rebase current onto · Interactive rebase from here · Create branch… · Create tag… · Rename… · Delete… · Push to… · Pull from upstream · Set upstream… · Reset current to here… · Copy name · Star · Compare with current (M05 range) · Git-flow finish (if prefix matches) · Custom commands (M16 Branch target).
Remote branch: Checkout (track) · Checkout as worktree · Merge into current · Rebase onto · Delete from remote · Copy name · Create PR (M15).
Remote: Fetch · Edit… · Remove… · Copy URL · Open in browser.
Tag: Checkout (detached) · Push… · Delete… · Details · Copy name.
Stash: Apply… · Pop · Rename… · Drop… · Create branch from stash (`git stash branch <name> stash@{n}`).
Submodule: Open in new tab · Update · Sync URL · Fetch · Remove…
Worktree: Open in new tab · Reveal · Remove…

## Edge cases
Detached HEAD (show "HEAD (detached at abc123)" at top of Branches). Thousands of branches: virtualize the tree. Branch with same name in local and remote. Unborn branch in empty repo. Upstream configured but remote deleted.

## Tests
Fixture with nested folders, gone upstream, worktree-checked-out branch, annotated+lightweight tags, 3 stashes, uninit submodule → snapshot the tree model.

## Open questions
1. Exact drag&drop menu options in Fork (record). 2. Does Fork sort branches by name or recency (any option)? 3. How the branch filter persists (per session or per repo).
