//! The end-to-end tests' way in (tools/e2e): started with `SPORK_E2E=<socket path>`, Spork reads
//! one JSON command a line on that Unix socket and answers each with one JSON line. Without the
//! variable none of this runs.
//!
//! Commands: `{"do": "action", "name": "spork::QuickLaunch"}` runs an action as its shortcut
//! would; `{"do": "keys", "keys": "cmd-shift-p"}` types keys; `{"do": "open", "what":
//! "branches" | "projects" | "settings", "page": "Editor/Font"}` opens a toolbar popup or a
//! Settings page; `{"do": "state"}` tells what shows; `{"do": "quit"}`.
//!
//! What shows, as macOS's accessibility sees it: `{"do": "ui"}` answers every window's tree of
//! elements (role, label, title, value, enabled, selected, frame in the window's points from its
//! top left); `{"do": "click", "label": "Fetch", "role": "AXMenuItem", "nth": 0, "button":
//! "right"}` clicks the center of the first element whose label or title is that text, and
//! `{"do": "click", "at": [x, y]}` that point of the largest window; without a label, the `nth`
//! element of the role (an unlabelled text field); `"value": "Ask"` in place of the label finds a
//! combo box by the choice it shows; `"in": "Branches Tree"` looks only under the element of that
//! label; `"hover": true` only moves the pointer there; `"count": 2` double-clicks and
//! `"modifiers": ["cmd", "shift"]` holds those keys down.
//! GPUI builds the tree once something asks for it, so the first `ui` may answer only the windows.
//! `{"do": "front"}` puts Spork's windows in front of other apps' before a screenshot, and
//! `{"do": "framed"}` tells whether each has drawn a frame since.
//! `{"do": "menubar", "path": ["Git"]}` shows a menu bar menu (or `["Git", "Patch"]`, its
//! submenu) to `ui` as the menu open last, until the next command that acts.
//!
//! `{"do": "pick", "paths": ["../fix.patch"]}` answers the next file picker that supports it
//! (Import Patches…, Change Shelves Location's Browse…) with those files, relative to the
//! repository, without opening it.
//!
//! `{"do": "clipboard", "text": "..."}` puts text on the clipboard and `{"do": "clipboard"}`
//! answers `{"text": ...}`: while the tests run, Spork's patch actions use a clipboard of their
//! own, so the user's isn't overwritten.
//!
//! `{"do": "git", "args": [...]}` runs git in the project's repository and reloads it;
//! `{"do": "open", "what": "file-history" | "annotate", "path": "README.md"}` opens a file's
//! history or annotations, `"what": "change"` a local change's diff; `{"do": "history"}` tells
//! what they show (file_view.rs).
//!
//! `{"do": "pulls", "pulls": [...]}` lists those pull / merge requests (each as the service's API
//! answers it) in place of the service's (`"listed": false`: once the list loads again); the
//! state's `opened` is the page Spork would have
//! opened in the browser last (hosting.rs).
//! `{"do": "drag", "label": "Resize", "by": [40, 30]}` drags an element (found as `click` finds
//! it) by that many points.
//! `{"do": "notify", "type": "info" | "warning" | "error", "title": "…", "text": "…", "actions":
//! ["Fetch", …], "sticky": true}` shows a balloon in the project window as a notification would (an
//! action logs "Ran Fetch" in the tool window when chosen); `{"do": "push", "type": …, "text": "…"}` pushes a
//! notification as Spork's views do, through the Notifications settings; `{"do": "balloons"}`
//! answers the balloons shown (`balloons`, oldest first, each with its `title`, `text`, `type`,
//! `sticky` and `actions`), the stub's count (`more`), the stripe's unread dot (`badge`,
//! "error", "info" or null) and how many notifications the tool window keeps (`logged`).
//! `{"do": "scroll", "label": "Branches Tree", "by": [0, -40]}` scrolls over an element (found as
//! `click` finds it, or `"at": [x, y]`) by that many points as a trackpad does, or `"lines": [0,
//! -3]` as a mouse wheel does; a negative `y` shows what's further down. `{"do": "scrollbar",
//! "label": …}` (or `"at"`) answers the scrollbar drawn over that element as it last painted: its
//! `opacity` (0 hidden, 1 shown), the content's `offset` and its `thumbs`, each with its `axis`,
//! its `width` and its `bounds`. The IDE's test channel answers both alike.

