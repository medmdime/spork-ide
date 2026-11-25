//! AI commit messages with the user's own provider (docs/10-ai.md): off until configured, and a
//! preview of exactly what will be sent before every request.

use crate::notices::Notification;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::InputState;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{
    ActiveTheme as _, Selectable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_ai::{DEFAULT_MODEL, Prepared, Provider};
use spork_git::{GitError, credential};

use crate::{RepoView, repos};

const ANTHROPIC_HOST: &str = "api.anthropic.com";
// the account name the key is filed under in the git credential helper
const KEY_USER: &str = "spork-ai";

/// What the user chose in "AI settings"; stored in `<config dir>/ai.conf`, the key in the
/// git credential helper. No file means AI is off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AiConfig {
    /// Anthropic, else an OpenAI-compatible endpoint (local models).
    anthropic: bool,
    model: String,
    endpoint: String,
}

impl AiConfig {
    fn file() -> Option<std::path::PathBuf> {
        Some(repos::config_dir()?.join("ai.conf"))
    }

    pub(crate) fn load() -> Option<Self> {
        let text = std::fs::read_to_string(Self::file()?).ok()?;
        let get = |key: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
                .unwrap_or("")
                .to_owned()
        };
        Some(AiConfig {
            anthropic: get("provider") != "compatible",
            model: get("model"),
            endpoint: get("endpoint"),
        })
    }

    fn save(&self) -> std::io::Result<()> {
        let file = Self::file().ok_or_else(|| std::io::Error::other("no settings folder"))?;
        std::fs::create_dir_all(file.parent().unwrap_or(&file))?;
        let provider = if self.anthropic {
            "anthropic"
        } else {
            "compatible"
        };
        std::fs::write(
            file,
            format!(
                "provider={provider}\nmodel={}\nendpoint={}\n",
                self.model, self.endpoint
            ),
        )
    }

    /// Where the key is filed in the credential helper.
    fn key_host(&self) -> String {
        if self.anthropic {
            return ANTHROPIC_HOST.into();
        }
        let rest = self
            .endpoint
            .split_once("://")
            .map_or(self.endpoint.as_str(), |(_, r)| r);
        rest.split('/').next().unwrap_or(rest).to_owned()
    }

    fn provider(&self) -> Result<Provider, GitError> {
        let key = credential::get(&self.key_host()).map(|(_, k)| k);
        Ok(if self.anthropic {
            Provider::Anthropic {
                key: key.ok_or_else(|| other("Add your Anthropic API key in AI settings."))?,
                model: self.model.clone(),
            }
        } else {
            Provider::OpenAiCompatible {
                endpoint: self.endpoint.clone(),
                key: key.unwrap_or_default(),
                model: self.model.clone(),
            }
        })
    }

    fn describe(&self) -> String {
        if self.anthropic {
            format!("Anthropic ({})", self.model)
        } else {
            format!("{} ({})", self.endpoint, self.model)
        }
    }
}

fn other(e: impl ToString) -> GitError {
    GitError::Other {
        code: None,
        stderr: e.to_string(),
    }
}

/// The open "AI settings" dialog's fields.
pub(crate) struct AiForm {
    anthropic: bool,
    model: Entity<InputState>,
    endpoint: Entity<InputState>,
    key: Entity<InputState>,
}

/// What an AI request is for, and what is sent with it.
#[derive(Clone)]
pub(crate) enum AiTask {
    /// The staged changes → the commit box.
    CommitMessage,
    /// A commit (id, subject) → review notes in a window.
    Review(String, String),
    /// A branch (base ref, branch) → title and description in the open "Create pull request" dialog.
    PullRequest(String, String),
    /// A commit (id, message) → a plain-language explanation in a window.
    Explain(String, String),
    /// A tag → release notes in a window; only commit subjects are sent.
    ReleaseNotes(String),
    /// A conflicted file (path) → a proposed resolution, written only if the user says so.
    Conflict(String),
    /// The interactive rebase plan's commits (id, subject) → pick / squash / fixup per row.
    RebasePlan(Vec<(String, String)>),
    /// A request in plain words → one of Spork's menu commands, run only after a confirm.
    Ask(String),
}

