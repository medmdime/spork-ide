//! The Welcome window (reference-spec.md §1, reference/idea/screenshots/frame/welcome.jpg): IntelliJ
//! shows it at launch when no project is reopened and again when the last project window closes;
//! closing it quits. A sidebar (the app, Projects, Customize, Plugins, Learn, a gear) beside the
//! page: the recent projects with New Project, Open and Clone Repository, or the theme, IDE font
//! and keymap.

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;

use fs::Fs;
use gpui_kit::component::input::{Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use settings::{BaseKeymap, Settings as _};
use spork_workbench::tokens::{Tokens, Typography as _};
use theme_settings::{ThemeAppearanceMode, ThemeSelection, ThemeSettings};
use util::ResultExt as _;

use crate::RecentProject;

thread_local! {
    /// The open Welcome window: asking again brings it forward.
    static OPEN: Cell<Option<AnyWindowHandle>> = const { Cell::new(None) };
}

const TITLE: &str = "Welcome to Spork IDE";
const SIZE: Size<Pixels> = Size {
    width: px(800.),
    height: px(650.),
};
/// IntelliJ's text is 0.875 of the IDE's rem (`spork_workbench`'s type scale): its "IDE font" 13
/// is the IDE's UI font size 14.857.
const TEXT_PER_REM: f32 = 0.875;
/// IntelliJ's IDE font sizes (reference-spec.md §1, Customize page).
const FONT_SIZES: [u32; 17] = [
    8, 9, 10, 11, 12, 13, 14, 16, 18, 20, 22, 24, 26, 28, 36, 48, 72,
];
/// The IDE's themes in IntelliJ's order: the Islands, then the classic ones.
const THEMES: [&[&str]; 2] = [
    &[
        "Islands Dark",
        "Islands Light",
        "Islands Darcula",
        "High Contrast",
    ],
    &[
        "Dark",
        "Light",
        "Light with Light Header",
        "Darcula",
        "IntelliJ Light",
    ],
];

/// Copy Path: a recent project's folders, a line each.
fn copy_paths(paths: &[std::path::PathBuf], cx: &mut App) {
    let text = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    cx.write_to_clipboard(ClipboardItem::new_string(text))
}

/// Shows the Welcome window, or brings it forward.
pub(crate) fn show(cx: &mut App) {
    if let Some(handle) = OPEN.get()
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
            title: Some(TITLE.into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.), px(8.))),
        }),
        app_id: Some(
            release_channel::ReleaseChannel::global(cx)
                .app_id()
                .to_owned(),
        ),
        ..WindowOptions::default()
    };
    match gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| Welcome::new(window, cx))
    }) {
        Ok((handle, _)) => OPEN.set(Some(handle)),
        Err(e) => log::error!("failed to open the Welcome window: {e:#}"),
    }
}

/// Closes the Welcome window: a project window took its place.
pub(crate) fn close(cx: &mut App) {
    if let Some(handle) = OPEN.take() {
        handle
            .update(cx, |_, window, _| window.remove_window())
            .log_err();
    }
}

/// A window closed: the Welcome window shows when no project window is left, and closing the
/// Welcome window with no project open quits, as IntelliJ's `WelcomeFrame` does.
pub(crate) fn on_window_closed(closed: WindowId, cx: &mut App) {
    let welcome = OPEN.get();
    if welcome.is_some_and(|handle| handle.window_id() == closed) {
        OPEN.set(None);
        if !crate::any_project_window(cx) {
            cx.quit();
        }
    } else if welcome.is_none() && !crate::any_project_window(cx) {
        show(cx);
    }
}

