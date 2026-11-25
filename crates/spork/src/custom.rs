//! Custom commands (docs/specs/M16 §1): shell commands or web links in the repository, branch,
//! commit and file menus, from `<config dir>/custom-commands.json` (personal) and a repository's
//! own `.spork/custom-commands.json` (shared). Shared ones never run until the user has read them
//! and trusted that exact file: any change to it asks again.
//!
//! A command may ask for inputs first (text, branch, file, checkbox), group under a heading with
//! `/` in its name, apply to some systems only (`os`), and stay quiet unless it fails (`silent`).

use std::path::{Path, PathBuf};

use crate::notices::Notification;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input as TextInput, InputState};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::component::{
    ActiveTheme as _, IndexPath, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::*;
use serde_json::Value;

use crate::{RepoView, repos};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Repository,
    Branch,
    Commit,
    File,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    /// Run by the shell in the repository folder, after `${…}` substitution.
    Shell(String),
    /// Opened in the browser, after `${…}` substitution (values percent-encoded).
    Url(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum InputKind {
    Text,
    /// Picked among the repository's branches.
    Branch,
    /// A path, typed or chosen in a file dialog.
    File,
    /// When ticked, `${input:name}` becomes this text as written (e.g. `--force`), else nothing.
    Checkbox(String),
}

/// A value asked for before the command runs; it fills `${input:<name>}`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Input {
    name: String,
    label: String,
    kind: InputKind,
    /// Text / file: the starting text; checkbox: "true" starts ticked.
    default: String,
}

/// One control of a command's input dialog.
#[derive(Clone)]
enum Field {
    Text(Entity<InputState>),
    Branch(Entity<SelectState<Vec<String>>>),
    /// An index into the dialog's ticks.
    Check(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Command {
    /// `Group/Name` puts it under a "Group" heading.
    pub(crate) name: String,
    target: Target,
    action: Action,
    inputs: Vec<Input>,
    /// Output shows only when it fails.
    silent: bool,
}

impl Command {
    /// What it does, as shown before trusting it.
    fn shown(&self) -> String {
        match &self.action {
            Action::Shell(command) => command.clone(),
            Action::Url(url) => format!("opens {url}"),
        }
    }
}

const TEMPLATE: &str = r#"{
  "commands": [
    {
      "name": "Show size of the repository",
      "target": "repository",
      "command": "du -sh .git"
    },
    {
      "name": "Log of this branch",
      "target": "branch",
      "command": "git log --oneline -20 ${ref}"
    },
    {
      "name": "Compare/With another branch",
      "target": "branch",
      "command": "git diff --stat ${input:other}...${ref}",
      "inputs": [{ "name": "other", "label": "Compare with", "type": "branch" }]
    },
    {
      "name": "Compare/On the web",
      "target": "branch",
      "url": "https://github.com/OWNER/${repo:name}/compare/${ref:short}"
    },
    {
      "name": "Push to Gerrit for review",
      "target": "branch",
      "os": ["macos", "linux"],
      "command": "git push ${input:draft} origin HEAD:refs/for/${input:target}",
      "silent": true,
      "inputs": [
        { "name": "target", "label": "Target branch", "type": "text", "default": "main" },
        { "name": "draft", "label": "Draft (work in progress)", "type": "checkbox", "value": "-o wip" }
      ]
    }
  ]
}
"#;

fn file() -> Option<PathBuf> {
    Some(repos::config_dir()?.join("custom-commands.json"))
}

/// Commands from the JSON text for this system (`os`: "macos", "linux", "windows", one or a list;
/// absent: all). Entries with an unknown target or type, or missing fields, are skipped.
pub(crate) fn parse(json: &str) -> Vec<Command> {
    parse_for(json, std::env::consts::OS)
}

fn parse_for(json: &str, os: &str) -> Vec<Command> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    v["commands"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| match &c["os"] {
            Value::String(one) => one == os,
            Value::Array(list) => list.iter().any(|o| o == os),
            _ => true,
        })
        .filter_map(|c| {
            let action = match (c["command"].as_str(), c["url"].as_str()) {
                (Some(cmd), None) => Action::Shell(cmd.to_owned()),
                (None, Some(url)) => Action::Url(url.to_owned()),
                _ => return None,
            };
            let inputs = c["inputs"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|i| {
                    let name = i["name"].as_str()?.to_owned();
                    Some(Input {
                        label: i["label"].as_str().unwrap_or(&name).to_owned(),
                        kind: match i["type"].as_str().unwrap_or("text") {
                            "text" => InputKind::Text,
                            "branch" => InputKind::Branch,
                            "file" => InputKind::File,
                            "checkbox" => InputKind::Checkbox(i["value"].as_str()?.to_owned()),
                            _ => return None,
                        },
                        default: match &i["default"] {
                            Value::String(s) => s.clone(),
                            Value::Bool(b) => b.to_string(),
                            _ => String::new(),
                        },
                        name,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(Command {
                name: c["name"].as_str()?.to_owned(),
                target: match c["target"].as_str()? {
                    "repository" => Target::Repository,
                    "branch" => Target::Branch,
                    "commit" => Target::Commit,
                    "file" => Target::File,
                    _ => return None,
                },
                action,
                inputs,
                silent: c["silent"].as_bool().unwrap_or(false),
            })
        })
        .collect()
}

pub(crate) fn load() -> Vec<Command> {
    file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|s| parse(&s))
        .unwrap_or_default()
}

/// Quotes a value for the shell, so a branch called `$(rm -rf ~)` stays a name.
fn quote(value: &str) -> String {
    if cfg!(windows) {
        // shortcut: cmd.exe has no complete escaping; double quotes cover spaces, `"` is dropped
        format!("\"{}\"", value.replace('"', ""))
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}

/// Percent-encodes everything but URL-safe characters, so a value stays one piece of a link.
fn url_encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Replaces `${name}` with the value of `vars[name]`, shell-quoted (or, `url`, percent-encoded);
/// `raw` values (checkbox texts the command's author wrote) go in as they are. Unknown variables
/// stay as they are.
fn expand_all(
    text: &str,
    vars: &[(String, String)],
    raw: &[(String, String)],
    url: bool,
) -> String {
    let mut out = text.to_owned();
    for (name, value) in raw {
        out = out.replace(&format!("${{{name}}}"), value);
    }
    for (name, value) in vars {
        let value = if url { url_encode(value) } else { quote(value) };
        out = out.replace(&format!("${{{name}}}"), &value);
    }
    out
}

/// `${repo:path}`, `${repo:name}`, and the target's own: `${ref}` / `${ref:short}`, `${sha}` /
/// `${sha:short}`, `${file}` / `${file:abs}`.
fn variables(root: &Path, subject: Option<&str>, target: Target) -> Vec<(String, String)> {
    let name = root
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let mut vars = vec![
        ("repo:path", root.display().to_string()),
        ("repo:name", name),
    ];
    if let Some(s) = subject {
        match target {
            Target::Repository => {}
            Target::Branch => {
                let short = s.rsplit_once('/').map_or(s, |(_, leaf)| leaf).to_owned();
                vars.extend([("ref", s.to_owned()), ("ref:short", short)]);
            }
            Target::Commit => {
                vars.extend([
                    ("sha", s.to_owned()),
                    ("sha:short", s.chars().take(7).collect()),
                ]);
            }
            Target::File => {
                vars.extend([
                    ("file", s.to_owned()),
                    ("file:abs", root.join(s).display().to_string()),
                ]);
            }
        }
    }
    vars.into_iter().map(|(n, v)| (n.to_owned(), v)).collect()
}

#[allow(
    clippy::disallowed_methods,
    reason = "runs inside run_op, on a background thread like the git runner"
)]
fn run(root: &Path, command: &str) -> Result<(Option<i32>, String), String> {
    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", command]);
        c
    };
    let out = cmd
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.code(), text))
}

