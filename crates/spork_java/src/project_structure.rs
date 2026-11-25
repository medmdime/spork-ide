//! IntelliJ's Project Structure dialog (File › Project Structure…, ⌘;,
//! `ProjectStructureConfigurable`), its pages for JDKs and language levels: Project (Name, SDK
//! with Edit, Language level, Compiler output), Modules (the module's Language level on Sources
//! and its Module SDK on Dependencies) and Platform Settings › SDKs (the SDK table: add, remove,
//! rename, the JDK home path). Changes wait for Apply or OK as IntelliJ's; a Maven or Gradle
//! project's language level goes to its build file, which is then synced.
// shortcut: Libraries, Facets, Artifacts, Global Libraries and Problems, a module's Paths tab and
// content roots, and an SDK's Classpath, Sourcepath, Annotations and Documentation Paths tabs
// are left out; add them when the IDE builds without the build tools (JPS-like projects)

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Global,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window,
    WindowBounds, WindowOptions, div, point, px, size,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use spork_workbench::controls::{ComboBox, dropdown, form_label};
use spork_workbench::{ActionButton, Tokens, Typography as _};
use util::ResultExt as _;
use workspace::Workspace;

use crate::java_project::{JavaProject, Tool};
use crate::jdk::{self, Jdk};
use crate::project_settings::{self, LanguageLevel, ProjectSettings};
use crate::sdks::{self, Sdk};

gpui::actions!(
    java,
    [
        /// File › Project Structure… (⌘;): the project's SDK, language level and modules, and
        /// the IDE's SDKs.
        ShowProjectStructureSettings
    ]
);

const TITLE: &str = "Project Structure";

pub(crate) fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &ShowProjectStructureSettings, _, cx| {
        if let Some(java) = JavaProject::of(workspace, cx) {
            open(java, cx);
        }
    });
}

/// The dialog's one window.
struct OpenDialog(gpui::AnyWindowHandle);

impl Global for OpenDialog {}

/// The dialog shown, for the end-to-end tests.
struct Shown(WeakEntity<ProjectStructure>);

impl Global for Shown {}

fn open(java: Entity<JavaProject>, cx: &mut App) {
    if let Some(OpenDialog(handle)) = cx.try_global::<OpenDialog>().map(|d| OpenDialog(d.0))
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let Some(root) = java.read(cx).root.clone() else {
        return;
    };
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(
            None,
            size(px(WIDTH), px(HEIGHT)),
            cx,
        )),
        window_min_size: Some(size(px(WIDTH), px(HEIGHT))),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some(TITLE.into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.), px(8.))),
        }),
        ..Default::default()
    };
    cx.defer(move |cx| {
        match gpui_kit::open_window(options, cx, move |window, cx| {
            spork_run::follow_ide_theme(window, cx);
            cx.new(|cx| ProjectStructure::new(java, root, window, cx))
        }) {
            Ok((handle, _)) => cx.set_global(OpenDialog(handle)),
            Err(error) => log::error!("failed to open {TITLE}: {error:#}"),
        }
    });
}

/// The catalogue's capture of the dialog (800×662), a little wider for the Modules and SDKs
/// pages' middle column.
const WIDTH: f32 = 900.;
const HEIGHT: f32 = 662.;
/// The sidebar's width and the forms' label column, measured on the catalogue's capture.
const SIDEBAR_WIDTH: f32 = 150.;
const LABEL_WIDTH: f32 = 112.;
const FIELD_WIDTH: f32 = 432.;
/// The Modules and SDKs pages' list of modules or SDKs.
const LIST_WIDTH: f32 = 200.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Project,
    Modules,
    Sdks,
}

