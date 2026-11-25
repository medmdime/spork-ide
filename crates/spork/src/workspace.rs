//! A window's tabs: one repository per tab, the tab strip in the title bar, and the session
//! (the open tabs) restored on the next launch (docs/specs/M01 §2).
// shortcut: one session for the app; with several windows, the one changed last is what reopens.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{
    AppMenuBar, ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem,
};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::shell::{
    CloseTab, CustomizeToolbar, NewTab, NextTab, OpenInEditor, OpenTerminal, Preferences,
    PreviousTab, QuickLaunch, ReopenClosedTab, RunAnything, ShowBackupAndSync, ShowPlugins,
    ThemeDark, ThemeLight, ThemeSync, ToggleFullScreen,
};
use crate::util::ResultExt as _;
use crate::{RepoView, askpass, repos};

pub(crate) struct Workspace {
    tabs: Vec<Entity<RepoView>>,
    active: usize,
    /// Windows / Linux menu bar in the title bar (macOS uses the native one).
    menu_bar: Entity<AppMenuBar>,
    /// Repositories of closed tabs, most recent last, for Reopen Closed Tab (M01 §2).
    closed: Vec<PathBuf>,
    /// The session as last written, so a tab's every update (a scroll) doesn't write it again.
    saved: String,
    _observers: Vec<Subscription>,
}

fn session_file() -> Option<PathBuf> {
    Some(repos::config_dir()?.join("session.txt"))
}

/// The repositories that were open in tabs last time, those that still exist.
pub(crate) fn last_session() -> Vec<PathBuf> {
    session_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|s| {
            s.lines()
                .map(PathBuf::from)
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default()
}

/// Where a new tab starts: the home folder isn't a repository, so it shows the welcome screen.
fn welcome_path() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Project windows open now: closing the last one brings back the Welcome window.
static PROJECT_WINDOWS: AtomicUsize = AtomicUsize::new(0);

/// A project window is closing: when it is the last, the Welcome window shows, as in the reference app.
fn last_window_closing(cx: &mut App) {
    if PROJECT_WINDOWS.load(Ordering::SeqCst) == 1 {
        cx.defer(|cx| crate::welcome::open(None, cx));
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        PROJECT_WINDOWS.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Workspace {
    /// The tab showing now.
    pub(crate) fn active_tab(&self) -> Entity<RepoView> {
        self.tabs[self.active].clone()
    }

    pub(crate) fn new(
        paths: Vec<PathBuf>,
        prompts: Option<smol::channel::Receiver<askpass::Prompt>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        PROJECT_WINDOWS.fetch_add(1, Ordering::SeqCst);
        window.on_window_should_close(cx, |_, cx| {
            last_window_closing(cx);
            true
        });
        let mut this = Self {
            tabs: Vec::new(),
            active: 0,
            menu_bar: AppMenuBar::new(cx),
            closed: Vec::new(),
            saved: String::new(),
            _observers: Vec::new(),
        };
        // only the first tab answers git's password prompts
        let mut prompts = prompts;
        for path in paths {
            this.add_tab(path, prompts.take(), window, cx);
        }
        if this.tabs.is_empty() {
            this.add_tab(welcome_path(), prompts.take(), window, cx);
        }
        this.activate(0, window, cx);
        this
    }

    fn add_tab(
        &mut self,
        path: PathBuf,
        prompts: Option<smol::channel::Receiver<askpass::Prompt>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = cx.new(|cx| RepoView::new(path, prompts, window, cx));
        crate::project_popup::register(window.window_handle(), tab.downgrade(), cx);
        // a tab's repository can change (switcher, clone): refresh titles and the session
        self._observers.push(cx.observe(&tab, |this, _, cx| {
            this.save_session(cx);
            cx.notify();
        }));
        self.tabs.push(tab);
    }

    fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(ix) else { return };
        self.active = ix;
        // menus and shortcuts reach the focused tab
        window.focus(&tab.read(cx).focus.clone(), cx);
        cx.notify();
    }

    fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.add_tab(welcome_path(), None, window, cx);
        self.activate(self.tabs.len() - 1, window, cx);
        self.save_session(cx);
    }

    fn close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() == 1 {
            last_window_closing(cx);
            return window.remove_window();
        }
        let tab = self.tabs.remove(ix);
        if let Some(Ok(repo)) = &tab.read(cx).repo {
            self.closed.push(repo.root.clone());
        }
        drop(self._observers.remove(ix)); // stops watching the closed tab
        let next = if self.active >= ix {
            self.active.saturating_sub(1)
        } else {
            self.active
        };
        self.activate(next.min(self.tabs.len() - 1), window, cx);
        self.save_session(cx);
    }

    /// Closes every tab but `keep`; they can be reopened one by one.
    fn close_others(&mut self, keep: usize, window: &mut Window, cx: &mut Context<Self>) {
        for ix in (0..self.tabs.len()).rev().filter(|ix| *ix != keep) {
            let tab = self.tabs.remove(ix);
            if let Some(Ok(repo)) = &tab.read(cx).repo {
                self.closed.push(repo.root.clone());
            }
            drop(self._observers.remove(ix));
        }
        self.activate(0, window, cx);
        self.save_session(cx);
    }

    fn reopen_closed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.closed.pop() else {
            return;
        };
        self.add_tab(path, None, window, cx);
        self.activate(self.tabs.len() - 1, window, cx);
        self.save_session(cx);
    }

    fn step(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.tabs.len() as isize;
        let ix = (self.active as isize + delta).rem_euclid(n) as usize;
        self.activate(ix, window, cx);
    }

    // shortcut: a failed save only loses the reopened tabs; it isn't reported.
    fn save_session(&mut self, cx: &App) {
        let paths: Vec<String> = self
            .tabs
            .iter()
            .filter_map(|t| match &t.read(cx).repo {
                Some(Ok(repo)) => Some(repo.root.display().to_string()),
                _ => None,
            })
            .collect();
        let session = paths.join("\n");
        if session == self.saved {
            return;
        }
        if let Some(file) = session_file() {
            std::fs::create_dir_all(file.parent().unwrap_or(&file)).log_err();
            std::fs::write(file, &session).log_err();
        }
        self.saved = session;
    }
}

