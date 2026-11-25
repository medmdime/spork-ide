# The reference app's look and behavior: a spec for Spork

Target: the reference app 1.1.24 (IntelliJ IDEA Community 2026.3, build
263.7307) on macOS. It uses the New UI with the Islands theme, default settings, at 100 % scale.
Goal: Spork looks and behaves so close to it that the two are hard to tell apart.

## How this was sourced
- Read from the installed `/Applications/Reference.app`. I did not launch it or decompile classes. I read:
  - every shipped plugin and module descriptor: actions, groups and their order, tool windows, settings
    pages, advanced-setting defaults, and the reference app's own comments;
  - the message bundles (labels) and the keymap XMLs;
  - the theme JSONs `ManyIslandsDark/Light` with their parents (`expUI_dark/light`, Darcula and IntelliJ Light);
  - the editor color schemes, the expui SVG icons, and the icon-mapping JSONs.
- Theme colors are resolved the way IntelliJ applies them. The parent theme loads first. A `*.tail` key in a
  later theme then replaces every inherited key that ends in `.tail`.
- Shortcuts are for the default macOS keymap, which is shown as "macOS" (file `Mac OS X 10.5+`). It inherits the
  `$default` shortcuts with Ctrl turned into ⌘, drops the ones that keymap removes, and adds its own.
- Layout values (px, insets, arcs, behavior) come from the reference app's own source (Apache-2.0): a shallow clone of
  the reference app's repository at its head. The main files are `JBUI.java`, the Islands customization, vcs-log, git4idea,
  vcs-impl, collaboration-tools, and the welcome-screen and settings code. Values are written in my own words; no
  code was copied.
- What the reference app changes from IntelliJ comes from its merged pull requests (titles and changed files), cited as
  "#N", and from the comments it leaves in the shipped descriptors. The README screenshot (Linux, an older build)
  was used for visual checks.
- All values are at 100 % scale. In IntelliJ an "arc" is a diameter, so arc 8 is a 4 px corner radius.
- Colors are given as "Islands Dark / Islands Light". "(inferred)" marks a value that was not read directly.
- The UI font is 13 px. "12 px" and similar are relative to it, as IntelliJ's size offsets are.

## 1. Welcome screen

### Which welcome screen
- The reference app shows the classic modal Welcome window, both at first launch and after the last project closes.
- The reference app ships the non-modal welcome (a project-like frame with a "Welcome" tab), but it never appears:
  - no welcome-project provider and no Welcome-tab content provider are registered;
  - so `welcome.screen.non.modal.enabled=true` has no effect.
  - Spork does not need the non-modal welcome.

### Window
- Title: "Welcome to the reference app", in a native title bar with a transparent title area.
- Size: 800×650, which is also the minimum. It can be resized; its size and position are remembered.
- First placement: centered horizontally, one third of the way down the screen.
- Layout:
  - left sidebar, 224 px wide;
  - 1 px vertical line, #26282C / #E9EAEE;
  - content area.
  - A 1 px line also runs along the top edge.
- Backgrounds: frame and sidebar #191A1C / #F7F8F9; content area #191A1C / #FFFFFF.