/// A row of the Projects page: a group (by its place in the groups), or a project (by its place
/// in the recent projects) at its depth.
#[derive(Clone, Copy)]
enum Row {
    Group(usize),
    Project(usize, usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Projects,
    Customize,
    Learn,
}

pub(crate) struct Welcome {
    focus: FocusHandle,
    page: Page,
    search: Entity<InputState>,
    /// The recent projects once read, newest first.
    projects: Option<Vec<RecentProject>>,
    /// The project row selected among those the search shows.
    selected: usize,
    /// IntelliJ's recent project groups.
    groups: Vec<crate::project_groups::ProjectGroup>,
    _subscriptions: Vec<Subscription>,
    _load: Task<()>,
}

impl Welcome {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search projects"));
        let subscriptions = vec![
            cx.subscribe_in(&search, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.selected = 0;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.open_selected(window, cx),
                _ => {}
            }),
            // the theme or the IDE font changed from the Customize page or elsewhere
            cx.observe_global_in::<settings::SettingsStore>(window, |_, _, cx| cx.notify()),
        ];
        search.update(cx, |search, cx| search.focus(window, cx));
        let load = crate::load_recent_projects(cx);
        let _load = cx.spawn(async move |this, cx| {
            let projects = load.await;
            this.update(cx, |this, cx| {
                this.projects = Some(projects);
                cx.notify();
            })
            .ok();
        });
        Self {
            focus: cx.focus_handle(),
            page: Page::Projects,
            search,
            projects: None,
            selected: 0,
            groups: crate::project_groups::read_groups(cx),
            _subscriptions: subscriptions,
            _load,
        }
    }

    /// The Projects page's rows: the groups first (the one with the most recent project first,
    /// then by name) with their projects while open, then the projects in no group. A search
    /// opens every group and shows the projects it finds.
    fn rows(&self, cx: &App) -> Vec<Row> {
        let query = self.search.read(cx).value().trim().to_lowercase();
        let projects: Vec<&RecentProject> = self.projects.iter().flatten().collect();
        let index_of =
            |paths: &Vec<PathBuf>| projects.iter().position(|project| &project.paths == paths);
        let mut order: Vec<usize> = (0..self.groups.len()).collect();
        let newest = |group: &crate::project_groups::ProjectGroup| {
            group
                .projects
                .iter()
                .filter_map(index_of)
                .min()
                .unwrap_or(usize::MAX)
        };
        order.sort_by(|a, b| {
            let (a, b) = (&self.groups[*a], &self.groups[*b]);
            newest(a).cmp(&newest(b)).then_with(|| a.name.cmp(&b.name))
        });
        let mut rows = Vec::new();
        let mut grouped = Vec::new();
        for gix in order {
            let group = &self.groups[gix];
            let members: Vec<usize> = group
                .projects
                .iter()
                .filter_map(index_of)
                .filter(|ix| projects[*ix].matches(&query))
                .collect();
            grouped.extend(group.projects.iter().filter_map(index_of));
            if !query.is_empty() && members.is_empty() {
                continue;
            }
            rows.push(Row::Group(gix));
            if group.expanded || !query.is_empty() {
                rows.extend(members.into_iter().map(|ix| Row::Project(ix, 1)));
            }
        }
        rows.extend(
            (0..projects.len())
                .filter(|ix| !grouped.contains(ix) && projects[*ix].matches(&query))
                .map(|ix| Row::Project(ix, 0)),
        );
        rows
    }

    fn shown_projects(&self, cx: &App) -> Vec<(usize, &RecentProject)> {
        let projects: Vec<&RecentProject> = self.projects.iter().flatten().collect();
        self.rows(cx)
            .into_iter()
            .filter_map(|row| match row {
                Row::Project(ix, _) => projects.get(ix).map(|project| (ix, *project)),
                Row::Group(_) => None,
            })
            .collect()
    }

    fn save_groups(&mut self, cx: &mut Context<Self>) {
        crate::project_groups::write_groups(&self.groups, cx);
        cx.notify();
    }

    /// IntelliJ's New Project Group: a name no other group has.
    fn new_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let names: Vec<String> = self.groups.iter().map(|group| group.name.clone()).collect();
        let this = cx.entity().downgrade();
        crate::project_groups::name_dialog(
            "Create New Project Group",
            "Project group name",
            String::new(),
            move |name| {
                names
                    .iter()
                    .any(|taken| taken == name.trim())
                    .then(|| SharedString::from("taken"))
            },
            false,
            move |name, _, cx| {
                this.update(cx, |this, cx| {
                    this.groups.push(crate::project_groups::ProjectGroup {
                        name,
                        ..Default::default()
                    });
                    this.save_groups(cx);
                })
                .ok();
            },
            window,
            cx,
        );
    }

    /// IntelliJ's Edit… on a group: Change Group Name.
    fn edit_group(&mut self, gix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self.groups.get(gix).map(|group| group.name.clone()) else {
            return;
        };
        let groups = self.groups.clone();
        let this = cx.entity().downgrade();
        crate::project_groups::name_dialog(
            "Change Group Name",
            "Enter group name:",
            current.clone(),
            move |name| crate::project_groups::rename_error(&groups, &current, name),
            true,
            move |name, _, cx| {
                this.update(cx, |this, cx| {
                    if let Some(group) = this.groups.get_mut(gix) {
                        group.name = name.trim().to_owned();
                    }
                    this.save_groups(cx);
                })
                .ok();
            },
            window,
            cx,
        );
    }

    fn move_project(&mut self, paths: Vec<PathBuf>, to: Option<String>, cx: &mut Context<Self>) {
        crate::project_groups::move_to_group(&mut self.groups, &paths, to.as_deref());
        self.save_groups(cx);
    }

    fn toggle_group(&mut self, gix: usize, cx: &mut Context<Self>) {
        if let Some(group) = self.groups.get_mut(gix) {
            group.expanded = !group.expanded;
        }
        self.save_groups(cx);
    }

    /// Remove from Recent Projects… on a group, once confirmed: the group and its projects go.
    fn remove_group(&mut self, gix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(group) = self.groups.get(gix).cloned() else {
            return;
        };
        let this = cx.entity().downgrade();
        let message = format!("Remove '{}' from recent projects list?", group.name);
        window.open_dialog(cx, move |dialog, _, _| {
            let (this, group) = (this.clone(), group.clone());
            crate::dialogs::confirm(
                dialog,
                "Remove Recent Project",
                message.clone(),
                "Remove",
                move |_, cx| {
                    let group = group.clone();
                    this.update(cx, |this, cx| {
                        let members: Vec<RecentProject> = this
                            .projects
                            .iter()
                            .flatten()
                            .filter(|project| group.projects.contains(&project.paths))
                            .cloned()
                            .collect();
                        for project in &members {
                            let forget = crate::forget_recent_project(project, cx);
                            cx.spawn(async move |_, _| forget.await).detach();
                        }
                        if let Some(projects) = &mut this.projects {
                            projects.retain(|project| !group.projects.contains(&project.paths));
                        }
                        this.groups.retain(|kept| kept.name != group.name);
                        this.selected = 0;
                        this.save_groups(cx);
                    })
                    .ok();
                },
            )
        });
    }

    /// A group's row: its name in bold, open or closed by a click, its ⋮ on hover.
    fn group_row(&self, gix: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let tokens = Tokens::get(cx);
        let Some(group) = self.groups.get(gix) else {
            return div().id(("project-group", gix));
        };
        let view = cx.entity().downgrade();
        let members: Vec<Vec<PathBuf>> = group.projects.clone();
        let more =
            spork_workbench::ActionButton::new(("project-group-more", gix), "general/moreVertical")
                .tooltip("More Actions")
                .dropdown_menu(move |menu: PopupMenu, _, _| {
                    let (open, new, edit, remove) =
                        (members.clone(), view.clone(), view.clone(), view.clone());
                    menu.item(PopupMenuItem::new("Open All Projects in Group").on_click(
                        move |_, _, cx| {
                            for paths in &open {
                                crate::open_project(paths.clone(), cx);
                            }
                        },
                    ))
                    .item(PopupMenuItem::new("Copy Path").disabled(true))
                    .separator()
                    .item(
                        PopupMenuItem::new("New Project Group").on_click(move |_, window, cx| {
                            new.update(cx, |this, cx| this.new_group(window, cx)).ok();
                        }),
                    )
                    .item(PopupMenuItem::new("Edit…").on_click(move |_, window, cx| {
                        edit.update(cx, |this, cx| this.edit_group(gix, window, cx))
                            .ok();
                    }))
                    .separator()
                    .item(
                        PopupMenuItem::new("Remove from Recent Projects…").on_click(
                            move |_, window, cx| {
                                remove
                                    .update(cx, |this, cx| this.remove_group(gix, window, cx))
                                    .ok();
                            },
                        ),
                    )
                });
        h_flex()
            .id(("project-group", gix))
            .role(Role::Button)
            .aria_label(SharedString::from(group.name.clone()))
            .aria_expanded(group.expanded)
            .group("project-group")
            .h(px(32.))
            .px(px(3.))
            .gap(px(4.))
            .rounded(px(4.))
            .cursor_pointer()
            .hover(move |row| row.bg(tokens.selection_hover))
            .child(
                spork_workbench::icon(match group.expanded {
                    true => "general/chevronDown",
                    false => "general/chevronRight",
                })
                .size(px(16.))
                .text_color(tokens.icon),
            )
            .child(
                div()
                    .flex_1()
                    .font_weight(FontWeight::BOLD)
                    .child(group.name.clone()),
            )
            .child(
                div()
                    .mr(px(14.))
                    .invisible()
                    .group_hover("project-group", |slot| slot.visible())
                    .child(more),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_group(gix, cx)))
    }

    fn select_next(&mut self, step: isize, cx: &mut Context<Self>) {
        let count = self.shown_projects(cx).len() as isize;
        if count > 0 {
            self.selected = (self.selected as isize + step).rem_euclid(count) as usize;
            cx.notify();
        }
    }

    fn copy_selected_path(&mut self, cx: &mut Context<Self>) {
        let paths = self
            .shown_projects(cx)
            .get(self.selected)
            .map(|(_, project)| project.paths.clone());
        if let Some(paths) = paths {
            copy_paths(&paths, cx);
        }
    }

    fn open_selected(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let paths = self
            .shown_projects(cx)
            .get(self.selected)
            .map(|(_, project)| project.paths.clone());
        if let Some(paths) = paths {
            crate::open_project(paths, cx);
        }
    }

    /// Removes the project at `ix` from the recent projects, once confirmed as IntelliJ asks.
    fn remove_project(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.projects.iter().flatten().nth(ix).cloned() else {
            return;
        };
        let this = cx.entity().downgrade();
        let message = format!("Remove '{}' from recent projects list?", project.name);
        window.open_dialog(cx, move |dialog, _, _| {
            let (this, project) = (this.clone(), project.clone());
            crate::dialogs::confirm(
                dialog,
                "Remove Recent Project",
                message.clone(),
                "Remove",
                move |_, cx| {
                    let forget = crate::forget_recent_project(&project, cx);
                    let path = project.paths.clone();
                    this.update(cx, |this, cx| {
                        if let Some(projects) = &mut this.projects {
                            projects.retain(|project| project.paths != path);
                        }
                        this.selected = 0;
                        cx.notify();
                        cx.spawn(async move |_, _| forget.await).detach();
                    })
                    .ok();
                },
            )
        });
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let nav = |id: &'static str, label: &'static str, page: Option<Page>| {
            let selected = page == Some(self.page);
            h_flex()
                .id(id)
                .role(Role::Tab)
                .aria_label(label)
                .aria_selected(selected)
                .h(px(32.))
                .mx(px(12.))
                .pl(px(23.))
                .rounded(px(4.))
                .cursor_pointer()
                .when(selected, |item| item.bg(tokens.selection))
                .child(label)
        };
        let icon = Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../../zed/resources/app-icon.png").to_vec(),
        ));
        let version = release_channel::AppVersion::global(cx);
        let gear = spork_workbench::ActionButton::new("welcome-settings", "general/settings")
            .tooltip("Settings")
            .dropdown_menu_with_anchor(Anchor::BottomLeft, |menu: PopupMenu, _, _| {
                // no workspace here to take OpenSettings: Settings for the IDE, as IntelliJ's
                // `WelcomeScreen.Settings` opens them for the default project
                menu.item(
                    PopupMenuItem::new("Settings…")
                        .on_click(|_, window, cx| spork_settings::open(None, None, window, cx)),
                )
                .menu("About", Box::new(zed_actions::About))
            });
        v_flex()
            .w(px(224.))
            .h_full()
            .flex_none()
            .bg(tokens.welcome_sidebar)
            .border_r_1()
            .border_color(tokens.separator)
            .child(
                h_flex()
                    .pt(px(28.))
                    .px(px(16.))
                    .pb(px(25.))
                    .gap(px(10.))
                    .child(img(icon).size(px(28.)))
                    .child(
                        v_flex().child("Spork IDE").child(
                            div()
                                .text_ui_xsmall()
                                .text_color(tokens.text_secondary)
                                .child(format!(
                                    "{}.{}.{}",
                                    version.major, version.minor, version.patch
                                )),
                        ),
                    ),
            )
            .child(
                v_flex()
                    .id("welcome-pages")
                    .role(Role::TabList)
                    .aria_label("Welcome")
                    .gap(px(1.))
                    .child(
                        nav("nav-projects", "Projects", Some(Page::Projects)).on_click(
                            cx.listener(|this, _, window, cx| {
                                this.page = Page::Projects;
                                this.search
                                    .update(cx, |search, cx| search.focus(window, cx));
                                cx.notify()
                            }),
                        ),
                    )
                    .child(
                        nav("nav-customize", "Customize", Some(Page::Customize)).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.page = Page::Customize;
                                cx.notify()
                            }),
                        ),
                    )
                    // IntelliJ's Plugins (`ShowPlugins`): Settings on its Plugins page
                    .child(
                        nav("nav-plugins", "Plugins", None).on_click(|_, window, cx| {
                            spork_settings::open(
                                None,
                                Some(spork_settings::Target::Plugins(String::new())),
                                window,
                                cx,
                            )
                        }),
                    )
                    .child(
                        nav("nav-learn", "Learn", Some(Page::Learn)).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.page = Page::Learn;
                                cx.notify()
                            },
                        )),
                    ),
            )
            .child(div().flex_1())
            .child(h_flex().py(px(15.)).px(px(14.)).child(gear))
    }

    fn projects_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(projects) = &self.projects else {
            return div().into_any_element();
        };
        if projects.is_empty() {
            return empty_state(cx).into_any_element();
        }
        let tokens = Tokens::get(cx);
        let mut shown = 0;
        let rows: Vec<AnyElement> = self
            .rows(cx)
            .into_iter()
            .filter_map(|row| match row {
                Row::Group(gix) => Some(self.group_row(gix, cx).into_any_element()),
                Row::Project(ix, depth) => {
                    let project = projects.get(ix)?;
                    let selected = shown == self.selected;
                    shown += 1;
                    // a group's projects under it, IntelliJ's 16 px further in
                    Some(
                        self.project_row(ix, project, selected, cx)
                            .ml(px(16. * depth as f32))
                            .into_any_element(),
                    )
                }
            })
            .collect();
        v_flex()
            .size_full()
            .py(px(13.))
            .px(px(12.))
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                cx.stop_propagation();
                this.select_next(-1, cx)
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                cx.stop_propagation();
                this.select_next(1, cx)
            }))
            .child(
                h_flex()
                    .gap_2()
                    // IntelliJ's search field here has no border
                    .child(
                        h_flex()
                            .flex_1()
                            .h(px(28.))
                            .gap(px(6.))
                            .px(px(6.))
                            .child(
                                spork_workbench::icon("general/search")
                                    .size(px(16.))
                                    .text_color(tokens.icon),
                            )
                            .child(
                                Input::new(&self.search)
                                    .appearance(false)
                                    .aria_label("Search projects")
                                    .flex_1(),
                            ),
                    )
                    .children(project_buttons()),
            )
            .child(div().mt(px(12.)).mb(px(10.)).h(px(1.)).bg(tokens.separator))
            .child(
                div()
                    .id("recent-projects")
                    .role(Role::List)
                    .aria_label("Recent Projects")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(v_flex().gap(px(2.)).children(rows)),
            )
            .into_any_element()
    }

    fn project_row(
        &self,
        ix: usize,
        project: &RecentProject,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let tokens = Tokens::get(cx);
        let view = cx.entity().downgrade();
        let menu_project = project.clone();
        // IntelliJ's natural order of the groups' names
        let mut group_names: Vec<String> =
            self.groups.iter().map(|group| group.name.clone()).collect();
        group_names.sort();
        let more =
            spork_workbench::ActionButton::new(("recent-project-more", ix), "general/moreVertical")
                .tooltip("More Actions")
                .dropdown_menu(move |menu: PopupMenu, window, cx| {
                    let (open, reveal, copy) = (
                        menu_project.paths.clone(),
                        menu_project.paths.clone(),
                        menu_project.paths.clone(),
                    );
                    let view = view.clone();
                    let (new, moved) = (view.clone(), view.clone());
                    let moving = menu_project.paths.clone();
                    let names = group_names.clone();
                    menu.item(
                        PopupMenuItem::new("Open Selected")
                            .on_click(move |_, _, cx| crate::open_project(open.clone(), cx)),
                    )
                    .item(
                        PopupMenuItem::new("Reveal in Finder").on_click(move |_, _, cx| {
                            if let Some(path) = reveal.first() {
                                cx.reveal_path(path)
                            }
                        }),
                    )
                    .item(
                        PopupMenuItem::new("Copy Path")
                            .on_click(move |_, _, cx| copy_paths(&copy, cx)),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new("New Project Group").on_click(move |_, window, cx| {
                            new.update(cx, |this, cx| this.new_group(window, cx)).ok();
                        }),
                    )
                    // IntelliJ's Move to Group: the groups, then Remove from Groups
                    .when(names.is_empty(), |menu| {
                        menu.item(PopupMenuItem::new("Move to Group").disabled(true))
                    })
                    .when(!names.is_empty(), |menu| {
                        menu.submenu("Move to Group", window, cx, move |menu, _, _| {
                            let menu = names.iter().fold(menu, |menu, name| {
                                let (moved, moving, name) =
                                    (moved.clone(), moving.clone(), name.clone());
                                menu.item(PopupMenuItem::new(name.clone()).on_click(
                                    move |_, _, cx| {
                                        moved
                                            .update(cx, |this, cx| {
                                                this.move_project(
                                                    moving.clone(),
                                                    Some(name.clone()),
                                                    cx,
                                                )
                                            })
                                            .ok();
                                    },
                                ))
                            });
                            let (moved, moving) = (moved.clone(), moving.clone());
                            menu.separator().item(
                                PopupMenuItem::new("Remove from Groups").on_click(
                                    move |_, _, cx| {
                                        moved
                                            .update(cx, |this, cx| {
                                                this.move_project(moving.clone(), None, cx)
                                            })
                                            .ok();
                                    },
                                ),
                            )
                        })
                    })
                    .separator()
                    .item(
                        PopupMenuItem::new("Remove from Recent Projects…").on_click(
                            move |_, window, cx| {
                                view.update(cx, |this, cx| this.remove_project(ix, window, cx))
                                    .ok();
                            },
                        ),
                    )
                });
        let paths = project.paths.clone();
        h_flex()
            .id(("recent-project", ix))
            .role(Role::Button)
            .aria_label(project.name.clone())
            .group("recent-project")
            .items_start()
            .p(px(4.))
            .rounded(px(4.))
            .cursor_pointer()
            .map(|row| match selected {
                true => row.bg(tokens.selection),
                false => row.hover(move |row| row.bg(tokens.selection_hover)),
            })
            .child(div().mt(px(6.)).ml(px(6.)).mr(px(8.)).child(
                spork_workbench::shell::project_badge(&project.name, project.slot),
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .m(px(4.))
                    .gap(px(4.))
                    .child(project.name.clone())
                    .child(
                        div()
                            .text_ui_small()
                            .text_color(tokens.text_secondary)
                            .truncate()
                            .child(project.location.clone()),
                    )
                    .children(project.branch.clone().map(|branch| {
                        h_flex()
                            .gap(px(4.))
                            .text_ui_small()
                            .text_color(tokens.text_secondary)
                            .child(
                                spork_workbench::icon("general/vcs")
                                    .size(px(14.))
                                    .text_color(tokens.text_secondary),
                            )
                            .child(branch)
                    })),
            )
            .child(
                div()
                    .mt(px(6.))
                    .mr(px(20.))
                    .when(!selected, |slot| {
                        slot.invisible()
                            .group_hover("recent-project", |slot| slot.visible())
                    })
                    .child(more),
            )
            .on_click(move |_, _, cx| crate::open_project(paths.clone(), cx))
    }

    fn customize_page(&self, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let settings = ThemeSettings::get_global(cx);
        let sync = matches!(
            settings.theme,
            ThemeSelection::Dynamic {
                mode: ThemeAppearanceMode::System,
                ..
            }
        );
        let app: &App = cx;
        let current = theme::ActiveTheme::theme(app).name.clone();
        let font_size = (f32::from(settings.ui_font_size_settings()) * TEXT_PER_REM).round() as u32;
        let keymap = *BaseKeymap::get_global(cx);
        let header = |text: &'static str| {
            div()
                .id(text)
                .role(Role::Heading)
                .aria_label(text)
                .pt(px(16.))
                .text_h3()
                .font_weight(FontWeight::BOLD)
                .child(text)
        };
        let row = |label: &'static str, control: AnyElement| {
            h_flex()
                .gap(px(8.))
                .child(spork_workbench::controls::form_label(label))
                .child(control)
        };
        let theme_combo = spork_workbench::controls::combo(
            "welcome-theme",
            current.clone(),
            false,
            move |menu, _, _| {
                THEMES
                    .iter()
                    .enumerate()
                    .fold(menu, |menu, (group, names)| {
                        let menu = if group > 0 { menu.separator() } else { menu };
                        names.iter().fold(menu, |menu, name| {
                            menu.item(
                                PopupMenuItem::new(*name)
                                    .checked(*name == current.as_ref())
                                    .on_click(move |_, _, cx| set_theme(name, cx)),
                            )
                        })
                    })
            },
        );
        let sync_check =
            spork_workbench::controls::labeled_check("welcome-sync", "Sync with OS", sync, cx)
                .on_click(move |_, _, cx| set_sync_with_os(!sync, cx));
        let font_combo = spork_workbench::controls::combo(
            "welcome-font",
            font_size.to_string(),
            false,
            move |menu, _, _| {
                FONT_SIZES.into_iter().fold(menu, |menu, size| {
                    menu.item(
                        PopupMenuItem::new(size.to_string())
                            .checked(size == font_size)
                            .on_click(move |_, _, cx| set_ui_font_size(size, cx)),
                    )
                })
            },
        );
        let keymap_name = BaseKeymap::OPTIONS
            .iter()
            .find(|(_, value)| *value == keymap)
            .map_or("", |(name, _)| *name);
        let keymap_combo = spork_workbench::controls::combo(
            "welcome-keymap",
            keymap_name,
            false,
            move |menu, _, _| {
                BaseKeymap::OPTIONS
                    .iter()
                    .fold(menu, |menu, &(name, value)| {
                        menu.item(
                            PopupMenuItem::new(name)
                                .checked(value == keymap)
                                .on_click(move |_, _, cx| set_base_keymap(value, cx)),
                        )
                    })
            },
        );
        let link = |id: &'static str, label: &'static str, action: Box<dyn Action>| {
            spork_workbench::controls::link(id, label, cx)
                .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
        };
        v_flex()
            .pt(px(23.))
            .pl(px(30.))
            .pr(px(20.))
            .pb(px(20.))
            .gap(px(12.))
            .text_color(tokens.text)
            .child(header("Appearance"))
            .child(
                h_flex()
                    .gap(px(12.))
                    .child(row("Theme:", theme_combo))
                    .child(sync_check),
            )
            .child(header("Accessibility"))
            .child(row("IDE font:", font_combo))
            .child(header("Keymap"))
            .child(h_flex().gap(px(12.)).child(keymap_combo).child(link(
                "welcome-keymap-configure",
                "Configure…",
                Box::new(zed_actions::OpenKeymap),
            )))
            .child(div().pt(px(16.)).child(link(
                "welcome-all-settings",
                "All settings…",
                Box::new(zed_actions::OpenSettings),
            )))
    }

    /// IntelliJ's Learn page comes from its IDE Features Trainer plugin and its lessons; what
    /// Spork IDE has of it is the keymap reference.
    fn learn_page(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .pt(px(23.))
            .pl(px(30.))
            .pr(px(20.))
            .gap(px(12.))
            .child(
                div()
                    .id("learn-title")
                    .role(Role::Heading)
                    .aria_label("Help and Resources")
                    .text_h3()
                    .font_weight(FontWeight::BOLD)
                    .child("Help and Resources"),
            )
            .child(
                spork_workbench::controls::link("learn-keymap", "Keymap Reference", cx).on_click(
                    |_, window, cx| window.dispatch_action(Box::new(zed_actions::OpenKeymap), cx),
                ),
            )
    }
}

