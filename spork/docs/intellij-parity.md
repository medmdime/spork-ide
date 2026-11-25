# IntelliJ parity: git features compared

Compared on 2026-11-14 against IntelliJ IDEA Community's open-source VCS code (git4idea, vcs-log,
vcs-impl, dvcs, diff, github, gitlab, collaboration-tools): its action declarations, settings
pages and message bundles. Only feature names and behavior are recorded here; no code or text
was copied. Spork is the first module of a JetBrains-like IDE, so IntelliJ is the reference now
(Fork was before: see fork-parity-gaps.md).

Each line: status, priority, the IntelliJ feature; for a partial one, what Spork lacks.
`[x]` Spork has it · `[~]` partial · `[ ]` missing.
A: used every day. B: useful, less frequent. C: niche, or deliberately different.

Totals: 115 present, 50 partial, 102 missing (267 features).

## Most important gaps for daily use
1. Compare with Current: commits one branch has and the other lacks
2. Show Diff with Working Tree: a branch's files against the local ones
3. Preview of the commits and files a push sends, before pushing
4. Push All up to Here: push only up to the selected commit
5. Fixup… / Squash Into…: commit local changes into an older commit (autosquash)
6. Date filter in the log
7. Recent branches in the branches popup and ⌘B
8. Compare a commit's file, or a file-history revision, with the local file
9. History of a folder
10. Create a patch from local changes; copy as patch
11. Revert Resolved: take back a wrong conflict resolution
12. Pull request details: description, commits, files, timeline
13. Pull request review: line comments, threads, approve / request changes
14. Merge, squash or rebase a pull request from the app
15. Pull request list filters: state, author, assignee, review state

## Git Branches popup and branch actions
- [x] A Branch widget in the toolbar with the branches popup (Update Project, Commit, Push, New Branch, then local / remote / tags)
- [~] A Speed search in the popup over branches and actions (search is the separate ⌘B picker; no actions in results)
- [ ] A Recent branches section
- [x] A Favorite branches (star)
- [~] B Group by directory (sidebar only; the popup lists flat names)
- [x] B Show or hide tags
- [~] A Incoming / outgoing markers on branches (sidebar counts; none in the popup or the widget)
- [~] B Fetch from the popup (toolbar Fetch instead)
- [x] A Checkout a local branch; a remote one as a new tracking branch
- [x] A Checkout with local changes: stash, switch, reapply; or drop the changes
- [ ] B Checkout a remote branch over a diverged local one: rebase it or drop local commits
- [x] A New branch from a branch, tag or commit
- [~] B New Branch dialog: checkout checkbox (no "set tracking branch", no "overwrite existing")
- [ ] B Checkout and Rebase onto Current
- [ ] B Checkout and Update (checkout, fetch its upstream, fast-forward)
- [ ] A Compare with Current: commits in each branch missing from the other
- [ ] A Show Diff with Working Tree
- [x] A Merge into Current
- [x] A Rebase Current onto Selected
- [~] A Pull into Current using Merge / Rebase (merge or rebase onto a remote branch; no fetch first)
- [~] A Update a branch without checking it out (fast-forward from the last fetch; one branch at a time)
- [x] A Push a branch (Push to…: remote, branch name, upstream, tags, force with lease)
- [x] A Rename, also on the remote
- [x] A Delete a local branch (unmerged work only after a second confirmation); delete a remote branch
- [~] B Tracked branch: set (no unset)
- [x] B Copy branch name
- [x] B New worktree from a branch
- [~] B Checkout Tag or Revision by typing a ref (only from tag and commit menus)
- [~] B Clean Up Branches dialog: prefix filter, last commit date, merge status, bulk delete (Spork: Delete Merged Branches)
- [x] A Ongoing operation actions: abort / continue rebase, merge, cherry-pick, revert (in a banner)
- [x] B Branches pane beside the log: click goes to the branch head, menu filters the log to it
- [ ] C Branch operations on all repositories of a multi-repository project (Spork: one repository per tab)

