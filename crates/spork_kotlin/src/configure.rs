//! IntelliJ's "Kotlin is not configured" banner over a Kotlin file of a Gradle or Maven project
//! whose build doesn't declare Kotlin (`KotlinSetupEnvironmentNotificationProvider`, a warning
//! `EditorNotificationPanel`), its Ignore, and its Configure: the build tool's configurator (the
//! only one, so no Choose Configurator popup) and its dialog, "Configure Kotlin with Gradle", which
//! adds Kotlin to the build as IntelliJ's configurators write it.

use std::path::{Path, PathBuf};

use editor::Editor;
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Role, StatefulInteractiveElement as _, Styled as _, Subscription,
    Window, div, px,
};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use spork_java::{JavaProject, Tool};
use spork_workbench::Tokens;
use spork_workbench::controls::{dialog_title, disabled_radio, labeled_radio};
use ui::DynamicSpacing;
use util::ResultExt as _;
use workspace::item::ItemHandle;
use workspace::{Pane, ToolbarItemEvent, ToolbarItemLocation, ToolbarItemView, Workspace};

/// The Kotlin version the configurators write, the one Spork IDE's Kotlin is built against.
// shortcut: IntelliJ lists the Kotlin versions it reads from Maven Central; list them once Spork
// IDE reads the repository's metadata
const KOTLIN_VERSION: &str = "2.4.10";

/// The text `KotlinProjectConfigurationBundle` gives the banner (`kotlin.not.configured`).
const NOT_CONFIGURED: &str = "Kotlin is not configured";

/// Adds the banner to the workspace's panes, and to each pane added later.
pub(crate) fn register(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    for pane in workspace.panes().to_vec() {
        add_to(&pane, window, cx);
    }
    cx.subscribe_in(&cx.entity(), window, |_, _, event, window, cx| {
        if let workspace::Event::PaneAdded(pane) = event {
            add_to(pane, window, cx);
        }
    })
    .detach();
}

fn add_to(pane: &Entity<Pane>, window: &mut Window, cx: &mut Context<Workspace>) {
    let banner = cx.new(|_| KotlinBanner {
        java: None,
        file: None,
        _subscription: None,
    });
    pane.update(cx, |pane, cx| {
        pane.toolbar()
            .update(cx, |toolbar, cx| toolbar.add_item(banner, window, cx))
    });
}

/// The projects whose banner was ignored (IntelliJ's suppressed modules), this session.
#[derive(Default)]
struct Ignored(Vec<PathBuf>);

impl gpui::Global for Ignored {}

pub(crate) struct KotlinBanner {
    /// The Java project of the active editor's Kotlin file.
    java: Option<Entity<JavaProject>>,
    file: Option<PathBuf>,
    _subscription: Option<Subscription>,
}

impl KotlinBanner {
    /// The project's folder and build tool, while its build declares no Kotlin.
    fn unconfigured(&self, cx: &App) -> Option<(PathBuf, Tool)> {
        let java = self.java.as_ref()?.read(cx);
        let (root, tool) = (java.root.clone()?, java.tool?);
        let ignored = cx
            .try_global::<Ignored>()
            .is_some_and(|ignored| ignored.0.contains(&root));
        (!ignored && !crate::kotlin_project::declares_kotlin(&root)).then_some((root, tool))
    }

