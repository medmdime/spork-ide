//! Kotlin on the JVM (ROADMAP, phase 10), as IntelliJ has it: the language (fwcd's tree-sitter
//! grammar, Zed's Kotlin extension's queries) on the Kotlin language server JetBrains publishes,
//! downloaded onto the user's machine; Kotlin detected in the Java project's import; the Kotlin
//! run configuration and the gutter's run icon on `main`, run by Gradle or Maven and debugged by
//! the server's own debug adapter; New ▸ Kotlin Class/File.

mod kotlin_lsp;
mod kotlin_project;
mod new_file;
mod run;

pub use kotlin_project::KotlinProject;

use std::sync::Arc;

use futures::FutureExt as _;
use gpui::App;
use language::{LanguageConfig, LanguageRegistry, LoadedLanguage};
use language_core::{LanguageQueries, QueryFile, QueryFileContents};
use serde_json::{Value, json};
use workspace::Workspace;

const CONFIG: &str = include_str!("../languages/kotlin/config.toml");

/// The Kotlin queries, by their file names (`highlights.scm`…).
const QUERIES: [(&str, &str); 7] = [
    (
        "highlights.scm",
        include_str!("../languages/kotlin/highlights.scm"),
    ),
    (
        "brackets.scm",
        include_str!("../languages/kotlin/brackets.scm"),
    ),
    (
        "indents.scm",
        include_str!("../languages/kotlin/indents.scm"),
    ),
    (
        "injections.scm",
        include_str!("../languages/kotlin/injections.scm"),
    ),
    (
        "outline.scm",
        include_str!("../languages/kotlin/outline.scm"),
    ),
    (
        "overrides.scm",
        include_str!("../languages/kotlin/overrides.scm"),
    ),
    (
        "runnables.scm",
        include_str!("../languages/kotlin/runnables.scm"),
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

/// Registers Kotlin and its language server with the IDE's languages, the Kotlin run
/// configuration, New ▸ Kotlin Class/File and each workspace's Kotlin.
pub fn init(languages: Arc<LanguageRegistry>, cx: &mut App) {
    run::init(cx);
    spork_java::add_new_items(new_file::new_items);
    kotlin_project::watch_expiry(&languages, cx);
    cx.observe_new(|workspace: &mut Workspace, _, cx| {
        kotlin_project::register(workspace, cx);
    })
    .detach();
    let config = match config() {
        Ok(config) => config,
        Err(error) => {
            log::error!("Kotlin's language config: {error:#}");
            return;
        }
    };
    languages.register_native_grammars([("kotlin", tree_sitter_kotlin::LANGUAGE)]);
    languages.register_lsp_adapter(config.name.clone(), Arc::new(kotlin_lsp::KotlinLspAdapter));
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
                    context_provider: Some(Arc::new(run::KotlinContextProvider)),
                    toolchain_provider: None,
                    manifest_name: None,
                })
            }
            .boxed()
        }),
    );
}

/// What the end-to-end tests read of Kotlin in the window's workspace: whether it is `detected`
/// and its language `server` (`running`, `busy` until it has imported and indexed, its `pid`).
pub fn e2e_state(workspace: &Workspace, cx: &App) -> Value {
    let Some(kotlin) = KotlinProject::of(workspace, cx) else {
        return Value::Null;
    };
    let kotlin = kotlin.read(cx);
    let server = kotlin.language_server(cx).map(|server| {
        json!({
            "running": true,
            "busy": kotlin.language_server_busy(cx),
            "pid": server.process_id(),
        })
    });
    json!({
        "detected": kotlin.detected,
        "server": server,
    })
}

#[cfg(test)]
mod tests {
    use super::{config, queries};

    fn language() -> language::Language {
        language::Language::new(
            config().expect("Kotlin's config"),
            Some(tree_sitter_kotlin::LANGUAGE.into()),
        )
        .with_queries(queries())
        .expect("Kotlin's queries")
    }

    #[test]
    fn the_queries_compile_against_the_grammar() {
        let language = language();
        assert_eq!(language.name().as_ref(), "Kotlin");
        assert!(
            language
                .grammar()
                .is_some_and(|grammar| grammar.highlights_config.is_some())
        );
    }

    #[test]
    fn the_runnables_find_each_main() {
        let text = "package a\n\nfun main() {}\n\nobject App {\n    @JvmStatic fun main(args: Array<String>) {}\n}\n\nclass Tool {\n    companion object {\n        @JvmStatic fun main(args: Array<String>) {}\n    }\n    fun main() {}\n}\n";
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_kotlin::LANGUAGE.into())
            .expect("the grammar");
        let tree = parser.parse(text, None).expect("a tree");
        let query = tree_sitter::Query::new(
            &tree_sitter_kotlin::LANGUAGE.into(),
            include_str!("../languages/kotlin/runnables.scm"),
        )
        .expect("the runnables query");
        let class = query
            .capture_index_for_name("kotlin_main_class")
            .expect("the class capture");
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut found: Vec<Option<String>> = Vec::new();
        let mut matches = cursor.matches(&query, tree.root_node(), text.as_bytes());
        while let Some(found_match) = tree_sitter::StreamingIterator::next(&mut matches) {
            found.push(
                found_match
                    .captures()
                    .iter()
                    .find(|capture| capture.index == class)
                    .and_then(|capture| capture.node.utf8_text(text.as_bytes()).ok())
                    .map(str::to_owned),
            );
        }
        assert_eq!(found, [None, Some("App".into()), Some("Tool".into())]);
    }
}