impl Page {
    fn name(self) -> &'static str {
        match self {
            Page::Project => "Project",
            Page::Modules => "Modules",
            Page::Sdks => "SDKs",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModuleTab {
    Sources,
    Dependencies,
}

/// What the dialog edits, applied by Apply or OK.
#[derive(Clone, Debug, PartialEq)]
struct Draft {
    name: String,
    /// The project SDK's home.
    sdk: Option<PathBuf>,
    /// None: the SDK's default.
    level: Option<LanguageLevel>,
    output: String,
    /// None: the project's.
    module_level: Option<LanguageLevel>,
    /// None: the project SDK.
    module_sdk: Option<PathBuf>,
    sdks: Vec<Sdk>,
}

struct ProjectStructure {
    java: Entity<JavaProject>,
    root: PathBuf,
    tool: Option<Tool>,
    page: Page,
    /// The pages shown, for the back and forward arrows.
    history: Vec<Page>,
    history_ix: usize,
    module_tab: ModuleTab,
    saved: Draft,
    draft: Draft,
    /// The SDKs page's selected SDK.
    selected_sdk: Option<usize>,
    /// JDKs found that the table doesn't hold (IntelliJ's "Detected SDKs").
    detected: Vec<Jdk>,
    name: Entity<InputState>,
    output: Entity<InputState>,
    sdk_name: Entity<InputState>,
    sdk_home: Entity<InputState>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

/// An input whose text is `text`.
fn input(text: &str, window: &mut Window, cx: &mut App) -> Entity<InputState> {
    cx.new(|cx| {
        let mut input = InputState::new(window, cx);
        input.set_value(text.to_owned(), window, cx);
        input
    })
}

impl ProjectStructure {
    fn new(
        java: Entity<JavaProject>,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let tool = java.read(cx).tool;
        let settings = ProjectSettings::load(&root);
        let env = jdk::process_env();
        let draft = Draft {
            name: java.read(cx).name(),
            sdk: jdk::for_project(&root, &env).map(|jdk| jdk.home),
            level: project_settings::language_level(&root, tool)
                .filter(|_| tool.is_some())
                .or(settings.language_level),
            output: shown(&settings.compiler_output_or_default(&root)),
            module_level: settings.module_language_level.filter(|_| tool.is_none()),
            module_sdk: settings.module_jdk,
            sdks: with_project_sdk(sdks::load(), jdk::for_project(&root, &env)),
        };
        let detected = jdk::find(&env);
        let name = input(&draft.name, window, cx);
        let output = input(&draft.output, window, cx);
        let sdk_name = input("", window, cx);
        let sdk_home = input("", window, cx);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let edits = |input: &Entity<InputState>,
                     edit: fn(&mut ProjectStructure, String),
                     window: &mut Window,
                     cx: &mut Context<Self>| {
            cx.subscribe_in(input, window, move |this, input, event, _, cx| {
                if let InputEvent::Change = event {
                    let text = input.read(cx).value().to_string();
                    edit(this, text);
                    cx.notify();
                }
            })
        };
        let _subscriptions = vec![
            edits(&name, |this, text| this.draft.name = text, window, cx),
            edits(&output, |this, text| this.draft.output = text, window, cx),
            edits(
                &sdk_name,
                |this, text| {
                    if let Some(sdk) = this.selected_sdk.and_then(|ix| this.draft.sdks.get_mut(ix))
                    {
                        sdk.name = text;
                    }
                },
                window,
                cx,
            ),
            edits(
                &sdk_home,
                |this, text| {
                    if let Some(sdk) = this.selected_sdk.and_then(|ix| this.draft.sdks.get_mut(ix))
                    {
                        sdk.home = expand(&text);
                    }
                },
                window,
                cx,
            ),
            cx.on_release({
                let handle = window.window_handle();
                move |_, cx| {
                    if cx
                        .try_global::<OpenDialog>()
                        .is_some_and(|open| open.0 == handle)
                    {
                        cx.remove_global::<OpenDialog>();
                    }
                }
            }),
        ];
        let shown = Shown(cx.entity().downgrade());
        cx.set_global(shown);
        let mut this = Self {
            java,
            root,
            tool,
            page: Page::Project,
            history: vec![Page::Project],
            history_ix: 0,
            module_tab: ModuleTab::Sources,
            saved: draft.clone(),
            draft,
            selected_sdk: None,
            detected,
            name,
            output,
            sdk_name,
            sdk_home,
            focus,
            _subscriptions,
        };
        this.select_sdk(0, window, cx);
        this
    }

    fn show(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page != page {
            self.history.truncate(self.history_ix + 1);
            self.history.push(page);
            self.history_ix = self.history.len() - 1;
            self.page = page;
        }
        cx.notify();
    }

    fn step(&mut self, back: bool, cx: &mut Context<Self>) {
        let ix = match back {
            true => self.history_ix.checked_sub(1),
            false => Some(self.history_ix + 1).filter(|ix| *ix < self.history.len()),
        };
        if let Some(ix) = ix {
            self.history_ix = ix;
            self.page = self.history[ix];
            cx.notify();
        }
    }

    fn select_sdk(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sdk) = self.draft.sdks.get(ix).cloned() else {
            self.selected_sdk = None;
            return;
        };
        self.selected_sdk = Some(ix);
        self.sdk_name.update(cx, |input, cx| {
            input.set_value(sdk.name.clone(), window, cx)
        });
        self.sdk_home.update(cx, |input, cx| {
            input.set_value(sdk.home.display().to_string(), window, cx)
        });
        cx.notify();
    }

    /// Adds `jdk` to the table unless it is there; its index.
    fn add_sdk(&mut self, jdk: &Jdk) -> usize {
        if let Some(ix) = self.draft.sdks.iter().position(|sdk| sdk.home == jdk.home) {
            return ix;
        }
        let name = sdks::unique_name(&self.draft.sdks, &jdk.suggested_name());
        self.draft.sdks.push(Sdk {
            name,
            home: jdk.home.clone(),
        });
        self.draft.sdks.len() - 1
    }

    /// Where a JDK added from a combo box goes: the project's SDK or the module's.
    fn add_and_use(
        &mut self,
        jdk: &Jdk,
        target: SdkTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ix = self.add_sdk(jdk);
        match target {
            SdkTarget::Project => self.draft.sdk = Some(jdk.home.clone()),
            SdkTarget::Module => self.draft.module_sdk = Some(jdk.home.clone()),
            SdkTarget::Table => {}
        }
        self.select_sdk(ix, window, cx);
        cx.notify();
    }

    fn remove_sdk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.selected_sdk else { return };
        if ix < self.draft.sdks.len() {
            self.draft.sdks.remove(ix);
        }
        self.select_sdk(ix.min(self.draft.sdks.len().saturating_sub(1)), window, cx);
        cx.notify();
    }

    fn modified(&self) -> bool {
        self.draft != self.saved
    }

    /// The level a Maven or Gradle project's build file gets, or a plain folder's JDT LS.
    fn effective_level(draft: &Draft) -> Option<LanguageLevel> {
        draft.module_level.or(draft.level)
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        let (draft, saved) = (self.draft.clone(), self.saved.clone());
        if draft == saved {
            return;
        }
        let root = self.root.clone();
        if draft.sdks != saved.sdks {
            sdks::save(&draft.sdks).log_err();
        }
        let mut settings = ProjectSettings::load(&root);
        let folder_name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        settings.name = (draft.name.trim() != folder_name && !draft.name.trim().is_empty())
            .then(|| draft.name.trim().to_owned());
        let output = expand(&draft.output);
        settings.compiler_output = (output != root.join("out")).then_some(output);
        settings.module_jdk = draft.module_sdk.clone();
        if self.tool.is_none() {
            settings.language_level = draft.level;
            settings.module_language_level = draft.module_level;
        }
        settings.save(&root).log_err();
        let level_changed = Self::effective_level(&draft) != Self::effective_level(&saved);
        let tool = self.tool;
        let project_jdk = draft
            .sdk
            .as_deref()
            .filter(|_| draft.sdk != saved.sdk || draft.module_sdk != saved.module_sdk)
            .and_then(jdk::at_folder);
        self.java.update(cx, |java, cx| {
            if let Some(jdk) = project_jdk {
                java.set_jdk(jdk, cx);
            } else if draft.module_sdk != saved.module_sdk || draft.sdks != saved.sdks {
                // JDT LS builds on the table's JDKs (`java.configuration.runtimes`)
                java.refresh(cx);
                java.restart_language_server(cx);
            }
            if level_changed {
                match (tool, Self::effective_level(&draft)) {
                    (Some(tool), Some(level)) => {
                        project_settings::write_language_level(&root, tool, level).log_err();
                        java.reload(cx);
                    }
                    // shortcut: "SDK default" leaves a build file's level as written; remove
                    // the setting once a build file's edit can delete one
                    (Some(_), None) => {}
                    (None, _) => java.restart_language_server(cx),
                }
            }
            cx.notify();
        });
        self.saved = draft;
        cx.notify();
    }

    fn close(&mut self, window: &mut Window) {
        window.remove_window();
    }
}

/// A path as the fields show it: the home folder as `~`.
fn shown(path: &Path) -> String {
    crate::jdk_download::shown_path(path)
}

/// A field's path, `~` the home folder.
fn expand(text: &str) -> PathBuf {
    match text.trim().strip_prefix("~/") {
        Some(rest) => paths::home_dir().join(rest),
        None => PathBuf::from(text.trim()),
    }
}

/// The table with the project's JDK in it, as IntelliJ lists the project SDK it set up.
fn with_project_sdk(mut table: Vec<Sdk>, project: Option<Jdk>) -> Vec<Sdk> {
    if let Some(jdk) = project
        && !table.iter().any(|sdk| sdk.home == jdk.home)
    {
        let name = sdks::unique_name(&table, &jdk.suggested_name());
        table.push(Sdk {
            name,
            home: jdk.home,
        });
    }
    table
}

/// What a JDK added from a combo box becomes.
#[derive(Clone, Copy)]
enum SdkTarget {
    Project,
    Module,
    Table,
}

/// A JDK's version as IntelliJ's SDK lists show it after its name.
fn version_of(home: &Path) -> String {
    jdk::at_folder(home).map_or_else(|| "JDK".to_owned(), |jdk| jdk.version_string())
}

/// An SDK list's row: its name, its version greyed.
fn sdk_row(name: &str, version: String) -> impl Fn(&mut Window, &mut App) -> AnyElement + use<> {
    let name = SharedString::from(name.to_owned());
    move |_, cx| {
        let muted = Tokens::get(cx).text_secondary;
        h_flex()
            .id(name.clone())
            .role(Role::Label)
            .aria_label(name.clone())
            .gap(px(4.))
            .child(spork_workbench::image("nodes/jdk", cx).size(px(16.)))
            .child(name.clone())
            .child(div().text_color(muted).child(version.clone()))
            .into_any_element()
    }
}

/// IntelliJ's SDK combo box's items (`SdkListModelBuilder`): the table's JDKs, Add SDK ▸
/// (Download JDK…, JDK…), then the JDKs found under "Detected SDKs".
fn sdk_menu(
    mut menu: PopupMenu,
    this: WeakEntity<ProjectStructure>,
    target: SdkTarget,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(dialog) = this.upgrade() else {
        return menu;
    };
    let (table, detected) = {
        let dialog = dialog.read(cx);
        let detected: Vec<Jdk> = dialog
            .detected
            .iter()
            .filter(|jdk| !dialog.draft.sdks.iter().any(|sdk| sdk.home == jdk.home))
            .cloned()
            .collect();
        (dialog.draft.sdks.clone(), detected)
    };
    if let SdkTarget::Module = target {
        let this = this.clone();
        menu = menu.item(PopupMenuItem::new("Project SDK").on_click(move |_, _, cx| {
            this.update(cx, |this, cx| {
                this.draft.module_sdk = None;
                cx.notify();
            })
            .log_err();
        }));
    }
    for sdk in table {
        let (this, home) = (this.clone(), sdk.home.clone());
        menu = menu.item(
            PopupMenuItem::element(sdk_row(&sdk.name, version_of(&sdk.home))).on_click(
                move |_, _, cx| {
                    let home = home.clone();
                    this.update(cx, |this, cx| {
                        match target {
                            SdkTarget::Module => this.draft.module_sdk = Some(home),
                            _ => this.draft.sdk = Some(home),
                        }
                        cx.notify();
                    })
                    .log_err();
                },
            ),
        );
    }
    let add = this.clone();
    menu = menu.submenu("Add SDK", window, cx, move |menu, _, _| {
        add_sdk_items(menu, add.clone(), target, "JDK…")
    });
    if !detected.is_empty() {
        menu = menu.item(PopupMenuItem::label("Detected SDKs"));
        for jdk in detected {
            let this = this.clone();
            let path = shown(
                jdk.home
                    .strip_suffix_path("Contents/Home")
                    .unwrap_or(&jdk.home),
            );
            menu = menu.item(
                PopupMenuItem::element({
                    let version = jdk.version_string();
                    move |_, cx| {
                        let muted = Tokens::get(cx).text_secondary;
                        h_flex()
                            .id(SharedString::from(path.clone()))
                            .role(Role::Label)
                            .aria_label(SharedString::from(path.clone()))
                            .gap(px(4.))
                            .child(spork_workbench::image("nodes/jdk", cx).size(px(16.)))
                            .child(path.clone())
                            .child(div().text_color(muted).child(version.clone()))
                            .into_any_element()
                    }
                })
                .on_click(move |_, window, cx| {
                    this.update(cx, |this, cx| this.add_and_use(&jdk, target, window, cx))
                        .log_err();
                }),
            );
        }
    }
    menu
}

trait StripSuffixPath {
    fn strip_suffix_path(&self, suffix: &str) -> Option<&Path>;
}

impl StripSuffixPath for PathBuf {
    fn strip_suffix_path(&self, suffix: &str) -> Option<&Path> {
        self.ends_with(suffix)
            .then(|| self.ancestors().nth(Path::new(suffix).components().count()))
            .flatten()
    }
}

/// Download JDK… and the JDK from disk, as the SDK combo box's Add SDK and the SDKs page's +
/// offer them (`add` IntelliJ's text for the second: "JDK…" in Add SDK, "Add JDK from disk…").
fn add_sdk_items(
    menu: PopupMenu,
    this: WeakEntity<ProjectStructure>,
    target: SdkTarget,
    add: &'static str,
) -> PopupMenu {
    let download = this.clone();
    menu.item(
        PopupMenuItem::new("Download JDK…").on_click(move |_, window, cx| {
            let this = download.clone();
            crate::jdk_download_dialog::open(
                Rc::new(move |package, target_dir, _, cx| {
                    let this = this.clone();
                    let workspace = this
                        .upgrade()
                        .and_then(|this| this.read(cx).java.read(cx).workspace.upgrade())
                        .map(|workspace| workspace.downgrade());
                    crate::jdk_download::start(
                        package,
                        target_dir,
                        Box::new(move |jdk, cx| {
                            if let Some(this) = this.upgrade() {
                                this.update(cx, |this, cx| {
                                    let ix = this.add_sdk(&jdk);
                                    match target {
                                        SdkTarget::Project => this.draft.sdk = Some(jdk.home),
                                        SdkTarget::Module => this.draft.module_sdk = Some(jdk.home),
                                        SdkTarget::Table => {}
                                    }
                                    this.selected_sdk = Some(ix);
                                    cx.notify();
                                });
                            }
                        }),
                        workspace,
                        cx,
                    )
                }),
                window,
                cx,
            )
        }),
    )
    .item(
        PopupMenuItem::new(add)
            .on_click(move |_, window, cx| add_from_disk(this.clone(), target, window, cx)),
    )
}

/// Add JDK from disk…: the system's folder dialog, then the JDK there, or IntelliJ's error.
fn add_from_disk(
    this: WeakEntity<ProjectStructure>,
    target: SdkTarget,
    window: &mut Window,
    cx: &mut App,
) {
    let picked = spork_workbench::prompt_for_paths(
        gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select Home Directory for JDK".into()),
        },
        cx,
    );
    window
        .spawn(cx, async move |cx| {
            let Ok(Ok(Some(mut paths))) = picked.await else {
                return;
            };
            let Some(folder) = paths.pop() else { return };
            cx.update(|window, cx| match jdk::at_folder(&folder) {
                Some(found) => {
                    this.update(cx, |this, cx| this.add_and_use(&found, target, window, cx))
                        .log_err();
                }
                None => {
                    let answer = window.prompt(
                        gpui::PromptLevel::Critical,
                        "The selected directory is not a valid home for JDK",
                        None,
                        &["OK"],
                        cx,
                    );
                    cx.background_spawn(async move { answer.await.log_err() })
                        .detach();
                }
            })
            .log_err();
        })
        .detach();
}

