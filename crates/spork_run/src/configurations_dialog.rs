//! The Run/Debug Configurations dialog (`RunConfigurable`): the configurations by type down the
//! left under the toolbar (Add New Configuration, Remove, Copy, Move into New Folder, Sort), the
//! selected one's form on the right (Name, Store as project file, its type's fields, Before
//! launch), Edit configuration templates…, and Run or Debug, Cancel, Apply and OK. A Shell Script
//! is one of Zed's tasks, the debug types its debug scenarios; Apply writes them back.

use collections::HashMap;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    Global, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, TaskExt as _,
    WeakEntity, Window, WindowBounds, WindowOptions, div, point, px, size,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use serde_json::Value;
use spork_workbench::controls::{labeled_check, link};
use spork_workbench::{ActionButton, Tokens};
use util::ResultExt as _;
use workspace::Workspace;

use crate::configurations::{self, Configuration, Field, SHELL_SCRIPT};
use crate::{ProjectConfigurations, run};

const TITLE: &str = "Run/Debug Configurations";
const TEMPLATES_TITLE: &str = "Run/Debug Configuration Templates";

pub(crate) fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &run::EditConfigurations, window, cx| {
        open(workspace, None, window, cx)
    });
}

/// The workbench's colors follow the IDE's theme, light or dark, in a dialog's window.
pub(crate) fn follow_ide_theme(window: &mut Window, cx: &mut App) {
    use theme::ActiveTheme as _;
    let mode = match cx.theme().appearance {
        theme::Appearance::Light => gpui_kit::component::ThemeMode::Light,
        theme::Appearance::Dark => gpui_kit::component::ThemeMode::Dark,
    };
    if gpui_kit::component::ActiveTheme::theme(cx).mode != mode {
        gpui_kit::component::Theme::change(mode, Some(window), cx);
    }
}

/// The dialog's one window.
struct OpenDialog(AnyWindowHandle);

impl Global for OpenDialog {}

/// Opens the dialog on the configuration named `shown`, or the run widget's.
pub(crate) fn open(
    workspace: &mut Workspace,
    shown: Option<SharedString>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if let Some(OpenDialog(handle)) = cx.try_global::<OpenDialog>().map(|d| OpenDialog(d.0))
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let Some(configurations) = ProjectConfigurations::of(workspace, cx) else {
        return;
    };
    let ide = (workspace.weak_handle(), window.window_handle());
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(None, size(px(960.), px(640.)), cx)),
        window_min_size: Some(size(px(800.), px(600.))),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some(TITLE.into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.), px(8.))),
        }),
        ..Default::default()
    };
    cx.defer(move |cx| {
        match gpui_kit::open_window(options, cx, move |window, cx| {
            follow_ide_theme(window, cx);
            cx.new(|cx| ConfigurationsDialog::new(configurations, ide, shown, window, cx))
        }) {
            Ok((handle, _)) => cx.set_global(OpenDialog(handle)),
            Err(error) => log::error!("failed to open {TITLE}: {error:#}"),
        }
    });
}

/// The configuration types Spork IDE runs: Shell Script, then those of the debug adapters.
pub(crate) fn types(cx: &mut App) -> Vec<Option<SharedString>> {
    let mut adapters: Vec<SharedString> = dap::DapRegistry::global(cx)
        .enumerate_adapters::<Vec<_>>()
        .into_iter()
        .map(|name| name.0)
        .collect();
    adapters.sort_by_key(|adapter| configurations::type_name(Some(adapter)));
    std::iter::once(None)
        .chain(adapters.into_iter().map(Some))
        .collect()
}

/// The templates new configurations start from, by type, in the IDE's config folder.
fn templates_file() -> std::path::PathBuf {
    paths::config_dir().join("run_configuration_templates.json")
}

/// What the tree shows selected.
#[derive(Clone, PartialEq, Debug)]
enum Selection {
    /// A configuration type's node (in the templates, its template).
    Type(Option<SharedString>),
    Folder(Option<SharedString>, String),
    Configuration(usize),
}

struct ConfigurationsDialog {
    configurations: Entity<ProjectConfigurations>,
    /// The IDE window and workspace that opened the dialog, which Run and Debug run in.
    ide: (WeakEntity<Workspace>, AnyWindowHandle),
    saved: Vec<Configuration>,
    draft: Vec<Configuration>,
    /// Edit configuration templates…: the dialog shows the types' templates instead.
    templates_mode: bool,
    templates: HashMap<String, Configuration>,
    saved_templates: HashMap<String, Configuration>,
    selected: Option<Selection>,
    fields: Vec<(Field, Entity<InputState>)>,
    folder_name: Entity<InputState>,
    /// A folder made with no configuration in it yet, shown until the dialog closes.
    empty_folder: Option<(Option<SharedString>, String)>,
    focus: FocusHandle,
    _fields: Vec<Subscription>,
}

