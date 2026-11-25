//! The Plugins page, as IntelliJ's (`PluginManagerConfigurable`): the Marketplace and Installed
//! tabs, a search over the list of plugin cards (logo, name, downloads, vendor, Install), and the
//! details of the one selected (Install or Uninstall, Overview, Additional Info). The plugins are
//! the IDE's extensions, from the extension registry the IDE keeps using (ROADMAP, Decision 12);
//! installing and uninstalling happen at once, as IntelliJ downloads at once. Extensions can't
//! be disabled, so neither the Installed cards' check box nor Disable shows. A plugin's theme
//! (and icon theme) is applied as it is installed and left as it is uninstalled, as IntelliJ's
//! (`LafDynamicPluginManager`); a newer version in the registry shows Update.

use std::collections::HashMap;
use std::sync::Arc;

use cloud_api_types::ExtensionMetadata;
use extension_host::{ExtensionStatus, ExtensionStore};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{self, DropdownMenu as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Value, json};
use settings::Settings as _;
use spork_workbench::{Tokens, Typography as _};
use theme::{ActiveTheme as _, Appearance, GlobalTheme, ThemeRegistry};
use theme_settings::ThemeSettings;
use util::ResultExt as _;

use crate::text_label;

/// Words the search finds the Plugins page by.
pub(crate) const WORDS: &str = "plugins marketplace installed extensions install uninstall";

gpui::actions!(
    settings_dialog,
    [
        /// Looks for plugins supporting the project's files that aren't installed (IntelliJ's
        /// Check Suggested Plugins).
        CheckSuggestedPlugins
    ]
);

