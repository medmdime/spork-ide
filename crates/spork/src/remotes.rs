//! Remotes in the sidebar: fetch one, add, edit URL, remove, open in browser (docs/specs/M09 §6, M03 §4).

use crate::notices::Notification;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::input::InputState;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::{WindowExt as _, v_flex};
use gpui_kit::*;
use spork_git::Ref;
use spork_git::remote::{self, Remote};
use spork_workbench::tree::TreeItem;

use crate::{RepoView, folder_tree};

impl RepoView {
    /// One folder per remote holding its branches, with the remote's own menu.
    pub(crate) fn remote_items(
        &self,
        remotes: &[Remote],
        branches: &[&Ref],
        open: bool,
        cx: &mut Context<Self>,
    ) -> Vec<TreeItem> {
        let view = cx.entity();
        let (hidden, favorites) = match &self.repo {
            Some(Ok(repo)) => (repo.hidden.clone(), repo.favorites.clone()),
            _ => (Vec::new(), Vec::new()),
        };
        remotes
            .iter()
            .map(|r| {
                let prefix = format!("{}/", r.name);
                let own: Vec<&Ref> = branches
                    .iter()
                    .copied()
                    .filter(|b| b.name.starts_with(&prefix))
                    .collect();
                let (remote, view) = (r.clone(), view.clone());
                TreeItem::new(r.name.clone())
                    .colored_icon("nodes/folder")
                    .click_to_toggle(true)
                    .default_open(open)
                    .children(folder_tree(
                        &own,
                        1,
                        open,
                        self.branches_group_dirs,
                        &crate::RefMarks {
                            hidden: &hidden,
                            favorites: &favorites,
                            picked: self.log_ui.branch_pick.as_ref(),
                        },
                        &view,
                    ))
                    .context_menu(move |menu, _, _| remote_menu(menu, &remote, &view))
            })
            .collect()
    }

    /// "Add remote" dialog; the new remote is fetched right away (M09 §6).
    pub(crate) fn add_remote_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.remote_name
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.clone_url
            .update(cx, |s, cx| s.set_value("", window, cx));
        let (view, name, url) = (
            cx.entity(),
            self.remote_name.clone(),
            self.clone_url.clone(),
        );
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, name, url) = (view.clone(), name.clone(), url.clone());
            dialog
                .title("Add remote")
                .description(
                    v_flex()
                        .gap_2()
                        .child(spork_workbench::TextField::new(&name))
                        .child(spork_workbench::TextField::new(&url)),
                )
                .confirm()
                .ok_text("Add and fetch")
                .on_ok(move |_, window, cx| {
                    let (name, url) = (
                        name.read(cx).value().trim().to_owned(),
                        url.read(cx).value().trim().to_owned(),
                    );
                    view.update(cx, |this, cx| {
                        this.run_op(
                            window,
                            cx,
                            move |root| {
                                remote::add(root, &name, &url)?;
                                remote::fetch(root, &name).map(|()| format!("Added {name}"))
                            },
                            notify_success,
                        )
                    });
                    true
                })
        });
    }

    /// Rename, fetch URL and a separate push URL (M09 §3).
    fn edit_remote_dialog(&mut self, remote: Remote, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let push = remote::push_url(&repo.root, &remote.name).unwrap_or_default();
        let field = |value: String,
                     placeholder: &'static str,
                     window: &mut Window,
                     cx: &mut Context<Self>| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value)
            })
        };
        let new_name = field(remote.name.clone(), "Name", window, cx);
        let url = field(remote.url.clone(), "URL", window, cx);
        let push = field(push, "Push URL (empty: push to the URL above)", window, cx);
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, name) = (view.clone(), remote.name.clone());
            let (new_name, url, push) = (new_name.clone(), url.clone(), push.clone());
            dialog
                .title(format!("Edit {name}"))
                .description(
                    v_flex()
                        .gap_2()
                        .child(spork_workbench::TextField::new(&new_name))
                        .child(spork_workbench::TextField::new(&url))
                        .child(spork_workbench::TextField::new(&push)),
                )
                .confirm()
                .ok_text("Save")
                .on_ok(move |_, window, cx| {
                    let [new_name, url, push] =
                        [&new_name, &url, &push].map(|i| i.read(cx).value().to_string());
                    let name = name.clone();
                    view.update(cx, |this, cx| {
                        this.run_op(
                            window,
                            cx,
                            move |root| {
                                remote::edit(root, &name, &new_name, &url, &push)
                                    .map(|()| format!("Updated {}", new_name.trim()))
                            },
                            notify_success,
                        )
                    });
                    true
                })
        });
    }

    fn confirm_remove_remote(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, name) = (view.clone(), name.clone());
            dialog
                .title(format!("Remove remote {name}?"))
                .description("Only this repository forgets the remote and its remote branches; nothing on the server changes.")
                .confirm()
                .ok_text("Remove")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let name = name.clone();
                    view.update(cx, |this, cx| {
                        this.run_op(
                            window,
                            cx,
                            move |root| remote::remove(root, &name).map(|()| format!("Removed {name}")),
                            notify_success,
                        )
                    });
                    true
                })
        });
    }
}

