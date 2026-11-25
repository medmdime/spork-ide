//! The dialogs the Welcome window, the Recent Projects popup and the File menu share: New
//! Project (IntelliJ's Empty Project), Get from Version Control (the reference app's Clone dialog, its
//! Repository URL source), and a confirmation.

use std::path::{Path, PathBuf};

use gpui_kit::component::dialog::Dialog;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_workbench::controls::dialog_title as title;
use spork_workbench::tokens::{Tokens, Typography as _};

/// Where IntelliJ-based IDEs suggest new projects go (IntelliJ IDEA's `~/IdeaProjects`).
pub fn projects_folder() -> PathBuf {
    util::paths::home_dir().join("SporkIdeProjects")
}

/// A form's label, in front of its field, the labels of a form one width.
fn label(text: &'static str) -> Stateful<Div> {
    spork_workbench::controls::form_label(text).w(px(90.))
}

/// A field's folder button: picks a folder for `field`.
fn browse(id: &'static str, field: Entity<InputState>) -> spork_workbench::ActionButton {
    spork_workbench::ActionButton::new(id, "nodes/folder")
        .tooltip("Browse…")
        .on_click(move |_, window, cx| {
            let picked = cx.prompt_for_paths(PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: None,
            });
            let field = field.clone();
            window
                .spawn(cx, async move |cx| {
                    if let Ok(Ok(Some(mut paths))) = picked.await
                        && let Some(folder) = paths.pop()
                    {
                        cx.update(|window, cx| {
                            field.update(cx, |field, cx| {
                                field.set_value(folder.display().to_string(), window, cx)
                            })
                        })
                        .ok();
                    }
                })
                .detach();
        })
}

/// IntelliJ's paths start with `~` for the home folder: the fields take them that way too.
fn expand_home(text: &str) -> PathBuf {
    match text.trim().strip_prefix("~/") {
        Some(rest) => util::paths::home_dir().join(rest),
        None => PathBuf::from(text.trim()),
    }
}

/// Whether `folder` can take a new project: missing, or empty.
pub fn usable(folder: &Path) -> bool {
    std::fs::read_dir(folder).map_or(true, |mut entries| entries.next().is_none())
}

/// A row of the New Project dialog's list: a generator's icon and name.
fn generator_row(
    id: impl Into<ElementId>,
    icon: Option<&'static str>,
    name: SharedString,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    let tokens = Tokens::get(cx);
    h_flex()
        .id(id)
        .role(Role::ListItem)
        .aria_label(name.clone())
        .aria_selected(selected)
        .h(px(28.))
        .px(px(8.))
        .gap(px(6.))
        .rounded(px(spork_workbench::tokens::metrics::RADIUS))
        .cursor_pointer()
        .when(selected, |row| row.bg(tokens.selection))
        .when(!selected, |row| {
            row.hover(move |row| row.bg(tokens.selection_hover))
        })
        .when_some(icon, |row, icon| {
            row.child(spork_workbench::image(icon, cx).size(px(16.)))
        })
        .child(name)
}