/// The plugins Zed suggests for the project's files (`extension_suggest`, by file name or
/// extension) that aren't installed and whose files no language knows, by id.
fn suggested_plugins(workspace: &workspace::Workspace, cx: &App) -> Vec<Arc<str>> {
    let project = workspace.project().read(cx);
    let languages = project.languages().clone();
    let installed = ExtensionStore::try_global(cx)
        .map(|store| {
            store
                .read(cx)
                .installed_extensions()
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut found: Vec<Arc<str>> = Vec::new();
    for worktree in project.visible_worktrees(cx) {
        let snapshot = worktree.read(cx).snapshot();
        for entry in snapshot.files(false, 0) {
            let Some(suggestion) = extension_suggest::suggest_extension(&entry.path) else {
                continue;
            };
            let known = languages
                .language_for_file_path(entry.path.as_std_path())
                .is_some();
            if !known
                && !installed.contains(&suggestion.extension_id)
                && !found.contains(&suggestion.extension_id)
            {
                found.push(suggestion.extension_id);
            }
        }
    }
    found
}

/// IntelliJ's Check Suggested Plugins: a notification naming the plugins to install, with
/// Configure plugins… (the Plugins page searching them), or "No suggested plugins found".
/// Counts the installed plugins the registry has another version of, for the count IntelliJ
/// shows on Plugins' row of the tree. Spork IDE installs them at startup unless the user turned
/// that off (`auto_update_extensions`), so it is mostly none.
pub(crate) fn count_updates(cx: &mut Context<crate::SettingsDialog>) {
    let Some(store) = ExtensionStore::try_global(cx) else {
        return;
    };
    let latest = store.update(cx, |store, cx| {
        store.fetch_extensions_with_update_available(cx)
    });
    cx.spawn(async move |dialog, cx| {
        let latest = latest.await?;
        dialog.update(cx, |dialog, cx| {
            let installed = ExtensionStore::global(cx).read(cx).installed_extensions();
            // the registry answers with each one's latest version
            dialog.plugin_updates = latest
                .iter()
                .filter(|extension| {
                    installed
                        .get(&extension.id)
                        .is_some_and(|entry| entry.manifest.version != extension.manifest.version)
                })
                .count();
            cx.notify();
        })
    })
    .detach_and_log_err(cx);
}

/// IntelliJ applies a theme plugin's theme as it is installed (not as it is updated, and not
/// while the theme follows the OS), and an icon pack as it is enabled: a plugin installed here,
/// from the Marketplace or from disk, gets its theme and icon theme applied the same way. Of a
/// plugin's themes, the first of the current theme's appearance, as a plugin's Dark and Light
/// come together.
pub(crate) fn apply_themes_of_installed_plugins(cx: &mut App) {
    let Some(store) = ExtensionStore::try_global(cx) else {
        return;
    };
    cx.subscribe(&store, |store, event, cx| {
        let extension_host::Event::ExtensionInstalled(id) = event else {
            return;
        };
        let registry = ThemeRegistry::global(cx);
        let current = cx.theme().appearance();
        let themes: Vec<_> = store
            .read(cx)
            .extension_themes(id)
            .filter_map(|name| registry.get(name).ok())
            .collect();
        let theme = themes
            .iter()
            .find(|theme| theme.appearance == current)
            .or(themes.first())
            .map(|theme| (theme.name.clone(), theme.appearance));
        let icon_theme = store.read(cx).extension_icon_themes(id).next().cloned();
        let follows_os = matches!(
            ThemeSettings::get_global(cx).theme,
            theme_settings::ThemeSelection::Dynamic {
                mode: settings::ThemeAppearanceMode::System,
                ..
            }
        );
        let theme = theme.filter(|_| !follows_os);
        if theme.is_none() && icon_theme.is_none() {
            return;
        }
        let system = theme::SystemAppearance::global(cx).0;
        settings::update_settings_file(<dyn fs::Fs>::global(cx), cx, move |content, _| {
            if let Some((name, appearance)) = theme {
                theme_settings::set_theme(content, name, appearance, system);
            }
            if let Some(icon_theme) = icon_theme {
                theme_settings::set_icon_theme(
                    content,
                    settings::IconThemeName(icon_theme),
                    system,
                );
            }
        });
    })
    .detach();
}

/// What uninstalling a plugin leaves in place of its theme and icon theme when they are the
/// current ones: IntelliJ's default theme of the same appearance, the default icon theme.
fn defaults_after_uninstall(id: &str, cx: &App) -> (Option<(&'static str, Appearance)>, bool) {
    let Some(store) = ExtensionStore::try_global(cx) else {
        return (None, false);
    };
    let store = store.read(cx);
    let theme = cx.theme();
    let theme = store
        .extension_themes(id)
        .any(|name| name.as_ref() == theme.name.as_ref())
        .then(|| match theme.appearance {
            Appearance::Dark => (settings::DEFAULT_DARK_THEME, Appearance::Dark),
            Appearance::Light => (settings::DEFAULT_LIGHT_THEME, Appearance::Light),
        });
    let icon_theme = GlobalTheme::icon_theme(cx).name.clone();
    let icon_theme = store
        .extension_icon_themes(id)
        .any(|name| name.as_ref() == icon_theme.as_ref());
    (theme, icon_theme)
}

pub(crate) fn register(workspace: &mut workspace::Workspace) {
    workspace.register_action(|workspace, _: &CheckSuggestedPlugins, _, cx| {
        struct SuggestedPlugins;
        let found = suggested_plugins(workspace, cx);
        let id = workspace::notifications::NotificationId::unique::<SuggestedPlugins>();
        workspace.show_notification(id, cx, move |cx| {
            use workspace::notifications::simple_message_notification::MessageNotification;
            cx.new(move |cx| match found.as_slice() {
                [] => MessageNotification::new("No suggested plugins found", cx),
                names => {
                    let (plugin, names_text) = match names {
                        [one] => ("plugin", one.to_string()),
                        many => (
                            "plugins",
                            many.iter()
                                .map(|name| name.as_ref())
                                .collect::<Vec<_>>()
                                .join(", "),
                        ),
                    };
                    // the Marketplace's search finds one plugin at a time: the first
                    let query = names
                        .first()
                        .map(|name| name.to_string())
                        .unwrap_or_default();
                    MessageNotification::new(
                        format!("Suggested {plugin} {names_text} available."),
                        cx,
                    )
                    .primary_message("Configure plugins…")
                    .primary_on_click(move |window, cx| {
                        window.dispatch_action(
                            Box::new(zed_actions::Extensions {
                                id: Some(query.clone()),
                                ..Default::default()
                            }),
                            cx,
                        )
                    })
                }
            })
        });
    });
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Marketplace,
    Installed,
}

/// A plugin as a card shows it.
#[derive(Clone)]
struct Plugin {
    id: Arc<str>,
    name: SharedString,
    version: SharedString,
    description: Option<SharedString>,
    vendor: SharedString,
    downloads: Option<u64>,
    repository: Option<SharedString>,
}

impl Plugin {
    fn from_metadata(metadata: &ExtensionMetadata) -> Self {
        Plugin {
            id: metadata.id.clone(),
            name: metadata.manifest.name.clone().into(),
            version: metadata.manifest.version.to_string().into(),
            description: metadata.manifest.description.clone().map(Into::into),
            vendor: metadata.manifest.authors.join(", ").into(),
            downloads: Some(metadata.download_count),
            repository: Some(metadata.manifest.repository.clone().into()),
        }
    }
}

/// The Marketplace's list: loading, loaded, or why it couldn't be.
enum Marketplace {
    Loading,
    Loaded(Vec<Plugin>),
    Failed(SharedString),
}

pub(crate) struct PluginsPage {
    tab: Tab,
    search: Entity<InputState>,
    /// None until the page first shows.
    marketplace: Option<Marketplace>,
    selected: Option<Arc<str>>,
    /// The registry's latest version of each installed plugin it has another version of.
    updates: HashMap<Arc<str>, Arc<str>>,
    _fetch: Task<()>,
    _fetch_updates: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl PluginsPage {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx));
        let mut _subscriptions = vec![cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change | InputEvent::PressEnter { .. }) {
                this.fetch(cx);
            }
        })];
        if let Some(store) = ExtensionStore::try_global(cx) {
            _subscriptions.push(cx.observe(&store, |_, _, cx| cx.notify()));
        }
        Self {
            tab: Tab::Marketplace,
            search,
            marketplace: None,
            selected: None,
            updates: HashMap::default(),
            _fetch: Task::ready(()),
            _fetch_updates: Task::ready(()),
            _subscriptions,
        }
    }

    /// The page shows: the Marketplace loads the first time.
    pub(crate) fn shown(&mut self, cx: &mut Context<Self>) {
        if self.marketplace.is_none() {
            self.fetch(cx);
        }
    }

    /// Shows the Marketplace searching for `query` (a plugin's id, from an "install this" link).
    pub(crate) fn search_marketplace(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tab = Tab::Marketplace;
        self.search.update(cx, |search, cx| {
            search.set_value(query.to_owned(), window, cx)
        });
        self.fetch(cx);
    }

    fn query(&self, cx: &App) -> String {
        self.search.read(cx).value().trim().to_owned()
    }

    fn fetch(&mut self, cx: &mut Context<Self>) {
        self.fetch_updates(cx);
        if self.tab != Tab::Marketplace {
            cx.notify();
            return;
        }
        let Some(store) = ExtensionStore::try_global(cx) else {
            return;
        };
        let query = self.query(cx);
        self.marketplace = Some(Marketplace::Loading);
        let fetch = store.update(cx, |store, cx| {
            store.fetch_extensions((!query.is_empty()).then_some(query.as_str()), None, cx)
        });
        self._fetch = cx.spawn(async move |this, cx| {
            let result = fetch.await;
            this.update(cx, |this, cx| {
                this.marketplace = Some(match result {
                    Ok(found) => {
                        Marketplace::Loaded(found.iter().map(Plugin::from_metadata).collect())
                    }
                    Err(error) => {
                        log::error!("Marketplace plugins are not loaded: {error:#}");
                        Marketplace::Failed("Marketplace plugins are not loaded.".into())
                    }
                });
                cx.notify();
            })
            .log_err();
        });
        cx.notify();
    }

    /// The installed plugins' newer versions, for their Update buttons.
    /// shortcut: the registry is asked about the plugins that auto-update (Zed's
    /// `auto_update_extensions`, all of them unless turned off one by one); ask about every one
    /// once a plugin can't be updated by hand for having auto-updates turned off.
    fn fetch_updates(&mut self, cx: &mut Context<Self>) {
        let Some(store) = ExtensionStore::try_global(cx) else {
            return;
        };
        let latest = store.update(cx, |store, cx| {
            store.fetch_extensions_with_update_available(cx)
        });
        self._fetch_updates = cx.spawn(async move |this, cx| {
            let Some(latest) = latest.await.log_err() else {
                return;
            };
            this.update(cx, |this, cx| {
                this.updates = latest
                    .into_iter()
                    .map(|extension| (extension.id, extension.manifest.version))
                    .collect();
                cx.notify();
            })
            .log_err();
        });
    }

    /// The newer version of an installed plugin, when the registry has one; none for a plugin
    /// installed from disk, which installing from disk again updates.
    fn update_for(&self, plugin: &Plugin, cx: &App) -> Option<Arc<str>> {
        let ExtensionStatus::Installed(installed) = Self::status(&plugin.id, cx) else {
            return None;
        };
        let from_disk = ExtensionStore::try_global(cx)
            .is_some_and(|store| store.read(cx).is_dev_extension(&plugin.id));
        if from_disk {
            return None;
        }
        let latest = match self.tab {
            // the Marketplace's card shows the registry's latest version
            Tab::Marketplace => Some(Arc::from(plugin.version.as_ref())),
            Tab::Installed => self.updates.get(&plugin.id).cloned(),
        };
        latest.filter(|latest| *latest != installed)
    }

    /// The plugins installed, the search narrowing them.
    fn installed(&self, cx: &App) -> Vec<Plugin> {
        let Some(store) = ExtensionStore::try_global(cx) else {
            return Vec::new();
        };
        let query = self.query(cx).to_lowercase();
        store
            .read(cx)
            .installed_extensions()
            .values()
            // a plugin installed from disk (Zed's dev extension) too, as IntelliJ lists it
            .map(|entry| {
                let manifest = &entry.manifest;
                Plugin {
                    id: manifest.id.clone(),
                    name: manifest.name.clone().into(),
                    version: manifest.version.to_string().into(),
                    description: manifest.description.clone().map(Into::into),
                    vendor: manifest.authors.join(", ").into(),
                    downloads: None,
                    repository: manifest.repository.clone().map(Into::into),
                }
            })
            .filter(|plugin| query.is_empty() || plugin.name.to_lowercase().contains(&query))
            .collect()
    }

    fn status(id: &str, cx: &App) -> ExtensionStatus {
        ExtensionStore::try_global(cx).map_or(ExtensionStatus::NotInstalled, |store| {
            store.read(cx).extension_status(id)
        })
    }

    /// Installs the plugin's latest version, or updates it to `version`.
    fn install(id: Arc<str>, version: Option<Arc<str>>, cx: &mut App) {
        let Some(store) = ExtensionStore::try_global(cx) else {
            return;
        };
        store.update(cx, |store, cx| match version {
            None => store.install_latest_extension(id, cx),
            Some(version) => store
                .upgrade_extension(id, version, cx)
                .detach_and_log_err(cx),
        });
    }

    fn uninstall(id: Arc<str>, window: &mut Window, cx: &mut App) {
        let Some(store) = ExtensionStore::try_global(cx) else {
            return;
        };
        let (theme, icon_theme) = defaults_after_uninstall(&id, cx);
        let task = store.update(cx, |store, cx| store.uninstall_extension(id, cx));
        window
            .spawn(cx, async move |cx| match task.await {
                Ok(()) if theme.is_some() || icon_theme => cx
                    .update(|_, cx| {
                        let system = theme::SystemAppearance::global(cx).0;
                        settings::update_settings_file(
                            <dyn fs::Fs>::global(cx),
                            cx,
                            move |content, _| {
                                if let Some((name, appearance)) = theme {
                                    theme_settings::set_theme(content, name, appearance, system);
                                }
                                if icon_theme {
                                    theme_settings::set_icon_theme(
                                        content,
                                        settings::IconThemeName(
                                            theme::DEFAULT_ICON_THEME_NAME.into(),
                                        ),
                                        system,
                                    );
                                }
                            },
                        )
                    })
                    .log_err(),
                Ok(()) => None,
                Err(error) => cx
                    .update(|window, cx| {
                        window.push_notification(
                            gpui_kit::component::notification::Notification::error(format!(
                                "Couldn't uninstall the plugin: {error}"
                            )),
                            cx,
                        )
                    })
                    .log_err(),
            })
            .detach();
    }

    /// The Marketplace and Installed tabs, IntelliJ's header over the page; the number of plugins
    /// installed in a counter.
    pub(crate) fn tabs(page: &Entity<Self>, cx: &App) -> AnyElement {
        let tokens = Tokens::get(cx);
        let this = page.read(cx);
        let installed = ExtensionStore::try_global(cx)
            .map_or(0, |store| store.read(cx).installed_extensions().len());
        let tab = |id: &'static str, label: &'static str, which: Tab, counter: Option<usize>| {
            let page = page.clone();
            let selected = this.tab == which;
            v_flex()
                .id(id)
                .role(Role::Tab)
                .aria_label(SharedString::from(label))
                .aria_selected(selected)
                .h(px(TAB_HEIGHT))
                .px(px(12.))
                .justify_center()
                .cursor_pointer()
                .child(h_flex().flex_1().gap(px(6.)).child(label).children(
                    counter.filter(|count| *count > 0).map(|count| {
                        div()
                            .h(px(14.))
                            .min_w(px(16.))
                            .px(px(4.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(tokens.counter)
                            .text_color(tokens.counter_text)
                            .text_ui_xsmall()
                            .child(count.to_string())
                    }),
                ))
                .child(
                    div()
                        .h(px(3.))
                        .rounded_t(px(2.))
                        .when(selected, |line| line.bg(tokens.accent)),
                )
                .on_click(move |_, _, cx| {
                    page.update(cx, |page, cx| {
                        page.tab = which;
                        page.selected = None;
                        page.fetch(cx);
                    })
                })
        };
        let weak = page.downgrade();
        // IntelliJ's gear: of its items, the one Spork IDE backs
        let gear = spork_workbench::ActionButton::new("plugins-gear", "general/settings")
            .tooltip("Manage Repositories, Configure Proxy or Install Plugin from Disk")
            .dropdown_menu(move |menu: menu::PopupMenu, _, _| {
                let page = weak.clone();
                menu.item(
                    menu::PopupMenuItem::new("Install Plugin from Disk").on_click(
                        move |_, window, cx| Self::install_from_disk(page.clone(), window, cx),
                    ),
                )
            });
        h_flex()
            .child(tab(
                "plugins-marketplace",
                "Marketplace",
                Tab::Marketplace,
                None,
            ))
            .child(tab(
                "plugins-installed",
                "Installed",
                Tab::Installed,
                Some(installed),
            ))
            .child(div().w(px(8.)))
            .child(gear)
            .into_any_element()
    }

    /// IntelliJ's Install Plugin from Disk (`InstallFromDiskAction`): the plugin chosen installed
    /// from where it is, then the Installed tab. Zed's plugins are folders, where IntelliJ's are
    /// zip and jar archives.
    fn install_from_disk(page: WeakEntity<Self>, window: &mut Window, cx: &mut App) {
        let picked = spork_workbench::prompt_for_paths(
            PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: Some("Install".into()),
            },
            cx,
        );
        window
            .spawn(cx, async move |cx| {
                let Some(path) = picked.await??.and_then(|paths| paths.into_iter().next()) else {
                    return anyhow::Ok(());
                };
                let task = cx.update(|_, cx| {
                    ExtensionStore::try_global(cx).map(|store| {
                        store.update(cx, |store, cx| {
                            store.install_dev_extension(path.clone(), cx)
                        })
                    })
                })?;
                let Some(task) = task else {
                    return Ok(());
                };
                match task.await {
                    Ok(()) => page.update(cx, |page, cx| {
                        page.tab = Tab::Installed;
                        page.selected = None;
                        page.fetch(cx);
                    })?,
                    Err(error) => cx.update(|window, cx| {
                        crate::export_import::report(
                            "Install Plugin",
                            format!(
                                "Fail to load plugin descriptor from file {}\n\n{error:#}",
                                path.display()
                            ),
                            window,
                            cx,
                        )
                    })?,
                }
                Ok(())
            })
            .detach_and_log_err(cx);
    }

    fn card(&self, plugin: &Plugin, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let selected = self.selected.as_ref() == Some(&plugin.id);
        let status = Self::status(&plugin.id, cx);
        let id = plugin.id.clone();
        let update = self.update_for(plugin, cx);
        let button = match (self.tab, &status) {
            (_, ExtensionStatus::Installed(_)) if update.is_some() => Some(install_button(
                SharedString::from(format!("update-{}", plugin.id)),
                "Update",
                false,
                plugin.id.clone(),
                update,
                cx,
            )),
            (Tab::Installed, ExtensionStatus::Upgrading) => Some(install_button(
                SharedString::from(format!("update-{}", plugin.id)),
                "Installing",
                true,
                plugin.id.clone(),
                None,
                cx,
            )),
            (Tab::Installed, _) => None,
            (_, ExtensionStatus::NotInstalled) => Some(install_button(
                SharedString::from(format!("install-{}", plugin.id)),
                "Install",
                false,
                plugin.id.clone(),
                None,
                cx,
            )),
            (_, ExtensionStatus::Installing | ExtensionStatus::Upgrading) => Some(install_button(
                SharedString::from(format!("install-{}", plugin.id)),
                "Installing",
                true,
                plugin.id.clone(),
                None,
                cx,
            )),
            (_, _) => Some(
                spork_workbench::Button::new(
                    SharedString::from(format!("installed-{}", plugin.id)),
                    "Installed",
                )
                .disabled(true)
                .into_any_element(),
            ),
        };
        let details = h_flex()
            .gap(px(7.))
            .text_ui_small()
            .text_color(tokens.text_secondary)
            .children(plugin.downloads.map(|downloads| {
                h_flex()
                    .gap(px(2.))
                    .child(
                        spork_workbench::icon("general/download")
                            .size(px(14.))
                            .text_color(tokens.text_secondary),
                    )
                    .child(downloads_text(downloads))
            }))
            .when(self.tab == Tab::Installed, |line| {
                line.child(plugin.version.clone())
            })
            .child(plugin.vendor.clone());
        h_flex()
            .id(("plugin-card", ix))
            .role(Role::ListItem)
            .aria_label(plugin.name.clone())
            .aria_selected(selected)
            .w_full()
            .p(px(10.))
            .gap(px(10.))
            .items_start()
            .cursor_pointer()
            .when(selected, |card| card.bg(tokens.plugin_selection))
            .when(!selected, |card| {
                card.hover(move |card| card.bg(tokens.selection_hover))
            })
            .child(
                spork_workbench::image("nodes/pluginLogo", cx)
                    .size(px(LOGO))
                    .flex_none(),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(5.))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .pt(px(2.))
                                    .font_weight(FontWeight::BOLD)
                                    .child(plugin.name.clone()),
                            )
                            .children(button),
                    )
                    .child(details),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected = Some(id.clone());
                cx.notify();
            }))
            .into_any_element()
    }

    fn details(&self, plugin: Option<&Plugin>, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let Some(plugin) = plugin else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(tokens.text_secondary)
                .child(text_label(
                    "plugin-none",
                    "Select plugin to preview details",
                ))
                .into_any_element();
        };
        let status = Self::status(&plugin.id, cx);
        let id = plugin.id.clone();
        let update = self.update_for(plugin, cx).map(|version| {
            install_button(
                "details-update".into(),
                "Update",
                false,
                plugin.id.clone(),
                Some(version),
                cx,
            )
        });
        let button = match status {
            ExtensionStatus::NotInstalled => install_button(
                "details-install".into(),
                "Install",
                false,
                plugin.id.clone(),
                None,
                cx,
            ),
            ExtensionStatus::Installing | ExtensionStatus::Upgrading => install_button(
                "details-install".into(),
                "Installing",
                true,
                plugin.id.clone(),
                None,
                cx,
            ),
            ExtensionStatus::Removing => {
                spork_workbench::Button::new("details-uninstall", "Uninstall")
                    .disabled(true)
                    .into_any_element()
            }
            ExtensionStatus::Installed(_) => {
                spork_workbench::Button::new("details-uninstall", "Uninstall")
                    .on_click(move |_, window, cx| Self::uninstall(id.clone(), window, cx))
                    .into_any_element()
            }
        };
        v_flex()
            .id("plugin-details")
            .size_full()
            .pt(px(15.))
            .px(px(20.))
            .gap(px(8.))
            .overflow_y_scrollbar()
            .child(
                div()
                    .text_h2()
                    .font_weight(FontWeight::BOLD)
                    .child(plugin.name.clone()),
            )
            .child(
                h_flex().gap(px(12.)).child(
                    div()
                        .text_color(tokens.text_link)
                        .child(plugin.vendor.clone()),
                ),
            )
            .child(
                h_flex().gap(px(12.)).children(update).child(button).child(
                    div()
                        .text_color(tokens.text_secondary)
                        .child(plugin.version.clone()),
                ),
            )
            .child(
                h_flex()
                    .mt(px(6.))
                    .border_b_1()
                    .border_color(tokens.plugin_border)
                    .child(
                        v_flex()
                            .h(px(TAB_HEIGHT))
                            .px(px(12.))
                            .justify_center()
                            .child(div().flex_1().flex().items_center().child("Overview"))
                            .child(div().h(px(3.)).rounded_t(px(2.)).bg(tokens.accent)),
                    ),
            )
            .child(
                v_flex()
                    .pt(px(16.))
                    .gap(px(12.))
                    .children(
                        plugin
                            .description
                            .clone()
                            .map(|description| text_label("plugin-description", description)),
                    )
                    .child(
                        v_flex()
                            .gap(px(4.))
                            .text_color(tokens.text_secondary)
                            .child(text_label(
                                "plugin-version",
                                format!("Version: {}", plugin.version),
                            ))
                            .children(plugin.downloads.map(|downloads| {
                                text_label(
                                    "plugin-downloads",
                                    format!("Downloads: {}", downloads_text(downloads)),
                                )
                            }))
                            .child(text_label("plugin-id", format!("Plugin ID: {}", plugin.id)))
                            .children(plugin.repository.clone()),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn e2e_state(&self, cx: &App) -> Value {
        let names = |plugins: &[Plugin]| -> Vec<String> {
            plugins
                .iter()
                .map(|plugin| plugin.name.to_string())
                .collect()
        };
        json!({
            "tab": match self.tab {
                Tab::Marketplace => "Marketplace",
                Tab::Installed => "Installed",
            },
            "marketplace": match &self.marketplace {
                None => Value::Null,
                Some(Marketplace::Loading) => json!("loading"),
                Some(Marketplace::Loaded(plugins)) => json!(names(plugins)),
                Some(Marketplace::Failed(error)) => json!(error.to_string()),
            },
            "installed": names(&self.installed(cx)),
            "selected": self.selected.as_deref(),
            "query": self.query(cx),
            // the updates not installed yet
            "updates": self
                .updates
                .iter()
                .filter(|(id, version)| {
                    matches!(Self::status(id, cx), ExtensionStatus::Installed(installed) if installed != **version)
                })
                .map(|(id, version)| (id.to_string(), Value::from(version.to_string())))
                .collect::<serde_json::Map<_, _>>(),
        })
    }
}

/// IntelliJ's header tabs: 40 px (`TabbedPane.tabHeight`), a 3 px line under the one selected.
const TAB_HEIGHT: f32 = 40.;
/// The plugin logo on a card, `PluginLogo`'s.
const LOGO: f32 = 40.;
/// The list's share of the page, the details the rest.
const LIST_SHARE: f32 = 0.45;

/// IntelliJ's Install button (and Update, the same): green outline, 72 px; `version` is the one
/// Update installs, the latest when there is none.
fn install_button(
    id: SharedString,
    label: &'static str,
    busy: bool,
    plugin: Arc<str>,
    version: Option<Arc<str>>,
    cx: &App,
) -> AnyElement {
    let tokens = Tokens::get(cx);
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(SharedString::from(label))
        .h(px(spork_workbench::tokens::metrics::CONTROL_HEIGHT))
        .min_w(px(spork_workbench::tokens::metrics::BUTTON_MIN_WIDTH))
        .px(px(12.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(spork_workbench::tokens::metrics::RADIUS))
        .border_1()
        .border_color(tokens.install_border)
        .text_color(tokens.install_text)
        .when(!busy, |button| {
            button.cursor_pointer().on_click(move |_, _, cx| {
                cx.stop_propagation();
                PluginsPage::install(plugin.clone(), version.clone(), cx)
            })
        })
        .child(label)
        .into_any_element()
}

/// A download count as IntelliJ writes it: 615, 2.3K, 38.5M.
fn downloads_text(count: u64) -> String {
    match count {
        0..1_000 => count.to_string(),
        1_000..1_000_000 => format!("{:.1}K", count as f64 / 1_000.),
        _ => format!("{:.1}M", count as f64 / 1_000_000.),
    }
}

impl Render for PluginsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let searching = !self.query(cx).is_empty();
        let (title, plugins, note): (Option<String>, Vec<Plugin>, Option<SharedString>) =
            match self.tab {
                Tab::Marketplace => match &self.marketplace {
                    None | Some(Marketplace::Loading) => (None, Vec::new(), None),
                    Some(Marketplace::Failed(error)) => (None, Vec::new(), Some(error.clone())),
                    Some(Marketplace::Loaded(plugins)) if plugins.is_empty() => {
                        (None, Vec::new(), Some("Nothing found".into()))
                    }
                    Some(Marketplace::Loaded(plugins)) => (
                        (!searching).then(|| "Top Downloads".to_owned()),
                        plugins.clone(),
                        None,
                    ),
                },
                Tab::Installed => {
                    let installed = self.installed(cx);
                    match installed.is_empty() {
                        true if searching => (None, installed, Some("Nothing found.".into())),
                        _ => (
                            Some(format!("User-installed ({})", installed.len())),
                            installed,
                            None,
                        ),
                    }
                }
            };
        let selected = self
            .selected
            .as_ref()
            .and_then(|id| plugins.iter().find(|plugin| &plugin.id == id))
            .cloned();
        let cards: Vec<AnyElement> = plugins
            .iter()
            .enumerate()
            .map(|(ix, plugin)| self.card(plugin, ix, cx))
            .collect();
        let loading =
            self.tab == Tab::Marketplace && matches!(self.marketplace, Some(Marketplace::Loading));
        h_flex()
            .size_full()
            .border_t_1()
            .border_color(tokens.plugin_border)
            .child(
                v_flex()
                    .w(relative(LIST_SHARE))
                    .h_full()
                    .bg(tokens.plugin_background)
                    .border_r_1()
                    .border_color(tokens.plugin_border)
                    .child(
                        div().p(px(6.)).child(
                            spork_workbench::TextField::new(&self.search)
                                .aria_label("Search plugins")
                                .prefix(
                                    spork_workbench::icon("general/searchWithHistory")
                                        .w(px(20.))
                                        .h(px(16.)),
                                )
                                .cleanable(true),
                        ),
                    )
                    .child(
                        v_flex()
                            .id("plugin-list")
                            .role(Role::List)
                            .aria_label(SharedString::from("Plugins"))
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .children(title.map(|title| {
                                text_label("plugin-group", title)
                                    .px(px(10.))
                                    .py(px(4.))
                                    .bg(tokens.plugin_section)
                                    .text_color(tokens.text_secondary)
                            }))
                            .when(loading, |list| {
                                list.child(
                                    div()
                                        .p(px(10.))
                                        .flex()
                                        .justify_center()
                                        .child(spork_workbench::Spinner::new()),
                                )
                            })
                            .children(note.map(|note| {
                                text_label("plugin-note", note)
                                    .p(px(10.))
                                    .flex()
                                    .justify_center()
                                    .text_color(tokens.text_secondary)
                            }))
                            .children(cards),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .bg(tokens.plugin_background)
                    .child(self.details(selected.as_ref(), cx)),
            )
    }
}