/// New Project, Open and Clone Repository, the Projects page's buttons.
fn project_buttons() -> [spork_workbench::Button; 3] {
    [
        spork_workbench::Button::new("welcome-new-project", "New Project")
            .on_click(|_, window, cx| crate::dialogs::new_project(window, cx)),
        spork_workbench::Button::new("welcome-open", "Open").on_click(|_, window, cx| {
            window.dispatch_action(Box::new(workspace::Open::default()), cx)
        }),
        spork_workbench::Button::new("welcome-clone", "Clone Repository")
            .on_click(|_, window, cx| crate::dialogs::get_from_vcs(window, cx)),
    ]
}

/// No recent project: IntelliJ's welcome text and three large buttons.
fn empty_state(cx: &App) -> Div {
    let tokens = Tokens::get(cx);
    let big = |id: &'static str, icon: &'static str, lines: &[&'static str]| {
        let label = lines.join(" ");
        v_flex()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .px(px(30.))
            .items_center()
            .gap(px(12.))
            .cursor_pointer()
            .child(
                div()
                    .size(px(60.))
                    .rounded(px(12.))
                    .bg(tokens.welcome_button)
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(move |tile| tile.bg(tokens.hover))
                    .child(spork_workbench::image(icon, cx).size(px(24.))),
            )
            .child(v_flex().items_center().children(lines.iter().copied()))
    };
    v_flex()
        .size_full()
        .items_center()
        .pt(px(105.))
        .child(
            div()
                .id("welcome-title")
                .role(Role::Heading)
                .aria_label(TITLE)
                .text_h0()
                .child(TITLE),
        )
        .child(
            v_flex()
                .mt(px(21.))
                .items_center()
                .gap(px(7.))
                .text_color(tokens.text_secondary)
                .child("Create a new project to start from scratch.")
                .child("Open existing project from disk or version control."),
        )
        .child(
            h_flex()
                .mt(px(27.))
                .items_start()
                .child(
                    big(
                        "big-new-project",
                        "welcome/createNewProjectTab",
                        &["New Project"],
                    )
                    .on_click(|_, window, cx| crate::dialogs::new_project(window, cx)),
                )
                .child(
                    big("big-open", "welcome/open", &["Open"]).on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(workspace::Open::default()), cx)
                    }),
                )
                .child(
                    big("big-clone", "welcome/fromVCSTab", &["Clone", "Repository"])
                        .on_click(|_, window, cx| crate::dialogs::get_from_vcs(window, cx)),
                ),
        )
}