/// New Project: IntelliJ's Empty Project, a folder (made under the location) with an optional
/// git repository, opened in a window of its own; the generators other crates add
/// (`crate::register_generator`) are listed beside it, each showing its own page.
pub(crate) fn new_project(window: &mut Window, cx: &mut App) {
    let generators = cx
        .try_global::<crate::Generators>()
        .map(|generators| generators.0.clone())
        .unwrap_or_default();
    // the generator shown, none for Empty Project, and the pages shown so far
    let chosen = cx.new(|_| None::<usize>);
    let pages = cx.new(|_| std::collections::HashMap::<usize, AnyView>::new());
    let name = cx.new(|cx| InputState::new(window, cx).default_value("untitled"));
    let location = cx.new(|cx| {
        InputState::new(window, cx).default_value(projects_folder().display().to_string())
    });
    let git = cx.new(|_| false);
    for field in [&name, &location] {
        window
            .subscribe(field, cx, |_, event: &InputEvent, window, _| {
                if matches!(event, InputEvent::Change) {
                    window.refresh();
                }
            })
            .detach();
    }
    let focused = name.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let tokens = Tokens::get(cx);
        let shown = *chosen.read(cx);
        let page = shown.and_then(|ix| {
            let generator = generators.get(ix)?;
            Some(pages.update(cx, |pages, cx| {
                pages
                    .entry(ix)
                    .or_insert_with(|| (generator.page)(window, cx))
                    .clone()
            }))
        });
        let list = (!generators.is_empty()).then(|| {
            let choose = |ix: Option<usize>| {
                let chosen = chosen.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    chosen.update(cx, |chosen, _| *chosen = ix);
                    window.refresh();
                }
            };
            v_flex()
                .w(px(200.))
                .flex_none()
                .gap(px(2.))
                .child(
                    generator_row(
                        "new-project-empty",
                        None,
                        "Empty Project".into(),
                        shown.is_none(),
                        cx,
                    )
                    .on_click(choose(None)),
                )
                .child(
                    spork_workbench::controls::form_label("Generators")
                        .pt(px(10.))
                        .pb(px(4.))
                        .px(px(8.))
                        .text_ui_small()
                        .text_color(tokens.text_secondary),
                )
                .children(generators.iter().enumerate().map(|(ix, generator)| {
                    generator_row(
                        ("new-project-generator", ix),
                        Some(generator.icon),
                        generator.name.clone(),
                        shown == Some(ix),
                        cx,
                    )
                    .on_click(choose(Some(ix)))
                }))
        });
        let dialog = dialog
            .title(title("New Project"))
            // IntelliJ's dialog is a window of its own; this one stays inside the window it is in
            .w(match list {
                Some(_) => px(860.).min(window.viewport_size().width - px(48.)),
                None => px(560.),
            })
            .close_button(false);
        if let Some(page) = page {
            return dialog.on_ok(|_, _, _| false).child(
                h_flex()
                    .items_start()
                    .gap(px(16.))
                    .children(list)
                    .child(div().flex_1().min_w_0().child(page)),
            );
        }
        let project = name.read(cx).value().trim().to_owned();
        let folder = expand_home(&location.read(cx).value()).join(&project);
        let problem = (!project.is_empty() && !usable(&folder)).then(|| {
            format!(
                "Directory {} is not empty in this location",
                folder.display()
            )
        });
        let ready = !project.is_empty() && problem.is_none();
        let git_on = *git.read(cx);
        let toggle = git.clone();
        let (create_folder, create_git) = (folder.clone(), git.clone());
        let create = move |window: &mut Window, cx: &mut App| {
            let (folder, git) = (create_folder.clone(), *create_git.read(cx));
            window.close_dialog(cx);
            create_project(folder, git, window, cx);
        };
        let on_enter = create.clone();
        dialog
            .on_ok(move |_, window, cx| {
                if ready {
                    on_enter(window, cx);
                }
                false
            })
            .child(
                h_flex().items_start().gap(px(16.)).children(list).child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(12.))
                        .child(
                            h_flex().child(label("Name:")).child(
                                spork_workbench::TextField::new(&name)
                                    .aria_label("Name:")
                                    .flex_1(),
                            ),
                        )
                        .child(
                            h_flex().child(label("Location:")).child(
                                spork_workbench::TextField::new(&location)
                                    .aria_label("Location:")
                                    .suffix(browse("new-project-browse", location.clone()))
                                    .flex_1(),
                            ),
                        )
                        .child(
                            div()
                                .pl(px(90.))
                                .text_ui_small()
                                .text_color(tokens.text_secondary)
                                .child(format!(
                                    "Project will be created in: {}",
                                    util::paths::PathExt::compact(&folder).display()
                                )),
                        )
                        .children(problem.map(|problem| {
                            div()
                                .pl(px(90.))
                                .text_ui_small()
                                .text_color(tokens.text_error)
                                .child(problem)
                        }))
                        .child(
                            div().pl(px(90.)).child(
                                spork_workbench::controls::labeled_check(
                                    "new-project-git",
                                    "Create Git repository",
                                    git_on,
                                    cx,
                                )
                                .on_click(move |_, window, cx| {
                                    toggle.update(cx, |on, _| *on = !*on);
                                    window.refresh();
                                }),
                            ),
                        )
                        .child(
                            h_flex()
                                .pt(px(8.))
                                .gap(px(8.))
                                .justify_end()
                                .child(
                                    spork_workbench::Button::new("new-project-cancel", "Cancel")
                                        .on_click(|_, window, cx| window.close_dialog(cx)),
                                )
                                .child(
                                    spork_workbench::Button::new("new-project-create", "Create")
                                        .primary(true)
                                        .disabled(!ready)
                                        .on_click(move |_, window, cx| create(window, cx)),
                                ),
                        ),
                ),
            )
    });
    focused.update(cx, |name, cx| name.focus(window, cx));
}

