mod application_menu;
pub mod collab;
mod main_toolbar;
mod onboarding_banner;

use crate::application_menu::{ApplicationMenu, show_menus};
use agent_settings::{AgentSettings, WindowLayout};
use arrayvec::ArrayVec;
use path::PathStyle;
pub use platform_title_bar::{
    self, DraggedWindowTab, MergeAllWindows, MoveTabToNewWindow, PlatformTitleBar,
    ShowNextWindowTab, ShowPreviousWindowTab,
};
use platform_title_bar::{SystemWindowTabs, TitleBarSettings};
use project::{WorktreePaths, delta_checkout_short_name, repo_identity_path};

#[cfg(not(target_os = "macos"))]
use crate::application_menu::{
    ActivateDirection, ActivateMenuLeft, ActivateMenuRight, OpenApplicationMenu,
};

use call::ActiveCall;
use client::{Client, UserStore};
use command_palette_hooks::CommandPaletteFilter;

use gpui::{
    AnyElement, AnyView, App, AppContext as _, Context, Entity, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Render, StatefulInteractiveElement, Styled, Subscription, TaskExt,
    WeakEntity, Window, actions, div,
};
use onboarding_banner::OnboardingBanner;
use project::{Project, git_store::GitStoreEvent, trusted_worktrees::TrustedWorktrees};
use remote::RemoteConnectionOptions;
use settings::{Settings as _, SettingsStore};

use std::any::TypeId;
use std::path::Path;
use std::sync::Arc;
use theme::ActiveTheme;
use ui::{
    ButtonLike, ContextMenu, IconWithIndicator, Indicator, PopoverMenu, PopoverMenuHandle,
    TintColor, Tooltip, prelude::*, utils::platform_title_bar_height,
};
use util::ResultExt;
use workspace::{AccessibleMode, MultiWorkspace, ToggleWorktreeSecurity, Workspace};

use zed_actions::OpenRemote;

pub use onboarding_banner::restore_banner;

const MAX_PROJECT_NAME_LENGTH: usize = 40;

actions!(
    collab,
    [
        /// Toggles the user menu dropdown.
        ToggleUserMenu,
        /// Toggles the project menu dropdown.
        ToggleProjectMenu
    ]
);

actions!(
    workspace,
    [
        /// Switches to the classic, editor-focused panel layout.
        UseClassicLayout,
        /// Switches to the agentic panel layout.
        UseAgenticLayout,
    ]
);

pub fn init(cx: &mut App) {
    platform_title_bar::PlatformTitleBar::init(cx);

    update_layout_action_filter(cx);

    cx.observe_global::<SettingsStore>(update_layout_action_filter)
        .detach();

    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        let Some(window) = window else {
            return;
        };
        let window_tabs = cx.new(|_| SystemWindowTabs::new());
        workspace.set_window_tabs_item(AnyView::from(window_tabs), cx);
        apply_title_bar_visibility(workspace, window, cx);

        cx.observe_global_in::<SettingsStore>(window, apply_title_bar_visibility)
            .detach();
        cx.observe_window_bounds(window, apply_title_bar_visibility)
            .detach();

        workspace.register_action(|_workspace, _: &UseClassicLayout, _window, cx| {
            set_window_layout(WindowLayout::Editor(None), cx);
        });

        workspace.register_action(|_workspace, _: &UseAgenticLayout, _window, cx| {
            set_window_layout(WindowLayout::Agent(None), cx);
        });

        #[cfg(not(target_os = "macos"))]
        workspace.register_action(|workspace, action: &OpenApplicationMenu, window, cx| {
            if let Some(titlebar) = workspace
                .titlebar_item()
                .and_then(|item| item.downcast::<TitleBar>().ok())
            {
                titlebar.update(cx, |titlebar, cx| {
                    if let Some(ref menu) = titlebar.application_menu {
                        menu.update(cx, |menu, cx| menu.open_menu(action, window, cx));
                    }
                });
            }
        });

        #[cfg(not(target_os = "macos"))]
        workspace.register_action(|workspace, _: &ActivateMenuRight, window, cx| {
            if let Some(titlebar) = workspace
                .titlebar_item()
                .and_then(|item| item.downcast::<TitleBar>().ok())
            {
                titlebar.update(cx, |titlebar, cx| {
                    if let Some(ref menu) = titlebar.application_menu {
                        menu.update(cx, |menu, cx| {
                            menu.navigate_menus_in_direction(ActivateDirection::Right, window, cx)
                        });
                    }
                });
            }
        });

        #[cfg(not(target_os = "macos"))]
        workspace.register_action(|workspace, _: &ActivateMenuLeft, window, cx| {
            if let Some(titlebar) = workspace
                .titlebar_item()
                .and_then(|item| item.downcast::<TitleBar>().ok())
            {
                titlebar.update(cx, |titlebar, cx| {
                    if let Some(ref menu) = titlebar.application_menu {
                        menu.update(cx, |menu, cx| {
                            menu.navigate_menus_in_direction(ActivateDirection::Left, window, cx)
                        });
                    }
                });
            }
        });
    })
    .detach();
}

