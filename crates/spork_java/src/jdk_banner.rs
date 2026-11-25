//! IntelliJ's "Project JDK is not defined" banner over a Java file's editor
//! (`SdkSetupNotificationProvider`, a warning `EditorNotificationPanel`), and its Setup SDK
//! link's popup: Download JDK… (greyed: no JDK download source yet), Add JDK from Disk…, and the
//! JDKs found.

use editor::Editor;
use gpui::{
    AppContext as _, Context, Entity, EventEmitter, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Role, StatefulInteractiveElement as _, Styled as _, Subscription,
    Window, div, px,
};
use gpui_kit::component::h_flex;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use spork_workbench::Tokens;
use ui::DynamicSpacing;
use util::ResultExt as _;
use workspace::item::ItemHandle;
use workspace::{Pane, ToolbarItemEvent, ToolbarItemLocation, ToolbarItemView, Workspace};

use crate::java_project::JavaProject;
use crate::jdk;
use crate::jdtls::NO_JDK;

/// Adds the banner to the workspace's panes, and to each pane added later.
pub(crate) fn register(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    for pane in workspace.panes().to_vec() {
        add_to(&pane, window, cx);
    }
    cx.subscribe_in(&cx.entity(), window, |_, _, event, window, cx| {
        if let workspace::Event::PaneAdded(pane) = event {
            add_to(pane, window, cx);
        }
    })
    .detach();
}

fn add_to(pane: &Entity<Pane>, window: &mut Window, cx: &mut Context<Workspace>) {
    let banner = cx.new(|_| JdkBanner {
        java: None,
        _subscription: None,
    });
    pane.update(cx, |pane, cx| {
        pane.toolbar()
            .update(cx, |toolbar, cx| toolbar.add_item(banner, window, cx))
    });
}

pub(crate) struct JdkBanner {
    /// The Java project of the active editor's Java file.
    java: Option<Entity<JavaProject>>,
    _subscription: Option<Subscription>,
}

impl JdkBanner {
    fn shown(&self, cx: &gpui::App) -> bool {
        self.java.as_ref().is_some_and(|java| {
            let java = java.read(cx);
            java.root.is_some() && java.jdk.is_none()
        })
    }

    fn location(&self, cx: &gpui::App) -> ToolbarItemLocation {
        match self.shown(cx) {
            true => ToolbarItemLocation::Secondary,
            false => ToolbarItemLocation::Hidden,
        }
    }
}

impl EventEmitter<ToolbarItemEvent> for JdkBanner {}

impl ToolbarItemView for JdkBanner {
    fn set_active_pane_item(
        &mut self,
        item: Option<&dyn ItemHandle>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> ToolbarItemLocation {
        let path = item
            .and_then(|item| item.act_as::<Editor>(cx))
            .and_then(|editor| editor.read(cx).buffer().read(cx).as_singleton())
            .and_then(|buffer| {
                let buffer = buffer.read(cx);
                let file = buffer.file()?;
                (file.path().extension() == Some("java"))
                    .then(|| file.as_local().map(|file| file.abs_path(cx)))?
            });
        self.java = path.and_then(|path| JavaProject::containing(&path, cx));
        self._subscription = self.java.as_ref().map(|java| {
            cx.observe(java, |this, _, cx| {
                cx.emit(ToolbarItemEvent::ChangeLocation(this.location(cx)));
                cx.notify();
            })
        });
        self.location(cx)
    }
}

impl Render for JdkBanner {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (Some(java), true) = (self.java.clone(), self.shown(cx)) else {
            return div().into_any_element();
        };
        let tokens = Tokens::get(cx);
        let found = jdk::find(&jdk::process_env());
        let setup = spork_workbench::controls::DropDownLink::new("setup-sdk", "Setup SDK")
            .without_chevron()
            .dropdown_menu(move |menu, _, _| {
                let add = {
                    let java = java.clone();
                    PopupMenuItem::new("Add JDK from Disk…")
                        .on_click(move |_, window, cx| add_jdk_from_disk(&java, window, cx))
                };
                // shortcut: greyed, IntelliJ's JDK list is a JetBrains service; offer one when
                // a vendor list is decided (the triage's "a JDK download source")
                let menu = menu
                    .item(PopupMenuItem::new("Download JDK…").disabled(true))
                    .item(add);
                found.iter().fold(menu, |menu, found| {
                    let (java, found) = (java.clone(), found.clone());
                    menu.item(
                        PopupMenuItem::new(format!(
                            "{} {} {}",
                            found.name(),
                            found.version,
                            found.home.display()
                        ))
                        .on_click(move |_, _, cx| {
                            java.update(cx, |java, cx| java.set_jdk(found.clone(), cx))
                        }),
                    )
                })
            });
        // the toolbar's padding around it, taken back: the banner spans the editor
        let (padding_x, padding_y) = (
            DynamicSpacing::Base08.rems(cx),
            DynamicSpacing::Base06.rems(cx),
        );
        spork_vcs::spork_look(cx)
            .child(
                h_flex()
                    .id("jdk-banner")
                    .role(Role::Group)
                    .aria_label(NO_JDK)
                    .mx(-padding_x)
                    .my(-padding_y)
                    .py(px(10.))
                    .px(px(12.))
                    .gap(px(8.))
                    .bg(tokens.banner_warning)
                    .border_b_1()
                    .border_color(tokens.banner_warning_border)
                    .text_color(tokens.text)
                    .child(spork_workbench::image("status/warning", cx).size(px(16.)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .pr(px(20.))
                            .child(NO_JDK),
                    )
                    .child(setup),
            )
            .into_any_element()
    }
}

/// Add JDK from Disk…: the system's folder dialog, then the folder made the project's JDK, or
/// IntelliJ's error when it holds none.
fn add_jdk_from_disk(java: &Entity<JavaProject>, window: &mut Window, cx: &mut gpui::App) {
    let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: None,
    });
    let java = java.clone();
    window
        .spawn(cx, async move |cx| {
            let Ok(Ok(Some(mut paths))) = picked.await else {
                return;
            };
            let Some(folder) = paths.pop() else {
                return;
            };
            match jdk::at_folder(&folder) {
                Some(found) => {
                    java.update(cx, |java, cx| java.set_jdk(found, cx));
                }
                None => {
                    let answer = cx.update(|window, cx| {
                        window.prompt(
                            gpui::PromptLevel::Critical,
                            "The selected directory is not a valid home for JDK",
                            None,
                            &["OK"],
                            cx,
                        )
                    });
                    if let Ok(answer) = answer {
                        answer.await.log_err();
                    }
                }
            }
        })
        .detach();
}
