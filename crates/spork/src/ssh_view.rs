//! Spork ▸ SSH Keys: copy a public key to add it on a git server, or generate one (docs/specs/M10 §4).

use crate::notices::Notification;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::InputState;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::RepoView;
use crate::ssh::{generate, keys, ssh_dir};

impl RepoView {
    pub(crate) fn ssh_keys_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = ssh_dir() else {
            return window.push_notification(Notification::error("No home folder found."), cx);
        };
        let existing = keys(&dir);
        let default_name = ["id_ed25519", "id_ed25519_spork"]
            .into_iter()
            .find(|n| !dir.join(n).exists())
            .unwrap_or("id_ed25519_2");
        let email = match &self.repo {
            Some(Ok(repo)) => spork_git::config::get(&repo.root, "user.email"),
            _ => None,
        };
        let field = |value: String,
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
        let name = field(default_name.into(), "File name", false, window, cx);
        let comment = field(
            email.unwrap_or_default(),
            "Comment, usually your email",
            false,
            window,
            cx,
        );
        let passphrase = field(
            String::new(),
            "Passphrase (optional, recommended)",
            true,
            window,
            cx,
        );
        let theme = cx.theme();
        let (muted, mono) = (theme.muted_foreground, theme.mono_font_family.clone());
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let rows = existing.iter().enumerate().map(|(i, k)| {
                let public = k.public.clone();
                let file = k.path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                h_flex()
                    .gap_2()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_sm().child(file))
                            .child(div().text_xs().font_family(mono.clone()).text_color(muted).truncate().child(public.clone())),
                    )
                    .child(Button::new(("copy-key", i)).ghost().small().label("Copy").on_click(move |_, window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(public.clone()));
                        window.push_notification(Notification::info("Public key copied"), cx);
                    }))
            });
            let (dir, name, comment, passphrase) = (dir.clone(), name.clone(), comment.clone(), passphrase.clone());
            let page = |id: &'static str, label: &'static str, url: &'static str| {
                Button::new(id).ghost().small().label(label).on_click(move |_, _, cx| cx.open_url(url))
            };
            dialog
                .title("SSH keys")
                .description(
                    v_flex()
                        .gap_3()
                        .child(div().text_sm().text_color(muted).child(
                            "Git servers recognise you by a public key. Copy one and add it in your account's SSH settings.",
                        ))
                        .children(rows)
                        .when(existing.is_empty(), |d| d.child(div().text_sm().child("No keys yet in ~/.ssh.")))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(page("gh-ssh", "GitHub SSH settings", "https://github.com/settings/keys"))
                                .child(page("gl-ssh", "GitLab SSH settings", "https://gitlab.com/-/user_settings/ssh_keys")),
                        )
                        .child(div().text_sm().child("Generate a new ed25519 key"))
                        .child(spork_workbench::TextField::new(&name))
                        .child(spork_workbench::TextField::new(&comment))
                        .child(spork_workbench::TextField::new(&passphrase)),
                )
                .confirm()
                .ok_text("Generate key")
                .on_ok(move |_, window, cx| {
                    let read = |s: &Entity<InputState>| s.read(cx).value().to_string();
                    match generate(&dir, read(&name).trim(), read(&comment).trim(), &read(&passphrase)) {
                        Ok(key) => {
                            cx.write_to_clipboard(ClipboardItem::new_string(key.public));
                            window.push_notification(
                                Notification::success("Key generated; its public key is copied: paste it in your server's SSH settings"),
                                cx,
                            );
                            true
                        }
                        Err(e) => {
                            window.push_notification(Notification::error(e), cx);
                            false
                        }
                    }
                })
        });
    }
}