impl ConfigurationsDialog {
    fn new(
        configurations: Entity<ProjectConfigurations>,
        ide: (WeakEntity<Workspace>, AnyWindowHandle),
        shown: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let list = configurations.read(cx).list.clone();
        // IntelliJ opens on the configuration the run widget shows, or the one edited
        let selected = shown
            .as_ref()
            .or(configurations.read(cx).selected.as_ref())
            .and_then(|name| list.iter().position(|c| &c.name() == name))
            .or((!list.is_empty()).then_some(0))
            .map(Selection::Configuration);
        let templates = load_templates();
        let folder_name = cx.new(|cx| InputState::new(window, cx));
        // Esc reaches the dialog before a field has the focus
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut this = Self {
            configurations,
            ide,
            saved: list.clone(),
            draft: list,
            templates_mode: false,
            saved_templates: templates.clone(),
            templates,
            selected: None,
            fields: Vec::new(),
            folder_name,
            empty_folder: None,
            focus: focus.clone(),
            _fields: Vec::new(),
        };
        this.select(selected, window, cx);
        this
    }

    fn modified(&self) -> bool {
        self.draft != self.saved || self.templates != self.saved_templates
    }

    /// The configuration the form edits: the selected one, or the selected type's template.
    fn edited(&self) -> Option<&Configuration> {
        match &self.selected {
            Some(Selection::Configuration(ix)) if !self.templates_mode => self.draft.get(*ix),
            Some(Selection::Type(adapter)) if self.templates_mode => {
                self.templates.get(&type_key(adapter.as_deref()))
            }
            _ => None,
        }
    }

    fn edited_mut(&mut self) -> Option<&mut Configuration> {
        match self.selected.clone() {
            Some(Selection::Configuration(ix)) if !self.templates_mode => self.draft.get_mut(ix),
            Some(Selection::Type(adapter)) if self.templates_mode => {
                let key = type_key(adapter.as_deref());
                Some(
                    self.templates
                        .entry(key)
                        .or_insert_with(|| template_for(adapter.as_deref())),
                )
            }
            _ => None,
        }
    }

    /// Shows a node: a configuration's or template's fields fill the form.
    fn select(
        &mut self,
        selection: Option<Selection>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected = selection;
        if let Some(Selection::Folder(_, name)) = &self.selected {
            let name = name.clone();
            self.folder_name
                .update(cx, |field, cx| field.set_value(name, window, cx));
        }
        if self.templates_mode {
            // the form edits the template: it exists once shown
            self.edited_mut();
        }
        let Some(configuration) = self.edited().cloned() else {
            self.fields.clear();
            self._fields.clear();
            cx.notify();
            return;
        };
        let mut fields = vec![
            Field::Name,
            Field::Command,
            Field::Arguments,
            Field::WorkingDirectory,
            Field::Environment,
        ];
        if configuration.adapter.as_deref() == Some(configurations::JAVA) {
            fields.push(Field::VmOptions);
        }
        if self.templates_mode {
            fields.retain(|field| *field != Field::Name);
        }
        self._fields.clear();
        self.fields = fields
            .into_iter()
            .map(|field| {
                let text = configuration.text(field);
                let input = cx.new(|cx| {
                    let mut input =
                        InputState::new(window, cx).placeholder(placeholder(&configuration, field));
                    input.set_value(text, window, cx);
                    input
                });
                (field, input)
            })
            .collect();
        for (field, input) in self.fields.clone() {
            let subscription = cx.subscribe_in(&input, window, move |this, input, event, _, cx| {
                if let InputEvent::Change = event {
                    let text = input.read(cx).value().to_string();
                    if let Some(configuration) = this.edited_mut() {
                        configuration.set_text(field, &text);
                    }
                    cx.notify();
                }
            });
            self._fields.push(subscription);
        }
        cx.notify();
    }

    fn add(&mut self, adapter: Option<SharedString>, window: &mut Window, cx: &mut Context<Self>) {
        let mut configuration = self
            .templates
            .get(&type_key(adapter.as_deref()))
            .cloned()
            .unwrap_or_else(|| template_for(adapter.as_deref()));
        let name = unique_name(&self.draft, "Unnamed");
        configuration.set_text(Field::Name, &name);
        if let Some(Selection::Folder(_, folder)) = &self.selected {
            configuration
                .entry
                .insert("folder".into(), folder.clone().into());
        }
        self.draft.push(configuration);
        let ix = self.draft.len() - 1;
        self.select(Some(Selection::Configuration(ix)), window, cx);
    }