/// Makes the project's folder (and its repository), then opens it.
pub fn create_project(folder: PathBuf, git: bool, window: &mut Window, cx: &mut App) {
    let work = cx.background_spawn({
        let folder = folder.clone();
        async move {
            std::fs::create_dir_all(&folder)?;
            if git {
                spork_git::init(&folder).map_err(|e| anyhow::anyhow!("{e}"))?;
            }
            anyhow::Ok(())
        }
    });
    window
        .spawn(cx, async move |cx| {
            let result = work.await;
            cx.update(|window, cx| match result {
                Ok(()) => crate::open_project(vec![folder], cx),
                Err(e) => window.push_notification(
                    Notification::error(format!("{e:#}")).title("Cannot Create Project"),
                    cx,
                ),
            })
            .ok();
        })
        .detach();
}

/// Get from Version Control: the reference app's Clone dialog on its Repository URL source: the URL, the
/// folder it goes in (which follows the URL until edited), a shallow clone's depth.
// shortcut: the GitHub and GitLab sources need Spork's accounts (crates/spork); they come with
// the version control's Clone dialog
pub(crate) fn get_from_vcs(window: &mut Window, cx: &mut App) {
    let parent = projects_folder();
    let url = cx.new(|cx| InputState::new(window, cx));
    let folder =
        cx.new(|cx| InputState::new(window, cx).default_value(parent.display().to_string()));
    let depth = cx.new(|cx| InputState::new(window, cx).default_value("1"));
    let shallow = cx.new(|_| false);
    window
        .subscribe(&url, cx, {
            let folder = folder.clone();
            move |url, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    let name = spork_git::repo_name_from_url(&url.read(cx).value());
                    let path = parent.join(name).display().to_string();
                    folder.update(cx, |folder, cx| folder.set_value(path, window, cx));
                    window.refresh();
                }
            }
        })
        .detach();
    let focused = url.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let tokens = Tokens::get(cx);
        let ready = !url.read(cx).value().trim().is_empty();
        let shallow_on = *shallow.read(cx);
        let toggle = shallow.clone();
        let (ok_url, ok_folder, ok_depth, ok_shallow) =
            (url.clone(), folder.clone(), depth.clone(), shallow.clone());
        let clone = move |window: &mut Window, cx: &mut App| {
            let url = ok_url.read(cx).value().trim().to_owned();
            if url.is_empty() {
                return;
            }
            let dest = expand_home(&ok_folder.read(cx).value());
            let depth = ok_depth.read(cx).value().trim().parse::<u32>().ok();
            let depth = depth.filter(|_| *ok_shallow.read(cx));
            window.close_dialog(cx);
            clone_repository(url, dest, depth, window, cx);
        };
        let on_enter = clone.clone();
        let source = h_flex()
            .id("clone-source")
            .role(Role::Tab)
            .aria_label("Repository URL")
            .aria_selected(true)
            .px(px(12.))
            .py(px(8.))
            .gap(px(8.))
            .bg(tokens.selection)
            .child(spork_workbench::icon("general/vcs").size(px(16.)))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Repository URL"),
            );
        let form = v_flex()
            .flex_1()
            .p(px(16.))
            .gap(px(12.))
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(spork_workbench::controls::form_label("Version control:"))
                    .child(spork_workbench::controls::combo(
                        "clone-vcs",
                        "Git",
                        false,
                        |menu, _, _| menu.item(PopupMenuItem::new("Git").checked(true)),
                    )),
            )
            .child(
                h_flex().child(label("URL:")).child(
                    spork_workbench::TextField::new(&url)
                        .aria_label("URL:")
                        .flex_1(),
                ),
            )
            .child(
                h_flex().child(label("Clone into:")).child(
                    spork_workbench::TextField::new(&folder)
                        .aria_label("Clone into:")
                        .suffix(browse("clone-browse", folder.clone()))
                        .flex_1(),
                ),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(
                        spork_workbench::controls::labeled_check(
                            "clone-shallow",
                            "Shallow clone with a history truncated to",
                            shallow_on,
                            cx,
                        )
                        .on_click(move |_, window, cx| {
                            toggle.update(cx, |on, _| *on = !*on);
                            window.refresh();
                        }),
                    )
                    .child(
                        div()
                            .w(px(64.))
                            .child(spork_workbench::TextField::new(&depth).disabled(!shallow_on)),
                    )
                    .child("commits"),
            );
        let footer = h_flex()
            .p(px(12.))
            .gap(px(8.))
            .justify_end()
            .border_t_1()
            .border_color(tokens.separator)
            .child(
                spork_workbench::Button::new("clone-cancel", "Cancel")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                spork_workbench::Button::new("clone-ok", "Clone")
                    .primary(true)
                    .disabled(!ready)
                    .on_click(move |_, window, cx| clone(window, cx)),
            );
        dialog
            .title(
                div()
                    .px(px(16.))
                    .pt(px(12.))
                    .child(title("Get from Version Control")),
            )
            .w(px(820.))
            .p_0()
            .close_button(false)
            .on_ok(move |_, window, cx| {
                on_enter(window, cx);
                false
            })
            .child(
                v_flex()
                    .h(px(420.))
                    .child(
                        h_flex()
                            .flex_1()
                            .items_start()
                            .child(
                                v_flex()
                                    .w(px(200.))
                                    .h_full()
                                    .border_r_1()
                                    .border_color(tokens.separator)
                                    .child(source),
                            )
                            .child(form),
                    )
                    .child(footer),
            )
    });
    focused.update(cx, |url, cx| url.focus(window, cx));
}

