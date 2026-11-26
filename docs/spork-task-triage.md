# The untested tasks, triaged

Every task of four task pages (`reference/idea/tasks`: the Navigate menu, Build, Tools and Everything else) in one of
three classes, as of the triage (2026-11-23):

- **(a) buildable now** on what Spork IDE has (Zed's engines, the language servers with JDT LS, Spork's version
  control, the phases' crates), grouped by the area and crate it lives in; a task already built is still (a) until a
  scenario ticks it;
- **(b) needs an engine or a decision first**, with the reason;
- **(c) out of scope**: a plugin or product area the ROADMAP's decisions leave out (notebooks and database tools,
  which are module 2; Kotlin; JetBrains' accounts and services; IntelliJ's internal tools).

Each line: the task's id, its name, its class (and its tests, when a scenario covers it).

Kotlin got its engine on 2026-11-25 (ROADMAP, phase 10e: the Kotlin language server and its debugger): Kotlin
Class/File is built; the other Kotlin tasks below still say (c) until they are triaged again. Built since without a task
of their own (wave 2, Kotlin): the Kotlin configuration's module, JRE and Shorten command line
(kotlin-run-configuration-module), Debug from the gutter (kotlin-gutter-debug), Kotlin function breakpoints and lambda
breakpoints with the Set Breakpoint popup (kotlin-breakpoint-variants), coroutine stacks with IntelliJ's "Async stack
trace" (kotlin-coroutine-frames), "Kotlin is not configured" with Configure (kotlin-not-configured), the
`build.gradle.kts` task gutter (kotlin-gradle-task-gutter), the server in a Kotlin project without Kotlin files
(kotlin-server-without-kotlin-files). The Generate items (`KotlinGenerate*`) and Convert Java to Kotlin are (b): the
Kotlin language server (271.2085.0) offers neither a command nor a code action for them.

Built by the coverage wave (2026-11-26), on IntelliJ's own coverage agent: Run with Coverage, the editor's coverage marks and popup, the Coverage tool window, Manage Coverage Reports…, Generate Coverage Report…, Hide Coverage, the Code Coverage question, Settings › Build, Execution, Deployment › Coverage and the configurations' Code Coverage options (`coverage-*` scenarios; their tasks are on the Run and Settings pages, not triaged here).

Built by the third debugger wave (2026-11-26), its tasks on the Run page (11-run, outside these four pages): Java
exception breakpoints of a class with IntelliJ's class chooser, Caught/Uncaught and class filters
(java-debug-exception-class); the variables' Inspect…, Mark Object…, Jump To Source, Jump To Type Source, View as and
Show types (java-debug-values: `XDebugger.JumpToTypeSource`, `Debugger.ShowTypes`, `XDebugger.Inspect`); Customize
Threads View…, Get Thread Dump, Export Threads… and Add Stepping Filter… (java-debug-thread-dump:
`Debugger.CustomizeThreadsView`, `Debugger.AddSteppingFilter`, `ExportThreads`; `DumpThreads` not ticked, a dump has no
locks); Evaluate's code fragment mode (java-debug-evaluate-fragment); the stepping filters' Add Pattern… and Remove, Make
Default, the HotSwap page's row of radios and the values refreshed after a Data Views change (java-debug-leftovers);
HotSwap's Restart Debug (java-debug-hotswap-restart); the Debug tool window's Layout Settings (java-debug-layout:
`Runner.RestoreLayout`, `Runner.ToggleTabLabels`, `XDebugger.SeparateWatches`); Kotlin's Toggle Method Breakpoint and
"Any exception" (kotlin-debug-method-exception). Kotlin's exception breakpoints of a class stay (b): the Kotlin server's
debugger takes only its caught and uncaught filters.

Built by the second Spring wave (2026-11-25), beside the tasks below it marks: run configurations kept in the workspace
until Store as project file, Update Running Application (Run menu), New Project ▸ Spring Boot, the Spring gutter
icons and Go to Declaration into a library's class.

Built by the third Spring wave (2026-11-26), without tasks of their own in the CE catalogue: the Update button and
"Update 'X' application" (spring-boot-update-resources), the Initializr's Server URL dialog, Retry, "Requires Spring
Boot version", dependency links and Kotlin projects (spring-boot-initializr), Search Everywhere's Endpoints tab and
Navigate ▸ Endpoint…, Endpoints' Compact List Items and side panel tabs, Navigate to related views
(spring-boot-endpoints-navigation), the HTTP Client's first slice (spring-boot-http-client) and the Beans tool window
(spring-boot-beans-tool-window).

Built without a task of its own (2026-11-23): every menu and every scrollbar of the IDE is Spork's (its popup menus'
look, its scrollbar), checked by the `popup-menu-look` and `scrollbar-behaviour` scenarios.

## Summary

| Page | Tasks | (a) buildable now | (b) needs an engine or a decision | (c) out of scope |
| --- | --- | --- | --- | --- |
| [05-navigate.md](../reference/idea/tasks/05-navigate.md) | 174 | 156 | 17 | 1 |
| [12-build.md](../reference/idea/tasks/12-build.md) | 10 | 7 | 3 | 0 |
| [13-tools.md](../reference/idea/tasks/13-tools.md) | 46 | 11 | 27 | 8 |
| [16-other.md](../reference/idea/tasks/16-other.md) | 821 | 369 | 194 | 258 |
| **All** | 1051 | 543 | 241 | 267 |

The main reasons in (b), by how many tasks they hold:

| Reason | Tasks |
| --- | --- |
| our own collaboration server first (Decision 2: Zed's collaboration comes back on it) | 24 |
| a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off | 19 |
| a Groovy engine (Decision 11 leaves Groovy and its language server to phase 10) | 14 |
| a task server engine (YouTrack, Jira, GitHub issues): a decision | 13 |
| a decision on version control beyond git (Spork's engine is git's; Mercurial isn't decided) | 10 |
| remote servers and deployment (Services' Clouds): no engine | 9 |
| IntelliJ's module model (modules, Project Structure, `.iml`): Spork IDE opens folders, JDT LS sees one project per build file | 9 |
| the debugger's memory view: java-debug's adapter reports no instance counts | 9 |
| a test runner engine (built in the test wave, 2026-11-25: the tasks left are buildable now) | 8 |
| a syntax filter in Zed's search (in comments, in string literals): phase 4 left it | 8 |
| an XML engine (schemas, DTD, validation): Spork IDE has no XML language server yet; a decision (Eclipse's LemMinX would fit Decision 11's way) | 7 |
| Method Hierarchy: no language server request gives it (LSP has call and type hierarchies only) | 6 |
| Type Hierarchy: Zed's `lsp-types` has no `typeHierarchyProvider` (phase 7), an engine change in `lsp` | 6 |
| a test framework engine: JDT LS offers no test generation (JUnit's set-up, tear-down and test methods) | 5 |
| inspection profiles: the language servers have none (phase 7) | 5 |
| floating and windowed tool windows: Zed's docks can't leave the project window | 5 |
| an Emmet engine: Zed's Emmet extension is a completion server, IntelliJ's edit points, preview and surround need Emmet itself | 4 |
| Pandoc, an external converter: a decision to depend on it | 4 |
| a decision on version control beyond git (Subversion isn't decided) | 4 |
| Java resource bundles and i18n: no engine (JDT LS has none) | 4 |
| no JDT LS action generates it | 4 |
| the Learn courses' content (phase 6 left the Welcome window's Learn): a decision | 4 |
| usage kinds: LSP references have no read, write or import kind (only document highlights tell read from write) | 4 |
| a decision on version control beyond git: the action is Subversion's and Perforce's (git has no such operation) | 3 |
| a Gradle build script model (Buildship gives JDT LS the classpath, not the DSL) | 3 |
| Maven's resolved model (effective POM, transitive dependencies): phase 10 reads the POM alone | 3 |
| IntelliJ's non-modal welcome screen (2025.3+, the reference app's): a decision whether Spork IDE's Welcome window becomes it | 3 |
| custom folding regions (`// region`): Zed's folds come from syntax and indentation | 2 |
| JDT LS builds the workspace, not one file | 1 |
| a Linux desktop entry: Linux only, a decision on Spork IDE's Linux packaging | 2 |
| Maven repository indices: no engine indexes the local repository | 2 |
| a decision on Java frameworks beyond the JDK (JavaFX, FXML) | 2 |
| the documentation popup's own navigation: the language servers' hover gives web links, not symbols to open in the popup | 2 |
| code vision (IntelliJ's usages and author inlays): Zed draws the servers' code lenses as code actions only | 2 |
| multi-editor files (IntelliJ's editors with tabs at the bottom, such as GUI forms): none in Spork IDE | 2 |
| External Tools: a decision whether Zed's tasks back them | 1 |
| IntelliJ's artifacts (Project Structure): no engine | 1 |
| a decision on version control beyond git (Perforce isn't decided) | 1 |
| IntelliJ's own Extract Method (its duplicates): the language servers' extract finds none | 1 |
| Java property files' resource bundles: no engine | 1 |
| IntelliJ's own code style properties (`ij_*`) for .editorconfig: no engine | 1 |
| find in the Markdown preview: Zed's preview isn't searchable (no `SearchableItem`) | 1 |
| a per-method exclusion list for parameter hints: Zed's inlay hints have none | 1 |
| parameter name hints during completion: Zed's inlay hints don't follow the lookup | 1 |
| Got It tooltips (IntelliJ's feature hints): Spork IDE shows none yet | 1 |
| a JDK download source: decided, the Foojay Disco API (built in the JDK wave) | 0 |
| WSL, on Windows only: a decision on Windows support | 1 |
| a coverage engine (built in the coverage wave, 2026-11-26: the task left is buildable now) | 1 |
| colour scheme editing (Settings › Editor › Color Scheme's options): phase 9b left it | 1 |
| Zed's minimap has no side setting | 1 |
| editor windows: Zed's panes can't open an item in a window of its own within the same project | 1 |
| IntelliJ's plugin permissions: Zed's extensions have capabilities in settings, not Allow Always decisions; a decision | 1 |
| the collapsed main menu is Linux's and Windows' (macOS has the system menu bar): a decision on those systems | 1 |
| knowing generated code: no language server marks it | 1 |
| split tool windows: Zed's docks show one panel at a time, IntelliJ's split mode needs two per side | 1 |
| live templates: Zed's snippets have no surround (selected text) variable | 1 |
| an overwrite mode: Zed's editor has none (only Vim's replace mode) | 1 |
| incoming changes: Subversion's and Mercurial's, not git's | 1 |
| the JSON schema in use: Zed's JSON language server picks it and tells no client | 1 |
| Power Save Mode: Zed has no switch for its background work | 1 |
| rendered doc comments: Zed's editor has no rendered view of comments | 1 |
| sliding (undocked) tool windows: Zed's docks take their space, IntelliJ's undocked ones draw over the editor | 1 |
| a decision on what Use Single Tool Window Bar does in Spork IDE's frame (`EnableStripeGroup`) | 1 |
| a built-in web server and browser preview: no engine | 1 |

## [05-navigate.md](../reference/idea/tasks/05-navigate.md)

### (a) Buildable now (156)

#### Navigation popups and the navigation bar — `spork_navigate` (15)

- `tool-window:Structure` Structure — (a)
- `dialog:GotoLine` Go to Line:Column… — (a)
- `dialog:SelectIn` Select In… — (a)
- `dialog:GotoRelated` Related Symbol… — (a)
- `action:Back` Back — (a) · tested: navigate-back-forward (pass)
- `action:Forward` Forward — (a) · tested: navigate-back-forward (pass)
- `action:GotoLine` Go to Line:Column… — (a) · tested: intellij-editor-keys (fail)
- `action:JumpToLastChange` Last Edit Location — (a) · tested: navigate-back-forward (pass)
- `action:JumpToNextChange` Next Edit Location — (a) · tested: navigate-back-forward (pass)
- `action:SelectIn` Select In… — (a)
- `action:ShowNavBar` Jump to Navigation Bar — (a)
- `action:GotoTest` Go to Test — (a)
- `action:GotoRelated` Related Symbol… — (a)
- `action:FileStructurePopup` File Structure — (a) · tested: file-structure (fail)
- `action:ShowFilePath` File Path — (a)

#### Search Everywhere, the Switcher and Recent Files — `spork_navigate` (10)

- `dialog:GotoClass` Go to Class… — (a) · tested: search-everywhere-code (fail)
- `dialog:GotoFile` Go to File… — (a) · tested: search-everywhere-files (fail)
- `dialog:GotoSymbol` Go to Symbol… — (a) · tested: search-everywhere-code (fail)
- `dialog:TextSearchAction` Text… — (a) · tested: search-everywhere-text (fail)
- `action:SearchEverywhere` Search Everywhere — (a) · tested: search-everywhere (fail)
- `action:GotoClass` Go to Class… — (a) · tested: search-everywhere-code (fail)
- `action:GotoFile` Go to File… — (a) · tested: intellij-workspace-keys (fail), search-everywhere-files (fail)
- `action:GotoSymbol` Go to Symbol… — (a) · tested: search-everywhere-code (fail)
- `action:TextSearchAction` Text… — (a) · tested: search-everywhere-text (fail)
- `action:GotoChangedFile` Go to Changed File… — (a) · not built: it is the diff viewer's file chooser (`Diff.KeymapGroup`), in the version control crates

#### Find tool window and usages — `spork_navigate` (find_panel), `spork_code` (11)

- `tool-window:Find` Find — (a)
- `context-menu:FindInFiles.Results.ContextMenu` FindInFiles.Results.ContextMenu — (a)
- `context-menu:UsageView.Popup` UsageView.Popup — (a)
- `action:PreviousOccurence` Previous Occurrence — (a) · tested: navigate-next-occurrence (pass)
- `action:NextOccurence` Next Occurrence — (a) · tested: navigate-next-occurrence (pass)
- `action:UsageView.Exclude` Exclude — (a) · tested: find-tool-window-exclude (pass)
- `action:UsageView.Include` Include — (a) · tested: find-tool-window-exclude (pass)
- `action:ExportToTextFile` Export to Text File — (a)
- `action:UsageView.ShowRecentFindUsages` Recent Find Usages — (a)
- `action:UsageView.Remove` Remove — (a) · tested: find-tool-window-exclude (pass)
- `action:UsageView.Rerun` Rerun — (a) · tested: find-tool-window-exclude (pass)

#### Bookmarks — `spork_navigate` (bookmarks) (91)

Built (2026-11-23, the keyboard wave): the A–Z mnemonics, the tool window's context menu and its actions, Next/Previous
Line Bookmark in Editor (a bookmark's place in the list rides in Zed's label, as its mnemonic does). Left in (a): the
gear options (Group Line Bookmarks by File, Navigate with Single Click, Always Select Opened Element, Enable Preview
Tab, Ask Before…), Open Editor Preview; Create Bookmark List… needs lists Zed's store doesn't keep (one per project).

- `context-menu:Bookmarks.ToolWindow.PopupMenu` Bookmarks.ToolWindow.PopupMenu — (a) · tested: bookmarks-tool-window-menu (pass)
- `action:BookmarksView.AutoscrollFromSource` Always Select Opened Element — (a)
- `action:BookmarksView.AskBeforeDeletingLists` Ask Before Deleting Multiple Bookmarks — (a)
- `action:BookmarksView.RewriteBookmarkType` Ask Before Rewriting Mnemonic — (a)
- `action:BookmarksView.ChooseType` Change Mnemonic… — (a) · tested: bookmarks-tool-window-menu (pass)
- `action:BookmarksView.Create` Create Bookmark List… — (a)
- `action:BookmarksView.Delete` Delete — (a) · tested: bookmarks-tool-window-menu (pass)
- `action:BookmarksView.Rename` Edit — (a) · tested: bookmarks-tool-window-menu (pass)
- `action:BookmarksView.OpenInPreviewTab` Enable Preview Tab — (a)
- `action:GotoBookmark0` GotoBookmark0 — (a)
- `action:GotoBookmark1` GotoBookmark1 — (a) · tested: bookmarks (fail)
- `action:GotoBookmark2` GotoBookmark2 — (a)
- `action:GotoBookmark3` GotoBookmark3 — (a)
- `action:GotoBookmark4` GotoBookmark4 — (a)
- `action:GotoBookmark5` GotoBookmark5 — (a)
- `action:GotoBookmark6` GotoBookmark6 — (a)
- `action:GotoBookmark7` GotoBookmark7 — (a)
- `action:GotoBookmark8` GotoBookmark8 — (a)
- `action:GotoBookmark9` GotoBookmark9 — (a)
- `action:GotoBookmarkA` GotoBookmarkA — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkB` GotoBookmarkB — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkC` GotoBookmarkC — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkD` GotoBookmarkD — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkE` GotoBookmarkE — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkF` GotoBookmarkF — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkG` GotoBookmarkG — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkH` GotoBookmarkH — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkI` GotoBookmarkI — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkJ` GotoBookmarkJ — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkK` GotoBookmarkK — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkL` GotoBookmarkL — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkM` GotoBookmarkM — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkN` GotoBookmarkN — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkO` GotoBookmarkO — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkP` GotoBookmarkP — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkQ` GotoBookmarkQ — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkR` GotoBookmarkR — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkS` GotoBookmarkS — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkT` GotoBookmarkT — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkU` GotoBookmarkU — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkV` GotoBookmarkV — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkW` GotoBookmarkW — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkX` GotoBookmarkX — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkY` GotoBookmarkY — (a) · tested: bookmarks-letters (pass)
- `action:GotoBookmarkZ` GotoBookmarkZ — (a) · tested: bookmarks-letters (pass)
- `action:BookmarksView.GroupLineBookmarks` Group Line Bookmarks by File — (a)
- `action:BookmarksView.DefaultGroup` Mark List as Default — (a) · tested: bookmarks-tool-window-menu (pass)
- `action:BookmarksView.MoveDown` Move Down — (a) · tested: bookmarks-tool-window-menu (pass)
- `action:BookmarksView.MoveUp` Move Up — (a) · tested: bookmarks-tool-window-menu (pass)
- `action:BookmarksView.AutoscrollToSource` Navigate with Single Click — (a)
- `action:GotoNextBookmarkInEditor` Next Line Bookmark in Editor — (a) · tested: bookmarks-tool-window-menu (pass)
- `action:BookmarksView.ShowPreview` Open Editor Preview — (a)
- `action:GotoPreviousBookmarkInEditor` Previous Line Bookmark in Editor — (a) · tested: bookmarks-tool-window-menu (pass)
- `action:BookmarksView.DeleteType` Remove Mnemonic — (a) · tested: bookmarks-tool-window-menu (pass)
- `action:BookmarksView.SortGroupBookmarks` Sort Bookmarks by Type & Name — (a) · tested: bookmarks-tool-window-menu (pass)
- `action:ToggleBookmark0` ToggleBookmark0 — (a)
- `action:ToggleBookmark1` ToggleBookmark1 — (a) · tested: bookmarks (fail)
- `action:ToggleBookmark2` ToggleBookmark2 — (a)
- `action:ToggleBookmark3` ToggleBookmark3 — (a)
- `action:ToggleBookmark4` ToggleBookmark4 — (a)
- `action:ToggleBookmark5` ToggleBookmark5 — (a)
- `action:ToggleBookmark6` ToggleBookmark6 — (a)
- `action:ToggleBookmark7` ToggleBookmark7 — (a)
- `action:ToggleBookmark8` ToggleBookmark8 — (a)
- `action:ToggleBookmark9` ToggleBookmark9 — (a)
- `action:ToggleBookmarkA` ToggleBookmarkA — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkB` ToggleBookmarkB — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkC` ToggleBookmarkC — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkD` ToggleBookmarkD — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkE` ToggleBookmarkE — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkF` ToggleBookmarkF — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkG` ToggleBookmarkG — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkH` ToggleBookmarkH — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkI` ToggleBookmarkI — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkJ` ToggleBookmarkJ — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkK` ToggleBookmarkK — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkL` ToggleBookmarkL — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkM` ToggleBookmarkM — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkN` ToggleBookmarkN — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkO` ToggleBookmarkO — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkP` ToggleBookmarkP — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkQ` ToggleBookmarkQ — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkR` ToggleBookmarkR — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkS` ToggleBookmarkS — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkT` ToggleBookmarkT — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkU` ToggleBookmarkU — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkV` ToggleBookmarkV — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkW` ToggleBookmarkW — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkX` ToggleBookmarkX — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkY` ToggleBookmarkY — (a) · tested: bookmarks-letters (pass)
- `action:ToggleBookmarkZ` ToggleBookmarkZ — (a) · tested: bookmarks-letters (pass)

#### Tool windows, tabs and window layout — `workspace` (frame.rs), `spork_navigate` (10)

- `action:GoToTab1` GoToTab1 — (a) · tested: window-editor-tabs (pass)
- `action:GoToTab2` GoToTab2 — (a) · tested: window-editor-tabs (pass)
- `action:GoToTab3` GoToTab3 — (a) · tested: window-editor-tabs (pass)
- `action:GoToTab4` GoToTab4 — (a) · tested: window-editor-tabs (pass)
- `action:GoToTab5` GoToTab5 — (a) · tested: window-editor-tabs (pass)
- `action:GoToTab6` GoToTab6 — (a) · tested: window-editor-tabs (pass)
- `action:GoToTab7` GoToTab7 — (a) · tested: window-editor-tabs (pass)
- `action:GoToTab8` GoToTab8 — (a) · tested: window-editor-tabs (pass)
- `action:GoToTab9` GoToTab9 — (a) · tested: window-editor-tabs (pass)
- `action:GoToLastTab` Select the Last Tab — (a) · tested: window-editor-tabs (pass)

#### Editor actions — `spork_editor`, the editor's keymap (7)

- `action:GotoNextError` Next Highlighted Error — (a) · tested: editor-inspection-widget (fail)
- `action:GotoPreviousError` Previous Highlighted Error — (a) · tested: editor-inspection-widget (fail)
- `action:MethodDown` Next Method — (a)
- `action:MethodUp` Previous Method — (a)
- `action:EditorMatchBrace` Move Caret to Matching Brace — (a) · built (editor and files wave) · tested: editor-toggle-case-and-braces (pass)
- `action:NextTemplateParameter` Next Live Template Parameter — (a)
- `action:PrevTemplateParameter` Previous Live Template Parameter — (a)

#### Project tool window, files and scratches — `project_panel` (file_operations, scratches) (1)

- `action:GoToLinkTarget` Go to Link Target — (a)

#### Code generation and refactoring on the language servers — `spork_code` (8)

- `context-menu:CallHierarchyPopupMenu` CallHierarchyPopupMenu — (a) · built (code-usages: Base on This Method, Jump to Source), not ticked: IntelliJ's menu holds Find Usages, Refactor and the project view's items too
- `action:GotoDeclaration` Go to Declaration or Usages — (a) · tested: code-navigation (fail)
- `action:GotoImplementation` Go to Implementation(s) — (a) · tested: code-navigation (fail)
- `action:GotoTypeDeclaration` Go to Type Declaration — (a) · tested: code-navigation (fail)
- `action:GotoSuperMethod` Go to Super Method — (a) · tested: code-navigation (fail)
- `action:CallHierarchy` Call Hierarchy — (a) · tested: code-usages (fail)
- `action:CallHierarchy.BaseOnThisMethod` Base on This Method — (a) · tested: code-usages (fail: its two screenshots only, stale goldens)
- `action:GotoDeclarationOnly` Go to Declaration — (a)

#### Version control — `spork_vcs` (2)

- `action:VcsShowNextChangeMarker` Next Change — (a) · tested: vcs-change-markers (fail)
- `action:VcsShowPrevChangeMarker` Previous Change — (a) · tested: vcs-change-markers (fail)

#### Menus — `zed` (app_menus) (1)

- `menu:GoToMenu` Navigate — (a)

### (b) Needs an engine or a decision first (17)

- `context-menu:JavaMethodHierarchyPopupMenu` JavaMethodHierarchyPopupMenu — (b) Method Hierarchy: no language server request gives it (LSP has call and type hierarchies only)
- `context-menu:MethodHierarchyPopupMenu` MethodHierarchyPopupMenu — (b) Method Hierarchy: no language server request gives it (LSP has call and type hierarchies only)
- `context-menu:TypeHierarchyPopupMenu` TypeHierarchyPopupMenu — (b) Type Hierarchy: Zed's `lsp-types` has no `typeHierarchyProvider` (phase 7), an engine change in `lsp`
- `context-menu:ExternalToolsGroupInHierarchyPopupMenu` ExternalToolsGroupInHierarchyPopupMenu — (b) External Tools: a decision whether Zed's tasks back them
- `dialog:GotoCustomRegion` Custom Folding… — (b) custom folding regions (`// region`): Zed's folds come from syntax and indentation
- `action:EmmetNextEditPoint` Next Emmet Edit Point — (b) an Emmet engine: Zed's Emmet extension is a completion server, IntelliJ's edit points, preview and surround need Emmet itself
- `action:EmmetPreviousEditPoint` Previous Emmet Edit Point — (b) an Emmet engine: Zed's Emmet extension is a completion server, IntelliJ's edit points, preview and surround need Emmet itself
- `action:GotoCustomRegion` Custom Folding… — (b) custom folding regions (`// region`): Zed's folds come from syntax and indentation
- `action:TypeHierarchy` Type Hierarchy — (b) Type Hierarchy: Zed's `lsp-types` has no `typeHierarchyProvider` (phase 7), an engine change in `lsp`
- `action:MethodHierarchy` Method Hierarchy — (b) Method Hierarchy: no language server request gives it (LSP has call and type hierarchies only)
- `action:MethodHierarchy.BaseOnThisMethod` Base on This Method — (b) Method Hierarchy: no language server request gives it (LSP has call and type hierarchies only)
- `action:MethodHierarchy.ImplementMethodAction` Implement Method — (b) Method Hierarchy: no language server request gives it (LSP has call and type hierarchies only)
- `action:MethodHierarchy.OverrideMethodAction` Override Method — (b) Method Hierarchy: no language server request gives it (LSP has call and type hierarchies only)
- `action:TypeHierarchyBase.BaseOnThisType` Base on This Type — (b) Type Hierarchy: Zed's `lsp-types` has no `typeHierarchyProvider` (phase 7), an engine change in `lsp`
- `action:TypeHierarchy.Class` Class Hierarchy — (b) Type Hierarchy: Zed's `lsp-types` has no `typeHierarchyProvider` (phase 7), an engine change in `lsp`
- `action:TypeHierarchy.Subtypes` Subtypes Hierarchy — (b) Type Hierarchy: Zed's `lsp-types` has no `typeHierarchyProvider` (phase 7), an engine change in `lsp`
- `action:TypeHierarchy.Supertypes` Supertypes Hierarchy — (b) Type Hierarchy: Zed's `lsp-types` has no `typeHierarchyProvider` (phase 7), an engine change in `lsp`

### (c) Out of scope (1)

- `action:GotoRow` Row… — (c) the database grid's data tables (module 2)

## [12-build.md](../reference/idea/tasks/12-build.md)

### (a) Buildable now (7)

#### Java, Maven and Gradle — `spork_java` (6)

- `context-menu:Java.BuildMenu` Java.BuildMenu — (a) · tested: java-build (pass)
- `action:CompileDirty` Build Project — (a) · tested: java-build (pass)
- `action:MakeModule` Build Module — (a) · tested: java-build (pass)
- `action:Compile` Rebuild — (a) · built on ⇧⌘F9, not ticked: IntelliJ shows it for a module selected in the Project tool window
- `action:CompileProject` Rebuild Project — (a) · tested: java-build (pass)
- `action:CompileFile` Recompile — (a) · built (the Java and HTML wave) on ⇧⌘F9 in a Java editor (the file marked changed for JDT LS's incremental build), not ticked: not in the Build menu, which a static menu bar can't show only with a file open

#### Menus — `zed` (app_menus) (1)

- `menu:BuildMenu` Build — (a) · built (java-build), not ticked: no Build Artifacts… nor Groovy Resources

### (b) Needs an engine or a decision first (3)

- `action:BuildArtifact` Build Artifacts… — (b) IntelliJ's artifacts (Project Structure): no engine
- `action:Groovy.CheckResources.Make` Build Resources — (b) a Groovy engine (Decision 11 leaves Groovy and its language server to phase 10)
- `action:Groovy.CheckResources.Rebuild` Rebuild Resources — (b) a Groovy engine (Decision 11 leaves Groovy and its language server to phase 10)

### (c) Out of scope (0)


## [13-tools.md](../reference/idea/tasks/13-tools.md)

### (a) Buildable now (11)

#### Java, Maven and Gradle — `spork_java` (3)

- `dialog:GenerateJavadoc` Generate Javadoc… — (a)
- `action:GenerateJavadoc` Generate Javadoc… — (a)
- `action:JShell.Console` JShell Console… — (a)

#### Menus — `zed` (app_menus) (3)

- `menu:ToolsMenu` Tools — (a)
- `dialog:CreateLauncherScript` Create Command Line Launcher… — (a)
- `action:CreateLauncherScript` Create Command Line Launcher… — (a)

#### Tasks & Contexts' contexts — `spork_projects` (the open tabs and bookmarks saved by name) (5)

- `dialog:context.load` Load Context… — (a)
- `dialog:context.save` Save Context… — (a)
- `action:context.save` Save Context… — (a)
- `action:context.load` Load Context… — (a)
- `action:context.clear` Clear Context — (a)

### (b) Needs an engine or a decision first (27)

- `dialog:Markdown.ConfigurePandoc` Configure Pandoc… — (b) Pandoc, an external converter: a decision to depend on it
- `dialog:tasks.configure.servers` Configure Servers… — (b) a task server engine (YouTrack, Jira, GitHub issues): a decision
- `dialog:tasks.create.changelist` Create Changelist… — (b) a task server engine (YouTrack, Jira, GitHub issues): a decision
- `dialog:tasks.edit` Edit Task… — (b) a task server engine (YouTrack, Jira, GitHub issues): a decision
- `dialog:tasks.switch` Switch Task… — (b) a task server engine (YouTrack, Jira, GitHub issues): a decision
- `dialog:ConvertSchemaAction` Convert Schema... — (b) an XML engine (schemas, DTD, validation): Spork IDE has no XML language server yet; a decision (Eclipse's LemMinX would fit Decision 11's way)
- `dialog:CreateDesktopEntry` Create Desktop Entry… — (b) a Linux desktop entry: Linux only, a decision on Spork IDE's Linux packaging
- `action:tasks.switch` Switch Task… — (b) a task server engine (YouTrack, Jira, GitHub issues): a decision
- `action:tasks.goto` tasks.goto — (b) a task server engine (YouTrack, Jira, GitHub issues): a decision
- `action:tasks.close` Close Active Task… — (b) a task server engine (YouTrack, Jira, GitHub issues): a decision
- `action:tasks.edit` Edit Task… — (b) a task server engine (YouTrack, Jira, GitHub issues): a decision
- `action:tasks.create.changelist` Create Changelist… — (b) a task server engine (YouTrack, Jira, GitHub issues): a decision
- `action:tasks.show.task.description` Show Description — (b) a task server engine (YouTrack, Jira, GitHub issues): a decision
- `action:tasks.open.in.browser` Open in Browser — (b) a task server engine (YouTrack, Jira, GitHub issues): a decision
- `action:tasks.analyze.stacktrace` Analyze Stack Trace From Task… — (b) a task server engine (YouTrack, Jira, GitHub issues): a decision
- `action:tasks.configure.servers` Configure Servers… — (b) a task server engine (YouTrack, Jira, GitHub issues): a decision
- `action:CWMHostShowPopupAction` Code With Me — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:CreateDesktopEntry` Create Desktop Entry… — (b) a Linux desktop entry: Linux only, a decision on Spork IDE's Linux packaging
- `action:ConvertSchemaAction` Convert Schema... — (b) an XML engine (schemas, DTD, validation): Spork IDE has no XML language server yet; a decision (Eclipse's LemMinX would fit Decision 11's way)
- `action:ValidateXml` Validate — (b) an XML engine (schemas, DTD, validation): Spork IDE has no XML language server yet; a decision (Eclipse's LemMinX would fit Decision 11's way)
- `action:GenerateDTD` Generate DTD from XML File — (b) an XML engine (schemas, DTD, validation): Spork IDE has no XML language server yet; a decision (Eclipse's LemMinX would fit Decision 11's way)
- `action:XSD2Document` Generate XSD Schema from XML File… — (b) an XML engine (schemas, DTD, validation): Spork IDE has no XML language server yet; a decision (Eclipse's LemMinX would fit Decision 11's way)
- `action:Document2XSD` Generate XML Document from XSD Schema… — (b) an XML engine (schemas, DTD, validation): Spork IDE has no XML language server yet; a decision (Eclipse's LemMinX would fit Decision 11's way)
- `action:Markdown.ImportFromDocx` Import Word Document… — (b) Pandoc, an external converter: a decision to depend on it
- `action:Markdown.Export` Export Markdown File To… — (b) Pandoc, an external converter: a decision to depend on it
- `action:Markdown.ConfigurePandoc` Configure Pandoc… — (b) Pandoc, an external converter: a decision to depend on it
- `action:Groovy.Console` Groovy Console — (b) a Groovy engine (Decision 11 leaves Groovy and its language server to phase 10)

### (c) Out of scope (8)

- `action:PsiViewer` View PSI Structure… — (c) IntelliJ plugin development (DevKit)
- `action:PsiViewerForContext` View PSI Structure of Current File… — (c) IntelliJ plugin development (DevKit)
- `action:DevKit.Bootstrap` Create IntelliJ IDEA Plugin… — (c) IntelliJ plugin development (DevKit)
- `action:KotlinCodeMigrationToggle` Enable Migrations Detection — (c) Kotlin (no engine chosen, left out with notebooks and database tools)
- `action:KotlinConsoleREPL` Kotlin REPL (Experimental) — (c) Kotlin (no engine chosen, left out with notebooks and database tools)
- `action:ShowKotlinBytecode` Show Kotlin Bytecode — (c) Kotlin (no engine chosen, left out with notebooks and database tools)
- `action:ConfigureKotlinInProject` Configure Kotlin in Project — (c) Kotlin (no engine chosen, left out with notebooks and database tools)
- `action:DecompileKotlinToJava` Decompile to Java — (c) Kotlin (no engine chosen, left out with notebooks and database tools)

## [16-other.md](../reference/idea/tasks/16-other.md)

### (a) Buildable now (368)

#### Search Everywhere, the Switcher and Recent Files — `spork_navigate` (19)

- `action:SearchEverywhere.CompleteCommand` Complete Command — (a) · built (second UI wave) · tested: search-everywhere-commands (pass)
- `action:DeleteRecentFiles` Delete from Recent Files — (a) · tested: list-keys (pass)
- `action:DeleteRecentFilesFallback` Delete from Recent Files — (a) · tested: list-keys (pass)
- `action:SwitcherBackward` Iterate Switcher Backward — (a) · tested: switcher-iterate (pass)
- `action:SwitcherBackwardFallback` Iterate Switcher Backward — (a) · tested: switcher-iterate (pass)
- `action:SwitcherForward` Iterate Switcher Forward — (a) · tested: switcher-iterate (pass)
- `action:SwitcherForwardFallback` Iterate Switcher Forward — (a) · tested: switcher-iterate (pass)
- `action:SearchEverywhere.NavigateToNextGroup` Next Group Element — (a) · tested: search-everywhere-keys (pass)
- `action:SearchEverywhere.NextTab` Next Tab — (a) · tested: search-everywhere (fail)
- `action:SearchEverywhere.NavigateToPrevGroup` Prev Group Element — (a) · tested: search-everywhere-keys (pass)
- `action:SearchEverywhere.PrevTab` Prev Tab — (a) · tested: search-everywhere (fail)
- `action:SearchEverywhereComponent` Search Everywhere Component — (a) · not built: the reference app's toolbar search field; the main toolbar has Search Everywhere's button
- `action:SearchEverywhere.SelectItem` Select Item — (a) · tested: search-everywhere-keys (pass)
- `action:SwitcherNextProblem` Select Next Problem File — (a) · built (second UI wave) · tested: switcher-problem-files (pass)
- `action:SwitcherNextProblemFallback` Select Next Problem File — (a) · built (second UI wave) · tested: switcher-problem-files (pass)
- `action:SwitcherPreviousProblem` Select Previous Problem File — (a) · built (second UI wave) · tested: switcher-problem-files (pass)
- `action:SwitcherPreviousProblemFallback` Select Previous Problem File — (a) · built (second UI wave) · tested: switcher-problem-files (pass)
- `action:Switcher` Switcher — (a) · tested: switcher-iterate (pass), switcher (fail)
- `action:SwitcherFallback` Switcher — (a) · tested: switcher-iterate (pass)

#### Find tool window and usages — `spork_navigate` (find_panel), `spork_code` (6)

- `action:UsageGrouping.Package` Package — (a)
- `action:UsageGrouping.Directory` UsageGrouping.Directory — (a)
- `action:UsageGrouping.DirectoryStructure` UsageGrouping.DirectoryStructure — (a)
- `action:UsageGrouping.FileStructure` UsageGrouping.FileStructure — (a)
- `action:UsageGrouping.Scope` UsageGrouping.Scope — (a)
- `action:UsageGrouping.ShortFilePath` UsageGrouping.ShortFilePath — (a)

#### Bookmarks — `spork_navigate` (bookmarks) (1)

- `action:OpenBookmarkGroup` Open All Bookmarked Files — (a) · tested: bookmarks-tool-window-menu (pass)

#### Tool windows, tabs and window layout — `workspace` (frame.rs), `spork_navigate` (17)

- `action:NextWindow` Activate Next Window — (a)
- `action:PreviousWindow` Activate Previous Window — (a)
- `action:CloseEditor` Close Active Editor — (a) · tested: window-editor-tabs (pass)
- `action:DecrementWindowHeight` Decrement Height — (a)
- `action:DecrementWindowWidth` Decrement Width — (a)
- `action:DockPinnedMode` Dock Pinned — (a)
- `action:DockUnpinnedMode` Dock Unpinned — (a)
- `action:ToggleDockMode` Docked Mode — (a)
- `action:FocusMainToolbar` Focus Main Toolbar — (a)
- `action:IncrementWindowHeight` Increment Height — (a)
- `action:IncrementWindowWidth` Increment Width — (a)
- `action:PinActiveTab` Pin Active Tab — (a) · tested: window-editor-tabs (pass)
- `action:PinActiveTabToggle` Pin Active Tab — (a) · tested: window-editor-tabs (pass)
- `action:TogglePinnedMode` Pinned Mode — (a)
- `action:ResetIdeScaleAction` Reset IDE Scale — (a)
- `action:ZoomInIdeAction` Zoom In IDE — (a)
- `action:ZoomOutIdeAction` Zoom Out IDE — (a)

#### Editor actions — `spork_editor`, the editor's keymap (22)

- `action:ShowParameterHintsSettings` Hints Settings… — (a)
- `action:ToggleInlineHintsAction` Toggle Parameter Name Hints — (a)
- `action:ClickLink` Click Link — (a)
- `action:DisableMinimap` DisableMinimap — (a)
- `action:SmartSelect` Extend Selection — (a) · built (editor and files wave) · tested: editor-multiple-carets (pass)
- `action:CodeFloatingToolbar.GotoNextMenu` Go to Next Toolbar Menu — (a)
- `action:CodeFloatingToolbar.GotoPrevMenu` Go to Previous Toolbar Menu — (a)
- `action:IncrementalSearch` Incremental Search — (a)
- `action:BraceOrQuoteOut` Jump Outside Current Bracket/Quote — (a)
- `action:MethodOverloadSwitchDown` Next Method Overload — (a)
- `action:OpenMinimapSettings` OpenMinimapSettings — (a)
- `action:MethodOverloadSwitchUp` Previous Method Overload — (a)
- `action:SaveDocument` Save File — (a)
- `action:ClassNameCompletion` Second Basic Completion — (a)
- `action:ShowPopupMenu` Show Context Menu — (a)
- `action:ShowFontsUsedByEditor` Show Fonts Used by Editor — (a)
- `action:QuickActionPopup` Show Quick Actions Popup — (a)
- `action:ShowSearchHistory` Show Search History — (a)
- `action:SmartUnSelect` Shrink Selection — (a) · built (editor and files wave) · tested: editor-multiple-carets (pass)
- `action:TogglePopupHints` Toggle Popup Hints for Current File — (a)
- `action:ToggleInlayHintsGloballyAction` ToggleInlayHintsGloballyAction — (a)
- `action:ToggleMinimap` ToggleMinimap — (a)

#### Documentation popup and tool window — `spork_editor` (documentation) (7)

- `action:QuickDocCopy` Copy Quick Doc — (a) · built (editor and files wave) · tested: editor-documentation-tool-window (pass)
- `action:Documentation.EditSource` Jump to Source — (a) · built (editor and files wave) · tested: editor-documentation-tool-window (pass)
- `action:Documentation.ViewExternal` View External Documentation — (a)
- `action:Documentation.ToggleAutoUpdate` Auto-Update from Source — (a) · built (editor and files wave) · tested: editor-documentation-tool-window (pass)
- `action:ShowHoverInfo` Hover Info — (a)
- `action:Documentation.ToggleAutoShow` Show Automatically During Completion — (a) · built (editor and files wave) · tested: editor-documentation-tool-window (pass)
- `action:Documentation.ToggleShowInPopup` Show Documentation Popup First — (a) · built (editor and files wave) · tested: editor-documentation-tool-window (pass)

#### Lists, trees, tables and popups from the keyboard — `spork_workbench` (tree, popup), gpui-kit, Zed's `ui` lists (86)

Built (2026-11-23, the keyboard wave): `spork_workbench::tree_keys`, the one model the Bookmarks, Find, Hierarchy tool
windows, Recent Files and the Switcher, File Structure, Recent Locations, Show Usages and the caret's list popups use.
Left in (a): Table-* and the column width actions (no table view yet), the numpad's Expand/Collapse/Fully Expand Tree
Node (GPUI doesn't tell the numpad's keys from the main ones), ScrollPane-*, Scroll to Center's test, Edit Source (Not
in Editor) and Jump to Source in trees, Edit Tree Node, the segmented buttons, the List-*ColumnExtendSelection pair.

- `action:PopupMenu-cancel` Close Context Menu — (a) · tested: popup-menu-keys (pass)
- `action:CollapseTreeNode` Collapse Tree Node — (a)
- `action:EditSourceNotInEditor` Edit Source (Not in Editor) — (a)
- `action:Table-startEditing` Edit Table Cell — (a)
- `action:Tree-startEditing` Edit Tree Node — (a)
- `action:IncreaseColumnWidth` Expand Selected Columns — (a)
- `action:ExpandTreeNode` Expand Tree Node — (a)
- `action:Tree-selectChildExtendSelection` Extend Selection to Child Node — (a) · tested: tree-keys (pass)
- `action:Table-selectFirstColumnExtendSelection` Extend Selection to First Column — (a)
- `action:Tree-selectFirstExtendSelection` Extend Selection to First Node — (a) · tested: tree-keys (pass)
- `action:List-selectFirstRowExtendSelection` Extend Selection to First Row — (a) · tested: list-keys (pass)
- `action:Table-selectFirstRowExtendSelection` Extend Selection to First Row — (a)
- `action:Table-selectLastColumnExtendSelection` Extend Selection to Last Column — (a)
- `action:Tree-selectLastExtendSelection` Extend Selection to Last Node — (a) · tested: tree-keys (pass)
- `action:List-selectLastRowExtendSelection` Extend Selection to Last Row — (a) · tested: list-keys (pass)
- `action:Table-selectLastRowExtendSelection` Extend Selection to Last Row — (a)
- `action:List-selectNextColumnExtendSelection` Extend Selection to Next Column — (a)
- `action:Table-selectNextColumnExtendSelection` Extend Selection to Next Column — (a)
- `action:Tree-selectParentExtendSelection` Extend Selection to Parent Node — (a) · tested: tree-keys (pass)
- `action:List-selectPreviousColumnExtendSelection` Extend Selection to Previous Column — (a)
- `action:Table-selectPreviousColumnExtendSelection` Extend Selection to Previous Column — (a)
- `action:Tree-selectNextExtendSelection` Extend Selection with Next Node — (a) · tested: tree-keys (pass)
- `action:List-selectNextRowExtendSelection` Extend Selection with Next Row — (a) · tested: list-keys (pass)
- `action:Table-selectNextRowExtendSelection` Extend Selection with Next Row — (a)
- `action:Tree-selectPreviousExtendSelection` Extend Selection with Previous Node — (a) · tested: tree-keys (pass)
- `action:List-selectPreviousRowExtendSelection` Extend Selection with Previous Row — (a) · tested: list-keys (pass)
- `action:Table-selectPreviousRowExtendSelection` Extend Selection with Previous Row — (a)
- `action:FullyExpandTreeNode` Fully Expand Tree Node — (a)
- `action:PopupMenu-return` Invoke Selected Action — (a) · tested: popup-menu-keys (pass)
- `action:Frontend.EditSource` Jump to Source — (a)
- `action:DecreaseColumnWidth` Narrow Down Selected Columns — (a)
- `action:ResetColumnsWidth` Reset Columns Width — (a)
- `action:ScrollPane-unitScrollDown` Scroll Down — (a)
- `action:ScrollPane-scrollEnd` Scroll End — (a)
- `action:ScrollPane-scrollHome` Scroll Home — (a)
- `action:ScrollPane-unitScrollLeft` Scroll Left — (a)
- `action:ScrollPane-scrollDown` Scroll Page Down — (a)
- `action:List-scrollDownExtendSelection` Scroll Page Down and Extend Selection — (a) · tested: list-keys (pass)
- `action:Table-scrollDownExtendSelection` Scroll Page Down and Extend Selection — (a)
- `action:Tree-scrollDownExtendSelection` Scroll Page Down and Extend Selection — (a) · tested: tree-keys (pass)
- `action:Tree-scrollDownChangeSelection` Scroll Page Down and Select Node — (a) · tested: tree-keys (pass)
- `action:List-scrollDown` Scroll Page Down and Select Row — (a) · tested: list-keys (pass)
- `action:Table-scrollDownChangeSelection` Scroll Page Down and Select Row — (a)
- `action:ScrollPane-scrollLeft` Scroll Page Left — (a)
- `action:ScrollPane-scrollRight` Scroll Page Right — (a)
- `action:ScrollPane-scrollUp` Scroll Page Up — (a)
- `action:List-scrollUpExtendSelection` Scroll Page Up and Extend Selection — (a) · tested: list-keys (pass)
- `action:Table-scrollUpExtendSelection` Scroll Page Up and Extend Selection — (a)
- `action:Tree-scrollUpExtendSelection` Scroll Page Up and Extend Selection — (a) · tested: tree-keys (pass)
- `action:Tree-scrollUpChangeSelection` Scroll Page Up and Select Node — (a) · tested: tree-keys (pass)
- `action:List-scrollUp` Scroll Page Up and Select Row — (a) · tested: list-keys (pass)
- `action:Table-scrollUpChangeSelection` Scroll Page Up and Select Row — (a)
- `action:ScrollPane-unitScrollRight` Scroll Right — (a)
- `action:ScrollTreeToCenter` Scroll to Center — (a)
- `action:ScrollPane-unitScrollUp` Scroll Up — (a)
- `action:Tree-selectAll` Select All Nodes — (a) · tested: tree-keys (pass)
- `action:PopupMenu-selectChild` Select Child Menu Item — (a) · tested: popup-menu-keys (pass)
- `action:Tree-selectChild` Select Child Node — (a) · tested: project-panel-tree-keys (pass), tree-keys (pass)
- `action:Table-selectFirstColumn` Select First Column — (a)
- `action:Tree-selectFirst` Select First Node — (a) · tested: project-panel-tree-keys (pass), tree-keys (pass)
- `action:List-selectFirstRow` Select First Row — (a) · tested: list-keys (pass)
- `action:Table-selectFirstRow` Select First Row — (a)
- `action:Table-selectLastColumn` Select Last Column — (a)
- `action:Tree-selectLast` Select Last Node — (a) · tested: project-panel-tree-keys (pass), tree-keys (pass)
- `action:List-selectLastRow` Select Last Row — (a) · tested: list-keys (pass)
- `action:Table-selectLastRow` Select Last Row — (a)
- `action:List-selectNextColumn` Select Next Column — (a) · tested: list-keys (pass)
- `action:Table-selectNextColumn` Select Next Column — (a)
- `action:PopupMenu-selectNext` Select Next Menu Item — (a) · tested: popup-menu-keys (pass)
- `action:Tree-selectNext` Select Next Node — (a) · tested: project-panel-tree-keys (pass), tree-keys (pass)
- `action:List-selectNextRow` Select Next Row — (a) · tested: list-keys (pass)
- `action:Table-selectNextRow` Select Next Row — (a)
- `action:SegmentedButton-right` Select Next Segmented Button — (a)
- `action:Tree-selectNextSibling` Select Next Sibling Node — (a) · tested: tree-keys (pass)
- `action:PopupMenu-selectParent` Select Parent Menu Item — (a) · tested: popup-menu-keys (pass)
- `action:Tree-selectParent` Select Parent Node — (a) · tested: tree-keys (pass)
- `action:Tree-selectParentNoCollapse` Select Parent Node — (a) · tested: tree-keys (pass)
- `action:List-selectPreviousColumn` Select Previous Column — (a) · tested: list-keys (pass)
- `action:Table-selectPreviousColumn` Select Previous Column — (a)
- `action:PopupMenu-selectPrevious` Select Previous Menu Item — (a) · tested: popup-menu-keys (pass)
- `action:Tree-selectPrevious` Select Previous Node — (a) · tested: project-panel-tree-keys (pass), tree-keys (pass)
- `action:List-selectPreviousRow` Select Previous Row — (a) · tested: list-keys (pass)
- `action:Table-selectPreviousRow` Select Previous Row — (a)
- `action:SegmentedButton-left` Select Previous Segmented Button — (a)
- `action:Tree-selectPreviousSibling` Select Previous Sibling Node — (a) · tested: tree-keys (pass)
- `action:SpeedSearch` Speed Search — (a) · tested: tree-keys (pass)

#### Text fields and panels — `spork_workbench` (controls) (5)

- `action:TextComponent.ClearAction` Clear Text — (a)
- `action:CollapseExpandableComponent` Collapse Component — (a)
- `action:ExpandExpandableComponent` Expand Component — (a)
- `action:CollapsiblePanel-toggle` Expand/Collapse Collapsible Panel — (a)
- `action:MaximizeActiveDialog` Maximize/Restore Active Dialog — (a)

#### Project tool window, files and scratches — `project_panel` (file_operations, scratches) (14)

- `context-menu:CommanderPopupMenu` CommanderPopupMenu — (a)
- `context-menu:ScopeViewPopupMenu` ScopeViewPopupMenu — (a)
- `action:CreateEditorConfigFile` EditorConfig File — (a)
- `action:NewDir` Directory/Package — (a) · tested: project-panel-new (pass)
- `action:NewFile` File — (a) · tested: project-panel-new (pass)
- `action:NewHtmlFile` HTML File — (a) · built (editor and files wave) · tested: project-panel-new-files (pass)
- `action:CheckIgnoredAndNotExcludedDirectories` Check Ignored but not Excluded Directories — (a)
- `action:Scratch.ExportToScratch` Export to Scratch File — (a)
- `action:NewElementSamePlace` New in This Directory… — (a) · built (editor and files wave) · tested: project-panel-new-files (pass)
- `action:NewScratchBuffer` New Scratch Buffer — (a) · built (editor and files wave) · tested: project-panel-new-files (pass)
- `action:NewEditorConfigFile` NewEditorConfigFile — (a)
- `action:OpenInBrowser` Open in Default Browser — (a)
- `action:QuickPreview` Quick Preview — (a)
- `action:Scratch.ShowFilesPopup` Show Scratch Files — (a) · built (second UI wave, scratch-files-popup), not ticked: its Delete (a scratch file deleted from the popup) is left

#### Welcome window and recent projects — `spork_projects` (20)

Built (2026-11-23, the second UI wave): recent project groups (New Project Group, Edit…, Move to Group), Plugins and
the gear's Settings…. Left in (a): the non-modal welcome screen's New items (Text File, Markdown File, File, New
Project), Branches and Cancel (the reference app's), Open / Open or Import (the system's file dialog), Reveal in Finder (a test
can't open Finder), Trust Project… (the tests trust every folder). Show Scratch Files… came with the same wave (the
Project tool window's group below).

- `action:WelcomeNewEmptyFile` Text File — (a)
- `action:WelcomeMarkdownNewFile` Markdown File — (a)
- `action:WelcomeScreen.DefaultNewProjectAction` New Project — (a)
- `action:ProjectFromVersionControl` Project from Version Control… — (a)
- `action:NewProject` Project… — (a)
- `action:WelcomeScreen.Plugins` Plugins — (a) · built (second UI wave) · tested: welcome-plugins-settings (pass)
- `action:WelcomeScreen.OpenProject` Open or Import — (a)
- `action:WelcomeScreen.Branches` Branches — (a)
- `action:WelcomeScreen.CancelRecentProjectTask` Cancel — (a)
- `action:WelcomeScreen.CopyProjectPath` Copy Path — (a) · built (editor and files wave) · tested: welcome-recent-project-menu (pass)
- `action:WelcomeScreen.EditGroup` Edit… — (a) · built (second UI wave) · tested: welcome-project-groups (pass)
- `action:welcome.screen.left.tab.new.empty.file` File — (a)
- `action:WelcomeScreen.CreateNewProject` New Project — (a) · tested: welcome-new-and-clone (pass)
- `action:WelcomeScreen.NewGroup` New Project Group — (a) · built (second UI wave) · tested: welcome-project-groups (pass)
- `action:WelcomeScreen.OpenDirectoryProject` Open — (a)
- `action:WelcomeScreen.OpenSelected` Open Selected — (a) · built (editor and files wave) · tested: welcome-recent-project-menu (pass)
- `action:WelcomeScreen.RemoveSelected` Remove from Recent Projects… — (a) · tested: welcome-recent-project-actions (pass)
- `action:ShowTrustProjectDialog` Trust Project… — (a)
- `action:WelcomeScreen.RevealIn` WelcomeScreen.RevealIn — (a)
- `action:WelcomeScreen.Settings` WelcomeScreen.Settings — (a) · built (second UI wave) · tested: welcome-plugins-settings (pass)

#### Problems and inspection results — `spork_tool_windows`, `spork_code` (13)

- `toolbar:ProblemsView.ToolWindow.Toolbar` ProblemsView.ToolWindow.Toolbar — (a)
- `context-menu:InspectionToolWindow.TreePopup` InspectionToolWindow.TreePopup — (a)
- `context-menu:ProblemsView.ToolWindow.TreePopup` ProblemsView.ToolWindow.TreePopup — (a) · built (editor and files wave) · tested: problems-tool-window-options (pass)
- `action:ProblemsView.OpenInPreviewTab` Enable Preview Tab — (a)
- `action:ProblemsView.AutoscrollToSource` Open Files with Single Click — (a) · built (editor and files wave) · tested: problems-tool-window-options (pass)
- `action:ProblemsView.ShowPreview` Open Editor Preview — (a)
- `action:ProblemsView.QuickFixes` Show Quick-Fixes — (a)
- `action:ProblemsView.SortFoldersFirst` Folders Always on Top — (a) · built (editor and files wave) · tested: problems-tool-window-options (pass)
- `action:ProblemsView.GroupByToolId` Group by Inspection — (a) · built (editor and files wave) · tested: problems-tool-window-options (pass)
- `action:ProblemsView.SortByName` Name — (a)
- `action:ProblemsView.SortBySeverity` Severity — (a)
- `action:ProblemsView.CopyProblemDescription` Copy Problem Description — (a) · built (editor and files wave) · tested: problems-tool-window-options (pass)
- `action:CodeInspection.OnEditor` Inspect Code with Editor Settings — (a)

#### Code generation and refactoring on the language servers — `spork_code` (6)

- `action:GenerateCopyright` Copyright — (a)
- `action:ShGenerateForLoop` For Loop — (a) · tested: code-generate-shell (pass)
- `action:ShGenerateUntilLoop` Until Loop — (a) · tested: code-generate-shell (pass)
- `action:ShGenerateWhileLoop` While Loop — (a) · tested: code-generate-shell (pass)
- `action:AnonymousToInner` Convert Anonymous to Inner… — (a)
- `action:FixDocComment` Fix Doc Comment — (a)

#### Java, Maven and Gradle — `spork_java` (53)

- `toolbar:Maven.NavigatorActionsToolbar` Maven.NavigatorActionsToolbar — (a)
- `context-menu:CompilerErrorViewPopupMenu` CompilerErrorViewPopupMenu — (a)
- `context-menu:Maven.BuildMenu` Maven.BuildMenu — (a) · built (java-maven-menus), not ticked: no Execute Before Run/Debug… nor Assign Shortcut…
- `context-menu:Maven.DependencyMenu` Maven.DependencyMenu — (a) · built (java-maven-menus), not ticked: no Analyze Dependency (b)
- `context-menu:Maven.DownloadAllGroupPopup` Maven.DownloadAllGroupPopup — (a) · tested: java-maven-menus (pass)
- `context-menu:Maven.NavigatorProjectMenu` Maven.NavigatorProjectMenu — (a) · built (java-maven-menus), not ticked: no Ignore/Unlink, Show Effective POM, Analyze Dependencies or run context configurations
- `context-menu:Maven.ProfileMenu` Maven.ProfileMenu — (a) · tested: java-maven-menus (pass)
- `context-menu:Maven.ProfilesMenu` Maven.ProfilesMenu — (a) · tested: java-maven-menus (pass)
- `context-menu:Maven.RunConfigurationMenu` Maven.RunConfigurationMenu — (a)
- `context-menu:Maven.SyncAllGroupPopup` Maven.SyncAllGroupPopup — (a) · tested: java-maven-menus (pass)
- `action:DelegateMethods` Delegate Methods… — (a) · tested: java-generate (pass)
- `action:CompactSourceFile` CompactSourceFile — (a)
- `action:NewClass` Java Class — (a) · tested: java-new-class (pass)
- `action:NewModuleInfo` NewModuleInfo — (a) · tested: java-new-class (pass)
- `action:NewPackageInfo` NewPackageInfo — (a) · tested: java-new-class (pass)
- `action:GenerateConstructor` Constructor — (a) · tested: java-generate (pass)
- `action:GenerateEquals` equals() and hashCode() — (a) · built (java-generate), not ticked: one chooser where IntelliJ has its wizard
- `action:GenerateGetter` Getter — (a) · tested: java-generate (pass)
- `action:GenerateGetterAndSetter` Getter and Setter — (a) · tested: java-generate (pass)
- `action:GenerateSetter` Setter — (a) · tested: java-generate (pass)
- `action:Actions.ActionsPlugin.GenerateToString` toString() — (a) · built (java-generate), not ticked: JDT LS's template, no template chooser
- `action:Maven.AssignShortcut` Assign Shortcut… — (a)
- `action:Maven.AfterCompile` Execute After Build — (a) · built, not ticked: no scenario runs it
- `action:Maven.AfterRebuild` Execute After Rebuild — (a) · built, not ticked: no scenario runs it
- `action:Maven.BeforeCompile` Execute Before Build — (a) · tested: java-maven-menus (pass)
- `action:Maven.BeforeRebuild` Execute Before Rebuild — (a) · built, not ticked: no scenario runs it
- `action:Maven.BeforeRun` Execute Before Run/Debug… — (a)
- `action:Maven.DownloadSelectedDocs` Download Documentation — (a) · built, not ticked: no scenario runs it
- `action:Maven.DownloadSelectedSources` Download Sources — (a) · tested: java-maven-menus (pass)
- `action:Maven.DownloadSelectedSourcesAndDocs` Download Sources and Documentation — (a) · built, not ticked: no scenario runs it
- `action:Maven.AddManagedFiles` Add Maven Projects — (a)
- `action:Maven.CollapseAll` Collapse All — (a)
- `action:Maven.ExecuteGoal` Execute Maven Goal — (a)
- `action:Maven.ExpandAll` Expand All — (a)
- `action:Maven.UpdateFolders` Generate Sources and Update Folders For All Projects — (a)
- `action:Maven.RunBuild` Run Maven Build — (a)
- `action:Maven.ToggleSkipTests` Toggle 'Skip Tests' Mode — (a)
- `action:Maven.ToggleOffline` Toggle Offline Mode — (a) · tested: java-maven-tool-window (pass)
- `action:Maven.Reimport` Reload All Maven Projects — (a) · tested: java-maven-menus (pass)
- `action:Maven.SyncIncrementally` Sync All Maven Projects — (a) · built, not ticked: no scenario runs it
- `action:Maven.ToggleProfile` Toggle Profile — (a) · tested: java-maven-menus (pass)
- `action:Maven.ResetProfiles` Reset Profiles — (a) · tested: java-maven-menus (pass)
- `action:Maven.EditRunConfiguration` Edit Run Configuration… — (a)
- `action:Maven.RemoveRunConfiguration` Remove Run Configuration — (a)
- `action:AddJdkAction` Add JDK from Disk… — (a) · built (the Java and HTML wave) in the "Project JDK is not defined" banner's Setup SDK popup (java-jdk-banner), and (the JDK wave) as Project Structure's JDK… and Add JDK from disk…; not ticked: no scenario picks a folder yet (`spork_workbench::pick_next` now answers it)
- `action:Maven.AlwaysShowArtifactId` Always Show ArtifactId — (a) · tested: java-maven-view-options
- `action:Maven.GroupProjects` Group Modules — (a) · built (the Java and HTML wave), not ticked: a one-module project shows no grouping
- `action:Gradle.MigrateToDaemonJvmCriteria` Migrate Project to Daemon JVM Criteria — (a)
- `action:Maven.ShowBasicPhasesOnly` Show Basic Phases Only — (a) · tested: java-maven-view-options
- `action:Gradle.ShowDaemons` Show Gradle Daemons — (a)
- `action:Maven.ShowIgnored` Show Ignored Projects — (a) · built (the Java and HTML wave), not ticked: no project can be ignored yet
- `action:Maven.ShowVersions` Show version — (a) · tested: java-maven-view-options
- `action:Maven.ToggleLookNested` Toggle Search For Maven Project Recursively — (a)

#### Version control — `spork_vcs` (13)

Built (2026-11-23, the version control, run and terminal wave): Next/Previous Difference (F7, ⇧F7) in every diff,
Close Diff Editor (⎋) in the diff tabs, the Commit tool window's Options ▸ Show on Double-Click ▸ Diff or Source. Left
in (a): Update, Compare with Specified Revision…, Open Blank Diff Window, Refresh and Force Refresh in the version
control views, Configure GPG Agent, the code review comments' Next/Previous.

- `action:UpdateFiles` Update — (a)
- `action:CloseDiffEditor` Close Diff Editor — (a) · built (version control, run and terminal wave) · tested: vcs-diff-navigation (pass)
- `action:CodeReview.NextComment` CodeReview.NextComment — (a)
- `action:CodeReview.PreviousComment` CodeReview.PreviousComment — (a)
- `action:Compare.Specified` Compare with Specified Revision… — (a)
- `action:gpg.agent.configuration` Configure GPG Agent — (a)
- `action:ForceRefresh` Force Refresh — (a)
- `action:NextDiff` Next Difference — (a) · built (version control, run and terminal wave) · tested: vcs-diff-navigation (pass)
- `action:ShowBlankDiffWindow` Open Blank Diff Window — (a)
- `action:PreviousDiff` Previous Difference — (a) · built (version control, run and terminal wave) · tested: vcs-diff-navigation (pass)
- `action:Refresh` Refresh — (a)
- `action:CommitView.ShowOnDoubleClick.EditorPreview` Show Diff on Double-Click — (a) · built (version control, run and terminal wave) · tested: vcs-commit-double-click (pass)
- `action:CommitView.ShowOnDoubleClick.Source` Show Source on Double-Click — (a) · built (version control, run and terminal wave) · tested: vcs-commit-double-click (pass)

#### Run, debug and Services — `spork_run` (the run and debug agent's) (20)

Built (2026-11-23, the version control, run and terminal wave): Edit Breakpoint, Quick Evaluate Expression, Send EOF,
Services' Group Services by Type and the run widget popup's All Configurations toggle (with its Pinned and Recent
groups and the rows' submenus, phase 8's leftovers). Left in (a): Services' toolbars, popup and tab actions (Open in
New Tab, Open Each in New Tab, Open Each Type in New Tab, Open in Separate Tool Window, Show Services Tree, Jump to
Services, Always Select Active Service, Configure Services Tool Window, Clouds and Run Dashboard), Reload Changed
Classes (built 2026-11-25 by the debugger wave). Built (2026-11-24, the Docker agent, `spork_docker` and `spork_run`'s `docker.rs`): Docker in Services, IntelliJ's
Docker plugin (the paid IDE's: no task of the CE catalogue lists it), its connections, tree, tabs and actions (ROADMAP,
phase 8's Docker paragraph); tested: docker-services-connections, docker-settings, docker-services-tree,
docker-services-actions.

- `toolbar:ServiceViewItemToolbar` ServiceViewItemToolbar — (a)
- `toolbar:ServiceViewItemToolbarGroup` ServiceViewItemToolbarGroup — (a)
- `toolbar:ServiceViewTreeToolbar` ServiceViewTreeToolbar — (a)
- `context-menu:ServiceViewItemPopup` ServiceViewItemPopup — (a)
- `action:ServiceView.SelectActiveService` Always Select Active Service — (a)
- `action:ServiceView.ConfigureServices` Configure Services Tool Window — (a)
- `action:ServiceView.ShowServices` Show Services Tree — (a)
- `action:ServiceView.JumpToServices` Jump to Services — (a)
- `action:ServiceView.OpenEachInNewTab` Open Each in New Tab — (a)
- `action:ServiceView.SplitByType` Open Each Type in New Tab — (a)
- `action:ServiceView.OpenInNewTab` Open in New Tab — (a)
- `action:ServiceView.OpenInToolWindow` Open in Separate Tool Window — (a)
- `action:ServiceView.GroupByContributor` Group Services by Type — (a) · built (version control, run and terminal wave) · tested: services-tool-window (pass)
- `action:ServiceView.ActivateDefaultRemoteServersServiceViewContributor` Clouds (Services) — (a)
- `action:EditBreakpoint` Edit Breakpoint — (a) · built (version control, run and terminal wave) · tested: debug-exception-breakpoints (pass)
- `action:QuickEvaluateExpression` Quick Evaluate Expression — (a) · built (version control, run and terminal wave) · tested: debug-quick-evaluate (pass)
- `action:Hotswap` Reload Changed Classes — (a) · built (debugger wave) · tested: java-debug-hotswap (pass)
- `action:ServiceView.ActivateRunDashboardServiceViewContributor` Run Dashboard (Services) — (a)
- `action:SendEOF` Send EOF — (a) · built (version control, run and terminal wave) · tested: run-send-eof (pass)
- `action:AllRunConfigurationsToggle` Toggle All Run/Debug Configurations — (a) · built (version control, run and terminal wave) · tested: run-configurations-popup (pass)

#### Status bar widgets — `zed` (status_widgets), `workspace` (11)

- `action:FocusStatusBar` Focus Status Bar — (a)
- `action:StatusBarWidgets.Toggle.Encoding` Toggle 'File Encoding' Widget — (a) · built (editor and files wave) · tested: status-bar-widgets (pass)
- `action:StatusBarWidgets.Toggle.git` Toggle 'Git Branch' Widget — (a)
- `action:StatusBarWidgets.Toggle.settingsEntryPointWidget` Toggle 'IDE and Project Settings' Widget — (a)
- `action:StatusBarWidgets.Toggle.CodeStyleStatusBarWidget` Toggle 'Indentation' Widget — (a) · built (editor and files wave) · tested: status-bar-widgets (pass)
- `action:StatusBarWidgets.Toggle.LanguageServiceStatusBarWidget` Toggle 'Language Services' Widget — (a) · built (editor and files wave) · tested: status-bar-widgets (pass)
- `action:StatusBarWidgets.Toggle.LineSeparator` Toggle 'Line Separator' Widget — (a) · built (editor and files wave) · tested: status-bar-widgets (pass)
- `action:StatusBarWidgets.Toggle.Position` Toggle 'Line:Column Number' Widget — (a) · built (editor and files wave) · tested: status-bar-widgets (pass)
- `action:StatusBarWidgets.Toggle.Memory` Toggle 'Memory Indicator' Widget — (a)
- `action:StatusBarWidgets.Toggle.Notifications` Toggle 'Notifications' Widget — (a)
- `action:StatusBarWidgets.Toggle.ReadOnlyAttribute` Toggle 'Read-Only Attribute' Widget — (a) · built (editor and files wave) · tested: status-bar-widgets (pass)

#### Settings, keymap and plugins — `spork_settings` (8)

Built (2026-11-23, the second UI wave): Add Shortcut…, Copy Option Path, Export and Import Settings, Install Plugin
from Disk, Check Suggested Plugins, and of the settings pages Tools › Actions on Save (Reformat code). Left: Smooth
Scroll Options and Maven Settings (no engine or page behind them), the Trusted Locations page (a `project` patch for
the trusted folders); Editor › File Types came with the same wave (settings-file-types).
Built (2026-11-24, s-settings): the Settings window as IntelliJ IDEA 2026.3's (its title with the project, size, pin,
tree order, project marks, Plugins' count of updates, Accessibility), its Appearance page with four UI Options backed
(`settings-ui-options`) and Editor › Font's preview.
Built (2026-11-25, s-plugins): the Plugins page for every kind of Zed extension (installed, loaded, updated with
IntelliJ's Update, uninstalled; a plugin's theme and icon theme applied as installed), tested by the `plugins-*`
scenarios with fixture extensions; no task of the catalogue names them beyond Plugins… and Install Plugin from Disk.

- `action:Maven.ShowSettings` Maven Settings — (a) · not built: no Build Tools › Maven page (Maven's settings are phase 10's)
- `action:SetShortcutAction` Add Shortcut… — (a) · built (second UI wave) · tested: search-everywhere-add-shortcut (pass)
- `action:CheckSuggestedPlugins` Check Suggested Plugins — (a) · built (second UI wave) · tested: plugins-check-suggested (pass)
- `action:CopySettingsPath` Copy Option Path — (a) · built (second UI wave) · tested: settings-copy-path (pass)
- `action:WelcomeScreen.Configure.Export` Export Settings — (a) · built as File ▸ Manage IDE Settings ▸ Export Settings… (second UI wave, settings-export-import), not ticked: the Welcome window has no Find Action
- `action:WelcomeScreen.Configure.Import` Import Settings… — (a) · built as File ▸ Manage IDE Settings ▸ Import Settings… (second UI wave, settings-export-import), not ticked: the Welcome window has no Find Action
- `action:plugin.InstallFromDiskAction` plugin.InstallFromDiskAction — (a) · built (second UI wave) · tested: settings-plugins-from-disk (pass)
- `action:OpenMouseWheelSmoothScrollSettings` Smooth Scroll Options — (a) · not built: Zed has no smooth scrolling to set

#### Markdown editing and preview — `markdown_preview`, `spork_editor` (28)

- `toolbar:Markdown.Toolbar.Floating` Markdown Editor Floating Toolbar — (a) · built (second UI wave) · tested: markdown-structure (pass)
- `toolbar:Markdown.Toolbar.Left` Markdown Editor Actions — (a) · nothing to build: IntelliJ's group is empty
- `toolbar:Markdown.Toolbar.Right` Markdown Editor Layout Actions — (a) · built (second UI wave) · tested: markdown-preview-sync (pass)
- `action:Markdown.GenerateTableOfContents` Generate Table Of Contents — (a) · built (editor and files wave) · tested: markdown-editing (pass)
- `action:org.intellij.plugins.markdown.ui.actions.styling.InsertImageAction` Insert Image — (a) · built (second UI wave) · tested: markdown-insert (pass)
- `action:Markdown.InsertEmptyTable` Insert Table — (a) · built (second UI wave) · tested: markdown-insert (pass)
- `action:Markdown.Preview.AdjustFontSize` Adjust Font Size… — (a) · built (second UI wave) · tested: markdown-preview-sync (pass)
- `action:Markdown.Table.SwapRows.SwapWithBelow` Move Row Down — (a) · built (editor and files wave) · tested: markdown-editing (pass)
- `action:Markdown.Table.SwapRows.SwapWithAbove` Move Row Up — (a) · built (editor and files wave) · tested: markdown-editing (pass)
- `action:Markdown.Table.SelectCurrentColumn.SelectContentCells` Select Column Cells — (a) · built (editor and files wave) · tested: markdown-editing (pass)
- `action:Markdown.Table.SelectRow` Select Row — (a) · built (editor and files wave) · tested: markdown-editing (pass)
- `action:org.intellij.plugins.markdown.ui.actions.styling.ToggleBoldAction` Bold — (a) · built (editor and files wave) · tested: markdown-editing (pass)
- `action:org.intellij.plugins.markdown.ui.actions.styling.ToggleCodeSpanAction` Code — (a) · built (editor and files wave) · tested: markdown-editing (pass)
- `action:Markdown.Styling.CreateLink` Create Link — (a) · built (editor and files wave) · tested: markdown-editing (pass)
- `action:Markdown.Styling.CreateOrChangeList` Create Or Change List — (a) · built (second UI wave) · tested: markdown-structure (pass)
- `action:org.intellij.plugins.markdown.ui.actions.styling.ToggleItalicAction` Italic — (a) · built (editor and files wave) · tested: markdown-editing (pass)
- `action:Markdown.Styling.SetHeaderLevel` Set Header Style — (a) · built (second UI wave) · tested: markdown-structure (pass)
- `action:org.intellij.plugins.markdown.ui.actions.styling.ToggleStrikethroughAction` Strikethrough — (a) · built (editor and files wave) · tested: markdown-editing (pass)
- `action:org.intellij.plugins.markdown.ui.actions.scrolling.AutoScrollAction` Sync Scroll — (a) · built (second UI wave) · tested: markdown-preview-sync (pass)
- `action:TextEditorWithPreview.Layout.EditorAndPreview` TextEditorWithPreview.Layout.EditorAndPreview — (a) · built (editor and files wave) · tested: markdown-preview-layout (pass)
- `action:TextEditorWithPreview.Layout.EditorOnly` TextEditorWithPreview.Layout.EditorOnly — (a) · built (editor and files wave) · tested: markdown-preview-layout (pass)
- `action:TextEditorWithPreview.Layout.PreviewOnly` TextEditorWithPreview.Layout.PreviewOnly — (a) · built (editor and files wave) · tested: markdown-preview-layout (pass)
- `action:org.intellij.plugins.markdown.ui.actions.styling.MarkdownIntroduceLinkReferenceAction` Convert to Reference — (a) · built (second UI wave) · tested: markdown-insert (pass)
- `action:org.intellij.plugins.markdown.ui.actions.styling.HeaderDownAction` Decrease Header Level — (a) · built (second UI wave) · tested: markdown-structure (pass)
- `action:Markdown.Preview.DecreaseFontSize` Decrease Preview Font Size — (a) · built (editor and files wave) · tested: markdown-preview-layout (pass)
- `action:org.intellij.plugins.markdown.ui.actions.styling.HeaderUpAction` Increase Header Level — (a) · built (second UI wave) · tested: markdown-structure (pass)
- `action:Markdown.Preview.IncreaseFontSize` Increase Preview Font Size — (a) · built (editor and files wave) · tested: markdown-preview-layout (pass)
- `action:Markdown.Preview.ResetFontSize` Reset Preview Font Size — (a) · built (editor and files wave) · tested: markdown-preview-layout (pass)

#### Terminal — `terminal_view`, `spork_terminal` (4)

Built (2026-11-23, the version control, run and terminal wave): Increase, Decrease and Reset Font Size, with the
Terminal tool window's IntelliJ header (its session tabs, New Tab, Rename Session, Close and Split from a tab's menu).
New Predefined Session lists and opens the shells found (`project` creates a terminal with a given shell), short of its
Settings item (the Settings dialog opens on a page only from its own crate).

- `action:TerminalNewPredefinedSession` New Predefined Session — (a)
- `action:TerminalDecreaseFontSize` Decrease Font Size — (a) · built (version control, run and terminal wave) · tested: terminal-tool-window (pass)
- `action:TerminalIncreaseFontSize` Increase Font Size — (a) · built (version control, run and terminal wave) · tested: terminal-tool-window (pass)
- `action:TerminalResetFontSize` Reset Font Size — (a) · built (version control, run and terminal wave) · tested: terminal-tool-window (pass)

#### Developer tools — `inspector_ui` (1)

- `action:UiDebugger` UI Debugger… — (a)

#### Menus — `zed` (app_menus) (1)

- `action:RestartIde` Restart the reference app… — (a)

#### Notifications — `spork_tool_windows` (notifications) (2)

- `action:ClearAllNotifications` Clear All Notifications — (a) · tested: frame-notifications-tool-window (pass)
- `action:MarkNotificationsAsRead` MarkNotificationsAsRead — (a) · built (second UI wave) · tested: notifications-mark-read (pass)

#### AI — edit predictions (`edit_prediction_ui`), `spork_editor` (2)

- `action:NextInlineCompletionSuggestionAction` Next Inline Proposal — (a) · bound (⌥], Zed's Next Edit Prediction), not ticked: testing it needs a prediction provider with several proposals
- `action:PrevInlineCompletionSuggestionAction` Previous Inline Proposal — (a) · bound (⌥[, Zed's Previous Edit Prediction), not ticked: testing it needs a prediction provider with several proposals

#### TODO tool window — `spork_navigate` (over the project search) (2)

- `action:TodoViewGroupByFlattenPackage` TodoViewGroupByFlattenPackage — (a) · built (editor and files wave) · tested: todo-tool-window (pass)
- `action:TodoViewGroupByShowPackages` TodoViewGroupByShowPackages — (a) · built (editor and files wave) · tested: todo-tool-window (pass)

#### HTML editing on tree-sitter — `spork_code` (7)

- `action:HtmlAddTableColumnAfter` Add a New Column to the Table After the Current One — (a) · tested: html-table-editing
- `action:HtmlAddTableColumnBefore` Add a New Column to the Table Before the Current One — (a) · tested: html-table-editing
- `action:HtmlTableCellNavigateRight` Navigate to Next Table Column — (a) · built (the Java and HTML wave) (html-table-editing), not ticked: Find Action writes it "Navigate To …" (its words from the action's name)
- `action:HtmlTableCellNavigateDown` Navigate to Next Table Row — (a) · built (the Java and HTML wave) (html-table-editing), not ticked: Find Action writes it "Navigate To …" (its words from the action's name)
- `action:HtmlTableCellNavigateLeft` Navigate to Previous Table Column — (a) · built (the Java and HTML wave) (html-table-editing), not ticked: Find Action writes it "Navigate To …" (its words from the action's name)
- `action:HtmlTableCellNavigateUp` Navigate to Previous Table Row — (a) · built (the Java and HTML wave) (html-table-editing), not ticked: Find Action writes it "Navigate To …" (its words from the action's name)
- `action:SurroundWithTagTemplate` Surround with Tag — (a) · built (the Java and HTML wave) (html-table-editing), not ticked: IntelliJ shows it in the floating code toolbar, which Spork IDE hasn't

### (b) Needs an engine or a decision first (194)

- `toolbar:FileChooserToolbar` File Chooser Toolbar — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `toolbar:Mq.Patches.Toolbar` Mq.Patches.Toolbar — (b) a decision on version control beyond git (Spork's engine is git's; Mercurial isn't decided)
- `toolbar:Perforce.Toolbar` Perforce.Toolbar — (b) a decision on version control beyond git (Perforce isn't decided)
- `toolbar:RemoteServersViewToolbar` Remote Servers — (b) remote servers and deployment (Services' Clouds): no engine
- `toolbar:RemoteServersViewToolbar.Top` RemoteServersViewToolbar.Top — (b) remote servers and deployment (Services' Clouds): no engine
- `toolbar:Svn.WorkingCopiesView.Toolbar` Svn.WorkingCopiesView.Toolbar — (b) a decision on version control beyond git (Subversion isn't decided)
- `context-menu:ExtractMethodToolWindow.TreePopup` ExtractMethodToolWindow.TreePopup — (b) IntelliJ's own Extract Method (its duplicates): the language servers' extract finds none
- `context-menu:Maven.RepositoryMenu` Maven.RepositoryMenu — (b) Maven repository indices: no engine indexes the local repository
- `context-menu:Mq.Patches.ContextMenu` Mq.Patches.ContextMenu — (b) a decision on version control beyond git (Spork's engine is git's; Mercurial isn't decided)
- `context-menu:TestTreePopupMenu` TestTreePopupMenu — (a) · built in part (test waves: Run, Debug, Jump to Source, Exclude from Suite; no Run with Coverage, Add to Pattern Suite or the diff's items) · tested: java-test-kinds
- `context-menu:ShowFilterPopup` ShowFilterPopup — (b) a syntax filter in Zed's search (in comments, in string literals): phase 4 left it
- `action:IntegrateFiles` Integrate — (b) a decision on version control beyond git: the action is Subversion's and Perforce's (git has no such operation)
- `action:CheckStatusForFiles` Check Status — (b) a decision on version control beyond git: the action is Subversion's and Perforce's (git has no such operation)
- `action:Compare.LastVersion` Compare with Latest Repository Version — (b) a decision on version control beyond git: the action is Subversion's and Perforce's (git has no such operation)
- `action:OpenCallWindowAction` Code With Me: Show Call Window — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:LeaveCallAction` Leave Call — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:ShowPermissionsAction` Code With Me: Permissions… — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:CopyJoinLinkAction` Copy Session Link — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:EnableVoiceCallAction` EnableVoiceCallAction — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:ShutdownCodeWithMe` End Session — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:GetJoinLinkAction` Start Code With Me Session — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:CWMOpenSettingsAction` Settings… — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:AddNewPropertyFile` AddNewPropertyFile — (b) Java property files' resource bundles: no engine
- `action:CreateResourceBundle` CreateResourceBundle — (b) Java resource bundles and i18n: no engine (JDT LS has none)
- `action:NewFxmlFile` FXML File — (b) a decision on Java frameworks beyond the JDK (JavaFX, FXML)
- `action:Gant.NewScript` Gant Script — (b) a Groovy engine (Decision 11 leaves Groovy and its language server to phase 10)
- `action:Groovy.NewClass` Groovy Class — (b) a Groovy engine (Decision 11 leaves Groovy and its language server to phase 10)
- `action:Groovy.NewScript` Groovy Script — (b) a Groovy engine (Decision 11 leaves Groovy and its language server to phase 10)
- `action:NewJavaFXApplication` JavaFX Application — (b) a decision on Java frameworks beyond the JDK (JavaFX, FXML)
- `action:NewModuleInGroup` Module — (b) IntelliJ's module model (modules, Project Structure, `.iml`): Spork IDE opens folders, JDT LS sees one project per build file
- `action:ExcludeFromValidation` Exclude from Validation — (b) JDT LS builds the workspace, not one file
- `action:Documentation.Back` Back — (b) the documentation popup's own navigation: the language servers' hover gives web links, not symbols to open in the popup
- `action:Documentation.Forward` Forward — (b) the documentation popup's own navigation: the language servers' hover gives web links, not symbols to open in the popup
- `action:EmmetPreview` Emmet Preview — (b) an Emmet engine: Zed's Emmet extension is a completion server, IntelliJ's edit points, preview and surround need Emmet itself
- `action:SurroundWithEmmet` Surround with Emmet — (b) an Emmet engine: Zed's Emmet extension is a completion server, IntelliJ's edit points, preview and surround need Emmet itself
- `action:AddGradleDslPluginAction` AddGradleDslPluginAction — (b) a Gradle build script model (Buildship gives JDT LS the classpath, not the DSL)
- `action:GenerateAfterClassMethod` AfterClass Method — (b) a test framework engine: JDT LS offers no test generation (JUnit's set-up, tear-down and test methods)
- `action:GenerateBeforeClassMethod` BeforeClass Method — (b) a test framework engine: JDT LS offers no test generation (JUnit's set-up, tear-down and test methods)
- `action:org.jetbrains.plugins.groovy.actions.generate.constructors.GroovyGenerateConstructorAction` Constructor — (b) a Groovy engine (Decision 11 leaves Groovy and its language server to phase 10)
- `action:org.jetbrains.plugins.groovy.actions.generate.equals.GroovyGenerateEqualsAction` equals() and hashCode() — (b) a Groovy engine (Decision 11 leaves Groovy and its language server to phase 10)
- `action:org.jetbrains.plugins.groovy.actions.generate.accessors.GroovyGenerateGetterAction` Getter — (b) a Groovy engine (Decision 11 leaves Groovy and its language server to phase 10)
- `action:org.jetbrains.plugins.groovy.actions.generate.accessors.GroovyGenerateGetterSetterAction` Getter and Setter — (b) a Groovy engine (Decision 11 leaves Groovy and its language server to phase 10)
- `action:Gradle.AddGradleDslDependencyAction` Gradle.AddGradleDslDependencyAction — (b) a Gradle build script model (Buildship gives JDT LS the classpath, not the DSL)
- `action:org.editorconfig.configmanagement.generate.EditorConfigGenerateLanguagePropertiesAction` Language Properties — (b) IntelliJ's own code style properties (`ij_*`) for .editorconfig: no engine
- `action:GenerateLogField` Logger — (b) no JDT LS action generates it
- `action:org.jetbrains.plugins.groovy.actions.generate.missing.GroovyGenerateMethodMissingAction` methodMissing() — (b) a Groovy engine (Decision 11 leaves Groovy and its language server to phase 10)
- `action:GenerateDataMethod` Parameters Method — (b) no JDT LS action generates it
- `action:org.jetbrains.plugins.groovy.actions.generate.missing.GroovyGeneratePropertyMissingAction` propertyMissing() — (b) a Groovy engine (Decision 11 leaves Groovy and its language server to phase 10)
- `action:GenerateProviderMethod` provider() — (b) no JDT LS action generates it
- `action:org.jetbrains.plugins.groovy.actions.generate.accessors.GroovyGenerateSetterAction` Setter — (b) a Groovy engine (Decision 11 leaves Groovy and its language server to phase 10)
- `action:GenerateSetUpMethod` SetUp Method — (b) a test framework engine: JDT LS offers no test generation (JUnit's set-up, tear-down and test methods)
- `action:GenerateSuperMethodCall` Super Method Call — (b) no JDT LS action generates it
- `action:GenerateTearDownMethod` TearDown Method — (b) a test framework engine: JDT LS offers no test generation (JUnit's set-up, tear-down and test methods)
- `action:GenerateTestMethod` Test Method — (b) a test framework engine: JDT LS offers no test generation (JUnit's set-up, tear-down and test methods)
- `action:org.jetbrains.plugins.groovy.actions.generate.tostring.GroovyGenerateToStringAction` toString() — (b) a Groovy engine (Decision 11 leaves Groovy and its language server to phase 10)
- `action:GenerateXmlTag` XML Tag… — (b) an XML engine (schemas, DTD, validation): Spork IDE has no XML language server yet; a decision (Eclipse's LemMinX would fit Decision 11's way)
- `action:DisableInspection` DisableInspection — (b) inspection profiles: the language servers have none (phase 7)
- `action:EditInspectionSettings` EditInspectionSettings — (b) inspection profiles: the language servers have none (phase 7)
- `action:Markdown.Preview.Find` Find… — (b) find in the Markdown preview: Zed's preview isn't searchable (no `SearchableItem`)
- `action:Maven.ViewDependencyAnalyzer` Maven.ViewDependencyAnalyzer — (b) Maven's resolved model (effective POM, transitive dependencies): phase 10 reads the POM alone
- `action:Maven.NavigatorDependencyAnalyzer` Maven.NavigatorDependencyAnalyzer — (b) Maven's resolved model (effective POM, transitive dependencies): phase 10 reads the POM alone
- `action:Maven.UpdateLocalIndex` Reindex repository — (b) Maven repository indices: no engine indexes the local repository
- `action:MemoryView.JumpToTypeSource` Jump To Type Source — (b) the debugger's memory view: java-debug's adapter reports no instance counts
- `action:MemoryView.ShowInstances` Show Instances — (b) the debugger's memory view: java-debug's adapter reports no instance counts
- `action:MemoryView.ShowNewInstances` Show New Instances — (b) the debugger's memory view: java-debug's adapter reports no instance counts
- `action:MemoryView.TrackingAction.NewInstancesTracking` Track New Instances — (b) the debugger's memory view: java-debug's adapter reports no instance counts
- `action:MemoryView.EnableTrackingWithClosedWindow` Enable Tracking With Hidden Memory View — (b) the debugger's memory view: java-debug's adapter reports no instance counts
- `action:MemoryView.ShowOnlyWithDiff` Show Non-Zero Diff Only — (b) the debugger's memory view: java-debug's adapter reports no instance counts
- `action:MemoryView.ShowOnlyTracked` Show Tracked Classes Only — (b) the debugger's memory view: java-debug's adapter reports no instance counts
- `action:MemoryView.ShowOnlyWithInstances` Show With Instances Only — (b) the debugger's memory view: java-debug's adapter reports no instance counts
- `action:MemoryView.SwitchUpdateMode` Update Loaded Classes On Debugger Stop — (b) the debugger's memory view: java-debug's adapter reports no instance counts
- `action:hg4idea.QDelete` Delete — (b) a decision on version control beyond git (Spork's engine is git's; Mercurial isn't decided)
- `action:hg4idea.QFold` Fold — (b) a decision on version control beyond git (Spork's engine is git's; Mercurial isn't decided)
- `action:hg4idea.QGotoFromPatches` Goto — (b) a decision on version control beyond git (Spork's engine is git's; Mercurial isn't decided)
- `action:hg4idea.QPushAction` Move and Push — (b) a decision on version control beyond git (Spork's engine is git's; Mercurial isn't decided)
- `action:hg4idea.QRefresh` Reload from file — (b) a decision on version control beyond git (Spork's engine is git's; Mercurial isn't decided)
- `action:ImportModule` Module from Existing Sources… — (b) IntelliJ's module model (modules, Project Structure, `.iml`): Spork IDE opens folders, JDT LS sees one project per build file
- `action:NewModule` Module… — (b) IntelliJ's module model (modules, Project Structure, `.iml`): Spork IDE opens folders, JDT LS sees one project per build file
- `action:ImportProject` Project from Existing Sources… — (b) IntelliJ's module model (modules, Project Structure, `.iml`): Spork IDE opens folders, JDT LS sees one project per build file
- `action:ShowSettingsWithAddedPattern` Disable Hints for Method — (b) a per-method exclusion list for parameter hints: Zed's inlay hints have none
- `action:ToggleCompletionHintsAction` Show Parameter Name Hints on Completion — (b) parameter name hints during completion: Zed's inlay hints don't follow the lookup
- `action:NavigateInBrowser` NavigateInBrowser — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:StopForwardingAction` StopForwardingAction — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:ForwardPortAction` ForwardPortAction — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:ForwardPortAndNavigateInBrowser` ForwardPortAndNavigateInBrowser — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:CombinePropertiesFilesAction` CombinePropertiesFilesAction — (b) Java resource bundles and i18n: no engine (JDT LS has none)
- `action:DissociateResourceBundleAction` Dissociate Resource Bundle — (b) Java resource bundles and i18n: no engine (JDT LS has none)
- `action:Gradle.ProjectViewDependencyAnalyzer` Gradle.ProjectViewDependencyAnalyzer — (b) a Gradle build script model (Buildship gives JDT LS the classpath, not the DSL)
- `action:Maven.ProjectViewDependencyAnalyzer` Maven.ProjectViewDependencyAnalyzer — (b) Maven's resolved model (effective POM, transitive dependencies): phase 10 reads the POM alone
- `action:SearchContext.Anywhere` SearchContext.Anywhere — (b) a syntax filter in Zed's search (in comments, in string literals): phase 4 left it
- `action:SearchContext.ExceptComments` SearchContext.ExceptComments — (b) a syntax filter in Zed's search (in comments, in string literals): phase 4 left it
- `action:SearchContext.ExceptCommentsAndStringLiterals` SearchContext.ExceptCommentsAndStringLiterals — (b) a syntax filter in Zed's search (in comments, in string literals): phase 4 left it
- `action:SearchContext.ExceptStringLiterals` SearchContext.ExceptStringLiterals — (b) a syntax filter in Zed's search (in comments, in string literals): phase 4 left it
- `action:SearchContext.InComments` SearchContext.InComments — (b) a syntax filter in Zed's search (in comments, in string literals): phase 4 left it
- `action:SearchContext.InStringLiterals` SearchContext.InStringLiterals — (b) a syntax filter in Zed's search (in comments, in string literals): phase 4 left it
- `action:Svn.RefreshWorkingCopies` Refresh — (b) a decision on version control beyond git (Subversion isn't decided)
- `action:excludeFromSuite` Exclude from Suite — (a) · built (test wave, round 2: a class of a Pattern configuration's patterns; a method's pattern is left) · tested: java-test-kinds
- `action:excludeFromTestNGSuite` Exclude from Suite — (a) the test runner engine exists (test wave): not built yet
- `action:openAssertEqualsDiff` View assertEquals Difference — (a) · built (test wave: the failure's "Click to see difference"; its ⌘D action is left) · tested: java-test-runner (pass)
- `action:CloseGotItTooltip` Close Got It Tooltip — (b) Got It tooltips (IntelliJ's feature hints): Spork IDE shows none yet
- `action:CommitView.CloseWindowedFloatingTwOnCommit` Close the Commit Tool Window in Window/Float Mode After Committing — (b) floating and windowed tool windows: Zed's docks can't leave the project window
- `action:OpenPortForwardingToolwindowAction` Code With Me: Show Port Forwarding Window — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:CodeVision.ShowMore` CodeVision.ShowMore — (b) code vision (IntelliJ's usages and author inlays): Zed draws the servers' code lenses as code actions only
- `action:ConfigureInspectionsAction` Configure Inspections… — (b) inspection profiles: the language servers have none (phase 7)
- `action:RemoteServers.ConnectServer` Connect — (b) remote servers and deployment (Services' Clouds): no engine
- `action:ContextDebug` ContextDebug — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:ContextRun` ContextRun — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:ToggleCreateChangeListForGuestChanges` Create Changelists for Guest Changes — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:RemoteServers.ChooseServerDeploymentWithDebug` Debug — (b) remote servers and deployment (Services' Clouds): no engine
- `action:Servers.DeployWithDebug` Debug — (b) remote servers and deployment (Services' Clouds): no engine
- `action:FileChooser.Delete` Delete… — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `action:RemoteServers.ChooseServerDeployment` Deploy — (b) remote servers and deployment (Services' Clouds): no engine
- `action:FileChooser.GotoDesktop` Desktop Directory — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `action:FileChooser.ToggleDetection` Detect Project Directories — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `action:RemoteServers.DisconnectServer` Disconnect — (b) remote servers and deployment (Services' Clouds): no engine
- `action:DownloadJdkAction` Download JDK… — (a) · tested: java-jdk-download (the JDK wave: IntelliJ's dialog on the Foojay Disco API, in the banner's Setup SDK, Project Structure's Add SDK and SDKs page, and the Application configuration's JRE field)
- `action:RemoteServers.EditDeploymentConfig` Edit Configuration — (b) remote servers and deployment (Services' Clouds): no engine
- `action:RemoteServers.EditServerConfig` Edit Configuration — (b) remote servers and deployment (Services' Clouds): no engine
- `action:EditPropertyValue` Edit Property Value… — (b) Java resource bundles and i18n: no engine (JDT LS has none)
- `action:ExportTestResults` Export Test Results… — (a) · built (test wave: IntelliJ's XML into the project's folder, without the export dialog) · tested: java-test-runner (pass)
- `action:FixWSLFirewall` Fix WSL Firewall Settings — (b) WSL, on Windows only: a decision on Windows support
- `action:FloatMode` Float — (b) floating and windowed tool windows: Zed's docks can't leave the project window
- `action:ToggleFloatingMode` Floating Mode — (b) floating and windowed tool windows: Zed's docks can't leave the project window
- `action:UniversalFileChooser.FocusPath` Focus Path Field — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `action:SingleUserFollowAction` Follow single user — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:FileChooser.GotoHome` Home Directory — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `action:FileChooser.GoToWslHome` Home Directory in WSL — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `action:ImportCoverage` Import External Coverage Report… — (b) a coverage engine (built in the coverage wave, 2026-11-26: the task left is buildable now; Manage Coverage Reports' Add reads an agent's `.ic`)
- `action:JumpToColorsAndFonts` Jump to Colors and Fonts — (b) colour scheme editing (Settings › Editor › Color Scheme's options): phase 9b left it
- `action:SingleJumpToCursorAction` Jump to cursor — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:ForceOthersToFollowAction` Make Everyone Follow You — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:FileChooser.GotoModule` Module Directory — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `action:ModuleSettings` Module Settings — (b) IntelliJ's module model (modules, Project Structure, `.iml`): Spork IDE opens folders, JDT LS sees one project per build file
- `action:MoveMinimap` MoveMinimap — (b) Zed's minimap has no side setting
- `action:NonModalWelcomeScreen.LeftTabActions.New.Action` New — (b) IntelliJ's non-modal welcome screen (2025.3+, the reference app's): a decision whether Spork IDE's Welcome window becomes it
- `action:FileChooser.NewFolder` New Directory… — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `action:FileChooser.NewFile` New File… — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `action:FileChooser.GoForward` Next Directory — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `action:NextLessonAction` Next Lesson — (b) the Learn courses' content (phase 6 left the Welcome window's Learn): a decision
- `action:CWMHostOpenSettingsAction` Open Code With Me Settings — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:OpenElementInNewWindow` Open in New Editor Window — (b) editor windows: Zed's panes can't open an item in a window of its own within the same project
- `action:WelcomeReopenProjectAction` Open Welcome Project — (b) IntelliJ's non-modal welcome screen (2025.3+, the reference app's): a decision whether Spork IDE's Welcome window becomes it
- `action:FileChooser.GoToParent` Parent Directory — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `action:NonModalWindow.PinUnpin` Pin Window — (b) IntelliJ's non-modal welcome screen (2025.3+, the reference app's): a decision whether Spork IDE's Welcome window becomes it
- `action:FileChooser.GoBackward` Previous Directory — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `action:PreviousLessonAction` Previous Lesson — (b) the Learn courses' content (phase 6 left the Welcome window's Learn): a decision
- `action:FileChooser.GotoProject` Project Directory — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `action:ShowRecentTests` Recent Tests — (a) the test runner engine exists (test waves): not built yet (round 2 did the test history, Import Tests from File… and Rerun Automatically)
- `action:FileChooser.Refresh` Refresh — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `action:RerunFailedTests` Rerun Failed Tests — (a) · built (test wave) · tested: java-test-runner (pass)
- `action:RerunTests` Rerun Tests — (a) · built (test wave) · tested: java-test-runner (pass)
- `action:ResetLearningProgressAction` Reset Learning Progress — (b) the Learn courses' content (phase 6 left the Welcome window's Learn): a decision
- `action:ResetPluginPermissions` Reset Plugin Permissions… — (b) IntelliJ's plugin permissions: Zed's extensions have capabilities in settings, not Allow Always decisions; a decision
- `action:RestartLessonAction` Restart Lesson — (b) the Learn courses' content (phase 6 left the Welcome window's Learn): a decision
- `action:FileChooser.GoToRoot` Root Directory — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `action:NextEditorTab` Select Next Tab in Multi-Editor File — (b) multi-editor files (IntelliJ's editors with tabs at the bottom, such as GUI forms): none in Spork IDE
- `action:FileChooser.LightEditGotoOpenedFile` Select Opened File — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `action:PreviousEditorTab` Select Previous Tab in Multi-Editor File — (b) multi-editor files (IntelliJ's editors with tabs at the bottom, such as GUI forms): none in Spork IDE
- `action:SeverityEditorDialog` SeverityEditorDialog — (b) inspection profiles: the language servers have none (phase 7)
- `action:MainMenuButton.ShowMenu` Show Collapsed Main Menu — (b) the collapsed main menu is Linux's and Windows' (macOS has the system menu bar): a decision on those systems
- `action:FileChooser.ShowHidden` Show Hidden Files and Directories — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `action:UsageFiltering.Imports` Show Import Statements — (b) usage kinds: LSP references have no read, write or import kind (only document highlights tell read from write)
- `action:Subversion.TogglePropertiesDiff` Show Properties — (b) a decision on version control beyond git (Subversion isn't decided)
- `action:UsageFiltering.ReadAccess` Show Read Access — (b) usage kinds: LSP references have no read, write or import kind (only document highlights tell read from write)
- `action:UsageFiltering.GeneratedCode` Show Results In Generated Code — (b) knowing generated code: no language server marks it
- `action:UsageFiltering.Comments` Show Usages in Comments — (b) a syntax filter in Zed's search (in comments, in string literals): phase 4 left it
- `action:Svn.Show.Working.Copies` Show Working Copies — (b) a decision on version control beyond git (Subversion isn't decided)
- `action:UsageFiltering.WriteAccess` Show Write Access — (b) usage kinds: LSP references have no read, write or import kind (only document highlights tell read from write)
- `action:FileChooser.TogglePathBar` Show/Hide Path Input Field — (b) a decision: IntelliJ uses the system's file dialog by default (Advanced Settings › Use native file chooser dialog, `ide.ui.native.file.chooser`, on), as Spork IDE does; its own File Chooser shows only with that setting off
- `action:ToggleSideMode` Split Mode — (b) split tool windows: Zed's docks show one panel at a time, IntelliJ's split mode needs two per side
- `action:SurroundWithLiveTemplate` Surround with Live Template… — (b) live templates: Zed's snippets have no surround (selected text) variable
- `action:TodoViewGroupByShowModules` TodoViewGroupByShowModules — (b) IntelliJ's module model (modules, Project Structure, `.iml`): Spork IDE opens folders, JDT LS sees one project per build file
- `action:StatusBarWidgets.Toggle.CwmWireStatsWidgetFactory` Toggle 'Code With Me Wire Stats' Widget — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:StatusBarWidgets.Toggle.InsertOverwrite` Toggle 'Editor Selection Mode' Widget — (b) an overwrite mode: Zed's editor has none (only Vim's replace mode)
- `action:StatusBarWidgets.Toggle.IncomingChanges` Toggle 'Incoming Changes' Widget — (b) incoming changes: Subversion's and Mercurial's, not git's
- `action:StatusBarWidgets.Toggle.InspectionProfile` Toggle 'Inspection Profile' Widget — (b) inspection profiles: the language servers have none (phase 7)
- `action:StatusBarWidgets.Toggle.JSONSchemaSelector` Toggle 'JSON Schema' Widget — (b) the JSON schema in use: Zed's JSON language server picks it and tells no client
- `action:StatusBarWidgets.Toggle.hg` Toggle 'Mercurial Branch' Widget — (b) a decision on version control beyond git (Spork's engine is git's; Mercurial isn't decided)
- `action:StatusBarWidgets.Toggle.InHgIncomingOutgoingWidget` Toggle 'Mercurial Incoming' Widget — (b) a decision on version control beyond git (Spork's engine is git's; Mercurial isn't decided)
- `action:StatusBarWidgets.Toggle.OutHgIncomingOutgoingWidget` Toggle 'Mercurial Outgoing' Widget — (b) a decision on version control beyond git (Spork's engine is git's; Mercurial isn't decided)
- `action:StatusBarWidgets.Toggle.PowerSaveMode` Toggle 'Power Save Mode' Widget — (b) Power Save Mode: Zed has no switch for its background work
- `action:CodeVisionToggleAction` Toggle Code Vision for Project — (b) code vision (IntelliJ's usages and author inlays): Zed draws the servers' code lenses as code actions only
- `action:ToggleRenderedDocPresentation` Toggle Rendered View — (b) rendered doc comments: Zed's editor has no rendered view of comments
- `action:UndockMode` Undock — (b) sliding (undocked) tool windows: Zed's docks take their space, IntelliJ's undocked ones draw over the editor
- `action:CurrentLeadUnfollowAction` Unfollow current lead — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:EscUnfollowUserAction` Unfollow current lead — (b) our own collaboration server first (Decision 2: Zed's collaboration comes back on it)
- `action:UsageGrouping.FlattenModules` UsageGrouping.FlattenModules — (b) IntelliJ's module model (modules, Project Structure, `.iml`): Spork IDE opens folders, JDT LS sees one project per build file
- `action:UsageGrouping.Module` UsageGrouping.Module — (b) IntelliJ's module model (modules, Project Structure, `.iml`): Spork IDE opens folders, JDT LS sees one project per build file
- `action:UsageGrouping.UsageType` UsageGrouping.UsageType — (b) usage kinds: LSP references have no read, write or import kind (only document highlights tell read from write)
- `action:TopStripeEnableAction` Use Single Tool Window Bar — (b) a decision on what Use Single Tool Window Bar does in Spork IDE's frame (`EnableStripeGroup`)
- `action:WebOpenInAction` Web Preview… — (b) a built-in web server and browser preview: no engine
- `action:WelcomeScreen.Configure.ProjectStructure` WelcomeScreen.Configure.ProjectStructure — (b) IntelliJ's module model (modules, Project Structure, `.iml`): Spork IDE opens folders, JDT LS sees one project per build file
- `action:WindowMode` Window — (b) floating and windowed tool windows: Zed's docks can't leave the project window
- `action:ToggleWindowedMode` Windowed Mode — (b) floating and windowed tool windows: Zed's docks can't leave the project window

### (c) Out of scope (259)

- `toolbar:DS.Tables.Left.Toolbar` DS.Tables.Left.Toolbar — (c) notebooks and data tables (module 2)
- `toolbar:DS.Tables.Left.Toolbar.Short` DS.Tables.Left.Toolbar.Short — (c) notebooks and data tables (module 2)
- `toolbar:DS.Tables.Right.Toolbar` DS.Tables.Right.Toolbar — (c) notebooks and data tables (module 2)
- `toolbar:Jupyter.AboveCodeCellAdditionalToolbar` Code Cell Toolbar — (c) notebooks and data tables (module 2)
- `toolbar:Jupyter.AboveCodeCellAdditionalToolbar.Ellipsis` Code Cell Toolbar More — (c) notebooks and data tables (module 2)
- `toolbar:Jupyter.AboveMarkdownCellAdditionalToolbar` Jupyter.AboveMarkdownCellAdditionalToolbar — (c) notebooks and data tables (module 2)
- `toolbar:Jupyter.AboveMarkdownCellAdditionalToolbar.Ellipsis` Jupyter.AboveMarkdownCellAdditionalToolbar.Ellipsis — (c) notebooks and data tables (module 2)
- `toolbar:JupyterCellToolbarLeft` JupyterCellToolbarLeft — (c) notebooks and data tables (module 2)
- `toolbar:JupyterCellToolbarLeftEllipsis` JupyterCellToolbarLeftEllipsis — (c) notebooks and data tables (module 2)
- `toolbar:JupyterCodeCellToolbarCustomizeActionsGroup` Jupyter Cell Toolbar — (c) notebooks and data tables (module 2)
- `toolbar:JupyterCustomizeToolbarGroup` Customize Toolbar — (c) notebooks and data tables (module 2)
- `toolbar:JupyterEditorLeftToolbarActions` Left — (c) notebooks and data tables (module 2)
- `toolbar:JupyterEditorLeftToolbarCustomizeActionsGroup` Jupyter Editor Toolbar — (c) notebooks and data tables (module 2)
- `toolbar:JupyterEditorRightToolbarActions` JupyterEditorRightToolbarActions — (c) notebooks and data tables (module 2)
- `toolbar:JupyterEditorRightToolbarAdditionalActions` More — (c) notebooks and data tables (module 2)
- `toolbar:JupyterToolbar.AddActionsPopup` JupyterToolbar.AddActionsPopup — (c) notebooks and data tables (module 2)
- `toolbar:JupyterToolbarQuickActions` JupyterToolbarQuickActions — (c) notebooks and data tables (module 2)
- `toolbar:JupyterToolbarQuickActionsRaw` JupyterToolbarQuickActionsRaw — (c) notebooks and data tables (module 2)
- `toolbar:KT.Tables.Right.Toolbar` KT.Tables.Right.Toolbar — (c) notebooks and data tables (module 2)
- `toolbar:PackageChecker.ProblemTab.ToolBar` PackageChecker.ProblemTab.ToolBar — (c) Package Checker, a JetBrains vulnerability service
- `toolbar:PackageChecker.ProblemTab.ToolBar.ShowSafe` PackageChecker.ProblemTab.ToolBar.ShowSafe — (c) Package Checker, a JetBrains vulnerability service
- `toolbar:PY.Tables.Right.Toolbar` Right — (c) notebooks and data tables (module 2)
- `toolbar:PY.Tables.Right.Toolbar.Customize.Actions.Group` Tables Toolbar — (c) notebooks and data tables (module 2)
- `toolbar:PY.Tables.Right.Toolbar.MoreActionsGroup` PY.Tables.Right.Toolbar.MoreActionsGroup — (c) notebooks and data tables (module 2)
- `toolbar:PY.Tables.Right.Toolbar.Static` PY.Tables.Right.Toolbar.Static — (c) notebooks and data tables (module 2)
- `context-menu:JupyterToolbar.AddActionsPopup` JupyterToolbar.AddActionsPopup — (c) notebooks and data tables (module 2)
- `context-menu:PackageChecker.Toolwindow.Tree.Popup` PackageChecker.Toolwindow.Tree.Popup — (c) Package Checker, a JetBrains vulnerability service
- `action:AIHub.NotInstalled.DiscoverFeatures` Discover Features… — (c) JetBrains AI promotion (Decision 5: AI is the user's own providers)
- `action:AIHub.NotInstalled.InlineCompletionSettings` Settings… — (c) JetBrains AI promotion (Decision 5: AI is the user's own providers)
- `action:LearnMore` About Code With Me — (c) JetBrains licensing and services
- `action:CWMManageLicense` Manage License… — (c) JetBrains licensing and services
- `action:CodeWithMeBackendReportIssue` Submit an Issue… — (c) JetBrains licensing and services
- `action:Qodana.NewInspectionKts` Custom Inspection — (c) Qodana, a JetBrains service
- `action:Kotlin.NewFile` Kotlin Class/File — (a) · built (the Kotlin wave) · tested: kotlin-new-file (pass)
- `action:NewKotlinNotebookAction` Kotlin Notebook — (c) notebooks and data tables (module 2)
- `action:Kotlin.NewScript` Kotlin Script — (c) Kotlin (no engine chosen, left out with notebooks and database tools)
- `action:DS.DimensionsWidget` Show Table Dimensions — (c) notebooks and data tables (module 2)
- `action:DomCollectionControl.Add` Add — (c) IntelliJ plugin development (DevKit)
- `action:DomCollectionControl.Edit` Edit — (c) IntelliJ plugin development (DevKit)
- `action:DomCollectionControl.Remove` Remove — (c) IntelliJ plugin development (DevKit)
- `action:GenerateTestDataPathFiles` @TestDataPath Files — (c) IntelliJ plugin development (DevKit)
- `action:GenerateTestDataPathMethods` @TestDataPath Methods — (c) IntelliJ plugin development (DevKit)
- `action:GenerateCreateUI` createUI(…) — (c) IntelliJ plugin development (DevKit)
- `action:KotlinGenerateEqualsAndHashCode` equals() and hashCode() — (b) the Kotlin language server offers no Generate action (271.2085.0)
- `action:KotlinGenerateMavenCompileExecutionAction` Kotlin compile execution — (c) Kotlin (no engine chosen, left out with notebooks and database tools)
- `action:KotlinGenerateMavenPluginAction` Kotlin plugin — (c) Kotlin (no engine chosen, left out with notebooks and database tools)
- `action:KotlinGenerateMavenTestCompileExecutionAction` Kotlin test-compile execution — (c) Kotlin (no engine chosen, left out with notebooks and database tools)
- `action:KotlinGenerateDataMethod` Parameters Function — (b) the Kotlin language server offers no Generate action (271.2085.0)
- `action:KotlinGenerateSecondaryConstructor` Secondary Constructor — (b) the Kotlin language server offers no Generate action (271.2085.0)
- `action:KotlinGenerateSetUpMethod` SetUp Function — (b) the Kotlin language server offers no Generate action (271.2085.0)
- `action:KotlinGenerateTearDownMethod` TearDown Function — (b) the Kotlin language server offers no Generate action (271.2085.0)
- `action:KotlinGenerateTestMethod` Test Function — (b) the Kotlin language server offers no Generate action (271.2085.0)
- `action:KotlinGenerateToString` toString() — (b) the Kotlin language server offers no Generate action (271.2085.0)
- `action:JupyterMarkdownToggleCellStateAction` JupyterMarkdownToggleCellStateAction — (c) notebooks and data tables (module 2)
- `action:AddCellPromoAction` AI — (c) JetBrains AI promotion (Decision 5: AI is the user's own providers)
- `action:NotebookInsertCodeCellAction` Code — (c) notebooks and data tables (module 2)
- `action:NotebookInsertMarkdownCellAction` Markdown — (c) notebooks and data tables (module 2)
- `action:JupyterCellAddTagInlayAction` Add Cell Tag — (c) notebooks and data tables (module 2)
- `action:JupyterCellRemoveTagAction` Remove Tag — (c) notebooks and data tables (module 2)
- `action:JupyterConvertDataImportToCodeAction` JupyterConvertDataImportToCodeAction — (c) notebooks and data tables (module 2)
- `action:JupyterCustomizeCellFloatingToolbarAction` Customize Cell Toolbar… — (c) notebooks and data tables (module 2)
- `action:JupyterExpandSelectionToCellAboveCommandModeAction` JupyterExpandSelectionToCellAboveCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterExpandSelectionToCellBelowCommandModeAction` JupyterExpandSelectionToCellBelowCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterNotebookCollapseOutputsCommandModeAction` JupyterNotebookCollapseOutputsCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterNotebookConvertToCodeCellCommandModeAction` JupyterNotebookConvertToCodeCellCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterNotebookConvertToMarkdownCellCommandModeAction` JupyterNotebookConvertToMarkdownCellCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterNotebookCopyCellCommandModeAction` JupyterNotebookCopyCellCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterNotebookCutCellCommandModeAction` JupyterNotebookCutCellCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterNotebookDeleteCellCommandModeAction` JupyterNotebookDeleteCellCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterNotebookInsertCellAboveCommandModeAction` JupyterNotebookInsertCellAboveCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterNotebookInsertCellBelowCommandModeAction` JupyterNotebookInsertCellBelowCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterNotebookInterruptKernelCommandModeAction` JupyterNotebookInterruptKernelCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterNotebookMergeSelectedCellsCommandModeAction` JupyterNotebookMergeSelectedCellsCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterNotebookMoveCellDownCommandModeAction` JupyterNotebookMoveCellDownCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterNotebookMoveCellUpCommandModeAction` JupyterNotebookMoveCellUpCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterNotebookPasteCellAboveCommandModeAction` JupyterNotebookPasteCellAboveCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterNotebookPasteCellCommandModeAction` JupyterNotebookPasteCellCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterNotebookRunCellInsertBelowCommandModeAction` JupyterNotebookRunCellInsertBelowCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterSelectCellAboveCommandModeAction` JupyterSelectCellAboveCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterSelectCellBelowCommandModeAction` JupyterSelectCellBelowCommandModeAction — (c) notebooks and data tables (module 2)
- `action:JupyterTurnCommandModeOnAction` Turn Command Mode On — (c) notebooks and data tables (module 2)
- `action:JupyterTurnEditModeOnAction` Turn Edit Mode On — (c) notebooks and data tables (module 2)
- `action:NotebookConvertToCodeCellActionToolbar` Convert Cell to Code — (c) notebooks and data tables (module 2)
- `action:NotebookConvertToMarkdownCellActionToolbar` Convert Cell to Markdown — (c) notebooks and data tables (module 2)
- `action:JupyterConvertToRawCellAction` Convert Cell to Raw NBConvert — (c) notebooks and data tables (module 2)
- `action:JupyterConvertToSqlCellActionToolbar` Convert Cell to SQL — (c) notebooks and data tables (module 2)
- `action:NotebookCopyCellAction` Copy Cell — (c) notebooks and data tables (module 2)
- `action:NotebookCutCellAction` Cut Cell — (c) notebooks and data tables (module 2)
- `action:NotebookMergeSelectedCellsAction` Merge Selected Cells — (c) notebooks and data tables (module 2)
- `action:NotebookPasteCellAboveAction` Paste Above Cell — (c) notebooks and data tables (module 2)
- `action:NotebookPasteCellAction` Paste Below Cell — (c) notebooks and data tables (module 2)
- `action:ToggleReadOnlyAction` ToggleReadOnlyAction — (c) notebooks and data tables (module 2)
- `action:ToggleShowExecutionCount` Show Execution Count — (c) notebooks and data tables (module 2)
- `action:NotebookInsertCodeCellActionPlusIcon` Code — (c) notebooks and data tables (module 2)
- `action:JupyterNotebookGithubCreateGistAction` Create Gist… — (c) notebooks and data tables (module 2)
- `action:JupyterInterruptKernelAction` Interrupt Kernel — (c) notebooks and data tables (module 2)
- `action:NotebookMoveCellDownAction` Move Cell Down — (c) notebooks and data tables (module 2)
- `action:NotebookMoveCellUpAction` Move Cell Up — (c) notebooks and data tables (module 2)
- `action:JupyterRestartKernelAction` Restart Kernel — (c) notebooks and data tables (module 2)
- `action:NotebookRunAllAction` Run All — (c) notebooks and data tables (module 2)
- `action:NotebookRunCellSelectBelowAction` Run Cell and Select Below — (c) notebooks and data tables (module 2)
- `action:JupyterCellAddTagAction` Add Cell Tag — (c) notebooks and data tables (module 2)
- `action:EditCellPromoAction` AI Edit Cell — (c) JetBrains AI promotion (Decision 5: AI is the user's own providers)
- `action:FixCellPromoAction` AI Fix Cell — (c) JetBrains AI promotion (Decision 5: AI is the user's own providers)
- `action:NotebookConvertToMarkdownCellAction` Convert Cell to Markdown — (c) notebooks and data tables (module 2)
- `action:NotebookRunCellAction` Run Cell — (c) notebooks and data tables (module 2)
- `action:NotebookSplitCellAction` Split Cell — (c) notebooks and data tables (module 2)
- `action:NotebookEditorGotoRunCellAction` Go To — (c) notebooks and data tables (module 2)
- `action:ShowKotlinNotebookPreferencesAction` Kotlin Notebook Plugin Settings… — (c) notebooks and data tables (module 2)
- `action:KotlinNotebookDependenciesComboBoxAction` KotlinNotebookDependenciesComboBoxAction — (c) notebooks and data tables (module 2)
- `action:KotlinNotebookSessionModeComboBox` KotlinNotebookSessionModeComboBox — (c) notebooks and data tables (module 2)
- `action:JupyterTrustAction` Trust Notebook — (c) notebooks and data tables (module 2)
- `action:JupyterOpenSettingsAction` Open Jupyter Settings… — (c) notebooks and data tables (module 2)
- `action:JupyterOpenNotebookInBrowserAction` Open Notebook in Browser — (c) notebooks and data tables (module 2)
- `action:JupyterCustomizeToolbarAction` Customize Jupyter Toolbar… — (c) notebooks and data tables (module 2)
- `action:JupyterInvertImageOutputSettingsAction` Configure Inversion… — (c) notebooks and data tables (module 2)
- `action:JupyterCopyImageOutputAction` Copy — (c) notebooks and data tables (module 2)
- `action:JupyterFitImageOutputAction` Fit Image to Editor Width — (c) notebooks and data tables (module 2)
- `action:JupyterInvertImageOutputAction` Invert Image — (c) notebooks and data tables (module 2)
- `action:JupyterSaveImageOutputAsAction` Save As… — (c) notebooks and data tables (module 2)
- `action:NotebookConvertToCodeCellAction` Convert Cell to Code — (c) notebooks and data tables (module 2)
- `action:JupyterConvertToSqlCellAction` Convert Cell to SQL — (c) notebooks and data tables (module 2)
- `action:NotebookDeleteCellAction` Delete Cell — (c) notebooks and data tables (module 2)
- `action:NotebookDuplicateCellBelowAction` Duplicate Cell Below — (c) notebooks and data tables (module 2)
- `action:NotebookMergeCellAboveAction` Merge Cell Above — (c) notebooks and data tables (module 2)
- `action:NotebookMergeCellBelowAction` Merge Cell Below — (c) notebooks and data tables (module 2)
- `action:NotebookRunAllAboveAction` Run Above Cells — (c) notebooks and data tables (module 2)
- `action:NotebookRunAllBelowAction` Run Cell and Below — (c) notebooks and data tables (module 2)
- `action:JupyterErrorOutputColoringAction` ASCII Coloring In Error Text Output — (c) notebooks and data tables (module 2)
- `action:JupyterClearOutputsAction` Clear All Outputs — (c) notebooks and data tables (module 2)
- `action:JupyterClearCellOutputAction` Clear Output — (c) notebooks and data tables (module 2)
- `action:JupyterCopyCellOutputAction` Copy Output — (c) notebooks and data tables (module 2)
- `action:JupyterOpenTextOutputInNewTabAction` Open Output In New Tab — (c) notebooks and data tables (module 2)
- `action:NotebookResetCellOutputSizeAction` Reset Output Size — (c) notebooks and data tables (module 2)
- `action:JupyterSaveTextOutputAsAction` Save As… — (c) notebooks and data tables (module 2)
- `action:JupyterEnableCellOutputScrollingAction` Scrolling for Outputs — (c) notebooks and data tables (module 2)
- `action:NotebookInsertDataInputCellAction` Data Import — (c) notebooks and data tables (module 2)
- `action:NotebookInsertRawCellAction` Raw NBConvert — (c) notebooks and data tables (module 2)
- `action:DS.OpenResultInNewTab` Open In New Tab — (c) notebooks and data tables (module 2)
- `action:CopyNotebookPathAction` Copy Path — (c) notebooks and data tables (module 2)
- `action:OpenSelectedNotebookAction` Open Selected — (c) notebooks and data tables (module 2)
- `action:RemoveNotebookFromListAction` Remove from Recent Notebooks — (c) notebooks and data tables (module 2)
- `action:RevealNotebookInFinderAction` Show in Explorer — (c) notebooks and data tables (module 2)
- `action:KotlinNotebookEnableAttachedProcessMode` Attach to Running Kernel — (c) notebooks and data tables (module 2)
- `action:KotlinNotebookEnableIdeProcessMode` Run in IDE Process — (c) notebooks and data tables (module 2)
- `action:KotlinNotebookEnableSeparateProcessMode` Run in Separate Process — (c) notebooks and data tables (module 2)
- `action:CopyLetsPlot` Copy Plot — (c) notebooks and data tables (module 2)
- `action:SaveAllLetsPlots` Save All Plots From This Output… — (c) notebooks and data tables (module 2)
- `action:SaveLetsPlot` Save Plot As… — (c) notebooks and data tables (module 2)
- `action:DumpMLCompletionFeatures` Copy ML Completion Features To Clipboard — (c) JetBrains' machine-learning completion ranking
- `action:DumpLookupElementWeights` Dump Lookup Element Weights to Log — (c) IntelliJ's internal maintenance and diagnostics
- `action:ShowExperiments` Experimental Features… — (c) IntelliJ's internal maintenance and diagnostics
- `action:IdeScriptingConsole` IDE Scripting Console — (c) IntelliJ's internal maintenance and diagnostics
- `action:ShowRegistry` Registry… — (c) IntelliJ's internal maintenance and diagnostics
- `action:NotebookOutputCollapseAllInCellAction` Collapse All Cell Outputs — (c) notebooks and data tables (module 2)
- `action:NotebookOutputCollapseAllAction` Collapse All Notebook Outputs — (c) notebooks and data tables (module 2)
- `action:NotebookOutputCollapseAllInSelectedCellsAction` Collapse Outputs of Selected Cells — (c) notebooks and data tables (module 2)
- `action:NotebookOutputCollapseSingleInCellAction` Collapse Selected Output — (c) notebooks and data tables (module 2)
- `action:DSCustomizeTablesRightToolbarAction` Customize Tables Toolbar… — (c) notebooks and data tables (module 2)
- `action:DS.Console.TableResult.DSShowDumpDialogGrid` DS.Console.TableResult.DSShowDumpDialogGrid — (c) notebooks and data tables (module 2)
- `action:DS.TableFiltering.OpenFilterView` Open Filter View — (c) notebooks and data tables (module 2)
- `action:DataViewOpenTableDocumentationAction` Open Table Documentation… — (c) notebooks and data tables (module 2)
- `action:StatisticsPanel.Show.Compact` Compact — (c) the database grid's data tables (module 2)
- `action:StatisticsPanel.Show.Detailed` Detailed — (c) the database grid's data tables (module 2)
- `action:StatisticsPanel.Show.Off` Off — (c) the database grid's data tables (module 2)
- `action:DS.Open.Tables.Settings` Settings… — (c) notebooks and data tables (module 2)
- `action:DS.StaticOutput` Static Output — (c) notebooks and data tables (module 2)
- `action:PCRefreshAction` Refresh — (c) Package Checker, a JetBrains vulnerability service
- `action:PCOnlyVulnerableToggleAction` Show Safe Dependencies — (c) Package Checker, a JetBrains vulnerability service
- `action:PCFindUsagesAction` Find Usages — (c) Package Checker, a JetBrains vulnerability service
- `action:PCNavigateToSource` Jump To Source — (c) Package Checker, a JetBrains vulnerability service
- `action:Qodana.ShowBuildPage` Open Build Page — (c) Qodana, a JetBrains service
- `action:Qodana.ShowBrowserPage` Open Qodana in Browser — (c) Qodana, a JetBrains service
- `action:Profiler.CollapseAllGroups` Profiler.CollapseAllGroups — (c) IntelliJ's internal IDE performance profiler
- `action:Profiler.ExpandAllGroups` Profiler.ExpandAllGroups — (c) IntelliJ's internal IDE performance profiler
- `action:Profiler.OpenBackTracesAction` Method Back Traces — (c) IntelliJ's internal IDE performance profiler
- `action:Profiler.OpenMergedCalleesAction` Method Merged Callees — (c) IntelliJ's internal IDE performance profiler
- `action:Profiler.ExcludeCallAction` Exclude Call — (c) IntelliJ's internal IDE performance profiler
- `action:Profiler.ExcludeSubTreeAction` Exclude Subtree — (c) IntelliJ's internal IDE performance profiler
- `action:Profiler.FocusOnCallAction` Focus On Call — (c) IntelliJ's internal IDE performance profiler
- `action:Profiler.FocusOnSubtreeAction` Focus On Subtree — (c) IntelliJ's internal IDE performance profiler
- `action:Qodana.ProblemsViewCloseReportAction` Close Report — (c) Qodana, a JetBrains service
- `action:Qodana.ProblemsView.ShowPreview` Open Editor Preview — (c) Qodana, a JetBrains service
- `action:Qodana.ProblemsView.Login` Qodana.ProblemsView.Login — (c) Qodana, a JetBrains service
- `action:Qodana.ProblemsView.RefreshReportAction` Refresh Report — (c) Qodana, a JetBrains service
- `action:Qodana.ProblemsView.OpenWebUI` Show Qodana in Browser — (c) Qodana, a JetBrains service
- `action:Qodana.SarifFileReportAction` Open SARIF with Qodana — (c) Qodana, a JetBrains service
- `action:ReconnectToRemoteJupyter` Reconnect to Jupyter… — (c) notebooks and data tables (module 2)
- `action:ReloadRemoteFileAction` Reload from Jupyter Server… — (c) notebooks and data tables (module 2)
- `action:RestoreDefaultExtensionScripts` Restore Default Extensions — (c) IntelliJ's internal maintenance and diagnostics
- `action:JupyterShutdownNotebookAction` Shutdown Kernel… — (c) notebooks and data tables (module 2)
- `action:SettingsSyncStatusAction` Backup and Sync — (c) Backup and Sync, a JetBrains account service
- `action:SettingsSyncSeparator` SettingsSyncSeparator — (c) Backup and Sync, a JetBrains account service
- `action:WelcomeScreen.DevelopPlugins` Plugin Development — (c) IntelliJ plugin development (DevKit)
- `action:AboutNbstripoutFilter` About Nbstripout — (c) notebooks and data tables (module 2)
- `action:Qodana.AddQodanaToCiAction` Add Qodana to CI Pipeline — (c) Qodana, a JetBrains service
- `action:settingsSync.history` Backup and Sync History — (c) Backup and Sync, a JetBrains account service
- `action:settingsSync.troubleShoot` Backup and Sync Troubleshooting — (c) Backup and Sync, a JetBrains account service
- `action:PromoBeans` Beans — built (2026-11-26, the third Spring wave): the paid edition's Beans tool window itself, on the Spring LS's index (spring-boot-beans-tool-window)
- `action:PromoSpring` Change Active Spring Profiles… — (c) IntelliJ IDEA Ultimate promotion
- `action:chart-demo` chart-demo — (c) IntelliJ's internal IDE performance profiler
- `action:Markdown.Extensions.CleanupExternalFiles` Clean Up Markdown Extensions External Files… — (c) IntelliJ's own Markdown extensions and its embedded browser
- `action:Kotlin.ClearScratch` Clear Kotlin Scratch — (c) Kotlin (no engine chosen, left out with notebooks and database tools)
- `action:Qodana.ClearQodanaCacheAction` Clear Qodana Reports Cache — (c) Qodana, a JetBrains service
- `action:NotebookInsertCodeCellAboveAction` Code Cell Above — (c) notebooks and data tables (module 2)
- `action:PromoOpenAPI` Configure OpenAPI Sources… — (c) IntelliJ IDEA Ultimate promotion
- `action:JupyterConfigureShortcutsAction` Configure Shortcuts… — (c) notebooks and data tables (module 2)
- `action:CopyKotlinProjectInformation` Copy Kotlin Project Overview To Clipboard — (c) Kotlin (no engine chosen, left out with notebooks and database tools)
- `action:CopySwingComponentScreenshot` Copy Screenshot — (c) notebooks and data tables (module 2)
- `action:PromoDatabase` Database — (c) IntelliJ IDEA Ultimate promotion
- `action:DumpFocusableComponentHierarchyAction` Dump Focusable Component Hierarchy — (c) IntelliJ's internal maintenance and diagnostics
- `action:JupyterMarkdownDestroyControllerAction` Edit Markdown Cell — (c) notebooks and data tables (module 2)
- `action:PromoEndpoints` Endpoints — built (2026-11-25, the second Spring wave): the paid edition's Endpoints tool window itself, on the Spring LS's request mappings (spring-boot-endpoints)
- `action:KotlinShellExecute` Execute Kotlin Code — (c) Kotlin (no engine chosen, left out with notebooks and database tools)
- `action:NotebookExpandSelectionToCellAboveAction` Expand Selection to Cell Above — (c) notebooks and data tables (module 2)
- `action:NotebookExpandSelectionToCellBelowAction` Expand Selection to Cell Below — (c) notebooks and data tables (module 2)
- `action:TableResult.GrowSelection` Extend Selection — (c) the database grid's data tables (module 2)
- `action:IgnoreNbstripoutFilter` Ignore Nbstripout — (c) notebooks and data tables (module 2)
- `action:InitializeMissingPluginUpdateSourcesAction` Initialize Missing Plugin Update Sources — (c) JetBrains' plugin repository
- `action:JunieInstallAction` Install Junie — (c) JetBrains AI promotion (Decision 5: AI is the user's own providers)
- `action:InstallNbstripoutFilter` Install Nbstripout — (c) notebooks and data tables (module 2)
- `action:PromoProfiler` IntelliJ Profiler — (c) IntelliJ IDEA Ultimate promotion
- `action:JupyterEditorCopyAction` JupyterEditorCopyAction — (c) notebooks and data tables (module 2)
- `action:JupyterEditorCutAction` JupyterEditorCutAction — (c) notebooks and data tables (module 2)
- `action:JupyterEditorPasteAction` JupyterEditorPasteAction — (c) notebooks and data tables (module 2)
- `action:PromoKubernetes` Kubernetes — (c) IntelliJ IDEA Ultimate promotion
- `action:LogFocusRequests` Log Focus Requests — (c) IntelliJ's internal maintenance and diagnostics
- `action:Markdown.OpenDevtools` Open Devtools Window For The Current Markdown Preview — (c) IntelliJ's own Markdown extensions and its embedded browser
- `action:Qodana.OpenReportAction` Open Local Report… — (c) Qodana, a JetBrains service
- `action:PromoPersistence` Persistence — (c) IntelliJ IDEA Ultimate promotion
- `action:Qodana.OpenQodanaCloudReportAction` Qodana.OpenQodanaCloudReportAction — (c) Qodana, a JetBrains service
- `action:Qodana.OpenQodanaCloudSettingsAction` Qodana.OpenQodanaCloudSettingsAction — (c) Qodana, a JetBrains service
- `action:com.intellij.remoterobot.recorder.action.OpenRecorderAction` Record UI Test — (c) IntelliJ's UI test robot
- `action:CallSaul` Repair the reference app — (c) IntelliJ's own caches and indexes (Spork IDE has none to repair)
- `action:RestartKotlinNotebookHighlighting` Restart Highlighting in Kotlin Notebook — (c) notebooks and data tables (module 2)
- `action:NotebookRunCellInsertBelowAction` Run Cell and Insert Below — (c) notebooks and data tables (module 2)
- `action:KotlinCodeMigration` Run Code Migrations — (c) Kotlin (no engine chosen, left out with notebooks and database tools)
- `action:CheckVfsSanity` Run VFS Health Check — (c) IntelliJ's own caches and indexes (Spork IDE has none to repair)
- `action:NotebookOutputSelectAllAction` Select All — (c) notebooks and data tables (module 2)
- `action:TableResult.SelectAllOccurrences` Select All Occurrences — (c) the database grid's data tables (module 2)
- `action:NotebookSelectCellAboveAction` Select Cell Above — (c) notebooks and data tables (module 2)
- `action:NotebookSelectCellBelowAction` Select Cell Below — (c) notebooks and data tables (module 2)
- `action:TableResult.SelectNextOccurrence` Select Next Occurrence — (c) the database grid's data tables (module 2)
- `action:JupyterSelectSourceAction` Select Source — (c) notebooks and data tables (module 2)
- `action:HidpiInfo` Show HiDPI Info — (c) IntelliJ's internal maintenance and diagnostics
- `action:Kotlin.Gradle.ShowDslLogs` Show Kotlin Gradle DSL Logs — (c) Kotlin (no engine chosen, left out with notebooks and database tools)
- `action:TableResult.ShrinkSelection` Shrink Selection — (c) the database grid's data tables (module 2)
- `action:Kotlin.StopScratch` Stop Scratch Execution — (c) Kotlin (no engine chosen, left out with notebooks and database tools)
- `action:SwitchFileBasedIndexStorageAction` Switch File-Based Index Storage — (c) IntelliJ's own caches and indexes (Spork IDE has none to repair)
- `action:StatusBarWidgets.Toggle.GridAggregator` Toggle 'Aggregator' Widget — (c) the database grid's data tables (module 2)
- `action:StatusBarWidgets.Toggle.VfsRefresh` Toggle 'File System Sync' Widget — (c) IntelliJ's own caches and indexes (Spork IDE has none to repair)
- `action:StatusBarWidgets.Toggle.light.edit.git` Toggle 'Git' Widget — (c) IntelliJ's internal maintenance and diagnostics
- `action:StatusBarWidgets.Toggle.GridPosition` Toggle 'Grid Position' Widget — (c) the database grid's data tables (module 2)
- `action:StatusBarWidgets.Toggle.kotlin.k2.mode` Toggle 'Kotlin plugin K2 mode' Widget — (c) Kotlin (no engine chosen, left out with notebooks and database tools)
- `action:StatusBarWidgets.Toggle.kotlin.script.definition` Toggle 'Kotlin Script definition' Widget — (c) Kotlin (no engine chosen, left out with notebooks and database tools)
- `action:StatusBarWidgets.Toggle.CaptureCpuSnapshot` Toggle 'Start CPU Usage Profiling' Widget — (c) IntelliJ's internal IDE performance profiler
- `action:ToggleFlameGraphViewAction` ToggleFlameGraphViewAction — (c) IntelliJ's internal IDE performance profiler
- `action:Qodana.RunQodanaAction` Try Code Analysis with Qodana — (c) Qodana, a JetBrains service
- `action:TableResult.UnselectPreviousOccurrence` Unselect Previous Occurrence — (c) the database grid's data tables (module 2)