/// IntelliJ's Language level combo box: its default item ("SDK default" on the Project page,
/// "Project default" on a module's), then its groups of levels.
fn level_combo(
    id: &'static str,
    current: Option<LanguageLevel>,
    default: &'static str,
    this: WeakEntity<ProjectStructure>,
    set: fn(&mut Draft, Option<LanguageLevel>),
) -> AnyElement {
    let label = current.map_or_else(|| default.to_owned(), LanguageLevel::text);
    dropdown(
        ComboBox::new(id, label)
            .aria_label("Language level:")
            .w_full(),
        false,
        move |menu, _, _| {
            // as wide as its longest level, as IntelliJ's list
            let mut menu = menu.max_w(px(640.));
            let choose = |level: Option<LanguageLevel>| {
                let this = this.clone();
                move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                    this.update(cx, |this, cx| {
                        set(&mut this.draft, level);
                        cx.notify();
                    })
                    .log_err();
                }
            };
            menu = menu.item(PopupMenuItem::new(default).on_click(choose(None)));
            for (group, levels) in project_settings::levels() {
                menu = menu.item(PopupMenuItem::label(group));
                for level in levels {
                    menu =
                        menu.item(PopupMenuItem::new(level.text()).on_click(choose(Some(level))));
                }
            }
            menu
        },
    )
}