## VCS Log: commit list, graph and filters
- [x] A Commit graph with branch and tag labels; a local branch and its remote copy share one label
- [x] A Branch filter, several branches at once, kept between launches
- [~] A User filter, with "me" (Find matches authors and selects them; nothing hidden, no "me")
- [ ] A Date filter: last 24 hours, 7 days, since / until
- [~] A Paths filter: commits touching chosen files or folders (Find in paths highlights; no tree picker)
- [~] A Text or hash filter with regex and match case (plain text; highlights instead of hiding)
- [ ] B Several Log tabs with their own filters; Log in an editor tab
- [x] B Sort topologically or by date
- [x] B Collapse / expand merged branches, all at once or one by one
- [ ] C Linearize merges
- [ ] C Long edges on or off
- [~] C Compact references: only the first label of a commit (local and remote merged; others still shown)
- [x] B Tag names on or off
- [~] C References left or right of the subject (right only)
- [ ] B Choose columns: author, date, hash (fixed: graph, subject, author, date)
- [ ] B Commit timestamp instead of author time
- [ ] B Highlight my commits, merge commits, the current branch
- [ ] B Highlight commits not cherry-picked to the current branch
- [~] B Show or hide details and diff preview; preview at the bottom or right (fixed layout)
- [ ] B Go to parent / child commit
- [~] A Go To Hash / Branch / Tag (Find takes a SHA; branches and tags by clicking the sidebar)
- [~] B Commit tooltip; maximize the message in a popup (Space quick look)
- [x] A Refresh (automatic on file changes)
- [ ] C Log index for fast path, text and author search (Spork asks git each time)