impl RepoView {
    /// Settings first if AI is off; else the diff for `task` is prepared and previewed.
    pub(crate) fn ai_start(&mut self, task: AiTask, window: &mut Window, cx: &mut Context<Self>) {
        let Some(config) = AiConfig::load() else {
            return self.ai_settings_dialog(window, cx);
        };
        let source = task.clone();
        // a review covers the commit's own change: its first parent (none for a root commit)
        let parent = match (&task, &self.repo) {
            (AiTask::Review(id, _) | AiTask::Explain(id, _), Some(Ok(repo))) => repo
                .history
                .commits
                .iter()
                .find(|c| &c.id == id)
                .and_then(|c| c.parents.first().cloned()),
            _ => None,
        };
        self.run_op(
            window,
            cx,
            move |root| {
                let diff = match &source {
                    AiTask::CommitMessage => spork_git::staged_diff(root)?,
                    AiTask::Review(id, _) | AiTask::Explain(id, _) => {
                        spork_git::commit_details(root, id, parent.as_deref(), false)?.patch
                    }
                    AiTask::PullRequest(base, branch) => {
                        spork_git::revision::branch_diff(root, base, branch)?
                    }
                    AiTask::Ask(question) => {
                        let names: Vec<String> = crate::shell::commands()
                            .into_iter()
                            .map(|(n, _)| n.to_string())
                            .collect();
                        return Ok(Prepared {
                            diff: format!("{question}\n\n{}", names.join("\n")),
                            omitted: Vec::new(),
                            secrets: Vec::new(),
                        });
                    }
                    AiTask::RebasePlan(commits) => {
                        // subjects only, like release notes
                        let lines: Vec<String> =
                            commits.iter().map(|(id, s)| format!("{id} {s}")).collect();
                        return Ok(Prepared {
                            diff: lines.join("\n"),
                            omitted: Vec::new(),
                            secrets: Vec::new(),
                        });
                    }
                    AiTask::Conflict(path) => {
                        let text = std::fs::read_to_string(root.join(path)).map_err(other)?;
                        let secrets = if spork_ai::looks_secret(&text) {
                            vec![path.clone()]
                        } else {
                            Vec::new()
                        };
                        return Ok(Prepared {
                            diff: text,
                            omitted: Vec::new(),
                            secrets,
                        });
                    }
                    AiTask::ReleaseNotes(tag) => {
                        // subjects only: nothing to filter, nothing secret in the code
                        let subjects = spork_git::revision::release_subjects(root, tag)?;
                        return Ok(Prepared {
                            diff: subjects.join("\n"),
                            omitted: Vec::new(),
                            secrets: Vec::new(),
                        });
                    }
                };
                if diff.trim().is_empty() {
                    return Err(other("There are no changes to send: stage some first."));
                }
                Ok(spork_ai::prepare(&diff))
            },
            move |this, prepared, window, cx| this.ai_confirm(config, prepared, task, window, cx),
        );
    }