    fn remove(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.selected.clone() {
            Some(Selection::Configuration(ix)) if ix < self.draft.len() => {
                self.draft.remove(ix);
                let next = (!self.draft.is_empty())
                    .then(|| Selection::Configuration(ix.min(self.draft.len() - 1)));
                self.select(next, window, cx);
            }
            Some(Selection::Folder(adapter, folder)) => {
                if self
                    .empty_folder
                    .as_ref()
                    .is_some_and(|(_, empty)| *empty == folder)
                {
                    self.empty_folder = None;
                }
                // removing a folder keeps its configurations, out of it
                for configuration in &mut self.draft {
                    if configuration.adapter == adapter
                        && folder_of(configuration) == Some(folder.as_str())
                    {
                        configuration.entry.remove("folder");
                    }
                }
                self.select(None, window, cx);
            }
            _ => {}
        }
    }

    fn copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Selection::Configuration(ix)) = self.selected.clone() else {
            return;
        };
        let Some(mut configuration) = self.draft.get(ix).cloned() else {
            return;
        };
        let name = unique_name(&self.draft, &configuration.name());
        configuration.set_text(Field::Name, &name);
        self.draft.insert(ix + 1, configuration);
        self.select(Some(Selection::Configuration(ix + 1)), window, cx);
    }

    /// Create New Folder, or Move into New Folder when a configuration is selected.
    fn new_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (adapter, moved) = match self.selected.clone() {
            Some(Selection::Configuration(ix)) => match self.draft.get(ix) {
                Some(configuration) => (configuration.adapter.clone(), Some(ix)),
                None => return,
            },
            Some(Selection::Type(adapter)) | Some(Selection::Folder(adapter, _)) => (adapter, None),
            None => (None, None),
        };
        let taken: Vec<&str> = self.draft.iter().filter_map(folder_of).collect();
        let name = (0..)
            .map(|n| match n {
                0 => "New Folder".to_owned(),
                n => format!("New Folder ({n})"),
            })
            .find(|name| !taken.contains(&name.as_str()))
            .unwrap_or_default();
        if let Some(ix) = moved
            && let Some(configuration) = self.draft.get_mut(ix)
        {
            configuration
                .entry
                .insert("folder".into(), name.clone().into());
        }
        // shortcut: an empty folder lasts until the dialog closes, as its configurations keep
        // its name; IntelliJ keeps it in the workspace file
        self.empty_folder = moved.is_none().then(|| (adapter.clone(), name.clone()));
        self.select(Some(Selection::Folder(adapter, name)), window, cx);
    }

    fn rename_folder(&mut self, cx: &mut Context<Self>) {
        let Some(Selection::Folder(adapter, old)) = self.selected.clone() else {
            return;
        };
        let new = self.folder_name.read(cx).value().trim().to_owned();
        if new.is_empty() || new == old {
            return;
        }
        for configuration in &mut self.draft {
            if configuration.adapter == adapter && folder_of(configuration) == Some(old.as_str()) {
                configuration
                    .entry
                    .insert("folder".into(), new.clone().into());
            }
        }
        if let Some((_, empty)) = &mut self.empty_folder
            && *empty == old
        {
            *empty = new.clone();
        }
        self.selected = Some(Selection::Folder(adapter, new));
        cx.notify();
    }

    fn sort(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = match &self.selected {
            Some(Selection::Configuration(ix)) => self.draft.get(*ix).map(Configuration::name),
            _ => None,
        };
        self.draft
            .sort_by_key(|configuration| configuration.name().to_lowercase());
        if let Some(name) = selected {
            let ix = self.draft.iter().position(|c| c.name() == name);
            self.select(ix.map(Selection::Configuration), window, cx);
        }
        cx.notify();
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        if self.templates != self.saved_templates {
            save_templates(&self.templates);
            self.saved_templates = self.templates.clone();
        }
        if self.draft != self.saved {
            // OK closes the dialog right away: the writing outlives it
            self.configurations
                .update(cx, |configurations, cx| {
                    configurations.save(self.draft.clone(), cx)
                })
                .detach_and_log_err(cx);
            self.saved = self.draft.clone();
        }
        cx.notify();
    }

    fn ok(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply(cx);
        match self.templates_mode {
            // the templates' dialog closes back to the configurations
            true => self.show_templates(false, window, cx),
            false => self.close(window, cx),
        }
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.templates_mode {
            true => {
                self.templates = self.saved_templates.clone();
                self.show_templates(false, window, cx);
            }
            false => self.close(window, cx),
        }
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = cx;
        window.remove_window();
    }

    fn show_templates(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.templates_mode = on;
        window.set_window_title(match on {
            true => TEMPLATES_TITLE,
            false => TITLE,
        });
        let selection = match on {
            true => Some(Selection::Type(None)),
            false => (!self.draft.is_empty()).then_some(Selection::Configuration(0)),
        };
        self.select(selection, window, cx);
    }

    /// Run or Debug: applies the changes, closes the dialog and runs the configuration.
    fn launch(&mut self, debug: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(configuration) = self.edited().cloned() else {
            return;
        };
        self.apply(cx);
        let (workspace, ide_window) = self.ide.clone();
        let name = configuration.name();
        self.configurations.update(cx, |configurations, cx| {
            configurations.selected = Some(name);
            cx.notify();
        });
        cx.defer(move |cx| {
            ide_window
                .update(cx, |_, window, cx| {
                    workspace
                        .update(cx, |workspace, cx| {
                            crate::run_widget::launch(
                                Some(&configuration),
                                debug,
                                workspace,
                                window,
                                cx,
                            )
                        })
                        .log_err();
                })
                .log_err();
        });
        self.close(window, cx);
    }
}