/// `SPORK_FPS=1`: in the title bar, how many frames the window drew in the last second, and how
/// long the slowest one kept the CPU busy (building, laying out and painting it). Frames are
/// only drawn when something changes, so it reads while scrolling; idle, the last reading stays.
/// The CPU time is the honest number: a window behind others gets fewer frames from macOS.
struct FrameMeter {
    frames: u32,
    since: std::time::Instant,
    last: std::time::Instant,
    /// When the frame being drawn started, and the slowest frame's CPU time this second.
    started: std::time::Instant,
    slowest: std::time::Duration,
    shown: Option<(u32, std::time::Duration)>,
}

impl FrameMeter {
    fn start(&mut self) {
        let now = std::time::Instant::now();
        let gap = now - self.last;
        (self.last, self.started) = (now, now);
        // a pause (nothing to draw) starts a new count rather than dragging the rate down
        if gap > std::time::Duration::from_millis(250) {
            (self.frames, self.since, self.slowest) = (0, now, Default::default());
            return;
        }
        self.frames += 1;
        if now - self.since >= std::time::Duration::from_secs(1) {
            self.shown = Some((self.frames, self.slowest));
            (self.frames, self.since, self.slowest) = (0, now, Default::default());
        }
    }

    /// Called when the window's last element paints: the frame's work is done.
    fn painted(&mut self) {
        self.slowest = self.slowest.max(self.started.elapsed());
    }
}

