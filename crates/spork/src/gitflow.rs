//! Git-flow from the repository menu and the branch menu (docs/specs/M13).

use crate::notices::Notification;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, v_flex};
use gpui_kit::*;
use spork_git::Outcome;
use spork_git::gitflow::{self, Gitflow, Kind};

use crate::RepoView;

fn noun(kind: Kind) -> &'static str {
    match kind {
        Kind::Feature => "feature",
        Kind::Bugfix => "bugfix",
        Kind::Release => "release",
        Kind::Hotfix => "hotfix",
    }
}

impl RepoView {
    fn gitflow(&self) -> Option<&Gitflow> {
        match &self.repo {
            Some(Ok(repo)) => repo.gitflow.as_ref(),
            _ => None,
        }
    }

    /// Repository ▸ Git Flow ▸ Start …: initializes git-flow first if the repository lacks it.
    pub(crate) fn gitflow_start(
        &mut self,
        kind: Kind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.gitflow().cloned() {
            Some(flow) => self.start_dialog(flow, kind, window, cx),
            None => self.run_op(window, cx, gitflow::init, move |this, flow, window, cx| {
                this.start_dialog(flow, kind, window, cx)
            }),
        }
    }

    fn start_dialog(
        &mut self,
        flow: Gitflow,
        kind: Kind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placeholder = match kind {
            Kind::Feature | Kind::Bugfix => "my-change",
            Kind::Release | Kind::Hotfix => "1.2.0",
        };
        self.flow_name.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.set_placeholder(placeholder, window, cx);
        });
        let (view, input) = (cx.entity(), self.flow_name.clone());
        let muted = cx.theme().muted_foreground;
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, input, flow) = (view.clone(), input.clone(), flow.clone());
            dialog
                .title(format!("Start {}", noun(kind)))
                .description(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().text_color(muted).child(format!(
                            "Creates {}<name> at {}, then switches to it.",
                            flow.prefix(kind),
                            flow.base(kind)
                        )))
                        .child(spork_workbench::TextField::new(&input)),
                )
                .confirm()
                .ok_text("Start")
                .on_ok(move |_, window, cx| {
                    let (name, flow) = (input.read(cx).value().to_string(), flow.clone());
                    view.update(cx, |this, cx| {
                        this.run_op(
                            window,
                            cx,
                            move |root| gitflow::start(root, &flow, kind, &name),
                            |_, branch, window, cx| {
                                window.push_notification(
                                    Notification::success(format!("Started {branch}")),
                                    cx,
                                )
                            },
                        )
                    });
                    true
                })
        });
    }

    /// Branch menu: "Finish feature …" at the end when `branch` is a git-flow branch.
    pub(crate) fn finish_item(
        menu: PopupMenu,
        view: &Entity<Self>,
        branch: &str,
        cx: &App,
    ) -> PopupMenu {
        let Some(flow) = view.read(cx).gitflow().cloned() else {
            return menu;
        };
        let Some((kind, name)) = flow.kind_of(branch).map(|(k, n)| (k, n.to_owned())) else {
            return menu;
        };
        let view = view.clone();
        menu.separator().item(
            PopupMenuItem::new(format!("Finish {} {name}…", noun(kind))).on_click(
                move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        this.finish_dialog(flow.clone(), kind, name.clone(), window, cx)
                    })
                },
            ),
        )
    }

    fn finish_dialog(
        &mut self,
        flow: Gitflow,
        kind: Kind,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let branch = format!("{}{name}", flow.prefix(kind));
        let steps = if matches!(kind, Kind::Feature | Kind::Bugfix) {
            format!("Merges {branch} into {} with a merge commit.", flow.develop)
        } else {
            let back = if kind == Kind::Hotfix {
                format!("{} (or the open release branch)", flow.develop)
            } else {
                flow.develop.clone()
            };
            format!(
                "Merges {branch} into {}, tags it {}{name}, merges the tag back into {back}.",
                flow.master, flow.tag
            )
        };
        // the tag message (release / hotfix only) reuses the "Create tag" field
        let message =
            matches!(kind, Kind::Release | Kind::Hotfix).then(|| self.tag_message.clone());
        self.finish_delete = true;
        if let Some(input) = &message {
            let text = format!(
                "{} {name}",
                if kind == Kind::Release {
                    "Release"
                } else {
                    "Hotfix"
                }
            );
            input.update(cx, |s, cx| s.set_value(text, window, cx));
        }
        let view = cx.entity();
        let muted = cx.theme().muted_foreground;
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let delete = view.read(cx).finish_delete;
            let check_view = view.clone();
            let (view, flow, name, message) =
                (view.clone(), flow.clone(), name.clone(), message.clone());
            dialog
                .title(format!("Finish {} {name}", noun(kind)))
                .description(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().text_color(muted).child(steps.clone()))
                        .children(message.as_ref().map(Input::new))
                        .child(
                            Checkbox::new("flow-finish-delete")
                                .label(format!("Delete {branch}"))
                                .checked(delete)
                                .on_click(move |checked, _, cx| {
                                    check_view.update(cx, |this, cx| {
                                        this.finish_delete = *checked;
                                        cx.notify();
                                    })
                                }),
                        ),
                )
                .confirm()
                .ok_text("Finish")
                .on_ok(move |_, window, cx| {
                    let text = message
                        .as_ref()
                        .map(|m| m.read(cx).value().to_string())
                        .unwrap_or_default();
                    let (flow, name) = (flow.clone(), name.clone());
                    let done = format!("Finished {} {name}", noun(kind));
                    view.update(cx, |this, cx| {
                        let keep = !this.finish_delete;
                        this.run_op(
                            window,
                            cx,
                            move |root| gitflow::finish(root, &flow, kind, &name, &text, keep),
                            move |this, outcome, window, cx| match outcome {
                                Outcome::Done => {
                                    window.push_notification(Notification::success(done), cx)
                                }
                                Outcome::Conflicts => {
                                    this.open_left(crate::LeftTool::Commit, cx);
                                    window.push_notification(
                                        Notification::warning(
                                            "Stopped on conflicts. Resolve them, Continue, then choose Finish again to run the remaining steps.",
                                        ),
                                        cx,
                                    );
                                }
                            },
                        )
                    });
                    true
                })
        });
    }
}