fn update_settings(
    cx: &mut App,
    update: impl FnOnce(&mut settings::SettingsContent) + 'static + Send,
) {
    settings::update_settings_file(<dyn Fs>::global(cx), cx, move |settings, _| {
        update(settings)
    });
}

fn set_theme(name: &'static str, cx: &mut App) {
    let Some(theme) = theme::ThemeRegistry::global(cx).get(name).log_err() else {
        return;
    };
    let appearance = theme.appearance;
    let system = theme::SystemAppearance::global(cx).0;
    update_settings(cx, move |settings| {
        theme_settings::set_theme(settings, name, appearance, system)
    });
}

/// Sync with OS on: the Islands themes follow the system's appearance; off: the one showing
/// stays.
fn set_sync_with_os(sync: bool, cx: &mut App) {
    let appearance = theme::ActiveTheme::theme(cx).appearance();
    let mode = match (sync, appearance.is_light()) {
        (true, _) => ThemeAppearanceMode::System,
        (false, true) => ThemeAppearanceMode::Light,
        (false, false) => ThemeAppearanceMode::Dark,
    };
    update_settings(cx, move |settings| theme_settings::set_mode(settings, mode));
}

fn set_ui_font_size(size: u32, cx: &mut App) {
    update_settings(cx, move |settings| {
        settings.theme.ui_font_size = Some(settings::FontSize(size as f32 / TEXT_PER_REM))
    });
}