    /// "What will be sent" (docs/10-ai.md §2): size, files left out, possible secrets.
    fn ai_confirm(
        &mut self,
        config: AiConfig,
        prepared: Prepared,
        task: AiTask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let (title, what) = match &task {
            AiTask::CommitMessage => (
                "Write the commit message with AI?",
                "your staged changes and your last commit subjects",
            ),
            AiTask::Review(..) => ("Review this commit with AI?", "this commit's changes"),
            AiTask::Explain(..) => (
                "Explain this commit with AI?",
                "this commit's message and changes",
            ),
            AiTask::ReleaseNotes(..) => (
                "Write release notes with AI?",
                "the subjects of this release's commits",
            ),
            AiTask::Conflict(..) => (
                "Suggest a resolution with AI?",
                "this file with its conflicts",
            ),
            AiTask::RebasePlan(..) => (
                "Suggest a rebase plan with AI?",
                "the subjects of these commits",
            ),
            AiTask::Ask(..) => (
                "Ask AI which command to run?",
                "your request and the names of Spork's commands",
            ),
            AiTask::PullRequest(..) => (
                "Write the description with AI?",
                "this branch's commit subjects and changes",
            ),
        };
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, config, prepared, task) =
                (view.clone(), config.clone(), prepared.clone(), task.clone());
            let kb = prepared.diff.len().div_ceil(1024);
            dialog
                .title(title)
                .description(
                    v_flex()
                        .gap_2()
                        .text_sm()
                        .child(format!("Sends {kb} KB of {what} to {}.", config.describe()))
                        .when(!prepared.omitted.is_empty(), |d| {
                            d.child(div().text_color(muted).child(format!(
                                "Left out: {}",
                                prepared.omitted.join(", ")
                            )))
                        })
                        .when(!prepared.secrets.is_empty(), |d| {
                            d.child(div().text_color(danger).child(format!(
                                "These changes look like they contain a secret: {}. Leave them out, or send anyway.",
                                prepared.secrets.join(", ")
                            )))
                        }),
                )
                .confirm()
                .ok_text(if prepared.secrets.is_empty() { "Send" } else { "Send anyway" })
                .on_ok(move |_, window, cx| {
                    let (config, prepared, task) = (config.clone(), prepared.clone(), task.clone());
                    view.update(cx, |this, cx| this.ai_generate(config, prepared, task, window, cx));
                    true
                })
        });
    }

    fn ai_generate(
        &mut self,
        config: AiConfig,
        prepared: Prepared,
        task: AiTask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(repo)) = &self.repo else { return };
        let root = repo.root.clone();
        self.ai_busy = true;
        cx.notify();
        let request = task.clone();
        self._ai = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let provider = config.provider().map_err(|e| e.to_string())?;
                    match &request {
                        AiTask::CommitMessage => {
                            let recent: Vec<String> =
                                spork_git::revision::recent_messages(&root, 10)
                                    .iter()
                                    .filter_map(|m| m.lines().next().map(str::to_owned))
                                    .collect();
                            spork_ai::commit_message(&provider, &prepared, &recent)
                        }
                        AiTask::Review(_, subject) => {
                            spork_ai::review(&provider, &prepared, subject)
                        }
                        AiTask::Explain(_, message) => {
                            spork_ai::explain(&provider, &prepared, message)
                        }
                        AiTask::ReleaseNotes(tag) => {
                            let subjects: Vec<String> =
                                prepared.diff.lines().map(str::to_owned).collect();
                            spork_ai::release_notes(&provider, tag, &subjects)
                        }
                        AiTask::Conflict(path) => {
                            spork_ai::resolve_conflicts(&provider, path, &prepared.diff)
                        }
                        // the chosen command comes back as its menu label (empty: none fits)
                        AiTask::Ask(question) => {
                            let names: Vec<String> = crate::shell::commands()
                                .into_iter()
                                .map(|(n, _)| n.to_string())
                                .collect();
                            spork_ai::ask(&provider, question, &names)
                                .map(|ix| ix.map(|i| names[i].clone()).unwrap_or_default())
                        }
                        // the steps come back as words, one per row, through the same text path
                        AiTask::RebasePlan(commits) => spork_ai::plan_rebase(&provider, commits)
                            .map(|steps| {
                                let words: Vec<&str> = steps
                                    .iter()
                                    .map(|s| match s {
                                        spork_ai::Step::Pick => "pick",
                                        spork_ai::Step::Squash => "squash",
                                        spork_ai::Step::Fixup => "fixup",
                                    })
                                    .collect();
                                words.join("\n")
                            }),
                        AiTask::PullRequest(base, branch) => {
                            let subjects =
                                spork_git::revision::branch_subjects(&root, base, branch)
                                    .unwrap_or_default();
                            spork_ai::pr_description(&provider, &prepared, &subjects)
                        }
                    }
                    .map_err(|e| e.to_string())
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.ai_busy = false;
                match (result, task) {
                    (Ok(message), AiTask::CommitMessage) => {
                        this.set_message(message.trim().to_owned(), window, cx)
                    }
                    (Ok(notes), AiTask::Review(id, subject)) => {
                        let short: String = id.chars().take(7).collect();
                        markdown_window(format!("Review of {short} {subject}"), notes, window, cx);
                    }
                    (Ok(text), AiTask::Explain(id, message)) => {
                        let short: String = id.chars().take(7).collect();
                        let subject = message.lines().next().unwrap_or("").to_owned();
                        markdown_window(format!("{short} {subject}"), text, window, cx);
                    }
                    (Ok(notes), AiTask::ReleaseNotes(tag)) => {
                        markdown_window(format!("Release notes for {tag}"), notes, window, cx);
                    }
                    (Ok(text), AiTask::Conflict(path)) => {
                        this.conflict_suggestion(path, text, window, cx)
                    }
                    (Ok(text), AiTask::PullRequest(..)) => {
                        let (title, body) = text.split_once("\n\n").unwrap_or((&text, ""));
                        let (title, body) = (title.trim().to_owned(), body.trim().to_owned());
                        if let Some((t, b)) = &this.pr_form {
                            t.update(cx, |s, cx| s.set_value(title, window, cx));
                            b.update(cx, |s, cx| s.set_value(body, window, cx));
                        }
                    }
                    (Ok(label), AiTask::Ask(question)) => {
                        this.confirm_command(question, label, window, cx)
                    }
                    (Ok(words), AiTask::RebasePlan(_)) => {
                        if let Some(plan) = &mut this.rebase_plan {
                            for ((_, action), word) in plan.rows.iter_mut().zip(words.lines()) {
                                *action = match word {
                                    "squash" => spork_git::TodoAction::Squash,
                                    "fixup" => spork_git::TodoAction::Fixup,
                                    _ => spork_git::TodoAction::Pick,
                                };
                            }
                            window.push_notification(
                                Notification::info(
                                    "Plan suggested: check it, then start the rebase",
                                ),
                                cx,
                            );
                        }
                    }
                    (Err(e), _) => window.push_notification(Notification::error(e), cx),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// Provider, model, endpoint and key; also how AI is turned off again.
    pub(crate) fn ai_settings_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let config = AiConfig::load();
        let input = |value: String,
                     placeholder: &str,
                     masked: bool,
                     window: &mut Window,
                     cx: &mut Context<Self>| {
            let placeholder = placeholder.to_owned();
            let state = cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(masked)
                    .placeholder(placeholder)
            });
            state.update(cx, |s, cx| s.set_value(value, window, cx));
            state
        };
        let (model, endpoint) = config.as_ref().map_or_else(
            || (DEFAULT_MODEL.to_owned(), String::new()),
            |c| (c.model.clone(), c.endpoint.clone()),
        );
        self.ai_form = Some(AiForm {
            anthropic: config.as_ref().is_none_or(|c| c.anthropic),
            model: input(model, "Model", false, window, cx),
            endpoint: input(endpoint, "http://localhost:11434/v1", false, window, cx),
            key: input(
                String::new(),
                "API key (kept in your git credential helper)",
                true,
                window,
                cx,
            ),
        });
        let view = cx.entity();
        let muted = cx.theme().muted_foreground;
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let Some(form) = &view.read(cx).ai_form else {
                return dialog;
            };
            let anthropic = form.anthropic;
            let (model, endpoint, key) = (form.model.clone(), form.endpoint.clone(), form.key.clone());
            let pick = |id: &'static str, label: &'static str, value: bool| {
                let view = view.clone();
                Button::new(id)
                    .small()
                    .label(label)
                    .selected(anthropic == value)
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            if let Some(form) = &mut this.ai_form {
                                form.anthropic = value;
                            }
                            cx.notify();
                        })
                    })
            };
            let (save_view, off_view) = (view.clone(), view.clone());
            dialog
                .title("AI settings")
                .description(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().text_color(muted).child(
                            "AI is optional. Spork shows what it will send before every request and never runs git commands on its own.",
                        ))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(pick("ai-anthropic", "Anthropic", true))
                                .child(pick("ai-compatible", "OpenAI-compatible / local", false)),
                        )
                        .child(spork_workbench::TextField::new(&model))
                        .when(!anthropic, |d| d.child(spork_workbench::TextField::new(&endpoint)))
                        .child(spork_workbench::TextField::new(&key))
                        .child(
                            Button::new("ai-off")
                                .ghost()
                                .small()
                                .label("Turn AI off")
                                .on_click(move |_, window, cx| {
                                    if let Some(file) = AiConfig::file() {
                                        std::fs::remove_file(file).ok();
                                    }
                                    off_view.update(cx, |this, _| this.ai_form = None);
                                    window.close_dialog(cx);
                                    window.push_notification(Notification::info("AI is off"), cx);
                                }),
                        ),
                )
                .confirm()
                .ok_text("Save")
                .on_ok(move |_, window, cx| {
                    let read = |s: &Entity<InputState>| s.read(cx).value().trim().to_owned();
                    let (model, endpoint, key) = (read(&model), read(&endpoint), read(&key));
                    save_view.update(cx, |this, cx| {
                        let Some(form) = this.ai_form.take() else { return };
                        let config = AiConfig {
                            anthropic: form.anthropic,
                            model: if model.is_empty() { DEFAULT_MODEL.into() } else { model },
                            endpoint,
                        };
                        this.run_op(
                            window,
                            cx,
                            move |_| {
                                if !config.anthropic && config.endpoint.is_empty() {
                                    return Err(other("Enter the endpoint, e.g. http://localhost:11434/v1 for Ollama."));
                                }
                                if !key.is_empty() {
                                    credential::store(&config.key_host(), KEY_USER, &key)?;
                                }
                                config.save().map_err(other)
                            },
                            |_, (), window, cx| window.push_notification(Notification::success("AI settings saved"), cx),
                        )
                    });
                    true
                })
        });
    }
}