/// A form's row: its label in the label column, then its control.
fn form_row(label: &'static str, control: impl IntoElement) -> gpui::Div {
    h_flex()
        .w_full()
        .gap(px(8.))
        .child(form_label(label).w(px(LABEL_WIDTH)))
        .child(control)
}

impl ProjectStructure {
    fn sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let header = |text: &'static str| {
            div()
                .px(px(8.))
                .pt(px(6.))
                .whitespace_nowrap()
                .font_weight(gpui::FontWeight::BOLD)
                .child(text)
        };
        let item = |page: Page, cx: &mut Context<Self>| {
            let selected = self.page == page;
            h_flex()
                .id(SharedString::from(format!("structure-{}", page.name())))
                .role(Role::TreeItem)
                .aria_label(page.name())
                .aria_selected(selected)
                .h(px(24.))
                .mx(px(6.))
                .pl(px(24.))
                .rounded(px(4.))
                .cursor_pointer()
                .when(selected, |row| row.bg(tokens.selection))
                .when(!selected, |row| {
                    row.hover(|row| row.bg(tokens.selection_hover))
                })
                .child(page.name())
                .on_click(cx.listener(move |this, _, _, cx| this.show(page, cx)))
        };
        let back = self.history_ix > 0;
        let forward = self.history_ix + 1 < self.history.len();
        v_flex()
            .id("structure-sidebar")
            .role(Role::Tree)
            .aria_label("Project Structure categories")
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .h_full()
            .pt(px(8.))
            .gap(px(2.))
            .border_r_1()
            .border_color(tokens.separator)
            .child(
                h_flex()
                    .px(px(6.))
                    .pb(px(8.))
                    .gap(px(4.))
                    .child(
                        ActionButton::new("structure-back", "general/left")
                            .tooltip("Back")
                            .disabled(!back)
                            .on_click(cx.listener(|this, _, _, cx| this.step(true, cx))),
                    )
                    .child(
                        ActionButton::new("structure-forward", "general/right")
                            .tooltip("Forward")
                            .disabled(!forward)
                            .on_click(cx.listener(|this, _, _, cx| this.step(false, cx))),
                    ),
            )
            .child(header("Project Settings"))
            .child(item(Page::Project, cx))
            .child(item(Page::Modules, cx))
            .child(header("Platform Settings"))
            .child(item(Page::Sdks, cx))
            .into_any_element()
    }

    /// The SDK combo box of the Project page or a module's Dependencies.
    fn sdk_combo(&self, target: SdkTarget, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        let name_of =
            |home: &Path| sdks::named(&self.draft.sdks, home).unwrap_or_else(|| shown(home));
        let (id, label, detail, aria) = match target {
            SdkTarget::Module => match &self.draft.module_sdk {
                Some(home) => ("module-sdk", name_of(home), version_of(home), "Module SDK:"),
                None => (
                    "module-sdk",
                    "Project SDK".to_owned(),
                    self.draft.sdk.as_deref().map(name_of).unwrap_or_default(),
                    "Module SDK:",
                ),
            },
            _ => match &self.draft.sdk {
                Some(home) => ("project-sdk", name_of(home), version_of(home), "SDK:"),
                None => ("project-sdk", "<No SDK>".to_owned(), String::new(), "SDK:"),
            },
        };
        let selected_home = match target {
            SdkTarget::Module => self.draft.module_sdk.clone().or(self.draft.sdk.clone()),
            _ => self.draft.sdk.clone(),
        };
        h_flex()
            .w_full()
            .gap(px(12.))
            .child(
                div().flex_1().min_w_0().child(dropdown(
                    ComboBox::new(id, label)
                        .aria_label(aria)
                        .detail(detail)
                        .w_full(),
                    false,
                    move |menu, window, cx| sdk_menu(menu, this.clone(), target, window, cx),
                )),
            )
            .child(
                spork_workbench::Button::new(SharedString::from(format!("{id}-edit")), "Edit")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let ix = selected_home.as_ref().and_then(|home| {
                            this.draft.sdks.iter().position(|sdk| &sdk.home == home)
                        });
                        if let Some(ix) = ix {
                            this.select_sdk(ix, window, cx);
                        }
                        this.show(Page::Sdks, cx);
                    })),
            )
            .into_any_element()
    }

    fn project_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let this = cx.entity().downgrade();
        let comment = |text: &'static str| {
            div()
                .text_ui_small()
                .text_color(tokens.text_secondary)
                .child(text)
        };
        v_flex()
            .id("structure-project")
            .role(Role::Group)
            .aria_label("Project")
            .p(px(20.))
            .gap(px(12.))
            .child(
                v_flex()
                    .gap(px(4.))
                    .child(div().font_weight(gpui::FontWeight::BOLD).child("Project"))
                    .child(comment(
                        "Default settings for all modules. Configure these parameters for each module on the module page as needed.",
                    )),
            )
            .child(form_row(
                "Name:",
                div().w(px(240.)).child(
                    spork_workbench::TextField::new(&self.name).aria_label("Name:"),
                ),
            ))
            .child(form_row(
                "SDK:",
                div()
                    .w(px(FIELD_WIDTH))
                    .child(self.sdk_combo(SdkTarget::Project, cx)),
            ))
            .child(form_row(
                "Language level:",
                div().w(px(FIELD_WIDTH)).child(level_combo(
                    "project-language-level",
                    self.draft.level,
                    "SDK default",
                    this,
                    |draft, level| draft.level = level,
                )),
            ))
            .child(
                v_flex()
                    .gap(px(4.))
                    .child(form_row(
                        "Compiler output:",
                        div().w(px(FIELD_WIDTH)).child(
                            spork_workbench::TextField::new(&self.output)
                                .aria_label("Compiler output:")
                                .suffix(self.browse("output-browse", self.output.clone())),
                        ),
                    ))
                    .child(div().pl(px(LABEL_WIDTH + 8.)).child(comment(
                        "Used for module subdirectories, Production and Test directories for the corresponding sources.",
                    ))),
            )
            .into_any_element()
    }

    /// A field's folder button, which sets it to the folder picked.
    fn browse(&self, id: &'static str, field: Entity<InputState>) -> impl IntoElement + use<> {
        ActionButton::new(id, "nodes/folder")
            .tooltip("Browse")
            .on_click(move |_, window, cx| {
                let picked = spork_workbench::prompt_for_paths(
                    gpui::PathPromptOptions {
                        files: false,
                        directories: true,
                        multiple: false,
                        prompt: None,
                    },
                    cx,
                );
                let field = field.clone();
                window
                    .spawn(cx, async move |cx| {
                        let Ok(Ok(Some(mut paths))) = picked.await else {
                            return;
                        };
                        let Some(folder) = paths.pop() else { return };
                        cx.update(|window, cx| {
                            field
                                .update(cx, |field, cx| field.set_value(shown(&folder), window, cx))
                        })
                        .log_err();
                    })
                    .detach();
            })
    }

    /// A page's middle column: its toolbar over its list.
    fn list_column(
        &self,
        toolbar: AnyElement,
        rows: Vec<AnyElement>,
        label: &'static str,
    ) -> gpui::Stateful<gpui::Div> {
        v_flex()
            .id(label)
            .w(px(LIST_WIDTH))
            .flex_none()
            .h_full()
            .child(toolbar)
            .child(
                v_flex()
                    .id(SharedString::from(format!("{label}-list")))
                    .role(Role::Tree)
                    .aria_label(label)
                    .flex_1()
                    .overflow_y_scroll()
                    .children(rows),
            )
    }

    fn list_row(
        id: SharedString,
        label: SharedString,
        icon: &'static str,
        selected: bool,
        cx: &App,
    ) -> gpui::Stateful<gpui::Div> {
        let tokens = Tokens::get(cx);
        h_flex()
            .id(id)
            .role(Role::TreeItem)
            .aria_label(label.clone())
            .aria_selected(selected)
            .h(px(24.))
            .mx(px(6.))
            .px(px(6.))
            .gap(px(4.))
            .rounded(px(4.))
            .cursor_pointer()
            .when(selected, |row| row.bg(tokens.selection))
            .when(!selected, |row| {
                row.hover(|row| row.bg(tokens.selection_hover))
            })
            .child(spork_workbench::image(icon, cx).size(px(16.)))
            .child(label)
    }

    fn modules_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let module = SharedString::from(self.draft.name.clone());
        let toolbar = h_flex()
            .h(px(32.))
            .px(px(6.))
            .gap(px(4.))
            .child(
                ActionButton::new("module-add", "general/add")
                    .tooltip("Add")
                    .disabled(true),
            )
            .child(
                ActionButton::new("module-remove", "general/remove")
                    .tooltip("Remove")
                    .disabled(true),
            )
            .child(
                ActionButton::new("module-copy", "general/copy")
                    .tooltip("Copy")
                    .disabled(true),
            )
            .into_any_element();
        let rows = vec![
            Self::list_row(
                "module-row".into(),
                module.clone(),
                "nodes/module",
                true,
                cx,
            )
            .into_any_element(),
        ];
        let tab =
            |id: &'static str, label: &'static str, tab: ModuleTab, cx: &mut Context<Self>| {
                spork_workbench::shell::header_tab(id, label, self.module_tab == tab, cx)
                    .role(Role::Tab)
                    .aria_label(label)
                    .aria_selected(self.module_tab == tab)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.module_tab = tab;
                        cx.notify();
                    }))
            };
        let this = cx.entity().downgrade();
        let body = match self.module_tab {
            ModuleTab::Sources => form_row(
                "Language level:",
                div().flex_1().min_w_0().child(level_combo(
                    "module-language-level",
                    self.draft.module_level,
                    "Project default",
                    this,
                    |draft, level| draft.module_level = level,
                )),
            )
            .into_any_element(),
            ModuleTab::Dependencies => form_row(
                "Module SDK:",
                div()
                    .flex_1()
                    .min_w_0()
                    .child(self.sdk_combo(SdkTarget::Module, cx)),
            )
            .into_any_element(),
        };
        h_flex()
            .size_full()
            .items_start()
            .child(
                self.list_column(toolbar, rows, "Modules")
                    .border_r_1()
                    .border_color(tokens.separator),
            )
            .child(
                v_flex()
                    .id("structure-module")
                    .role(Role::Group)
                    .aria_label(module)
                    .flex_1()
                    .min_w_0()
                    .p(px(16.))
                    .gap(px(12.))
                    .child(form_row(
                        "Name:",
                        div()
                            .w(px(240.))
                            .child(spork_workbench::TextField::new(&self.name).aria_label("Name:")),
                    ))
                    .child(
                        h_flex()
                            .gap(px(4.))
                            .border_b_1()
                            .border_color(tokens.separator)
                            .child(tab("module-sources", "Sources", ModuleTab::Sources, cx))
                            .child(tab(
                                "module-dependencies",
                                "Dependencies",
                                ModuleTab::Dependencies,
                                cx,
                            )),
                    )
                    .child(body),
            )
            .into_any_element()
    }

    fn sdks_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let this = cx.entity().downgrade();
        let toolbar = h_flex()
            .h(px(32.))
            .px(px(6.))
            .gap(px(4.))
            .child(
                ActionButton::new("sdk-add", "general/add")
                    .tooltip("Add New SDK")
                    .dropdown_menu(move |menu, _, _| {
                        add_sdk_items(menu, this.clone(), SdkTarget::Table, "Add JDK from disk…")
                    }),
            )
            .child(
                ActionButton::new("sdk-remove", "general/remove")
                    .tooltip("Remove")
                    .disabled(self.selected_sdk.is_none())
                    .on_click(cx.listener(|this, _, window, cx| this.remove_sdk(window, cx))),
            )
            .into_any_element();
        let rows = self
            .draft
            .sdks
            .iter()
            .enumerate()
            .map(|(ix, sdk)| {
                Self::list_row(
                    SharedString::from(format!("sdk-{ix}")),
                    SharedString::from(sdk.name.clone()),
                    "nodes/jdk",
                    self.selected_sdk == Some(ix),
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| this.select_sdk(ix, window, cx)))
                .into_any_element()
            })
            .collect();
        let details = match self.selected_sdk {
            Some(_) => v_flex()
                .id("structure-sdk")
                .role(Role::Group)
                .aria_label("SDK")
                .flex_1()
                .min_w_0()
                .p(px(16.))
                .gap(px(12.))
                .child(form_row(
                    "Name:",
                    div()
                        .w(px(240.))
                        .child(spork_workbench::TextField::new(&self.sdk_name).aria_label("Name:")),
                ))
                .child(form_row(
                    "JDK home path:",
                    div().flex_1().min_w_0().child(
                        spork_workbench::TextField::new(&self.sdk_home)
                            .aria_label("JDK home path:")
                            .suffix(self.browse("sdk-home-browse", self.sdk_home.clone())),
                    ),
                ))
                .into_any_element(),
            None => div()
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(tokens.text_secondary)
                .child("Select a setting to view or edit its details here")
                .into_any_element(),
        };
        h_flex()
            .size_full()
            .items_start()
            .child(
                self.list_column(toolbar, rows, "SDKs")
                    .border_r_1()
                    .border_color(tokens.separator),
            )
            .child(details)
            .into_any_element()
    }

    fn buttons(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        h_flex()
            .flex_none()
            .px(px(12.))
            .py(px(10.))
            .gap(px(12.))
            .border_t_1()
            .border_color(tokens.separator)
            .child(spork_workbench::controls::help_button("structure-help", cx))
            .child(div().flex_1())
            .child(
                spork_workbench::Button::new("structure-cancel", "Cancel")
                    .on_click(cx.listener(|this, _, window, _| this.close(window))),
            )
            .child(
                spork_workbench::Button::new("structure-apply", "Apply")
                    .disabled(!self.modified())
                    .on_click(cx.listener(|this, _, _, cx| this.apply(cx))),
            )
            .child(
                spork_workbench::Button::new("structure-ok", "OK")
                    .primary(true)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.apply(cx);
                        this.close(window);
                    })),
            )
            .into_any_element()
    }
}

