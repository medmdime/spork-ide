//! IntelliJ's quick fix "Set language level to N - …" (`IncreaseLanguageLevelFix`) on JDT LS's
//! error for a feature a newer level brings ("The Java feature 'Record Pattern' is only
//! available with source level 21 and above"): the project's level set as Project Structure sets
//! it, a Maven or Gradle project's in its build file, then synced.

use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use editor::{CodeActionProvider, Editor};
use gpui::{App, Entity, Task, Window};
use language::Buffer;
use project::{CodeAction, LspAction, ProjectTransaction};
use util::ResultExt as _;

use crate::java_project::JavaProject;
use crate::project_settings::{self, LanguageLevel, ProjectSettings};

/// Offers the fix in every editor of a project.
pub(crate) fn init(cx: &mut App) {
    cx.observe_new(|editor: &mut Editor, window, cx| {
        let Some(window) = window else { return };
        if editor.project().is_some() {
            editor.add_code_action_provider(Rc::new(SetLanguageLevel), window, cx);
        }
    })
    .detach();
}

/// The release a JDT LS diagnostic says a feature needs.
fn needed_level(message: &str) -> Option<u32> {
    let rest = message.split("only available with source level ").nth(1)?;
    rest.split_whitespace().next()?.parse().ok()
}

/// IntelliJ's text of the fix (`set.language.level.to.0`).
fn title(level: LanguageLevel) -> String {
    format!("Set language level to {}", level.text())
}

struct SetLanguageLevel;

impl CodeActionProvider for SetLanguageLevel {
    fn id(&self) -> Arc<str> {
        "java-language-level".into()
    }

    fn code_actions(
        &self,
        buffer: &Entity<Buffer>,
        range: Range<text::Anchor>,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Vec<CodeAction>>> {
        let buffer = buffer.read(cx);
        let is_java = buffer
            .file()
            .is_some_and(|file| file.path().extension() == Some("java"));
        if !is_java {
            return Task::ready(Ok(Vec::new()));
        }
        let snapshot = buffer.snapshot();
        let mut levels: Vec<u32> = snapshot
            .diagnostics_in_range::<_, usize>(range.clone(), false)
            .filter_map(|entry| needed_level(entry.diagnostic.message.as_ref()))
            .collect();
        levels.sort_unstable();
        levels.dedup();
        let actions = levels
            .into_iter()
            .map(|feature| CodeAction {
                server_id: lsp::LanguageServerId(usize::MAX),
                range: range.clone(),
                lsp_action: LspAction::Action(Box::new(lsp::CodeAction {
                    title: title(LanguageLevel::new(feature)),
                    kind: Some(lsp::CodeActionKind::QUICKFIX),
                    data: Some(serde_json::json!({ "feature": feature })),
                    ..lsp::CodeAction::default()
                })),
                resolved: true,
            })
            .collect();
        Task::ready(Ok(actions))
    }

    fn apply_code_action(
        &self,
        buffer: Entity<Buffer>,
        action: CodeAction,
        _: bool,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<Result<ProjectTransaction>> {
        let feature = match &action.lsp_action {
            LspAction::Action(action) => action
                .data
                .as_ref()
                .and_then(|data| data.get("feature"))
                .and_then(serde_json::Value::as_u64)
                .and_then(|feature| u32::try_from(feature).ok()),
            _ => None,
        };
        let path = buffer
            .read(cx)
            .file()
            .and_then(|file| file.as_local().map(|file| file.abs_path(cx)));
        let java = path.and_then(|path| JavaProject::containing(&path, cx));
        if let (Some(feature), Some(java)) = (feature, java) {
            let level = LanguageLevel::new(feature);
            java.update(cx, |java, cx| {
                let Some(root) = java.root.clone() else {
                    return;
                };
                match java.tool {
                    Some(tool) => {
                        project_settings::write_language_level(&root, tool, level).log_err();
                        java.reload(cx);
                    }
                    None => {
                        let mut settings = ProjectSettings::load(&root);
                        settings.module_language_level = None;
                        settings.language_level = Some(level);
                        settings.save(&root).log_err();
                        java.restart_language_server(cx);
                    }
                }
                cx.notify();
            });
        }
        Task::ready(Ok(ProjectTransaction::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::{needed_level, title};
    use crate::project_settings::LanguageLevel;

    #[test]
    fn reads_the_level_jdt_ls_asks_for() {
        assert_eq!(
            needed_level(
                "The Java feature 'Record Pattern' is only available with source level 21 and above"
            ),
            Some(21)
        );
        assert_eq!(
            needed_level("Type mismatch: cannot convert from String to int"),
            None
        );
        assert_eq!(
            title(LanguageLevel::new(21)),
            "Set language level to 21 - Record patterns, pattern matching for switch"
        );
    }
}