/// A type's key in the templates file: its debug adapter, or "Shell Script".
fn type_key(adapter: Option<&str>) -> String {
    adapter.unwrap_or(SHELL_SCRIPT).to_owned()
}

/// A new configuration of a type, as no template changed it.
fn template_for(adapter: Option<&str>) -> Configuration {
    match adapter {
        None => Configuration::new_task(""),
        Some(adapter) => Configuration::new_debug(adapter, ""),
    }
}

fn load_templates() -> HashMap<String, Configuration> {
    let Ok(text) = std::fs::read_to_string(templates_file()) else {
        return HashMap::default();
    };
    let Ok(Value::Object(templates)) = settings::parse_json_with_comments::<Value>(&text) else {
        return HashMap::default();
    };
    templates
        .into_iter()
        .filter_map(|(key, entry)| {
            let Value::Object(entry) = entry else {
                return None;
            };
            let adapter = (key != SHELL_SCRIPT).then(|| SharedString::from(key.clone()));
            Some((
                key,
                Configuration {
                    adapter,
                    shared: true,
                    entry,
                },
            ))
        })
        .collect()
}

fn save_templates(templates: &HashMap<String, Configuration>) {
    let json: serde_json::Map<String, Value> = templates
        .iter()
        .map(|(key, template)| (key.clone(), Value::Object(template.entry.clone())))
        .collect();
    let written = serde_json::to_string_pretty(&Value::Object(json))
        .map_err(anyhow::Error::from)
        .and_then(|text| {
            std::fs::create_dir_all(paths::config_dir())?;
            std::fs::write(templates_file(), text)?;
            Ok(())
        });
    written.log_err();
}

/// `base`, or `base (1)`, `base (2)`… when a configuration has that name.
fn unique_name(configurations: &[Configuration], base: &str) -> String {
    let taken = |name: &str| configurations.iter().any(|c| c.name().as_ref() == name);
    if !taken(base) {
        return base.to_owned();
    }
    (1..)
        .map(|n| format!("{base} ({n})"))
        .find(|name| !taken(name))
        .unwrap_or_default()
}

fn folder_of(configuration: &Configuration) -> Option<&str> {
    configuration.entry.get("folder").and_then(Value::as_str)
}

/// A field's label in IntelliJ's form for the configuration's type.
fn label(configuration: &Configuration, field: Field) -> &'static str {
    match (field, configuration.adapter.as_deref()) {
        (Field::Name, _) => "Name:",
        (Field::Command, None) => "Script text:",
        (Field::Command, Some("Debugpy")) => "Script:",
        (Field::Command, Some("JavaScript")) => "JavaScript file:",
        (Field::Command, Some(configurations::JAVA)) => "Main class:",
        (Field::Command, Some(_)) => "Executable:",
        (Field::Arguments, None) => "Script options:",
        (Field::Arguments, Some(_)) => "Program arguments:",
        (Field::WorkingDirectory, _) => "Working directory:",
        (Field::Environment, _) => "Environment variables:",
        (Field::AllowMultiple, _) => "Allow multiple instances",
        (Field::BeforeLaunch, _) => "Before launch",
        (Field::VmOptions, _) => "VM options:",
    }
}

fn placeholder(configuration: &Configuration, field: Field) -> &'static str {
    match (field, configuration.is_task()) {
        (Field::Arguments, false) => "Program arguments",
        (Field::Environment, _) => "Environment variables or .env files",
        _ => "",
    }
}