enum Shared {
    None,
    Trusted(Vec<Command>),
    /// The file's text, to show before trusting it.
    Untrusted(String),
}

/// FNV-1a: a stable fingerprint of the trusted file (std's hasher may change between Rust versions).
fn fingerprint(text: &str) -> String {
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    });
    format!("{hash:016x}")
}

fn trust_file() -> Option<PathBuf> {
    Some(repos::config_dir()?.join("trusted-commands.txt"))
}

/// `<repo path>\t<fingerprint>` per line.
fn trusted(root: &Path, text: &str) -> bool {
    let line = format!("{}\t{}", root.display(), fingerprint(text));
    trust_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .is_some_and(|s| s.lines().any(|l| l == line))
}

fn shared(root: &Path) -> Shared {
    let Ok(text) = std::fs::read_to_string(root.join(".spork/custom-commands.json")) else {
        return Shared::None;
    };
    if trusted(root, &text) {
        Shared::Trusted(parse(&text))
    } else {
        Shared::Untrusted(text)
    }
}

/// Shows every shared command in full; "Trust" records this exact file for this repository.
fn trust_dialog(root: PathBuf, text: String, window: &mut Window, cx: &mut App) {
    let commands = parse(&text);
    let theme = cx.theme();
    let (muted, mono) = (theme.muted_foreground, theme.mono_font_family.clone());
    window.open_alert_dialog(cx, move |dialog, _, _| {
        let (root, text) = (root.clone(), text.clone());
        let rows = commands.iter().map(|c| {
            v_flex()
                .gap_0p5()
                .child(div().text_sm().child(c.name.clone()))
                .child(div().text_xs().font_family(mono.clone()).text_color(muted).child(c.shown()))
        });
        dialog
            .title("Trust this repository's commands?")
            .description(
                v_flex()
                    .gap_3()
                    .child(div().text_sm().child(
                        "This repository brings its own custom commands. Spork runs them as you, in a shell. Only trust them if you trust whoever can change this repository.",
                    ))
                    .child(v_flex().id("shared-commands").max_h(px(300.)).gap_2().children(rows).overflow_y_scrollbar()),
            )
            .confirm()
            .ok_text("Trust these commands")
            .on_ok(move |_, window, cx| {
                let Some(file) = trust_file() else { return true };
                let mut list = std::fs::read_to_string(&file).unwrap_or_default();
                list.push_str(&format!("{}\t{}\n", root.display(), fingerprint(&text)));
                let saved = std::fs::create_dir_all(file.parent().unwrap_or(&file)).and_then(|()| std::fs::write(&file, list));
                match saved {
                    Ok(()) => window.push_notification(Notification::success("Trusted: the commands are in the menus now"), cx),
                    Err(e) => window.push_notification(Notification::error(e.to_string()), cx),
                }
                true
            })
    });
}

