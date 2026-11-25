# M05 — Commit list, graph, search, bisect

Fork surfaces: RevisionsHeader, RevisionListStatusBar, RevisionGraphTooltip, RevisionDetailsTooltip, SearchTab,
ContextSearchBar, GitPointsView, View-menu toggles, Bisect. Phase: P1 (bisect P3).
Components: `VirtualList` (rows) with a custom-painted graph cell, `Tooltip`, `Input`, `StatusBar`, `Badge`, `Avatar`.

## 1. Data loading
- Source: gix revwalk from the selected tips (default: all local + remote branches + tags + HEAD + stash heads unless hidden), sorted by
  `commit_sort`: **date** (committer date, parents-before-children kept) or **topo** (`--topo-order` semantics).
- Row: `{oid, parents[], author{name,email}, author_time, committer_time, subject, refs[]}`. Body loaded lazily for details.
- Streaming: first 2 000 rows rendered immediately, then batches of 20 000 on the background executor; graph layout is incremental (§2) so batches append.
- "Uncommitted changes" pseudo-row at the top when the worktree/index is dirty (count of files), pinned to HEAD's lane.
- `max_commits` (0 = unlimited) caps memory for giant repos.

## 2. Graph layout (our algorithm — implement in `spork_graph`, pure, snapshot-tested)
Input: rows in display order, each with `parents`. State: `lanes: Vec<Option<Oid>>` (the commit each lane is waiting for).
For each row `c`:
1. `lane = first i where lanes[i]==c`; if none → first `None` slot, else push new (new tip).
2. All other `j` with `lanes[j]==c` → these lanes **merge into** `lane` (emit edges j→lane), set `lanes[j]=None`.
3. If `c` has parents: `lanes[lane] = parents[0]` (first parent continues straight). For each extra parent `p`:
   if some lane already waits for `p` → emit edge lane→that lane; else take first `None` slot (or push) → `lanes[k]=p`, emit edge lane→k.
   If no parents → `lanes[lane]=None` (root).
4. Trim trailing `None`s. Emit `RowLayout{node_lane, edges_in, edges_out, pass_through_lanes}`.
Colours: by lane *identity* (a lane keeps its colour while it continues), from an 8-colour theme palette; HEAD's lane gets the accent colour.
Rendering: per row paint vertical pass-throughs, curved/diagonal connectors between rows, node circle (merge = hollow ring, stash = square, uncommitted = dashed).
Limits: compress when lanes > 40 (draw "+N" gutter). First-parent mode simply ignores parents[1..].

## 3. Columns & header
Graph · Description (ref badges then subject; compact branch labels pref shortens `origin/feature/x` → `o/f/x`) · Author (avatar + name) · Date (relative < 7 days, else absolute; tooltip absolute) · SHA (7). Header right-click toggles columns; widths persisted.
Ref badges: local branch, remote branch, tag, HEAD, stash; current branch bold; local+remote at same commit merged into one badge "main ⇄ origin".

## 4. View toggles (View menu; persisted per repo)
Show All Commits vs current branch only (`Filter Current Branch`) · Show HEAD (scroll+select) · Hide Tags · Hide Stashes in commit list · **Show Lost Commits (reflog)** = add tips from `git reflog --format=%H` of HEAD and all branches (unreachable commits rendered faded) · **Collapse All Merges** = first-parent only · Branch filter from M03. Status bar: "Filtered by: <filters> · Clear filter".

## 5. Search
- **Quick filter bar** (`Cmd-F` in list, ContextSearchBar): matches SHA prefix, author, subject within loaded rows; Enter/Shift-Enter next/prev; highlights matches; doesn't reload.
- **Search tab** (deep): fields Message / Author / Committer / Content (pickaxe) / Path / Date range; git:
  `git log --all --format=… -z [--grep=<re> -i] [--author=<re>] [-S<string> | -G<re>] [--since --until] [-- <path>]`, streamed, cancellable; results list → selecting shows details. Saved recent searches.

## 6. Tooltips
Node hover: full SHA, author/committer, dates, parents, refs. Ref badge hover: upstream + ahead/behind.

## 7. Keyboard
↑/↓ move, PgUp/PgDn, Home/End, `Enter` focus details, `Space` quick-look diff of first file, `Cmd-C` copy SHA, `Cmd-Shift-C` copy subject, `←` jump to first parent, `→` to child, `H` go to HEAD.

## 8. Bisect (Repository ▸ Bisect)
Start: mark bad (default HEAD) and good (selected) → `git bisect start <bad> <good>`. Banner: "Bisecting · ~N steps left · Good / Bad / Skip / Reset". Buttons → `git bisect good|bad|skip`, Reset → `git bisect reset`. Rows annotated from `git bisect log`. Result: highlight first bad commit.

## Edge cases
Shallow clones (grafted roots → dashed top edge); octopus merges (≥3 parents); commits with identical timestamps; orphan branches (multiple roots); replace refs; 1M+ commits; repo with 50k tags (load tags lazily for badges).

## Tests
`spork_graph` snapshot suite: linear, simple merge, criss-cross, octopus, orphan roots, stash with untracked parent, first-parent mode, 1M synthetic commits < 1 s layout. Parity: Spork row order == `git log --date-order` / `--topo-order` for fixtures.

## Open questions (REA candidates)
1. Fork's lane colour assignment rules and how it keeps branch colours stable across refreshes (describe in prose).
2. How Fork chooses which refs are "tips" by default (does it include remote branches? tags?).
3. Exact quick-filter matching semantics (prefix vs substring, case).
4. What `GitPointsView` renders (likely the graph node/branch "points").
