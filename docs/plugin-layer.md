# A plugin layer of our own: a proposal

Status: proposal only, nothing of it is built. The user decides the scope first (the last section lists the
decisions). It builds on what the plugins wave found making every kind of Zed extension work in Spork IDE (ROADMAP,
Next, item 3) and on IntelliJ's plugin model (`~/Projects/intellij-community`).

## 1. What Zed's extensions are, as Spork IDE runs them

Zed's extension system is one of the engines Spork IDE keeps (Decision 14). The plugins wave checked every kind of
extension end to end, with fixture extensions built from `crates/extension_api` and served by the test registry
(`tools/e2e/extensions_http.py`, `tools/e2e/fixtures/extensions`), and by hand with four real ones from Zed's registry
(Git Firefly, HTML, JetBrains Themes, TOML: API 0.7.0, wasi-sdk grammars, taplo downloaded from GitHub).

| Kind | In Spork IDE |
| --- | --- |
| Languages, grammars (WebAssembly tree-sitter parsers) | Work: highlighting, outline, brackets, the editor's language. A language Spork IDE has natively (HTML) shadows the extension's; its language server still attaches. |
| Language servers (`language_server_command`, downloads) | Work: `download_file`, `process:exec`, GitHub releases; diagnostics, completion and the LSP menus `spork_code` builds. |
| Themes | Work: listed in Settings › Appearance, applied as installed (IntelliJ's way, since this wave). Reach the editor and the parts of the frame still drawn from Zed's theme, not Spork's components (see below). |
| Icon themes | Work: applied as installed, as IntelliJ's icon packs; the Project tool window's and the tabs' file icons. |
| Snippets | Work: in the completion, as IntelliJ's live templates show there. Fixed this wave: they now leave with their extension. |
| Context servers (MCP) | Work: started for AI Chat's agent, which lists their tools. Configured in AI Chat's own settings view (Zed's, until AI Assistant is rebuilt). |
| Debug adapters, debug locators | Adapters work: a configuration type of Run/Debug Configurations, debugging in the Debug tool window. Locators load but aren't checked end to end: they turn a language's runnables into debug scenarios, and the fixture language has none. |
| Slash commands, indexed docs providers | Don't work: upstream Zed removed text threads, slash commands and docs indexing (zed#52757); the API keeps the calls, nothing calls them. |
| Language model providers | Unreleased upstream (API 0.8.0, dev builds only; Zed's registry rejects them). |
| Capabilities (`process:exec`, `download_file`, `npm:install`) | Work, through `granted_extension_capabilities`, which grants everything by default. |

How the engine is built, which decides what a layer on top of it can be:

- **WebAssembly components** run in wasmtime. The API is a WIT world, `zed:extension`, versioned by copying it
  (`crates/extension_api/wit/since_v0.0.1` … `since_v0.8.0`); an extension's `zed:api-version` custom section picks
  the host-side binding (`extension_host/src/wasm_host/wit.rs`), so every old extension keeps loading.
- **Contributions are data plus callbacks.** Themes, icon themes, languages, grammars and snippets are files the host
  reads; the WebAssembly part only answers the host's questions (which command starts this language server, how to
  label a completion, which binary is this debug adapter, which command starts this context server). An extension
  cannot add an action, a keymap entry, a panel, a settings page, a notification, a menu item or a status bar widget:
  **Zed's extensions contribute no UI at all.**
- **Sandbox.** WASI with one preopened folder, the extension's work folder. `process:exec`, `download_file` and
  `npm:install` are checked against `granted_extension_capabilities` (`extension_host/src/capability_granter.rs`);
  the `http-client` interface's `fetch` is not checked, and the commands an extension returns for a language server,
  a debug adapter or a context server run outside the sandbox with the user's rights.
- **Distribution.** Zed's registry (`api.zed.dev/extensions`), fed by pull requests to `zed-industries/extensions`,
  built and validated by `extension_cli` in Zed's CI. Archives aren't signed. `extension_cli` writes the archive's
  `extension.toml` from its own manifest type and copies only the files it knows, so anything else in a package is
  dropped on the way to Zed's registry. Spork IDE keeps using Zed's registry for now (Decision 12).

## 2. IntelliJ's plugin model, the reference

An IntelliJ plugin is a JVM module with a descriptor, `META-INF/plugin.xml` (parsed by
`platform/core-impl/src/com/intellij/ide/plugins/PluginDescriptorLoader.kt`; the model is
`platform/core-api/src/com/intellij/ide/plugins/IdeaPluginDescriptor.java`): its id, name, version, vendor, the IDE
builds it supports (`<idea-version since-build until-build>`), what it depends on (`<depends>`, optional ones with a
config file of their own), and what it contributes:

- **Extension points.** The platform declares its extension points in XML (`<extensionPoint name= interface= dynamic=>`)
  and plugins register implementations under `<extensions defaultExtensionNs="com.intellij">`; plugins declare their
  own for other plugins (`platform/extensions/src/com/intellij/openapi/extensions/ExtensionPointName.kt`).
- **Actions** (`AnAction`, `platform/editor-ui-api/.../actionSystem/AnAction.java`): `<actions><action id class text>`
  with `<add-to-group group-id anchor>` (a menu, a toolbar, a context menu) and `<keyboard-shortcut keymap
  first-keystroke>` per keymap; Find Action finds every action by its text.
- **Tool windows**: the `toolWindow` extension point (`ToolWindowFactory`, its id, anchor, icon, secondary or not;
  declared in `platform/platform-api/resources/intellij.platform.ide.xml`).
- **Settings pages**: `applicationConfigurable` and `projectConfigurable` (`Configurable`: `createComponent`,
  `isModified`, `apply`, `reset`, which is the Settings dialog's Apply/Cancel; `parentId` places it in the tree;
  `platform/ide-core/resources/intellij.platform.ide.core.xml`).
- **Notifications**: `notificationGroup` (balloon, sticky balloon or tool window; `NotificationGroup.kt`).
- **Run configurations**: `configurationType` (`ConfigurationType` → `ConfigurationFactory` → `RunConfiguration` with its
  `SettingsEditor`), `programRunner`, `runLineMarkerContributor` for the gutter
  (`platform/execution/resources/intellij.platform.execution.xml`).
- **Version control**: `vcs` (`AbstractVcs`, a whole VCS) and hooks such as `checkinHandlerFactory` (before and after a
  commit: checks, reformatting, the commit dialog's options; `platform/vcs-api/resources/intellij.platform.vcs.xml`,
  `platform/vcs-impl/resources/META-INF/VcsExtensionPoints.xml`).
- **Code insight**: `localInspection` (`LocalInspectionTool`, with its language, group path and default level, shown
  in Settings › Editor › Inspections) and `intentionAction` (`IntentionAction`, the ⌥⏎ menu;
  `platform/analysis-api/resources/META-INF/Analysis.analyzer.xml`), `completion.contributor`.
- **File templates**: `internalFileTemplate`, `fileTemplateGroup` and the plugin's `fileTemplates/` (Velocity
  templates the New menu offers).
- Others the IDE leans on: `statusBarWidgetFactory`, `editorNotificationProvider` (banners), `postStartupActivity`.

Plugins load and unload without a restart when every extension point they use is `dynamic="true"`
(`platform/platform-impl/src/com/intellij/ide/plugins/DynamicPlugins.kt`). There is **no sandbox**: a plugin runs in
the IDE's JVM with all its rights. Trust comes from the Marketplace (review, `PluginSignatureVerifier.kt`'s signatures)
and compatibility from the build ranges. Distribution is JetBrains Marketplace, custom repositories (an
`updatePlugins.xml` URL, Settings › Plugins › ⚙ › Manage Plugin Repositories) and Install Plugin from Disk.