impl RepoView {
    /// Appends the custom commands for `target` (with their subject: branch, commit, file) to a menu.
    pub(crate) fn custom_items(
        menu: PopupMenu,
        view: &Entity<Self>,
        target: Target,
        subject: Option<String>,
        cx: &App,
    ) -> PopupMenu {
        Self::custom_items_with(menu, view, target, subject, Vec::new(), cx)
    }

    /// [`Self::custom_items`] with more variables, e.g. `${sha}` beside `${file}` in file history.
    pub(crate) fn custom_items_with(
        menu: PopupMenu,
        view: &Entity<Self>,
        target: Target,
        subject: Option<String>,
        extra: Vec<(String, String)>,
        cx: &App,
    ) -> PopupMenu {
        let mut commands = load();
        let mut untrusted = None;
        if let Some(Ok(repo)) = &view.read(cx).repo {
            match shared(&repo.root) {
                Shared::Trusted(cmds) => commands.extend(cmds),
                Shared::Untrusted(text) => untrusted = Some((repo.root.clone(), text)),
                Shared::None => {}
            }
        }
        commands.retain(|c| c.target == target);
        let trust = untrusted.filter(|_| target == Target::Repository);
        if commands.is_empty() && trust.is_none() {
            return menu;
        }
        let menu = menu.separator();
        let menu = match trust {
            Some((root, text)) => {
                let view = view.clone();
                menu.item(
                    PopupMenuItem::new("Review this repository's commands…").on_click(
                        move |_, window, cx| {
                            let (root, text) = (root.clone(), text.clone());
                            view.update(cx, |_, cx| trust_dialog(root, text, window, cx))
                        },
                    ),
                )
            }
            None => menu,
        };
        let item = |menu: PopupMenu, label: &str, c: Command| {
            let (view, subject, extra) = (view.clone(), subject.clone(), extra.clone());
            menu.item(
                PopupMenuItem::new(label.to_owned()).on_click(move |_, window, cx| {
                    let (c, subject, extra) = (c.clone(), subject.clone(), extra.clone());
                    view.update(cx, |this, cx| {
                        this.start_custom(c, subject, extra, window, cx)
                    })
                }),
            )
        };
        // shortcut: `Group/Name` shows under a "Group" heading rather than in a submenu (sidebar
        // menus can't nest); nest them if long groups get in the way.
        let mut groups: Vec<(String, Vec<Command>)> = Vec::new();
        let mut menu = menu;
        for c in commands {
            match c.name.rsplit_once('/') {
                None => menu = item(menu, &c.name.clone(), c),
                Some((group, _)) => match groups.iter_mut().find(|(g, _)| g == group) {
                    Some((_, list)) => list.push(c),
                    None => groups.push((group.to_owned(), vec![c])),
                },
            }
        }
        groups.into_iter().fold(menu, |menu, (group, list)| {
            let menu = menu.separator().label(group.replace('/', " ▸ "));
            list.into_iter().fold(menu, |menu, c| {
                let leaf = c.name.rsplit_once('/').map_or("", |(_, l)| l).to_owned();
                item(menu, &leaf, c)
            })
        })
    }

