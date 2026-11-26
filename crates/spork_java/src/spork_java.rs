//! Java and the JVM build tools (ROADMAP, phase 10), the Eclipse way under IntelliJ's interface:
//! the Java language (tree-sitter's grammar, Zed's Java extension's queries) on Eclipse's JDT
//! language server, which imports Maven projects through m2e and Gradle ones through Buildship.

mod actuator;
mod application;
mod beans;
mod breakpoints;
pub mod build_files;
mod build_tool_window;
mod build_view;
pub mod coverage;
mod debugger;
mod endpoints;
mod generate;
pub mod gradle_model;
mod hotswap;
mod initializr;
mod java_project;
pub mod jdk;
mod jdk_banner;
pub mod jdk_download;
mod jdk_download_dialog;
pub mod jdtls;
mod kotlin_compiler;
mod language_level_fix;
mod libraries;
mod new_class;
pub mod pom;
pub mod project_settings;
pub mod project_structure;
pub mod sdks;
pub mod spring_boot;
pub mod spring_boot_ls;
mod spring_markers;
pub mod testing;
mod thread_dump;
mod values;

pub use application::{TEST_CONFIGURATION, jre_combo, test_template, test_variables};
pub use build_tool_window::{gradle_tool_window, maven_tool_window};
pub use build_view::{build, build_panel};
pub use generate::java;
pub use java_project::{JavaProject, Tool};
pub use new_class::{add_new_items, create_and_open};

use std::sync::Arc;

use futures::FutureExt as _;
use gpui::App;
use language::{LanguageConfig, LanguageRegistry, LoadedLanguage};
use language_core::{LanguageQueries, QueryFile, QueryFileContents};
use serde_json::{Value, json};
use workspace::Workspace;

const CONFIG: &str = include_str!("../languages/java/config.toml");

/// The Java queries, by their file names (`highlights.scm`…).
const QUERIES: [(&str, &str); 9] = [
    (
        "debugger.scm",
        include_str!("../languages/java/debugger.scm"),
    ),
    (
        "highlights.scm",
        include_str!("../languages/java/highlights.scm"),
    ),
    (
        "brackets.scm",
        include_str!("../languages/java/brackets.scm"),
    ),
    ("indents.scm", include_str!("../languages/java/indents.scm")),
    (
        "injections.scm",
        include_str!("../languages/java/injections.scm"),
    ),
    ("outline.scm", include_str!("../languages/java/outline.scm")),
    (
        "overrides.scm",
        include_str!("../languages/java/overrides.scm"),
    ),
    (
        "runnables.scm",
        include_str!("../languages/java/runnables.scm"),
    ),
    (
        "textobjects.scm",
        include_str!("../languages/java/textobjects.scm"),
    ),
];

fn config() -> anyhow::Result<LanguageConfig> {
    Ok(LanguageConfig::from_toml(CONFIG)?)
}

fn queries() -> LanguageQueries {
    LanguageQueries::from_files(QUERIES.iter().filter_map(|(name, text)| {
        let file = name.parse::<QueryFile>().ok()?;
        Some(QueryFileContents::new(
            file,
            std::borrow::Cow::Borrowed(*text),
        ))
    }))
}

/// Registers Java and its language server with the IDE's languages, the Application run
/// configuration, and each workspace's Java project with its build tool's tool window.
pub fn init(languages: Arc<LanguageRegistry>, cx: &mut App) {
    application::init(cx);
    testing::init(cx);
    coverage::init(cx);
    debugger::init(cx);
    breakpoints::init(cx);
    hotswap::init(cx);
    values::init(cx);
    spring_boot::init(cx);
    initializr::init(cx);
    spring_markers::init(cx);
    endpoints::init(cx);
    generate::init(cx);
    kotlin_compiler::init(cx);
    language_level_fix::init(cx);
    let roles = Arc::new(java_project::JavaFolderRoles::default());
    project_panel::set_folder_roles(roles.clone(), cx);
    cx.set_global(java_project::FolderRoles(roles));
    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        let Some(window) = window else { return };
        java_project::register(workspace, window, cx);
    })
    .detach();
    let config = match config() {
        Ok(config) => config,
        Err(error) => {
            log::error!("Java's language config: {error:#}");
            return;
        }
    };
    languages.register_native_grammars([
        ("java", tree_sitter_java::LANGUAGE),
        ("properties", tree_sitter_properties::LANGUAGE),
    ]);
    languages.register_lsp_adapter(config.name.clone(), Arc::new(jdtls::JdtlsAdapter));
    register_configuration_languages(&languages);
    languages.register_language(
        config.name.clone(),
        config.grammar.clone(),
        config.matcher.clone(),
        config.hidden,
        None,
        Arc::new(move || {
            let config = config.clone();
            async move {
                Ok(LoadedLanguage {
                    config,
                    queries: queries(),
                    context_provider: Some(Arc::new(application::JavaContextProvider)),
                    toolchain_provider: None,
                    manifest_name: None,
                })
            }
            .boxed()
        }),
    );
}