impl RepoView {
    /// The proposed file; "Write to file" replaces the working file, and the user reviews the
    /// diff and stages it to mark the conflict resolved (docs/10-ai.md rule 2: AI never acts alone).
    fn conflict_suggestion(
        &mut self,
        path: String,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(repo)) = &self.repo else { return };
        let file = repo.root.join(&path);
        let view = cx.entity();
        let theme = cx.theme();
        let (muted, mono) = (theme.muted_foreground, theme.mono_font_family.clone());
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, file, text) = (view.clone(), file.clone(), text.clone());
            dialog
                .title(format!("Suggested resolution of {path}"))
                .w(px(720.))
                .description(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().text_color(muted).child(
                            "Check it before using it: AI can be wrong. Writing replaces the file with this text; review the diff and stage the file to mark it resolved.",
                        ))
                        .child(
                            v_flex()
                                .id("conflict-suggestion")
                                .max_h(px(420.))
                                .text_xs()
                                .font_family(mono.clone())
                                .child(text.clone()).overflow_y_scrollbar(),
                        ),
                )
                .confirm()
                .ok_text("Write to file")
                .on_ok(move |_, window, cx| {
                    match std::fs::write(&file, &text) {
                        Ok(()) => window.push_notification(Notification::success("Written: review it, then stage the file"), cx),
                        Err(e) => window.push_notification(Notification::error(e.to_string()), cx),
                    }
                    view.update(cx, |this, cx| this.reload(cx));
                    true
                })
        });
    }
}