fn apply_title_bar_visibility(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let visible = TitleBarSettings::is_title_bar_visible(window, cx);

    if !visible {
        workspace.clear_titlebar_item(window, cx);
    } else if workspace.titlebar_item().is_none() {
        let multi_workspace = workspace.multi_workspace().cloned();
        let item = cx.new(|cx| TitleBar::new("title-bar", workspace, multi_workspace, window, cx));
        workspace.set_titlebar_item(AnyView::from(item), window, cx);
    }

    #[cfg(target_os = "macos")]
    window.set_native_titlebar_visibility(visible);
}

/// Hides or shows the panel layout actions in the command palette based on
/// whether AI is currently disabled.
fn update_layout_action_filter(cx: &mut App) {
    let disable_ai = project::DisableAiSettings::get_global(cx).disable_ai;
    let layout_actions = [
        TypeId::of::<UseClassicLayout>(),
        TypeId::of::<UseAgenticLayout>(),
    ];
    CommandPaletteFilter::update_global(cx, |filter, _| {
        if disable_ai {
            filter.hide_action_types(&layout_actions);
        } else {
            filter.show_action_types(layout_actions.iter());
        }
    });
}

fn set_window_layout(layout: WindowLayout, cx: &App) {
    let fs = <dyn fs::Fs>::global(cx);
    drop(AgentSettings::set_layout(layout, fs, cx));
}

pub struct TitleBar {
    platform_titlebar: Entity<PlatformTitleBar>,
    project: Entity<Project>,
    user_store: Entity<UserStore>,
    client: Arc<Client>,
    workspace: WeakEntity<Workspace>,
    multi_workspace: Option<WeakEntity<MultiWorkspace>>,
    application_menu: Option<Entity<ApplicationMenu>>,
    run_widget: Entity<spork_run::RunWidget>,
    _subscriptions: Vec<Subscription>,
    banner: Option<Entity<OnboardingBanner>>,
    screen_share_popover_handle: PopoverMenuHandle<ContextMenu>,
    _diagnostics_subscription: Option<gpui::Subscription>,
}