    /// Runs `c`, asking for its inputs first when it has any.
    fn start_custom(
        &mut self,
        c: Command,
        subject: Option<String>,
        extra: Vec<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if c.inputs.is_empty() {
            return self.run_custom(c, subject, extra, Vec::new(), Vec::new(), window, cx);
        }
        let Some(Ok(repo)) = &self.repo else { return };
        let branches: Vec<String> = repo
            .refs
            .iter()
            .filter(|r| r.kind != spork_git::RefKind::Tag)
            .map(|r| r.name.clone())
            .collect();
        let head = repo.refs.iter().find(|r| r.is_head).map(|r| r.name.clone());
        let ticks = cx.new(|_| {
            c.inputs
                .iter()
                .map(|i| i.default == "true")
                .collect::<Vec<_>>()
        });
        let fields: Vec<Field> = c
            .inputs
            .iter()
            .enumerate()
            .map(|(ix, input)| match &input.kind {
                InputKind::Text | InputKind::File => {
                    let default = input.default.clone();
                    Field::Text(cx.new(|cx| InputState::new(window, cx).default_value(default)))
                }
                InputKind::Branch => {
                    // the branch the command was started on, else the current one
                    let want = subject
                        .clone()
                        .filter(|_| c.target == Target::Branch)
                        .or(head.clone());
                    let at = want
                        .and_then(|w| branches.iter().position(|b| *b == w))
                        .map(|row| IndexPath::default().row(row));
                    let list = branches.clone();
                    Field::Branch(
                        cx.new(|cx| SelectState::new(list, at, window, cx).searchable(true)),
                    )
                }
                InputKind::Checkbox(_) => Field::Check(ix),
            })
            .collect();
        let view = cx.entity();
        let muted = cx.theme().muted_foreground;
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let rows = c
                .inputs
                .iter()
                .zip(&fields)
                .enumerate()
                .map(|(ix, (input, field))| {
                    let label = div().text_sm().text_color(muted).child(input.label.clone());
                    match field {
                        Field::Text(state) if input.kind == InputKind::File => {
                            let state = state.clone();
                            v_flex().gap_1().child(label).child(
                                h_flex()
                                    .gap_2()
                                    .child(div().flex_1().child(TextInput::new(&state)))
                                    .child(
                                        Button::new(("custom-file", ix))
                                            .small()
                                            .label("Choose…")
                                            .on_click(move |_, window, cx| {
                                                let picked =
                                                    cx.prompt_for_paths(PathPromptOptions {
                                                        files: true,
                                                        directories: true,
                                                        multiple: false,
                                                        prompt: None,
                                                    });
                                                let state = state.clone();
                                                window
                                                    .spawn(cx, async move |cx| {
                                                        let Ok(Ok(Some(mut paths))) = picked.await
                                                        else {
                                                            return;
                                                        };
                                                        let Some(path) = paths.pop() else {
                                                            return;
                                                        };
                                                        cx.update(|window, cx| {
                                                            state.update(cx, |s, cx| {
                                                                s.set_value(
                                                                    path.display().to_string(),
                                                                    window,
                                                                    cx,
                                                                )
                                                            })
                                                        })
                                                        .ok();
                                                    })
                                                    .detach();
                                            }),
                                    ),
                            )
                        }
                        Field::Text(state) => {
                            v_flex().gap_1().child(label).child(TextInput::new(state))
                        }
                        Field::Branch(state) => v_flex()
                            .gap_1()
                            .child(label)
                            .child(Select::new(state).small()),
                        Field::Check(i) => {
                            let (ticks, i) = (ticks.clone(), *i);
                            v_flex().child(
                                Checkbox::new(("custom-check", i))
                                    .label(input.label.clone())
                                    .checked(ticks.read(cx)[i])
                                    .on_click(move |checked, window, cx| {
                                        ticks.update(cx, |t, _| t[i] = *checked);
                                        window.refresh();
                                    }),
                            )
                        }
                    }
                });
            let (view, c, fields, ticks) = (view.clone(), c.clone(), fields.clone(), ticks.clone());
            let (subject, extra) = (subject.clone(), extra.clone());
            dialog
                .title(c.name.rsplit('/').next().unwrap_or(&c.name).to_owned())
                .description(v_flex().gap_3().children(rows))
                .confirm()
                .ok_text("Run")
                .on_ok(move |_, window, cx| {
                    let (mut vars, mut raw) = (Vec::new(), Vec::new());
                    for (input, field) in c.inputs.iter().zip(&fields) {
                        let key = format!("input:{}", input.name);
                        match (field, &input.kind) {
                            (Field::Text(s), _) => {
                                vars.push((key, s.read(cx).value().trim().to_owned()))
                            }
                            (Field::Branch(s), _) => vars.push((
                                key,
                                s.read(cx).selected_value().cloned().unwrap_or_default(),
                            )),
                            (Field::Check(i), InputKind::Checkbox(value)) => raw.push((
                                key,
                                if ticks.read(cx)[*i] {
                                    value.clone()
                                } else {
                                    String::new()
                                },
                            )),
                            (Field::Check(_), _) => {}
                        }
                    }
                    let (c, subject, extra) = (c.clone(), subject.clone(), extra.clone());
                    view.update(cx, |this, cx| {
                        this.run_custom(c, subject, extra, vars, raw, window, cx)
                    });
                    true
                })
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn run_custom(
        &mut self,
        c: Command,
        subject: Option<String>,
        extra: Vec<(String, String)>,
        inputs: Vec<(String, String)>,
        raw: Vec<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(repo)) = &self.repo else { return };
        let mut vars = variables(&repo.root, subject.as_deref(), c.target);
        vars.extend(extra);
        vars.extend(inputs);
        let command = match &c.action {
            Action::Url(url) => return cx.open_url(&expand_all(url, &vars, &raw, true)),
            Action::Shell(command) => expand_all(command, &vars, &raw, false),
        };
        let (name, silent) = (
            c.name.rsplit('/').next().unwrap_or(&c.name).to_owned(),
            c.silent,
        );
        self.run_op(
            window,
            cx,
            move |root| {
                run(root, &command).map_err(|e| spork_git::GitError::Other {
                    code: None,
                    stderr: e,
                })
            },
            move |_, (code, output), window, cx| {
                if silent && code == Some(0) {
                    return window
                        .push_notification(Notification::success(format!("{name} finished")), cx);
                }
                let muted = cx.theme().muted_foreground;
                let mono = cx.theme().mono_font_family.clone();
                let status = match code {
                    Some(0) => "finished".to_owned(),
                    Some(n) => format!("failed with exit code {n}"),
                    None => "was stopped".to_owned(),
                };
                window.open_dialog(cx, move |dialog, _, _| {
                    dialog.title(format!("{name} {status}")).child(
                        v_flex()
                            .id("custom-output")
                            .max_h(px(420.))
                            .text_xs()
                            .font_family(mono.clone())
                            .text_color(muted)
                            .child(if output.trim().is_empty() {
                                "(no output)".to_owned()
                            } else {
                                output.clone()
                            })
                            .overflow_y_scrollbar(),
                    )
                });
            },
        );
    }