fn set_base_keymap(keymap: BaseKeymap, cx: &mut App) {
    update_settings(cx, move |settings| {
        settings.base_keymap = Some(keymap.into())
    });
}

impl Render for Welcome {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let sidebar = self.sidebar(cx);
        let page = match self.page {
            Page::Projects => self.projects_page(cx),
            Page::Customize => div()
                .id("customize")
                .size_full()
                .overflow_y_scrollbar()
                .child(self.customize_page(cx))
                .into_any_element(),
            Page::Learn => self.learn_page(cx).into_any_element(),
        };
        spork_vcs::spork_look(cx).size_full().child(
            v_flex()
                .size_full()
                .track_focus(&self.focus)
                .key_context("WelcomeScreen")
                .bg(tokens.island)
                .text_color(tokens.text)
                .text_ui()
                // the menu bar's commands that make sense without a project
                // as IntelliJ's speed search: typing on the Projects page searches
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    let keystroke = &event.keystroke;
                    if this.page != Page::Projects
                        || keystroke.modifiers.platform
                        || keystroke.modifiers.control
                        || this.search.read(cx).focus_handle(cx).is_focused(window)
                    {
                        return;
                    }
                    let Some(typed) = keystroke
                        .key_char
                        .clone()
                        .filter(|typed| !typed.trim().is_empty())
                    else {
                        return;
                    };
                    this.search.update(cx, |search, cx| {
                        let text = format!("{}{typed}", search.value());
                        search.set_value(text, window, cx);
                        search.focus(window, cx);
                    });
                    cx.stop_propagation();
                }))
                .on_action(cx.listener(|this, _: &crate::CopyProjectPath, _, cx| {
                    this.copy_selected_path(cx)
                }))
                .on_action(|_: &crate::NewProject, window, cx| {
                    crate::dialogs::new_project(window, cx)
                })
                .on_action(|_: &crate::GetFromVersionControl, window, cx| {
                    crate::dialogs::get_from_vcs(window, cx)
                })
                .child(spork_workbench::shell::window_title(TITLE, cx))
                .child(
                    h_flex()
                        .flex_1()
                        .min_h_0()
                        .child(sidebar)
                        .child(div().flex_1().min_w_0().h_full().child(page)),
                ),
        )
    }
}