use std::io::{BufRead as _, BufReader, Write as _};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

use gpui_kit::component::Root;
use gpui_kit::*;
use serde_json::{Value, json};

use crate::util::ResultExt as _;
use crate::workspace::Workspace;

type Command = (Value, smol::channel::Sender<Value>);

// shortcut: the test channel is a Unix socket, so the end-to-end tests run on macOS and Linux only
#[cfg(not(unix))]
pub(crate) fn start(_: &mut App) {}

#[cfg(unix)]
pub(crate) fn start(cx: &mut App) {
    let Some(path) = std::env::var_os("SPORK_E2E") else {
        return;
    };
    // for `scrollbar`
    gpui_kit::base::record_painted_scrollbars(true);
    // a socket left by an earlier run
    if std::path::Path::new(&path).exists() {
        std::fs::remove_file(&path).log_err();
    }
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(e) => return eprintln!("e2e: can't listen on {}: {e}", path.to_string_lossy()),
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

#[cfg(unix)]
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
                smol::block_on(answer.recv()).unwrap_or_else(|_| json!({"error": "Spork quit"}))
            }
            Err(e) => json!({"error": format!("not JSON: {e}")}),
        };
        if writeln!(writer, "{answer}").is_err() {
            break;
        }
    }
}

/// The first project window and its workspace.
fn workspace(cx: &mut App) -> Option<(AnyWindowHandle, Entity<Workspace>)> {
    cx.windows().into_iter().find_map(|handle| {
        let workspace = handle
            .update(cx, |root, _, cx| {
                let root = root.downcast::<Root>().ok()?;
                root.read(cx).view().clone().downcast::<Workspace>().ok()
            })
            .ok()??;
        Some((handle, workspace))
    })
}

thread_local! {
    /// The windows yet to draw a frame since `{"do": "front"}`.
    static UNFRAMED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// The menu bar menu `{"do": "menubar"}` shows, until the next command that acts.
    static MENU_BAR: std::cell::RefCell<Option<Value>> = const { std::cell::RefCell::new(None) };
    /// The files `{"do": "pick", "paths": [...]}` (relative to the repository) gave the next
    /// file picker that asks [`take_picked`], which then doesn't open: a test can't drive the
    /// native one.
    static PICKED: std::cell::RefCell<Option<Vec<std::path::PathBuf>>> =
        const { std::cell::RefCell::new(None) };
}

/// The files a test picked in place of the user, once.
pub(crate) fn take_picked() -> Option<Vec<std::path::PathBuf>> {
    PICKED.take()
}

thread_local! {
    /// The clipboard while the tests run (`{"do": "clipboard"}`).
    static CLIPBOARD: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Whether the tests drive this Spork.
pub(crate) fn active() -> bool {
    static ACTIVE: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| std::env::var_os("SPORK_E2E").is_some());
    *ACTIVE
}

/// The clipboard's text: the tests' own clipboard while they run.
pub(crate) fn clipboard_text(cx: &App) -> Option<String> {
    if active() {
        return CLIPBOARD.with_borrow(Clone::clone);
    }
    cx.read_from_clipboard().and_then(|item| item.text())
}

