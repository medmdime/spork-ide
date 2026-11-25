//! Settings › Build, Execution, Deployment › Compiler › Kotlin Compiler (IntelliJ's
//! `KotlinCompilerConfigurableTab`): its fields in IntelliJ's order and texts. A Gradle or Maven
//! project that builds Kotlin gets the Kotlin version, Language version, API version and Target
//! JVM version in its build file (`build_files`), then is synced; the options IntelliJ's own
//! builder alone reads are kept for the project in the data folder.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{
    AnyView, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Role, StatefulInteractiveElement as _, Styled as _, Subscription,
    Window, div, px,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::{h_flex, v_flex};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use spork_workbench::controls::{ComboBox, dropdown, form_label, labeled_check};
use spork_workbench::{Tokens, Typography as _};
use util::ResultExt as _;

use crate::build_files::{self, KotlinOptions};
use crate::java_project::{JavaProject, Tool};

/// The language and API versions the bundled compiler supports (Kotlin's stable
/// `LanguageVersion`s, oldest first, as IntelliJ's combo box lists them).
const VERSIONS: [&str; 5] = ["1.9", "2.0", "2.1", "2.2", "2.3"];
/// The newest of them, the compiler's default.
const LATEST: &str = "2.3";
/// Kotlin's `JvmTarget`s, as IntelliJ's Target JVM version lists them; 1.8 is the default.
const JVM_TARGETS: [&str; 18] = [
    "1.8", "9", "10", "11", "12", "13", "14", "15", "16", "17", "18", "19", "20", "21", "22", "23",
    "24", "25",
];
/// Embed source code into source map's choices.
const EMBEDDINGS: [&str; 3] = [
    "Never",
    "Always",
    "When inlining a function from other module with embedded sources",
];
/// Module kind's choices.
const MODULE_KINDS: [&str; 4] = [
    "Plain (put to global scope)",
    "AMD",
    "CommonJS",
    "UMD (detect AMD or CommonJS if available, fallback to plain)",
];

/// What the page shows: the build's options and the ones IntelliJ's builder alone reads, kept
/// in the data folder (IntelliJ's `kotlinc.xml`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct KotlinSettings {
    report_warnings: bool,
    version: String,
    language_version: String,
    api_version: String,
    additional_arguments: String,
    keep_alive: bool,
    jvm_incremental: bool,
    jvm_target: String,
    js_incremental: bool,
    source_maps: bool,
    source_map_prefix: String,
    embed_sources: String,
    copy_runtime: bool,
    destination: String,
    module_kind: String,
}

impl Default for KotlinSettings {
    fn default() -> Self {
        Self {
            report_warnings: true,
            version: String::new(),
            language_version: LATEST.into(),
            api_version: LATEST.into(),
            additional_arguments: String::new(),
            keep_alive: true,
            jvm_incremental: true,
            jvm_target: "1.8".into(),
            js_incremental: true,
            source_maps: false,
            source_map_prefix: String::new(),
            embed_sources: EMBEDDINGS[2].into(),
            copy_runtime: true,
            destination: "lib".into(),
            module_kind: MODULE_KINDS[0].into(),
        }
    }
}

fn file(root: &Path) -> PathBuf {
    crate::jdtls::project_data(root, "kotlinc").with_extension("json")
}

/// The build file of a project that builds Kotlin, and its tool.
fn kotlin_build(root: &Path) -> Option<(Tool, PathBuf)> {
    let tool = Tool::detect(root)?;
    let path = crate::project_settings::build_file(root, tool)?;
    let text = std::fs::read_to_string(&path).ok()?;
    let options = read_options(tool, &text);
    options.version.is_some().then_some((tool, path))
}

fn read_options(tool: Tool, text: &str) -> KotlinOptions {
    match tool {
        Tool::Maven => build_files::maven_kotlin(text),
        Tool::Gradle => build_files::gradle_kotlin(text),
    }
}

/// The `major.minor` language version of a Kotlin version: `2.1.0` → `2.1`.
fn language_of(version: &str) -> Option<String> {
    let mut parts = version.split('.');
    Some(format!("{}.{}", parts.next()?, parts.next()?))
}

