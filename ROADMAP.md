# Spork IDE: the roadmap

Spork IDE is a fork of [Zed](https://github.com/zed-industries/zed) turned into an IDE that looks and works like a
JetBrains IDE (IntelliJ IDEA's New UI, the Islands themes) one to one. Zed gives the engine: GPUI, the editor, buffers,
languages and LSP, projects and worktrees, the terminal, tasks and the debugger. Everything people see and touch is
rebuilt to IntelliJ's design, with
[intellij-community](https://github.com/JetBrains/intellij-community) (Apache-2.0) as the reference for how each piece
looks and behaves. Spork, the git client built so far (module 1), becomes the IDE's version control.

## Where things are

| What | Where |
| --- | --- |
| This monorepo | `~/Projects/spork-ide`, branch `spork`; remote `upstream` = zed-industries/zed (no GitHub fork yet) |
| Spork, the git client | its crates in `crates/` (the root workspace, since phase 2), its tools, docs and scenarios in `spork/`; its old repository is `~/Projects/Spork` |
| IntelliJ's source, for reference | `~/Projects/intellij-community` (file tree only: `git show HEAD:<path>` reads a file) |
| The reference app, IntelliJ stripped to a git client | `/Applications/Reference.app`, catalogued in Spork's `docs/reference-app` (2231 functionalities) |
| IntelliJ IDEA Community Edition 2025.2.5 | `/Applications/IntelliJ IDEA CE.app`, catalogued in `reference/idea` (3161 functionalities) |
| The IDE's tasks | `reference/idea/tasks`: IntelliJ IDEA's and the reference app's functionalities together (3346), ticked by `tools/e2e` |

Licences: Zed's crates are GPL-3.0-or-later (GPUI and a few others Apache-2.0), Spork is GPL-3.0-or-later, so the two
combine as GPL-3.0-or-later. intellij-community is Apache-2.0: its icons, texts and values may be used with its notice.
No JetBrains logos or product names (IntelliJ, IDEA, JetBrains) appear in the product, except the colour scheme and
theme names Darcula and IntelliJ Light, kept by Decision 13; and no Zed branding either: "Zed" is Zed Industries'
trademark.

## Principles

1. **Zed's engines stay, Zed's UI goes (Decision 14).** Zed's hard functionality stays and is patched as little as
   possible: GPUI, the editor's model, `language` and the language servers (`lsp`), the debugger (`dap`), `project`
   and `worktree`, the terminal engine, `task`, `settings`, the extension and plugin system, remote development. Its
   UI is replaced by Spork's components (`spork_workbench`, gpui-kit as Spork themes it): one implementation of each
   component, shared by Spork and Spork IDE, so the two look and scroll the same. A Zed UI piece that's replaced is
   deleted, not restyled. Our own engines may replace Zed's in a later step.
2. **Adapt, don't drop.** Zed's features stay (remote development, collaboration, Vim mode, extensions, AI, REPL)
   and are rebuilt to IntelliJ's design with Spork's components. Only Zed's git UI (replaced by Spork's) and Zed's
   own services go.
3. **It always builds and runs.** Every step is a commit on `spork` that builds, passes the tests and starts.
4. **Every functionality has its end-to-end test.** Each IntelliJ functionality, and each Zed feature kept, is a task
   (`reference/idea/tasks`, Spork's `docs/reference-app/tasks` extended to the whole IDE in phase 0), ticked only by a
   passing end-to-end scenario, as the git client is now.
5. **Upstream merges stay possible for the engines.** `upstream/main` merges into `spork` every week or two. Our
   changes to engine crates stay small and are listed in `PATCHES.md`, so each merge knows what to keep; the UI crates
   Spork's components replace are ours and may diverge or go.

## Decisions (2026-11-20, 11–13 on 2026-11-21, 14 on 2026-11-23)

| # | Question | Decision |
| --- | --- | --- |
| 1 | Remote development over SSH | Keep, in IntelliJ's design (its remote development UI). |
| 2 | Collaboration (calls, channels, shared projects) | Keep, in IntelliJ's design. It needs a server of our own: `crates/collab` (AGPL) is Zed's server, and Zed's servers are Zed's. Until then (phase 1 on) there is no sign-in and no collaboration panel; both come back with that server. |
| 3 | Vim mode | Keep, as IntelliJ has it: a plugin, off until enabled. |
| 4 | Extensions | Keep, shown and managed in Spork's Plugins interface (Settings › Plugins) now. |
| 5 | AI and REPL | Keep, adapted: AI as IntelliJ's AI Assistant tool window, with the user's own providers (Zed's hosted models need a Zed account). |
| 6 | Repository layout | One monorepo, this one: Spork's history comes in under `spork/`, then its crates join the root workspace. |
| 7 | UI components | Zed's `ui` crate long term, restyled with Spork's IntelliJ tokens; gpui-kit (Spork's views) bridges while they move over. |
| 8 | Tests | An end-to-end scenario for every functionality. |
| 9 | A second reference app | IntelliJ IDEA Community Edition 2025.2.5 (installed), catalogued the way the reference app was. The reference app is built from a newer platform (263), so where the two differ the reference app decides. |
| 10 | Public GitHub fork | Later, when the user says. Releases (2026-11-25): each wave's tag is published on a public release mirror, [medmdime/spork-ide](https://github.com/medmdime/spork-ide), one commit per release without the third-party catalogues (`script/publish-mirror`), its downloads built by `.github/workflows/release.yml`. |
| 11 | Java, Kotlin, Maven and Gradle (366 tasks) | In scope, the Eclipse way under IntelliJ's interface: Eclipse's engines (the JDT language server, with m2e for Maven and Buildship for Gradle) behind IntelliJ's tool windows, dialogs and actions (phase 10). Eclipse has no maintained Kotlin engine: Kotlin's is JetBrains' Kotlin language server, downloaded onto the user's machine, with its own debugger (2026-11-25, phase 10e). |
| 12 | Zed's extension registry | Kept for now: the Plugins page lists and downloads extensions from `api.zed.dev/extensions`, and the app checks it for updates at startup, the one Zed service Spork IDE still calls. A registry of our own may come later. |
| 13 | Darcula and IntelliJ Light | Keep the names, as IntelliJ has them. |
| 14 | How much of Zed to keep | Zed's engines only, "the nice to have": the debugger, the language servers, the editor core, the plugin system, remote development over SSH (the remote server) and the other hard functionality. Zed's UI is replaced by the components Spork and Spork IDE share (Spork is the reference for every surface it has; one implementation each, the same scrolling) and deleted where replaced. Our own engines come in a later step. The reference app is IntelliJ IDEA 2026.3 (the paid edition) besides the 2025.2.5 CE catalogue. |

## What happens to each of Zed's 264 crates

Counted from `cargo metadata` on `upstream/main` (2026-11-20). "Tied to" lists the kept crates that use a group; they
say how much work its removal is.

### Removed

| Group | Crates | Tied to |
| --- | --- | --- |
| Git UI (Spork's replaces it) | git_ui, git_ui_core, git_hosting_providers | project, project_panel, sidebar, title_bar, zed |
| Zed's services | hang_telemetry, feedback, auto_update, auto_update_ui, auto_update_helper (Spork's updater comes later), ai_onboarding, language_models_cloud, web_search_providers; telemetry's sink drops every event | `client`, cloud_api_client, cloud_api_types and cloud_llm_client stay as plumbing (with Zed's sign-in removed, nothing signs in, so nothing calls them), as do telemetry and telemetry_events; github_releases (language servers and debug adapters from GitHub) and zed_credentials_provider (the keychain for every provider's API keys) aren't Zed's services and stay |

### Kept and rebuilt to IntelliJ's design

| Group | Crates | Becomes |
| --- | --- | --- |
| Collaboration | call, channel, collab, collab_ui, livekit_api, livekit_client, audio | IntelliJ's collaboration UI, on our own collab server |
| Remote development | remote, remote_connection, remote_server, recent_projects' remote parts | IntelliJ's remote development (SSH) UI |
| Vim | vim, vim_mode_setting, which_key | A plugin, off by default, as IntelliJ's |
| Extensions | extension, extension_host, extensions_ui, extension_cli, language_extension, theme_extension, debug_adapter_extension | Settings › Plugins (Marketplace, Installed) |
| AI | agent*, acp_*, action_log, edit_prediction*, copilot*, language_model*, the providers, context_server, web_search*, sidebar | AI Assistant tool window, inline completion as IntelliJ's |
| REPL | repl | Kept, restyled |
| Onboarding and developer tools | onboarding, language_onboarding, component_preview, inspector_ui, input_latency_ui, miniprofiler_ui | Onboarding as IntelliJ's New UI onboarding; developer tools in development builds |

### Kept as Zed has them (the engine)

gpui and its platform crates, collections, sum_tree, rope, text, clock, multi_buffer, buffer_diff, streaming_diff,
language, language_core, languages, grammars, lsp, project, worktree, fs, paths, util, settings, settings_content,
settings_json, db, sqlez, task, dap, dap_adapters, terminal, snippet, fuzzy, fuzzy_nucleo, outline, markdown, prettier,
node_runtime, http_client, net, scheduler, extension, extension_host, zed_extension_api, json_schema_store, image_viewer,
svg_preview, mermaid_render, tabular_data_preview (seed of module 2, the database grid), zlog, ztracing.

### The surfaces, Zed crate by crate

| Zed crate | Becomes (IntelliJ) |
| --- | --- |
| zed | `spork_ide`: the app, its name, bundle id, icons, paths, menus |
| workspace (docks, panes, status bar) | The frame: tool window stripes (left, right, bottom, split), Islands panels, editor tabs, status bar widgets |
| title_bar, platform_title_bar | The main toolbar: project widget, VCS widget, run widget, Search Everywhere, settings |
| ui, theme, theme_settings, syntax_theme, icons, file_icons | Islands Dark and Light from Spork's `spork_workbench` tokens, expUI icons, Inter and JetBrains Mono |
| editor (its views) | IntelliJ's editor: gutter (line numbers, change markers, breakpoints, run icons, folding), inspection widget, intention bulb, completion, parameter info, quick documentation |
| project_panel | Project view (scopes, Project Files, Scratches) |
| outline_panel, outline | Structure tool window and File Structure popup (⌘F12) |
| terminal_view | Terminal tool window |
| command_palette | Find Action (⇧⌘A) |
| file_finder, project_symbols, tab_switcher, go_to_line | Search Everywhere (⇧⇧), Go to File/Class/Symbol, Recent Files (⌘E), Switcher (⌃⇥), Go to Line (⌘L) |
| search | Find and Replace bars, Find in Files dialog |
| diagnostics | Problems tool window and the editor's inspection widget |
| debugger_ui | Debug tool window and its layout |
| tasks_ui, task | Run configurations, the run widget, Run tool window |
| settings_ui, keymap_editor | The Settings dialog (Spork's settings window), Keymap page |
| recent_projects | Welcome screen, Recent Projects |
| breadcrumbs | Navigation bar and editor breadcrumbs |
| notifications, activity_indicator | Notifications tool window, balloons, background-task progress in the status bar |
| extensions_ui | Plugins page |
| markdown_preview | Markdown editor with its split preview |
| theme_selector, language_selector, encoding_selector, line_ending_selector, toolchain_selector | Quick Switch Scheme and the status bar's file type, encoding, line separator and interpreter widgets |
| project_bookmarks, call_hierarchy | Bookmarks tool window, Call Hierarchy |
| git (data layer) | Kept in phase 1 (the editor's change markers and file colors use it), then fed by `spork_git` in phase 2 |
| assets/keymaps | IntelliJ's macOS keymap (and Windows/Linux) |

## Phases

Each phase ends with the app building, starting, its end-to-end suite passing and its tasks ticked. The task counts are
the open ones in `reference/idea/tasks` today (all of them: no IDE scenario covers a functionality yet).

### Phase 0: the fork

- Rename the product: app and binary `Spork IDE`, bundle id, icons, `~/Library/Application Support/Spork IDE`, URL
  scheme, crash and log folders; no Zed name or logo left in what users see.
- `PATCHES.md` for our changes to engine crates; upstream merge routine.
- The catalogue extended to the whole IDE: IntelliJ IDEA Community's actions, menus, tool windows and settings
  (from intellij-community's `*Actions.xml` and message bundles, and screenshots of the running IDE), so every phase
  has its tasks.
- CI: fmt, clippy, tests, the end-to-end suite on macOS.

**The catalogue is done (2026-11-21).** IntelliJ IDEA CE 2025.2.5 (IC-252.28238.7) is catalogued in `reference/idea`
the way the reference app was, by the same tools (`spork/tools/reference-catalog`, now taking any IntelliJ-based app) on a Java
project of its own (`ide-fixture.sh`): 3161 functionalities (2450 actions, 220 context menus and popups, 160 toolbars,
206 Settings pages, 89 dialogs, 22 tool windows, 12 menus, 2 windows), each with the plugin it comes from, its
translations from the bundled language packs and its screenshot. `reference/idea/tasks` (`spork/tools/e2e/tasks.py
--ide`) joins it with the reference app's into 3346 tasks: 2046 in both (the reference app's entry decides), 1115 IntelliJ IDEA's only,
185 the reference app's only. 2067 come from the platform and 1143 from a plugin: 366 from Java, Kotlin, Groovy, Maven and
Gradle, 320 from Git, GitHub, GitLab, Mercurial, Subversion and Perforce, 457 from the others (Jupyter, Markdown,
Terminal, Code With Me…); 136 of the reference app's own have no plugin recorded.

**Status (2026-11-22): done.** The rename, `PATCHES.md`, the catalogue and CI (`.github/workflows/ci.yml`; it runs on
the release mirror, its end-to-end jobs only when started by hand). Releases (2026-11-25): a tag pushed by
`script/publish-mirror` builds Spork and Spork IDE for macOS (Apple silicon), Windows and Linux (x64) and publishes them
on the mirror's Releases page, unsigned; the first is v0.1.0-alpha.1.

### Phase 1: strip

Remove Zed's git UI and Zed's services, one group per commit. Exit: the app opens a folder, edits, searches and runs
a terminal, with nothing removed still referenced.

**Status (2026-11-22): done.** Zed's git UI, telemetry, crash and hang uploads, feedback, auto-update, the Zed account
and sign-in, hosted models, edit predictions and plans are gone; the extension registry stays (Decision 12).

### Phase 2: Spork comes in (version control)

- Spork's crates (`spork_git`, `spork_graph`, `spork_patch`, `spork_hosting`, `spork_ai`, `spork_terminal`,
  `spork_workbench`, the views) move from `spork/` into the root workspace. gpui-kit comes in as source and builds
  against this fork's GPUI: Spork's GPUI is a snapshot of Zed 194 commits behind this fork, so the gap is small.
- Extensions show in Spork's Plugins interface.
- Git, Commit, Shelf and Stash tool windows, the branches popup, the Git menu and VCS Operations popup come from
  Spork; the end-to-end harness (`e2e.rs`, `run.py`, the scenarios) comes along.
- The editor's change markers, file status colors and Annotate read Spork's git engine.
- Unblocks most of the 262 git tasks Spork couldn't do alone: files at a revision in an editor tab, line actions,
  Local History, Changelists.

**Status (2026-11-22).** Done: Spork's crates and the vendored gpui-kit in the root workspace; the Git (⌘9) and Commit
(⌘0) tool windows, the branches popup (from the main toolbar's VCS widget and Git ▸ Branches…) and the Git menu in the
IDE (`crates/spork_vcs`); the editor's change markers and IntelliJ's line-status popup, Rollback Lines and Next/Previous
Change; file status colours in the Project tool window and editor tabs; Annotate with Git Blame as IntelliJ's
annotations; diffs (Commit, Repository Diff) and files at a revision in editor tabs; the Git tool window's Console tab;
the Git menu following a merge or rebase; git's password prompts; the Commit tool window's Shelf and Stash tabs (their
toolbars, menus and dialogs, Git ▸ Uncommitted Changes opening them) with a stash's or shelved changelist's diff in an
editor tab as IntelliJ's diff preview (`Stash@{0}: README.md`, `Shelved: README.md`); the Git tool window's Worktrees
tab beside Log and Console (shown with linked worktrees or after Worktrees…, New Worktree…, Refresh, Delete…, Prune,
Open Worktree in a project window of its own); file colours in Search Everywhere, Recent Files and the Switcher (phase
4's). The markers and Annotate read Zed's git layer (`buffer_diff`, `GitBlame`), which follows the unsaved buffer;
`spork_git` reads the disk only. Left: extensions in the Plugins page; deleting a worktree open in a window (IntelliJ
asks to close it first); Show Diff and a double click in the Stash and Shelf tabs focusing the diff tab.

### Phase 3: the frame (main window)

Tool window stripes and Islands panels, the main toolbar and its widgets, editor tabs as IntelliJ's, status bar
widgets, the menu bar in IntelliJ's order (File, Edit, View, Navigate, Code, Refactor, Build, Run, Tools, Git, Window,
Help), the Islands themes and expUI icons everywhere, IntelliJ's keymap. Tasks: main window 245, Window menu 82,
View menu 66, Help 43.

**Status (2026-11-22).** Done: the Islands themes (and Darcula, IntelliJ Light and the others), fonts, expUI and file
icons, IntelliJ's macOS keymap (259 of its 812 shortcuts reach an action); the stripes and Islands panels, the Project
tool window, editor tabs, the status bar; the main toolbar (project, VCS and run widgets, Search Everywhere, the gear)
and the main menu in IntelliJ's order; the Problems tool window (⌘6, `crates/spork_tool_windows`: its File and Project
Errors tabs over the language servers' diagnostics, the tree of files and problems with their severity icons, View
Options, Show Quick-Fixes, Open Editor Preview, Expand and Collapse All, a double click or ⏎ going to the problem) and
the Notifications tool window (the right stripe's bell: the project's notifications, Zed's and Spork's, newest first
with their times and actions under Timeline and Clear all); Remove from Sidebar and the left stripe's ⋯ (More tool
windows); the editor tab row's ⋮ (`EditorTabsEntryPoint`); the run widget's More Actions ⋮ and IntelliJ's spacing
between the toolbar's widgets; File ▸ Recent Projects listing the recent projects (rebuilding the menu bar doesn't move
the focus: a tool window closed while the window isn't active gives the focus back once it is, GPUI reporting focus
changes to active windows only); the Git menu following a merge started while the repository loads; since the editor
and files wave (2026-11-23) the Problems tool window's context menu (Show Quick-Fixes, Copy Problem Description, Jump to
Source), Group by Inspection (by the diagnostic's source), Folders Always on Top, Open Files with Single Click and Enable
Preview Tab, the tab row's ⋮ Unsplit, Unsplit All and Change Splitter Orientation while split, the status bar's
right-click widgets menu (Line:Column Number, Language Services, Line Separator, File Encoding, Indentation, Read-Only
Attribute, Hide 'the widget'); since the frame wave (2026-11-23, Decision 14) the frame is Spork's components
(`spork_workbench::shell`): the stripes, their buttons and the line before the first split tool window, the editor tab
pill, its close and pin buttons and the tab row, the status bar, the project's glow behind the main toolbar, the toolbar
and stripes faded in a background window, the islands flush with the stripes, toolbar and status bar as Spork's, the
navigation bar in Spork's status text; IntelliJ's default stripe layout (Run, Debug, Build, Find, Bookmarks, TODO,
Hierarchy and Documentation under ⋯ until they first show); no threads sidebar (its status bar toggle, keys, menu item
and Find Action entries gone); since the menus and scrollbars wave (2026-11-23, Decision 14) every menu, Zed's
`ContextMenu` with its keys and actions (the editor's, the gutter's, the Project tool window's, the tab row's, the tool
windows' ⋮ and options, dropdowns), looks as Spork's popup menus, from the metrics, shadow and shortcut writing they
share (`gpui_base::menu_style`), dropdowns open a quarter rem below their trigger as Spork's popovers, and every
scrollbar (panels' and lists', the editor's with its markers kept, the terminal's) is Spork's own scrollbar
(`gpui_base::Scrollbar`: thin, over the content, shown while scrolling, wider under the pointer, faded after 2 s) in
Spork's colours. Left: External Libraries (phase 10), the Problems tool window's Vulnerable Dependencies
and Qodana tabs; the Notifications tool window's Suggestions and per-notification menu (its unread highlight, Clear All
Notifications and Mark All Notifications as Read came with the second UI wave, the stripe's unread dot with the
balloons wave); Bookmark Open Tabs… (no Zed action yet); the ⋯ popup's Move to Right Side; a removed tool
window's place, and which tool windows have shown, kept across restarts; the status bar's right widgets in Spork's
status text (13 px, muted: they are Zed's crates' labels at 11 px); one implementation, Spork's and the IDE's, of the
islands (`frame::island` paints the corners over, `shell::island` clips), the tool windows' header and its buttons
(`frame::tool_window_header`, `header_button`) and the stripe's ⋯ (Spork's is a gpui-kit button); the status bar's
Memory Indicator, Git Branch, Notifications and IDE and Project Settings widgets (Spork IDE has none) and its menu on
an empty part of the bar; Spork's overscroll bounce in the IDE's panels and
the editor, and a mouse wheel's line, 24 px in the IDE's panels where Spork's lists move 21 (trackpads scroll alike).

**Known bug (2026-11-23).** Restore Default Layout run within about 3 s of opening the AI Chat tool window leaves
AI Chat open: the panel reopens its dock from work it started when shown (`crates/agent_ui`). The
`frame-tool-window-layout` scenario waits 3 s before restoring until this is fixed.

### Phase 4: search and navigation

Search Everywhere with its tabs, Find Action, Go to File/Class/Symbol, Recent Files and Locations, Switcher, File
Structure, Find in Files, back and forward, bookmarks. Tasks: Navigate menu 174, Edit menu's find part.

**Status (2026-11-22).** Done: the navigation popups (`crates/spork_navigate`: Search Everywhere and its tabs, Find
Action, Go to Class/File/Symbol/Text, Recent Files, Recently Changed Files, Recent Locations, the Switcher, File
Structure, Last/Next Edit Location, back and forward); the editor's find and replace bar as IntelliJ's (`search`'s
`find_bar.rs`: history, Cc/W/.*, "3/12" and "0 results", previous/next, Search In Selection, More's Open Results in Find
Window and multiple carets, Replace, Replace All, Exclude, Preserve case, ⌘R in the bar); Find in Files and Replace in
Files (the popup with its file mask, In Project, Module, Directory and Scope, the results and their preview, Open in
Find Window, Replace and Replace All); the Find tool window (⌘3, its results tree and left toolbar); file names in their
version control status's colour in Search Everywhere (files and symbols), Recent Files and the Switcher (Recent
Locations' titles keep the scheme's foreground, as IntelliJ's); bookmarks (F3, the ⌥F3 mnemonic chooser, ⌃digit and
⌃⇧digit, Rename Bookmark…, Remove Mnemonic, ⌘F3 and ⌥⌘F3 popups, the Bookmarks tool window ⌘2, IntelliJ's gutter icons);
Edit ▸ Find and Edit ▸ Bookmarks in IntelliJ's order. Left: the find bar's and popup's In Comments / In String Literals
filters (Zed's search has no syntax filter), Scroll to Results During Typing, the popup's pin and search history list;
the Find tool window's grouping options and settings (its exclusion and context menu came with the keyboard and
navigation wave, below); bookmark lists (Add Bookmark to Another List, Create Bookmark List…, the Breakpoints group),
file and folder bookmarks (the Bookmarks tool window's context menu and Move Up/Down came with that wave), the gutter
menu's IntelliJ texts; Find Usages and Recent Find Usages (phase 7, on LSP). Since the second UI wave (2026-11-23):
Search Everywhere's Complete Command (⇥ writes out a `/` command; `/files main` searches Files), Recent Files' and the
Switcher's files with errors on a red wavy line with Select Next/Previous Problem File (F2, ⇧F2). Left: Go to Changed
File… (the diff viewer's file chooser, the version control crates'), the Search Everywhere Component toolbar field.

### Phase 5: the editor

Gutter, folding, multi-caret, inlays, completion and parameter info popups, quick documentation, intention actions
(⌥⏎), the inspection widget, soft wraps, split editors, IntelliJ's editor actions and their keys. Tasks: Editor 248,
Edit menu 91.

**Status (2026-11-22).** Done (`crates/spork_editor`, through hooks in `editor`: `git.rs`'s and `look.rs`): IntelliJ's
New UI gutter (the line numbers a point smaller, then the icons, the folding outline and the change markers, with
`EditorGutterLayout`'s widths), the folding outline's chevrons (over the hovered gutter, and on a collapsed region) and
the folded `...` on the folded text colours, Expand/Collapse (⌘=, ⌘-), Recursively (⌥⌘=, ⌥⌘-), All (⇧⌘=, ⇧⌘-), Fold
Selection (⌘.) and Fold Code Block (⇧⌘.); the right margin at 120, the caret row and the indent guides in IntelliJ's
colours; the completion lookup (the kinds' icons, the typed letters in blue, the tail and the right-aligned type, "Press
⏎ to insert, ⇥ to replace"; ⌃Space, ⌃⇧Space, ⏎, ⇥, ⌃.) with its ⋮ (Sort by Name, Quick Documentation, Code Completion
Settings) and the documentation beside it (F1 in the lookup, or by itself after Settings › Editor › General › Code
Completion's delay; that page's sorting, suggestions as you type and insertion by `.`, `,`, `;`, `=`, `:`, `(` or a
space), the intentions of ⌥⏎ with the fixes' red and yellow bulbs, the preview of the change the language server
resolves, the › submenu (Edit intention settings, Disable '…', Settings › Editor › Intentions turning them back on) and
the 16 px light bulb, parameter info (⌘P: every signature, the parameter at the caret bold), the documentation popup
(F1: the external documentation's link, the definition's module, Jump to Source and ⋮ with Show on Mouse Move, Show
Automatically During Completion, Adjust Font Size…'s slider, Back and Forward greyed, View External Documentation) and
the problem popup's surface; the inspection widget (the counts of errors, warnings and weak warnings or the green check,
its popup, a click and its arrows to the next and previous problem) and Next/Previous Highlighted Error (F2, ⇧F2: the
highest severity first, the problem's popup after the jump); the editing keys checked against the catalogue (caret moves
and selections, Split Line ⌘⏎, Start New Line, Join Lines, Cut up to Line End, ⌃G, ⌃⇧G, Clone Caret Above/Below ⌥⌥↑/↓,
Add Carets to Ends of Selected Lines ⌥⇧G, Extend/Shrink Selection, Move Line, Move Statement on Move Line), Soft-Wrap,
Split Right/Down, Unsplit All and the splitters (⌥⇥, ⌥⇧⇥); 266 of the catalogue's 812 macOS shortcuts reach an action
(+7: Split Line, Move Statement Up/Down, Type-Matching Completion, Insert Dot, Fold Code Block, Paste as Plain Text).
Since the editor and files wave (2026-11-23, `spork_editor`'s `editing.rs`, `paste_history.rs`,
`documentation_window.rs`, `markdown.rs`): Column Selection Mode (⇧⌘8: ⇧↑ ⇧↓ grow the block, a plain drag selects a
column), Move Caret to Code Block Start/End (⌥⌘[ ⌥⌘], with selection), Page Top/Bottom (⌘⇞ ⌘⇟, with selection), Move
Caret to Matching Brace (⌃M, IntelliJ's navigation offsets), Paste from History… (⇧⌘V: the last 100 copies in IntelliJ's
Choose Content to Paste chooser), Unsplit and Change Splitter Orientation; IntelliJ's Documentation tool window (on the
right stripe once used: Open in Documentation Tool Window, Quick Documentation showing there while it shows or with Show
Documentation Popup First off, Auto-Update from Source, Back/Forward greyed, Jump to Source) and the popup's ⋮ in
IntelliJ's order (Restore Size greyed, Copy Quick Doc); IntelliJ's Markdown editing (Bold ⌘B, Italic ⌘I, Strikethrough
⇧⌘S, Code ⇧⌘C, Create Link ⇧⌘U, Generate Table Of Contents, the table rows' Move Up/Down, Select Row and Select Column
Cells) and its Editor / Editor and Preview / Preview layouts in the file's top right corner over Zed's preview (beside
the editor in the next pane, or in its place). Left: the lookup's Quick Definition and the documentation's own
navigation (links don't open in the popup or the tool window) and its Adjust Font Size… in the tool window;
inspections' submenus (Fix all, Suppress) and hiding the light bulb when every intention there is disabled; the
inspection widget's "Analyzing…", its popup's highlighting level and ⋮; the gutter's 14 px icons (run markers are phase
8's) and breakpoints over the line numbers; Expand to Level; Next/Previous Method on the name (Zed's goes to the
outline item's start); the Markdown preview in the editor's own tab (IntelliJ's TextEditorWithPreview would need a
Markdown item of its own) and Convert to Reference's Replace Duplicates; inlays. Since the second UI wave (2026-11-23,
`markdown_styling.rs`): Set Header Style, Increase/Decrease Header Level, Create Or Change List, Convert to Reference,
Insert Table with its size grid and Insert Image with its dialog (⌘U; both, with Create Link and Generate Table Of
Contents, in a Markdown file's Generate popup), the floating toolbar over a selection, the preview's Sync Scroll and
Adjust Font Size… (its right-click menu).

### Phase 6: project and files

Project view and scopes, New File/Directory/Scratch, file operations, Local History, Recent Projects and the Welcome
screen, project settings; the remote-development plumbing left in `project` is untangled. Tasks: File menu 62.

**Status (2026-11-22).** Done (`crates/spork_projects`, `crates/spork_local_history`, `project_panel`'s
`file_operations.rs` and `scratches.rs`): the Welcome window (Projects with search, New Project, Open, Clone Repository
and each project's More Actions; Customize: theme, Sync with OS, IDE font, keymap; Plugins opens the extensions page;
Learn's keymap reference), shown at launch with nothing to reopen and when the last project window closes; the Recent
Projects popup of the project widget and File ▸ Recent Projects ▸ Manage Projects…'s dialog (their search keeps every
typed key: the popup's speed search no longer takes them from the search field); Close Project, Close All Projects,
Close Other Projects; New… (⌘N), File, Directory and Scratch File (⇧⌘N) with IntelliJ's popups and checks, scratches in
the config folder under "Scratches and Consoles"; Rename… (⇧F6), Move… (F6), Copy… (F5, and Paste after Copy), Delete
with IntelliJ's confirmation, Copy Path/Reference…, Open In ▸ Finder and Terminal, the Project tool window's context
menu in IntelliJ's order; Local History (saved and external changes in the data folder, 5 days, 1 MB a file), Show
History with Revert, Put Label…; File ▸ Recent Projects lists the recent projects (phase 3). Since the editor and files wave (2026-11-23): the New… popup's HTML File, New in This Directory… (⌃⌥N), New Scratch
Buffer (`buffer1.txt` to `buffer5.txt`), the Welcome window's Copy Path (⇧⌘C). Left: External Libraries
(nothing language-agnostic knows a project's libraries: it comes with the language toolchains, phase 10), Show Project
History, Show History for Selection, Recent Changes, the Welcome window's Learn courses and custom icons, Trust
Project…, the Get from Version Control dialog's GitHub and GitLab sources, project settings and
scopes, the `project` crate's remote plumbing. Since the second UI wave (2026-11-23): recent project groups (New
Project Group, Move to Group, Edit…, a group's Open All Projects in Group and Remove from Recent Projects…), the
Welcome window's Plugins opening Settings' Plugins page and its gear's Settings…, Show Scratch Files (Recent
Locations' popup over the scratch files; deleting one from it is left).

### Phase 7: code intelligence

IntelliJ's Code and Refactor menus on LSP: Reformat, Optimize Imports, Generate, Comment, Rename, Extract, Change
Signature where the server offers it; inspections in the Problems tool window. Tasks: Code menu 94, Refactor menu 66.

**Status (2026-11-22).** Done (`crates/spork_code`, and usage tabs in `spork_navigate`'s Find tool window): IntelliJ's
code intelligence on the language servers, each item greyed where the active file's servers can't do it, each
refactoring the server's code actions of its kind as IntelliJ's own LSP support maps them (none: IntelliJ's hint, one:
it runs, several: a chooser), the files it changes saved. Code: Generate… (⌘N, IntelliJ's Generate popup over the
server's generating actions), Surround With… (⌥⌘T), Reformat Code (⌥⌘L) and the Reformat File dialog (⌥⇧⌘L: its scope,
Optimize imports and Code cleanup where the server has them), Optimize Imports (⌃⌥O), Move Statement Down/Up (⇧⌘↓/↑,
over the syntax tree), Inspect Code… (the Specify Inspection Scope dialog, then the servers' diagnostics over each file
of the scope, its results in the Problems tool window's inspection tab: "Inspections on Project 'fixture'", the
Inspection Results root, the files and their problems with their severity icons and IntelliJ's counts, Rerun, Previous
and Next Problem, Show Quick Fixes, Expand and Collapse All; IntelliJ's notification when it finds nothing). Refactor:
Refactor This… (⌃T), Rename… (⇧F6) in place with its popup and, pressed again, IntelliJ's Rename dialog (Search in
comments and strings, Search for text occurrences, Preview in the Find tool window with Refactor and Cancel), Change
Signature… (⌘F6), Extract/Introduce ▸ Introduce Variable…, Introduce Constant…, Extract Method… (⌥⌘V, ⌥⌘C, ⌥⌘M), Inline…
(⌥⌘N). Navigate: Declaration or Usages (⌘B, a declaration's usages on itself), Implementation(s) (⌥⌘B) with IntelliJ's
chooser, Type Declaration (⌃⇧B), Super Method (⌘U), Call Hierarchy (⌃⌥H) in the Hierarchy tool window (callers and
callees); Show Usages (⌥⌘F7, IntelliJ's popup: header, count, rows by file, preview, Open in Find Tool Window), Find
Usages (⌥F7) into the Find tool window, Quick Definition (⌥Space). Left: ⌘-click stays Zed's go to definition; Type
Hierarchy (greyed: Zed's lsp-types has no `typeHierarchyProvider`), Method Hierarchy, Go to Test, Related Symbol…; the
refactorings no server offers as code actions (Move, Copy, Safe Delete, Pull Members Up and the like) and Change
Signature's own dialog; the in-place rename's live update of the other usages and its options inlay, the in-place rename
of what Extract introduces; Code Cleanup…, Analyze ▸, live templates; the Specify Inspection Scope dialog's Custom
scope, Inspect injected code and inspection profile, the inspection results' grouping by inspection, View Options, Edit
Inspection Settings and Export (the language servers have no inspection profiles).

### Phase 8: run and debug

Run configurations and their dialog, the run widget, Run and Debug tool windows, breakpoints, Services. Tasks: Run
menu 410, Build 10, Tools 46.

**Status (2026-11-23).** Done (`crates/spork_run`, hooks in `editor`, `debugger_ui`, `project` and `terminal`): the run
widget's popup (the configurations by type with their icons, Run and Debug on a row, Current File, Edit Configurations…)
and Stop while something runs; Run… and Debug… (⌃⌥R, ⌃⌥D); the Run/Debug Configurations dialog over Zed's tasks (Shell
Script) and debug scenarios (a type per debug adapter): the tree by type with Add New Configuration, Remove, Copy, Move
into New Folder and Sort, each type's form (Name, Allow multiple instances, Store as project file: `.zed/` or the user's
files, the command or executable, arguments, working directory, environment variables, a debug configuration's Before
launch), Edit configuration templates…, Run or Debug, Cancel, Apply, OK; the Run tool window (⌘4) running every task of
the workspace: a tab per run (reused unless pinned, Pin Tab, Close Tab…), its console (Zed's terminal: input, ⌘-click on
file:line) opening with the command line and ending with "Process finished with exit code N" in IntelliJ's colours,
Rerun (⌘R), Stop (⌘F2), Scroll to End, Clear All; the Run menu in IntelliJ's order with its keys (⌃R, ⌃D, ⌃⌥R, ⌃⌥D,
⌘F2); the Debug tool window (⌘5) in IntelliJ's layout over Zed's debugger: a tab per session, the toolbar (Rerun, Stop,
Resume Program, Pause Program, Step Over, Step Into, Step Out, View Breakpoints…, Mute Breakpoints, More), Threads &
Variables (the thread, its frames, Hide Frames from Libraries, the evaluate field with ⏎ and ⇧⌘⏎ watches, Zed's
variables tree) and Console; Zed's execution point highlight and inline values; breakpoints: IntelliJ's gutter icons
(line, disabled, conditional, logging, verified, invalid, muted), ⌘F8, the right-click popup (Enabled, Suspend,
Condition, More, Done), the Breakpoints dialog (⇧⌘F8), Toggle Breakpoint Enabled, Mute Breakpoints. The debugger's
scenarios run `tools/e2e/dap_fixture.py`, a debug adapter playing the fixture's program: lldb-dap (Xcode's command line
tools) asks macOS for Developer Tools Access.

**Phase 8b (2026-11-23).** Done: the debugger's actions, each greyed where the adapter can't (as IntelliJ greys them):
Smart Step Into (⇧F7: the adapter's `stepInTargets` in IntelliJ's "Method to Step Into" chooser, one target stepped
into at once, none a plain Step Into), Force Step Over (⌥⇧F8) and Force Run to Cursor (⌥⌘F9) with the breakpoints
skipped until the program stops, Reset Frame (the adapter's `restartFrame`), the toolbar's More and Run ▸ Debugging
Actions in IntelliJ's order; the Evaluate dialog (⌥F8: Expression with its history of 50, "Use ⇧⌘⏎ to add to
Watches", the result tree with its children and errors in red, Evaluate, Close; it closes with the session); the
watches' toolbar beside the variables tree (New Watch…, Remove Watch, Move Watch Up/Down, Duplicate Watch), the watches
after the variables in their order, Add to Watches in the evaluate field; Attach to Process… (⌥⇧F5: IntelliJ's "Attach
To" popup of the local processes under Recent and their debugger, CodeLLDB, GDB, Debugpy or JavaScript, the speed
search); the Breakpoints dialog's exception breakpoints (a group per adapter with its `exceptionBreakpointFilters`, sent
to its sessions and kept for the next); the Services tool window (⌘8: Add Service ▸ Run Configuration…, the
configurations of the types added grouped by type or by status, View Options, Expand and Collapse All, a configuration's
Run, Debug and Stop, its run's or debug session's console on the right, a run of a listed configuration shown there);
Show Running List (IntelliJ's "Running List" balloon) and Stop Background Processes… (⇧⌘F2, the language servers'
cancellable work); the run console's file:line links on a plain click and the process's stderr in the console's error
colour (a POSIX shell runs the command with its stderr through a pipe that writes it back coloured, the exit status
kept). Left: Coverage, the profiler and Build; the form's Modify options; debugging the current file without a configuration; Suspend All or Thread (a debug adapter breakpoint
has no suspend policy), Remove once hit and dependent breakpoints (Zed's breakpoint store keeps no such properties:
they need its persisted schema extended); Evaluate's code fragment mode (Expand), Smart Step Into's in-place chooser in
the editor (the popup is IntelliJ's fallback), New Watch's inline editor (it focuses the evaluate field, ⏎ adds), a
watch's duplicate (Zed keeps one watch per expression: Duplicate makes a variable a watch); Attach to Process's dialog
(IntelliJ 2025.2's default; the popup is the one its registry key brings back), remote and Docker hosts; Services' Open
in New Tab, Group Services by Type, Restore Hidden Configurations and debug sessions in Services (they open in the
Debug tool window); the status bar's "Show all (N)" processes link (Zed's activity indicator shows one task); the
console's Soft-Wrap (a terminal always wraps at its width and has no horizontal scrolling), Print (GPUI has no
printing), Up/Down the Stack Trace, and stderr in non-POSIX shells (fish, Nushell, PowerShell): a console of its own
reading two pipes would do these, a decision for later.

**Docker in Services (2026-11-24).** Done (`crates/spork_docker`, `spork_run`'s `docker.rs`), IntelliJ's Docker
plugin (the paid IDE's, so not in the CE catalogue: its texts from IntelliJ IDEA 2026.3's bundle, its behaviour from
its documentation): the Docker Engine API, the versioned REST API, over a connection's Unix socket or TCP (HTTP/1.1 of
our own on the standard library: requests, chunked streams, hijacked connections, multiplexed frames); the connections
saved per IDE, the local daemon found as IntelliJ finds it (Docker Desktop's `~/.docker/run/docker.sock`, Colima's,
OrbStack's, Podman's machine's, `/var/run/docker.sock`) and added as "Docker" until connections are saved, again each
time Services shows; Settings › Build, Execution, Deployment › Docker (the list with Add and Remove, Name, "Connect to
Docker daemon with:" Docker for Mac, Colima, Unix socket, TCP socket with its Engine API URL, the "Connection
successful" or "Cannot connect: …" check as the form changes) and Services' Add Service ▸ Docker Connection (IntelliJ's
New Docker Connection dialog, the same form); a connection's node (double-click or Connect, Disconnect, Edit
Configuration, Delete Connections, Pull Image…, Clean Up), under it each Compose project with its services and their
containers, then Containers (the others, running and stopped, with their status), Images, Networks and Volumes, the
nodes kept up to date from the daemon's events; a container's Dashboard (Start, Restart, Stop, Terminal, its environment
variables, ports and volumes), Log (followed, stderr in the console's error colour), Files (the folders through an exec
of `ls`), Attached Console, Inspection and Terminal tabs; an image's Dashboard (Create Container, its properties, tags
and containers), Layers and Labels; a network's (properties, containers, IPAM) and a volume's (driver, mountpoint, the
containers using it) Dashboard and Inspection; the actions in the item toolbar and the context menu: Start, Restart,
Pause, Unpause, Stop and Delete Container (IntelliJ's Confirm Deletion), Show Files, Copy Image ID, Copy Container ID,
Inspect, Attach, Create Terminal ▸ As Container User or As Root User (Zed's terminal running the IDE's own binary as
the bridge to an exec's TTY, raw, its size followed), the Images group's pull console ("Image to pull:"), Delete Image,
Create Container (image, name, ports, environment variables, command, Run), Delete Network and Volume, Clean Up (the
group's prune, or the whole daemon's: containers, volumes, networks, dangling images and the build cache) and a Compose
project's Start All, Stop and Down through the `docker compose` CLI when it is installed. The scenarios run
`tools/e2e/docker_engine.py`, a Docker Engine API on a Unix socket in the test's folder, and `tools/e2e/fake_docker`'s
`docker` for Compose. Left: SSH, WSL, Minikube and Podman's own connection choices, TLS (the TCP socket's Certificates
folder), path mappings and the Tools page's executables; Exec, Show Processes, Open Project, Save as a New Image, Push
Image, Copy Docker Image, Create Network and Volume, Compose services' Scale, Start and Stop, Docker registries, dev
containers, Docker run configurations and Dockerfile and Compose file support, the View Options' Docker filters, the
Dashboard's editing, the Files tab's editing, downloads and uploads, the Docker plugin's own icons (generic platform
icons stand in).

**The debugger wave, Java (2026-11-25).** Done (`spork_java`'s `debugger.rs`, `spork_run`, hooks in `debugger_ui`;
`research/jvm-debugger.md` is the gap report it follows): Java's debugger behaves as IntelliJ's on java-debug (the
Application, Spring Boot, JUnit and Remote JVM Debug configurations; Kotlin has its own server's debugger), every
feature tested on the real java-debug (the `java-debug-*` scenarios on the fixtures' `Debuggee`, Maven's and
Gradle's), not on `dap_fixture.py`. Zed's debugger reaches java-debug through a proxy of the debug adapter protocol on
the loopback that gives it IntelliJ's behaviour where the protocol leaves java-debug free: Suspend: All (a stop pauses
the other threads, Resume Program resumes them all, Pause Program suspends the VM, a step runs the other threads),
logging breakpoints that keep their condition, with IntelliJ's "Breakpoint reached at
com.example.Debuggee.measure(Debuggee.java:44)" and "Evaluate and log" as an expression, "Breakpoint Condition
Error"'s question at each stop where a condition fails, threads, frames and values in IntelliJ's form (`"main"@1`,
`hello:9, Greeting (com.example)`, `{ArrayList@23}  size = 3`, a frame's single "Local" scope flattened). IntelliJ's
stepping filters (Step Into skips `java.*` and the others, synthetic methods); Force Step Into (⌥⇧F7) without them;
library and JDK frames hidden by default, each run folded into "N hidden frames", opened read-only from JDT LS's
class file contents (attached or decompiled source); the program in a terminal of its own as the Console tab, with its
input, its command line, "Connected to the target VM…", the debugger's own output, "Disconnected from the target VM…"
and "Process finished with exit code N"; Java Exception Breakpoints' "Any exception" with its Caught and Uncaught
notifications in the Breakpoints dialog before any session; HotSwap: IntelliJ's "Reload Changed Classes for …"
question after JDT LS builds changed classes during a session, Reload Changed Classes (Find Action), "N classes
reloaded", "Loaded classes are up to date. Nothing to reload." and "HotSwap failed"; Remote JVM Debug, a configuration
type (Debugger mode, Host, Port, the JVM's command line arguments to copy) attaching java-debug to a JVM's JDWP agent,
Stop detaching; Attach to Process listing the JVMs whose agent listens, as `com.example.Main (5005)` under Java, their
session named `localhost:5005`; the JUnit configuration type (JUnit 4's runner on the test classpath, run and
debugged); the thread chooser, the frames' popup (Reset Frame, Copy Stack, Hide Frames from Libraries) and the
variables' popup in IntelliJ's items (Set Value…, Copy Value, Copy Name, Evaluate Expression…, Add to Watches, Toggle
Field Watchpoint, Zed's write data breakpoint, untested on Java), each item java-debug can't serve greyed; Java's
inline values (a `debugger.scm`). Left (P0/P1 of
the report), after the second debugger wave's paragraph below: log + suspend together, the exit code after Stop
(java-debug's `vm.exit(0)`), thread groups and states, Smart Step Into on lambdas, lambda breakpoints, Debug for Maven
goals and Gradle tasks, "Attach debugger" in a console, JUnit 5 and TestNG and IntelliJ's test tree. java-debug
listens on every network interface without authentication (its `ServerSocket(0, 1)`): only a patched java-debug can
bind it to the loopback, a decision for the user.

**The debugger wave 2, Java (2026-11-25).** Done (`spork_run`'s `breakpoints.rs` and `smart_step_into.rs`,
`spork_java`'s `debugger.rs`, `breakpoints.rs` and `hotswap.rs`, `spork_settings`' `debugger.rs`; small marked
patches in `editor`, `project`, `workspace` and `debugger_ui`, PATCHES.md), each tested on java-debug by a
`java-debug-*` scenario: inline values for every language as IntelliJ draws them, after the line's end (`name: value`
once per variable in the line's order, `DEBUGGER_INLINED_VALUES`' italic grey, the execution line's colour and a value
changed since the last stop in `DEBUGGER_INLINED_VALUES_MODIFIED`'s); IntelliJ's breakpoint properties kept with the
breakpoint (the store, and a table of Spork IDE's own in the workspace database) and edited in the popup (its light
variant) and the Breakpoints dialog: Suspend: All or Thread (sent as the `thread` breakpoint mode, which the Java
proxy offers and serves with java-debug's own event-thread suspension), Remove once hit (Toggle Temporary Line
Breakpoint ⌥⇧⌘F8; the breakpoint a session stops at is removed), "Disable until hitting the following breakpoint"
with After hit (not sent until its master is hit in the session), pass count shown in the popup when set; method
breakpoints (Toggle Method Breakpoint: a breakpoint on the declaration line, which java-debug makes the method's
entry; Watch: Method entry, Method exit greyed, Emulated) and field watchpoints (Toggle Field Watchpoint in the editor
or on a field in the variables: java-debug's data breakpoint of `com.example.Debuggee#counter`; Field access, Field
modification), their gutter icons and the dialog's Java Method Breakpoints and Java Field Watchpoints groups, Run ▸
Toggle Breakpoint's items; Run to Cursor (⌥F9) and a click on a line number running a suspended program to it; Smart
Step Into in place (the line's calls highlighted in `DEBUGGER_SMART_STEP_INTO_*`'s colours, ←/→/⇥ to choose, ⏎ to step,
⎋ to leave, IntelliJ's hint), the popup where a call has no place; HotSwap's floating "Code changed:" toolbar in Java
editors after an edit during a session (Apply HotSwap saves, builds and reloads, then "Code has been reloaded"; Hide);
Settings › Build, Execution, Deployment › Debugger (Click line number to perform run to cursor), Data Views › Java
(java-debug's views), HotSwap (Build project before reloading classes, Suggest HotSwap in the editor, Reload classes
after compilation: Always, Never, Ask) and Stepping (Skip synthetic methods, Skip constructors, Do not step into the
classes with IntelliJ's list, Resume only the current thread), applied to JDT LS and the running sessions as they
change. Left: method exit (java-debug has none; a fork's decision), dependent and temporary method breakpoints (the
stop is on the method's first line, not its declaration's), the Make Default suspend policy, adding and removing
stepping filter patterns (the list only turns them on and off), the HotSwap page's radio buttons under their label (IntelliJ's sit on its row), Smart Step Into's mouse choice and lambda targets (java-debug lists none), the HotSwap
toolbar's not-hot-swappable state ("Restart Debug"), live refresh of the variables after a Data Views change (the
next stop shows it), the exit code after Stop: java-debug 0.53.2 (its newest, only in VS Code's extension, not on Maven
Central) destroys the process it launched itself, but a program in Spork IDE's console is attached through the JDI
listening connector (no `vm.process()`), so it still calls `vm.exit(0)`: not upgraded.

**The debugger wave 3, Java and Kotlin (2026-11-26).** Done (`spork_run`'s `breakpoints.rs`, `class_chooser.rs`,
`threads.rs`, `debug_window.rs` and `evaluate_dialog.rs`; `spork_java`'s `debugger.rs`, `values.rs`, `thread_dump.rs`
and `hotswap.rs`; `spork_kotlin`'s `breakpoints.rs`; `spork_settings`' Stepping and HotSwap pages; small marked patches in
`debugger_ui` and `project`, PATCHES.md), each tested on java-debug or the Kotlin server by a `java-debug-*` or
`kotlin-*` scenario: Java exception breakpoints of a class (the Breakpoints dialog's + ▸ Java Exception Breakpoints, Java
Method Breakpoints and Field Watchpoints greyed; IntelliJ's "Enter Exception Class" chooser, Search by Name over JDT LS's
classes; "Exception breakpoint for …" with Enabled, Class filters (IntelliJ's field, `-` excluding) and its Caught and
Uncaught notifications, one kept on; "Any exception breakpoint" with its class filters too; kept with the adapter's), sent
as java-debug takes them: one Caught/Uncaught pair, the classes (all of them under "Any exception") and the class
filters in its `exceptionFilters` setting, the proxy checking each exception stop against its breakpoint where the
breakpoints differ; the variables view's popup in IntelliJ's order with Inspect… ("Inspect 'name'", the value's tree, a
lazy `toString()` value opened as IntelliJ shows it), Mark Object…/Unmark Object (F3, "Select Object Label", the
`[label]` before the name wherever the object shows), Jump To Source (F4, ⌘↓: a frame's variable's declaration),
Jump To Type Source (⇧F4: the value's class from JDT LS, a library's read-only), View as ▸ Create… (greyed), Auto,
Primitive and Hex (`0xD`), Object and toString (evaluated in the value's frame), Collection, and Show types (the
values' `{Type@id}`), each applied at once; Customize Threads View… (its eight options: the frames' line, class,
package, source file and argument types, synthetic frames, the current thread first, the thread groups as
`"main"@1 in group "main": WAIT` from Java's own thread states), Get Thread Dump (More and Run ▸ Debugging Actions: a
"Dump HH:MM:SS" tab, the threads with their state, the filter, each thread's text as IntelliJ's dump writes it, Copy
to Clipboard and Export to Text File), Export Threads… (IntelliJ's Export Preview, `threads_report.txt`), Add Stepping
Filter… (the frame's class); the Evaluate dialog's code fragment mode (Expand ⇧⏎ or ⌥M, "Code fragment:", statements
evaluated with ⌘⏎ — java-debug's evaluation runs them —, Collapse, the mode kept); Settings › Stepping's + ▸ Add
Pattern… ("New Filter") and − on the selected pattern, the HotSwap page's radios on their label's row, the suspend
policy's Make Default (new breakpoints suspend their thread), HotSwap's Restart Debug when the change isn't
hot-swappable (a class's shape changed or the file doesn't parse, as `JvmBaseSourceFileChangeCompatibilityChecker`
finds it), Rerun and Restart Debug starting a Java or Kotlin session again (their adapters can't restart one in
place), "Reload Changed Classes" asked only after a project's Java file changed (JDT LS also builds a library's source
opened from a frame), the values taken again at once after a Data Views change; the Debug tool window's Layout Settings (Threads &
Frames ▸ Combined, Threads, Side-by-Side, Show Frames Only, Don't Show; Variables; Console; Separate Watches with the
watches' own view; Show Tab Labels; Restore Default Layout; kept); Kotlin: Toggle Method Breakpoint on the function
around the caret (the server's function breakpoint) and "Kotlin Exception Breakpoints › Any exception" before any
session (the server's caught and uncaught filters). Left: Kotlin's exception breakpoints of a class and class filters
(the server's debugger takes only its two filters, on IntelliJ's own "Any exception"), Kotlin's thread dump (the server
has a `threadDump` request, not wired); a thread dump's locks (JDI's monitor information isn't reachable through
java-debug), its sort, merge and group toggles; the class chooser's Project tab and Throwables found by name only (JDT
LS's type hierarchy would find the others), Add Class… in the stepping filters, the exception breakpoint per-class
notifications of a subclass where two breakpoints' differ (java-debug stops at it as its superclass's); Mark Object's
colour chooser and `label_DebugLabel` in expressions, a marked value without its `{Type@id}` (Show types off) loses its
label; Jump To Source on a field inside a value's tree; View as's Map/List renderers and Java Type Renderers; Show
types and the layout kept while the IDE runs only for Show types; the dump's virtual threads; Export Preview's
Overwrite/Append question; IntelliJ's Memory and Overhead views in the layout menu; exception breakpoints kept for
every project (Zed keeps an adapter's filters so), where IntelliJ keeps them per project.

### Phase 9: settings

The Settings dialog over Zed's settings (one store), Keymap, Plugins, Appearance; every page from the extended
catalogue. Tasks: Settings 193.

**Status (2026-11-22).** Done: IntelliJ's Settings dialog (`crates/spork_settings`), opened by ⌘,, the app menu and
the toolbar gear: the search over the tree of groups (pages found by name or by an option, the options found outlined
on the page), breadcrumbs with back and forward, Cancel / Apply / OK with IntelliJ's semantics (changes wait for Apply,
a changed page in the link colour, Esc empties the search then cancels); pages over Zed's settings, each option the
`settings.json` key it writes (Appearance with the themes, Sync with OS, the zoom and the interface font; System
Settings; Editor › General, Appearance, Editor Tabs, Font, Color Scheme, Code Style, Other File Types; Debugger ›
Data Views; Markdown; Tools › Terminal; Advanced Settings); Version Control › Git, Spork's page; the Keymap page over
Zed's keymap (the keymap picked, the actions in IntelliJ's groups with the menu bar's texts, search by name and by
shortcut, Add Keyboard Shortcut with IntelliJ's dialog and its conflicts, Remove, into the user keymap on Apply); the
Plugins page over the extension registry (Marketplace and Installed, cards, details, Install and Uninstall; its test
uses a local registry, `tools/e2e/extensions_http.py`). Left: IntelliJ's pages and options Spork IDE can't back with
a setting yet (most of Editor's, Build Tools, the languages' pages), disabling a plugin (extensions can't be), keymap
copies and mouse shortcuts, plugin logos and ratings (the registry has none), IntelliJ's IDE zoom by steps of 10 %.
Phase 9b (2026-11-23) triaged all 206 of IntelliJ IDEA CE's pages (`docs/spork-settings-pages.md`: 1 fully backed,
36 partly, 169 left out) and rebuilt the partly backed ones with IntelliJ's options in its order, groups, labels and
controls, the rest left out: Editor › General and its Appearance, Breadcrumbs, Code Completion, Code Folding, Editor
Tabs, Smart Keys and Sticky Lines, Code Editing, Font (Typography Settings), Color Scheme, Code Style, Appearance,
System Settings, Terminal, Markdown, Data Views, Advanced Settings (Local History's retention, a setting of
`spork_local_history` now); Spork's Version Control pages join Git's (Commit fully matches the reference app's). Left: the
pages IntelliJ fills from engines Spork IDE lacks (colour scheme editing, inspections, live templates, scopes, file
colours, HTTP proxy, trusted locations), the preview of Font, the options of partly backed pages. Since
the second UI wave (2026-11-23): Copy Settings Path (the tree's, breadcrumbs' and options' right-click menu), File ▸
Manage IDE Settings' Export Settings… and Import Settings…, the Plugins page's gear with Install Plugin from Disk,
Check Suggested Plugins, Tools › Actions on Save's Reformat code, Editor › File Types (its patterns and ignored
files), and Find Action's ⌥⏎ Add Shortcut… (the Keyboard Shortcut dialog written at once). Left: Smooth Scroll Options (Zed has no smooth scrolling), Maven Settings (no Build
Tools page), Copy Settings Link (no `spork-ide://` settings links), the Plugins gear's other items. Since 2026-11-24 the
window is IntelliJ IDEA 2026.3's (`docs/spork-settings-pages.md`): "Settings – project", 900×700, its title after
the traffic lights, the search over the tree in 2026.3's order (Accessibility a page of its own), the project's mark
on a project's pages and breadcrumbs, the count of plugin updates on Plugins, back/forward and the pin (Float mode,
a floating window; Window mode at first, as GPUI's floating window hides with the IDE), the "?" and Cancel / Apply /
OK, measured on the user's screenshot; Appearance as 2026.3's with its UI Options backed (`ui_options` in the
settings, which the project's glow and both menus follow; the full path in the window's title; smaller tree indents;
the preferred themes Sync with OS picks) and Editor › Font's preview. Left: Compact mode and the tool window options
(the frame's metrics), Smooth scrolling, the editor colour scheme apart from the theme, 2026.3's unified Plugins
list (Installed, Bundled, Suggested in one search), the pin kept across restarts.

### Phase 10: Java and the JVM build tools

IntelliJ's Java, Maven and Gradle surfaces (the Maven and Gradle tool windows, project import, run configurations,
the Java actions in Code, Refactor and Build) on Eclipse's engines: the JDT language server with m2e and Buildship, and
Eclipse's Java debugger over the debug adapter protocol (Decision 11). Tasks: the 366 the catalogue lists under the Java,
Kotlin, Groovy, Maven and Gradle plugins.

**Status (2026-11-22): first slice.** Done (`crates/spork_java`, hooks in `spork_run` and `project_panel`): Java's
language (tree-sitter-java, Zed's Java extension's queries) on Eclipse's JDT LS, downloaded into the data folder with
Eclipse's SHA-256, its release following the newest JDK found (1.43.0 on Java 17, 1.62.0 on 21, the newest on 25),
"Project JDK is not defined" without one; diagnostics, completion and the LSP menus phase 7 builds; the import:
IntelliJ's "Maven/Gradle 'x' build scripts found" with Load and Skip, JDT LS importing through m2e or Buildship with its
progress in the status bar, a project imported before loaded again; the Project tool window's source, resource and test
roots and compact packages; the Maven and Gradle tool windows on the right stripe (the project's tree: Lifecycle,
Plugins and Dependencies, Gradle's Tasks by group and Dependencies by configuration; Sync All, Generate Sources and
Update Folders, Run Maven Build, Execute Maven Goal / Gradle Task as Run Anything, Toggle Offline Mode, Toggle 'Skip
Tests' Mode, Expand and Collapse All; a double click running a phase, goal or task with the project's `mvnw` or
`gradlew` in the Run tool window, Execute greyed without one); the Application run configuration (VM options, main
class, program arguments, working directory, environment) run after JDT LS's build on its runtime classpath, and
debugged through Eclipse's java-debug (fetched like the server); the gutter's run icon on `main`. Left: Maven's plugins
and dependencies from the POM alone (no parent POM, effective model or transitive dependencies), Download Sources,
Add/Link and Unlink, Analyze Dependencies, the Build Tools settings, the tool windows' context menus and Run
Configurations node, the Java banner as an editor banner with Setup SDK, External Libraries, module classpath (one
module), the Java actions of Code, Refactor and Build beyond phase 7's, Kotlin and Groovy.

**Phase 10b (2026-11-23).** Done: IntelliJ's Build menu (`Java.BuildMenu`) on JDT LS's build: Build Project (⌘F9),
Build Module, Rebuild Project, and Rebuild (⇧⌘F9) by its key; the Build tool window (bottom, after Debug): the build's
tree (the project's line with its status, time and duration, the files with the compiler's errors and warnings and their
messages, a double click opening the place), the selected line's text on the right (a message with its place, the
build's summary in IntelliJ's words), Rerun and Stop, its Build Output tab; every document saved first. The Java
Generate items (⌘N) on JDT LS's source actions, each where the class has something to generate: Constructor (Choose
Super Class Constructor, Choose Fields to Initialize by Constructor), Getter, Setter, Getter and Setter, equals() and
hashCode() (after IntelliJ's question when they exist), toString(), Override Methods… (⌃O) and Implement Methods… (⌃I)
by declaring class, Delegate Methods… (target, then methods), in IntelliJ's member chooser (the members under their
class, ⌘- and ⇧-click, Select None, Cancel, OK), the code written as IntelliJ writes it (the editor's indentation, no
Eclipse TODO comment, equals() on `java.util.Objects`); the three in the Code menu. New ▸ Java Class in a source folder
(IntelliJ's New Java Class popup: Class, Interface, Record, Enum, Annotation, Exception, IntelliJ's templates,
`sub.Name` making the package, its name checked), package-info.java and module-info.java. The Maven tool window's
menus: Sync/Reload All Maven Projects and Download Sources and/or Documentation as the toolbar's popups, the project's
menu (Sync Project, Generate Sources and Update Folders, Open or Create 'settings.xml', the downloads, Run Maven
Build), a phase's or goal's (Run Maven Build, Execute Before/After Build and Rebuild, the goals run around the IDE's
builds into the Build tool window's output and marked on their node, kept in the data folder), a dependency's
downloads (Maven's dependency plugin, then JDT LS reloads), the profiles' check boxes with Toggle Profile and Reset
Profiles (`-P` on Maven's runs). Shell scripts' Generate: For Loop, While Loop, Until Loop. The Hierarchy tool
window's row menu: Base on This Method (⌃⌥H in the tree) and Jump to Source. Left: the "Project JDK is
not defined" banner, Setup SDK and Add JDK from Disk… (this Mac always has a JDK, and Add JDK needs the system's file
dialog: neither can be shown in a test yet), External Libraries in the Project tool window, the Application
configuration's Modify options, Recompile, Build Artifacts…, the member chooser's toolbar and options and equals()'s
wizard pages, toString()'s templates, Execute Before Run/Debug… and Assign Shortcut…, Maven's run configurations node,
the tool windows' view options (Show Basic Phases Only and the like, in the header's ⋮ IntelliJ fills), Show Effective
POM and Analyze Dependencies, Gradle's daemons and its context menus, Generate Javadoc…, JShell Console….

**Phase 10c, the Java and HTML wave (2026-11-23).** Done: External Libraries in the Project tool window (after the
project's folders: the project's JDK `< 17 >` and the jars of its test classpath as JDT LS resolves it, named `Maven:
junit:junit:4.13.2` or `Gradle: …`, each with its jar; an invisible worktree on a folder of links in the data folder,
which `project_panel`'s folder roles show and name); the "Project JDK is not defined" editor banner (IntelliJ's warning
panel) with Setup SDK's popup (Download JDK… greyed, Add JDK from Disk… through the system's folder dialog, the JDKs
found), the JDK chosen kept as the project's and JDT LS run on it, `SPORK_IDE_JDKS` naming the JDK homes to use instead
of searching (a scenario's `env` sets it empty); the Application configuration's Modify options (`spork_run`'s
configuration type hook: Allow multiple instances, Do not build before run, Add VM options, the rest greyed); Recompile
(⇧⌘F9 in a Java editor: the file marked changed for JDT LS's incremental build, in the Build tool window); the Maven
tool window's ⋮ (Group Modules, Show Ignored Projects, Show Basic Phases Only, Always Show ArtifactId, Show version) and
the Gradle one's (Group Modules, Group Tasks, Display Tasks Inherited from Sub-Projects, Show Ignored); Gradle's context
menus (the project's Open Gradle Config, Sync Gradle Project, Refresh Gradle Dependencies; a task's Run 'fixture [task]'
and Execute Before/After Sync, Build and Rebuild; a dependency's Analyze Dependencies… greyed). HTML on tree-sitter
(`spork_code`: tree-sitter-html with Zed's HTML extension's queries, so HTML files are HTML without the extension): the
table's cell navigation, Add a New Column to the Table Before/After the Current One in the Generate popup, Surround with
Tag as IntelliJ's `T` template. Left: External Libraries' JDK shows its home folder where IntelliJ shows its modules,
and a jar doesn't open into its packages; Setup SDK's Download JDK… (a JDK source to decide); Modify options' Redirect
input, Modify classpath, Shorten command line, the "provided" scope, Logs, Code Coverage and Before Launch fragments;
Recompile in the Build menu; Unlink, Ignore, Tasks Activation, Composite Build Configuration and Assign Shortcut… in
Gradle's menus; Show Gradle Daemons, Migrate Project to Daemon JVM Criteria, Generate Javadoc…, JShell Console…,
Maven's Execute Before Run/Debug…, Assign Shortcut… and Run Configurations node; HTML's closing tags and tag rename
(the HTML language server's), Surround with Tag in a floating code toolbar.

**Phase 10d, the JDK wave (2026-11-25).** Done (`spork_java`): Download JDK… as IntelliJ's dialog (Version, Vendor
with the version's vendors by name and the others' nearest release under Other Versions, Location in IntelliJ's
folder, `~/Library/Java/JavaVirtualMachines/<vendor>-<version>` on macOS and `~/.jdks` elsewhere, the archive's size,
IntelliJ's checks) on the Foojay Disco API, each archive's checksum verified (the Disco API's, its checksum file's,
Corretto's and SapMachine's own) before it is unpacked, the archive deleted once unpacked (the end-to-end tests keep
theirs, `SPORK_IDE_KEEP_JDK_ARCHIVES`), as a background task in the status bar that its × cancels;
offered in the banner's Setup SDK (now enabled), Project Structure's Add SDK ▸ Download JDK… and SDKs page, and the
Application configuration's JRE field; `jdk_download::download_jdk` is the same install without the dialog. The SDK
table (`jdk.table.json`) and IntelliJ's names for JDKs (`zulu-17`, from the `release` file's vendor). File › Project
Structure… (⌘;): Project (Name, SDK with Edit, Language level with IntelliJ's full list and descriptions, Compiler
output), Modules (the module's Language level on Sources, Module SDK on Dependencies), Platform Settings › SDKs (add,
remove, rename, the JDK home path). The project's JDK (or its module's) runs JDT LS, which gets every JDK as
`java.configuration.runtimes`, the Application configurations and the Maven and Gradle runs (`JAVA_HOME`, Gradle's
JVM); `jdk::for_project` is the project JDK for the other engines. A Maven or Gradle project's language level is
written to its build file as it sets one (`maven.compiler.release`, `source`/`target`, the compiler plugin's
configuration; the Gradle toolchain or `sourceCompatibility`; IntelliJ's compiler plugin configuration or a `java`
block when it sets none), then synced; a plain folder's goes to JDT LS's compiler options; JDT LS's "only available
with source level N" error gets IntelliJ's "Set language level to N - …" quick fix. Settings › Build, Execution,
Deployment › Compiler › Kotlin Compiler with IntelliJ's fields, a Kotlin build's language version, API version, JVM
target and Kotlin version written to its build file (`compilerOptions`, `kotlin.compiler.*` or the Kotlin plugin's
configuration). Left: Project Structure's Libraries, Facets, Artifacts, Global Libraries and Problems, a module's
Paths tab and content roots, several modules, an SDK's Classpath, Sourcepath, Annotations and Documentation Paths;
"SDK default" for a build file leaves its level as written, a preview level writes no `--enable-preview`, the
LTS levels aren't bold; Early-Access and JavaFX JDKs, IntelliJ's own runtime, JDK update notifications; the quick
fix sits after the intentions in the popup; the Kotlin Compiler's other options are kept for the project but not
written to the build.

**Phase 10e, Kotlin (2026-11-25).** Kotlin's engine is JetBrains' Kotlin language server (`Kotlin/kotlin-lsp`,
IntelliJ's own Kotlin analysis and JVM debugger run headless, the "Kotlin language server" in the UI): under
JetBrains' licence, so never bundled; Spork IDE downloads its newest release from JetBrains onto the user's machine
(`languages/Kotlin language server`, checked against its SHA-256, the older builds removed) and, as each build expires
about a month after its release, sets an expired one aside with a balloon and fetches the newest; it runs without its
statistics (`env -u INTELLIJ_DATA_SHARING …`, `--data-sharing=none`). Done (`crates/spork_kotlin`, hooks in
`spork_java`, `spork_run`, `project` and `dap`): Kotlin's language (fwcd's tree-sitter-kotlin, Zed's Kotlin
extension's queries); Kotlin detected in the Load flow (a build that declares it, or `.kt` files) and the server
started on Load for Gradle (Kotlin DSL and Groovy) and Maven, Kotlin-only and mixed with Java, its import and indexing
in the status bar; diagnostics, completion (Java's members from Kotlin too) and the LSP menus phase 7 builds;
`src/main/kotlin` and `src/test/kotlin` as source and test roots; the gutter's run icon on `main` (top level, an
`object`'s or a companion's) run by the project's tool, IntelliJ's "Build and run using: Gradle" (a `MainKt.main()`
`JavaExec` task an init script adds) or Maven (`compile`, then `java` on the runtime classpath through the exec
plugin), its popup's Debug through a debug locator; the Kotlin run configuration (Main
class, VM options, Program arguments, Working directory, Environment variables) in the Run/Debug Configurations
dialog, run in the Run tool window and debugged in the Debug tool window by the server's debug adapter: breakpoints in
top-level functions, inlined lambdas, classes and the Java classes of a mixed project, stepping, Kotlin expressions in
watches and evaluation, frames as IntelliJ writes them (`greet:13, MainKt (com.example)`), Stop; New ▸ Kotlin
Class/File (IntelliJ's popup, kinds, templates and name checks). Wave 2 (2026-11-25): the form's "Use classpath of
module:" (the build's modules as IntelliJ names them, `fixture.app.main`), "JRE:" (the Application's) and "Shorten
command line:" (none, JAR manifest, @argfile, as IntelliJ's init script shortens Gradle's `JavaExec`; Maven's exec
plugin's manifest jar), a configuration running in its module's Gradle project and source set (a subproject's main in
the subproject); Debug from the gutter's run icon (its popup IntelliJ's Run 'MainKt' and Debug 'MainKt'); Kotlin
function breakpoints (the line that declares a function, IntelliJ's method breakpoint icon, the server's function
breakpoints, stopping as the function starts; the debugger's method breakpoints, in the Breakpoints dialog's Kotlin
Function Breakpoints) and lambda breakpoints (IntelliJ's Set Breakpoint popup: Line, each lambda, Line and Lambda(s), the
server's breakpoints at the lambdas' columns), both kept across restarts with the breakpoints' other properties; coroutine stacks as frames under
IntelliJ's "Async stack trace" separators (the server's label frames, which Zed's protocol types refused); "Kotlin is
not configured" over a Kotlin file of a build without Kotlin, with Ignore and Configure ("Configure Kotlin with
Gradle"/"Maven", writing the build as IntelliJ's configurators do); the `build.gradle.kts` task gutter (`tasks.register`,
`by tasks.registering`, run as `fixture [hello]`); the server started on a Kotlin Gradle script in a Kotlin project
without Kotlin files. Left: property watchpoints (the server's debugger has no data breakpoints), the Coroutines view
(the debugger exposes no coroutine dump), Convert Java to Kotlin and the Kotlin Generate items (the server offers neither
a command nor a code action for them, 271.2085.0), the variants' highlight in the editor and IntelliJ
2026.3's inline breakpoint markers (its default over the popup), Configure's version list (IntelliJ reads
Maven Central's; 2.4.10 only) and its Single module choice, the Gradle task gutter's Debug, test gutters and runs,
Refactor ▸ Extract/Introduce's Kotlin items, Kotlin Bytecode, scratch files and scripts, Languages & Frameworks ›
Kotlin; a Maven or Groovy Gradle project without Kotlin files isn't analysed until one is opened.

**Spring Boot, the Spring wave (2026-11-25).** IntelliJ IDEA 2026.3's Spring Boot (its paid edition's plugin) on Spring
Tools' Spring Boot language server (EPL-1.0, the `vscode-spring-boot` 2.4.1 vsix from Open VSX, SHA-256 checked,
downloaded once a Spring Boot project opens), bridged to JDT LS as VS Code runs it (its bundles in JDT LS, its `sts/*`
requests answered by JDT LS's commands, JDT LS's classpath callbacks run by it). Done (`crates/spork_java`'s
`spring_boot.rs`, `spring_boot_ls.rs`, `actuator.rs`; hooks in `spork_run` and `spork_settings`):
a Spring Boot project found when it opens (its build file names Spring Boot), both servers run on a JDK 21 or newer (Temurin's,
through `jdk_download::download_jdk` into the data folder's `jdks`, when the machine has none, never offered as a
project's JDK; the project still builds and runs on its own);
on each import a **Spring Boot** run configuration for each application without one (a class annotated
`@SpringBootApplication` or `@EnableAutoConfiguration` whose `main` starts Spring), named after its class and selected,
unless Settings › Languages & Frameworks › Spring › Spring Boot › Project Import › "Create run configuration
automatically" is off; the Spring Boot type (Add New Configuration, its form with the Spring group of Modify options:
Active profiles, Enable debug output, Hide banner, Disable launch optimization, Disable JMX endpoints, Override
configuration properties as IntelliJ's table; IntelliJ's VM options, ANSI colours, the JMX endpoints, a
`spork.boot.process` marker; its checks), Run and Debug through java-debug, the gutter's run on a Spring Boot `main`
making (and selecting) its Spring Boot configuration; Services shows Spring Boot configurations by default under
"Spring Boot", a starting application's node until `Started … in` and then its port's link (`:8080/`), Edit Default
Path ("Configure Default Path"); the Actuator tab beside the console in Services (Beans with Bean definition, Injected
into and Depends on, Health, Mappings, Environment, Refresh, "Updated at", the library filters; "Spring Boot Actuator
is not enabled." with its link adding the starter and "Restart application", "Mapping endpoint is not enabled."),
read as IntelliJ reads it, attaching to the application's JVM and calling its endpoint MBeans over JMX;
`application*.properties` and `application*.yml` as languages of their own (Properties on tree-sitter-properties)
with the Spring Boot configuration file icon, completion, quick documentation, validation in IntelliJ's messages
and go to declaration from the Spring LS. Left (the second Spring wave below did Update Running Application, New
Project › Spring Boot, the gutters, Endpoints and the `jdt://` declarations): the Beans tool window, JMX authentication and
the Scheduled debugger, Show/Hide Values, the Actuator tab in the Run and Debug tool windows (it is in Services, where
Spring Boot runs show), the YAML and Properties names IntelliJ shows for the two languages.

**The debugger wave (2026-11-25).** IntelliJ's auto-link: a project opened with one build tool's scripts (Maven's
or Gradle's) is linked and loaded without a notification; with both, each has its "build scripts found" notification
with Load and Skip (`java-build-scripts-found`). Java's debugger as IntelliJ's: phase 8's debugger wave paragraph.

**The fixes before alpha.2 (2026-11-25).** java-debug's DAP server listens on the loopback interface only (its
`JavaDebugServer` patched from source in `crates/spork_java/java-debug-patch`, put into the official jar at install,
`PATCHES.md`); the Spring Boot language server's configuration file problems are warnings, as IntelliJ's two
inspections report them; `application*.properties` and `.yml` are Spring Boot's languages in a Spring Boot project only,
plain Properties and YAML with no Spring server elsewhere (`spring-boot-not-spring`); the Spring servers' JDK is
downloaded into the data folder's `jdks`, never offered as a project's JDK; a downloaded JDK's archive is deleted once
unpacked; `spring-boot-actuator-states` finds the Actuator tab before clicking it (a window's first accessibility
lookup comes back empty). Left: the JDWP socket java-debug listens on while it launches a program to debug is on
every interface until the program connects, a few seconds (Eclipse JDI's `SocketTransportService`, in JDT LS).

**Spring Boot, the second Spring wave (2026-11-25).** Done: run configurations are the workspace's as IntelliJ's
(`workspace.xml`): one made on import, from the gutter or with Add New Configuration has "Store as project file" off
and is kept in the IDE's files of the project (the data folder's `workspace/<folder>-<hash>`), ticking the box moves it
into `.zed/tasks.json` or `.zed/debug.json` and unticking moves it back, for every type (`run-configurations-store`;
the user's global `tasks.json` and `debug.json` are no longer read as configurations). Update Running Application (⌘F10,
Run ▸ Debugging Actions) on a running configuration whose type updates it, a choice of them ("Select Process to
Update") when several do; the Spring Boot configuration's On 'Update' action and On frame deactivation (Modify options
▸ Spring, a combo row each) with IntelliJ's policies: Update resources and Update classes and resources build the
project (a debug session then reloads the classes as the build's HotSwap asks), Update trigger file writes
`.restartTriggerFile` into the module's class folders for DevTools, Hot swap classes and update trigger file if failed
builds, reloads the classes in a debug session and writes the trigger file when it isn't debugged or HotSwap fails; the
trigger file policies need DevTools and add `-Dspring.devtools.restart.trigger-file` (`spring-boot-update`, a run's
class changed and restarted by DevTools). New Project ▸ Spring Boot (`initializr.rs`, a generator `spork_projects`'
New Project lists beside Empty Project): IntelliJ's two pages on start.spring.io's metadata API (Server URL, Name,
Location, Create Git repository, Language, Type, Group, Artifact, Package name, JDK, Java, Packaging, Configuration;
then Spring Boot and the dependencies' tree with its search and Added dependencies), Create downloading `starter.zip`,
unpacking it and opening it, which links and imports it and makes its Spring Boot configuration
(`spring-boot-new-project`, on start.spring.io's answers recorded in `tools/e2e/fixtures/initializr`); IntelliJ's
segmented button in `spork_workbench`. The Endpoints tool window (right, IntelliJ's Web icon standing in): the HTTP
Server endpoints of the Spring LS's request mappings under the module, Show From Tests, Group by Module, the selected
one's details, a double click going to its handler; the request mapping's gutter globe with Show all endpoints of
module and Open in web browser; the bean icon on a `@Component` class or `@Bean` method (Navigate to the autowired
dependencies) and the autowired icon on an injection point (Choose Autowired Candidate), from the Spring LS's index
(`spring/index/beans`, read again on `spring/index/updated`), in the gutter's icons area through `editor`'s
`LineMarkers` hook (`spring-boot-endpoints`, `spring-boot-beans`). Go to Declaration into a library's class (`jdt://`
sources, JDT LS's `classFileContentsSupport` on): the definition's class written read-only from JDT LS's
`java/classFileContents` and opened, as a debugger frame's (`project`'s `NonFileUriResolver`;
`spring-boot-library-declaration`). A project outside git whose sources are deeper than the worktree's first scan
starts JDT LS too. Left: IntelliJ's variant tags for the update policies (Spork's forms have no tags; the policies are
combo rows), the Run tool window's Update button and the menu item's "Update 'X' application" text, Update resources
copying the resources alone; the Initializr's Server URL dialog and Retry, "Requires Spring Boot version", the
dependency links (Guide, Reference), Reformat code; Endpoints' Compact List Items, the HTTP Client and other endpoint
types, Go to Endpoint and Search Everywhere's tab, the Examples tab; Open in HTTP Client and Navigate to related views;
the Beans tool window; IntelliJ's own Endpoints and bean icons (theirs are the paid edition's).

**Spring Boot, the third Spring wave (2026-11-26).** Done: Update finished: the Update button ("Update 'X'
application", ⌘F10) after Rerun on a running Spring Boot application whose configuration has an On 'Update' action, in
the Services item toolbar, the Run tool window and the Debug tool window, and Run ▸ Debugging Actions' item reading
"Update 'X' application" with one such application running ("Update Running Application…" otherwise); Update resources
copies the changed resources into the class folder, no build (`spring-boot-update-resources`). The Initializr: the ⚙
beside the Server URL opens IntelliJ's "Server URL" dialog (OK for a URL only), the URL set is kept for the next
wizard, Retry after a failed connection, a dependency the chosen Spring Boot can't take greyed with "Requires Spring
Boot version >=4.1.0 and <4.2.0-M1" and Create refused with "Selected dependencies: … are not available for version
…", the dependency's links under its description (its guides, Reference, Web site; templated URLs left out as
IntelliJ's panel leaves them), Kotlin and Gradle Kotlin DSL projects created and imported (their applications found in
their Kotlin sources, `DemoApplicationKt`), Settings › Spring › Spring Initializr › "Reformat code when creating a new
project" reformatting the new project's Java sources with JDT LS (`spring-boot-initializr`, `spring-boot-new-project`,
`spring-boot-settings`). Endpoints: Search Everywhere's Endpoints tab (after Actions) and Navigate ▸ Endpoint… (⌘⇧\),
Compact List Items (the endpoints otherwise on two lines, their type and framework under them), the side panel's HTTP
Client, Documentation and Examples tabs (the request with Submit Request and Open in HTTP Client; the cURL example), the
request mapping's Open in HTTP Client and Navigate to related views (a `@Controller` handler's template)
(`spring-boot-endpoints-navigation`, `spring-boot-endpoints`). The HTTP Client, first slice (`crates/spork_http`):
`.http` and `.rest` files as the HTTP Request language (tree-sitter-http's grammar, highlighted), method and header
completion, each request's run icon in the gutter, the editor's toolbar (Run All Requests in File, "Run with:" and the
`http-client.env.json` and `http-client.private.env.json` environments, `{{variables}}` and `@name = value`), the runs
in Services' "HTTP Request" node with IntelliJ's console text (the request, status line, headers, body, "Response file
saved." and "Response code: …; Time: …; Content length: …"), each response saved and linked under its request
(`<> 2026-11-26T013102.200.txt`), New ▸ HTTP Request, a scratch HTTP Request, Open in HTTP Client's
`generated-requests.http` (`spring-boot-http-client`). The Beans tool window (2026.3 has no Spring tool window; Beans
took its place): the Spring LS's beans by module, their stereotype under them unless Compact List Items, Show From
Tests, the side panel's Documentation, a double click going to the definition (`spring-boot-beans-tool-window`). Left:
IntelliJ's own Beans and Endpoints icons (paid; the Dependencies and Web tool windows' stand in); the Initializr's
error as a dialog (it shows in the wizard), the Kotlin project's Spring LS (it needs JDT LS's classpath) and
reformatting Kotlin sources; Endpoints' search field and its Type/Framework/Module filters, the Examples tab's other
generators (Retrofit) and options, the HTTP Client tab's embedded editor and response; the HTTP Client's response
handler scripts, `< file` bodies, dynamic variables (`{{$uuid}}`), the response view's folded headers, View As and
the response diff, cookies, Convert from cURL, the HTTP Request run configuration type, ⌘/ in `.http` files (the
editor would continue `# ` after a `###` separator), the ⌘N New popup's HTTP Request; the Beans tool window's search,
diagram and its Spring options (Show Implicit Beans, Show XML Infrastructure Beans, Change Active Spring Profiles…).

**The test wave (2026-11-25).** IntelliJ's test running for Java and Kotlin. Done (`spork_java`'s `testing.rs` and
`test-runner/`, `spork_run`'s `test_runner.rs`, Kotlin's in `spork_kotlin`; hooks in `editor` and `project_panel`): the
gutter's run icons on JUnit 4, JUnit 5 and TestNG test classes and methods in Java and Kotlin (IntelliJ's `gutter/run`
icon, also on `main` now), their popup (Run '…' ⌃⇧R, Debug '…' ⌃⇧D, Modify Run Configuration…) and, after a run, each
test's and class's last state (passed, failed, error); the JUnit configuration as IntelliJ's form (Test kind: All in
package, Pattern, Class, Method, with the fields each takes) and a TestNG type; a run from the gutter, the Project tool
window's Run/Debug 'Tests in 'com.example'' (or a test file's, or the test root's 'All Tests') and the test tree's menu
makes a temporary configuration (kept for the project, the last five, its icon translucent, selected), which the run
widget's Save Configuration keeps (private until "Store as project file"); tests run as IntelliJ runs them: a Gradle
project's by Gradle (`:cleanTest :test --tests …` with an init script whose test listener reports each test), a Maven
project's on Spork IDE's own test runner (`spork.tests.TestRunner`, JUnit 4, the JUnit Platform with its launcher of the
engine's version from Maven Central when the project lacks it, TestNG) after JDT LS's build on its test classpath, or
for Kotlin after Maven's `test-compile` on the exec plugin's; the events (IntelliJ's service messages) reach the Run
tool window on a port of the loopback interface. The Run tool window's test runner: the tree (Test Results, the classes,
their tests, state icons, durations), the status line ("1 test failed, 1 passed, 1 ignored  3 tests total, 14 ms", as
2026.3's `TestStatusLine` writes it), the toolbar (Rerun, Rerun Failed Tests, Stop, Show Passed, Show Ignored, the
Sorting Options popup, ⋮ with Expand/Collapse All, Previous/Next Failed Test, Export Test Results… and Test Runner
Settings), the options kept from run to run, the root's console and a node's own output and failure with its stack
trace's links to the project's sources and a comparison failure's "Click to see difference" (Zed's file diff of Expected
and Actual), Jump to Source on a double click; Debug from the gutter: Java's test on java-debug, Kotlin's by Gradle's
`--debug-jvm` (or the runner's JDWP agent on Maven) with the Kotlin server's debugger attached. Left (round 2 below
did the Debug tool window's tree, ⌃⇧R/⌃⇧D, the test history, Import Tests from File, auto-test, Exclude from Suite,
Scroll To and Open Source at Exception, the multi-project test task, JUnit 5 on Kotlin + Maven, the fork mode, the
other kinds and a Gradle project's debugged Java test): the test tree in
the Debug tool window (a debugged test reports no tree), Run with Coverage and the profiler, ⌃⇧R/⌃⇧D on the caret's
test, Recent Tests, test history and Import Tests from File, auto-test, Exclude from Suite and the Add to Suite actions,
the export dialog (XML into the project's folder only), IntelliJ's diff window for a comparison (Zed's file diff stands
in), Scroll To and Open Source at Exception, a multi-project Gradle build's test task (the root's runs), JUnit 5 on
Kotlin + Maven (the exec plugin's classpath lacks the launcher), the fork mode, Category/Tags/UniqueId/All in directory
kinds, TestNG's Suite and Group kinds, a debugged Java test in a Gradle project runs on the test runner rather than
Gradle.

**The coverage wave (2026-11-26).** IntelliJ's code coverage for the JVM on IntelliJ's own coverage agent
(`intellij-coverage-agent` 1.0.777, the version 2026.3 bundles, Apache-2.0, from Maven Central with its SHA-256
checked, downloaded the first time a run gathers coverage, never bundled): it counts each line's hits and each
condition's true and false hits and tracks each test, where JaCoCo (IntelliJ 2026.3's default runner) gives covered or
not and no per-test data. Done (`crates/spork_coverage`, the engine in `spork_java`'s `coverage.rs` and
`coverage-helper/`; hooks in `spork_run`, `editor`, `project_panel` and `workspace`'s frame): Run with Coverage for
the Application, JUnit, TestNG, Kotlin and Spring Boot configurations (the Run menu's Run with Coverage, the run
widget's ⋮ "Run 'X' with Coverage", the gutter popup's on a test and on `main`, the Project tool window's "Run 'Tests in
'com.example'' with Coverage"), the agent as IntelliJ passes it (its arguments file, exact hits, the configuration's
package as the default pattern, unloaded classes, `*Generated*` left out): `-javaagent` on the `java` command line and
the test runner, on the Maven exec plugin's JVM, on Gradle's test tasks through the test init script and on the Kotlin
`JavaExec` task; a run with coverage stops softly (SIGINT, a second Stop kills) so the JVM writes its data; the data
read by Spork IDE's helper on the project's JDK. The editor: each line marked in IntelliJ's left free painters area (8
px between the line numbers and the icons, `LINE_FULL_COVERAGE`, `LINE_PARTIAL_COVERAGE`, `LINE_NONE_COVERAGE` of
Islands Dark and Light), marks following the edits and a line edited losing its mark; a click shows IntelliJ's popup
(Previous/Next Coverage Mark with the next line's status, Show Tests Covering Line, Hide coverage, "Hits: 3" with each
condition's true and false hits and each switch's cases, or "Covered x/y branches"). The Coverage tool window (right, a
side tool window, added when coverage first shows and opened as "Activate Coverage View" says): its tab (closing it
hides the coverage), Flatten Packages, Navigate with Single Click, Always Select Opened Element, Generate Coverage
Report…, Filters (Hide Fully Covered Classes), the tree by package (chains collapsed) and class with Class, Method,
Line and Branch, % as IntelliJ counts them, sorting by a column, ⏎ and double click going to the class. The Project
tool window's "50% classes, 55% lines covered" and "x% methods, y% lines covered". Run ▸ Manage Coverage Reports… (⌥⌘F6,
"Choose Coverage Suite to Display": the suites with their time, Add, Remove, Delete, No Coverage, Show Selected),
Generate Coverage Report… (IntelliJ's dialog, the HTML report of the agent's reporter into the folder, opened in the
browser) and Hide Coverage; the Code Coverage question when a run's coverage comes in while other coverage shows
(Replace Active Suites, Add to Active Suites, Do Not Apply Collected Coverage, Don't ask again); Settings › Build,
Execution, Deployment › Coverage (when new coverage is gathered, Activate Coverage View, Show coverage in the project
view, Branch coverage, Track per test coverage, Collect coverage in test folders, Exclude annotations); Modify options ▸
Code Coverage ▸ Coverage settings (Specify classes and packages, Exclude classes and packages, their pattern tables);
per-test tracking on the test runner (each test's start and end told to the agent as IntelliJ's listener does).
Scenarios: `coverage-junit-maven`, `coverage-gradle`, `coverage-application`, `coverage-kotlin`, `coverage-spring-boot`,
`coverage-options`. Left: JaCoCo and the runner choice (IntelliJ's default; its name can't show either), Import
External Coverage Report…, the Filters' Show Only Classes with Uncommitted Changes, the suites' runner node in
Manage Coverage Reports, Edit Coverage Colors and Show Bytecode in the popup, the "unique test" gutter icon, a file
changed since the run shows no marks (IntelliJ maps its lines through Local History) and no "Coverage data outdated"
banner, the class and package choosers of Add Class…/Add Package… (a pattern is typed), per-test tracking on Gradle
(IntelliJ has none either), the options are the IDE's rather than the project's.

**The test wave, round 2 (2026-11-26).** Done (`spork_run`'s `test_runner.rs`, `run_widget.rs`, `run_panel.rs`,
`debug_window.rs`, `spork_java`'s `testing.rs` and `test-runner/`, `spork_kotlin`'s test Debug; hooks in `editor`,
`project_panel` and `zed`'s menus): Run and Debug Context Configuration (⌃⇧R, ⌃⇧D; Linux and Windows Ctrl+Shift+F10):
the caret's test method, test class or `main` (the innermost runnable whose declaration holds the caret, as IntelliJ's
`ConfigurationContext`), or the Project tool window's file or folder (a test class, `Tests in 'com.example'`, `All
Tests`, `Main.main()` for a Java `main`), made temporary and selected, the configuration of that name run when one
exists; the run widget's Current File runs and debugs the file's outermost runnable (debugging the current file
without a configuration); Run ▸ Run, Debug and Run with Coverage named after what they run (`Run 'GreetingTest'`, `Run
'GreetingTest.java'`, IntelliJ's greyed "The file in the editor is not runnable") (`java-test-context`). A debugged
test runs as IntelliJ debugs one: its run (by Gradle in a Gradle project, Java's too now, or the test runner) with its
JVM waiting on a free port of the loopback interface, then java-debug or the Kotlin server's debugger attached; the
run is hidden from the Run tool window and its tree, toolbar and consoles are the debug session's Console tab, Stop
ends the test JVM, the tree's Rerun and Rerun Failed Tests debug again (`java-test-debug`, `kotlin-test-debug`,
`java-test-gradle-multi`). A multi-project Gradle build's tests run in their project's task (`:app:cleanTest
:app:test --tests …`, every project's for a pattern), the test tasks of projects without the selected tests run none
(`java-test-gradle-multi`). JUnit 5 on Kotlin + Maven: the runner adds the JUnit Platform's launcher of the engine's
version to the exec plugin's classpath (from the local repository beside the engine, downloaded from Maven Central
and its SHA-1 checked when missing) (`kotlin-test-maven-junit5`). JUnit's All in directory, Category, UniqueId and
Tags kinds, TestNG's Group and Suite, with their fields in the form (Directory:, Category:, UniqueId:, Tag expression:,
Group:, Suite:) and IntelliJ's missing-value messages; Pattern as IntelliJ's `||`-separated set, with Exclude from
Suite in the test tree's menu; JUnit's Modify options ▸ Tests ▸ Fork mode (None, Method, Class: each test method or
class in a JVM of its own); on Gradle Tags, Category and Group by the frameworks' own filters (`java-test-kinds`). The
test runner's Test Runner Settings as IntelliJ's (Track Running Test, Show Inline Statistics, Scroll To ▸ Stack
Trace/Top/Bottom, Open Source at Exception, Navigate with Single Click, Set Auto-Test Delay ▸ 1s–10s, Select First
Failed Test When Finished, Include Non-Started Tests in Rerun Failed), each changing what it says; the test history
(each finished run's results as IntelliJ's XML in the IDE's files of the project, the last ten), the toolbar's Test
History popup opening one in a tab of its own, Import Tests from File… (IntelliJ's exported XML too); Rerun
Automatically (the run again after a save, once the delay passes) (`java-test-runner-options`). Left: Recent Tests
(⌃⇧;), Add to JUnit Pattern Suite and Exclude from Suite on a method's pattern, Show Auto-Test Status in the Editor,
the Scroll to Running Test button, the test history in Run ▸ Import Test Results; the Project tool window's context
for a Kotlin `main` file and in a subproject's source roots; a JUnit configuration of a Kotlin project debugged from
the run widget goes to java-debug (the gutter's and ⌃⇧D use the Kotlin server's); Gradle runs neither UniqueId nor
Suite kinds and ignores the fork mode; a forked JVM of the Kotlin + Maven runner lacks the added launcher; ⌥⌘R in the
Debug tool window reruns the last task (Zed's `task::Rerun` in `Workspace && !Terminal` outranks the keymap's
Resume Program there); the gutter's popup says `Run 'GreetingTest.greets'` where IntelliJ's says `Run 'greets()'`.

### Next (the user's plan, 2026-11-23)

1. **Spork's look everywhere**: the frame (stripes, toolbar and its glow, tab bar, status bar), menus and popups,
   scrollbars and scrolling, toasts as IntelliJ's balloons, icons and their brightness: one set of components shared
   by Spork and Spork IDE (Decision 14).
   **Status (2026-11-25).** Done: the frame, menus and scrollbars, and every notification as IntelliJ's balloon, each
   one implementation in `spork_workbench`/`gpui_base` that both apps use (the frame, menus and scrollbars, and
   balloons waves; their status lines are in phase 3). Left: Zed's `ContextMenu` code under Spork's look (101 callers),
   overscroll bounce and the mouse wheel's line height, the threads sidebar crate (unreachable, three of its tests
   ignored).
2. **Services with Docker**, connected the way JetBrains IDEs do; **the Settings dialog** as IntelliJ IDEA 2026.3's.
   **Status (2026-11-25).** Done: Docker in Services on the Engine API (`spork_docker`), connections in Services and
   Settings › Build, Execution, Deployment › Docker; the Settings window as 2026.3's (tree, banner, back/forward, pin,
   the Appearance page's options backed). Left: Docker over SSH, WSL, Minikube and TLS, exec, push, registries,
   Docker run configurations; Compact mode, smooth scrolling, the Plugins page's single list, an IDE Notifications page.
3. **Zed's plugins**: every Zed extension works in Spork IDE, with a plugin layer of our own on top of it.
   **Status (2026-11-25, the plugins wave).** Done: every kind of Zed extension installs from the Plugins page,
   loads, updates and uninstalls, tested end to end with fixture extensions built from `crates/extension_api`
   (`tools/e2e/fixtures/extensions`, served by the test registry): languages with WebAssembly grammars, language
   servers downloaded and run (`download_file`, `process:exec`), themes (applied as installed and listed in
   Appearance, as IntelliJ's theme plugins; they reach the editor and the frame's Zed-drawn parts, not Spork's
   components), icon themes (applied as installed, as IntelliJ's icon packs: the Project tool window's and tabs'
   icons), snippets in the completion (now gone with their extension: an engine fix), context servers for AI Chat's
   agent, debug adapters as Run/Debug Configurations types debugging in the Debug tool window; IntelliJ's Update on
   the Plugins page; checked by hand with four real extensions (Git Firefly, HTML, JetBrains Themes, TOML with taplo).
   Left: debug locators load but aren't checked end to end (they answer a language's runnables, which the fixture language has none of), `npm:install` end to end (needs
   npm's registry), slash commands and docs indexing (removed upstream), extension language model providers
   (unreleased upstream). Install Plugin from Disk stays the dev-extension flow (a folder, compiled as Zed does;
   installing again rebuilds it). The layer of our own is a proposal waiting for the user's decisions:
   `docs/plugin-layer.md`.
4. **The user's testing round**: the user tries everything against IntelliJ IDEA 2026.3; each difference or bug found
   becomes a fix and an end-to-end test.
   Noticed before it: the editor's right-click menu lists Zed's items, not IntelliJ's; tool-window shortcuts show
   their AZERTY key equivalents (⌘& for ⌘1) where IntelliJ may show the digits.

### Then

The rest of the catalogue (821 tasks under "everything else"), module 2 (the database grid, starting from
`tabular_data_preview`) and AI Assistant on `spork_ai`. Collaboration comes back on a server of our own, with its
sign-in (Decision 2): phase 1 removed Zed's account sign-in and hid the collaboration panel.
`docs/spork-task-triage.md` sorts the untested tasks of the Navigate menu, Build, Tools and "everything else" (1051)
into buildable now, needing an engine or a decision first, and out of scope; the first buildable ones are built (Go to
Line:Column, Select In…, Jump to Navigation Bar, File Path, Next/Previous Occurrence, Go to Test, Related Symbol…,
Jump to Last Tool Window, Tool Window Layouts ▸ Default and Restore Current Layout); the editor and files wave
built the TODO tool window (`spork_tool_windows`' `todo.rs`: IntelliJ's default patterns in comments over the project
search, Project and Current File tabs, Group By Modules, Packages and Flatten View, Preview) and the triage's editor,
documentation, Markdown, files, Welcome, Problems and status bar items listed in phases 3, 5 and 6.

**The keyboard and navigation wave (2026-11-23).** Done: IntelliJ's tree and list keys as one model the IDE's trees
and lists share (`spork_workbench::tree_keys`: `TreeAction`'s moves, ↑↓ round the ends, ← collapsing or going to the
parent, → expanding or going to the first child, Home/End and ⌘Home/⌘End, ⌥↑/⌥↓, Page Up/Down by the visible rows, the
next and previous sibling, ⇧ extending from the anchor, ⌘A, Expand/Collapse All ⌘=/⌘-, Scroll to Center ⌃L, and speed
search: typing finds the first matching row, its match highlighted, ↑↓ Home End among the matches, ⎋ ends it, ⌘F shows
it), in the Bookmarks, Find and Hierarchy tool windows, the Bookmarks popups, Recent Files and the Switcher (←→
between its lists, ⇧ selections, Delete from Recent Files ⌫ ⌦ ⌘⌫), File Structure, Recent Locations, Show Usages and
the caret's list popups (Refactor This, Generate, Select In, File Path); the Project tool window's ← selects the
parent; context menus' ↑↓→←⏎⎋ checked; bookmarks: Toggle and Go to Bookmark A to Z, the Bookmarks tool window's
context menu (Mark List as Default, Rename Bookmark List… with Use as default list, Delete Bookmark List asking first,
Rename Bookmark… F2, Delete Bookmark ⌫, Remove/Assign/Change Mnemonic… ⌥F3, Move Up/Down ⌥⌘↑/↓, Sort Bookmarks by Type
& Name, Open All Bookmarked Files ⌘↓), IntelliJ's empty text once no bookmark is left, Next/Previous Line Bookmark in
Editor; Search Everywhere's Next/Prev Group Element (⇟ ⌘↓, ⇞ ⌘↑); Window ▸ Editor Tabs' actions checked (Go to Tab 1–9
and the Last Tab, Select Next/Previous Tab, Pin Active Tab, the Close actions with Close All but Pinned and Close All
Tabs closing pinned tabs, as IntelliJ's) and Window ▸ Active Tool Window ▸ Resize (Stretch to Left/Right/Top/Bottom,
⌃⌥ arrows); the Find tool window's Exclude ⌫ (struck through, skipped by Next/Previous Occurrence), Include ⇧⌫, Remove
⌘X, Rerun ⌘R, Jump to Source ⌘↓, Open in Right Split ⇧⏎ and their context menu, and "Occurrence N of M" after the
status bar's path. Left: tables (no table view yet: Table-*, the column width actions), the numpad's tree keys (GPUI
doesn't tell the numpad's + - * from the main keys), ScrollPane-*, the other views' adoption of the shared keys (the
Problems, Services, Maven and Gradle tool windows, the git client's trees), speed search where a popup filters as
typed, bookmark lists beyond the project's one (Zed's store has one list: Create Bookmark List…, Add to Another List),
file and folder bookmarks, the Bookmarks tool window's gear options, tool window view modes (pinned, unpinned,
docked), IDE zoom by steps of 10 %, the Find tool window's grouping options, Recent Find Usages and Export to Text
File, Next and Previous Project Window (macOS doesn't let the tests' windows become key).

**The version control, run and terminal wave (2026-11-23).** Done: in every diff, Next and Previous Difference (F7,
⇧F7; Spork's diff bar names them so too), and in the IDE's diff tabs, which now give the diff the focus, Close Diff
Editor (⎋); the Commit tool window's Options ▸ Show on Double-Click ▸ Diff or Source (kept in Spork's settings), and
Jump to Source and a diff line's double-click opening the file in the IDE's editor rather than an external one; the run
widget's popup as IntelliJ's `RunToolbarPopup` (Pinned, Recent Configurations and the All Configurations toggle once
there are five, kept per project; a row's submenu with Run, Debug, Edit…, Delete… and its confirmation, Pin or Unpin;
the current file's Run with Parameters…), Send EOF (⌘D) in the Run tool window, Edit Breakpoint (⇧⌘F8 on a
breakpoint's line, its popup's ⇧⌘F8 opening the Breakpoints dialog), Quick Evaluate Expression (⌥⌘F8, IntelliJ's value
hint under the expression, its children as it opens); the Terminal tool window's IntelliJ header over Zed's terminal
(`terminal_view`'s `tool_window.rs`): the sessions as tabs named Local, Local (2)…, New Tab (⌘T and +), a tab's menu
(Rename Session in IntelliJ's "Session name:" popup, Close Tab, Close All Tabs, Close Other Tabs, Split Right, Split
Down), New Predefined Session (the shells found, bash, zsh, fish or pwsh, each a session named after it; `project`
creates a terminal with a given shell), Find Action's Increase, Decrease and Reset Font Size in Terminal; Services' Group
Services by Type (⌃T, the configurations under a Run Configurations node). Left: version control's Update, Compare with
Specified Revision…, Open Blank Diff Window, Refresh and Force Refresh (⌘R, ⌥⇧⌘R) in its views, Configure GPG Agent and
the code review comments' navigation; Debug on the current file without a configuration (Zed's debug locators say no
scenario for a plain file yet) and the form's Modify options; Services' leftovers (Open in New Tab, Open Each in New
Tab, Open Each Type in New Tab, Open in Separate Tool Window, Show Services Tree, Jump to Services, Always Select
Active Service, Configure Services Tool Window, its item toolbar and popup) and Reload Changed Classes; the terminal's
New Predefined Session's Settings item, Move to Editor, the tab's Split and Move and Unsplit, the ⋮'s Engine and
Settings.

**The balloons wave (2026-11-24).** Done: every notification of Spork and Spork IDE is IntelliJ's New UI balloon,
one component (`spork_workbench::balloon`): its 370 px body, 6 px corners, the type icon, a bold title over text that
wraps at 240 px and collapses to two to four lines behind a chevron, action links 16 px apart folding into "More ▾",
the ⋮ and close button under the pointer (⌥-click closes them all), 10 s before it fades in 200 ms (a sticky balloon
5 minutes), waiting while hovered; three in the frame's corner, 10 px from the stripe and the status bar, newest
farthest, the older ones counted on a "N more notifications" stub that opens the Notifications tool window. In the IDE
Zed's `MessageNotification`, `LanguageServerPrompt`, error notifications and status toasts draw it (the toast layer and
Zed's toast views deleted), Spork's views' notifications join the workspace's balloons, Window ▸ Notifications has
Close First and Close All (greyed while none shows), ⎋ leaves the balloons alone, showing the Notifications tool window closes the balloons and its
stripe button has IntelliJ's unread dot (red for an error). Spork shows them as its Notifications settings page says
(Display balloon notifications, Notification location, a group's popup type and Show in tool window; its errors are
"VCS important messages", sticky). Left: the IDE's Notifications settings page (the Settings dialog has none yet); Play
sound, Read aloud and system notifications; Don't Show Again (the IDE's balloons have Don't Show Again For This Project,
Spork's Settings… only); a tool window balloon pointing at its stripe button; the time starting only once the app is
active and pausing in the background; the agent's OS-level popup when it finishes (`agent_ui`'s `AgentNotification`,
IntelliJ's system notification) and the collaboration call popups (no collaboration until our server, Decision 2).

## How the work is done

The same way as the git client's parity waves: the tasks of a phase are split into parts, each part goes to an agent
in its own worktree, and the parts merge with every check and the whole end-to-end suite passing. Lessons that carry
over: unique action and method names across the app, no duplicate helpers, one shared set of test steps, an id in a
scenario's `covers` only when it fully matches.

## Risks

- **Upstream drift.** Zed moves fast; the frame (phase 3) rewrites crates Zed keeps changing. Merging often and keeping
  engine changes small is the mitigation.
- **`project` is tied to remote development and Zed's `client`.** Removing those takes untangling, scheduled for
  phase 6 instead of phase 1.
- **Two component libraries for a while.** gpui-kit (Spork) and Zed's `ui` live side by side until Spork's views move
  to `ui`.
- **Scope.** IntelliJ's feature set is huge; the catalogue keeps it countable, and each phase ships something usable.