The gap between the two models is the point of a layer of our own: what IntelliJ's plugins mostly add (actions, tool
windows, settings pages, run configurations, VCS hooks, inspections, file templates) is exactly what Zed's extensions
cannot.

## 3. The options

### A. No layer of our own: Zed's extensions only

Spork IDE's own features stay built-in crates (`spork_*`); what plugins need goes upstream to Zed's extension API.

- **Contributes**: what Zed's API allows, nothing more (no UI).
- **Sandbox**: Zed's, as above.
- **Distribution**: Zed's registry only.
- **Compatibility**: total, by definition.
- **Cost**: none now; upstream decides what comes and when (Zed has rejected UI extensions so far).
- **First**: nothing.

### B. A Spork world beside Zed's, in the same engine (recommended)

A second, versioned WIT world, `spork:plugin@0.1.0`, loaded by the same extension host and wasmtime. A package is a
Zed extension plus, optionally, `spork.toml` (its Spork contributions, declared) and `spork.wasm` (a component of the
Spork world): Zed's files stay as Zed's tools build them, Spork's sit beside them, so `extension_cli` and Zed ignore
them. A crate of ours (`spork_plugins`) reads `spork.toml`, registers the contributions and calls the component; the
`spork_plugin_api` crate is to it what `zed_extension_api` is to Zed's.

