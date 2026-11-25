//! The Welcome window, as the reference app's (docs/reference-spec.md §1): shown at launch when there is no
//! project to reopen, and again when the last project closes. A sidebar (the app, Projects,
//! Customize, a gear) beside the page: the recent projects with New Project / Open / Clone
//! Repository, or the theme, language, font size and keymap.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::prefs::{Appearance, Prefs};
use crate::{RepoView, repos};

type Prompts = smol::channel::Receiver<crate::askpass::Prompt>;

thread_local! {
    /// The open Welcome window: asking again brings it to the front.
    static OPEN: RefCell<Option<AnyWindowHandle>> = const { RefCell::new(None) };
    /// git's password questions, kept for the first project opened from here.
    static PROMPTS: RefCell<Option<Prompts>> = const { RefCell::new(None) };
}

const SIZE: Size<Pixels> = Size {
    width: px(800.),
    height: px(650.),
};
/// IntelliJ's "IDE font" sizes, within what Spork allows (9–24).
const FONT_SIZES: [u32; 11] = [9, 10, 11, 12, 13, 14, 16, 18, 20, 22, 24];

/// Shows the Welcome window, or brings it to the front. `prompts` go to the first project opened
/// from it.
pub(crate) fn open(prompts: Option<Prompts>, cx: &mut App) {
    if prompts.is_some() {
        PROMPTS.with_borrow_mut(|stored| *stored = prompts);
    }
    let open = OPEN.with_borrow(|w| *w);
    if let Some(handle) = open
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    // centered across, a third of the way down, as IntelliJ places it
    let bounds = match cx.primary_display() {
        Some(display) => {
            let area = display.bounds();
            let x = area.origin.x + (area.size.width - SIZE.width) / 2.;
            let y = area.origin.y + (area.size.height - SIZE.height) / 3.;
            Bounds::new(point(x, y), SIZE)
        }
        None => Bounds::centered(None, SIZE, cx),
    };
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(SIZE),
        titlebar: Some(TitlebarOptions {
            title: Some("Welcome to Spork".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.), px(12.))),
        }),
        ..Default::default()
    };
    match gpui_kit::open_window(options, cx, |window, cx| {
        // the user's theme and text size, as a project window applies them
        Prefs::load().apply_appearance(window, cx);
        cx.new(|cx| Welcome::new(window, cx))
    }) {
        Ok((handle, _)) => OPEN.with_borrow_mut(|w| *w = Some(handle)),
        Err(e) => eprintln!("failed to open the Welcome window: {e}"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Projects,
    Customize,
}

/// A recent project as the list shows it.
struct Project {
    path: PathBuf,
    name: String,
    /// The path with `~` for the home folder.
    shown: String,
    branch: Option<String>,
    slot: usize,
}

impl Project {
    fn load(path: PathBuf) -> Self {
        Self {
            name: repos::display_name(&path),
            shown: repos::tilde(&path),
            branch: repos::head_branch(&path),
            slot: spork_workbench::shell::project_slot(&path),
            path,
        }
    }

    fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || self.name.to_lowercase().contains(query)
            || self.shown.to_lowercase().contains(query)
    }
}

pub(crate) struct Welcome {
    focus: FocusHandle,
    page: Page,
    search: Entity<InputState>,
    projects: Vec<Project>,
    _subscription: Subscription,
}

