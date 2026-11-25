# Settings pages: what Spork IDE has of IntelliJ's

Every page of IntelliJ IDEA CE 2025.2.5's Settings (`reference/idea/settings.md`, 206 pages), triaged for
`crates/spork_settings`. Since 2026-11-24 the window, its tree's order and the Appearance page follow IntelliJ IDEA
2026.3 (the user's, measured on its screenshot): the "Settings – project" window of 900×700, its pin (Float or Window
mode), the project's mark on a project's page in the tree and the breadcrumbs, the count of plugin updates on Plugins,
and Appearance & Behavior › Accessibility, a page of its own in 2026.3.

- **a**: every option has a Zed setting, a Spork preference or behaviour another phase built behind it; the page is
  built to match IntelliJ's and its end-to-end scenario names it in `covers`.
- **b**: some options are backed: the page has those, in IntelliJ's order, groups and labels, the rest left out; it
  isn't claimed.
- **c**: nothing is backed: the page is left out.

Counts: **1 a**, **38 b**, **167 c**. Version Control's pages are Spork's (the reference app's
entry decides, Decision 9). Options Zed has that IntelliJ's pages don't (the minimap, hiding the mouse while typing,
restoring unsaved files) aren't on IntelliJ's pages: they stay in `settings.json`.

| Page | | Backed, and left out |
| --- | --- | --- |
| Appearance & Behavior | b | the group page: its description and links to the pages Spork IDE has |
| Appearance & Behavior › Appearance | b | As 2026.3 has it: Theme with Sync with OS and its gear (the themes Sync with OS picks, `theme.light` and `theme.dark`), Use custom font and its size; UI Options in two columns: Always show full path in window header (`window_title_format` with `${filePath}`), Use project colors in main toolbar (`ui_options.use_project_colors_in_toolbar`, the project's glow `spork_workbench::shell::project_glow` paints), Display icons in menu items (`ui_options.show_icons_in_menus`, both menus); Tree Views: Show indent guides, Use smaller indents (`project_panel.indent_size` 12 against 20); Tool Windows: Widescreen tool window layout (`bottom_dock_layout`). Left out: Editor color scheme (Zed's theme is its editor's scheme too), Different tool window background and the tool window bars' and names' options, side-by-side layouts and sizes (the frame's, being rebuilt), Compact mode (the frame's and the lists' metrics, the same), Keep popups open for toggle items (a menu's checked item can't tell a toggle from a choice, and menus are built once as they open: a kept-open menu would show stale checks), Drag-and-drop with Alt pressed only, Smooth scrolling (GPUI scrolls a wheel's lines at once: an animation in the shared scroll), the mnemonics (none on macOS), Background Image…, Presentation Mode, Antialiasing |
| Appearance & Behavior › Accessibility | b | 2026.3's page: Vision's Zoom (`ui_font_size`, moved from Appearance). Left out: contrast scrollbars, red-green vision deficiency, Screen Reader Mode, Sound Signals |
| Appearance & Behavior › Menus and Toolbars | c | the menus and toolbars can't be customised |
| Appearance & Behavior › System Settings | b | Confirm before exiting the IDE, Reopen projects on startup, Autosave's idle and switching options. Left out: closing a tool window with a running process, Open project in, Default project directory, Back up files before saving, Sync external changes (Zed always reloads), the "Autosave cannot be disabled" comment (Zed's can) |
| Appearance & Behavior › System Settings › Data Sharing | c | nothing is collected or sent (phase 1) |
| Appearance & Behavior › System Settings › Date Formats | c | Spork's own page has these for the Git tool window only; IntelliJ's are IDE-wide (Local History, Notifications) |
| Appearance & Behavior › System Settings › HTTP Proxy | c | Zed's one `proxy` URL backs Manual proxy configuration alone (HTTP or SOCKS, host, port); unset, Zed takes the environment's proxy, which is neither IntelliJ's No proxy nor its Auto-detect, so the page would be one radio button: left out |
| Appearance & Behavior › System Settings › Language and Region | c | the interface is English only |
| Appearance & Behavior › System Settings › Passwords | c | passwords always go to the Keychain |
| Appearance & Behavior › System Settings › Server Certificates | c | no certificate store of the IDE's own |
| Appearance & Behavior › System Settings › Updates | c | no updater yet (phase 1 removed Zed's) |
| Appearance & Behavior › File Colors | c | file colours by scope come with gaps-2's file colours (`spork_navigate`); revisit after that merge |
| Appearance & Behavior › Scopes | c | no scopes |
| Appearance & Behavior › Notifications | c | notifications have no groups or display types to set |
| Appearance & Behavior › Data Editor and Viewer | c | no database grid yet (module 2) |
| Appearance & Behavior › Quick Lists | c | no quick lists |
| Appearance & Behavior › Required Plugins | c | no per-project plugin requirements |
| Appearance & Behavior › Trusted Locations | c | not built yet: Zed trusts worktrees and their parents (`project::trusted_worktrees`) but exposes no list to edit; a small `project` patch would make this page (a) |
| Appearance & Behavior › Path Variables | c | no path variables |
| Appearance & Behavior › Presentation Assistant | c | no presentation assistant |
| Keymap | b | phase 9's page over Zed's keymap: the keymap picked, IntelliJ's action groups, search by name and shortcut, Add Keyboard Shortcut and Remove. Left out: keymap copies, mouse and abbreviation shortcuts |
| Editor · Build, Execution, Deployment · Languages & Frameworks · … (4 pages) | b | the group page |
| Editor › General | b | Change font size with Command+Mouse Wheel in (without Active editor / All editors: Zed zooms every editor), Move code fragments with drag-and-drop, Vertical and Horizontal scroll offset, Remove trailing spaces on (All lines only), Ensure every saved file ends with a line break. Left out: Soft Wraps (Zed wraps by language, not file pattern), Virtual Space, the scroll jumps, Caret Movement, smooth scrolling and caret behavior, Rich-Text Copy, Keep trailing spaces on caret line, Remove trailing blank lines (one Zed setting with the line break) |
| Editor › General › Auto Import | c | imports are the language servers' |
| Editor › General › Appearance | b | Caret blinking (no interval), Use block caret, Highlight occurrences of selected text, Show hard wrap and visual guides, Show line numbers (Absolute, Hybrid), Show whitespaces, Show indent guides. Left out: full line height caret, Relative line numbers, method separators, whitespace kinds, intention bulb and preview, rendered doc comments, code lens, inlay hints font, tag tree highlighting |
| Editor › General › Breadcrumbs | b | Show breadcrumbs (`toolbar.breadcrumbs`, Zed's, at the top). Left out: Placement (no bottom breadcrumbs), the languages, Manage colors |
| Editor › General › Code Completion | b | Show suggestions as you type, Show the documentation popup (no delay), Show the parameter info popup (no delay). Left out: case matching, single suggestions, sorting, insertion keys, parentheses, command and ML completion, HTML, parameter hints, full signatures |
| Editor › General › Code Folding | b | Show code folding arrows (`gutter.folds`). Left out: Always / On mouse hover (`spork_editor` shows them on hover, gaps-1's crate), bottom arrows, Fold by default |
| Editor › General › Console | c | the Run tool window's console is a terminal: no soft wraps, command history, buffer in KB, encoding or stack trace folding |
| Editor › General › Editor Tabs | b | Tab placement (Top, None), Show pinned tabs in a separate row, Show file icon, Close button position (Left, Right, None), Enable preview tab, Tab limit, When the current tab is closed, activate. Left out: Show tabs in (one row, scrolling or squeezing, multiple rows), file extension, directory for non-unique names and full path on hover (always on in Zed), Mark modified, Tab Order, When tabs exceed the limit |
| Editor › General › Gutter Icons | c | gutter icons can't be turned off one kind at a time (only the run markers, `gutter.runnables`) |
| Editor › General › Inline Completion | c | no inline completion providers in the IDE yet (AI Assistant) |
| Editor › General › Postfix Completion | c | no postfix templates |
| Editor › General › Smart Keys | b | Insert paired brackets, Surround selection on typing quote or brace, Smart indent, Reformat on paste (None, Indent block). Left out: Home and End, Insert pair quote (one Zed setting with brackets), Reformat block on '}', CamelHumps, double ⌥ carets (`spork_editor`'s, no setting), Tab out of brackets, Insert pair '}', block comments, doc comment stubs, Unindent on Backspace, JavaDoc, Kotlin |
| Editor › General › Smart Keys › HTML/CSS · Editor › General › Smart Keys › JSON · Editor › General › Smart Keys › YAML · … (4 pages) | c | language-specific typing helpers the language servers don't offer |
| Editor › General › Sticky Lines | b | Show sticky lines while scrolling (`sticky_scroll.enabled`). Left out: Maximum number of lines, the languages, Manage colors |
| Editor › Code Editing | b | Show quick documentation on hover, Tooltip delay. Left out: brace, scope and usage highlighting (always on), refactoring options, error stripe, autoreparse, Next Error scope, @SuppressWarnings |
| Editor › Font | b | Font, Size, Line height, Enable ligatures, Typography Settings' Main weight and Fallback font, and the preview beside them (in the font, size, line height and ligatures as edited; not editable). Left out: Reader mode, Bold weight, character variants |
| Editor › Color Scheme | b | Scheme (the theme) and Change IDE Theme…. Left out: the links to the colour pages |
| Editor › Color Scheme › General · Editor › Color Scheme › Language Defaults · Editor › Color Scheme › Color Scheme Font · … (29 pages) | c | Zed's themes aren't edited in the IDE |
| Editor › Code Style | b | Line separator (System-Dependent, \n, \r\n), Hard wrap at, Visual guides. Left out: Scheme, indent detection, Classic Mac OS, Wrap on typing, Detect and use existing file indents, EditorConfig (always on in Zed), the Formatter tab |
| Editor › Code Style › Java · Editor › Code Style › Kotlin · Editor › Code Style › EditorConfig · … (13 pages) | c | formatting is the language servers' and formatters' |
| Editor › Code Style › Other File Types | b | Use tab character, Tab size. Left out: Scheme, Smart tabs, Indent (Zed indents by the tab size) |
| Editor › Inspections | c | diagnostics are the language servers': no inspection profiles |
| Editor › File and Code Templates | c | no file templates |
| Editor › File Encodings | c | Zed has no default encoding setting |
| Editor › Live Templates | c | Zed's snippets have no editor in the IDE yet |
| Editor › File Types | b | Recognized File Types: the languages, each with the file name patterns the user gave it (`file_types`), Add Wildcard and remove; Ignored Files and Folders (`file_scan_exclusions`): add, edit, remove. Left out: a language's own patterns (Zed's can't be edited or removed), new file types, hashbang patterns, the operating system's file associations |
| Editor › Copyright and the 13 pages under it | c | no copyright plugin |
| Editor › Inlay Hints | c | Zed's hints are three kinds over a master switch, not IntelliJ's tree by language |
| Editor › Emmet | c | no Emmet |
| Editor › Intentions | c | intentions are the language servers' code actions: no list to enable |
| Editor › Language Injections and the page under it | c | no language injection settings |
| Editor › Natural Languages and the 2 pages under it | c | no grammar or spelling checker |
| Editor › Reader Mode | c | no reader mode |
| Editor › TextMate Bundles | c | languages come from Zed's grammars and extensions |
| Editor › TODO | c | no TODO patterns or tool window |
| Plugins | b | phase 9's page over Zed's extension registry: Marketplace and Installed. Left out: disabling a plugin, logos, ratings |
| Version Control | b | the group page; Issue Navigation, Mercurial, Perforce and Subversion aren't there |
| Version Control › Changelists | b | Spork's page, all of the reference app's options; in the IDE the project tree and the editor don't follow the inactive-changelist options |
| Version Control › Commit | a | Spork's page, as the reference app's (the reference app decides): Clear initial commit message and the commit message inspections, which Spork's Commit tool window applies |
| Version Control › Confirmation | b | Spork's page; the editor's gutter and error stripe options and file creation/deletion confirmations don't reach Zed's editor and Project tool window |
| Version Control › Directory Mappings | b | Spork's page, over the repositories the Git tool window shows; the editor and the Project tool window follow Zed's git layer instead |
| Version Control › File Status Colors | b | Spork's page; the Project tool window and editor tabs take their colours from the theme |
| Version Control › Issue Navigation | c | Spork's page has no patterns to add and links nothing: left out of the tree |
| Version Control › Log | b | Spork's page, over Spork's log in the Git tool window; not checked option by option in the IDE |
| Version Control › Shelf | b | Spork's page; the Shelf tool window comes with gaps-2's merge |
| Version Control › Git | b | Spork's page (phase 9); not checked option by option in the IDE |
| Version Control › GitHub | b | Spork's page; the IDE has no pull request tool windows for the PR options |
| Version Control › GitLab | b | Spork's page; the IDE has no merge request tool windows |
| Version Control › Mercurial · Version Control › Perforce · Version Control › Subversion · … (6 pages) | c | git only |
| Build, Execution, Deployment › Build Tools and the 9 pages under it | c | phase 10's Maven and Gradle import has no settings yet |
| Build, Execution, Deployment › Compiler and the 7 pages under it | c | JDT LS builds; no compiler settings |
| Build, Execution, Deployment › Coverage | c | no coverage |
| Build, Execution, Deployment › Debugger | c | none of its options has a Zed or Spork setting behind it (in the tree to reach Data Views) |
| Build, Execution, Deployment › Debugger › Async Stack Traces · Build, Execution, Deployment › Debugger › Data Views › Java · Build, Execution, Deployment › Debugger › Data Views › Java Type Renderers · … (5 pages) | c | Java's debugger (Eclipse's java-debug) and Zed's take none of these options |
| Build, Execution, Deployment › Debugger › Data Views | b | Show values inline (`inlay_hints.show_value_hints`). Left out: sorting, auto expressions, value tooltips |
| Build, Execution, Deployment › Java Profiler | c | no profiler |
| Build, Execution, Deployment › Kubernetes | c | a promotion page |
| Build, Execution, Deployment › Remote Jar Repositories | c | no JAR repositories |
| Languages & Frameworks › JavaFX · Languages & Frameworks › JavaScript · Languages & Frameworks › JVM Logging · … (14 pages) | c | language and framework settings the language servers don't take |
| Languages & Frameworks › Markdown | b | Default layout (Editor, Preview), Preview font size. Left out: the rendering engine, Editor and Preview, Preview layout, scroll sync, code fence options, grouping, commands, PlantUML, Custom CSS, Pandoc |
| Tools › Actions on Save | b | Reformat code (`format_on_save`, all file types, the whole file) and the link Configure autosave options…. Left out: the table's Activated On column (always Any save), the file types and changed-lines choices (Zed formats every language's file whole, a language's own `format_on_save` aside), optimize imports, rearrange, cleanup, copyright, build |
| Tools › Code With Me · Tools › CSV Formats · Tools › Database · … (19 pages) | c | no such tool in the IDE |
| Tools › Diff & Merge | c | Spork's page exists for the git client; the IDE's diffs don't follow it yet |
| Tools › Terminal | b | Start directory, Environment variables, Font, Fallback, Size, Line height, Shell path, Copy to clipboard on selection, Use Option as Meta key, Cursor shape. Left out: Terminal engine, Column width, Configure colors…, Default tab name, separators, bell, closing, mouse reporting, Escape, middle click, IDE shortcuts, shell integration, hyperlinks |
| Backup and Sync | c | no settings sync |
| Advanced Settings | b | IDE › Duration of storing changes in Local History (Spork IDE's `spork_local_history`), Terminal › Terminal scrollback buffer size, User Interface › Use native file chooser dialog. Left out: the rest, and Show modified only |