- **Contributes**, declared in `spork.toml` the way `plugin.xml` declares them, behaviour in `spork.wasm`:
  - actions with their text, icon, menu group and anchor, and keymap entries per keymap (IntelliJ's macOS and
    Windows/Linux), found by Find Action; running one calls `run-action(id, context)`, which answers with effects the
    IDE performs (open a file at a place, edit a buffer, show a notification, run a task, open a URL);
  - notifications as IntelliJ's balloons, with their actions as links;
  - settings pages in the Settings dialog, described by a JSON schema and drawn with Spork's controls (check boxes,
    combo boxes, text fields), their values under the plugin's key in `settings.json`, Apply and Cancel as IntelliJ's;
  - tool windows with declarative content (a tree, a list, a table, a form, Markdown), refreshed by the plugin and
    drawn by Spork's components, their toolbar actions plugin actions; no GPUI drawing of its own;
  - run configuration types over Zed's tasks and debug scenarios (fields from a schema; the component turns them into
    a task template or a debug scenario), and gutter run icons from a tree-sitter query, as `runnables.scm` already
    are;
  - VCS hooks before and after a commit (checks that return problems for the Commit tool window, as IntelliJ's
    `checkinHandlerFactory`), branch and log decorations; through Spork's VCS engine (`spork_git`), not a new VCS;
  - inspections and intentions as an in-process diagnostics and code-actions source beside the language servers
    (Problems tool window, the editor's highlighting, ⌥⏎), with a page in Settings › Editor › Inspections;
  - file templates for the New menu (`file_operations.rs`'s popup), with IntelliJ's variables;
  - status bar widgets with text and a popup of actions.
- **Sandbox and capabilities**: the component stays in WASI with its work folder. New capabilities, in Zed's form and
  granted the same way: `project:read` (the project's files, read-only, as a preopened folder), `project:write`
  (edits through the IDE, so they are undoable and in Local History), `vcs:read`, `vcs:hook`, `network` by host (and
  the `http-client` checked against it, which Zed's isn't). IntelliJ asks nothing at install; Spork IDE could show what
  a plugin asks for in its details and ask once, as browsers do. UI contributions need no capability: they are data
  the IDE draws.
- **Distribution**: Spork plugins can't go through Zed's registry (its CI drops files it doesn't know, and it is Zed's
  service). Our own registry with the same API shape as `api.zed.dev/extensions` (a static index and archives on a
  CDN or GitHub Releases are enough), so `ExtensionStore` lists and installs from both; the Plugins page's ⚙ gets
  Manage Plugin Repositories, as IntelliJ's. Archives signed (ed25519, the key in the IDE), which Zed's aren't.
- **Compatibility**: every Zed extension keeps working unchanged (same host, same files). A package can be both: in Zed
  its languages and servers work and the Spork parts are ignored; in Spork IDE both work. The Spork world is versioned
  as Zed's is (`since_v0.1.0` …), old plugins keep loading.
- **Cost**, in waves like this one: the manifest, loader, world and `spork_plugin_api` with actions, keymap and
  notifications, one; settings pages, one; run configurations and VCS hooks, one to two; inspections and intentions,
  one; tool windows with declarative content, two; the registry with signing and Manage Plugin Repositories, one to
  two. Upstream merges touch it little: it sits beside `extension_host`, with a hook or two in it.
- **First**: actions with keymap entries and Find Action, notifications, and settings pages: the smallest set that
  makes a useful plugin (a formatter or a tool runner with its options and a shortcut), every surface already built by
  Spork's components. Then run configurations and VCS hooks, then tool windows.

### C. Native plugins (Rust dynamic libraries in the IDE's process)

IntelliJ's model as it is: a plugin is code in the IDE's process with the whole API (GPUI views, entities, the
project).

- **Contributes**: anything the IDE's own crates can, custom-drawn tool windows included.
- **Sandbox**: none, as IntelliJ's.
- **Distribution**: our own registry, one binary per platform and per IDE version.
- **Compatibility**: with Zed's extensions, unaffected (they stay on the engine); with itself, poor: Rust has no stable
  ABI, so a plugin is built against one exact IDE build (or behind a C ABI or `abi_stable`, which loses most of the
  API), and Zed's internal APIs change every week.
- **Cost**: high now and on every upstream merge.
- **First**: only as first-party, bundled plugins compiled in, which is what the `spork_*` crates already are.

### D. Out-of-process plugins over a protocol

A plugin is a separate process in any language, speaking a JSON-RPC protocol of ours, as language servers (LSP),
agents (ACP) and context servers (MCP) already do in the IDE, and as VS Code's extension host does.

- **Contributes**: the same declarative set as B, through messages instead of WIT calls.
- **Sandbox**: the operating system's (a process per plugin; macOS's sandbox profiles could confine it later), weaker
  than WebAssembly's until then: a process can read anything the user can.
- **Distribution**: our own registry, with a binary per platform or a runtime to install (Node, Python), as Zed's
  `npm:install` does for language servers.
- **Compatibility**: independent of Zed's extensions; a second plugin system beside them, with its own lifecycle,
  crashes and versions.
- **Cost**: the protocol, process management and their tests before any contribution, then the same work as B.
- **First**: nothing B can't do: a B plugin that needs a long-running helper can already start one with
  `process:exec`, as Zed's extensions start language servers.

## 4. Recommendation

B. It keeps Zed's extensions whole and the engine shared, adds what IntelliJ's plugins are mostly made of, keeps the
WebAssembly sandbox and its capabilities (tightened where Zed's are loose), and draws every contribution with Spork's
components, so plugins look like the IDE. A stays the fallback if a layer of our own isn't wanted yet; C only for
first-party code; D isn't worth its second lifecycle while B's `process:exec` covers helpers.

## 5. Decisions for the user

1. **A layer of our own, now or later?** Or Zed's extensions only (A) until the user's testing round is done.
2. **Which option**: B (recommended), C or D.
3. **UI**: declarative only (trees, lists, tables, forms, Markdown drawn by Spork's components), or plugins drawing
   their own views (only C allows it)?
4. **First contributions**: actions, keymap and notifications, settings pages (recommended); which next: run
   configurations, VCS hooks, inspections and intentions, tool windows, file templates?
5. **Distribution**: our own registry, where it is hosted, who reviews what is published, the signing key; whether the
   Marketplace tab shows Zed's and ours together (and whether Decision 12, Zed's registry, stays).
6. **Consent**: does installing a plugin show and ask for its capabilities? Should Spork IDE's defaults stop granting
   every capability to Zed's extensions (`granted_extension_capabilities` grants all today)?
7. **Languages for plugins**: Rust only (`wit-bindgen`, as Zed's), or every language with WebAssembly component
   tooling (JavaScript, Python, Go)?
8. **API promise**: how long a `spork:plugin` version keeps loading, and whether the two worlds' versions move together.