## VCS Log: actions on commits
- [x] A Copy revision number (SHA); copy subject
- [x] A Checkout revision (detached HEAD)
- [x] A Show repository at revision (All files)
- [x] A New branch / new tag at a commit
- [~] A Reset current branch to here: soft, mixed, hard (no keep)
- [x] A Revert one or several commits
- [x] A Cherry-pick one or several commits
- [~] A Undo Commit, changes back to local (Reset soft on its parent; Undo Last Change drops the changes)
- [x] A Edit commit message, any commit
- [ ] A Fixup… / Squash Into…: commit local changes as fixup of the chosen commit (Spork's fixup melds a commit into its parent)
- [x] A Squash selected commits
- [x] A Drop commits
- [x] A Interactively rebase from here
- [ ] A Push All up to Here
- [ ] B Push Selected Commits: cherry-pick them onto the upstream and push
- [ ] C Add commits to another remote branch
- [ ] B Drop selected changes: take some files' changes out of a commit
- [ ] B Extract selected changes into a separate commit
- [x] A Compare versions: two selected commits
- [ ] B Compare a commit's file with the local version; compare the version before with local
- [x] B Show a file at a revision; open it; restore it (Get)
- [ ] B Cherry-pick or revert only the selected files of a commit
- [x] B Create patch from commits
- [x] B Open the commit on GitHub / GitLab
- [x] B Changes of a merge against each parent
- [ ] B Containing branches in the details ("in 3 branches")
- [ ] B GPG / SSH signature status of commits
- [x] B Issue keys and #123 in messages as links

## Commit tool window (Local Changes)
- [x] A Changes tree grouped by directory
- [x] A Untracked files listed, added by staging
- [x] A Show / hide ignored files
- [x] A Staging area: stage, unstage, stage all, unstage all
- [ ] B Stage all tracked files only
- [ ] C Stage without content (`git add -N`)
- [ ] B Commit all when nothing is staged
- [x] A Stage, unstage or discard selected lines and hunks
- [x] A Rollback files (untracked ones go to the Trash)
- [x] A Amend, with a warning when the commit is already pushed
- [x] A Commit message history
- [x] A Commit template from git config
- [x] A Commit and Push
- [~] B HEAD, staged and local versions in one three-side diff (separate staged and unstaged diffs only)
- [x] B Open in editor; external diff tool; file history from a changed file
- [x] B Ignore with a pattern, in .gitignore or .git/info/exclude
- [x] B Copy path from the repository root
- [~] B Stash selected files (one file at a time)
- [~] B Diff in its own window or tab (Space quick look only)
- [x] B Conflicted files with Accept Yours / Accept Theirs / Merge

## Commit options and checks
- [x] A Sign-off
- [~] A Run git hooks on / off (offered only after a hook refuses)
- [~] B Author of this one commit (per repository only)
- [x] B Sign commits with GPG or SSH (repository settings)
- [ ] C Extra commit with file moves, so renames stay detected
- [ ] B Warn before committing on a detached HEAD or during a rebase
- [ ] B Warn on CRLF line endings
- [ ] B Warn on files larger than N MB
- [ ] C Warn on file names Windows refuses
- [~] B Message checks: subject length (fixed 50 / 72 colors; no body line limit, no blank-line check, no setting)
- [x] B Spelling in the message
- [ ] B Right margin in the message box

## Stash
- [x] A Stash with a message, optionally untracked files
- [ ] B Keep index: staged changes stay in place
- [x] A Stash list with diff preview
- [x] A Pop, apply, drop
- [ ] B Clear all stashes
- [x] B Unstash as a new branch
- [ ] B Reinstate the index when applying
- [~] B Stash diff against the parent or the local file (parent only)
- [x] B Unstash conflicts go to the conflict view

## Shelf (IntelliJ's own; Spork relies on stash)
- [~] C Save to Shelf: keep a copy without removing changes (Spork: snapshot stash "Keep my changes in place")
- [ ] C Shelve / unshelve changes as patches outside git; unshelve some files
- [ ] C Rename, delete, restore applied, show applied shelves; import patches; storage folder
- [ ] C Stashes and shelves in one tab

## Patches
- [ ] B Create a patch from local changes
- [ ] B Copy as patch to the clipboard
- [x] B Apply a patch file to the working copy, or as commits (`git am`)
- [ ] B Apply a patch from the clipboard
- [~] C Apply-patch dialog: map paths, strip prefixes, fix hunks by hand (3-way fallback only)

## Update, fetch, pull
- [x] A Fetch all remotes; fetch one remote
- [x] A Fetch automatically every N minutes
- [ ] C Check the remote for incoming commits without fetching
- [ ] B Fetch tags: all, none, git default
- [x] A Update Project: pull with merge or rebase, stashing local changes around it
- [ ] B Update info afterwards: the files the update changed, filtered by paths
- [~] A Pull dialog: remote branch, rebase, auto-stash (no --ff-only, --no-ff, --squash, --no-commit, several branches)
- [ ] B Update a force-pushed branch (rebase local commits onto the rewritten upstream)
- [ ] C Unshallow repository

## Push
- [ ] A Preview of the commits and files to push, per repository
- [x] A Editable target branch; set upstream
- [~] B Push tags: all, or the current branch's (all only)
- [x] A Force push with lease, also offered when a push is rejected
- [ ] B Protected branches: no force push there, preview pushes to them; rules from GitHub
- [~] B Push rejected: update with merge or rebase, then push again (offers Pull or force; no retry)
- [ ] B Push without hooks (`--no-verify`)

## Merge, rebase, cherry-pick, reset
- [~] A Merge dialog: no-ff, squash, no-commit (no ff-only, message, no-verify, unrelated histories, several branches)
- [~] A Rebase dialog: onto (no --onto with an old base, other branch to rebase, --update-refs, --rebase-merges, --keep-empty, --root, autosquash)
- [x] A Continue, skip, abort rebase; commit or abort merge; continue / abort cherry-pick; abort revert
- [~] A Reset HEAD: soft, mixed, hard (no keep)
- [x] A Cherry-pick with the "cherry picked from" line (-x); without committing
- [ ] C Empty cherry-pick: commit it or skip, as a setting

## Interactive rebase
- [x] A Pick, edit, squash, fixup, drop; reorder by dragging
- [~] A Reword in the plan, typing the new message there (reword is a separate action)
- [ ] B Unite selected rows into one commit with a new message
- [ ] C View the git todo list
- [ ] B From the root commit
- [x] B Stop to edit, amend, continue
- [ ] C Rewrite in memory without touching the working tree

## Conflict resolution and the three-way merge tool
- [x] A Conflicted files with their state; accept yours / theirs
- [~] A Three-way merge tool with left, result and right panes over the base (Spork: per-conflict choices in one pane)
- [x] A Accept left, right or both for each conflict
- [ ] B Apply all non-conflicting changes; resolve simple conflicts automatically
- [x] B Merge in an external tool
- [ ] B Revert Resolved: unmark a file resolved by mistake
- [ ] C Conflict details: the commit that brought the conflict
- [ ] C Compare any two of the three versions (base, yours, theirs)

## Diff viewer
- [x] A Side-by-side and unified
- [x] A Syntax highlighting with word-level changes
- [~] A Ignore whitespace (one switch; no trim, empty lines or formatting modes)
- [~] B Highlighting level: lines, words, split, characters, none (words only)
- [x] A Collapse unchanged fragments / show the entire file
- [~] B Context lines setting (collapsed or entire file)
- [x] A Next / previous difference
- [x] A Search in the diff
- [x] B Soft wraps, visible whitespace, line numbers
- [ ] C Indent guides
- [x] B All files of a commit in one scrolling diff
- [ ] B Go to the next file after the last change
- [ ] C Swap sides
- [x] B External diff tool
- [x] B Image diff; binary files noted
- [ ] C Compare two files, folders, the clipboard, or a blank diff window

## File and folder history
- [x] A File history following renames, with each commit's diff
- [ ] B History of a folder
- [ ] B History of selected lines (`git log -L`)
- [ ] B Compare two revisions of the file
- [ ] B Compare a revision with the local file
- [x] B Restore, open or save a revision
- [ ] B History up to here
- [ ] B Filters in file history: branch, user, date

## Blame (Annotate)
- [x] A Blame a file; jump from a line to its commit
- [x] A Annotate the previous revision
- [~] B Annotate a chosen revision (previous revision only)
- [~] B Options: ignore whitespace, detect moves in a file or across files (moves always detected; no whitespace switch)
- [ ] C Colors by author or age; names as initials, full name or email
- [ ] C Diff on hover
- [ ] C Show / hide merge sources

## Remotes, clone, repository
- [x] A Clone: URL, folder, shallow depth, submodules too
- [x] A Clone from an account's repository list
- [x] A Create a repository (`git init`)
- [x] B Manage remotes: add, edit (with push URL), remove, test the connection
- [x] B Credential helper; SSH passphrases
- [x] B Git console (Activity Log)
- [~] C VCS operations popup (Command Palette; not limited to git actions)
- [ ] B Choose the git executable; show its version

## Worktrees
- [x] B Worktree list
- [x] B New worktree, also from a branch
- [x] B Open a worktree
- [x] B Delete a worktree
- [ ] C Prune stale worktrees; mark prunable ones
- [ ] C Lock / unlock a worktree
- [ ] B Open a pull request in a worktree

## Tags
- [x] A New tag on a commit, lightweight or annotated (Spork also signs)
- [ ] C Force: replace an existing tag
- [x] B Push a tag; push all tags
- [x] B Delete a tag, locally and on the remote
- [x] B Checkout a tag
- [x] B Merge or rebase onto a tag

## Submodules
- [x] B Submodule changes in diffs (commits gained and lost)
- [x] C Clone submodules recursively
(IntelliJ Community has no LFS support; Spork has, see the last section.)

## GitHub
- [x] A Log in through the browser or with a token; GitHub Enterprise; several accounts
- [x] A Clone from GitHub
- [ ] B Share project on GitHub: create the repository and push
- [ ] C Create a gist
- [ ] C Sync a fork with its upstream
- [~] B Open in browser / copy link: repository and commit (no file at a line)
- [x] A Pull request list (open ones)
- [ ] A Pull request filters: state, author, assignee, label, review state, quick filters, sort
- [ ] A Pull request details: description, commits, files with diffs, timeline
- [x] A CI checks and review verdict per pull request
- [ ] B Merge state: conflicts, required reviews, unresolved conversations
- [~] A Create pull request: base, head, title, description, template (no draft, reviewers, assignees, labels)
- [x] A Push the branch when creating the pull request
- [x] A Check out a pull request's branch
- [x] B Open a pull request in the browser; copy its URL
- [ ] A Merge, squash and merge, rebase and merge
- [ ] B Close, reopen; mark ready for review or draft
- [ ] B Request or re-request reviews; set myself as reviewer; edit assignees and labels
- [ ] A Review comments on diff lines; reply; resolve / unresolve threads
- [ ] A Submit a review: approve, request changes, comment; pending comments
- [ ] B Apply a suggested change locally
- [ ] B Mark files viewed
- [~] C Delete the merged branch (Spork deletes remote branches from the sidebar, not from the pull request)

## GitLab
- [x] A Accounts on gitlab.com and self-hosted servers; clone from GitLab
- [ ] B Share project on GitLab
- [ ] C Create a snippet
- [x] A Merge request list (open ones), with pipeline and approval status
- [ ] A Merge request filters
- [ ] A Merge request details and timeline, with events on or off
- [~] A Create merge request (no reviewers, assignees, labels, squash, delete source branch)
- [x] A Check out a merge request's branch
- [ ] A Approve, merge (squash, delete source branch), close, reopen
- [ ] A Discussions on diff lines: draft comments, replies, resolve, submit

## Git settings page
(Commit warnings, protected branches and push settings are listed above.)
- [x] B Staging area (Spork always works with it; IntelliJ can switch to changelists)
- [x] A Update method: merge or rebase (repository settings)
- [~] B Clean the working tree before updating with stash or shelve (stash, as a pull checkbox)
- [~] B "Cherry-picked from" suffix added automatically on protected branches (a menu choice each time)
- [ ] B Show the Push dialog after Commit and Push (always, or for protected branches)
- [ ] C Set up the GPG agent's passphrase prompt
- [ ] C Never run commit hooks (advanced setting)

## Version Control settings
- [x] B Issue navigation: issue keys in messages link to the tracker
- [~] B External diff and merge tools (git's difftool / mergetool config; no settings page)

## Editor-only, for the later IDE
Not compared: these need the code editor.
- Inline blame (annotations) in the editor gutter; author hints above code
- Line status markers in the gutter: next / previous change, rollback, diff of a change, lines into the commit from there
- Changelists: named groups of changes, the active one, moving lines between them, conflicts on edit
- Editable diff: edit the local side in the diff; resolve import conflicts
- Before-commit checks: reformat, rearrange, optimize imports, analyze code, TODO check, cleanup, copyright, run tests
- Review mode: pull / merge request comments in the editor
- History for a method or class; history for an editor selection
- Project tree colors for changed files and folders; files changed in the last N days
- Ask to add new files to git, or remove deleted ones; restore open editors per branch
- Local History (IDE snapshots, not git)

## Spork has, IntelliJ Community lacks
Git LFS · git-flow · bisect · statistics · undo last change · lost commits (reflog) ·
conflict check before merge / rebase · drag and drop of labels and commits · stash rename and
snapshot · first-parent view · search in diff content (`log -S` / `-G`) · AI messages,
reviews, rebase plans and conflict help · custom commands · notifications from the services ·
Gitea / Forgejo, Bitbucket and Azure DevOps · SSH key manager

## The reference app 1.1.24
The reference app is IntelliJ Community stripped to its git client
(1.1.24: IntelliJ 2026.3, build 263.7307; read from the release's macOS app on 2026-11-14). Its
git features are IntelliJ's, compared above. What it ships besides, and where Spork stands:

Bundled plugins: platform VCS, Git, GitHub, GitLab, Modal Commit Interface, Terminal, TextMate
bundles, Tasks and Contexts, Navigation Bar, Structure View, Recent Files, Bookmarks, Images, XML,
and its own customization plugin.
- [x] A The log in the main editor area by default (Spork: the log is the main area)
- [x] B No `.idea` folder in the repository (Spork keeps its settings in git config and its own folder)
- [x] B No file indexing (Spork has none; search runs through git)
- [~] B Syntax highlighting for many languages through TextMate bundles, Vue added (Spork: the diff view's highlighter; no Vue)
- [x] A Terminal tool window with tabs (since 2026-11-14)
- [ ] B Modal Commit Interface: commit from a dialog instead of the Commit tool window
- [ ] B Tasks and Contexts: open an issue as a task, with its own branch and saved context
- [~] C Navigation bar (breadcrumbs) (Spork: repository › branch in the status bar)
- [ ] C Recent Files (⌘E), bookmarks, structure view, image viewer: editor features, for the later IDE
- [ ] C Plugins from the JetBrains Marketplace (Spork: custom commands instead)
