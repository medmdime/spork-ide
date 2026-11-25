//! Git / ssh prompts (password, username, key passphrase) answered in a dialog (docs/specs/M10).

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::notices::Notification;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use smol::channel::{Receiver, Sender};
use spork_git::askpass;

use crate::RepoView;

/// A prompt from git plus where to send the answer (`None` cancels the git command).
pub type Prompt = (String, Sender<Option<String>>);

/// A key and the passphrase just typed for it, handed to ssh-add's own prompt about that key.
static HANDOFF: Mutex<Option<(String, String)>> = Mutex::new(None);

/// The handed-off passphrase when `prompt` asks about its key; ssh-add words it differently from
/// ssh (`Enter passphrase for /k/id:`), so only the path is matched.
fn take_for(handoff: &mut Option<(String, String)>, prompt: &str) -> Option<String> {
    let (key, _) = handoff.as_ref()?;
    prompt
        .contains(key.as_str())
        .then(|| handoff.take())?
        .map(|(_, passphrase)| passphrase)
}

/// The private key an ssh passphrase prompt is about: `Enter passphrase for key '/k/id':`.
fn key_of(prompt: &str) -> Option<PathBuf> {
    let (_, rest) = prompt.split_once("passphrase for key '")?;
    let (key, _) = rest.rsplit_once('\'')?;
    (!key.is_empty()).then(|| PathBuf::from(key))
}

/// Starts the prompt server and routes every later git command's prompts to it.
pub fn start() -> Option<Receiver<Prompt>> {
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let addr = listener.local_addr().ok()?.to_string();
    let token = askpass::random_token();
    askpass::install(askpass::Config {
        program: std::env::current_exe().ok()?,
        addr,
        token: token.clone(),
    });
    let (tx, rx) = smol::channel::unbounded::<Prompt>();
    std::thread::spawn(move || {
        askpass::serve(listener, &token, |prompt| {
            if let Some(passphrase) = HANDOFF
                .lock()
                .ok()
                .and_then(|mut h| take_for(&mut h, &prompt))
            {
                return Some(passphrase);
            }
            let (reply, answer) = smol::channel::bounded(1);
            tx.send_blocking((prompt, reply)).ok()?;
            // a dialog closed without OK drops the sender, which cancels
            answer.recv_blocking().ok().flatten()
        })
    });
    Some(rx)
}

impl RepoView {
    /// Shows one prompt from git. Secrets (passwords, tokens, passphrases) are masked.
    pub fn ask(
        &mut self,
        prompt: String,
        reply: Sender<Option<String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lower = prompt.to_lowercase();
        let secret = ["password", "passphrase", "token"]
            .iter()
            .any(|w| lower.contains(w));
        let title = match () {
            _ if lower.contains("passphrase") => "SSH key passphrase",
            _ if lower.contains("password") => "Password",
            _ if lower.contains("username") => "Username",
            _ => "Git needs your input",
        };
        self.prompt_input.update(cx, |input, cx| {
            input.set_masked(secret, window, cx);
            input.set_value("", window, cx);
        });
        let input = self.prompt_input.clone();
        let muted = cx.theme().muted_foreground;
        let key = key_of(&prompt);
        let remember_label = if cfg!(target_os = "macos") {
            "Remember in the Keychain"
        } else {
            "Keep unlocked until I log out (SSH agent)"
        };
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let remember = view.read(cx).remember_passphrase;
            let (check_view, ok_view, key) = (view.clone(), view.clone(), key.clone());
            let (ok_reply, cancel_reply, input) = (reply.clone(), reply.clone(), input.clone());
            dialog
                .title(title)
                .description(
                    v_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child(prompt.trim().to_owned()),
                        )
                        .child(
                            spork_workbench::TextField::new(&input)
                                .when(secret, |i| i.mask_toggle()),
                        )
                        .when(key.is_some(), |d| {
                            d.child(
                                Checkbox::new("remember-passphrase")
                                    .label(remember_label)
                                    .checked(remember)
                                    .on_click(move |checked, _, cx| {
                                        check_view.update(cx, |this, cx| {
                                            this.remember_passphrase = *checked;
                                            cx.notify();
                                        })
                                    }),
                            )
                        }),
                )
                .confirm()
                .ok_text("Continue")
                .on_ok(move |_, window, cx| {
                    let answer = input.read(cx).value().to_string();
                    ok_reply.try_send(Some(answer.clone())).ok();
                    if let Some(key) = key.clone().filter(|_| ok_view.read(cx).remember_passphrase)
                    {
                        ok_view.update(cx, |this, cx| this.remember_key(key, answer, window, cx));
                    }
                    true
                })
                .on_cancel(move |_, _, _| {
                    cancel_reply.try_send(None).ok();
                    true
                })
        });
    }

    /// Adds `key` to the SSH agent (and Keychain) with the passphrase just typed (M10 §3).
    fn remember_key(
        &mut self,
        key: PathBuf,
        passphrase: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Ok(mut handoff) = HANDOFF.lock() {
            *handoff = Some((key.to_string_lossy().into_owned(), passphrase));
        }
        let task = cx.background_spawn(async move {
            let added = askpass::add_to_agent(&key);
            // never leave a passphrase behind, whether ssh-add asked for it or not
            HANDOFF.lock().map(|mut h| h.take()).ok();
            added
        });
        cx.spawn_in(window, async move |_, cx| {
            let result = task.await;
            cx.update(|window, cx| {
                let note = match result {
                    Ok(()) => Notification::success("Passphrase remembered"),
                    Err(e) => Notification::error(format!("Couldn't remember the passphrase: {e}")),
                };
                window.push_notification(note, cx);
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    // explicit: `super::*` would pull in gpui's `test` attribute, which shadows #[test]
    use super::{key_of, take_for};

    #[test]
    fn hands_the_passphrase_to_ssh_add_once() {
        let mut handoff = Some(("/k/id".to_owned(), "pw".to_owned()));
        assert_eq!(take_for(&mut handoff, "Enter passphrase for /other:"), None);
        assert_eq!(
            take_for(&mut handoff, "Enter passphrase for /k/id: ").as_deref(),
            Some("pw")
        );
        assert_eq!(take_for(&mut handoff, "Enter passphrase for /k/id: "), None);
    }

    #[test]
    fn finds_the_key_in_ssh_prompts() {
        assert_eq!(
            key_of("Enter passphrase for key '/home/a/.ssh/id_ed25519': ").unwrap(),
            std::path::Path::new("/home/a/.ssh/id_ed25519")
        );
        assert_eq!(key_of("Password for 'https://u@host':"), None);
        assert_eq!(key_of("Enter passphrase for key '':"), None);
    }
}
