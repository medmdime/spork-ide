//! The end-to-end tests' way in (tools/e2e): started with `SPORK_E2E=<socket path>`, the IDE reads
//! one JSON command a line on that Unix socket and answers each with one JSON line. Without the
//! variable none of this runs. The commands are Spork's (crates/spork/src/e2e.rs), so the
//! scenarios of both read alike.
//!
//! Commands: `{"do": "action", "name": "workspace::ToggleLeftDock"}` runs an action as its
//! shortcut would (`"args": {…}` its arguments); `{"do": "keys", "keys": "cmd-shift-p"}` types
//! keys, a ⌘ or ⌃ shortcut on the key the keymap's key equivalents give it on this keyboard
//! layout; `{"do": "type", "text": "a/b.txt"}` types text into the focused field, a keystroke a
//! character; `{"do": "open", "path":
//! "src/main.rs"}` opens a file of the project (relative to its first root) as the file finder
//! would; `{"do": "open_project", "path": "/abs/folder"}` opens a folder as `spork-ide <folder>`
//! would (`"new_window": true` in a window of its own); `{"do": "state"}` tells what shows; `{"do": "quit"}`.
//!
//! The state: `roots`, the project's folders; `items`, the open items (a file's path in its
//! root, another item's tab text) and `active`, the active one; `docks`, each dock's `open`, the
//! `panel` it shows and its `size` (a tool window's width, or height at the bottom); `split`, the
//! editor area's outermost split (`horizontal`, side by side, or `vertical`; null unsplit);
//! `focused`, the key contexts from the window down to the focused element
//! (`["Workspace", "Pane", "Editor"]`); `windows`, each window's `title`, `active` and `size`;
//! `look`, the `theme` showing and its `appearance`, the theme `selection` (`{"mode", "light",
//! "dark"}` or a name), the `icon_theme`, the `ui_font` and `buffer_font` with their sizes, the
//! Markdown preview's `markdown_preview_font_size`, the
//! editor's `line_height` and the `base_keymap`. With no project window open (the Welcome window
//! shows), the commands go to the first window and `state` has no `roots` or `items` (but has
//! `settings`).
//!
//! What shows, as macOS's accessibility sees it: `{"do": "ui"}` answers every window's tree of
//! elements (role, label, title, value, enabled, selected, frame in the window's points from its
//! top left); `{"do": "click", "label": "main.rs", "role": "AXRow", "nth": 0, "button":
//! "right"}` (or "back", "forward": a mouse's buttons 4 and 5) clicks the center of the first element whose label or title is that text, and
//! `{"do": "click", "at": [x, y]}` that point of the project window, `{"do": "click",
//! "line_number": 31}` that line's number in the active editor's gutter (`"gutter_mark": 31`
//! its coverage mark), `{"do": "click",
//! "console_text": "src/main.rs:4"}` that text in the Run tool window's console; without a label, the `nth`
//! element of the role; `"value": "Ask"` in place of the label finds a combo box by the choice
//! it shows; `"in": "Project Panel"` looks only under the element of that label; `"hover": true`
//! only moves the pointer there; `"count": 2` double-clicks and `"modifiers": ["cmd", "shift"]`
//! holds those keys down. GPUI builds the tree once something asks for it (and only when the app
//! runs with `ZED_EXPERIMENTAL_A11Y=1`), so the first `ui` may answer only the windows.
//! `{"do": "activate"}` makes the project window the active one, as a click into it would: some
//! actions (the theme selector's) open in the active window, or a new one when none is; with
//! "window", the window of that title (a dialog's).
//! `{"do": "front"}` puts the IDE's windows in front of other apps' before a screenshot, and
//! `{"do": "framed"}` tells whether each has drawn a frame since.
//! `{"do": "menubar", "path": ["File"]}` shows a menu bar menu (or `["File", "Open Recent"]`, its
//! submenu, or `[]` the menu bar's menus) to `ui` as the menu open last, until the next command
//! that acts.
//! `{"do": "drag", "label": "main.rs", "by": [40, 30]}` drags an element (found as `click` finds
//! it) by that many points.
//! `{"do": "scroll", "label": "Project Panel", "by": [0, -40]}` scrolls over an element (found as
//! `click` finds it, or `"at": [x, y]`) by that many points as a trackpad does, or `"lines": [0,
//! -3]` as a mouse wheel does; a negative `y` shows what's further down. `{"do": "scrollbar",
//! "label": …}` (or `"at"`) answers the scrollbar drawn over that element as it last painted: its
//! `opacity` (0 hidden, 1 shown), the content's `offset` and its `thumbs`, each with its `axis`,
//! its `width` and its `bounds`.
//! `{"do": "file_color", "path": "src/main.rs"}` answers the colour the Project tool window and
//! the editor tabs give that file for its version control status: its `name` (`Modified`,
//! `VersionControlUntracked`…) and `hex`.
//! `{"do": "vcs_console"}` answers the git commands the Git tool window's Console lists, as it
//! writes them (`git commit -F -`): `commands`, and the `last` one.
//! `{"do": "keys", "keys": "a b", "window": "Settings"}` types into the window of that title, as
//! `{"do": "type", "text": "…", "window": "Settings"}` does.
//! `{"do": "setting", "path": "/tab_size"}` answers the `value` the IDE's settings hold at that
//! JSON pointer (`settings.json`'s key path), its defaults and the user's file merged, or one of the
//! settings Spork IDE's crates keep (`/spork/local_history/days_to_keep`); the state's
//! `settings` is what the Settings dialog shows (`spork_settings::e2e_state`), null while closed;
//! `vcs` the line at the top of the active diff tab's diff (`spork_vcs::e2e_state`); `terminal`
//! the Terminal tool window's sessions, the active one, its panes, a rename under way and the
//! terminals' font size (`terminal_view::tool_window::e2e_state`);
//! `run` the run configurations, the Run tool window's tabs (title, whether it runs, its console's
//! text) and the breakpoints (`spork_run::e2e_state`); `code` whether the active file's language
//! servers are ready, the name an in-place rename shows and the last Inspect Code run's problems
//! (`spork_code::e2e_state`); `editor` the active editor's language, diagnostics and the
//! completions its menu shows; `context_servers` the project's context servers (MCP) and their
//! status; `java` the JDK, the Java language server and the Maven and Gradle tool windows
//! (`spork_java::e2e_state`); `kotlin` whether Kotlin is detected and its language server
//! (`spork_kotlin::e2e_state`); `http` the HTTP Client's runs and environments
//! (`spork_http::e2e_state`); `documentation` the Documentation tool window's symbol (`shown`), its
//! `text`, `popup_first` and `auto_update` (`spork_editor::documentation_e2e_state`);
//! `notifications` the Notifications tool window's `count`, `unread` and `expired` ones, the
//! `balloons` shown (each its title or text, oldest first), the "N more" stub's count (`more`) and
//! the stripe's unread dot (`badge`: "error", "info" or null)
//! (`spork_tool_windows::notifications_e2e_state`); `coverage` the coverage suites known and
//! shown, the Coverage tool window's rows, the active editor's gutter marks and their popup
//! (`spork_coverage::e2e_state`).
//! `{"do": "notify", "type": "info" | "warning" | "error", "title": "…", "text": "…", "actions":
//! ["Fetch", …], "sticky": true}` shows a notification as the workspace's balloon (an action logs
//! "Ran Fetch" when chosen); `{"do": "push", "type": …, "title": "…", "text": "…"}` pushes one as
//! Spork's views do (gpui-kit's `push_notification`).
//! `{"do": "pick", "paths": ["settings.zip"]}` answers the next file picker that asks through
//! `spork_workbench::prompt_for_paths` (relative to the project's first folder), which then
//! doesn't open.
//! `{"do": "modifiers", "held": ["ctrl"]}` presses (or, left out of the list, lets go of) modifier
//! keys alone, as the keyboard reports them: ⇧ pressed and let go twice is IntelliJ's double ⇧,
//! and letting go of ⌃ chooses in the Switcher.
#![cfg(unix)]

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};