impl Render for TitleBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.multi_workspace.is_none() {
            if let Some(mw) = self
                .workspace
                .upgrade()
                .and_then(|ws| ws.read(cx).multi_workspace().cloned())
            {
                self.multi_workspace = Some(mw.clone());
                self.platform_titlebar.update(cx, |titlebar, _cx| {
                    titlebar.set_multi_workspace(mw);
                });
            }
        }

        let title_bar_settings = *TitleBarSettings::get_global(cx);
        let button_layout = title_bar_settings.button_layout;

        let show_menus = show_menus(cx);

        let mut children = <ArrayVec<_, 5>>::new();

        let mut project_name = None;
        let mut project_root = None;
        if let Some(worktree) = self.effective_active_worktree(cx) {
            project_root = Some(worktree.read(cx).abs_path());
            let path_style = worktree.read(cx).path_style();
            let repository = self.get_repository_for_worktree(&worktree, path_style, cx);
            let worktree_abs_path = worktree.read(cx).abs_path();
            project_name = worktree
                .read(cx)
                .root_name()
                .file_name()
                .map(SharedString::from);
            if let Some(repo) = &repository {
                let repo = repo.read(cx);
                let worktree_paths = self.project.read(cx).worktree_paths(cx);
                let delta_checkout =
                    delta_checkout_identity(&worktree_paths, &worktree_abs_path, path_style);
                let identity = match &delta_checkout {
                    Some((source_path, _)) => source_path,
                    None => repo_identity_path(&repo.common_dir_abs_path, path_style),
                };
                let display_name =
                    if path_style.extension(identity) == Some(std::ffi::OsStr::new("git")) {
                        path_style.file_stem(identity)
                    } else {
                        path_style.file_name(identity)
                    };

                if let Some(repo_name) = display_name.and_then(|n| n.to_str()) {
                    let visible_worktrees_in_repo =
                        self.visible_worktrees_in_repository(repo, path_style, cx);
                    let name = if visible_worktrees_in_repo == 1 {
                        if let Ok(relative) = path_style
                            .strip_prefix(&worktree_abs_path, &*repo.work_directory_abs_path)
                        {
                            if relative.as_os_str().is_empty() {
                                repo_name.to_string()
                            } else {
                                format!(
                                    "{}{}{}",
                                    repo_name,
                                    path_style.primary_separator(),
                                    relative.display()
                                )
                            }
                        } else {
                            repo_name.to_string()
                        }
                    } else {
                        repo_name.to_string()
                    };
                    project_name = Some(SharedString::from(name));
                }
            }
        }

        // Spork IDE: Spork's main toolbar: the project and VCS widgets side by side, the toolbar
        // faded in a background window
        let window_active = window.is_window_active();
        children.push(
            h_flex()
                .h_full()
                .gap_1()
                .when(!window_active, |toolbar| {
                    toolbar.opacity(spork_workbench::shell::INACTIVE_ALPHA)
                })
                .map(|title_bar| {
                    let mut render_project_items = title_bar_settings.show_project_items;
                    title_bar
                        .when_some(
                            self.application_menu.clone().filter(|_| !show_menus),
                            |title_bar, menu| {
                                // Hide the project/branch items to make room when the
                                // menu bar is expanded -- except in accessible mode,
                                // where the menu bar is always expanded but those
                                // controls must still remain reachable.
                                render_project_items &= !menu
                                    .update(cx, |menu, cx| menu.all_menus_shown(cx))
                                    || cx.accessible_mode();
                                title_bar.child(menu)
                            },
                        )
                        .children(self.render_restricted_mode(cx))
                        .when(render_project_items, |title_bar| {
                            let vcs_widget = self
                                .workspace
                                .upgrade()
                                .and_then(|workspace| spork_vcs::vcs_widget(&workspace, cx));
                            title_bar
                                .children(self.render_project_host(cx))
                                .child(self.render_project_name(
                                    project_name,
                                    project_root.as_deref(),
                                    window,
                                    cx,
                                ))
                                .children(vcs_widget)
                        })
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .into_any_element(),
        );

        children.push(self.render_collaborator_list(window, cx).into_any_element());

        if title_bar_settings.show_onboarding_banner {
            if let Some(banner) = &self.banner {
                children.push(banner.clone().into_any_element())
            }
        }

        let status = self.client.status();
        let status = &*status.borrow();

        children.push(
            h_flex()
                .pr(px(spork_workbench::tokens::metrics::MAIN_TOOLBAR_END_INSET))
                .gap(px(spork_workbench::tokens::metrics::MAIN_TOOLBAR_GAP))
                .when(!window_active, |toolbar| {
                    toolbar.opacity(spork_workbench::shell::INACTIVE_ALPHA)
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(self.run_widget.clone())
                .child(self.render_call_controls(window, cx))
                .children(self.render_connection_status(status))
                .child(
                    spork_vcs::spork_look(cx)
                        .flex()
                        .gap(px(spork_workbench::tokens::metrics::MAIN_TOOLBAR_GAP))
                        .children(main_toolbar::search_and_settings()),
                )
                .into_any_element(),
        );

        if show_menus {
            self.platform_titlebar.update(cx, |this, _| {
                this.set_button_layout(button_layout);
                this.set_children(
                    self.application_menu
                        .clone()
                        .map(|menu| menu.into_any_element()),
                );
            });

            let height = platform_title_bar_height(window);
            let title_bar_color = self.platform_titlebar.update(cx, |platform_titlebar, cx| {
                platform_titlebar.title_bar_color(window, cx)
            });

            v_flex()
                .w_full()
                .child(self.platform_titlebar.clone().into_any_element())
                .child(
                    h_flex()
                        .bg(title_bar_color)
                        .h(height)
                        .pl_2()
                        .justify_between()
                        .w_full()
                        .children(children),
                )
                .into_any_element()
        } else {
            self.platform_titlebar.update(cx, |this, _| {
                this.set_button_layout(button_layout);
                this.set_children(children);
            });
            self.platform_titlebar.clone().into_any_element()
        }
    }
}

impl TitleBar {
    pub fn new(
        id: impl Into<ElementId>,
        workspace: &Workspace,
        multi_workspace: Option<WeakEntity<MultiWorkspace>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let project = workspace.project().clone();
        let git_store = project.read(cx).git_store().clone();
        let user_store = workspace.app_state().user_store.clone();
        let client = workspace.app_state().client.clone();
        let active_call = ActiveCall::global(cx);

        let platform_style = PlatformStyle::platform();
        let application_menu = match platform_style {
            PlatformStyle::Mac => {
                if option_env!("ZED_USE_CROSS_PLATFORM_MENU").is_some() {
                    Some(cx.new(|cx| ApplicationMenu::new(window, cx)))
                } else {
                    None
                }
            }
            PlatformStyle::Linux | PlatformStyle::Windows => {
                Some(cx.new(|cx| ApplicationMenu::new(window, cx)))
            }
        };

        let mut subscriptions = Vec::new();
        subscriptions.push(
            cx.observe(&workspace.weak_handle().upgrade().unwrap(), |_, _, cx| {
                cx.notify()
            }),
        );

        subscriptions.push(cx.observe(&active_call, |this, _, cx| this.active_call_changed(cx)));
        subscriptions.push(
            cx.subscribe(&git_store, move |_, _, event, cx| match event {
                GitStoreEvent::ActiveRepositoryChanged(_)
                | GitStoreEvent::RepositoryUpdated(_, _, true) => {
                    cx.notify();
                }
                _ => {}
            }),
        );
        subscriptions.push(cx.observe(&user_store, |_a, _, cx| cx.notify()));
        if let Some(workspace_entity) = workspace.weak_handle().upgrade() {
            subscriptions.push(cx.subscribe(
                &workspace_entity,
                |_, _, event: &workspace::Event, cx| {
                    if matches!(event, workspace::Event::WorktreeCreationChanged) {
                        cx.notify();
                    }
                },
            ));
        }
        subscriptions.push(cx.observe_button_layout_changed(window, |_, _, cx| cx.notify()));
        if let Some(trusted_worktrees) = TrustedWorktrees::try_get_global(cx) {
            subscriptions.push(cx.subscribe(&trusted_worktrees, |_, _, _, cx| {
                cx.notify();
            }));
        }

        let platform_titlebar = cx.new(|cx| {
            // Spork IDE: on the workspace's frame, under the project's glow
            let mut titlebar = PlatformTitleBar::new(id, cx)
                .without_system_window_tabs()
                .transparent();
            if let Some(mw) = multi_workspace.clone() {
                titlebar = titlebar.with_multi_workspace(mw);
            }
            titlebar
        });

        let banner = None;
        let run_widget = cx.new(|cx| spork_run::RunWidget::new(workspace, cx));

        let mut this = Self {
            platform_titlebar,
            application_menu,
            run_widget,
            workspace: workspace.weak_handle(),
            multi_workspace,
            project,
            user_store,
            client,
            _subscriptions: subscriptions,
            banner,
            screen_share_popover_handle: PopoverMenuHandle::default(),
            _diagnostics_subscription: None,
        };

        this.observe_diagnostics(cx);

        this
    }

    /// Returns the worktree to display in the title bar.
    /// - Prefer the worktree owning the project's active repository
    /// - Fall back to the first visible worktree
    pub fn effective_active_worktree(&self, cx: &App) -> Option<Entity<project::Worktree>> {
        let project = self.project.read(cx);

        if let Some(repo) = project.active_repository(cx) {
            let repo = repo.read(cx);
            let repo_path = &repo.work_directory_abs_path;

            for worktree in project.visible_worktrees(cx) {
                let worktree_path = worktree.read(cx).abs_path();
                if worktree_path == *repo_path || worktree_path.starts_with(repo_path.as_ref()) {
                    return Some(worktree);
                }
            }
        }

        project.visible_worktrees(cx).next()
    }

    fn get_repository_for_worktree(
        &self,
        worktree: &Entity<project::Worktree>,
        path_style: PathStyle,
        cx: &App,
    ) -> Option<Entity<project::git_store::Repository>> {
        let project = self.project.read(cx);
        let git_store = project.git_store().read(cx);
        let worktree_path = worktree.read(cx).abs_path();

        git_store
            .repositories()
            .values()
            .filter(|repo| {
                let repo_path = &repo.read(cx).work_directory_abs_path;
                worktree_path == *repo_path
                    || path_style.starts_with(&worktree_path, repo_path.as_ref())
            })
            .max_by_key(|repo| repo.read(cx).work_directory_abs_path.as_os_str().len())
            .cloned()
    }

    fn visible_worktrees_in_repository(
        &self,
        repository: &project::git_store::Repository,
        path_style: PathStyle,
        cx: &App,
    ) -> usize {
        let repo_path = &repository.work_directory_abs_path;
        self.project
            .read(cx)
            .visible_worktrees(cx)
            .filter(|worktree| {
                let worktree_path = worktree.read(cx).abs_path();
                worktree_path == *repo_path
                    || path_style.starts_with(&worktree_path, repo_path.as_ref())
            })
            .count()
    }

    fn render_remote_project_connection(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let workspace = self.workspace.clone();

        let options = self.project.read(cx).remote_connection_options(cx)?;
        let host: SharedString = options.display_name().into();

        let (tooltip_title, icon) = match options {
            RemoteConnectionOptions::Ssh(_) => ("Remote Project", IconName::Server),
            RemoteConnectionOptions::Wsl(_) => ("Remote Project", IconName::Linux),
            RemoteConnectionOptions::Docker(_dev_container_connection) => {
                ("Dev Container", IconName::Box)
            }
            #[cfg(any(test, feature = "test-support"))]
            RemoteConnectionOptions::Mock(_) => ("Mock Remote Project", IconName::Server),
        };

        let (indicator_color, meta) = match self.project.read(cx).remote_connection_state(cx)? {
            remote::ConnectionState::Connecting => (Color::Info, format!("Connecting to: {host}")),
            remote::ConnectionState::Connected => (Color::Success, format!("Connected to: {host}")),
            remote::ConnectionState::HeartbeatMissed => (
                Color::Warning,
                format!("Connection attempt to {host} missed. Retrying..."),
            ),
            remote::ConnectionState::Reconnecting => (
                Color::Warning,
                format!("Lost connection to {host}. Reconnecting..."),
            ),
            remote::ConnectionState::Disconnected => {
                (Color::Error, format!("Disconnected from {host}"))
            }
        };

        let icon_color = match self.project.read(cx).remote_connection_state(cx)? {
            remote::ConnectionState::Connecting => Color::Info,
            remote::ConnectionState::Connected => Color::Default,
            remote::ConnectionState::HeartbeatMissed => Color::Warning,
            remote::ConnectionState::Reconnecting => Color::Warning,
            remote::ConnectionState::Disconnected => Color::Error,
        };

        let meta = SharedString::from(meta);

        Some(
            PopoverMenu::new("remote-project-menu")
                .menu(move |window, cx| {
                    let workspace_entity = workspace.upgrade()?;
                    let fs = workspace_entity.read(cx).project().read(cx).fs().clone();
                    Some(recent_projects::RemoteServerProjects::popover(
                        fs,
                        workspace.clone(),
                        None,
                        window,
                        cx,
                    ))
                })
                .trigger_with_tooltip(
                    ButtonLike::new("remote_project")
                        .selected_style(ButtonStyle::Tinted(TintColor::Accent))
                        .child(
                            h_flex()
                                .gap_2()
                                .max_w_32()
                                .child(
                                    IconWithIndicator::new(
                                        Icon::new(icon).size(IconSize::Small).color(icon_color),
                                        Some(Indicator::dot().color(indicator_color)),
                                    )
                                    .indicator_border_color(Some(
                                        cx.theme().colors().title_bar_background,
                                    ))
                                    .into_any_element(),
                                )
                                .child(Label::new(host).size(LabelSize::Small).truncate()),
                        ),
                    move |_window, cx| {
                        Tooltip::with_meta(
                            tooltip_title,
                            Some(&OpenRemote::default()),
                            meta.clone(),
                            cx,
                        )
                    },
                )
                .anchor(gpui::Anchor::TopLeft)
                .into_any_element(),
        )
    }

    pub fn render_restricted_mode(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let has_restricted_worktrees =
            TrustedWorktrees::has_restricted_worktrees(&self.project.read(cx).worktree_store(), cx);
        if !has_restricted_worktrees {
            return None;
        }

        let button = Button::new("restricted_mode_trigger", "Restricted Mode")
            .style(ButtonStyle::Tinted(TintColor::Warning))
            .label_size(LabelSize::Small)
            .color(Color::Warning)
            .start_icon(
                Icon::new(IconName::Warning)
                    .size(IconSize::Small)
                    .color(Color::Warning),
            )
            .tooltip(|_, cx| {
                Tooltip::with_meta(
                    "You're in Restricted Mode",
                    Some(&ToggleWorktreeSecurity),
                    "Mark this project as trusted and unlock all features",
                    cx,
                )
            })
            .on_click({
                cx.listener(move |this, _, window, cx| {
                    this.workspace
                        .update(cx, |workspace, cx| {
                            workspace.show_worktree_trust_security_modal(true, window, cx)
                        })
                        .log_err();
                })
            });

        if ui::utils::MACOS_SDK_26_OR_LATER {
            // Make up for Tahoe's traffic light buttons having less spacing around them
            Some(div().child(button).ml_0p5().into_any_element())
        } else {
            Some(button.into_any_element())
        }
    }

    pub fn render_project_host(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.project.read(cx).is_via_remote_server() {
            return self.render_remote_project_connection(cx);
        }

        if self.project.read(cx).is_disconnected(cx) {
            return Some(
                Button::new("disconnected", "Disconnected")
                    .disabled(true)
                    .color(Color::Disabled)
                    .label_size(LabelSize::Small)
                    .into_any_element(),
            );
        }

        let host = self.project.read(cx).host()?;
        let host_user = self.user_store.read(cx).get_cached_user(host.user_id)?;
        let participant_index = self
            .user_store
            .read(cx)
            .participant_indices()
            .get(&host_user.legacy_id)?;

        Some(
            Button::new("project_owner_trigger", host_user.username.clone())
                .color(Color::Player(participant_index.0))
                .label_size(LabelSize::Small)
                .tab_index(0isize)
                .tooltip(move |_, cx| {
                    let tooltip_title = format!(
                        "{} is sharing this project. Click to follow.",
                        host_user.username
                    );

                    Tooltip::with_meta(tooltip_title, None, "Click to Follow", cx)
                })
                .on_click({
                    let host_peer_id = host.peer_id;
                    cx.listener(move |this, _, window, cx| {
                        this.workspace
                            .update(cx, |workspace, cx| {
                                workspace.follow(host_peer_id, window, cx);
                            })
                            .log_err();
                    })
                })
                .into_any_element(),
        )
    }

    fn render_project_name(
        &self,
        name: Option<SharedString>,
        root: Option<&Path>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> impl IntoElement {
        let display_name = if let Some(ref name) = name {
            util::truncate_and_trailoff(name, MAX_PROJECT_NAME_LENGTH)
        } else {
            "Open Recent Project".to_string()
        };

        // Spork IDE: IntelliJ's Recent Projects popup
        PopoverMenu::new("recent-projects-menu")
            .menu(|window, cx| Some(cx.new(|cx| spork_projects::ProjectPopup::new(window, cx))))
            .trigger(main_toolbar::ProjectWidget::new(display_name.into(), root))
            .anchor(gpui::Anchor::TopLeft)
            .into_any_element()
    }

    fn active_call_changed(&mut self, cx: &mut Context<Self>) {
        self.observe_diagnostics(cx);
        cx.notify();
    }

    fn observe_diagnostics(&mut self, cx: &mut Context<Self>) {
        let diagnostics = ActiveCall::global(cx)
            .read(cx)
            .room()
            .and_then(|room| room.read(cx).diagnostics().cloned());

        if let Some(diagnostics) = diagnostics {
            self._diagnostics_subscription = Some(cx.observe(&diagnostics, |_, _, cx| cx.notify()));
        } else {
            self._diagnostics_subscription = None;
        }
    }

    fn share_project(&mut self, cx: &mut Context<Self>) {
        let active_call = ActiveCall::global(cx);
        let project = self.project.clone();
        active_call
            .update(cx, |call, cx| call.share_project(project, cx))
            .detach_and_log_err(cx);
    }

    fn unshare_project(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let active_call = ActiveCall::global(cx);
        let project = self.project.clone();
        active_call
            .update(cx, |call, cx| call.unshare_project(project, cx))
            .log_err();
    }

    fn render_connection_status(&self, status: &client::Status) -> Option<AnyElement> {
        match status {
            client::Status::ConnectionError
            | client::Status::ConnectionLost
            | client::Status::Reauthenticating
            | client::Status::Reconnecting
            | client::Status::ReconnectionError { .. } => Some(
                div()
                    .id("disconnected")
                    .child(Icon::new(IconName::Disconnected).size(IconSize::Small))
                    .tooltip(Tooltip::text("Disconnected"))
                    .into_any_element(),
            ),
            client::Status::UpgradeRequired => Some(
                Label::new("Please update Spork IDE to Collaborate")
                    .size(LabelSize::Small)
                    .into_any_element(),
            ),
            _ => None,
        }
    }
}

fn delta_checkout_identity<'a>(
    worktree_paths: &'a WorktreePaths,
    folder_path: &Path,
    path_style: PathStyle,
) -> Option<(&'a Path, SharedString)> {
    let clone = worktree_paths.delta_clone(folder_path)?;
    let worktree_name = delta_checkout_short_name(clone, folder_path, path_style)?;
    let (source_path, _) = worktree_paths
        .ordered_pairs()
        .find(|(_, folder)| folder.as_path() == folder_path)?;
    Some((source_path, worktree_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs::Fs as _;
    use gpui::{
        AnyWindowHandle, Empty, EntityId, SystemWindowTab, SystemWindowTabController,
        TestAppContext, VisualTestContext,
    };
    use platform_title_bar::SidebarWindowControls;
    use settings::{SidebarSide, TitleBarVisibility};

    #[gpui::test]
    async fn test_title_bar_visibility(cx: &mut TestAppContext) {
        let (workspace, cx) = init_test(TitleBarVisibility::Never, cx).await;
        assert_eq!(chrome(&workspace, cx), Chrome::HIDDEN);

        set_visibility(Some(TitleBarVisibility::Always), cx);
        assert_eq!(chrome(&workspace, cx), Chrome::SHOWN);
        let title_bar = title_bar_id(&workspace, cx);

        cx.update_global::<SettingsStore, _>(|store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.title_bar.get_or_insert_default().show_menus = Some(true);
            });
        });
        cx.run_until_parked();
        assert_eq!(title_bar_id(&workspace, cx), title_bar);

        toggle_fullscreen(&workspace, cx);
        assert_eq!(
            chrome(&workspace, cx),
            Chrome {
                title_bar: true,
                ..Chrome::HIDDEN
            }
        );

        set_visibility(Some(TitleBarVisibility::HideInFullScreen), cx);
        assert_eq!(chrome(&workspace, cx), Chrome::HIDDEN);

        toggle_fullscreen(&workspace, cx);
        assert_eq!(chrome(&workspace, cx), Chrome::SHOWN);

        toggle_fullscreen(&workspace, cx);
        assert_eq!(chrome(&workspace, cx), Chrome::HIDDEN);

        toggle_fullscreen(&workspace, cx);
        set_visibility(Some(TitleBarVisibility::Never), cx);
        assert_eq!(chrome(&workspace, cx), Chrome::HIDDEN);

        set_visibility(None, cx);
        assert_eq!(chrome(&workspace, cx), Chrome::SHOWN);
    }

    #[gpui::test]
    async fn test_window_tabs_stay_mounted_without_title_bar(cx: &mut TestAppContext) {
        let other_window = cx.add_window(|_, _| Empty);
        let (workspace, cx) = init_test(TitleBarVisibility::Never, cx).await;
        workspace.update_in(cx, |_, window, cx| {
            let workspace_window = window.window_handle();
            SystemWindowTabController::add_tab(
                cx,
                workspace_window.window_id(),
                Vec::from([
                    SystemWindowTab::new(SharedString::from("workspace"), workspace_window),
                    SystemWindowTab::new(
                        SharedString::from("other"),
                        AnyWindowHandle::from(other_window),
                    ),
                ]),
            );
            SystemWindowTabController::set_visible(cx, true);
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(chrome(&workspace, cx), Chrome::HIDDEN);
        assert!(window_tabs_mounted(cx));

        set_visibility(Some(TitleBarVisibility::HideInFullScreen), cx);
        assert_eq!(chrome(&workspace, cx), Chrome::SHOWN);
        assert!(window_tabs_mounted(cx));

        toggle_fullscreen(&workspace, cx);
        assert_eq!(chrome(&workspace, cx), Chrome::HIDDEN);
        assert!(window_tabs_mounted(cx));
    }

    #[derive(Debug, PartialEq)]
    struct Chrome {
        title_bar: bool,
        left_sidebar: SidebarWindowControls,
        right_sidebar: SidebarWindowControls,
    }

    impl Chrome {
        const SHOWN: Self = Self {
            title_bar: true,
            left_sidebar: SidebarWindowControls {
                traffic_lights: cfg!(target_os = "macos"),
                left: !cfg!(target_os = "macos"),
                right: false,
            },
            right_sidebar: SidebarWindowControls {
                traffic_lights: false,
                left: false,
                right: !cfg!(target_os = "macos"),
            },
        };
        const HIDDEN: Self = Self {
            title_bar: false,
            left_sidebar: NO_WINDOW_CONTROLS,
            right_sidebar: NO_WINDOW_CONTROLS,
        };
    }

    const NO_WINDOW_CONTROLS: SidebarWindowControls = SidebarWindowControls {
        traffic_lights: false,
        left: false,
        right: false,
    };

    #[gpui::test]
    async fn test_delta_checkout_identity(cx: &mut TestAppContext) {
        let (_workspace, cx) = init_test(TitleBarVisibility::Always, cx).await;
        let fs = fs::FakeFs::new(cx.executor());
        let source = Path::new(util::path!("/repo"));
        let clone = Path::new(util::path!("/store/clones/thread-alpha/repo.git"));
        let checkout = Path::new(util::path!("/store/worktrees/thread-alpha/repo"));
        let source_objects = source.join(".git").join("objects");
        for directory in [
            source_objects.clone(),
            clone.join("delta"),
            clone.join("objects").join("info"),
            checkout.to_path_buf(),
        ] {
            fs.create_dir(&directory).await.unwrap();
        }
        for (file, content) in [
            (
                source.join(".git").join("HEAD"),
                "ref: refs/heads/main\n".to_string(),
            ),
            (clone.join("HEAD"), "ref: refs/heads/feature\n".to_string()),
            (clone.join("delta").join("copy-baseline"), String::new()),
            (
                clone.join("objects").join("info").join("alternates"),
                format!("{}\n", source_objects.display()),
            ),
            (
                checkout.join(".git"),
                format!("gitdir: {}\n", clone.display()),
            ),
        ] {
            fs.insert_file(file, content.into_bytes()).await;
        }
        let project = Project::test(fs, [source, checkout], cx).await;
        project
            .update(cx, |project, cx| project.git_scans_complete(cx))
            .await;

        let identities = project.read_with(cx, |project, cx| {
            let worktree_paths = project.worktree_paths(cx);
            [source, checkout].map(|folder_path| {
                delta_checkout_identity(&worktree_paths, folder_path, PathStyle::local())
                    .map(|(source_path, worktree_name)| (source_path.to_path_buf(), worktree_name))
            })
        });
        assert_eq!(
            identities,
            [
                None,
                Some((source.to_path_buf(), SharedString::from("thread-alpha")))
            ]
        );
    }

    async fn init_test(
        visibility: TitleBarVisibility,
        cx: &mut TestAppContext,
    ) -> (Entity<Workspace>, &mut VisualTestContext) {
        cx.update(|cx| {
            let mut settings = SettingsStore::test(cx);
            settings.update_user_settings(cx, |settings| {
                settings.title_bar.get_or_insert_default().show = Some(visibility);
            });
            cx.set_global(settings);
            cx.set_global(db::AppDatabase::test_new());
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            command_palette_hooks::init(cx);
            // the main toolbar's controls are Spork's
            gpui_kit::init(cx);
            spork_workbench::install(cx);
        });
        let project = Project::test(fs::FakeFs::new(cx.executor()), [], cx).await;
        cx.update(|cx| {
            let project = project.read(cx);
            call::init(project.client(), project.user_store(), cx);
            init(cx);
        });
        let (workspace, cx) =
            cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
        cx.run_until_parked();
        (workspace, cx)
    }

    fn set_visibility(visibility: Option<TitleBarVisibility>, cx: &mut VisualTestContext) {
        cx.update_global::<SettingsStore, _>(|store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.title_bar.get_or_insert_default().show = visibility;
            });
        });
        cx.run_until_parked();
    }

    fn toggle_fullscreen(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) {
        workspace.update_in(cx, |_, window, _| window.toggle_fullscreen());
        cx.run_until_parked();
    }

    fn chrome(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Chrome {
        workspace.update_in(cx, |workspace, window, cx| Chrome {
            title_bar: workspace.titlebar_item().is_some(),
            left_sidebar: SidebarWindowControls::new(SidebarSide::Left, window, cx),
            right_sidebar: SidebarWindowControls::new(SidebarSide::Right, window, cx),
        })
    }

    fn title_bar_id(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Option<EntityId> {
        workspace.read_with(cx, |workspace, _| {
            workspace.titlebar_item().map(|item| item.entity_id())
        })
    }

    fn window_tabs_mounted(cx: &mut VisualTestContext) -> bool {
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        cx.debug_bounds("system-window-tabs")
            .is_some_and(|bounds| bounds.size.height > px(0.))
    }
}
