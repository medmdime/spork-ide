//! Repository settings: local identity, commit signing and merge / pull defaults, all in
//! `git config --local` (docs/specs/M02 §2, M10 §5).

use crate::notices::Notification;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::InputState;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, v_flex};
use gpui_kit::*;
use spork_git::config::{get, get_local, set_local};

use crate::RepoView;

pub(crate) struct RepoSettings {
    no_ff: bool,
    rebase: bool,
    /// `commit.gpgsign`; git and gpg / ssh-agent handle the passphrase.
    sign: bool,
    /// `gpg.format = ssh`: sign with an SSH key instead of GPG.
    ssh_signing: bool,
}

impl RepoView {
    pub(crate) fn settings_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let root = repo.root.clone();
        // shortcut: a few quick `git config` reads on the UI thread; move them off it if opening ever lags.
        let field = |key: &str, window: &mut Window, cx: &mut Context<Self>| {
            let local = get_local(&root, key).unwrap_or_default();
            let global = match get(&root, key) {
                Some(v) if local.is_empty() => format!("{v} (global)"),
                _ => match key {
                    "user.name" => "Name",
                    "user.email" => "Email",
                    "spork.issueTracker" => {
                        "Issue tracker for PROJ-123 keys, e.g. https://jira.example.com/browse/"
                    }
                    _ => "Signing key: a GPG key id, or the path of an SSH public key",
                }
                .to_owned(),
            };
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(global));
            input.update(cx, |s, cx| s.set_value(local, window, cx));
            input
        };
        let (name, email, key, tracker) = (
            field("user.name", window, cx),
            field("user.email", window, cx),
            field("user.signingkey", window, cx),
            field("spork.issueTracker", window, cx),
        );
        self.settings = Some(RepoSettings {
            no_ff: get_local(&root, "merge.ff").as_deref() == Some("false"),
            rebase: get_local(&root, "pull.rebase").as_deref() == Some("true"),
            sign: get(&root, "commit.gpgsign").as_deref() == Some("true"),
            ssh_signing: get(&root, "gpg.format").as_deref() == Some("ssh"),
        });
        let view = cx.entity();
        let muted = cx.theme().muted_foreground;
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let Some(s) = &view.read(cx).settings else {
                return dialog;
            };
            let (no_ff, rebase, sign, ssh_signing) = (s.no_ff, s.rebase, s.sign, s.ssh_signing);
            let toggle = |id: &'static str, label: &'static str, on: bool, set: fn(&mut RepoSettings, bool)| {
                let view = view.clone();
                Checkbox::new(id).label(label).checked(on).on_click(move |checked, _, cx| {
                    view.update(cx, |this, cx| {
                        if let Some(s) = &mut this.settings {
                            set(s, *checked);
                        }
                        cx.notify();
                    })
                })
            };
            let (view, name, email, key, tracker) =
                (view.clone(), name.clone(), email.clone(), key.clone(), tracker.clone());
            dialog
                .title("Repository settings")
                .description(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().text_color(muted).child(
                            "Author for commits in this repository. Leave empty to use your global identity.",
                        ))
                        .child(spork_workbench::TextField::new(&name))
                        .child(spork_workbench::TextField::new(&email))
                        .child(toggle(
                            "no-ff",
                            "Always create a merge commit (merge.ff = false)",
                            no_ff,
                            |s, on| s.no_ff = on,
                        ))
                        .child(toggle(
                            "pull-rebase",
                            "Pull with rebase instead of merge (pull.rebase = true)",
                            rebase,
                            |s, on| s.rebase = on,
                        ))
                        .child(toggle("sign", "Sign commits (commit.gpgsign)", sign, |s, on| s.sign = on))
                        .child(toggle(
                            "ssh-sign",
                            "Sign with an SSH key instead of GPG (gpg.format = ssh)",
                            ssh_signing,
                            |s, on| s.ssh_signing = on,
                        ))
                        .child(spork_workbench::TextField::new(&key))
                        .child(spork_workbench::TextField::new(&tracker)),
                )
                .confirm()
                .ok_text("Save")
                .on_ok(move |_, window, cx| {
                    let text = |input: &Entity<InputState>| {
                        let v = input.read(cx).value().trim().to_owned();
                        (!v.is_empty()).then_some(v)
                    };
                    let (name, email, key, tracker) =
                        (text(&name), text(&email), text(&key), text(&tracker));
                    view.update(cx, |this, cx| {
                        let Some(s) = this.settings.take() else { return };
                        let (no_ff, rebase, sign, ssh) = (s.no_ff, s.rebase, s.sign, s.ssh_signing);
                        this.run_op(
                            window,
                            cx,
                            move |root| {
                                set_local(root, "user.name", name.as_deref())?;
                                set_local(root, "user.email", email.as_deref())?;
                                set_local(root, "merge.ff", no_ff.then_some("false"))?;
                                set_local(root, "pull.rebase", rebase.then_some("true"))?;
                                // written locally either way, so this repository's choice wins over the global one
                                set_local(root, "commit.gpgsign", Some(if sign { "true" } else { "false" }))?;
                                set_local(root, "gpg.format", ssh.then_some("ssh"))?;
                                set_local(root, "user.signingkey", key.as_deref())?;
                                set_local(root, "spork.issueTracker", tracker.as_deref())
                            },
                            |_, (), window, cx| {
                                window.push_notification(Notification::success("Settings saved"), cx)
                            },
                        )
                    });
                    true
                })
        });
    }
}