use gpui::{AnyWindowHandle, App, Entity, Keystroke, OwnedMenuItem, Result, TaskExt as _};
#[cfg(target_os = "macos")]
use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, point, px,
};
use project::context_server_store::ContextServerStatus;
use serde_json::{Value, json};
use settings::{BaseKeymap, Settings as _};
use theme::{ActiveTheme as _, GlobalTheme};
use theme_settings::ThemeSettings;
use util::ResultExt as _;
use workspace::dock::Dock;
use workspace::{ItemHandle, MultiWorkspace, OpenOptions, Workspace};

type Command = (Value, smol::channel::Sender<Value>);

pub fn start(cx: &mut App) {
    let Some(path) = std::env::var_os("SPORK_E2E") else {
        return;
    };
    // for `scrollbar`
    gpui_base::record_painted_scrollbars(true);
    // a socket left by an earlier run
    if std::path::Path::new(&path).exists() {
        std::fs::remove_file(&path).log_err();
    }
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(e) => return log::error!("e2e: can't listen on {}: {e}", path.to_string_lossy()),
    };
    let (commands, received) = smol::channel::unbounded::<Command>();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let commands = commands.clone();
            std::thread::spawn(move || serve(stream, commands));
        }
    });
    cx.spawn(async move |cx| {
        while let Ok((command, reply)) = received.recv().await {
            let answer = cx.update(|cx| run(&command, cx));
            // the test hung up: nobody to answer
            reply.send(answer).await.ok();
        }
    })
    .detach();
}

fn serve(stream: UnixStream, commands: smol::channel::Sender<Command>) {
    let Some(mut writer) = stream.try_clone().log_err() else {
        return;
    };
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { break };
        let answer = match serde_json::from_str::<Value>(&line) {
            Ok(command) => {
                let (reply, answer) = smol::channel::bounded(1);
                if smol::block_on(commands.send((command, reply))).is_err() {
                    break;
                }
                smol::block_on(answer.recv()).unwrap_or_else(|_| json!({"error": "the IDE quit"}))
            }
            Err(e) => json!({"error": format!("not JSON: {e}")}),
        };
        if writeln!(writer, "{answer}").is_err() {
            break;
        }
    }
}

/// The first project window and the workspace it shows.
fn workspace(cx: &mut App) -> Option<(AnyWindowHandle, Entity<Workspace>)> {
    cx.windows().into_iter().find_map(|handle| {
        let multi_workspace = handle.downcast::<MultiWorkspace>()?;
        let workspace = multi_workspace.read(cx).ok()?.workspace().clone();
        Some((handle, workspace))
    })
}

thread_local! {
    /// The windows yet to draw a frame since `{"do": "front"}`.
    static UNFRAMED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// The menu bar menu `{"do": "menubar"}` shows, until the next command that acts.
    static MENU_BAR: std::cell::RefCell<Option<Value>> = const { std::cell::RefCell::new(None) };
}