    fn location(&self, cx: &App) -> ToolbarItemLocation {
        match self.unconfigured(cx) {
            Some(_) => ToolbarItemLocation::Secondary,
            None => ToolbarItemLocation::Hidden,
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        cx.emit(ToolbarItemEvent::ChangeLocation(self.location(cx)));
        cx.notify();
    }
}

impl EventEmitter<ToolbarItemEvent> for KotlinBanner {}

impl ToolbarItemView for KotlinBanner {
    fn set_active_pane_item(
        &mut self,
        item: Option<&dyn ItemHandle>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> ToolbarItemLocation {
        self.file = item
            .and_then(|item| item.act_as::<Editor>(cx))
            .and_then(|editor| editor.read(cx).buffer().read(cx).as_singleton())
            .and_then(|buffer| {
                let buffer = buffer.read(cx);
                let file = buffer.file()?;
                (file.path().extension() == Some("kt"))
                    .then(|| file.as_local().map(|file| file.abs_path(cx)))?
            });
        self.java = self
            .file
            .as_deref()
            .and_then(|path| JavaProject::containing(path, cx));
        self._subscription = self
            .java
            .as_ref()
            .map(|java| cx.observe(java, |this, _, cx| this.refresh(cx)));
        self.location(cx)
    }
}

impl Render for KotlinBanner {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((root, tool)) = self.unconfigured(cx) else {
            return div().into_any_element();
        };
        let tokens = Tokens::get(cx);
        let this = cx.entity().downgrade();
        let configure = {
            let (root, this) = (root.clone(), this.clone());
            spork_workbench::controls::link("kotlin-configure", "Configure", cx).on_click(
                move |_, window, cx| {
                    let this = this.clone();
                    open_dialog(
                        root.clone(),
                        tool,
                        move |cx| {
                            this.update(cx, |this, cx| this.refresh(cx)).log_err();
                        },
                        window,
                        cx,
                    )
                },
            )
        };
        let ignore = spork_workbench::controls::link("kotlin-ignore", "Ignore", cx).on_click(
            move |_, _, cx| {
                cx.default_global::<Ignored>().0.push(root.clone());
                this.update(cx, |this, cx| this.refresh(cx)).log_err();
            },
        );
        // the toolbar's padding around it, taken back: the banner spans the editor
        let (padding_x, padding_y) = (
            DynamicSpacing::Base08.rems(cx),
            DynamicSpacing::Base06.rems(cx),
        );
        spork_vcs::spork_look(cx)
            .child(
                h_flex()
                    .id("kotlin-banner")
                    .role(Role::Group)
                    .aria_label(NOT_CONFIGURED)
                    .mx(-padding_x)
                    .my(-padding_y)
                    .py(px(10.))
                    .px(px(12.))
                    .gap(px(8.))
                    .bg(tokens.banner_warning)
                    .border_b_1()
                    .border_color(tokens.banner_warning_border)
                    .text_color(tokens.text)
                    .child(spork_workbench::image("status/warning", cx).size(px(16.)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .pr(px(20.))
                            .child(NOT_CONFIGURED),
                    )
                    .child(configure)
                    .child(ignore),
            )
            .into_any_element()
    }
}

/// IntelliJ's configurator dialog (`ConfigureDialogWithModulesAndVersion`): "Configure Kotlin
/// with Gradle", the modules to configure (a project of one module: All modules), the Kotlin
/// version, OK and Cancel.
// shortcut: a project's one module only, as Spork IDE imports one; offer Single module and All
// modules containing Kotlin files once it imports several
fn open_dialog(
    root: PathBuf,
    tool: Tool,
    on_configured: impl Fn(&mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let title = format!("Configure Kotlin with {}", tool.name());
    let module = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let on_configured = std::rc::Rc::new(on_configured);
    window.open_dialog(cx, move |dialog, _, cx| {
        let (root, on_configured) = (root.clone(), on_configured.clone());
        let row = |label: &'static str, control: gpui::AnyElement| {
            h_flex()
                .gap(px(8.))
                .child(div().flex_none().child(label))
                .child(div().flex_1().min_w_0().child(control))
        };
        dialog
            .title(dialog_title(title.clone()))
            .on_ok(|_, _, _| false)
            .w(px(460.))
            .close_button(false)
            .child(
                v_flex()
                    .id("configure-kotlin-dialog")
                    .role(Role::Group)
                    .aria_label(title.clone())
                    .gap(px(8.))
                    .child(labeled_radio("kotlin-all-modules", "All modules", true, cx))
                    .child(disabled_radio(
                        "kotlin-single-module",
                        "Single module:",
                        false,
                        cx,
                    ))
                    .child(disabled_radio(
                        "kotlin-modules-with-kotlin",
                        "All modules containing Kotlin files:",
                        false,
                        cx,
                    ))
                    .child(
                        div()
                            .pl(px(24.))
                            .text_color(Tokens::get(cx).text_disabled)
                            .child(module.clone()),
                    )
                    .child(div().h(px(4.)))
                    .child(row(
                        "Kotlin compiler and runtime version:",
                        spork_workbench::controls::labeled_combo(
                            "kotlin-version",
                            "Kotlin compiler and runtime version:",
                            KOTLIN_VERSION,
                            0.,
                            false,
                            |menu, _, _| {
                                menu.item(gpui_kit::component::menu::PopupMenuItem::new(
                                    KOTLIN_VERSION,
                                ))
                            },
                        ),
                    ))
                    .child(
                        h_flex()
                            .pt(px(12.))
                            .gap(px(8.))
                            .justify_end()
                            .child(
                                spork_workbench::Button::new("dialog-cancel", "Cancel")
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            )
                            .child(
                                spork_workbench::Button::new("dialog-ok", "OK")
                                    .primary(true)
                                    .on_click(move |_, window, cx| {
                                        window.close_dialog(cx);
                                        configure(&root, tool, cx).log_err();
                                        on_configured(cx);
                                    }),
                            ),
                    ),
            )
    });
}

/// The project's JDK's feature release (`17`), which the Gradle build's toolchain and Maven's
/// JVM target name; IntelliJ's module SDK's.
fn jvm_release(root: &Path, cx: &App) -> String {
    JavaProject::containing(root, cx)
        .and_then(|java| java.read(cx).jdk.clone())
        .map(|jdk| jdk.major.to_string())
        .unwrap_or_else(|| "17".into())
}

/// Adds Kotlin to the project's build file, as IntelliJ's configurator for its build tool does.
fn configure(root: &Path, tool: Tool, cx: &App) -> anyhow::Result<()> {
    let jvm = jvm_release(root, cx);
    let (file, edit): (PathBuf, fn(&str, &str, &str) -> String) = match tool {
        Tool::Maven => (root.join("pom.xml"), configure_pom),
        Tool::Gradle if root.join("build.gradle.kts").is_file() => {
            (root.join("build.gradle.kts"), configure_gradle_kts)
        }
        Tool::Gradle => (root.join("build.gradle"), configure_gradle_groovy),
    };
    let text = std::fs::read_to_string(&file)?;
    std::fs::write(&file, edit(&text, KOTLIN_VERSION, &jvm))?;
    Ok(())
}

/// Inserts `line` as the last line of the top-level block `name { … }`, or adds the block, at the
/// start (`plugins`) or the end.
fn into_block(text: &str, name: &str, line: &str, at_start: bool) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let open = format!("{name} {{");
    if let Some(start) = lines.iter().position(|l| l.trim_end() == open)
        && let Some(end) = lines[start..].iter().position(|l| l == "}")
    {
        lines.insert(start + end, format!("    {line}"));
    } else {
        let block = [open, format!("    {line}"), "}".to_owned()];
        match at_start {
            true => {
                lines.splice(0..0, block.into_iter().chain([String::new()]));
            }
            false => {
                if lines.last().is_some_and(|last| !last.is_empty()) {
                    lines.push(String::new());
                }
                lines.extend(block);
            }
        }
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// IntelliJ's Gradle Kotlin DSL configurator: the Kotlin JVM plugin, kotlin-test for the tests and
/// the JVM toolchain.
fn configure_gradle_kts(text: &str, version: &str, jvm: &str) -> String {
    let text = into_block(
        text,
        "plugins",
        &format!("kotlin(\"jvm\") version \"{version}\""),
        true,
    );
    let mut text = into_block(
        &text,
        "dependencies",
        "testImplementation(kotlin(\"test\"))",
        false,
    );
    text.push_str(&format!("kotlin {{\n    jvmToolchain({jvm})\n}}\n"));
    text
}

/// IntelliJ's Gradle Groovy configurator.
fn configure_gradle_groovy(text: &str, version: &str, jvm: &str) -> String {
    let text = into_block(
        text,
        "plugins",
        &format!("id 'org.jetbrains.kotlin.jvm' version '{version}'"),
        true,
    );
    let mut text = into_block(
        &text,
        "dependencies",
        "testImplementation 'org.jetbrains.kotlin:kotlin-test'",
        false,
    );
    text.push_str(&format!("kotlin {{\n    jvmToolchain({jvm})\n}}\n"));
    text
}

/// Inserts `content` before the project's `close` tag (`    </dependencies>`, at the project's
/// level), or the block `open` … `close` around it before `before`.
fn into_pom(text: &str, open: &str, close: &str, content: &str, before: &str) -> String {
    if let Some(at) = text.find(close) {
        return format!("{}{content}{}", &text[..at], &text[at..]);
    }
    match text.rfind(before) {
        Some(at) => format!("{}{open}\n{content}{close}\n{}", &text[..at], &text[at..]),
        None => text.to_owned(),
    }
}

/// IntelliJ's Maven configurator: `kotlin.version`, kotlin-stdlib and kotlin-test, and the
/// kotlin-maven-plugin compiling the Kotlin source folders.
// shortcut: the project's own level is found by its four-space indent; read the POM's tree once
// a POM with other indents needs configuring
fn configure_pom(text: &str, version: &str, jvm: &str) -> String {
    let text = into_pom(
        text,
        "\n    <properties>",
        "    </properties>",
        &format!("        <kotlin.version>{version}</kotlin.version>\n"),
        "\n</project>",
    );
    let dependency = |artifact: &str, scope: &str| {
        format!(
            "        <dependency>\n            <groupId>org.jetbrains.kotlin</groupId>\n            <artifactId>{artifact}</artifactId>\n            <version>${{kotlin.version}}</version>\n{scope}        </dependency>\n"
        )
    };
    let dependencies = dependency("kotlin-stdlib", "")
        + &dependency("kotlin-test", "            <scope>test</scope>\n");
    let text = into_pom(
        &text,
        "\n    <dependencies>",
        "    </dependencies>",
        &dependencies,
        "\n</project>",
    );
    let plugin = format!(
        "            <plugin>\n                <groupId>org.jetbrains.kotlin</groupId>\n                <artifactId>kotlin-maven-plugin</artifactId>\n                <version>${{kotlin.version}}</version>\n                <executions>\n                    <execution>\n                        <id>compile</id>\n                        <phase>compile</phase>\n                        <goals>\n                            <goal>compile</goal>\n                        </goals>\n                        <configuration>\n                            <sourceDirs>\n                                <sourceDir>src/main/kotlin</sourceDir>\n                                <sourceDir>src/test/kotlin</sourceDir>\n                            </sourceDirs>\n                        </configuration>\n                    </execution>\n                    <execution>\n                        <id>test-compile</id>\n                        <phase>test-compile</phase>\n                        <goals>\n                            <goal>test-compile</goal>\n                        </goals>\n                    </execution>\n                </executions>\n                <configuration>\n                    <jvmTarget>{jvm}</jvmTarget>\n                </configuration>\n            </plugin>\n"
    );
    if text.contains("        </plugins>") {
        return into_pom(&text, "", "        </plugins>", &plugin, "\n</project>");
    }
    let plugins = format!("        <plugins>\n{plugin}        </plugins>\n");
    into_pom(
        &text,
        "\n    <build>",
        "    </build>",
        &plugins,
        "\n</project>",
    )
}

#[cfg(test)]
mod tests {
    use super::{configure_gradle_kts, configure_pom};

    #[test]
    fn adds_kotlin_to_a_gradle_kotlin_script_as_intellij() {
        let before = "plugins {\n    id(\"java\")\n}\n\nrepositories {\n    mavenCentral()\n}\n\ndependencies {\n    testImplementation(\"org.junit.jupiter:junit-jupiter\")\n}\n";
        assert_eq!(
            configure_gradle_kts(before, "2.4.10", "17"),
            "plugins {\n    id(\"java\")\n    kotlin(\"jvm\") version \"2.4.10\"\n}\n\nrepositories {\n    mavenCentral()\n}\n\ndependencies {\n    testImplementation(\"org.junit.jupiter:junit-jupiter\")\n    testImplementation(kotlin(\"test\"))\n}\nkotlin {\n    jvmToolchain(17)\n}\n"
        );
    }

    #[test]
    fn adds_kotlin_to_a_pom_as_intellij() {
        let before = "<project>\n    <modelVersion>4.0.0</modelVersion>\n    <artifactId>a</artifactId>\n</project>\n";
        let after = configure_pom(before, "2.4.10", "17");
        assert!(after.contains(
            "    <properties>\n        <kotlin.version>2.4.10</kotlin.version>\n    </properties>\n"
        ));
        assert!(after.contains("<artifactId>kotlin-stdlib</artifactId>"));
        assert!(after.contains("    <build>\n        <plugins>\n            <plugin>"));
        assert!(after.contains("<jvmTarget>17</jvmTarget>"));
        assert!(after.ends_with("</project>\n"));
        // an existing section takes the new lines
        let with_deps = "<project>\n    <dependencies>\n        <dependency/>\n    </dependencies>\n</project>\n";
        let after = configure_pom(with_deps, "2.4.10", "17");
        assert_eq!(after.matches("<dependencies>").count(), 1);
    }
}