thread_local! {
    static METER: std::cell::RefCell<Option<FrameMeter>> = std::cell::RefCell::new(
        std::env::var_os("SPORK_FPS").map(|_| {
            let now = std::time::Instant::now();
            FrameMeter {
                frames: 0,
                since: now,
                last: now,
                started: now,
                slowest: Default::default(),
                shown: None,
            }
        }),
    );
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // the active tab's project and branch widgets and its git actions, built in its context
        let tab = self.tabs[self.active].clone();
        // the project's color glows behind the toolbar; the toolbar fades in a background window
        let window_active = window.is_window_active();
        let glow = tab.read(cx).tab_root().map(|root| {
            let slot = spork_workbench::shell::project_slot(&root);
            spork_workbench::shell::project_glow(
                slot,
                px(100.),
                window_active,
                cx.theme().mode.is_dark(),
                spork_workbench::Tokens::get(cx).frame,
                cx,
            )
        });
        let toolbar = tab.update(cx, |view, cx| view.main_toolbar(cx));
        let fps = METER.with_borrow_mut(|m| {
            m.as_mut().map(|m| {
                m.start();
                match m.shown {
                    Some((frames, slowest)) => format!(
                        "{frames} fps · slowest frame {:.1} ms CPU",
                        slowest.as_secs_f32() * 1000.
                    ),
                    None => "measuring…".to_owned(),
                }
            })
        });
        let metering = fps.is_some();
        let theme = cx.theme();
        let (active_bg, hover, muted, dirty_dot) = (
            theme.background,
            theme.list_hover,
            theme.muted_foreground,
            theme.warning,
        );
        let titles: Vec<(String, bool, Option<PathBuf>)> = self
            .tabs
            .iter()
            .map(|t| {
                let t = t.read(cx);
                (t.tab_title(), t.has_changes(), t.tab_root())
            })
            .collect();
        let workspace = cx.entity();
        let strip = h_flex()
            .h_full()
            .gap_1()
            .children(
                titles
                    .into_iter()
                    .enumerate()
                    .map(|(ix, (title, dirty, root))| {
                        let active = ix == self.active;
                        let menu_target = workspace.clone();
                        h_flex()
                            .id(("tab", ix))
                            .h(px(28.))
                            .pl_3()
                            .pr_1()
                            .gap_1()
                            .rounded(theme.radius)
                            .text_sm()
                            .when(active, |t| t.bg(active_bg))
                            .when(!active, |t| t.text_color(muted).hover(move |t| t.bg(hover)))
                            .on_click(
                                cx.listener(move |this, _, window, cx| {
                                    this.activate(ix, window, cx)
                                }),
                            )
                            // a dot while the repository has uncommitted changes
                            .when(dirty, |t| {
                                t.child(div().size(px(6.)).flex_none().rounded_full().bg(dirty_dot))
                            })
                            .child(div().max_w(px(180.)).truncate().child(title))
                            .context_menu(move |menu, _, _| {
                                let tab = |label: &str,
                                           f: fn(
                                    &mut Workspace,
                                    usize,
                                    &mut Window,
                                    &mut Context<Workspace>,
                                )| {
                                    let w = menu_target.clone();
                                    PopupMenuItem::new(label.to_owned()).on_click(
                                        move |_, window, cx| {
                                            w.update(cx, |this, cx| f(this, ix, window, cx))
                                        },
                                    )
                                };
                                let (copy, reveal) = (root.clone(), root.clone());
                                menu.item(tab("Close Tab", Workspace::close_tab))
                                    .item(tab("Close Other Tabs", Workspace::close_others))
                                    .separator()
                                    .item(tab("Open in Terminal", |this, ix, window, cx| {
                                        this.activate(ix, window, cx);
                                        window.dispatch_action(Box::new(OpenTerminal), cx);
                                    }))
                                    .item(tab("Open in Editor", |this, ix, window, cx| {
                                        this.activate(ix, window, cx);
                                        window.dispatch_action(Box::new(OpenInEditor), cx);
                                    }))
                                    .item(
                                        PopupMenuItem::new("Show in File Manager")
                                            .disabled(reveal.is_none())
                                            .on_click(move |_, _, cx| {
                                                reveal.iter().for_each(|p| cx.reveal_path(p))
                                            }),
                                    )
                                    .item(
                                        PopupMenuItem::new("Copy Path")
                                            .disabled(copy.is_none())
                                            .on_click(move |_, _, cx| {
                                                if let Some(p) = &copy {
                                                    cx.write_to_clipboard(
                                                        ClipboardItem::new_string(
                                                            p.display().to_string(),
                                                        ),
                                                    )
                                                }
                                            }),
                                    )
                            })
                            .child(
                                Button::new(("close-tab", ix))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Close)
                                    .tooltip("Close tab")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.close_tab(ix, window, cx)
                                    })),
                            )
                    }),
            );
        // one repository needs no tabs: the project widget names it, as in IntelliJ
        let strip = (self.tabs.len() > 1).then_some(strip);
        let appearance = tab.read(cx).prefs.appearance;
        let (search, settings) = (
            spork_workbench::ActionButton::new("quick-launch", "general/search")
                .size(spork_workbench::ActionSize::MainToolbar)
                .tooltip("Search Everywhere (⌘P)")
                .on_click(|_, window, cx| window.dispatch_action(Box::new(QuickLaunch), cx)),
            spork_workbench::ActionButton::new("preferences", "general/settings")
                .size(spork_workbench::ActionSize::MainToolbar)
                .tooltip("IDE and Project Settings")
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
                    use crate::prefs::Appearance;
                    let theme =
                        move |label: &'static str, value: Appearance, action: Box<dyn Action>| {
                            PopupMenuItem::new(label)
                                .checked(appearance == value)
                                .on_click(move |_, window, cx| {
                                    window.dispatch_action(action.boxed_clone(), cx)
                                })
                        };
                    // The reference app's gear (IntelliJ's SettingsEntryPointGroup); Theme is off while the
                    // theme follows the OS, as there
                    let synced = appearance == Appearance::System;
                    let themes = PopupMenu::build(window, cx, move |menu, _, _| {
                        menu.item(theme("Islands Dark", Appearance::Dark, Box::new(ThemeDark)))
                            .item(theme(
                                "Islands Light",
                                Appearance::Light,
                                Box::new(ThemeLight),
                            ))
                            .separator()
                            .item(theme(
                                "Sync with OS",
                                Appearance::System,
                                Box::new(ThemeSync),
                            ))
                    });
                    menu.menu_with_icon(
                        "Run Anything…",
                        spork_workbench::icon("general/runAnything"),
                        Box::new(RunAnything),
                    )
                    .separator()
                    .menu_with_icon(
                        "Settings…",
                        spork_workbench::icon("general/settings"),
                        Box::new(Preferences),
                    )
                    .menu("Plugins…", Box::new(ShowPlugins))
                    .separator()
                    .menu("Backup and Sync…", Box::new(ShowBackupAndSync))
                    .separator()
                    .item(PopupMenuItem::submenu("Theme…", themes).disabled(synced))
                    .submenu("Keymap…", window, cx, |menu, _, _| {
                        menu.item(PopupMenuItem::new("macOS").checked(true))
                    })
                    .submenu("View Mode…", window, cx, |menu, _, _| {
                        menu.menu("Full Screen", Box::new(ToggleFullScreen))
                    })
                    .separator()
                    .menu("Customize Toolbar…", Box::new(CustomizeToolbar))
                }),
        );
        v_flex()
            .size_full()
            .text_sm()
            .bg(theme.title_bar)
            .on_action(cx.listener(|this, _: &NewTab, window, cx| this.new_tab(window, cx)))
            .on_action(
                cx.listener(|this, _: &CloseTab, window, cx| {
                    this.close_tab(this.active, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &NextTab, window, cx| this.step(1, window, cx)))
            .on_action(
                cx.listener(|this, _: &ReopenClosedTab, window, cx| this.reopen_closed(window, cx)),
            )
            .on_action(cx.listener(|this, _: &PreviousTab, window, cx| this.step(-1, window, cx)))
            .children(glow)
            .child(
                TitleBar::new()
                    .h(px(40.))
                    // flat, the frame's color under the project's glow
                    .bg(transparent_black())
                    .border_b_0()
                    .when(cfg!(not(target_os = "macos")), |t| {
                        t.child(self.menu_bar.clone())
                    })
                    .child(
                        h_flex()
                            .gap_1()
                            .when(!window_active, |t| {
                                t.opacity(spork_workbench::shell::INACTIVE_ALPHA)
                            })
                            .children(toolbar)
                            .children(strip),
                    )
                    .child(
                        h_flex()
                            .pr(px(spork_workbench::tokens::metrics::MAIN_TOOLBAR_END_INSET))
                            .gap(px(spork_workbench::tokens::metrics::MAIN_TOOLBAR_GAP))
                            .when(!window_active, |t| {
                                t.opacity(spork_workbench::shell::INACTIVE_ALPHA)
                            })
                            .children(
                                fps.map(|text| {
                                    div().text_xs().text_color(muted).px_2().child(text)
                                }),
                            )
                            .child(search)
                            .child(settings),
                    ),
            )
            .child(div().flex_1().min_h_0().child(tab))
            // painted last: the end of the frame's work, for the meter
            .when(metering, |root| {
                root.child(
                    canvas(
                        |_, _, _| {},
                        |_, _, _, _| {
                            METER.with_borrow_mut(|m| m.as_mut().map(FrameMeter::painted));
                        },
                    )
                    .size_0(),
                )
            })
    }
}