/// Clones `url` into `dest` in the background, then opens it; a failure shows as a
/// notification, as IntelliJ's "Clone failed" balloon.
fn clone_repository(
    url: String,
    dest: PathBuf,
    depth: Option<u32>,
    window: &mut Window,
    cx: &mut App,
) {
    let work = cx.background_spawn(async move {
        let parent = dest
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow::anyhow!("{} has no parent folder", dest.display()))?;
        let name = dest
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or_else(|| anyhow::anyhow!("{} has no name", dest.display()))?;
        std::fs::create_dir_all(&parent)?;
        spork_git::clone(&url, &parent, &name, "", depth).map_err(|e| anyhow::anyhow!("{e}"))
    });
    window
        .spawn(cx, async move |cx| {
            let result = work.await;
            cx.update(|window, cx| match result {
                Ok(cloned) => crate::open_project(vec![cloned], cx),
                Err(e) => window.push_notification(
                    Notification::error(format!("{e:#}")).title("Clone failed"),
                    cx,
                ),
            })
            .ok();
        })
        .detach();
}

/// A question with Cancel and its `ok` button, as IntelliJ's `Messages.showYesNoDialog`.
pub(crate) fn confirm(
    dialog: Dialog,
    heading: &'static str,
    message: String,
    ok: &'static str,
    on_ok: impl Fn(&mut Window, &mut App) + Clone + 'static,
) -> Dialog {
    let on_enter = on_ok.clone();
    dialog
        .title(title(heading))
        .w(px(420.))
        .close_button(false)
        .on_ok(move |_, window, cx| {
            on_enter(window, cx);
            true
        })
        .child(
            v_flex()
                .gap(px(16.))
                .child(
                    div()
                        .id("dialog-message")
                        .role(Role::Label)
                        .aria_label(message.clone())
                        .child(message),
                )
                .child(
                    h_flex()
                        .gap(px(8.))
                        .justify_end()
                        .child(
                            spork_workbench::Button::new("confirm-cancel", "Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            spork_workbench::Button::new("confirm-ok", ok)
                                .primary(true)
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    on_ok(window, cx);
                                }),
                        ),
                ),
        )
}