impl RepoView {
    /// Spork ▸ Ask Git (AI): a request in plain words.
    pub(crate) fn ask_git_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("e.g. undo my last commit but keep the changes")
        });
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, input) = (view.clone(), input.clone());
            dialog
                .title("Ask Git")
                .description(spork_workbench::TextField::new(&input))
                .confirm()
                .ok_text("Ask")
                .on_ok(move |_, window, cx| {
                    let question = input.read(cx).value().trim().to_owned();
                    if question.is_empty() {
                        return false;
                    }
                    view.update(cx, |this, cx| {
                        this.ai_start(AiTask::Ask(question), window, cx)
                    });
                    true
                })
        });
    }

    /// Names the command AI picked and runs it, through its normal dialog, only on OK.
    fn confirm_command(
        &mut self,
        question: String,
        label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_, action)) = crate::shell::commands()
            .into_iter()
            .find(|(n, _)| n.as_ref() == label)
        else {
            return window.push_notification(
                Notification::info(format!("No Spork command fits: {question}")),
                cx,
            );
        };
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let action = action.boxed_clone();
            dialog
                .title(format!("Run \u{201c}{label}\u{201d}?"))
                .description(format!("For: {question}"))
                .confirm()
                .ok_text("Run")
                .on_ok(move |_, window, cx| {
                    let action = action.boxed_clone();
                    // after the dialog closes, so the action reaches the repository view
                    window.defer(cx, move |window, cx| window.dispatch_action(action, cx));
                    true
                })
        });
    }
}

/// AI output in Markdown, with a Copy button for the text as written.
fn markdown_window(title: String, text: String, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, _| {
        let copy = text.clone();
        dialog.title(title.clone()).w(px(640.)).child(
            v_flex()
                .gap_2()
                .child(
                    v_flex()
                        .id("ai-markdown")
                        .max_h(px(480.))
                        .child(TextView::markdown("ai-markdown-text", text.clone()))
                        .overflow_y_scrollbar(),
                )
                .child(
                    Button::new("ai-copy")
                        .ghost()
                        .small()
                        .label("Copy as Markdown")
                        .on_click(move |_, window, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                            window.push_notification(Notification::info("Copied"), cx);
                        }),
                ),
        )
    });
}