    /// Opens `custom-commands.json` in the default editor, creating it with examples first.
    pub(crate) fn edit_custom_commands(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = file() else { return };
        if !path.exists() {
            let written = std::fs::create_dir_all(path.parent().unwrap_or(&path))
                .and_then(|()| std::fs::write(&path, TEMPLATE));
            if let Err(e) = written {
                return window.push_notification(Notification::error(e.to_string()), cx);
            }
        }
        cx.open_with_system(&path);
        window.push_notification(
            Notification::info("Commands take effect the next time you open a menu"),
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    // explicit: `super::*` would pull in gpui's `test` attribute, which shadows #[test]
    use super::{
        Action, InputKind, TEMPLATE, Target, expand_all, fingerprint, parse, parse_for, variables,
    };
    use std::path::Path;

    #[test]
    fn parses_and_expands_safely() {
        let commands = parse_for(TEMPLATE, "macos");
        assert_eq!(commands.len(), 5);
        assert_eq!(commands[1].target, Target::Branch);
        assert_eq!(parse_for(TEMPLATE, "windows").len(), 4); // the Gerrit one is macos / linux only
        assert!(matches!(&commands[3].action, Action::Url(u) if u.contains("${ref:short}")));
        let gerrit = &commands[4];
        assert!(gerrit.silent);
        assert_eq!(gerrit.inputs[0].default, "main");
        assert_eq!(gerrit.inputs[1].kind, InputKind::Checkbox("-o wip".into()));
        assert_eq!(commands[2].inputs[0].kind, InputKind::Branch);
        // a command needs exactly one of command / url, and known input types
        assert!(parse(r#"{"commands":[{"name":"x","target":"branch"}]}"#).is_empty());
        assert!(
            parse(r#"{"commands":[{"name":"x","target":"branch","command":"y","inputs":[{"name":"a","type":"radio"}]}]}"#)
                .is_empty()
        );
        assert!(parse("not json").is_empty());
        assert_eq!(
            parse(r#"{"commands":[{"name":"x","target":"nope","command":"y"}]}"#).len(),
            0
        );

        // the trust fingerprint is stable and changes with any edit
        assert_eq!(fingerprint("a"), "af63dc4c8601ec8c");
        assert_ne!(
            fingerprint(TEMPLATE),
            fingerprint(&TEMPLATE.replace("du", "rm"))
        );

        let vars = variables(
            Path::new("/r/app"),
            Some("feature/$(rm -rf ~)"),
            Target::Branch,
        );
        let cmd = expand_all(
            "git log ${ref} # ${ref:short} ${repo:name} ${nope}",
            &vars,
            &[],
            false,
        );
        if cfg!(not(windows)) {
            assert_eq!(
                cmd,
                "git log 'feature/$(rm -rf ~)' # '$(rm -rf ~)' 'app' ${nope}"
            );
            assert_eq!(
                expand_all("echo ${ref}", &[("ref".into(), "it's".into())], &[], false),
                r"echo 'it'\''s'"
            );
            // checkbox texts go in as written; an unticked one leaves nothing
            let on = [("input:f".to_owned(), "--force".to_owned())];
            let off = [("input:f".to_owned(), String::new())];
            assert_eq!(
                expand_all("git push ${input:f} x", &[], &on, false),
                "git push --force x"
            );
            assert_eq!(
                expand_all("git push ${input:f} x", &[], &off, false),
                "git push  x"
            );
        }
        // links: values percent-encoded, not shell-quoted
        let vars = [("ref:short".to_owned(), "a b&c/d".to_owned())];
        assert_eq!(
            expand_all("https://h/compare/${ref:short}", &vars, &[], true),
            "https://h/compare/a%20b%26c/d"
        );
    }
}