impl Welcome {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search projects"));
        let _subscription = cx.subscribe(&search, |_, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                cx.notify()
            }
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            focus,
            page: Page::Projects,
            search,
            projects: repos::recent().into_iter().map(Project::load).collect(),
            _subscription,
        }
    }

    fn close(window: &mut Window) {
        OPEN.with_borrow_mut(|w| *w = None);
        window.remove_window();
    }

    /// Opens `path` in a project window, in place of this one.
    fn open_project(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        crate::open_repo_window(vec![path], PROMPTS.take(), cx);
        Self::close(window);
    }

    fn open_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(mut paths))) = picked.await
                && let Some(folder) = paths.pop()
            {
                this.update_in(cx, |this, window, cx| this.open_project(folder, window, cx))
                    .ok();
            }
        })
        .detach();
    }

    /// New Project and Clone Repository: their dialogs belong to a project window, so one opens
    /// (on its empty page) and shows the dialog.
    fn in_project_window(
        &mut self,
        dialog: fn(&mut RepoView, &mut Window, &mut Context<RepoView>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((handle, workspace)) = crate::open_repo_window(Vec::new(), PROMPTS.take(), cx) {
            let tab = workspace.read(cx).active_tab();
            handle
                .update(cx, |_, window, cx| {
                    tab.update(cx, |v, cx| dialog(v, window, cx))
                })
                .ok();
        }
        Self::close(window);
    }

    /// Changes a preference at once, as the Welcome window's Customize page does.
    fn customize(
        &mut self,
        change: impl FnOnce(&mut Prefs),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut prefs = Prefs::load();
        change(&mut prefs);
        prefs.save();
        prefs.apply_appearance(window, cx);
        cx.refresh_windows();
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (muted, selected) = (theme.muted_foreground, theme.list_active);
        let nav = |id: &'static str, label: &'static str, page: Page, cx: &mut Context<Self>| {
            let on = self.page == page;
            h_flex()
                .id(id)
                .h(px(32.))
                .mx_2()
                .pl(px(16.))
                .rounded(px(4.))
                .cursor_pointer()
                .when(on, |r| r.bg(selected))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.page = page;
                    cx.notify();
                }))
        };
        let icon: Arc<Image> = Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../icon/spork.png").to_vec(),
        ));
        let gear = Button::new("welcome-gear")
            .ghost()
            .child(spork_workbench::icon("general/settings").size(px(16.)))
            .dropdown_menu_with_anchor(Anchor::BottomLeft, |menu, _, _| {
                menu.menu("Settings…", Box::new(crate::shell::Preferences))
                    .menu("About", Box::new(crate::shell::About))
            });
        v_flex()
            .w(px(224.))
            .h_full()
            .flex_none()
            .child(
                h_flex()
                    .pt(px(28.))
                    .px(px(10.))
                    .pb(px(25.))
                    .gap_2()
                    .child(img(icon).size(px(28.)))
                    .child(
                        v_flex().child("Spork").child(
                            div()
                                .text_size(px(11.))
                                .text_color(muted)
                                .child(env!("CARGO_PKG_VERSION")),
                        ),
                    ),
            )
            .child(nav("nav-projects", "Projects", Page::Projects, cx))
            .child(nav("nav-customize", "Customize", Page::Customize, cx))
            .child(div().flex_1())
            .child(h_flex().py(px(15.)).px(px(14.)).child(gear))
    }

    fn projects_page(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.projects.is_empty() {
            return self.empty_state(cx).into_any_element();
        }
        let query = self.search.read(cx).value().trim().to_lowercase();
        let theme = cx.theme();
        let (muted, hover, line) = (theme.muted_foreground, theme.list_hover, theme.border);
        let rows: Vec<_> = self
            .projects
            .iter()
            .enumerate()
            .filter(|(_, p)| p.matches(&query))
            .map(|(ix, p)| self.project_row(ix, p, muted, hover, cx))
            .collect();
        let action = |id: &'static str, label: &'static str| Button::new(id).outline().label(label);
        v_flex()
            .size_full()
            .py(px(13.))
            .px(px(12.))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div().flex_1().child(
                            spork_workbench::TextField::new(&self.search)
                                .prefix(spork_workbench::icon("general/search").size(px(16.))),
                        ),
                    )
                    .child(action("new-project", "New Project").on_click(cx.listener(
                        |this, _, window, cx| {
                            this.in_project_window(RepoView::new_repo_dialog, window, cx)
                        },
                    )))
                    .child(
                        action("open", "Open").on_click(
                            cx.listener(|this, _, window, cx| this.open_dialog(window, cx)),
                        ),
                    )
                    .child(action("clone", "Clone Repository").on_click(cx.listener(
                        |this, _, window, cx| {
                            this.in_project_window(RepoView::clone_dialog, window, cx)
                        },
                    ))),
            )
            .child(div().mt(px(12.)).mb(px(10.)).h(px(1.)).bg(line))
            .child(
                div()
                    .id("recent-projects")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(v_flex().gap_0p5().children(rows)),
            )
            .into_any_element()
    }

    fn project_row(
        &self,
        ix: usize,
        p: &Project,
        muted: Hsla,
        hover: Hsla,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (open, reveal, copy, forget) = (
            p.path.clone(),
            p.path.clone(),
            p.path.clone(),
            p.path.clone(),
        );
        let view = cx.entity();
        let more = Button::new(("project-more", ix))
            .ghost()
            .xsmall()
            .child(spork_workbench::icon("general/settings").size(px(16.)))
            .tooltip("More Actions")
            .dropdown_menu(move |menu, _, _| {
                let (open_view, forget_view) = (view.clone(), view.clone());
                let (open, reveal, copy, forget) =
                    (open.clone(), reveal.clone(), copy.clone(), forget.clone());
                menu.item(
                    PopupMenuItem::new("Open Selected").on_click(move |_, window, cx| {
                        let path = open.clone();
                        open_view.update(cx, |this, cx| this.open_project(path, window, cx))
                    }),
                )
                .item(
                    PopupMenuItem::new("Reveal in Finder")
                        .on_click(move |_, _, cx| cx.reveal_path(&reveal)),
                )
                .item(PopupMenuItem::new("Copy Path").on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy.display().to_string()))
                }))
                .separator()
                .item(PopupMenuItem::new("Remove from Recent Projects").on_click(
                    move |_, _, cx| {
                        repos::forget(&forget);
                        forget_view.update(cx, |this, cx| {
                            this.projects.retain(|prefs| prefs.path != forget);
                            cx.notify();
                        })
                    },
                ))
            });
        let path = p.path.clone();
        h_flex()
            .id(("project", ix))
            .group("project-row")
            .items_start()
            .p(px(4.))
            .rounded(px(4.))
            .cursor_pointer()
            .hover(move |r| r.bg(hover))
            .child(
                div()
                    .mt(px(6.))
                    .ml(px(6.))
                    .mr(px(8.))
                    .child(spork_workbench::shell::project_badge(&p.name, p.slot)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .m(px(4.))
                    .gap(px(4.))
                    .child(p.name.clone())
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .truncate()
                            .child(p.shown.clone()),
                    )
                    .children(p.branch.clone().map(|branch| {
                        h_flex()
                            .gap_1()
                            .text_xs()
                            .text_color(muted)
                            .child(spork_workbench::icon("general/vcs").size(px(12.)))
                            .child(branch)
                    })),
            )
            .child(
                div()
                    .mt(px(6.))
                    .mr(px(20.))
                    .invisible()
                    .group_hover("project-row", |s| s.visible())
                    .child(more),
            )
            .on_click(
                cx.listener(move |this, _, window, cx| this.open_project(path.clone(), window, cx)),
            )
    }

    /// No recent project: IntelliJ's welcome text and three large buttons.
    fn empty_state(&self, cx: &mut Context<Self>) -> Div {
        let new = big_button(
            "big-new",
            "welcome/createNewProjectTab",
            &["New Project"],
            cx,
        )
        .on_click(cx.listener(|this, _, window, cx| {
            this.in_project_window(RepoView::new_repo_dialog, window, cx)
        }));
        let open = big_button("big-open", "welcome/open", &["Open"], cx)
            .on_click(cx.listener(|this, _, window, cx| this.open_dialog(window, cx)));
        let clone = big_button(
            "big-clone",
            "welcome/fromVCSTab",
            &["Clone", "Repository"],
            cx,
        )
        .on_click(cx.listener(|this, _, window, cx| {
            this.in_project_window(RepoView::clone_dialog, window, cx)
        }));
        empty_state(None, [new, open, clone], cx)
    }

    /// Theme: Islands Dark or Light, off while it follows the OS.
    fn theme_combo(&self, sync: bool, dark: bool, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity();
        let pick = move |label: &'static str, checked: bool, change: fn(&mut Prefs)| {
            let this = this.clone();
            PopupMenuItem::new(label)
                .checked(checked)
                .on_click(move |_, window, cx| {
                    this.update(cx, |this, cx| this.customize(change, window, cx))
                })
        };
        let current = if dark {
            "Islands Dark"
        } else {
            "Islands Light"
        };
        spork_workbench::controls::combo("welcome-theme", current, sync, move |menu, _, _| {
            menu.item(pick("Islands Dark", dark, |prefs| {
                prefs.appearance = Appearance::Dark
            }))
            .item(pick("Islands Light", !dark, |prefs| {
                prefs.appearance = Appearance::Light
            }))
        })
    }

    /// IDE font: the interface's text size.
    fn font_size_combo(&self, size: u32, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity();
        spork_workbench::controls::combo(
            "welcome-font",
            size.to_string(),
            false,
            move |menu, _, _| {
                FONT_SIZES.into_iter().fold(menu, |menu, n| {
                    let view = view.clone();
                    menu.item(
                        PopupMenuItem::new(n.to_string())
                            .checked(n == size)
                            .on_click(move |_, window, cx| {
                                // 13 is the default, stored as 0 (prefs.rs)
                                let stored = if n == 13 { 0 } else { n };
                                view.update(cx, |this, cx| {
                                    this.customize(|prefs| prefs.ui_font_size = stored, window, cx)
                                })
                            }),
                    )
                })
            },
        )
    }

    fn customize_page(&self, cx: &mut Context<Self>) -> Div {
        let prefs = Prefs::load();
        let sync = prefs.appearance == Appearance::System;
        let dark = cx.theme().mode.is_dark();
        let header = |text: &'static str| {
            div()
                .pt_4()
                .pb_1()
                .text_size(px(16.))
                .font_weight(FontWeight::BOLD)
                .child(text)
        };
        let row =
            |label: &'static str, control: AnyElement| h_flex().gap_2().child(label).child(control);
        let theme = self.theme_combo(sync, dark, cx);
        let font = self.font_size_combo(prefs.text_size() as u32, cx);
        let only = |id: &'static str, label: &'static str| {
            spork_workbench::controls::combo(id, label, false, move |menu, _, _| {
                menu.item(PopupMenuItem::new(label).checked(true))
            })
        };
        let sync_check =
            spork_workbench::controls::labeled_check("welcome-sync", "Sync with OS", sync, cx)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.customize(
                        move |prefs| {
                            prefs.appearance = if sync {
                                if dark {
                                    Appearance::Dark
                                } else {
                                    Appearance::Light
                                }
                            } else {
                                Appearance::System
                            }
                        },
                        window,
                        cx,
                    )
                }));
        let link = |id: &'static str, label: &'static str, cx: &mut Context<Self>| {
            div()
                .id(id)
                .text_color(cx.theme().link)
                .cursor_pointer()
                .child(label)
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(crate::shell::Preferences), cx)
                })
        };
        v_flex()
            .pt(px(23.))
            .pl(px(30.))
            .pr(px(20.))
            .pb(px(20.))
            .gap_3()
            .child(header("Appearance"))
            .child(
                h_flex()
                    .gap_3()
                    .child(row("Theme:", theme))
                    .child(sync_check),
            )
            .child(header("Language and Region"))
            .child(row("Language:", only("welcome-language", "English")))
            .child(row("Region:", only("welcome-region", "Not specified")))
            .child(header("Accessibility"))
            .child(row("IDE font:", font))
            .child(header("Keymap"))
            .child(
                h_flex()
                    .gap_3()
                    .child(only("welcome-keymap", "macOS"))
                    .child(link("keymap-configure", "Configure…", cx)),
            )
            .child(
                div()
                    .pt_4()
                    .child(link("all-settings", "All settings…", cx)),
            )
    }
}