/// Puts `text` on the clipboard: the tests' own while they run.
pub(crate) fn write_clipboard(text: String, cx: &mut App) {
    if active() {
        return CLIPBOARD.set(Some(text));
    }
    cx.write_to_clipboard(ClipboardItem::new_string(text));
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
    let Some((window, workspace)) = workspace(cx) else {
        return json!({"error": "no project window"});
    };
    let tab = workspace.read(cx).active_tab();
    let done = |result: Result<()>| match result {
        Ok(()) => json!({"ok": true}),
        Err(e) => json!({"error": e.to_string()}),
    };
    match what {
        "action" => {
            let name = command["name"].as_str().unwrap_or_default();
            let action = match cx.build_action(name, None) {
                Ok(action) => action,
                Err(e) => return json!({"error": e.to_string()}),
            };
            done(window.update(cx, |_, window, cx| {
                // the project view's actions are reached from the focus, which a window in the
                // background may not have
                let focus = tab.read(cx).focus.clone();
                if !focus.contains_focused(window, cx) {
                    focus.focus(window, cx);
                }
                window.dispatch_action(action, cx);
            }))
        }
        "keys" => {
            let keys = command["keys"].as_str().unwrap_or_default();
            done(window.update(cx, |_, window, cx| {
                for key in keys.split_whitespace() {
                    // shortcut: GPUI parses only 'static text, so each typed key leaks its few
                    // bytes; fine for a test run, use an owned parser if GPUI gets one
                    match Keystroke::parse(Box::leak(key.to_owned().into_boxed_str())) {
                        Ok(keystroke) => {
                            window.dispatch_keystroke(keystroke, cx);
                        }
                        Err(e) => eprintln!("e2e: {e}"),
                    }
                }
            }))
        }
        "open" => {
            let page = command["page"].as_str().map(str::to_owned);
            let popup = command["what"].as_str().unwrap_or_default().to_owned();
            let path = command["path"].as_str().unwrap_or_default().to_owned();
            done(window.update(cx, |_, window, cx| {
                tab.update(cx, |this, cx| match popup.as_str() {
                    "branches" => this.toggle_branches_popup(window, cx),
                    "projects" => this.toggle_project_popup(window, cx),
                    "settings" => this.settings_page(page, window, cx),
                    // a file's history or its annotations, as Git ▸ Show History / Annotate open them
                    "file-history" => this.open_file(path, false, cx),
                    "annotate" => this.open_file(path, true, cx),
                    // a local change's diff, as a click on it in the Commit tool window
                    "change" => {
                        let selection = crate::changes::ChangeSelection {
                            path,
                            staged: false,
                        };
                        this.select_change(Some(selection), cx);
                        this.view = crate::View::Changes;
                    }
                    _ => {}
                })
            }))
        }
        // what the file history, annotations and diff viewer show (file_view.rs)
        "history" => crate::file_view::e2e_state(&tab, cx),
        "state" => {
            let settings = crate::settings_window::shown_page(cx);
            let this = tab.read(cx);
            let commits = match &this.repo {
                Some(Ok(repo)) => repo.history.commits.len(),
                _ => 0,
            };
            json!({
                "repo": this.tab_root(),
                "view": format!("{:?}", this.view),
                "left": format!("{:?}", this.left_tools),
                "active_tool": format!("{:?}", this.active_tool),
                "console": this.show_console,
                "terminal": this.show_terminal,
                "notifications": this.show_notifications,
                "branches_popup": this.branches_popup_open,
                "project_popup": this.project_popup_open,
                "search_everywhere": this.search_everywhere.is_some(),
                "selected": this.selected,
                "commits": commits,
                "settings": settings,
                "appearance": format!("{:?}", this.prefs.appearance),
                "log": this.log_state(),
                "opened": crate::hosting::opened_url(),
            })
        }
        #[cfg(target_os = "macos")]
        "click" => {
            draw_windows(cx);
            let (size, at) = match accessibility::locate(command) {
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
            let button = if command["button"].as_str() == Some("right") {
                MouseButton::Right
            } else {
                MouseButton::Left
            };
            // "count": 2 double-clicks; "modifiers": ["cmd", "shift", "alt", "ctrl"] held
            let click_count = command["count"].as_u64().map_or(1, |n| n as usize);
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
            crate::shell::set_git_menu(tab.read(cx).git_menu_state(), cx);
            let path: Vec<&str> = command["path"]
                .as_array()
                .map(|path| path.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            match menu_bar(&path, window, &tab, cx) {
                Ok(menu) => {
                    MENU_BAR.set(Some(menu));
                    json!({"ok": true})
                }
                Err(e) => json!({ "error": e }),
            }
        }
        "git" => {
            let args: Vec<&str> = command["args"]
                .as_array()
                .map(|args| args.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let Some(root) = tab.read(cx).tab_root() else {
                return json!({"error": "no repository open"});
            };
            #[allow(
                clippy::disallowed_methods,
                reason = "a test step waits for its git command before it answers"
            )]
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(&args)
                .output();
            match output {
                Ok(out) if out.status.success() => {
                    tab.update(cx, |this, cx| this.reload(cx));
                    json!({"ok": true})
                }
                Ok(out) => json!({"error": String::from_utf8_lossy(&out.stderr).trim()}),
                Err(e) => json!({"error": e.to_string()}),
            }
        }
        // pull / merge requests as the hosting service's API lists them, in place of its own
        "pulls" => match tab.update(cx, |this, cx| {
            let listed = command["listed"].as_bool() != Some(false);
            this.e2e_pulls(&command["pulls"], listed, cx)
        }) {
            Ok(()) => json!({"ok": true}),
            Err(e) => json!({ "error": e }),
        },
        #[cfg(target_os = "macos")]
        "drag" => drag(command, window, cx),
        "notify" => {
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
            done(window.update(cx, |_, window, cx| {
                crate::notices::e2e_notify(
                    &text("type").unwrap_or_default(),
                    text("title"),
                    text("text").unwrap_or_default(),
                    actions,
                    sticky,
                    window,
                    cx,
                )
            }))
        }
        "push" => {
            let text = command["text"].as_str().unwrap_or_default().to_owned();
            let note = match command["type"].as_str().unwrap_or_default() {
                "error" => crate::notices::Notification::error(text),
                "warning" => crate::notices::Notification::warning(text),
                "success" => crate::notices::Notification::success(text),
                _ => crate::notices::Notification::info(text),
            };
            done(window.update(cx, |_, window, cx| {
                use gpui_kit::component::WindowExt as _;
                window.push_notification(note, cx)
            }))
        }
        "balloons" => window
            .update(cx, |_, window, cx| crate::notices::e2e_state(window, cx))
            .unwrap_or_else(|e| json!({"error": e.to_string()})),
        "scroll" => scroll(command, window, cx),
        "scrollbar" => match accessibility::locate(command) {
            Ok((_, (x, y))) => match gpui_kit::base::painted_scrollbar_at(point(px(x), px(y))) {
                Some(painted) => {
                    let round = |value: f32| (f64::from(value) * 100.).round() / 100.;
                    json!({
                        "opacity": round(painted.opacity),
                        "offset": [round(f32::from(painted.offset.x)), round(f32::from(painted.offset.y))],
                        "thumbs": painted.thumbs.iter().map(|thumb| json!({
                            "axis": if thumb.vertical { "vertical" } else { "horizontal" },
                            "width": round(f32::from(thumb.width)),
                        "bounds": [round(f32::from(thumb.bounds.origin.x)), round(f32::from(thumb.bounds.origin.y)), round(f32::from(thumb.bounds.size.width)), round(f32::from(thumb.bounds.size.height))],
                        })).collect::<Vec<_>>(),
                    })
                }
                None => json!({"error": format!("no scrollbar at {x}, {y}")}),
            },
            Err(e) => json!({ "error": e }),
        },
        "pick" => {
            let Some(root) = tab.read(cx).tab_root() else {
                return json!({"error": "no repository open"});
            };
            let paths = command["paths"].as_array().map(|paths| {
                paths
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|p| root.join(p))
                    .collect()
            });
            PICKED.set(paths);
            json!({"ok": true})
        }
        "clipboard" => {
            if let Some(text) = command["text"].as_str() {
                CLIPBOARD.set(Some(text.to_owned()));
                // what the clipboard enables shows once drawn again
                cx.refresh_windows();
            }
            json!({"text": CLIPBOARD.with_borrow(Clone::clone)})
        }
        other => json!({"error": format!("unknown command {other:?}")}),
    }
}