impl ConfigurationsDialog {
    fn tree(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let mut rows: Vec<AnyElement> = Vec::new();
        let row = |id: SharedString, depth: usize, selected: bool, cx: &App| {
            h_flex()
                .id(id)
                .role(Role::TreeItem)
                .aria_selected(selected)
                .flex_none()
                .h(px(24.))
                .mx(px(6.))
                .pl(px(6. + 18. * depth as f32))
                .pr(px(6.))
                .gap(px(4.))
                .rounded(px(4.))
                .cursor_pointer()
                .when(selected, |row| row.bg(Tokens::get(cx).selection))
                .when(!selected, |row| {
                    row.hover(move |row| row.bg(tokens.selection_hover))
                })
        };
        let chevron = || {
            spork_workbench::icon("general/chevronDown")
                .size(px(16.))
                .text_color(tokens.icon)
        };
        let types: Vec<Option<SharedString>> = match self.templates_mode {
            true => types(cx),
            false => {
                let mut used: Vec<Option<SharedString>> = Vec::new();
                for configuration in &self.draft {
                    if !used.contains(&configuration.adapter) {
                        used.push(configuration.adapter.clone());
                    }
                }
                if let Some((adapter, _)) = &self.empty_folder
                    && !used.contains(adapter)
                {
                    used.push(adapter.clone());
                }
                used.sort_by_key(|adapter| configurations::type_name(adapter.as_deref()));
                used
            }
        };
        for adapter in types {
            let name = configurations::type_name(adapter.as_deref());
            let icon = configurations::type_icon(adapter.as_deref());
            let selection = Selection::Type(adapter.clone());
            let selected = self.selected.as_ref() == Some(&selection);
            rows.push(
                row(format!("type-{name}").into(), 0, selected, cx)
                    .aria_label(name.clone())
                    .when(!self.templates_mode, |row| {
                        row.aria_expanded(true).child(chevron())
                    })
                    .child(spork_workbench::image(icon, cx).size(px(16.)))
                    .child(div().font_weight(gpui::FontWeight::BOLD).child(name))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select(Some(selection.clone()), window, cx)
                    }))
                    .into_any_element(),
            );
            if self.templates_mode {
                continue;
            }
            let mut folders: Vec<String> = Vec::new();
            for configuration in self.draft.iter().filter(|c| c.adapter == adapter) {
                if let Some(folder) = folder_of(configuration)
                    && !folders.iter().any(|f| f == folder)
                {
                    folders.push(folder.to_owned());
                }
            }
            if let Some((empty_adapter, empty)) = &self.empty_folder
                && *empty_adapter == adapter
                && !folders.contains(empty)
            {
                folders.push(empty.clone());
            }
            for folder in folders.iter().map(Some).chain([None]) {
                if let Some(folder) = folder {
                    let selection = Selection::Folder(adapter.clone(), folder.clone());
                    let selected = self.selected.as_ref() == Some(&selection);
                    rows.push(
                        row(format!("folder-{folder}").into(), 1, selected, cx)
                            .aria_label(SharedString::from(folder.clone()))
                            .aria_expanded(true)
                            .child(chevron())
                            .child(spork_workbench::image("nodes/folder", cx).size(px(16.)))
                            .child(folder.clone())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select(Some(selection.clone()), window, cx)
                            }))
                            .into_any_element(),
                    );
                }
                for (ix, configuration) in self.draft.iter().enumerate() {
                    if configuration.adapter != adapter
                        || folder_of(configuration) != folder.map(String::as_str)
                    {
                        continue;
                    }
                    let selection = Selection::Configuration(ix);
                    let selected = self.selected.as_ref() == Some(&selection);
                    let depth = 1 + usize::from(folder.is_some());
                    rows.push(
                        row(format!("configuration-{ix}").into(), depth, selected, cx)
                            .aria_label(configuration.name())
                            .child(div().w(px(16.)))
                            .child(spork_workbench::image(configuration.icon(), cx).size(px(16.)))
                            .child(div().truncate().child(configuration.name()))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select(Some(selection.clone()), window, cx)
                            }))
                            .into_any_element(),
                    );
                }
            }
        }
        let empty = rows.is_empty();
        v_flex()
            .id("configurations-tree")
            .role(Role::Tree)
            .aria_label("Run/Debug Configurations")
            .flex_1()
            .min_h_0()
            .py(px(4.))
            .overflow_y_scroll()
            .children(rows)
            .when(empty, |tree| {
                tree.child(
                    v_flex()
                        .items_center()
                        .pt(px(40.))
                        .gap(px(4.))
                        .text_color(tokens.text_secondary)
                        .child("No run configurations added.")
                        .child(self.add_link("Add new…", "configurations-add-new-tree", cx)),
                )
            })
            .into_any_element()
    }

    /// A link opening the Add New Configuration popup.
    fn add_link(&self, text: &'static str, id: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        spork_workbench::controls::DropDownLink::new(id, text)
            .without_chevron()
            .dropdown_menu(move |menu, window, cx| add_menu(menu, this.clone(), window, cx))
            .into_any_element()
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        let configuration_selected = matches!(self.selected, Some(Selection::Configuration(_)));
        let removable =
            configuration_selected || matches!(self.selected, Some(Selection::Folder(..)));
        let folder_tooltip = match configuration_selected {
            true => "Move into New Folder",
            false => "Create New Folder",
        };
        h_flex()
            .id("configurations-toolbar")
            .role(Role::Toolbar)
            .aria_label("Run/Debug Configurations Toolbar")
            .flex_none()
            .h(px(32.))
            .px(px(6.))
            .gap(px(4.))
            .child(
                ActionButton::new("configuration-add", "general/add")
                    .tooltip("Add New Configuration (⌘N)")
                    .disabled(self.templates_mode)
                    .dropdown_menu(move |menu, window, cx| {
                        add_menu(menu, this.clone(), window, cx)
                    }),
            )
            .child(
                ActionButton::new("configuration-remove", "general/remove")
                    .tooltip("Remove Configuration (⌘⌫)")
                    .disabled(self.templates_mode || !removable)
                    .on_click(cx.listener(|this, _, window, cx| this.remove(window, cx))),
            )
            .child(
                ActionButton::new("configuration-copy", "general/copy")
                    .tooltip("Copy Configuration (⌘D)")
                    .disabled(self.templates_mode || !configuration_selected)
                    .on_click(cx.listener(|this, _, window, cx| this.copy(window, cx))),
            )
            .child(
                ActionButton::new("configuration-folder", "actions/newFolder")
                    .tooltip(folder_tooltip)
                    .disabled(self.templates_mode)
                    .on_click(cx.listener(|this, _, window, cx| this.new_folder(window, cx))),
            )
            .child(
                ActionButton::new("configuration-sort", "objectBrowser/sortAlphabetically")
                    .tooltip("Sort Configurations")
                    .disabled(self.templates_mode)
                    .on_click(cx.listener(|this, _, window, cx| this.sort(window, cx))),
            )
            .into_any_element()
    }

    fn form(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        if let Some(Selection::Folder(..)) = &self.selected {
            return v_flex()
                .p(px(16.))
                .gap(px(8.))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .child(spork_workbench::controls::form_label("Name:"))
                        .child(div().flex_1().child(
                            spork_workbench::TextField::new(&self.folder_name).aria_label("Name:"),
                        )),
                )
                .into_any_element();
        }
        let Some(configuration) = self.edited().cloned() else {
            return v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .child(self.add_link("Add new run configuration…", "configurations-add-new", cx))
                .child(
                    div()
                        .text_color(tokens.text_secondary)
                        .child("or select run configuration to edit"),
                )
                .into_any_element();
        };
        let field = |field: Field| -> AnyElement {
            let Some((_, input)) = self.fields.iter().find(|(f, _)| *f == field) else {
                return div().into_any_element();
            };
            let text = label(&configuration, field);
            h_flex()
                .gap(px(8.))
                .child(
                    div()
                        .w(px(160.))
                        .flex_none()
                        .child(spork_workbench::controls::form_label(text)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(spork_workbench::TextField::new(input).aria_label(text)),
                )
                .into_any_element()
        };
        let shared = configuration.shared;
        let multiple = configuration.text(Field::AllowMultiple) == "true";
        let header = match self.templates_mode {
            true => None,
            false => Some(
                h_flex()
                    .gap(px(12.))
                    .child(div().flex_1().child(field(Field::Name)))
                    .when(configuration.is_task(), |row| {
                        row.child(
                            labeled_check(
                                "allow-multiple",
                                "Allow multiple instances",
                                multiple,
                                cx,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let Some(configuration) = this.edited_mut() {
                                        let text = if multiple { "" } else { "true" };
                                        configuration.set_text(Field::AllowMultiple, text);
                                    }
                                    cx.notify();
                                },
                            )),
                        )
                    })
                    .child(
                        labeled_check("store-as-project-file", "Store as project file", shared, cx)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(configuration) = this.edited_mut() {
                                    configuration.shared = !configuration.shared;
                                }
                                cx.notify();
                            })),
                    ),
            ),
        };
        v_flex()
            .id("configuration-form")
            .flex_1()
            .min_w_0()
            .pt(px(15.))
            .pl(px(15.))
            .pr(px(15.))
            .gap(px(10.))
            .overflow_y_scroll()
            .children(header)
            .when(!self.templates_mode, |form| {
                form.child(div().h(px(1.)).bg(tokens.separator))
            })
            // the type's own Modify options, over its fields (a runner's hook)
            .children(self.modify_options(&configuration, cx))
            // IntelliJ's Application: VM options, the main class, then the program's arguments
            .when(
                configuration.adapter.as_deref() == Some(configurations::JAVA)
                    && configurations::runner(&configuration, cx)
                        .is_none_or(|runner| runner.shows(&configuration, Field::VmOptions)),
                |form| form.child(field(Field::VmOptions)),
            )
            .child(field(Field::Command))
            .child(field(Field::Arguments))
            .child(field(Field::WorkingDirectory))
            .child(field(Field::Environment))
            .child(
                div()
                    .pl(px(168.))
                    .text_color(tokens.text_secondary)
                    .child("Separate variables with semicolon: VAR=value; VAR1=value1"),
            )
            .when(!configuration.is_task(), |form| {
                form.child(self.before_launch(&configuration, cx))
            })
            .into_any_element()
    }

    /// IntelliJ's "Build and run" title with the Modify options link (⌥M) and its popup of the
    /// type's options by group, where the configuration's runner has some.
    fn modify_options(
        &self,
        configuration: &Configuration,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let options = configurations::runner(configuration, cx)?.modify_options(configuration);
        if options.is_empty() {
            return None;
        }
        let this = cx.entity().downgrade();
        let options = std::rc::Rc::new(options);
        let link = spork_workbench::controls::DropDownLink::new("modify-options", "Modify options")
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu;
                let mut group = "";
                for option in options.iter() {
                    if option.group != group {
                        group = option.group;
                        menu = menu.label(group);
                    }
                    let (this, toggle) = (this.clone(), option.toggle);
                    menu = menu.item(
                        PopupMenuItem::new(option.label)
                            .checked(option.checked)
                            .disabled(toggle.is_none())
                            .on_click(move |_, _, cx| {
                                let Some(toggle) = toggle else { return };
                                this.update(cx, |this, cx| {
                                    if let Some(configuration) = this.edited_mut() {
                                        toggle(configuration);
                                    }
                                    cx.notify();
                                })
                                .log_err();
                            }),
                    );
                }
                menu
            });
        Some(
            h_flex()
                .justify_between()
                .child(
                    div()
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("Build and run"),
                )
                .child(link)
                .into_any_element(),
        )
    }

    /// A debug configuration's Before launch: the task it runs first (its `build`).
    fn before_launch(&self, configuration: &Configuration, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let build = configuration.text(Field::BeforeLaunch);
        let tasks: Vec<SharedString> = self
            .draft
            .iter()
            .filter(|c| c.is_task())
            .map(Configuration::name)
            .collect();
        let has_build = !build.is_empty();
        v_flex()
            .gap(px(4.))
            .child(
                div()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Before launch"),
            )
            .child(
                v_flex()
                    .border_1()
                    .border_color(tokens.control_border)
                    .rounded(px(4.))
                    .child(
                        h_flex()
                            .id("before-launch-toolbar")
                            .role(Role::Toolbar)
                            .aria_label("Before launch")
                            .px(px(4.))
                            .gap(px(4.))
                            .border_b_1()
                            .border_color(tokens.separator)
                            .child(
                                ActionButton::new("before-launch-add", "general/add")
                                    .tooltip("Add")
                                    .disabled(tasks.is_empty())
                                    .dropdown_menu({
                                        let this = cx.entity().downgrade();
                                        move |menu, window, cx| {
                                            let tasks = tasks.clone();
                                            let this = this.clone();
                                            menu.label("Add New Task").submenu(
                                                "Run Another Configuration",
                                                window,
                                                cx,
                                                move |menu, _, _| {
                                                    tasks.iter().fold(menu, |menu, name| {
                                                        let (this, name) =
                                                            (this.clone(), name.clone());
                                                        menu.item(
                                                            PopupMenuItem::new(name.clone())
                                                                .on_click(move |_, _, cx| {
                                                                    this.update(cx, |this, cx| {
                                                                        if let Some(configuration) =
                                                                            this.edited_mut()
                                                                        {
                                                                            configuration.set_text(
                                                                                Field::BeforeLaunch,
                                                                                &name,
                                                                            );
                                                                        }
                                                                        cx.notify();
                                                                    })
                                                                    .log_err();
                                                                }),
                                                        )
                                                    })
                                                },
                                            )
                                        }
                                    }),
                            )
                            .child(
                                ActionButton::new("before-launch-remove", "general/remove")
                                    .tooltip("Remove")
                                    .disabled(!has_build)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(configuration) = this.edited_mut() {
                                            configuration.set_text(Field::BeforeLaunch, "");
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("before-launch-list")
                            .role(Role::List)
                            .aria_label("Before launch tasks")
                            .min_h(px(48.))
                            .px(px(8.))
                            .py(px(4.))
                            .map(|list| match has_build {
                                true => list.child(
                                    div()
                                        .id("before-launch-task")
                                        .role(Role::ListItem)
                                        .aria_label(SharedString::from(format!("Run '{build}'")))
                                        .child(format!("Run '{build}'")),
                                ),
                                false => list.text_color(tokens.text_secondary).child(
                                    div()
                                        .id("before-launch-empty")
                                        .role(Role::Label)
                                        .aria_label("There are no tasks to run before launch")
                                        .child("There are no tasks to run before launch"),
                                ),
                            }),
                    ),
            )
            .into_any_element()
    }

    fn buttons(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let modified = self.modified();
        // a configuration whose type has a runner runs, as IntelliJ's Application does
        let runnable = (!self.templates_mode)
            .then(|| {
                self.edited().map(|configuration| {
                    configuration.is_task() || configurations::runner(configuration, cx).is_some()
                })
            })
            .flatten();
        let executor = runnable.map(|task| {
            let (label, icon, debug) = match task {
                true => ("Run", "run/run", false),
                false => ("Debug", "run/debug", true),
            };
            // the executor's icon before its name, as IntelliJ's split button has it
            spork_workbench::Button::new("configuration-launch", label)
                .flex_row_reverse()
                .child(spork_workbench::image(icon, cx).size(px(16.)))
                .on_click(cx.listener(move |this, _, window, cx| this.launch(debug, window, cx)))
        });
        h_flex()
            .flex_none()
            .px(px(12.))
            .py(px(8.))
            .gap(px(12.))
            .border_t_1()
            .border_color(tokens.separator)
            .child(div().flex_1())
            .children(executor)
            .child(
                spork_workbench::Button::new("configurations-cancel", "Cancel")
                    .on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx))),
            )
            .child(
                spork_workbench::Button::new("configurations-apply", "Apply")
                    .disabled(!modified)
                    .on_click(cx.listener(|this, _, _, cx| this.apply(cx))),
            )
            .child(
                spork_workbench::Button::new("configurations-ok", "OK")
                    .primary(true)
                    .on_click(cx.listener(|this, _, window, cx| this.ok(window, cx))),
            )
            .into_any_element()
    }
}