fn run(command: &Value, cx: &mut App) -> Value {
    let what = command["do"].as_str().unwrap_or_default();
    match what {
        "quit" => {
            cx.quit();
            return json!({"ok": true});
        }
        #[cfg(target_os = "macos")]
        "ui" => {
            draw_windows(cx);
            return accessibility::tree(MENU_BAR.with_borrow(Clone::clone));
        }
        // before a screenshot: a covered window shows what it last presented
        #[cfg(target_os = "macos")]
        "front" => {
            accessibility::front();
            let windows = cx.windows();
            UNFRAMED.set(windows.len());
            for handle in windows {
                handle
                    .update(cx, |_, window, _| {
                        window.on_next_frame(|window, _| {
                            // drawn and presented in this same frame
                            window.refresh();
                            UNFRAMED.set(UNFRAMED.get().saturating_sub(1));
                        })
                    })
                    .log_err();
            }
            return json!({"ok": true});
        }
        "framed" => return json!({"framed": UNFRAMED.get() == 0}),
        "state" => {}
        // anything that acts closes the menu, as a click elsewhere would
        _ => MENU_BAR.set(None),
    }
    // without a project window (the Welcome window shows), commands go to the first window
    let found = workspace(cx);
    let Some(window) = found
        .as_ref()
        .map(|(window, _)| *window)
        .or_else(|| cx.windows().first().copied())
    else {
        return json!({"error": "no window"});
    };
    let workspace = found.map(|(_, workspace)| workspace);
    let done = |result: Result<()>| match result {
        Ok(()) => json!({"ok": true}),
        Err(e) => json!({"error": e.to_string()}),
    };
    match what {
        "state" => state(window, workspace.as_ref(), cx),
        "action" => {
            let name = command["name"].as_str().unwrap_or_default();
            let action = match cx.build_action(name, command.get("args").cloned()) {
                Ok(action) => action,
                Err(e) => return json!({"error": e.to_string()}),
            };
            done(window.update(cx, |_, window, cx| window.dispatch_action(action, cx)))
        }
        "activate" => {
            // a dialog's window by its title ("window"), the project window otherwise
            let window = titled(command, cx).unwrap_or(window);
            done(window.update(cx, |_, window, _| window.activate_window()))
        }
        "keys" => {
            let keys = command["keys"].as_str().unwrap_or_default();
            let os = cx.platform_info().os;
            let mapper = cx.keyboard_mapper().clone();
            let window = titled(command, cx).unwrap_or(window);
            done(window.update(cx, |_, window, cx| {
                for key in keys.split_whitespace() {
                    match Keystroke::parse_with_os(key, os) {
                        // a shortcut is pressed where the keymap's key equivalents put it on this
                        // keyboard layout: ⌘9 is ⌘ç on AZERTY, ⌃1 ⌃&
                        Ok(keystroke)
                            if keystroke.modifiers.platform || keystroke.modifiers.control =>
                        {
                            let keystroke = mapper.map_key_equivalent(keystroke, true);
                            window.dispatch_keystroke(keystroke.inner().clone(), cx);
                        }
                        Ok(keystroke) => {
                            window.dispatch_keystroke(keystroke, cx);
                        }
                        Err(e) => log::error!("e2e: {e}"),
                    }
                }
            }))
        }
        // types text into the focused field, a character a keystroke: paths and names with
        // characters `keys` would read as separators or modifiers
        "type" => {
            let text = command["text"].as_str().unwrap_or_default().to_owned();
            let window = titled(command, cx).unwrap_or(window);
            done(window.update(cx, |_, window, cx| {
                for typed in text.chars() {
                    let key = typed.to_lowercase().to_string();
                    let keystroke = Keystroke {
                        modifiers: gpui::Modifiers {
                            shift: typed.is_uppercase(),
                            ..gpui::Modifiers::default()
                        },
                        key,
                        key_char: Some(typed.to_string()),
                    };
                    window.dispatch_keystroke(keystroke, cx);
                }
            }))
        }
        "open" => {
            let path = command["path"].as_str().unwrap_or_default();
            let Some(workspace) = workspace else {
                return json!({"error": "no project window"});
            };
            let Some(root) = workspace
                .read(cx)
                .project()
                .read(cx)
                .visible_worktrees(cx)
                .next()
                .map(|worktree| worktree.read(cx).abs_path())
            else {
                return json!({"error": "no folder open"});
            };
            let path = root.join(path);
            done(window.update(cx, |_, window, cx| {
                workspace.update(cx, |workspace, cx| {
                    workspace
                        .open_abs_path(path, OpenOptions::default(), window, cx)
                        .detach_and_log_err(cx);
                })
            }))
        }
        "pick" => {
            // relative to the project's first folder, as `open`'s path
            let root = workspace.and_then(|workspace| {
                workspace
                    .read(cx)
                    .project()
                    .read(cx)
                    .visible_worktrees(cx)
                    .next()
                    .map(|worktree| worktree.read(cx).abs_path().to_path_buf())
            });
            let paths = command["paths"]
                .as_array()
                .map(|paths| {
                    paths
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|path| match &root {
                            Some(root) => root.join(path),
                            None => std::path::PathBuf::from(path),
                        })
                        .collect()
                })
                .unwrap_or_default();
            spork_workbench::pick_next(paths);
            json!({"ok": true})
        }
        "open_project" => {
            let path = std::path::PathBuf::from(command["path"].as_str().unwrap_or_default());
            let app_state = workspace::AppState::global(cx);
            // "new_window": true opens it in a window of its own, as IntelliJ opens a project
            let options = match command["new_window"].as_bool() {
                Some(true) => OpenOptions {
                    open_mode: workspace::OpenMode::NewWindow,
                    ..OpenOptions::default()
                },
                _ => OpenOptions::default(),
            };
            workspace::open_paths(&[path], app_state, options, cx).detach_and_log_err(cx);
            json!({"ok": true})
        }
        #[cfg(target_os = "macos")]
        "click" => {
            draw_windows(cx);
            // "console_text": the text where the Run tool window's console shows it
            // "line_number": that line's number in the active editor's gutter
            let line_number = command["line_number"]
                .as_u64()
                .and_then(|line| u32::try_from(line).ok());
            // "gutter_mark": that line's mark in the gutter's left free painters area (coverage)
            let gutter_mark = command["gutter_mark"]
                .as_u64()
                .and_then(|line| u32::try_from(line).ok());
            let found = match (command["console_text"].as_str(), line_number) {
                _ if let Some(line) = gutter_mark => workspace
                    .as_ref()
                    .and_then(|workspace| workspace.read(cx).active_item_as::<editor::Editor>(cx))
                    .and_then(|editor| {
                        editor.update(cx, |editor, cx| {
                            editor.free_painter_mark_point(line.saturating_sub(1), cx)
                        })
                    })
                    .map(|at| (None, (f32::from(at.x), f32::from(at.y))))
                    .ok_or_else(|| format!("the editor shows no mark on line {line}")),
                (_, Some(line)) => workspace
                    .as_ref()
                    .and_then(|workspace| workspace.read(cx).active_item_as::<editor::Editor>(cx))
                    .and_then(|editor| {
                        editor.update(cx, |editor, cx| {
                            editor.line_number_point(line.saturating_sub(1), cx)
                        })
                    })
                    .map(|at| (None, (f32::from(at.x), f32::from(at.y))))
                    .ok_or_else(|| format!("the editor shows no line {line}")),
                (Some(text), None) => workspace
                    .as_ref()
                    .and_then(|workspace| {
                        spork_run::run_panel::e2e_console_point(workspace.read(cx), text, cx)
                    })
                    .map(|at| (None, at))
                    .ok_or_else(|| format!("the console doesn't show {text}")),
                (None, None) => accessibility::locate(command),
            };
            let (size, at) = match found {
                Ok(found) => found,
                Err(e) => return json!({ "error": e }),
            };
            // the window the element is in, told by its size; the project window for a point
            let target = size
                .and_then(|(width, height)| {
                    cx.windows().into_iter().find(|handle| {
                        handle
                            .update(cx, |_, window, _| {
                                let size = window.viewport_size();
                                (f32::from(size.width) - width).abs() < 1.
                                    && (f32::from(size.height) - height).abs() < 1.
                            })
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(window);
            // "back" and "forward": a mouse's buttons 4 and 5
            let button = match command["button"].as_str() {
                Some("right") => MouseButton::Right,
                Some("back") => MouseButton::Navigate(gpui::NavigationDirection::Back),
                Some("forward") => MouseButton::Navigate(gpui::NavigationDirection::Forward),
                _ => MouseButton::Left,
            };
            // "count": 2 double-clicks; "modifiers": ["cmd", "shift", "alt", "ctrl"] held
            let click_count = command["count"]
                .as_u64()
                .and_then(|count| usize::try_from(count).ok())
                .unwrap_or(1);
            let held: Vec<&str> = command["modifiers"]
                .as_array()
                .map(|keys| keys.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            done(target.update(cx, |_, window, cx| {
                let position = point(px(at.0), px(at.1));
                let modifiers = Modifiers {
                    platform: held.contains(&"cmd"),
                    shift: held.contains(&"shift"),
                    alt: held.contains(&"alt"),
                    control: held.contains(&"ctrl"),
                    ..Modifiers::default()
                };
                // the move first: a context menu opens only over the element GPUI sees hovered
                let events = [
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position,
                        pressed_button: None,
                        modifiers,
                    }),
                    PlatformInput::MouseDown(MouseDownEvent {
                        button,
                        position,
                        modifiers,
                        click_count,
                        first_mouse: false,
                    }),
                    PlatformInput::MouseUp(MouseUpEvent {
                        button,
                        position,
                        modifiers,
                        click_count,
                    }),
                ];
                // "hover": true moves the pointer there only, as over a submenu's item
                let hover = command["hover"].as_bool() == Some(true);
                for event in events.into_iter().take(if hover { 1 } else { 3 }) {
                    window.dispatch_event(event, cx);
                }
            }))
        }
        "menubar" => {
            let path: Vec<&str> = command["path"]
                .as_array()
                .map(|path| path.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            match menu_bar(&path, window, cx) {
                Ok(menu) => {
                    MENU_BAR.set(Some(menu));
                    json!({"ok": true})
                }
                Err(e) => json!({ "error": e }),
            }
        }
        #[cfg(target_os = "macos")]
        "drag" => drag(command, window, cx),
        #[cfg(target_os = "macos")]
        "scroll" => scroll(command, window, cx),
        #[cfg(target_os = "macos")]
        "scrollbar" => match accessibility::locate(command) {
            Ok((_, (x, y))) => match gpui_base::painted_scrollbar_at(point(px(x), px(y))) {
                Some(painted) => json!({
                    "opacity": rounded(painted.opacity),
                    "offset": [rounded(f32::from(painted.offset.x)), rounded(f32::from(painted.offset.y))],
                    "thumbs": painted.thumbs.iter().map(|thumb| json!({
                        "axis": if thumb.vertical { "vertical" } else { "horizontal" },
                        "width": rounded(f32::from(thumb.width)),
                        "bounds": [rounded(f32::from(thumb.bounds.origin.x)), rounded(f32::from(thumb.bounds.origin.y)), rounded(f32::from(thumb.bounds.size.width)), rounded(f32::from(thumb.bounds.size.height))],
                    })).collect::<Vec<_>>(),
                }),
                None => json!({"error": format!("no scrollbar at {x}, {y}")}),
            },
            Err(e) => json!({ "error": e }),
        },
        "vcs_console" => {
            let commands: Vec<String> = spork_git::activity()
                .iter()
                .map(|command| format!("git {}", command.args))
                .collect();
            json!({"last": commands.last(), "commands": commands})
        }
        "notify" => match &workspace {
            Some(workspace) => {
                let text = |key: &str| command[key].as_str().map(str::to_owned);
                let actions = command["actions"]
                    .as_array()
                    .map(|actions| {
                        actions
                            .iter()
                            .filter_map(|action| action.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                let sticky = command["sticky"].as_bool().unwrap_or(false);
                workspace.update(cx, |workspace, cx| {
                    spork_tool_windows::e2e_notify(
                        workspace,
                        &text("type").unwrap_or_default(),
                        text("title"),
                        text("text").unwrap_or_default(),
                        actions,
                        sticky,
                        cx,
                    )
                });
                json!({"ok": true})
            }
            None => json!({"error": "no project window"}),
        },
        "push" => {
            use gpui_kit::component::WindowExt as _;
            use gpui_kit::component::notification::Notification;
            let text = command["text"].as_str().unwrap_or_default().to_owned();
            let note = match command["type"].as_str().unwrap_or_default() {
                "error" => Notification::error(text),
                "warning" => Notification::warning(text),
                _ => Notification::info(text),
            };
            let note = match command["title"].as_str() {
                Some(title) => note.title(title.to_owned()),
                None => note,
            };
            done(window.update(cx, |_, window, cx| window.push_notification(note, cx)))
        }
        "file_color" => match &workspace {
            Some(workspace) => {
                file_color(command["path"].as_str().unwrap_or_default(), workspace, cx)
            }
            None => json!({"error": "no project window"}),
        },
        "setting" => {
            let path = command["path"].as_str().unwrap_or_default();
            let merged =
                serde_json::to_value(cx.global::<settings::SettingsStore>().merged_settings())
                    .unwrap_or_default();
            let value =
                spork_settings::owned_setting(path, cx).or_else(|| merged.pointer(path).cloned());
            json!({ "value": value })
        }
        "modifiers" => {
            let held: Vec<&str> = command["held"]
                .as_array()
                .map(|keys| keys.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let modifiers = gpui::Modifiers {
                platform: held.contains(&"cmd"),
                shift: held.contains(&"shift"),
                alt: held.contains(&"alt"),
                control: held.contains(&"ctrl"),
                ..gpui::Modifiers::default()
            };
            done(window.update(cx, |_, window, cx| {
                window.dispatch_event(
                    gpui::PlatformInput::ModifiersChanged(gpui::ModifiersChangedEvent {
                        modifiers,
                        capslock: gpui::Capslock::default(),
                    }),
                    cx,
                );
            }))
        }
        other => json!({"error": format!("unknown command {other:?}")}),
    }
}

/// The colour of a file of the first folder for its version control status, from the function
/// the Project tool window and the editor tabs both draw it with.
fn file_color(path: &str, workspace: &Entity<Workspace>, cx: &mut App) -> Value {
    let project = workspace.read(cx).project().read(cx);
    let Some(root) = project.visible_worktrees(cx).next() else {
        return json!({"error": "no folder open"});
    };
    let Some(project_path) = project.find_project_path(root.read(cx).abs_path().join(path), cx)
    else {
        return json!({"error": format!("{path} isn't in the project")});
    };
    let Some(ignored) = project
        .entry_for_path(&project_path, cx)
        .map(|entry| entry.is_ignored)
    else {
        return json!({"error": format!("{path} isn't in the project")});
    };
    let status = project
        .git_store()
        .read(cx)
        .repository_and_path_for_project_path(&project_path, cx)
        .and_then(|(repository, repo_path)| repository.read(cx).status_for_path(&repo_path))
        .map(|entry| entry.status.summary())
        .unwrap_or_default();
    let color = editor::items::entry_git_aware_label_color(status, ignored, false);
    let hsla = color.color(cx);
    let rgba = gpui::Rgba::from(hsla);
    let byte = |channel: f32| (channel * 255.).round() as u8;
    json!({
        "name": format!("{color:?}"),
        "hex": format!("#{:02x}{:02x}{:02x}", byte(rgba.r), byte(rgba.g), byte(rgba.b)),
    })
}

/// What a test checks the IDE shows, besides the screen; without a project window, its windows
/// and focus only.
/// A command's `"window"`: the title of the window its keys or text go to (the project window's
/// otherwise).
fn titled(command: &Value, cx: &mut App) -> Option<AnyWindowHandle> {
    let title = command["window"].as_str()?;
    cx.windows().into_iter().find(|handle| {
        handle
            .update(cx, |_, window, _| window.window_title() == title)
            .unwrap_or(false)
    })
}

fn state(window: AnyWindowHandle, workspace: Option<&Entity<Workspace>>, cx: &mut App) -> Value {
    let focused: Vec<String> = window
        .update(cx, |_, window, _| {
            window
                .context_stack()
                .iter()
                .filter_map(|context| Some(context.primary()?.key.to_string()))
                .collect()
        })
        .log_err()
        .unwrap_or_default();
    let windows: Vec<Value> = cx
        .windows()
        .into_iter()
        .filter_map(|handle| {
            handle
                .update(cx, |_, window, _| {
                    let size = window.viewport_size();
                    json!({
                        "title": window.window_title(),
                        "active": window.is_window_active(),
                        "size": [f32::from(size.width), f32::from(size.height)],
                    })
                })
                .log_err()
        })
        .collect();
    let Some(workspace) = workspace else {
        return json!({
            "roots": [],
            "items": [],
            "focused": focused,
            "windows": windows,
            "look": look(cx),
            "settings": spork_settings::e2e_state(cx),
        });
    };
    let mut run = spork_run::e2e_state(workspace.read(cx), cx);
    let frames = spork_run::e2e_frames(workspace, cx);
    if let (Some(debug), Value::Object(frames)) =
        (run.get_mut("debug").and_then(Value::as_object_mut), frames)
    {
        debug.extend(frames);
    }
    let vcs = spork_vcs::e2e_state(workspace.read(cx), cx);
    let terminal = terminal_view::tool_window::e2e_state(workspace.read(cx), cx);
    let code = spork_code::e2e_state(workspace.read(cx), cx);
    let java = spork_java::e2e_state(workspace.read(cx), cx);
    let kotlin = spork_kotlin::e2e_state(workspace.read(cx), cx);
    let http = spork_http::e2e_state(workspace.read(cx), cx);
    let documentation = spork_editor::documentation_e2e_state(workspace.read(cx), cx);
    let notifications = spork_tool_windows::notifications_e2e_state(workspace.read(cx), cx);
    let coverage = spork_coverage::e2e_state(workspace.read(cx), cx);
    let editor = editor_state(workspace.read(cx), cx);
    let context_servers = context_servers_state(workspace.read(cx), cx);
    // each open dock's size, as the tool window's width (height at the bottom)
    let docks = workspace.read(cx).all_docks().map(|dock| dock.clone());
    let sizes: Vec<Option<f32>> = docks
        .iter()
        .map(|dock| {
            window
                .update(cx, |_, window, cx| {
                    dock.read(cx).stored_active_panel_size(window, cx)
                })
                .ok()
                .flatten()
                .map(f32::from)
        })
        .collect();
    let workspace = workspace.read(cx);
    // the editor area's split: its outermost axis, null unsplit
    let split = match &workspace.center().root {
        workspace::Member::Axis(axis) => Value::from(match axis.axis {
            gpui::Axis::Horizontal => "horizontal",
            gpui::Axis::Vertical => "vertical",
        }),
        workspace::Member::Pane(_) => Value::Null,
    };
    let roots: Vec<String> = workspace
        .project()
        .read(cx)
        .visible_worktrees(cx)
        .map(|worktree| worktree.read(cx).abs_path().to_string_lossy().into_owned())
        .collect();
    // a file by its path in its root, anything else (a file that is its own root, as a library's
    // source) by its tab's text
    let name = |item: &dyn ItemHandle| match item.project_path(cx) {
        Some(path) if !path.path.is_empty() => path.path.as_unix_str().to_owned(),
        _ => item.tab_content_text(0, cx).to_string(),
    };
    let items: Vec<String> = workspace
        .items(cx)
        .map(|item| name(item.as_ref()))
        .collect();
    let active = workspace.active_item(cx).map(|item| name(item.as_ref()));
    let dock = |dock: &Entity<Dock>| {
        let size = docks
            .iter()
            .position(|other| other == dock)
            .and_then(|ix| sizes.get(ix).copied().flatten());
        let dock = dock.read(cx);
        json!({
            "open": dock.is_open(),
            "panel": dock.visible_panel().map(|panel| panel.persistent_name()),
            "size": size,
        })
    };
    json!({
        "roots": roots,
        "items": items,
        "active": active,
        "docks": {
            "left": dock(workspace.left_dock()),
            "bottom": dock(workspace.bottom_dock()),
            "right": dock(workspace.right_dock()),
        },
        "split": split,
        "focused": focused,
        "windows": windows,
        "look": look(cx),
        "settings": spork_settings::e2e_state(cx),
        "run": run,
        "vcs": vcs,
        "terminal": terminal,
        "code": code,
        "java": java,
        "kotlin": kotlin,
        "http": http,
        "editor": editor,
        "documentation": documentation,
        "notifications": notifications,
        "coverage": coverage,
        "context_servers": context_servers,
    })
}

/// The active editor's `language`, its `diagnostics` (each `"severity: message"`, as the language
/// servers said them, top to bottom), the `completions` its menu lists (their labels), when it
/// shows one, and its `file`'s name and whether it is `read_only`.
fn editor_state(workspace: &Workspace, cx: &App) -> Value {
    let Some(editor) = workspace.active_item_as::<editor::Editor>(cx) else {
        return Value::Null;
    };
    let editor = editor.read(cx);
    let diagnostics: Vec<String> = editor
        .buffer()
        .read(cx)
        .as_singleton()
        .map(|buffer| {
            let snapshot = buffer.read(cx).snapshot();
            snapshot
                .diagnostics_in_range::<_, usize>(0..snapshot.len(), false)
                .map(|entry| {
                    let severity = match entry.diagnostic.severity {
                        lsp::DiagnosticSeverity::ERROR => "error",
                        lsp::DiagnosticSeverity::WARNING => "warning",
                        lsp::DiagnosticSeverity::INFORMATION => "info",
                        _ => "hint",
                    };
                    format!("{severity}: {}", entry.diagnostic.message)
                })
                .collect()
        })
        .unwrap_or_default();
    let completions: Option<Vec<String>> = match editor.context_menu().borrow().as_ref() {
        Some(editor::code_context_menus::CodeContextMenu::Completions(menu))
            if editor.has_visible_completions_menu() =>
        {
            Some(
                menu.completions
                    .borrow()
                    .iter()
                    .map(|completion| completion.label.text.clone())
                    .collect(),
            )
        }
        _ => None,
    };
    let language = editor.buffer().read(cx).as_singleton().and_then(|buffer| {
        buffer
            .read(cx)
            .language()
            .map(|language| language.name().to_string())
    });
    let file = editor.buffer().read(cx).as_singleton().and_then(|buffer| {
        let buffer = buffer.read(cx);
        let name = buffer.file()?.file_name(cx).to_owned();
        Some((name, buffer.read_only()))
    });
    // the caret as the status bar writes it: line:column
    let caret = {
        let snapshot = editor.buffer().read(cx).snapshot(cx);
        let head = editor::ToPoint::to_point(&editor.selections.newest_anchor().head(), &snapshot);
        format!("{}:{}", head.row + 1, head.column + 1)
    };
    // the lines showing the debugger's values, as shown: `44: code  name: "Ada"`
    let inline_values: Vec<String> = editor
        .buffer()
        .read(cx)
        .as_singleton()
        .map(|buffer| {
            let text = buffer.read(cx).text();
            editor
                .inline_value_lines()
                .iter()
                .map(|(row, values)| {
                    let code = text.lines().nth(*row as usize).unwrap_or_default();
                    format!("{}: {}{values}", row + 1, code.trim())
                })
                .collect()
        })
        .unwrap_or_default();
    json!({
        "language": language,
        "caret": caret,
        "inline_values": inline_values,
        "diagnostics": diagnostics,
        "completions": completions,
        "file": file.as_ref().map(|(name, _)| name),
        "read_only": file.map(|(_, read_only)| read_only),
    })
}

/// The project's context servers (MCP) by id, each its status: "starting", "running",
/// "stopped" or "error: …".
fn context_servers_state(workspace: &Workspace, cx: &App) -> Value {
    let store = workspace.project().read(cx).context_server_store();
    let store = store.read(cx);
    let servers: serde_json::Map<String, Value> = store
        .server_ids()
        .iter()
        .map(|id| {
            let status = match store.status_for_server(id) {
                Some(ContextServerStatus::Starting) => "starting".to_owned(),
                Some(ContextServerStatus::Running) => "running".to_owned(),
                Some(ContextServerStatus::Error(error)) => format!("error: {error}"),
                Some(_) | None => "stopped".to_owned(),
            };
            (id.0.to_string(), Value::from(status))
        })
        .collect();
    Value::Object(servers)
}

fn look(cx: &App) -> Value {
    let theme = cx.theme();
    let settings = ThemeSettings::get_global(cx);
    json!({
        "theme": theme.name,
        "appearance": if theme.appearance().is_light() { "light" } else { "dark" },
        "selection": settings.theme,
        "icon_theme": GlobalTheme::icon_theme(cx).name,
        "ui_font": settings.ui_font.family,
        "ui_font_size": rounded(f32::from(settings.ui_font_size(cx))),
        "buffer_font": settings.buffer_font.family,
        "buffer_font_size": rounded(f32::from(settings.buffer_font_size(cx))),
        "markdown_preview_font_size": rounded(f32::from(settings.markdown_preview_font_size(cx))),
        "line_height": rounded(settings.line_height()),
        "base_keymap": BaseKeymap::get_global(cx).to_string(),
    })
}

/// A size to two decimals, so a scenario can expect `14.86` of an f32's `14.857`.
fn rounded(value: f32) -> f64 {
    (f64::from(value) * 100.).round() / 100.
}

/// The menu bar menu at `path` (`["File"]`, `["File", "Open Recent"]`) as accessibility shows an
/// open one: AppKit draws the menu bar out of process, so it is read from the menus GPUI set, each
/// item enabled as AppKit would ask GPUI, by whether the focused element handles its action. An
/// empty path shows the menu bar itself, its menus as items.
fn menu_bar(path: &[&str], window: AnyWindowHandle, cx: &mut App) -> Result<Value, String> {
    let menus = cx.get_menus().ok_or("no menu bar")?;
    let Some((first, rest)) = path.split_first() else {
        let children: Vec<Value> = menus
            .iter()
            .map(
                |menu| json!({"role": "AXMenuItem", "title": menu.name, "enabled": !menu.disabled}),
            )
            .collect();
        return Ok(json!({
            "title": "menu bar",
            "tree": {"role": "AXMenu", "children": children},
        }));
    };
    let mut items = menus
        .into_iter()
        .find(|menu| menu.name == *first)
        .ok_or(format!("no {first} menu"))?
        .items;
    for name in rest {
        items = items
            .into_iter()
            .find_map(|item| match item {
                OwnedMenuItem::Submenu(menu) if menu.name == *name => Some(menu.items),
                _ => None,
            })
            .ok_or(format!("no {name} submenu"))?;
    }
    let children = window
        .update(cx, |_, window, cx| {
            items
                .iter()
                .filter_map(|item| match item {
                    OwnedMenuItem::Separator => Some(json!({"role": "AXMenuItem"})),
                    OwnedMenuItem::Submenu(menu) => Some(json!({
                        "role": "AXMenuItem",
                        "title": menu.name,
                        "enabled": !menu.disabled,
                        "children": [{"role": "AXMenu"}],
                    })),
                    OwnedMenuItem::Action {
                        name,
                        action,
                        disabled,
                        ..
                    } => Some(json!({
                        "role": "AXMenuItem",
                        "title": name,
                        "enabled": !disabled && window.is_action_available(action.as_ref(), cx),
                    })),
                    OwnedMenuItem::SystemMenu(_) => None,
                })
                .collect::<Vec<_>>()
        })
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "title": format!("menu bar: {}", path.join(" ▸ ")),
        "tree": {"role": "AXMenu", "children": children},
    }))
}

/// The IDE's windows read through macOS's accessibility from inside the process: asking from
/// outside would need the Accessibility permission.
#[cfg(target_os = "macos")]
mod accessibility {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, sel};
    use objc2_foundation::{NSArray, NSPoint, NSRect, NSString};
    use serde_json::{Map, Value, json};

    const MAX_DEPTH: usize = 64;

    /// An accessibility attribute of `object`, None when it doesn't answer that message.
    macro_rules! ask {
        ($object:expr, $selector:ident) => {{
            // SAFETY: an Objective-C object asked whether it answers a selector before it is sent;
            // NSAccessibility documents each selector asked and its return type
            unsafe {
                let answers: bool = msg_send![$object, respondsToSelector: sel!($selector)];
                if answers { Some(msg_send![$object, $selector]) } else { None }
            }
        }};
    }

    /// The IDE's windows and each one's content view, the root of its accessibility tree.
    fn windows() -> Vec<(Retained<AnyObject>, Retained<AnyObject>)> {
        // SAFETY: GPUI runs commands on the main thread, where AppKit may be messaged;
        // NSApplication, `windows` and `contentView` are documented with these types
        unsafe {
            let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
            let windows: Retained<NSArray<AnyObject>> = msg_send![&*app, windows];
            windows
                .iter()
                .filter_map(|window| {
                    let view: Option<Retained<AnyObject>> = msg_send![&*window, contentView];
                    Some((window, view?))
                })
                .collect()
        }
    }

    /// The window's content rectangle on screen, which element frames are measured against.
    fn content(window: &AnyObject) -> NSRect {
        // SAFETY: NSWindow's `frame` and `contentRectForFrameRect:` take and give an NSRect
        unsafe {
            let frame: NSRect = msg_send![window, frame];
            msg_send![window, contentRectForFrameRect: frame]
        }
    }

    fn text(value: Option<Option<Retained<NSString>>>) -> Option<String> {
        value
            .flatten()
            .map(|text| text.to_string())
            .filter(|text| !text.is_empty())
    }

    /// What an element shows as its value, as text.
    fn shown(object: &AnyObject) -> Option<String> {
        let value: Option<Option<Retained<AnyObject>>> = ask!(object, accessibilityValue);
        // SAFETY: every Objective-C object answers `description` with an NSString
        value
            .flatten()
            .map(|value| unsafe { msg_send![&*value, description] })
            .map(|text: Retained<NSString>| text.to_string())
    }

    fn element(object: &AnyObject, content: NSRect, depth: usize) -> Value {
        let mut node = Map::new();
        let texts = [
            ("role", text(ask!(object, accessibilityRole))),
            ("label", text(ask!(object, accessibilityLabel))),
            ("title", text(ask!(object, accessibilityTitle))),
            ("value", shown(object).filter(|text| !text.is_empty())),
        ];
        for (key, text) in texts {
            if let Some(text) = text {
                node.insert(key.into(), json!(text));
            }
        }
        if ask!(object, isAccessibilityEnabled) == Some(false) || !pressable(object) {
            node.insert("enabled".into(), json!(false));
        }
        if ask!(object, isAccessibilitySelected) == Some(true) {
            node.insert("selected".into(), json!(true));
        }
        let frame: Option<NSRect> = ask!(object, accessibilityFrame);
        if let Some(frame) = frame.filter(|f| f.size.width > 0.0 && f.size.height > 0.0) {
            // AppKit measures from the screen's bottom left, GPUI from the window's top left
            let top = content.origin.y + content.size.height - (frame.origin.y + frame.size.height);
            node.insert(
                "frame".into(),
                json!([
                    frame.origin.x - content.origin.x,
                    top,
                    frame.size.width,
                    frame.size.height
                ]),
            );
        }
        let children: Option<Option<Retained<NSArray<AnyObject>>>> =
            ask!(object, accessibilityChildren);
        if let Some(children) = children.flatten().filter(|_| depth < MAX_DEPTH) {
            let children: Vec<Value> = children
                .iter()
                .map(|child| element(&child, content, depth + 1))
                .collect();
            if !children.is_empty() {
                node.insert("children".into(), json!(children));
            }
        }
        Value::Object(node)
    }

    /// GPUI can't mark an element disabled for accessibility; a greyed menu item is one left
    /// without a press action.
    fn pressable(object: &AnyObject) -> bool {
        if text(ask!(object, accessibilityRole)).as_deref() != Some("AXMenuItem") {
            return true;
        }
        // SAFETY: NSAccessibility's `isAccessibilitySelectorAllowed:` takes a selector and gives a
        // BOOL; asked only of an object that answers it
        unsafe {
            let answers: bool =
                msg_send![object, respondsToSelector: sel!(isAccessibilitySelectorAllowed:)];
            !answers
                || msg_send![object, isAccessibilitySelectorAllowed: sel!(accessibilityPerformPress)]
        }
    }

    /// Every window's tree; `menu`, a menu bar menu shown with `{"do": "menubar"}`, comes last
    /// as the menu open last.
    pub(super) fn tree(menu: Option<Value>) -> Value {
        let mut windows: Vec<Value> = windows()
            .iter()
            .map(|(window, view)| {
                let title = text(ask!(&**window, title));
                json!({"title": title, "tree": element(view, content(window), 0)})
            })
            .collect();
        windows.extend(menu);
        json!({ "windows": windows })
    }

    /// Puts the IDE's windows in front of every other app's, in their order, without activating
    /// it: macOS draws a window again once it shows.
    pub(super) fn front() {
        // SAFETY: GPUI runs commands on the main thread, where AppKit may be messaged;
        // `orderedWindows` and `orderFrontRegardless` are documented with these types
        unsafe {
            let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
            let windows: Retained<NSArray<AnyObject>> = msg_send![&*app, orderedWindows];
            let back_to_front: Vec<_> = windows.iter().collect();
            for window in back_to_front.into_iter().rev() {
                let () = msg_send![&*window, orderFrontRegardless];
            }
        }
    }

    /// The center of the `nth` element `wanted` picks, depth first from `object`, counting only
    /// those under an element `within` picks (or `within` itself).
    fn find(
        object: &AnyObject,
        wanted: &dyn Fn(&AnyObject) -> bool,
        within: &dyn Fn(&AnyObject) -> bool,
        inside: bool,
        nth: &mut usize,
        depth: usize,
    ) -> Option<NSPoint> {
        let inside = inside || within(object);
        if inside && wanted(object) {
            if *nth == 0 {
                let frame: NSRect = ask!(object, accessibilityFrame)?;
                return Some(NSPoint::new(
                    frame.origin.x + frame.size.width / 2.0,
                    frame.origin.y + frame.size.height / 2.0,
                ));
            }
            *nth -= 1;
        }
        let children: Option<Option<Retained<NSArray<AnyObject>>>> =
            ask!(object, accessibilityChildren);
        let children = children.flatten().filter(|_| depth < MAX_DEPTH)?;
        children
            .iter()
            .find_map(|child| find(&child, wanted, within, inside, nth, depth + 1))
    }

    /// A window's width and height, a point's x and y, in points.
    type Size = (f32, f32);
    type Point = (f32, f32);

    /// Where to click for `command`: the window's size and the point in it from its top left. A
    /// bare `at` point has no window size: it is the project window's.
    pub(super) fn locate(command: &Value) -> Result<(Option<Size>, Point), String> {
        if let Some(at) = command["at"].as_array() {
            return match at.iter().filter_map(Value::as_f64).collect::<Vec<_>>()[..] {
                [x, y] => Ok((None, (x as f32, y as f32))),
                _ => Err("at is [x, y]".into()),
            };
        }
        let label = command["label"].as_str().unwrap_or_default();
        let role = command["role"].as_str();
        // a combo box is told by the choice it shows: {"value": "Ask"} instead of a label
        let value = command["value"].as_str();
        let named = |object: &AnyObject, label: &str| {
            [
                text(ask!(object, accessibilityLabel)),
                text(ask!(object, accessibilityTitle)),
            ]
            .iter()
            .flatten()
            .any(|text| text == label)
        };
        // "in": the label of an element to look under, when the label shows elsewhere too
        let scope = command["in"].as_str();
        let within = |object: &AnyObject| scope.is_none_or(|scope| named(object, scope));
        let wanted = |object: &AnyObject| {
            let named = match value {
                Some(value) => shown(object).as_deref() == Some(value),
                // no label: any element of the role, the `nth` of them
                None => label.is_empty() || named(object, label),
            };
            named
                && role.is_none_or(|role| {
                    text(ask!(object, accessibilityRole)).as_deref() == Some(role)
                })
        };
        let mut nth = command["nth"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0);
        for (window, view) in windows() {
            if let Some(center) = find(&view, &wanted, &within, false, &mut nth, 0) {
                let content = content(&window);
                let top = content.origin.y + content.size.height - center.y;
                return Ok((
                    Some((content.size.width as f32, content.size.height as f32)),
                    ((center.x - content.origin.x) as f32, top as f32),
                ));
            }
        }
        Err(format!(
            "nothing labelled {:?} on screen",
            value.unwrap_or(label)
        ))
    }
}

/// Draws every window now. A window updates its accessibility tree and what a click hits only
/// when it draws, which macOS stops while other windows cover it.
#[cfg(target_os = "macos")]
fn draw_windows(cx: &mut App) {
    for handle in cx.windows() {
        handle
            .update(cx, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            })
            .log_err();
    }
}

/// `{"do": "scroll", "label": "Project Panel", "by": [0, -40]}`: a scroll over the element of that
/// label (found as `click` finds it), of `by` points as a trackpad's or `lines` as a mouse wheel's.
#[cfg(target_os = "macos")]
fn scroll(command: &Value, window: AnyWindowHandle, cx: &mut App) -> Value {
    let at = match accessibility::locate(command) {
        Ok((_, at)) => at,
        Err(e) => return json!({ "error": e }),
    };
    let amount = |key: &str| -> Option<(f32, f32)> {
        match command[key]
            .as_array()?
            .iter()
            .filter_map(Value::as_f64)
            .collect::<Vec<_>>()[..]
        {
            [dx, dy] => Some((dx as f32, dy as f32)),
            _ => None,
        }
    };
    let delta = match (amount("by"), amount("lines")) {
        (Some((dx, dy)), _) => gpui::ScrollDelta::Pixels(point(px(dx), px(dy))),
        (None, Some((dx, dy))) => gpui::ScrollDelta::Lines(point(dx, dy)),
        (None, None) => return json!({"error": "by or lines is [dx, dy]"}),
    };
    let position = point(px(at.0), px(at.1));
    let result = window.update(cx, |_, window, cx| {
        // the move first: GPUI scrolls what it sees hovered
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers: Modifiers::default(),
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::Scroll(gpui::ScrollEvent {
                position,
                delta,
                modifiers: Modifiers::default(),
                touch_phase: gpui::TouchPhase::Moved,
                source: gpui::ScrollSource::default(),
            }),
            cx,
        );
    });
    match result {
        Ok(()) => json!({"ok": true}),
        Err(e) => json!({"error": e.to_string()}),
    }
}

/// `{"do": "drag", "label": "main.rs", "by": [40, 30]}`: presses the center of the element of
/// that label (found as `click` finds it), moves the pointer that many points and lets go, in the
/// project window: a tab or a splitter dragged.
#[cfg(target_os = "macos")]
fn drag(command: &Value, window: AnyWindowHandle, cx: &mut App) -> Value {
    let from = match accessibility::locate(command) {
        Ok((_, from)) => from,
        Err(e) => return json!({ "error": e }),
    };
    let by: Vec<f32> = command["by"]
        .as_array()
        .map(|by| {
            by.iter()
                .filter_map(Value::as_f64)
                .map(|value| value as f32)
                .collect()
        })
        .unwrap_or_default();
    let [dx, dy] = by[..] else {
        return json!({"error": "by is [dx, dy]"});
    };
    let at = |share: f32| point(px(from.0 + dx * share), px(from.1 + dy * share));
    let (button, modifiers) = (MouseButton::Left, Modifiers::default());
    let result = window.update(cx, |_, window, cx| {
        let mut events = vec![
            PlatformInput::MouseMove(MouseMoveEvent {
                position: at(0.),
                pressed_button: None,
                modifiers,
            }),
            PlatformInput::MouseDown(MouseDownEvent {
                button,
                position: at(0.),
                modifiers,
                click_count: 1,
                first_mouse: false,
            }),
        ];
        // in steps: GPUI starts a drag once the pointer has moved a little while pressed
        events.extend((1..=4).map(|step| {
            PlatformInput::MouseMove(MouseMoveEvent {
                position: at(step as f32 / 4.),
                pressed_button: Some(button),
                modifiers,
            })
        }));
        events.push(PlatformInput::MouseUp(MouseUpEvent {
            button,
            position: at(1.),
            modifiers,
            click_count: 1,
        }));
        for event in events {
            window.dispatch_event(event, cx);
        }
    });
    match result {
        Ok(()) => json!({"ok": true}),
        Err(e) => json!({"error": e.to_string()}),
    }
}