/// The menu bar menu at `path` (`["Git"]`, `["Git", "Patch"]`) as accessibility shows an open
/// one: AppKit draws the menu bar out of process, so it is read from the menus GPUI set, each
/// item enabled as AppKit would ask GPUI, by whether the focused view handles its action.
fn menu_bar(
    path: &[&str],
    window: AnyWindowHandle,
    tab: &Entity<crate::RepoView>,
    cx: &mut App,
) -> std::result::Result<Value, String> {
    let menus = cx.get_menus().ok_or("no menu bar")?;
    let (first, rest) = path.split_first().ok_or("path names the menu, [\"Git\"]")?;
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
    let focus = tab.read(cx).focus.clone();
    let children = window
        .update(cx, |_, window, cx| {
            if !focus.contains_focused(window, cx) {
                focus.focus(window, cx);
            }
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

/// Spork's windows read through macOS's accessibility from inside the process: asking from
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

    /// Spork's windows and each one's content view, the root of its accessibility tree.
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

    fn element(object: &AnyObject, content: NSRect, depth: usize) -> Value {
        let mut node = Map::new();
        let value: Option<Option<Retained<AnyObject>>> = ask!(object, accessibilityValue);
        // SAFETY: every Objective-C object answers `description` with an NSString
        let value = value
            .flatten()
            .map(|value| unsafe { msg_send![&*value, description] })
            .map(|text: Retained<NSString>| text.to_string());
        let texts = [
            ("role", text(ask!(object, accessibilityRole))),
            ("label", text(ask!(object, accessibilityLabel))),
            ("title", text(ask!(object, accessibilityTitle))),
            ("value", value.filter(|text| !text.is_empty())),
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

    /// GPUI can't mark an element disabled for accessibility; a greyed menu item is one gpui-kit
    /// leaves without a press action. Buttons say it themselves (spork_workbench's
    /// `A11yDisabled`): one opening a dropdown takes no press either.
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

    /// Puts Spork's windows in front of every other app's, in their order, without activating
    /// Spork: macOS draws a window again once it shows.
    pub(super) fn front() {
        // SAFETY: GPUI runs commands on the main thread, where AppKit may be messaged;
        // `orderedWindows` and `orderFrontRegardless` are documented with these types
        unsafe {
            let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
            let windows: Retained<NSArray<AnyObject>> = msg_send![&*app, orderedWindows];
            let back_to_front: Vec<_> = windows.iter().collect();
            for window in back_to_front.into_iter().rev() {
                let _: () = msg_send![&*window, orderFrontRegardless];
            }
        }
    }

    /// The `nth` element matching `wanted` under `object`, depth first: its center on screen.
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
            let shown: Option<Option<Retained<AnyObject>>> = ask!(object, accessibilityValue);
            // SAFETY: every Objective-C object answers `description` with an NSString
            let shown = shown
                .flatten()
                .map(|shown| unsafe { msg_send![&*shown, description] })
                .map(|text: Retained<NSString>| text.to_string());
            let named = match value {
                Some(value) => shown.as_deref() == Some(value),
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
/// when it draws, which macOS stops while other windows cover it; and one that shows nothing new
/// (Settings) would keep answering its first, empty tree.
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

/// `{"do": "scroll", "label": "Branches Tree", "by": [0, -40]}`: a scroll over the element of that
/// label (found as `click` finds it), of `by` points as a trackpad's or `lines` as a mouse wheel's.
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
        (Some((dx, dy)), _) => ScrollDelta::Pixels(point(px(dx), px(dy))),
        (None, Some((dx, dy))) => ScrollDelta::Lines(point(dx, dy)),
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
            PlatformInput::Scroll(ScrollEvent {
                position,
                delta,
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
                source: ScrollSource::default(),
            }),
            cx,
        );
    });
    match result {
        Ok(()) => json!({"ok": true}),
        Err(e) => json!({"error": e.to_string()}),
    }
}

/// `{"do": "drag", "label": "Resize", "by": [40, 30]}`: presses the center of the element of that
/// label (found as `click` finds it), moves the pointer that many points and lets go, in the
/// project window: a popup's corner or a splitter dragged.
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
                .map(|v| v as f32)
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