/// Add New Configuration: the types Spork IDE runs.
fn add_menu(
    menu: PopupMenu,
    this: WeakEntity<ConfigurationsDialog>,
    _: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    types(cx)
        .into_iter()
        .fold(menu.label("Add New Configuration"), |menu, adapter| {
            let this = this.clone();
            let name = configurations::type_name(adapter.as_deref());
            menu.item(
                PopupMenuItem::new(name)
                    .icon(spork_workbench::icon(configurations::type_icon(
                        adapter.as_deref(),
                    )))
                    .on_click(move |_, window, cx| {
                        this.update(cx, |this, cx| this.add(adapter.clone(), window, cx))
                            .log_err();
                    }),
            )
        })
}

impl Focusable for ConfigurationsDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ConfigurationsDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        self.rename_folder(cx);
        let title = match self.templates_mode {
            true => TEMPLATES_TITLE,
            false => TITLE,
        };
        let tree = self.tree(cx);
        let toolbar = self.toolbar(cx);
        let form = self.form(cx);
        let buttons = self.buttons(cx);
        v_flex()
            .id("configurations-dialog")
            .key_context("RunConfigurationsDialog")
            .track_focus(&self.focus)
            .size_full()
            .bg(tokens.panel)
            .text_color(tokens.text)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.cancel(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(spork_workbench::shell::window_title(title, cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(
                        v_flex()
                            .w(gpui::relative(0.3))
                            .min_w(px(240.))
                            .flex_none()
                            .h_full()
                            .border_r_1()
                            .border_color(tokens.separator)
                            .child(toolbar)
                            .child(tree)
                            .when(!self.templates_mode, |column| {
                                column.child(
                                    div().flex_none().p(px(10.)).child(
                                        link("edit-templates", "Edit configuration templates…", cx)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.show_templates(true, window, cx)
                                            })),
                                    ),
                                )
                            }),
                    )
                    .child(div().flex_1().min_w_0().h_full().flex().child(form)),
            )
            .child(buttons)
    }
}