fn load(root: Option<&Path>) -> KotlinSettings {
    let Some(root) = root else {
        return KotlinSettings::default();
    };
    let mut settings: KotlinSettings = std::fs::read_to_string(file(root))
        .ok()
        .and_then(|text| serde_json::from_str(&text).log_err())
        .unwrap_or_default();
    if let Some((tool, path)) = kotlin_build(root)
        && let Ok(text) = std::fs::read_to_string(path)
    {
        let options = read_options(tool, &text);
        let default = options.version.as_deref().and_then(language_of);
        settings.version = options.version.clone().unwrap_or_default();
        settings.language_version = options
            .language_version
            .or(default.clone())
            .unwrap_or_else(|| LATEST.into());
        settings.api_version = options
            .api_version
            .or(default)
            .unwrap_or_else(|| LATEST.into());
        if let Some(jvm_target) = options.jvm_target {
            settings.jvm_target = jvm_target;
        }
    }
    settings
}

/// The page's view, for the Settings dialog.
pub struct KotlinCompilerPage {
    root: Option<PathBuf>,
    saved: KotlinSettings,
    edited: KotlinSettings,
    version: Entity<InputState>,
    arguments: Entity<InputState>,
    prefix: Entity<InputState>,
    destination: Entity<InputState>,
    changed: Rc<dyn Fn(&mut App)>,
    _subscriptions: Vec<Subscription>,
}

/// Registers the page with the Settings dialog.
pub(crate) fn init(cx: &mut App) {
    spork_settings::register_page(spork_settings::KOTLIN_COMPILER_PAGE, make, cx);
}

fn make(
    root: Option<PathBuf>,
    changed: Rc<dyn Fn(&mut App)>,
    window: &mut Window,
    cx: &mut App,
) -> Rc<dyn spork_settings::ExternalPage> {
    let page = cx.new(|cx| KotlinCompilerPage::new(root, changed, window, cx));
    Rc::new(Page(page))
}