/// A 60 px rounded square with an icon and its label below (lines stacked), as IntelliJ's
/// empty welcome page. Add `on_click`.
pub(crate) fn big_button(
    id: &'static str,
    icon: &str,
    label: &[&'static str],
    cx: &App,
) -> Stateful<Div> {
    let fill = match cx.theme().mode.is_dark() {
        true => rgb(0x2B2D30),
        false => rgb(0xF7F8F9),
    };
    let hover = cx.theme().secondary_hover;
    v_flex()
        .id(id)
        .px(px(30.))
        .items_center()
        .gap(px(12.))
        .cursor_pointer()
        .child(
            div()
                .size(px(60.))
                .rounded(px(12.))
                .bg(fill)
                .flex()
                .items_center()
                .justify_center()
                .hover(move |s| s.bg(hover))
                .child(spork_workbench::image(icon, cx).size(px(24.))),
        )
        .child(v_flex().items_center().children(label.iter().copied()))
}

/// IntelliJ's empty welcome page: the title, two grey lines (or what went wrong opening a
/// folder), and the large buttons.
pub(crate) fn empty_state(
    problem: Option<String>,
    buttons: impl IntoIterator<Item = Stateful<Div>>,
    cx: &App,
) -> Div {
    let muted = cx.theme().muted_foreground;
    let lines: Vec<String> = match problem {
        Some(problem) => vec![problem],
        None => vec![
            "Create a new project to start from scratch.".into(),
            "Open existing project from disk or version control.".into(),
        ],
    };
    v_flex()
        .size_full()
        .items_center()
        .pt(px(105.))
        .child(
            div()
                .text_size(px(26.))
                .font_weight(FontWeight::BOLD)
                .child("Welcome to Spork"),
        )
        .child(
            v_flex()
                .mt(px(21.))
                .items_center()
                .gap(px(7.))
                .text_color(muted)
                .children(lines),
        )
        .child(h_flex().mt(px(27.)).items_start().children(buttons))
}

impl Render for Welcome {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, line) = (theme.background, theme.foreground, theme.title_bar);
        let side = match theme.mode.is_dark() {
            true => rgb(0x191A1C),
            false => rgb(0xF7F8F9),
        };
        let sidebar = self.sidebar(cx);
        let page = match self.page {
            Page::Projects => self.projects_page(cx),
            Page::Customize => div()
                .id("customize")
                .size_full()
                .overflow_y_scrollbar()
                .child(self.customize_page(cx))
                .into_any_element(),
        };
        v_flex()
            .size_full()
            .track_focus(&self.focus)
            .bg(bg)
            .text_color(fg)
            .text_sm()
            // the menu bar's commands that make sense without a project
            .on_action(|_: &crate::shell::Preferences, window, cx| {
                crate::settings_window::open(None, Prefs::load(), None, window, cx)
            })
            .on_action(
                cx.listener(|this, _: &crate::shell::OpenRepository, window, cx| {
                    this.open_dialog(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::shell::CloneRepository, window, cx| {
                    this.in_project_window(RepoView::clone_dialog, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::shell::NewRepository, window, cx| {
                    this.in_project_window(RepoView::new_repo_dialog, window, cx)
                }),
            )
            // the title bar's row: the traffic lights over the sidebar, the title centered
            .child(spork_workbench::shell::window_title("Welcome to Spork", cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(sidebar.h_full().bg(side).border_r_1().border_color(line))
                    .child(div().flex_1().min_w_0().h_full().child(page)),
            )
    }
}