impl RepoView {
    /// Branch menu "Push to…": remote, remote branch name and options (M09 §3).
    /// `origin/feature/x` → `("origin", "feature/x")`; remote names may contain `/`, so the
    /// longest remote the name starts with wins.
    pub(crate) fn split_remote_branch(&self, name: &str) -> Option<(String, String)> {
        let Some(Ok(repo)) = &self.repo else {
            return None;
        };
        repo.remotes
            .iter()
            .filter_map(|r| {
                Some((
                    r.name.clone(),
                    name.strip_prefix(&format!("{}/", r.name))?.to_owned(),
                ))
            })
            .max_by_key(|(remote, _)| remote.len())
    }
}

fn remote_menu(menu: PopupMenu, remote: &Remote, view: &Entity<RepoView>) -> PopupMenu {
    let action =
        |label: String, f: fn(&mut RepoView, Remote, &mut Window, &mut Context<RepoView>)| {
            let (view, remote) = (view.clone(), remote.clone());
            PopupMenuItem::new(label).on_click(move |_, window, cx| {
                view.update(cx, |this, cx| f(this, remote.clone(), window, cx))
            })
        };
    let (url, web) = (remote.url.clone(), remote::web_url(&remote.url));
    menu.item(action(
        format!("Fetch {}", remote.name),
        |this, r, window, cx| {
            this.run_op(
                window,
                cx,
                move |root| remote::fetch(root, &r.name).map(|()| format!("Fetched {}", r.name)),
                notify_success,
            )
        },
    ))
    .item(action("Test connection".into(), |this, r, window, cx| {
        this.run_op(
            window,
            cx,
            move |root| {
                remote::test_connection(root, &r.name)
                    .map(|n| format!("Connected to {}: {n} branches", r.name))
            },
            notify_success,
        )
    }))
    .item(action("Edit…".into(), RepoView::edit_remote_dialog))
    .item(action("Remove…".into(), |this, r, window, cx| {
        this.confirm_remove_remote(r.name, window, cx)
    }))
    .item(PopupMenuItem::separator())
    .item(
        PopupMenuItem::new("Copy URL").on_click(move |_, window, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(url.clone()));
            window.push_notification(Notification::info("URL copied"), cx);
        }),
    )
    .item(
        PopupMenuItem::new("Open in browser")
            .disabled(web.is_none())
            .on_click(move |_, _, cx| {
                if let Some(web) = &web {
                    cx.open_url(web);
                }
            }),
    )
}

fn notify_success(
    _: &mut RepoView,
    message: String,
    window: &mut Window,
    cx: &mut Context<RepoView>,
) {
    window.push_notification(Notification::success(message), cx);
}