impl Render for ProjectStructure {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let page = match self.page {
            Page::Project => self.project_page(cx),
            Page::Modules => self.modules_page(cx),
            Page::Sdks => self.sdks_page(cx),
        };
        v_flex()
            .id("project-structure")
            .key_context("ProjectStructure")
            .track_focus(&self.focus)
            .size_full()
            .bg(tokens.panel)
            .text_color(tokens.text)
            .text_ui()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.close(window);
                    cx.stop_propagation();
                }
            }))
            .child(spork_workbench::shell::window_title(TITLE, cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(self.sidebar(cx))
                    .child(div().flex_1().min_w_0().h_full().child(page)),
            )
            .child(self.buttons(cx))
    }
}

/// What the end-to-end tests read of the open dialog: its page, the project's SDK and language
/// level as edited, and the SDK table's names.
pub(crate) fn e2e_state(cx: &App) -> serde_json::Value {
    let Some(dialog) = cx.try_global::<Shown>().and_then(|shown| shown.0.upgrade()) else {
        return serde_json::Value::Null;
    };
    let dialog = dialog.read(cx);
    serde_json::json!({
        "page": dialog.page.name(),
        "sdk": dialog.draft.sdk.as_deref().and_then(|home| sdks::named(&dialog.draft.sdks, home)),
        "level": dialog.draft.level.map(LanguageLevel::text),
        "module_level": dialog.draft.module_level.map(LanguageLevel::text),
        "sdks": dialog.draft.sdks.iter().map(|sdk| sdk.name.clone()).collect::<Vec<_>>(),
        "modified": dialog.modified(),
    })
}