impl KotlinCompilerPage {
    fn new(
        root: Option<PathBuf>,
        changed: Rc<dyn Fn(&mut App)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let saved = load(root.as_deref());
        let field = |text: &str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut input = InputState::new(window, cx);
                input.set_value(text.to_owned(), window, cx);
                input
            })
        };
        let version = field(&saved.version, window, cx);
        let arguments = field(&saved.additional_arguments, window, cx);
        let prefix = field(&saved.source_map_prefix, window, cx);
        let destination = field(&saved.destination, window, cx);
        let edits = |input: &Entity<InputState>,
                     set: fn(&mut KotlinSettings, String),
                     window: &mut Window,
                     cx: &mut Context<Self>| {
            cx.subscribe_in(input, window, move |this, input, event, _, cx| {
                if let InputEvent::Change = event {
                    set(&mut this.edited, input.read(cx).value().to_string());
                    this.changed(cx);
                }
            })
        };
        let _subscriptions = vec![
            edits(&version, |s, text| s.version = text, window, cx),
            edits(
                &arguments,
                |s, text| s.additional_arguments = text,
                window,
                cx,
            ),
            edits(&prefix, |s, text| s.source_map_prefix = text, window, cx),
            edits(&destination, |s, text| s.destination = text, window, cx),
        ];
        Self {
            root,
            edited: saved.clone(),
            saved,
            version,
            arguments,
            prefix,
            destination,
            changed,
            _subscriptions,
        }
    }

    fn changed(&self, cx: &mut Context<Self>) {
        cx.notify();
        let changed = self.changed.clone();
        cx.defer(move |cx| changed(cx));
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        if self.edited == self.saved {
            return;
        }
        let Some(root) = self.root.clone() else {
            self.saved = self.edited.clone();
            return;
        };
        let (edited, saved) = (self.edited.clone(), self.saved.clone());
        let mut written = false;
        if let Some((tool, path)) = kotlin_build(&root) {
            let changed = |new: &String, old: &String| (new != old).then(|| new.clone());
            let options = KotlinOptions {
                language_version: changed(&edited.language_version, &saved.language_version),
                api_version: changed(&edited.api_version, &saved.api_version),
                jvm_target: changed(&edited.jvm_target, &saved.jvm_target),
                version: changed(&edited.version, &saved.version)
                    .filter(|version| !version.trim().is_empty()),
            };
            if options != KotlinOptions::default()
                && let Some(text) = std::fs::read_to_string(&path).log_err()
            {
                let result = match tool {
                    Tool::Maven => build_files::set_maven_kotlin(&text, &options),
                    Tool::Gradle => Ok(build_files::set_gradle_kotlin(&text, &options)),
                };
                if let Some(text) = result.log_err() {
                    written = std::fs::write(&path, text).log_err().is_some();
                }
            }
        }
        if let Some(folder) = file(&root).parent() {
            std::fs::create_dir_all(folder).log_err();
        }
        if let Some(text) = serde_json::to_string_pretty(&edited).log_err() {
            std::fs::write(file(&root), text).log_err();
        }
        if written && let Some(java) = JavaProject::containing(&root, cx) {
            java.update(cx, |java, cx| {
                if java.linked {
                    java.reload(cx);
                }
            });
        }
        self.saved = edited;
        cx.notify();
    }

    fn check(
        &self,
        id: &'static str,
        label: &'static str,
        on: bool,
        set: fn(&mut KotlinSettings),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        labeled_check(id, label, on, cx).on_click(cx.listener(move |this, _, _, cx| {
            set(&mut this.edited);
            this.changed(cx);
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn choice(
        &self,
        id: &'static str,
        label: &'static str,
        depth: usize,
        current: &str,
        choices: &'static [&'static str],
        set: fn(&mut KotlinSettings, &'static str),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let this = cx.entity().downgrade();
        row(
            label,
            depth,
            dropdown(
                ComboBox::new(id, current.to_owned())
                    .aria_label(label)
                    .w_full(),
                false,
                move |menu, _, _| {
                    choices.iter().fold(menu, |menu, choice| {
                        let this = this.clone();
                        menu.item(PopupMenuItem::new(*choice).on_click(move |_, _, cx| {
                            this.update(cx, |this, cx| {
                                set(&mut this.edited, choice);
                                this.changed(cx);
                            })
                            .log_err();
                        }))
                    })
                },
            ),
        )
    }
}

/// IntelliJ's page puts its controls in a column after the labels' widest.
const LABEL_WIDTH: f32 = 300.;
const INDENT: f32 = 24.;

/// A row of `depth` groups in, its control in the page's one column.
fn row(label: &'static str, depth: usize, control: impl IntoElement) -> gpui::Div {
    h_flex()
        .w_full()
        .gap(px(8.))
        .child(form_label(label).w(px(LABEL_WIDTH - INDENT * depth as f32)))
        .child(div().flex_1().min_w_0().max_w(px(320.)).child(control))
}

/// IntelliJ's titled separator over a group.
fn title(text: &'static str, cx: &App) -> gpui::Stateful<gpui::Div> {
    h_flex()
        .id(text)
        .role(Role::Label)
        .aria_label(text)
        .w_full()
        .pt(px(14.))
        .gap(px(8.))
        .child(text)
        .child(div().flex_1().h(px(1.)).bg(Tokens::get(cx).separator))
}

impl Render for KotlinCompilerPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let edited = self.edited.clone();
        let field = |input: &Entity<InputState>, label: &'static str| {
            spork_workbench::TextField::new(input).aria_label(label)
        };
        v_flex()
            .id("kotlin-compiler")
            .role(Role::Group)
            .aria_label("Kotlin Compiler")
            .size_full()
            .px(px(16.))
            .pb(px(16.))
            .gap(px(9.))
            .overflow_y_scroll()
            .text_ui()
            .child(self.check(
                "kotlin-report-warnings",
                "Report compiler warnings",
                edited.report_warnings,
                |s| s.report_warnings = !s.report_warnings,
                cx,
            ))
            .child(row(
                "Kotlin compiler version",
                0,
                field(&self.version, "Kotlin compiler version"),
            ))
            .child(self.choice(
                "kotlin-language-version",
                "Language version",
                0,
                &edited.language_version,
                &VERSIONS,
                |s, choice| s.language_version = choice.into(),
                cx,
            ))
            .child(self.choice(
                "kotlin-api-version",
                "API version",
                0,
                &edited.api_version,
                &VERSIONS,
                |s, choice| s.api_version = choice.into(),
                cx,
            ))
            .child(row(
                "Additional command line parameters:",
                0,
                field(&self.arguments, "Additional command line parameters:"),
            ))
            .child(self.check(
                "kotlin-keep-alive",
                "Keep compiler process alive between invocations",
                edited.keep_alive,
                |s| s.keep_alive = !s.keep_alive,
                cx,
            ))
            .child(title("Kotlin to JVM", cx))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap(px(9.))
                    .child(self.check(
                        "kotlin-jvm-incremental",
                        "Enable incremental compilation",
                        edited.jvm_incremental,
                        |s| s.jvm_incremental = !s.jvm_incremental,
                        cx,
                    ))
                    .child(self.choice(
                        "kotlin-jvm-target",
                        "Target JVM version",
                        1,
                        &edited.jvm_target,
                        &JVM_TARGETS,
                        |s, choice| s.jvm_target = choice.into(),
                        cx,
                    )),
            )
            .child(title("Kotlin to JavaScript", cx))
            .child(
                v_flex()
                    .pl(px(INDENT))
                    .gap(px(9.))
                    .child(self.check(
                        "kotlin-js-incremental",
                        "Enable incremental compilation",
                        edited.js_incremental,
                        |s| s.js_incremental = !s.js_incremental,
                        cx,
                    ))
                    .child(self.check(
                        "kotlin-source-maps",
                        "Generate source maps",
                        edited.source_maps,
                        |s| s.source_maps = !s.source_maps,
                        cx,
                    ))
                    .child(h_flex().pl(px(LABEL_WIDTH - INDENT + 8.)).child(
                        div().w(px(320.)).child(
                            field(&self.prefix, "Source map prefix").disabled(!edited.source_maps),
                        ),
                    ))
                    .child(self.choice(
                        "kotlin-embed-sources",
                        "Embed source code into source map:",
                        1,
                        &edited.embed_sources,
                        &EMBEDDINGS,
                        |s, choice| s.embed_sources = choice.into(),
                        cx,
                    ))
                    .child(self.check(
                        "kotlin-copy-runtime",
                        "Copy library runtime files",
                        edited.copy_runtime,
                        |s| s.copy_runtime = !s.copy_runtime,
                        cx,
                    ))
                    .child(
                        div().pl(px(INDENT)).child(row(
                            "Destination directory",
                            2,
                            field(&self.destination, "Destination directory")
                                .disabled(!edited.copy_runtime),
                        )),
                    )
                    .child(self.choice(
                        "kotlin-module-kind",
                        "Module kind:",
                        1,
                        &edited.module_kind,
                        &MODULE_KINDS,
                        |s, choice| s.module_kind = choice.into(),
                        cx,
                    )),
            )
    }
}

/// The page as the Settings dialog holds it.
struct Page(Entity<KotlinCompilerPage>);

impl spork_settings::ExternalPage for Page {
    fn view(&self) -> AnyView {
        self.0.clone().into()
    }

    fn modified(&self, cx: &App) -> bool {
        let page = self.0.read(cx);
        page.edited != page.saved
    }

    fn apply(&self, cx: &mut App) {
        self.0.update(cx, |page, cx| page.apply(cx));
    }

    fn e2e_state(&self, cx: &App) -> Value {
        let page = self.0.read(cx);
        serde_json::json!({
            "version": page.edited.version,
            "language_version": page.edited.language_version,
            "api_version": page.edited.api_version,
            "jvm_target": page.edited.jvm_target,
        })
    }
}