### Sidebar
- Logo block at the top:
  - the app icon at 28 px, with 29 px above and 27 px below;
  - beside it, "the reference app" at 13 px regular (#D1D3D9 / #000000) and "1.1.24" at 11 px #73767C;
  - text padding 28 px top, 10 px sides, 25 px bottom;
  - on hover a copy icon appears (expui/general/copy.svg, tooltip "Copy About");
  - the block's tooltip shows the build number.
- Navigation: three text items with no icons, in this order: **Projects**, **Customize**, **Plugins**.
  - "Learn" is removed (#173).
  - Items are indented 16 px, with 8 px top and bottom padding (about 32 px tall), 13 px text.
  - Selected item fill: #2A4371 / #D0DFFE. Projects is selected on open.
  - Plugins shows a grey count pill 20 px from the right edge when updates exist.
- Gear at the bottom: expui/general/settings.svg, at least 26×26, padding 15 px vertical and 14 px horizontal.
  - Its menu: Settings… (⌘,) · Check for Updates… · About · --- · Edit Custom Properties… ·
    Edit Custom VM Options… · Special Files and Folders… · Collect Logs and Diagnostic Data.

### Projects page
- Padding: 13 px top and bottom, 12 px left and right.
- Top bar:
  - search field filling the width, with expui/general/search.svg and placeholder "Search projects";
  - then three normal (not blue) buttons: **New Project**, **Open**, **Clone Repository**.
  - No "⋮" More button: it appears only after a 4th action, and there are only 3.
  - New Project opens an embedded panel whose only template is "Empty Project" (inferred).
- Below the bar: 12 px gap, a 1 px line (#484B4E (inferred) / #E9EAEE), then 10 px gap before the list.
- Recent project row:
  - 4 px padding all round; height follows the content, about 56 px with name, path and branch (inferred).
  - Badge cell: 20 px badge, top-aligned, margins 6 px top, 6 px left, 8 px right.
  - Text column (4 px margin, 4 px between lines):
    - name at 13 px, #D1D3D9 / #000000;
    - path shortened to `~/…`, 12 px #73767C;
    - branch line, 12 px grey, with expui/general/vcs.svg tinted grey in front (shown when known, inferred).
  - A missing project shows its name in #73767C and a greyed badge.
  - Hover fill #FFFFFF10 / #00000008. Selection fill #2A4371 / #D0DFFE.
  - Hover-only buttons at the right: 22 px slots, 4 px apart, 20 px from the edge.
    - Branches (expui/general/vcs.svg): opens a branches popup without opening the project.
    - Gear, "More Actions": Open Selected · Reveal in Finder · Copy Path ⇧⌘C · --- · New Project Group ·
      Move to Group ▸ · Edit… · --- · Set Custom Project Icon… · --- · Remove from Recent Projects… ⌫.
    - × only when the folder no longer exists.
  - Keys: ↩ opens the project; ⌥⌦ removes it. Projects can be grouped into collapsible groups.
- Project badge:
  - 20×20 px, corner arc 0.4 × size (8 px arc, about 4 px radius).
  - Fill: a diagonal gradient from the slot's End color (bottom-left) to its Start color (top-right).
  - Initials: white, JetBrains Mono SemiBold, 13 px at 20 px size (13 × size / 20), centered.
  - Initials rule:
    - two capitals from CamelCase ("MyProject" → MP);
    - otherwise the first letters of the first and last words split on space , - _ . ("my-cool-app" → MA);
    - otherwise the first letter only.
  - Color: a new project takes the next slot of a stored counter (random start, wraps after 9) and keeps it.
    It is not a hash of the name.
  - A custom `.idea/icon.svg` or `icon.png` replaces the badge, drawn with a 2 px inset.
  - Palette, Start→End, the same in both themes:
    1 #E08855→#E9806F · 2 #B08B14→#BB7F19 · 3 #A1A359→#87AA59 · 4 #3B92B8→#6183EC ·
    5 #3574F0→#7A64F0 · 6 #C84D8F→#A956CF · 7 #955AE0→#A84DE0 · 8 #24A394→#279CCD · 9 #5FAD65→#3D968B.
- Empty state (no recent projects): a centered column on #191A1C / #FFFFFF.
  - "Welcome to the reference app" at 26 px bold, 105 px from the top, 21 px below it.
  - Two grey lines (#73767C), 7 px apart: "Create a new project to start from scratch." and
    "Open existing project from disk or version control."
  - 27 px gap, then three large buttons in a row.
    - Each is a 60×60 rounded square, fill #2B2D30 / #F7F8F9, 30 px side padding, with its label 12 px below.
    - Focus ring 2 px #3574F0.
    - Icons: New Project (expui/welcome/createNewProjectTab.svg), Open (expui/welcome/open.svg),
      Clone Repository (expui/welcome/fromVCSTab.svg, label on two lines "Clone / Repository").

### Customize page
- Scrolls; padding 23 px top, 30 px left, 20 px bottom and right.
- Section headers are bold at 16 px.
- Appearance:
  - "Theme:" combo, a "Sync with OS" checkbox, and a small button to pick the theme for light/dark OS.
  - Theme list:
    - Islands Dark, Islands Light, Islands Darcula, High Contrast;
    - ---;
    - Dark, Light, Light with Light Header, Darcula.
  - An indented editor color scheme combo below it (inferred: its items).
- Language and Region: "Language:" combo (English) with "Requires restart" beside it; "Region:" combo
  ("Not specified") with a help comment.
- Accessibility:
  - "IDE font:" editable combo: 8, 9, 10, 11, 12, 13, 14, 16, 18, 20, 22, 24, 26, 28, 36, 48, 72.
  - Checkbox "Adjust colors for red-green vision deficiency", comment "Requires restart. For protanopia and
    deuteranopia.", link "How it works".
- Keymap: combo (see section 8) and a "Configure…" link.
- Then the links "Import Settings…" and "All settings…".

### Plugins page
- The embedded plugin manager: Marketplace and Installed tabs with search.

### What the reference app removes or renames
- No Learn tab.
- Strings say "the reference app" ("Welcome to the reference app", "Confirm before exiting the reference app"). A few labels still say "IDE"
  ("IDE font:", the gear tooltip "IDE and Project Settings").

## 2. Main frame

### Window and Islands geometry
- Frame background, behind the islands, toolbar, stripes and status bar: #26282C / #E9EAEE (`MainWindow.background`).
- Island fill (tool windows and the editor area): #191A1C / #FFFFFF.
- How an island is drawn:
  - each island is a full component; its outer 3 px band (half of `Island.borderWidth` 6) is painted in the frame color;
  - the corners are filled along a quadratic curve of radius 10 px (`Island.arc` 20 is the diameter).
- Island outline: 1 px at that 3 px offset, in `Island.borderColor`.
  - #191A1C in dark, which equals the fill, so it is invisible; #FFFFFF in light.
  - Straight segments stop 14 px from each corner; the corners are arcs 20 px in diameter.
- Content inset: 3 px inside a tool-window island, 2 px inside the editor area.
- Gaps:
  - two islands are 6 px apart (3 + 3) in the frame color;
  - the same 6 px separates the islands from the toolbar and from the status bar (inferred);
  - a window side without a stripe gets 4 px more (`Islands.emptyGap`).
- Compact UI density: arc 16, band 2 px, arc length 10.
- Inactive window: the island and gradient painting are drawn at 56 % alpha (`Island.inactiveAlpha`).
- Project color gradient (on by default: Appearance "Use project colors in main toolbar"):
  - the top of the frame is tinted with a ramp centered on the project widget, 600 px to each side and 300 px tall;
  - the ramp goes from the frame color to the project color mixed at 85 %, and fades down into the frame color;
  - the islands are not tinted.
- Project color (gradient start) per slot:
  - dark: #664832, #5E4724, #45522F, #335661, #394773, #613861, #503D70, #325959, #365439;
  - light: #F7D7CB, #F5E6B8, #E2F2C9, #CBE4F7, #D4D2FC, #ECD3F5, #E4D3F5, #CEF0E9, #CEEDD4.
  - The slot is the badge's slot from section 1.
- One project per window. No in-window project tab strip: macOS native window tabs appear only if the user merges
  windows.

### Main toolbar (macOS)
- 40 px tall (32 in compact mode), full width.
- Background: the frame color plus the project gradient. No bottom border.
- 78 px left inset for the traffic lights (26 + 52); 0 in full screen.
- No hamburger menu on macOS. The native menu bar has File, Edit, View, Navigate, Code, Refactor, Tools, Git,
  Window, Help (empty Build/Run menus are hidden, inferred).
- Left group, in order:
  - Project widget;
  - Update Project (expui/vcs/update.svg, ↙, ⌘T);
  - Push… (expui/vcs/push.svg, ↗, ⇧⌘K);
  - VCS widget (current branch);
  - Merge/Rebase widget, only during a merge, rebase, cherry-pick or revert;
  - separator.
  - The reference app put Update and Push in the toolbar (#111) and left of the branch widget (#305).
- Center group: empty. The File Name widget shows only when editor tabs are hidden. There is no Run widget.
- Right group, in order:
  - Search Everywhere (expui/general/search.svg, ⇧⇧);
  - Settings gear (expui/general/settings.svg, tooltip "IDE and Project Settings");
  - update widget, only when an update exists.
  - No AI, Code With Me or notification bell in the toolbar.
- Icon buttons: 30×30 px with a 20 px icon.
  - hover #FFFFFF17 / #00000012; pressed #FFFFFF29 / #00000020;
  - 6 px radius (arc 12).
- Dropdown widgets: 13 px text; padding 5 px top and bottom, 10 px left, 6 px right; a 16 px
  expui/general/chevronDown.svg at the end; same hover with a 6 px radius.
- Project widget:
  - 20 px badge · 6 px · project name · 2 px · chevron;
  - at most 500 px wide; tooltip is the `~/…` path;
  - the chevron appears once the project has loaded.
- VCS widget:
  - expui/general/vcs.svg (16 px) · branch name · incoming/outgoing marks · chevron;
  - at most 350 px wide, the end truncated (inferred from `MainToolbar.Dropdown.maxWidth`).
  - Incoming: a 12 px blue ↙ arrow, #548AF7 / #3574F0. Outgoing: a 12 px green ↗ arrow, #57965C / #369650.
  - When diverged, both show, incoming first. No numbers in the widget.
  - Without a repository it reads "Version control" (inferred) and its popup offers Create Git Repository….
- Settings gear menu: Run Anything · --- · Settings… ⌘, · Plugins… · --- · Theme ▸ · Keymap ▸ · View Mode ▸.
- Toolbar separators: #FFFFFF21 / #00000020.

### Tool-window stripes
- Left stripe:
  - each button cell is 37×40 px, about 38 px wide in all;
  - the clickable square is 30×30 with padding 5 top, 5 left, 5 bottom, 2 right;
  - 20 px icon, 6 px radius (arc 12).
- Right stripe: the mirror image (padding 5, 2, 5, 5).
- Stripe background is the frame color, with no visible border.
- Button states:
  - icon in the default icon color;
  - hover #FFFFFF17 / #00000012;
  - open: filled #3871E1 with a white icon.
- Left stripe, top group (fresh layout):
  - Project (expui/toolwindows/project.svg, ⌘1);
  - Commit (expui/toolwindows/commit.svg, ⌘0);
  - Pull Requests (github/expui/pullRequests.svg), only with a GitHub remote;
  - Merge Requests (gitlab/expui/gitLabToolWindow.svg), only with a GitLab remote;
  - "⋯" More tool windows (Bookmarks and others).
- Left stripe, bottom group: Terminal (⌥F12) and Git (Version Control, expui/toolwindows/vcs.svg, ⌘9). The
  screenshot shows Terminal above Git (order inferred).
- Right stripe: Notifications (expui/toolwindows/notifications.svg) only.
- Removed in the reference app: Structure, Services, Run, Debug, Problems, Hierarchy, Build.
- Default visibility: all closed, except Project the first time a project opens (inferred). The log fills the editor area.
- Tool-window header:
  - 41 px tall (31 compact), same fill as the island, no line under it;
  - title 13 px **bold**, 12 px left inset;
  - actions at the right with an 8 px inset, ending with ⋮ (expui/general/moreVertical.svg) and Hide
    (expui/general/hide.svg).
- With several contents the header shows tabs instead of the title, as 13 px regular labels.
  The selected tab is a rounded pill with a 6 px radius (inferred).

### Editor tabs (the log lives here)
- Tab row about 40 px tall (inferred).
- Each tab is a pill 28 px tall, inset 4 px left and right in its slot, 6 px radius (arc 12). The first tab starts 3 px in.
- Pill colors:
  - selected, editor focused: fill #233558 / #E3EBFE, 1 px border #2E4D89 / #A7C5FF;
  - selected, focus elsewhere: fill #26282C / #E9EAEE, border #40434A / #D1D3D9;
  - hovered: fill #FFFFFF17 / #00000012, no border;
  - other tabs: no fill.
- No underline. Tab text 13 px; unselected tabs are drawn at 75 % opacity.

### Status bar
- About 30 px tall: 13 px text plus 6 px widget padding above and below (inferred).
- Frame-color background, no top border.
- 13 px text, #9FA2A8 / #5F6269. Hover: text #D1D3D9 / #000000, fill #FFFFFF17 / #00000012.
- Left: the navigation bar as breadcrumbs (New UI default), project › folders › file, with chevrons.
  - For the log tab the screenshot shows the log's virtual file name ("PsiBinaryFile:Log") with a branch icon,
    a reference app quirk.
- Right, only when they apply to the focused editor: background-task progress, line:column, line separator,
  encoding, indent, read-only lock. With the log focused none of these show.
- Not shown by default:
  - the git branch widget (hidden while the main toolbar is visible);
  - the memory indicator;
  - notifications (they are in the right stripe).

## 3. Project tool window
- Id "Project", left stripe, expui/toolwindows/project.svg, ⌘1. Added back in the reference app (#146).
- Only the "Project" view exists, so the header shows a plain bold title "Project" with no view dropdown (inferred).
- Header actions, in order:
  - New… (expui/general/add.svg with a small ▾, ⌘N; disabled with no selection);
  - Select Opened File (expui/general/locate.svg);
  - Expand Selected (expui/general/expandAll.svg, ⌘=);
  - Collapse All (expui/general/collapseAll.svg, ⌘-);
  - ⋮;
  - Hide.
- ⋮ menu:
  - Behavior ▸: Enable Preview Tab, Open Files with Single Click, Open Directories with Single Click,
    Always Select Opened File.
  - Appearance ▸:
    - "Show" header: Modules, Members, Excluded Files, Visibility Icons, Libraries Contents, Scratches and Consoles,
      File Details;
    - ---;
    - Flatten Modules, Flatten Packages, Abbreviate Qualified Names, Hide Empty Middle Packages, Compact Directories;
    - ---;
    - File Nesting…;
    - ---;
    - Customize Tree Views….
  - Sort By ▸: Manual Order, Name, Type, Modification Time (Newest First), Modification Time (Oldest First),
    ---, Folders Always on Top.
  - Edit Scopes….
- Tree:
  - rows 24 px (22 compact), indent 7 + 11 px per level;
  - chevrons expui/general/chevronRight.svg and chevronDown.svg; no indent guides by default;
  - padding 4 px top and bottom, 12 px left and right.
- Tree colors:
  - background = the island fill #191A1C / #FFFFFF (inferred: tool windows repaint their content);
  - text #D1D3D9 / #000000;
  - hover #FFFFFF10 / #00000008;
  - selection: a rounded rectangle with a 4 px radius; focused #2A4371 / #D0DFFE, unfocused #33353B / #E9EAEE.
- Root node:
  - the directory name in bold (or "dir [module]" with the module part bold);
  - then its location in grey (#73767C), relative to home, e.g. "~/orca/workspaces/Spork/capricorn".
- Icons: folders expui/nodes/folder.svg; files expui/fileTypes/*; excluded folders expui/nodes/excludeRoot.svg
  (orange, inferred).
- File status colors apply to file names only (table in section 5).
  - Folders are not tinted.
  - Ignored files use the ignored color.
  - Gitignored folders are not excluded automatically.
- Last root node: "Scratches and Consoles" (expui/fileTypes/scratches.svg), shown by default (inferred).
  - Files outside the project may get a FileColor.Yellow background, #291F1C / #FFF9EB (inferred).
- Removed in the reference app: the "External Libraries" node (#239), and Find Usages, Find/Replace in Path, Inspect Code,
  Reformat and Optimize Imports in the context menu (section 9).

## 4. Git Log (in the editor area, the reference app's default)

### Editor tab
- The log opens as an editor tab because "Show the log in the editor window" defaults to on
  (Settings › Version Control › Log, with a "<app name> setting" badge, #251). While it is on, the Git tool window has
  no Log tab.
- Icon: expui/general/vcs.svg.
- Title:
  - "Log" with no filters; one filter shows its value, e.g. "Log: HEAD".
  - Several filters use forms joined by spaces: "containing X", "by me", "on main", "for path", "since …", "in root".
  - Graph options come first: "--first-parent", "linearized".
  - Cut to 150 characters; the tooltip repeats it.
- The main tab has no × button and ignores ⌘W (#89). It cannot be split. It opens unfocused and not pinned.
- "Open New Git Log Tab" (expui/general/openNewTab.svg, ⌥G):
  - in the log toolbar, and in the editor-tabs ▾ dropdown just before Recent Files;
  - the new tab copies the current filters and can be closed;
  - each tab's title follows its own filters.
- On restart, tabs with filters reopen, plus at most one tab without filters.

### Branches pane (left side of the log)
- Collapsed by default in the editor log; the state is then saved per tab.
- Collapsed strip:
  - a full-height button about 36 px wide (inferred) with a 1 px right border;
  - expui/general/chevronRight.svg about 8 px from the top;
  - "Branches" rotated −90° (reads bottom to top), about 11 px (inferred);
  - click anywhere to expand; hover #FFFFFF17 / #00000012 (inferred for dark).
- Expanded layout: vertical toolbar | 1 px line | search field above the tree | 1 px splitter | log.
  The pane starts at 30 % of the width; the split is saved.
- Vertical toolbar, top to bottom:
  1. Hide Git Branches (expui/general/chevronLeft.svg)
  2. ---
  3. New Branch… (expui/general/add.svg, ⌥⌘N)
  4. Update Selected (expui/vcs/update.svg; hidden with no remotes)
  5. Delete Branch (expui/general/delete.svg)
  6. Compare with Current (expui/vcs/diff.svg, ⌘D)
  7. Show My Branches (toggle, expui/general/search.svg)
  8. Fetch (expui/vcs/fetch.svg)
  9. Mark/Unmark As Favorite (expui/nodes/star.svg)
  10. Navigate Log to Selected Branch Head (expui/general/locate.svg, ⌥F1)
  11. ---
  12. Settings gear (expui/general/settings.svg)
  13. Expand All (⌘=)
  14. Collapse All (⌘-)
- Gear menu:
  - header "On Single Click": Update Branch Filter (off), Navigate Log to Branch Head (off);
  - ---;
  - Group by Directory (on), Group by Repository (only with several repositories), Show Tags (on).
- Search field: borderless, placeholder "Branch or tag", magnifier on the left, filters as you type.
  ↑/↓ move into the tree; ⌘F switches between field and tree.
- Tree, top level:
  - "HEAD (Current Branch)", a leaf with no icon;
  - "Local";
  - "Remote", with a folder node per remote (origin, …);
  - "Tags", when Show Tags is on.
  - Empty groups are hidden. There is no Recent group.
  - Grouping by directory splits names on "/". Everything starts expanded.
- Order inside a group: current branch, favorites, folders holding favorites, other folders, then the rest.
  Names sort naturally.
- Rows 24 px. Icons:
  - plain branch: expui/general/vcs.svg;
  - favorite: expui/nodes/star.svg, #F2C55C / #FFAF0F;
  - current branch: icons/new/currentBranchLabel.svg; current and favorite: currentBranchFavoriteLabel.svg
    (same yellows);
  - tag: branchLabel.svg in grey;
  - folder: expui/nodes/folder.svg.
- Default favorites: master, main, origin/master, origin/main.
- Incoming/outgoing after the name, in a small font: ↙ count #548AF7 / #3574F0, ↗ count #57965C / #369650.
  Counts above 99 show "99+".
- Selection #2A4371 / #D0DFFE (unfocused #33353B / #E9EAEE); hover #FFFFFF10 / #00000008.
- Double-click or ↩ sets the log's Branch filter to the selected branches.

### Log toolbar
- Layout, left to right:
  - text filter;
  - Branch, User, Date and Paths chips;
  - Graph Options (icons/new/intelliSort.svg, with a green dot when not at defaults);
  - ---;
  - Open New Git Log Tab;
  - then, pushed right: Resume Indexing (only while indexing is paused), Refresh (expui/general/refresh.svg, ⌘R),
    Cherry-Pick (icons/new/cherryPick.svg), View Options (eye, expui/general/show.svg),
    Go To Hash/Branch/Tag (expui/general/search.svg, ⌘F).
- Toolbar buttons 22×22; toolbar insets 5 px vertical, 7 px horizontal; about 32 px tall (inferred).
- Text filter:
  - search field with history, placeholder "Text or hash", at least 150 px wide;
  - inline toggles Regex (expui/inline/regex.svg ".*") and Match Case (expui/inline/matchCase.svg "Cc"),
    both off, shown with a pressed background when on;
  - filters on ↩ or when focus leaves, not while typing; empty field removes the filter;
  - tooltip "Filter by Commit Message or Hashes ⌘L";
  - matches are highlighted in the Subject column.
- Filter chips:
  - unset: the name in #73767C, 3 px gap, expui/general/chevronDown.svg;
  - set: "Branch: " in grey, the value in #D1D3D9 / #000000, then expui/general/closeSmall.svg
    (closeSmallHovered on hover); clicking × clears it;
  - hover turns the name to the normal text color;
  - focus: a 2 px rounded ring (arc 10) in #3871E1;
  - ↩ or ↓ opens the popup; ⌦ or ⌫ clears; the popup footer reads "Press ⌫ to clear the filter".
- Chip popups:
  - Branch: Select…, HEAD, Recent ▸, Favorites (when there are several), each favorite, ---, Local ▸, &lt;remote&gt; ▸;
  - User: Select…, me, then a "Recent" section;
  - Date: Select…, Last 24 hours, Last 7 days;
  - Paths: a tree selector.
- Graph Options popup:
  - "Sort" header: By Commit Date (default), Topologically;
  - "Options" header: First Parent, No Merges;
  - "Branch Actions" header: Collapse Linear Branches, Expand Linear Branches.
- View Options (eye) popup:
  - "Show" header: Root Names (only with several roots), Compact References View (on), Tag Names (off),
    Long Edges (off), Commit Timestamp (off), References on the Left (off), Columns ▸ (Author on, Hash off, Date on);
  - ---;
  - "Highlight" header: My Commits (on), Merge Commits (on), Current Branch (on), Not Cherry-Picked Commits.

### Commit table
- Columns: Subject (the graph is drawn inside it), Author, Date. Hash is hidden.
  - A Root color column appears only with several roots.
- No header row. Column edges can still be dragged (3 px grab zone); double-click an edge to reset its width.
- Author and Date widths fit their text over the first 1000 rows, at most 300 px. Subject takes the rest.
- No grid lines.
- Rows 26 px. 13 px text.
- Background: the island fill #191A1C / #FFFFFF.
- 8 px extra padding left of the first column and right of the last.
- Date (author date by default):
  - "Moments ago", "A minute ago", "N minutes ago", or "1 hour ago", up to 61 minutes;
  - then "Today 6:56 PM" or "Yesterday 6:56 PM";
  - then the macOS short date and time, e.g. "17/6/26, 6:56 pm" (en_GB).
- Author:
  - the name, or the email's local part when there is no name;
  - a "*" suffix when the committer differs from the author;
  - clipped without an ellipsis (inferred).
- Highlights:
  - my commits: bold in the Author column only, when the repository has more than one author;
  - merge commits: the whole row's text dimmed to #6F737A / #818594;
  - commits that don't match a filter but stay for the graph: #808080;
  - with a branch filter on another branch, commits on the current branch get #1D2336 / #EDF3FF behind them.
- Selection: a rounded rectangle (4 px radius, arc 8) inset 4 px from the left and right edges.
  - Focused #2A4371 / #D0DFFE; unfocused #33353B / #E9EAEE; text #D1D3D9 / #000000.
  - A run of several selected rows is rounded only at its top and bottom.
- Hover: 7 % white over the row in dark (about #292A2C); #E9EAEC in light.
- Ref labels:
  - right-aligned inside Subject by default; 12 px text in #6F737A / #6C707E, also on selected rows;
  - 4 px padding before and after; 6 px between labels in compact view (12 px otherwise); 1 px between icon and text;
  - the icon is a 45° tag outline with a 1 px stroke in the ref color, filled with the row color; the front tag has
    a dot; about 18 px tall (inferred);
  - each extra color adds another tag stacked about 5.8 px to the right.
  - Ref colors: HEAD #F5D273 / #FFAF0F · local #5FAD65 / #369650 · remote #B589EC / #834DF0 ·
    tag and other #868A91 / #6C707E.
  - Compact view (default): one label per commit, named after its first group, with all its colors stacked.
    Groups in order: current branch; tracked pairs named "origin &amp; main"; single branches. HEAD folds into the
    first group. Tags show only an icon unless Tag Names is on.
  - Labels take at most 1/3 of the free width.
  - Long names: "../rest" after the first "/", then cut with "…", at most 22 characters.
  - Hovering a label lists all its refs in a popup.
- Detached HEAD: a warning icon and "Detached HEAD" in #D59637 / #A56906.

### Graph
- Everything scales by row height ÷ 22, so ×1.18 at 26 px rows.
- At 1× (Retina values in brackets):
  - lane step 17 px (18.5);
  - line 1 px (1.5), highlighted line 3 px (2.5);
  - dot diameter 9 px (8.5), highlighted 11 px (9.5);
  - row center 13 (12.5).
- The text starts after 18.9 px per lane (at least min(6, lanes) lanes), plus a 2.36 px gap.
- Round caps and joins, antialiased.
- HEAD commit: a 13 px disc in the branch color, a 2 px ring in the row color, and a 5 px inner dot.
  Highlighted, it becomes a solid 15 px disc.
- Merge commits: an ordinary dot; only the row text is dimmed.
- Hover: the edges and dots under the mouse get an outline (drawn thicker in #D1D3D9 dark / #000000 light, then in
  their own color on top). The cursor becomes a hand.
- Lane color:
  - a branch's main line is colored from the String hash of the best ref at its head;
    preference: origin/master|main, other remotes, master|main, local branches, tags, current branch, HEAD;
  - side fragments use their layout index;
  - formula: r = id·200+30, g = id·130+50, b = id·90+100; each becomes |n mod 100| + 70; take the hue of that
    RGB; output HSB(hue, 0.6, 0.6) in dark or HSB(hue, 0.6, 0.7) in light.
  - Examples, dark / light:
    - origin/master #99783D / #B38C47
    - origin/main #4D3D99 / #5A47B3
    - master #66993D / #77B347
    - main #3D8A99 / #47A1B3
    - fragments 1–5: #3D8A99, #573D99, #3D5499, #3D9982, #743D99 (dark)
- Long edges (Long Edges off by default):
  - an edge spanning 30 or more rows is drawn only next to its ends, with arrowheads 0.3 × row height long at
    about ±33°;
  - clicking an arrow jumps to the other end.
- Skipped (filtered-out) commits are bridged with dotted edges.

### Right panel
- The table takes 70 % of the width; the right column 30 %.
- The right column holds the changed-files tree above the commit details, split 70/30.
- 1 px splitters, #26282C / #E9EAEE. Proportions are saved.
- Diff preview is off by default. Double-clicking a file opens its diff in an editor tab.
- Changes toolbar:
  - left: Show Diff (expui/vcs/diff.svg, ⌘D), Revert Selected Changes (expui/vcs/revert.svg),
    History Up to Here (expui/general/history.svg), Show Only Affected Changes (only with a Paths filter, inferred),
    View Options (eye);
  - right-aligned: Expand All (expui/general/expandAll.svg), Collapse All (expui/general/collapseAll.svg).
- Its View Options:
  - "Group By" header: Directory (on, ⌃P), Repository;
  - "Layout" header: Show Details (on), Show Diff Preview (off), Diff Preview Location ▸ Bottom / Right.
- Changes tree:
  - grouped by directory, 24 px rows, folder icons;
  - the root and folder rows add a grey count: "16 files", "2 files";
  - file names in their status colors (table in section 5);
  - empty text "Select commit to view changes".
- Commit details:
  - background from `Tree.background`, #191A1C / #F7F8F9 (light may render #FFFFFF; check);
  - message: JetBrains Mono 13 px, bold subject, plain body; long subjects cut with "…"; links and hashes clickable;
    padding 14 px top, 14 px left, 10 px bottom;
  - line 1 at 13 px: "c1c9c29a author &lt;email link&gt; on 16/6/26 at 11:31 pm" (8-character hash);
  - line 2 in #808080: "committed on &lt;date&gt; at &lt;time&gt;", or "committed by &lt;name&gt; &lt;email&gt; on …"
    when the committer differs; padding 10 px top, 14 px left, 10 px bottom;
  - ref labels (branches, then tags), 4 px apart;
  - containing branches: "In 12 branches: HEAD, master, dont-o…  Show all", cut to fit; expanded, one branch per
    line with "Hide". Other states: "In branches: loading…", "Not in any branch". Padding 14 px left and bottom.

## 5. Commit tool window (Local Changes)

### Mode
- Non-modal by default: the commit panel is always visible under the changes tree.
  - The Modal Commit plugin ships, but its advanced setting "Use modal commit interface for Git and Mercurial"
    is off.
  - There is no toggle and no "Commit…" button inside the window.
- The staging area is off by default, so changelists and checkboxes are used.

### Tabs
- Commit, Shelf, Stash, Worktrees, in that order.
  - Shelf shows only when shelves exist; Stash only when a stash exists; Worktrees when there are 2 or more worktrees.
- With Commit alone, the header shows just the bold title "Commit".

### Layout
- The changes tree takes the top 60 %, the commit panel the bottom 40 %, split by a 1 px line.
- The commit panel is at least 120 px tall.

### Toolbar (above the tree)
1. Refresh (expui/general/refresh.svg, ⌘R)
2. Rollback… (expui/vcs/revert.svg, ⌥⌘Z)
3. Show Diff (expui/vcs/diff.svg, ⌘D)
4. Shelve Silently (expui/vcs/shelve.svg, ⇧⌘H)
5. View Options (eye): "Group By" header with Directory (⌃P) and Repository; "Show" header with Ignored Files
6. ---
7. Select Opened File (expui/general/locate.svg)
8. Expand All (⌘=)
9. Collapse All (⌘-)
- The header ⋮ adds "Show on Double-Click ▸ Diff / Source" to the usual tool-window items.

### Changes tree
- Rows 24 px, indent 7 + 11 px, chevron icons, same colors as the Project tree.
- Default grouping is by repository only; repository nodes show only with several repositories, so a single
  repository gives a flat list.
- Expanded by default; "Unversioned Files" starts collapsed (the state is remembered).
- Root row:
  - checkbox, then "Changes" in **bold**;
  - then a space, a thin space (U+2009), and the count in grey #73767C: "1 file", "N files",
    or "N directories and M files";
  - while refreshing, ", updating…" follows in grey.
- File row:
  - three-state checkbox;
  - file-type icon (expui/fileTypes/*);
  - file name in its status color;
  - in the flat list, space + thin space + the parent path in grey #73767C. The path is relative to the root;
    files at the root show the root folder name.
- "Unversioned Files": regular weight, grey count, its own checkbox. Over 1000 files it shows a "browse" link
  instead of the list.
- Checkboxes:
  - Space toggles the selected rows; clicking the box toggles it;
  - the partial state is a white 11×2 bar.
- Double-click or ↩ opens the diff in an editor tab.
- File status colors:

| Status | Dark | Light |
|---|---|---|
| Modified, renamed, changed outside | #70AEFF | #0033B3 |
| Added, copied | #73BD79 | #067D17 |
| Unversioned | #E88F89 | #B23247 |
| Deleted | #6F737A | #6C707E |
| Ignored | #D69A6B | #8C4F00 |
| Conflict | #DE6A66 | #DE1B2E |
| Merged | #CF84CF | #643CB8 |

### Amend and status row (above the message)
- "Amend" checkbox (⌃⌥M), then a dropdown link "last commit ▾" in #71A1FE / #2F5EB9.
  - The link's popup lists the unpushed commits up to the first merge, at most 5 rows visible.
  - Its first row is bold "last commit" plus the subject in grey; the current choice gets
    expui/actions/checked.svg.
  - Once a commit is picked, the link shows its subject cut to 20 characters.
- Then Commit Message History (expui/general/history.svg, ⌃M).
- Right side, only when files are checked: "N added   N modified   N deleted", each in its status color, 3 spaces
  apart. With little room it compacts to "+N   *N   -N".
- Padding: 0 above and below, 12 px on the right.

### Message field
- An editor: JetBrains Mono 13 px, line spacing 1.2, soft wrap.
- Background is the editor fill #191A1C / #FFFFFF; text #BCBEC4 / #080808.
- Placeholder "Commit Message", which stays visible while focused.
- 6 px inner left padding.
- Border: a rounded outline (4 px radius) in #40434A / #D1D3D9; when focused, a 2 px #3871E1 ring.
- 3 px gap above and below the field, 11 px left and right.
- No right-margin guide by default; message checks are off.
- After a successful commit the message is cleared ("Clear initial commit message" is on, #120).

### Buttons row
- Padding 3 px top and bottom, 9 px left, 12 px right.
- Above it: an error or progress line when needed. Errors use expui/status/error.svg with "Select files to
  commit" or "Specify commit message".
- "Commit":
  - blue (#3871E1, white text) only while focus is in the commit panel; no split arrow for Git;
  - reads "Amend Commit" when Amend is on;
  - ⌘↩ or ⌃↩ commits.
- "Commit and Push…": a plain button, ⌥⌘K.
- Gear at the right edge: expui/general/settings.svg, "Show Commit Options", ⌃O.
- Commit options popup:
  - opens above the gear, at least 300 px wide, padding 10 px vertical and 14 px horizontal;
  - no "Commit Options" header (#371);
  - a "Git" group: "Author:" editable combo (a different author shows a warning outline) and a
    "Sign-off commit" checkbox;
  - no "Run Git hooks", reformat, optimize imports, code analysis or cleanup checks (disabled in the reference app).
- After a commit: the message is cleared, the tree refreshes, and a notification "N files committed: &lt;subject&gt;"
  appears (inferred wording). Commit and Push opens the Push dialog next (inferred).

### Staging-area mode (off by default)
- Nodes: Staged, Unstaged, Untracked (plus Unmerged and Ignored), with no checkboxes.
- Toolbar: Show Diff, Refresh, Stage All Tracked (expui/general/add.svg), Stash Silently, View Options.
- The button reads "Commit All" when nothing is staged (inferred).

## 6. Pull Requests (GitHub) and Merge Requests (GitLab) tool windows
- Both are on the left stripe and closable.
  - They become available only when the project has a GitHub (or Enterprise) remote, or a GitLab remote.
  - Icons: github/expui/pullRequests.svg and gitlab/expui/gitLabToolWindow.svg; a dot badge shows when the list
    has updates.
- Tabs instead of a title:
  - the first tab, the repository name, cannot be closed;
  - each opened item adds a closable "#123" (GitHub) or "!123" (GitLab) tab;
  - "New Pull Request at &lt;repo&gt;".
- Title actions:
  - GitHub: Create Pull Request… (expui/general/add.svg, ⌘N) and Select Opened Pull Request (locate);
  - GitLab: Create Merge Request….
  - The ⋮ menu adds switching repository or account.
- Login / selector screen (padding 30 px vertical, 16 px horizontal; gaps 10 px horizontal, 16 px vertical):
  - row 1: a repository combo and an account selector (20 px avatar);
  - row 2: buttons 16 px apart:
    - GitHub: "View Pull Requests", "Log In via GitHub…", link "Log in with Token…";
    - GitLab: "View Merge Requests", "Log In to gitlab.com…", "Log In to Custom Server…", "Log in with Token…";
  - an error line, then the grey hint "Change repository or account later under the More icon".
- List:
  - 8 px top padding; search field (8 px top, 10 px sides) with history;
  - filter row 10 px below: quick-filter button (expui/general/filter.svg), then chips 4 px apart:
    - GitHub: State, Author, Label, Assignee, Review, Sort;
    - GitLab: State, Author, Assignee, Reviewer, Label;
  - quick filters:
    - GitHub: Open, Your pull requests, Assigned to you (default), Review requests;
    - GitLab: Open, Includes my changes, Needs my review, Assigned to me (default), Closed;
    - the last used filter is remembered.
- List row:
  - padding 4 px top, 5 px bottom, 19 px sides; selection is a rounded rectangle inset 13 px.
  - Line 1, left: unread blue dot, title, and a label icon when there are labels.
  - Line 1, right:
    - a state tag only when not plain open (DRAFT, CLOSED, MERGED): 11 px, padding 0×4, radius 4,
      text #868A91 / #6C707E, fill #43454A / #DFE1E5;
    - conflict icon; CI icon (GitLab);
    - up to 2 assignee avatars (20 px);
    - up to 2 reviewer avatars (18 px) with a ring per review state: approved #57965C / #5FB865,
      waiting #E08855 / #EC8F4C, needs review #6F737A / #818594;
    - comment icon with the unresolved count.
  - Line 2: 12 px grey "#123 · created &lt;date&gt;, by &lt;author&gt;".
- List menu:
  - GitHub: Show · Checkout Branch · Open in Worktree · View Pull Request in Browser · Copy Pull Request URL · ---
    · Reload ⌘R;
  - GitLab: the same with "Merge Request" wording, and Refresh List ⌘R.
- Empty states: "No pull requests matching filters", "No pull requests in {repo}", plus a "Clear filters" link.
- Detail tab (margins 16 px left, 14 px right), top to bottom:
  1. Title at 18 px bold, then "#123" as a grey link; a state tag at top right; 8 px below, the link "View Timeline"
     (opens the timeline in an editor tab).
  2. Commits selector "All commits (N)" on the left; branch chip "base ← head" on the right
     (Checkout, Open in Worktree, Show in Git Log, Copy Branch Name; fill #393B40 / #EBECF0).
  3. Selected commit info.
  4. Changed-files tree, which fills the remaining height.
  5. Status checks, at most 149 px tall: "X approved review", "Waiting for review from X", with status icons.
  6. Buttons 16 px from the bottom:
     - Merge split button (Merge… / Squash and Merge… / Rebase);
     - "Submit Review (N)…";
     - More ▸ Request Review…, Re-request Review…, Close Pull Request, Reopen Pull Request, Set Myself as Reviewer.

## 7. Settings dialog
- From a project, Settings opens as a non-modal window (advanced setting `ide.ui.non.modal.settings.window` is on).
  - Title "Settings – &lt;project&gt;".
  - Initial and minimum size 900×700, remembered.
  - A pin toggle (expui/general/pin.svg, on) keeps it floating above the project window.
- From the welcome screen, it is a modal dialog titled "Settings".
- Left column:
  - 20 % of the width, background #191A1C / #F7F8F9;
  - search field at the top: padding 7 px top, 5 px sides, 6 px bottom; no placeholder; ⌘F focuses it;
  - category tree below: top-level names bold, children regular; row padding 2 px vertical, 10 px horizontal;
  - names of modified pages turn blue (#2F5EB9 in light).
- Right header:
  - breadcrumbs in bold ("Version Control › Git"), padding 11 px top, 6 px left, 10 px right;
  - a project-scope icon on per-project pages;
  - a "Revert changes" link when the page is modified;
  - back/forward arrows and the pin at the far right.
- A page with children but no own content shows its description and links to the children.
- Bottom bar:
  - 1 px divider; padding 8 px vertical, 12 px horizontal;
  - round "?" at the left;
  - at the right, in macOS order: **Cancel**, **Apply**, **OK**. OK is the default (#3871E1); Apply is enabled only
    after a change.
- Opens on Appearance the first time.
- Category tree, in order (empty groups are hidden):
  - **Appearance &amp; Behavior**:
    - Appearance, Accessibility, Menus and Toolbars;
    - System Settings ▸ (Date Formats, HTTP Proxy, Language and Region, Passwords, Server Certificates, Updates);
    - File Colors, Scopes, Notifications, Quick Lists, Required Plugins, Trusted Locations, Path Variables,
      Presentation Assistant.
  - **Keymap**.
  - **Editor**:
    - General ▸ (sub-pages, inferred);
    - Code Editing;
    - Font ▸ (Console Font);
    - Color Scheme, Code Style, Inspections, File and Code Templates, File Encodings, Live Templates, File Types,
      Inlay Hints, Emmet, Intentions, Reader Mode, TextMate Bundles.
  - **Plugins**.
  - **Version Control**: Changelists, Commit, Confirmation, Directory Mappings, File Status Colors, Ignored Files
    (inferred), Issue Navigation, Log, Shelf, then Git, GitHub, GitLab.
  - **Build, Execution, Deployment**: Build Tools, Debugger, Run Targets, Run Widget (inferred).
  - **Languages &amp; Frameworks**: Schemas and DTDs ▸ (Default XML Schemas, XML Catalog), Template Data Languages.
  - **Tools**: Actions on Save, Diff &amp; Merge ▸ (External Diff Tools), External Tools, Startup Tasks, Terminal,
    Web Browsers and Preview.
  - **Backup and Sync** (inferred).
  - **Advanced Settings**.
- "<app name> setting" badge (#367), shown after reference-app-only checkboxes:
  - a pill 16 px tall, fully rounded, 6 px side padding;
  - text "<app name> setting" at 11 px medium;
  - fill #FFFFFF29 / #191A1C1A, text #FFFFFFCC / #5F6269.

### Appearance & Behavior › Appearance
- "Theme:" combo, "Sync with OS", the OS-preference button, then the indented editor scheme combo.
- "Different tool window background" (Islands only). Comment: "Use lighter color in the dark theme and darker color
  in the light theme as a background".
- "Use custom font:" with a font combo and "Font size:".
- UI Options:
  - Compact mode;
  - Always show full path in window header;
  - Enable smooth scrolling;
  - Use project colors in main toolbar (on);
  - Custom application icon;
  - Keep popups open for toggle items;
  - Drag-and-drop with Alt pressed only;
  - Display icons in menu items;
  - "Background Image…" button.
- Tree Views: Show indent guides, Use smaller indents, Expand node with single click.
- Tool Windows: Show tool window bars, Show tool window names, Side-by-side layout on the left / right,
  Widescreen tool window layout, Remember size for each tool window.
- Presentation Mode: "Zoom:", "Font size:".
- Antialiasing: "IDE:", "Editor:".

### Appearance & Behavior › System Settings
- "Confirm before exiting the reference app".
- What to do with a running process when its tool window closes: Terminate / Disconnect / Ask.
- Project:
  - Reopen projects on startup;
  - "Open project in" New window / Current window / Ask;
  - "Default project directory:";
  - **"Store project settings in the project root directory"**: reference-app-only, badge, "Requires restart" (#191);
  - **"Enable indexing files for search"**: reference-app-only, badge (#360).
- Files: autosave options, worded with "the reference app" ("Save files if the reference app is idle for N seconds", …).

### Version Control
- Root page: only a description and links to its sub-pages.
- Git:
  - "Path to Git executable:" with a Test button;
  - "Automatically exclude ignored directories from the analysis".
  - Commit group:
    - Enable staging area (off). Comment: "This will disable changelists support. Only for non-modal commit
      interface.";
    - Warn about CRLF, detached HEAD or rebase, files larger than 50 MB, problematic file names (all on);
    - Add the 'cherry-picked from' suffix.
  - Push group:
    - Auto-update if push of the current branch was rejected (off);
    - Show Push dialog for Commit and Push, with "only when committing to protected branches" indented below;
    - "Protected branches:", with "Load branch protection rules from GitHub" indented below.
  - Update group:
    - "Update method:" Merge / Rebase / Branch Default (Merge);
    - "Clean working tree using:" Shelve;
    - **Fetch remote changes automatically: on** (#362);
    - Check remote for incoming changes;
    - Execute Branch Operations on All Roots.
  - "Fetch tags:" Auto / Sync / Always / Never. "Use credential helper".
  - Stash group: Combine stashes and shelves in one tab; Show Diff compares stashed changes with the local file or
    with the parent commit.
  - Cherry-Pick group: on an empty cherry-pick, Skip commit / Create empty commit.
  - "Filter "Update Project" information by paths:".
- Commit:
  - "Clear initial commit message" (on);
  - "Commit message inspections:" table: Blank line between subject and body, Limit subject line length,
    Limit body line length;
  - Commit Checks group;
  - Advanced Commit Checks group, with "Run advanced checks after a commit is done".
- Log:
  - Location group: **"Show the log in the editor window"**, on, reference-app-only, badge.
  - View Options group:
    - Show only the first reference for a commit (on);
    - Show tag names (off);
    - Show the commit time rather than the author time (off);
    - Show references on the left (off);
    - Display changes to each merged commit separately (off);
    - Issue preview on hover after 500 ms (on);
    - Diff preview panel (off), Location Bottom / Right;
    - Visible Columns: Author, Date, Hash.
  - Indexing group: "Enable indexing for project".
  - File History group: Display details panel, Show file names, Diff preview, Columns.
- Confirmation:
  - when files are created: Ask / Add silently / Do not add;
  - when files are deleted: Ask / Remove silently / Do not remove;
  - show options before operations;
  - Changes group: Check for conflicts every 5 minutes, Highlight files changed in the last N days, when a patch is
    created, Restore workspace when switching branches, Limit history to N rows.
- Changelists: Create changelists automatically, Allow putting changes within one file into different changelists,
  inactive-changelist and conflict options.
- GitHub:
  - account list (empty text "No accounts added", link "Add account…");
  - Clone git repositories using ssh;
  - Automatically mark opened files as viewed;
  - Enable unread markers;
  - "Connection timeout: N seconds".
- GitLab: account list, Automatically mark opened files as viewed, Clone using SSH, collapsible
  "Custom Server OAuth Applications".
- Keymap page:
  - "Get more keymaps in Settings | Plugins" link; keymap combo with a gear;
  - toolbar: Expand All, Collapse All, Edit Shortcut, system-conflict warning;
  - a search field and "Find Actions by Shortcut";
  - an action tree with the right-click menu Add Keyboard Shortcut, Add Mouse Shortcut, Add Abbreviation,
    Remove, Reset Shortcuts.

## 8. Keymaps and default macOS bindings
- Keymap combo on macOS, in order:
  1. **macOS** (the default);
  2. Emacs;
  3. IntelliJ IDEA Classic;
  4. macOS System Shortcuts;
  5. Sublime Text;
  6. Sublime Text (macOS).
  - Windows, GNOME, KDE and XWin keymaps are hidden on macOS.
  - The reference app removed the keymap-extension plugins (#227).
- Commit ⌘K (also focuses the message) · commit from the panel ⌘↩ or ⌃↩ · Commit and Push ⌥⌘K · Push ⇧⌘K ·
  Update Project ⌘T.
- VCS Operations Popup ⌃V · New Branch ⌥⌘N · Show Diff ⌘D · Rollback and Rollback Lines ⌥⌘Z · Add ⌥⌘A ·
  Move to Another Changelist ⇧⌘M · Commit Message History ⌃M · Amend ⌃⌥M · Commit Options ⌃O ·
  Shelve Silently ⇧⌘H · Unshelve ⇧⌘U · Rename branch / edit commit message F2 · Copy Revision Number ⌥⇧⌘C.
- No shortcut: Pull, Fetch, Git Branches popup (its ⇧⌘` is removed on macOS), Show History, Annotate, Merge, Rebase,
  Stash, Clone, Cherry-Pick, Create Patch, Reset, Revert.
- Log:
  - Refresh ⌘R · Focus Text Filter ⌘L · Go To Hash/Branch/Tag ⌘F · New log tab ⌥G (#216; Ctrl+G removed in #135);
  - Show All Affected Files ⌃⌘A · parent commit → · child commit ← · commit tooltip F1 or ⌃J;
  - Navigate Log to Selected Branch Head ⌥F1.
- Diff:
  - Next / previous difference F7 / ⇧F7 · next / previous file ⌃⇧→ / ⌃⇧←;
  - Accept Left / Right ⌃⌘→ / ⌃⌘← · other side ⌃⇧⇥ · diff settings ⇧⌘D · Jump to Source ⌘↓ or F4.
- Tool windows: Commit ⌘0 · Project ⌘1 · Bookmarks ⌘2 · Git ⌘9 · Terminal ⌥F12 · last tool window F12 ·
  hide active ⇧⎋ · hide all ⇧⌘F12.
- Global:
  - Settings ⌘, · Search Everywhere ⇧⇧ · Find Action ⇧⌘A · Go to File ⇧⌘O · Recent Files ⌘E · Find in Files ⇧⌘F;
  - New ⌘N · Close Tab ⌘W · next / previous tab ⇧⌘] / ⇧⌘[ · Back / Forward ⌘[ / ⌘];
  - Reload All from Disk ⌥⌘Y · Copy Path ⇧⌘C · Expand / Collapse All ⌘= / ⌘- · Save All ⌘S.
- Trees and filters:
  - Group by Directory ⌃P · toggle a checkbox Space;
  - filter chips: ↩ or ↓ opens, ⌦ or ⌫ clears;
  - delete a branch in the branches pane ⌘⌦;
  - toggle favorite in the branches popup Space.

## 9. Menus and popups (separators as "---")

### Commit table (log)
- Copy Revision Number (expui/general/copy.svg, ⌥⇧⌘C)
- Create Patch… (expui/vcs/patch.svg)
- Cherry-Pick (icons/new/cherryPick.svg)
- ---
- Checkout ▸: the local branches at this commit, then "Revision 'abc12345'". With no local branch there, a single
  item "Checkout Revision 'abc12345'".
- Show Repository at Revision
- Compare Versions (two commits)
- Compare with Local
- ---
- Reset Current Branch to Here… (expui/vcs/revert.svg)
- Revert Commit ("Revert Commits" for several)
- Undo Commit…
- ---
- Edit Commit Message… (F2)
- Fixup…
- Squash Into…
- Drop Commits
- Squash Commits…
- Interactively Rebase from Here…
- Push All up to Here…
- ---
- Branch operations:
  - "Branch 'x'" ▸ for each branch at the commit, or one "Branches" ▸ when there are more than 2;
  - with no other branch: "Rebase &lt;current&gt; onto Selected Commit";
  - then "Tag 'v1'" ▸, or "Tags" ▸ when there are several.
- New Branch… (⌥⌘N)
- New Tag…
- ---
- Go to Child Commit (←)
- Go to Parent Commit (→)
- ---
- Open on GitHub / View in browser (GitLab), only for hosted remotes.

### Changes tree in the log
- Show Diff (⌘D) · Show Diff in a New Tab or Window · Compare with Local · Compare Before with Local ·
  Edit Source (⌘↓) · Open Repository Version
- ---
- Revert Selected Changes · Cherry-Pick Selected Changes · Extract Selected Changes to Separate Commit… ·
  Drop Selected Changes · Create Patch… · Get · History Up to Here · Show Changes to Parents (toggle, off)

### Branches pane tree (the same items as the branches popup's branch submenu)
- Local branch:
  - Checkout · New Branch from 'x'… · Checkout and Rebase onto 'cur' · Checkout and Update
  - ---
  - Compare with 'cur' · Show Diff with Working Tree
  - ---
  - Rebase 'cur' onto 'x' · Merge 'x' into 'cur'
  - ---
  - New Worktree from 'x'…
  - ---
  - Update · Push… (⇧⌘K) · Tracked Branch 'origin/x' ▸
  - ---
  - Rename… (F2) · Delete
- Current branch:
  - New Branch from 'cur'…
  - ---
  - Show Diff with Working Tree
  - ---
  - New Worktree…
  - ---
  - Update · Push… · Tracked Branch ▸
  - ---
  - Rename…
- Remote branch:
  - Checkout · New Branch from… · Checkout and Rebase onto 'cur'
  - ---
  - Compare · Show Diff with Working Tree
  - ---
  - Rebase · Merge
  - ---
  - New Worktree…
  - ---
  - Pull into 'cur' Using Rebase · Pull into 'cur' Using Merge
  - ---
  - Delete
- Tag:
  - Checkout
  - ---
  - Show Diff with Working Tree
  - ---
  - Merge
  - ---
  - New Worktree…
  - ---
  - Push to &lt;remote&gt;
  - ---
  - Delete
- Several branches: Compare Branches · Show Files Diff · Update Selected · Delete Branches.
- A remote node: Edit Remote · Remove Remote. The "Remote" group: Manage Remotes….
- Names in labels are quoted and cut at 40 characters.

### Git branches popup (opened from the toolbar VCS widget)
- At least 375×312, resizable, size remembered.
- Fill #26282C / #FFFFFF, border #33353B / #D1D3D9.
- Header: an 8 px drag strip, then a search field "Search for branches and actions" with Fetch and a gear on its right.
- Top actions:
  - Commit… (⌘K);
  - ---;
  - Abort / Continue items, only during an operation;
  - New Branch… (⌥⌘N) · New Worktree… · Checkout Tag or Revision…;
  - ---.
  - The reference app removed Update Project and Push from here; they are in the toolbar.
- Sections: Recent, Local, Remote, Tags. Recent and Local start expanded.
- Rows 24 px:
  - icon, 4 px, name;
  - incoming/outgoing counts;
  - the tracked remote branch right-aligned in #808080;
  - a submenu arrow.
  - A selected non-favorite row shows expui/nodes/starEmpty.svg; click it or press Space to make it a favorite.
- Gear menu:
  - Show Actions in Search Results (on)
  - ---
  - Group by Directory (on)
  - Show Recent Branches (on)
  - Show Tags (on)
  - Restore Popup Size

### Commit tool window, modified file
- Merge…, Accept Theirs, Accept Yours, --- and Revert Resolved (conflicts only)
- Commit File… · Rollback… (⌥⌘Z) · Move to Another Changelist… (⇧⌘M) · Show Diff (⌘D) ·
  Show Diff in a New Tab or Window · Jump to Source (⌘↓)
- Copy Path/Reference… ▸:
  - Absolute Path ⇧⌘C · File Name;
  - ---;
  - Path with Line Number · Path from Content Root · Path from Source Root · Path From Repository Root ·
    Copy Link to GitHub/GitLab Repository;
  - ---;
  - Copy Reference ⌥⇧⌘C.
- ---
- Delete (⌫) · Add to VCS (⌥⌘A) and Add to .gitignore ▸ (unversioned only)
- ---
- New Changelist… · Delete Changelist · Set Active Changelist (⌃Space) · Edit Changelist… (F2)
- Create Patch from Local Changes… · Copy as Patch to Clipboard · Shelve Changes…
- ---
- Refresh
- ---
- Local History ▸
- Then the Git items inline, not in a submenu (#113):
  - Commit File… · Add · Add to .gitignore;
  - ---;
  - Annotate · Compare with Same Repository Version · Compare with Revision… · Compare with Branch or Tag… ·
    Show History · Show History for Selection… · Show Current Revision · Rollback… · Rollback File…;
  - ---;
  - Resolve Conflicts… · Revert Resolved.
  - Some entries may appear twice (inferred).

### Project tool window, file or folder
- New ▸ (⌘N)
- ---
- Cut · Copy · Copy Paths (⇧⌘C) · Copy Path/Reference… ▸ · Paste
- Jump to Source (⌘↓)
- ---
- Rename… (⇧F6) · Refactor ▸ · Bookmarks ▸
- ---
- Delete (⌫)
- ---
- Open in Right Split (⇧↩)
- Open In ▸: Finder, Associated Application, Terminal, GitHub/GitLab, File Path ⌥⌘F12
- ---
- Local History ▸ · the Git items inline (as above) · Reload from Disk
- ---
- Compare Files (⌘D, two selected) · Compare File with Editor
- ---
- External Tools
- ---
- Mark Directory As ▸
- Then Create Gist… / Create Snippet… when a hosted remote exists.
- The reference app removed Find Usages, Find in Path, Replace in Path, Inspect Code, Reformat Code and Optimize Imports.

### Project widget popup
- Open… (expui/general/open.svg)
- Clone Repository…
- Header "Open Projects", then each open project.
- Header "Recent Projects", then up to 100 recent projects.
- No "New Project…" item.
- Typing filters by name and path.
- Headers: padding 3 px vertical, 16 px sides; no line above the first.
- Project rows:
  - 6 px vertical padding;
  - 20 px badge, top-aligned, 8 px gap;
  - name 13 px, then path and branch at 12 px #73767C;
  - selection inset 8 px, 4 px radius, #2A4371 (inferred, theme fallback differs) / #E3EBFE.

## 10. Design tokens

### Fonts
- UI: the macOS system font (SF Pro, `.AppleSystemUIFont`), 13 px regular. It is not Inter.
- Type scale from 13 px:
  - h0 25 bold · h1 22 bold · h2 18 · h3 16 · h4 14 bold;
  - regular 13 · medium 12 · small 11.
- Monospace: JetBrains Mono 13 px, line height 1.2. Used for diffs, the commit message field and the commit details
  message.
- Other uses: tool-window titles bold 13; popup footers 12; help-tooltip description 12; ref labels 12.

### Colors
- Text:
  - default #D1D3D9 / #000000;
  - secondary #73767C;
  - muted #9FA2A8 / #5F6269;
  - disabled #4C4F56 / #9FA2A8.
- Links #71A1FE / #2F5EB9. Error #F57E84 / #C54E58. Warning #D59637 / #A56906. Success #6DB083 / #338555.
- Accent #3871E1 in both themes: focus, default button, open stripe button. Checkboxes use #3574F0.
- Separators #33353B / #E9EAEE, also in popups and menus.
- Icon stroke #CED0D6 / #6C707E (expui SVG defaults).

### Selection and hover
- Lists, trees, tables:
  - focused selection #2A4371 / #D0DFFE, text unchanged;
  - unfocused #33353B / #E9EAEE;
  - hover #FFFFFF10 / #00000008;
  - secondary selection #233558 / #E3EBFE.
- Selection is a rounded rectangle, 4 px radius:
  - trees: inside 12 px side padding;
  - log table: inset 4 px;
  - popups: inset 8 px.
- Rows: lists and trees 24 px (22 compact); log table 26 px; popup list rows 24 px.

### Controls
- Buttons:
  - at least 72×28 (72×24 compact), 4 px radius, 1 px border #40434A / #D1D3D9;
  - fill transparent / #FFFFFF; text 13 px;
  - default button: fill and border #3871E1, white text;
  - split-button separator #FFFFFF3B / #FFFFFF45.
- Text fields: at least 49×28, 4 px radius, border #40434A / #D1D3D9, fill #191A1C / #FFFFFF.
- Combo boxes:
  - at least 49×28; padding 1 px top, 9 px left, 1 px bottom, 6 px right;
  - 28 px arrow area, chevronDown 16 px;
  - fill #26282C / #FFFFFF when not editable.
- Checkbox:
  - a 16 px box with a 3 px radius, centered in a 24 px cell; 4 px gap to the label;
  - unchecked: #2B2D30 fill, #5A5D63 border / #FFFFFF fill, #A8ADBD border;
  - checked: #3574F0 with a 2 px white check;
  - focused: 2 px #3574F0 border.
- Radio: a 16 px circle with the same colors and a 6 px white dot.
- Focus ring:
  - 2 px, outside the control, follows its radius;
  - #3871E1 in both themes;
  - error #C54E58 / #E4656E; warning #A56906 / #C28013.
- Icon buttons in toolbars:
  - 22×22 with a 16 px icon, 4 px radius;
  - hover #FFFFFF17 / #00000012; pressed #FFFFFF29 / #00000020;
  - toolbar insets 5 px vertical, 7 px horizontal.
- Segmented button: selected fill #26282C / #FFFFFF, border #5F6269 / #B5B7BD.
- Counter: fill #73767C / #8B8E94, text #000000 / #FFFFFF.

### Popups and menus
- Popups:
  - fill #26282C / #FFFFFF, 1 px border #33353B / #D1D3D9;
  - about 8 px corner radius with the system shadow (inferred).
- List popup rows:
  - 24 px; item padding 1 px vertical, 12 px horizontal; icon-to-text gap 6 px;
  - selection inset 8 px, 4 px radius, #2A4371 / #D0DFFE;
  - shortcuts right-aligned in #73767C.
- Popup body padding: 8 px top (with no header), 8 px bottom.
- Popup header: padding 12, 10, 10, 10; 24 px tall (28 with controls).
- Popup search field: padding 4 px vertical, 12 px horizontal.
- Separators: 4 px vertical, 8 px horizontal insets. A labeled separator uses #73767C text (inferred) with
  insets 3 px vertical, 16 px horizontal.
- Popup footer: fill #26282C / #F7F8F9, 12 px text in #73767C, padding 8 px vertical, 12 px horizontal.
- Context menus:
  - fill #26282C / #FFFFFF, border #33353B / #E9EAEE, 8 px corner radius, 6 px padding at top and bottom;
  - item selection 4 px radius, with outer insets 1 px vertical / 7 px horizontal and inner padding 6 px horizontal;
  - shortcut text #73767C;
  - separators 9 px tall with a 1 px line;
  - at least 180 px wide.

### Tooltips, scrollbars, banners
- Tooltip: fill #33353B / #FFFFFF, border #33353B / #D1D3D9, text #D1D3D9 / #000000 at 13 px, 4 px radius.
- Help tooltip:
  - shortcut in #9FA2A8 / #73767C;
  - description at 12 px in #9FA2A8 / #5F6269, 6 px below the title;
  - placed 17 px below the cursor on macOS.
- Validation tooltip, at most 384 px wide:
  - error: fill #56272B / #FFF6F5, border #80383E / #FFC4C5;
  - warning: fill #44321D / #FFF6E9, border #694820 / #F4CD9A.
- Scrollbars:
  - macOS overlay style: thumb only, shown while scrolling or on hover;
  - dark thumb #80808059, hover #8080808C;
  - light thumb #00000033, hover #00000080;
  - about 7 px wide, widening on hover (inferred).
- Banners, 6 px radius, fill / border:
  - info #233558 / #F7F8FF, border #2E4D89 / #BDD3FF;
  - success #203B2A / #F5FAF3, border #29583C / #BBDBC2;
  - warning #44321D / #FFF6E9, border #694820 / #F4CD9A;
  - error #56272B / #FFF6F5, border #80383E / #FFC4C5.
- Notification balloon: fill #33353B / #FFFFFF, border #33353B / #D1D3D9.

## Biggest visible differences to fix first
Judged against Spork's current code (`crates/spork/src`, `crates/spork_workbench`), ordered by how much each changes the
first impression.

1. **Log chrome.** Put the log in an editor-tab row inside the main island: a "Log" pill tab with the branch icon and
   no ×.
   - Drop Spork's column header row (Graph / Description / Author / Date / Commit).
   - The reference app shows no header.
2. **Details to the right, not below.**
   - The reference app splits the table and a right column 70/30.
   - The right column has the changed-files tree ("16 files", folders with grey "N files") above the details.
   - Spork stacks details under the log.
3. **Graph look.**
   - Draw the graph inside the Subject column; Spork gives it its own column.
   - Use 1 px lines and 9 px solid dots, with the HEAD ring node and no hollow merge rings.
   - Lanes are about 17 px apart at 26 px rows.
   - Colors are hashed from the ref name (HSB S 0.6, B 0.6 / 0.7, muted). Spork uses 6 saturated theme colors and
     1.5 px strokes.
4. **Commit rows.**
   - 26 px rows; Subject, Author and Date only (no hash column).
   - Dates read "Today 6:56 PM" or "17/6/26, 6:56 pm", not "3d".
   - Merge commits dimmed to #6F737A / #818594; my commits bold in Author.
5. **Ref labels.**
   - Right-aligned inside the subject, 12 px grey text.
   - Stacked tag icons in the ref colors.
   - One compact label per commit ("origin & main"), long names cut to 22 characters.
6. **Log toolbar.**
   - Order: "Text or hash" field with .* / Cc toggles first, then the Branch / User / Date / Paths chips
     ("Branch: HEAD ×").
   - Right side: Refresh, Cherry-Pick, eye, search.
   - Spork's bar is a different layout: "Current branch" toggle, "Only … ✕", the search on the right, a commit count.
7. **Commit tool window.**
   - One "Changes N files" tree with checkboxes and a collapsed "Unversioned Files" group.
   - The 9-item toolbar.
   - "Amend [last commit ▾]" with the history clock and the added/modified/deleted legend.
   - A JetBrains Mono message box with "Commit Message".
   - "Commit" / "Commit and Push…" and a gear for Author and Sign-off.
   - Spork shows Staged / Unstaged sections, a filter field, and Amend / Sign-off checkboxes in the panel.
8. **Branches live in the log.**
   - A collapsed "Branches" strip and pane: HEAD (Current Branch) / Local / Remote / Tags, with a vertical toolbar.
   - It replaces Spork's Fork-style "Repository" tool window (Favorites, Remotes, PRs, Stashes, Worktrees,
     Submodules with colored section icons).
9. **Main toolbar content and order.**
   - Project badge + name ▾ · ↙ Update · ↗ Push · ⎇ branch ▾ with arrow icons on the left; Search and the gear on
     the right.
   - Spork shows the branch chip with "↑2 ↓1" text before Pull / Push / Fetch / Stash buttons.
10. **UI font.** Use the macOS system font (SF Pro) at 13 px instead of Inter. Every label changes.
11. **No in-window repo tab strip.** The reference app uses one project per window. Spork draws repository tabs in the title
    bar.
12. **Project badge.**
    - A diagonal 2-color gradient from the 9-slot palette, assigned round-robin.
    - JetBrains Mono SemiBold initials by the CamelCase / first-and-last-word rule.
    - Spork uses flat colors from a name hash with Inter Bold 10 px.
13. **Project color gradient.** Add the 600×300 project-color glow behind the toolbar, centered on the project widget.
14. **Stripes.**
    - Left top: Project, Commit (+ Pull/Merge Requests), "⋯". Left bottom: Terminal, Git. Right stripe: Notifications.
    - Spork has Commit, Project, Repository on top and Terminal, Git, Activity at the bottom, with no right stripe.
15. **Git branches popup.**
    - A resizable 375×312 searchable tree popup with Recent / Local / Remote / Tags.
    - Stars, in/out counts, the tracked branch at the right.
    - Top actions: Commit…, New Branch…, New Worktree…, Checkout Tag or Revision….
    - Spork's popup is a flat menu that still lists Update Project and Push.
16. **Selection and hover style.**
    - Rounded 4 px selection inset from the edges, #2A4371 / #D0DFFE focused and #33353B / #E9EAEE unfocused.
    - Log hover is 7 % white in dark and #E9EAEC in light.
    - Check that Spork's lists and the log match these.
17. **Welcome screen.**
    - A separate 800×650 window: left nav (Projects / Customize / Plugins), search, and the New Project / Open /
      Clone Repository buttons.
    - Badge rows with name, path and branch.
    - The empty state with three 60 px tiles.
    - Spork shows cards inside the repository window.
18. **Status bar.** Show only the focused file's breadcrumbs and task progress. The reference app shows nothing for the log, and
    no branch; Spork shows "repo › branch".
19. **Settings window.**
    - One 900×700 non-modal Settings window with the category tree (Version Control › Git / Log / Commit, …).
    - Bold breadcrumbs, and Cancel / Apply / OK.
    - Spork has a "Repository settings" dialog and separate preferences.
20. **Controls.**
    - 16 px #3574F0 checkboxes with a 3 px radius.
    - 28 px buttons with a 4 px radius and a 1 px border; the default button is #3871E1.
    - 2 px #3871E1 focus rings.
    - Islands editor-tab pills: 28 px, 6 px radius, #233558 / #E3EBFE with a border.
    - Bold 13 px tool-window titles in a 41 px header.
    - Spork's gpui-kit controls should be checked against these.