/// What the end-to-end tests read of Java in the window's workspace: the `jdk` found, the build
/// `tool`, whether the project is `linked` (loaded), the language `server` (`running`, `busy`,
/// its `pid`), the Maven goals' `triggers`, the toggled `profiles`, whether it is `downloading`
/// sources, the build tool window's `selected` row and the Build tool window's last `build`.
pub fn e2e_state(workspace: &Workspace, cx: &App) -> Value {
    let Some(java) = JavaProject::of(workspace, cx) else {
        return Value::Null;
    };
    let java = java.read(cx);
    let server = java.language_server(cx).map(|server| {
        json!({
            "running": true,
            "busy": java.language_server_busy(cx),
            "pid": server.process_id(),
        })
    });
    json!({
        "jdk": java.jdk.as_ref().map(|jdk| jdk.version.clone()),
        "jdk_name": java.jdk.as_ref().map(jdk::Jdk::name),
        "language_level": java
            .root
            .as_deref()
            .and_then(|root| project_settings::language_level(root, java.tool))
            .map(project_settings::LanguageLevel::text),
        "structure": project_structure::e2e_state(cx),
        "tool": java.tool.map(Tool::name),
        "linked": java.linked,
        "offline": java.offline,
        "skip_tests": java.skip_tests,
        "triggers": java.triggers.iter().map(|(trigger, goal)| format!("{} {goal}", trigger.label())).collect::<Vec<_>>(),
        "profiles": java.profiles.iter().map(|(name, on)| format!("{}{name}", if *on { "" } else { "!" })).collect::<Vec<_>>(),
        "downloading": java.downloading.is_some(),
        "server": server,
        "selected": java.tool.and_then(|tool| build_tool_window::e2e_selected(workspace, tool, cx)),
        "build": build_view::e2e_state(workspace, cx),
        "spring": spring_boot::e2e_state(java, cx),
        "downloads": jdk_download::e2e_state(cx),
        "hotswap": hotswap::e2e_state(workspace, cx),
    })
}

/// Properties (tree-sitter-properties, MIT, with its highlights query), and Spring Boot's
/// configuration files: `application*.properties` and `application*.yml` (the default
/// `file_types`) as languages of their own in a Spring Boot project, which only the Spring Boot
/// language server serves, as IntelliJ's Spring Boot plugin takes them over from Properties and
/// YAML; elsewhere they stay Properties and YAML.
fn register_configuration_languages(languages: &Arc<LanguageRegistry>) {
    let in_spring_boot_project: language::FileTypesCondition = Arc::new(|file, cx| {
        project::File::from_dyn(Some(file))
            .is_some_and(|file| spring_boot::is_project(&file.worktree.read(cx).abs_path()))
    });
    for language in [spring_boot_ls::PROPERTIES, spring_boot_ls::YAML] {
        languages.set_file_types_condition(language.into(), in_spring_boot_project.clone());
    }
    let properties = |name: &'static str, suffixes: Vec<gpui::SharedString>| LanguageConfig {
        name: name.into(),
        grammar: Some("properties".into()),
        matcher: Arc::new(language::LanguageMatcher {
            path_suffixes: suffixes,
            ..Default::default()
        }),
        line_comments: vec!["# ".into()],
        ..Default::default()
    };
    let yaml = {
        let mut config = grammars::load_config("yaml");
        config.name = spring_boot_ls::YAML.into();
        config.matcher = Arc::default();
        config
    };
    let configs = [
        (properties("Properties", vec!["properties".into()]), false),
        (properties(spring_boot_ls::PROPERTIES, Vec::new()), false),
        (yaml, true),
    ];
    for (config, is_yaml) in configs {
        if config.name.as_ref() != "Properties" {
            languages.register_lsp_adapter(
                config.name.clone(),
                Arc::new(spring_boot_ls::SpringBootLsAdapter),
            );
        }
        languages.register_language(
            config.name.clone(),
            config.grammar.clone(),
            config.matcher.clone(),
            config.hidden,
            None,
            Arc::new(move || {
                let config = config.clone();
                async move {
                    let queries = match is_yaml {
                        true => grammars::load_queries("yaml"),
                        false => LanguageQueries::from_files([QueryFileContents::new(
                            QueryFile::Highlights,
                            std::borrow::Cow::Borrowed(tree_sitter_properties::HIGHLIGHTS_QUERY),
                        )]),
                    };
                    Ok(LoadedLanguage {
                        config,
                        queries,
                        context_provider: None,
                        toolchain_provider: None,
                        manifest_name: None,
                    })
                }
                .boxed()
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{config, queries};

    #[test]
    fn the_queries_compile_against_the_grammar() {
        let language = language::Language::new(
            config().expect("Java's config"),
            Some(tree_sitter_java::LANGUAGE.into()),
        )
        .with_queries(queries())
        .expect("Java's queries");
        assert_eq!(language.name().as_ref(), "Java");
        assert!(
            language
                .grammar()
                .is_some_and(|grammar| grammar.highlights_config.is_some())
        );
    }
}
