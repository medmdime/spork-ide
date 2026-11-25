//! File › Manage IDE Settings: IntelliJ's Export Settings… (`ExportSettingsAction`,
//! `ChooseComponentsToExportDialog`) and Import Settings… (`ImportSettingsAction`) over the
//! IDE's configuration folder: the components chosen go into a zip with IntelliJ's marker entry,
//! and come back from one. Zed reloads its settings, keymap, tasks and themes as they change, so
//! an import applies at once, where IntelliJ restarts.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::Context as _;
use async_zip::ZipEntryBuilder;
use async_zip::base::write::ZipFileWriter;
use futures::AsyncWriteExt as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::*;

gpui::actions!(
    settings_dialog,
    [
        /// Writes the chosen settings into a zip (File › Manage IDE Settings › Export Settings…).
        ExportSettings,
        /// Reads the chosen settings back from such a zip (Import Settings…).
        ImportSettings,
    ]
);

/// The zip's entry telling it holds settings, as IntelliJ's names its product's.
const MARKER: &str = "Spork IDE Global Settings";

/// The configuration folder's components, as Export Settings lists them: a name, and its file
/// or folder.
const COMPONENTS: &[(&str, &str)] = &[
    ("Debug Configurations", "debug.json"),
    ("Keymap", "keymap.json"),
    ("Settings", "settings.json"),
    ("Snippets", "snippets"),
    ("Tasks", "tasks.json"),
    ("Themes", "themes"),
];

pub(crate) fn register(workspace: &mut workspace::Workspace) {
    workspace.register_action(|_, _: &ExportSettings, window, cx| export_settings(window, cx));
    workspace.register_action(|_, _: &ImportSettings, window, cx| import_settings(window, cx));
}

/// The components of `folder` that are there, in their (alphabetical) order.
fn present_components(folder: &Path) -> Vec<(&'static str, &'static str)> {
    COMPONENTS
        .iter()
        .copied()
        .filter(|(_, file)| folder.join(file).exists())
        .collect()
}

/// The files of a component, file or folder, as paths relative to `folder`.
fn component_files(folder: &Path, component: &str) -> Vec<PathBuf> {
    let path = folder.join(component);
    if path.is_file() {
        return vec![PathBuf::from(component)];
    }
    let mut files = Vec::new();
    let mut pending = vec![path];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if let Ok(relative) = path.strip_prefix(folder) {
                files.push(relative.to_path_buf());
            }
        }
    }
    files.sort();
    files
}

async fn write_zip(
    folder: PathBuf,
    components: Vec<&'static str>,
    to: PathBuf,
) -> anyhow::Result<()> {
    if let Some(parent) = to.parent() {
        smol::fs::create_dir_all(parent).await?;
    }
    let mut out = smol::fs::File::create(&to)
        .await
        .with_context(|| format!("creating {}", to.display()))?;
    let mut writer = ZipFileWriter::new(&mut out);
    for component in components {
        for file in component_files(&folder, component) {
            let data = smol::fs::read(folder.join(&file)).await?;
            let name = file.to_string_lossy().replace('\\', "/");
            writer
                .write_entry_whole(
                    ZipEntryBuilder::new(name.into(), async_zip::Compression::Deflate),
                    &data,
                )
                .await?;
        }
    }
    writer
        .write_entry_whole(
            ZipEntryBuilder::new(MARKER.to_owned().into(), async_zip::Compression::Stored),
            &[],
        )
        .await?;
    writer.close().await?;
    out.flush().await?;
    Ok(())
}

/// The dialog's list of components with Select All, Select None and Invert, and, for an export,
/// the zip's path.
struct ComponentsDialog {
    title: &'static str,
    components: Vec<(&'static str, &'static str)>,
    chosen: Vec<bool>,
    /// Export's path field; none for an import.
    path: Option<Entity<InputState>>,
    on_ok: Rc<dyn Fn(Vec<&'static str>, Option<PathBuf>, &mut Window, &mut App)>,
    _subscription: Option<Subscription>,
}

impl ComponentsDialog {
    fn chosen(&self) -> Vec<&'static str> {
        self.components
            .iter()
            .zip(&self.chosen)
            .filter(|(_, chosen)| **chosen)
            .map(|((_, file), _)| *file)
            .collect()
    }

    fn path(&self, cx: &App) -> Option<PathBuf> {
        let path = self.path.as_ref()?.read(cx).value().trim().to_owned();
        Some(PathBuf::from(path))
    }

    fn ready(&self, cx: &App) -> bool {
        self.chosen.iter().any(|chosen| *chosen)
            && self
                .path
                .as_ref()
                .is_none_or(|path| !path.read(cx).value().trim().is_empty())
    }

    fn ok(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ready(cx) {
            return;
        }
        let (chosen, path) = (self.chosen(), self.path(cx));
        window.close_dialog(cx);
        (self.on_ok)(chosen, path, window, cx);
    }
}

impl Render for ComponentsDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = spork_workbench::Tokens::get(cx);
        let label = match self.path {
            Some(_) => "Please check the settings to export:",
            None => "Please check all components to import:",
        };
        let rows = self.components.iter().enumerate().map(|(ix, (name, _))| {
            spork_workbench::controls::labeled_check(
                SharedString::from(format!("component-{ix}")),
                *name,
                self.chosen.get(ix).copied().unwrap_or_default(),
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(chosen) = this.chosen.get_mut(ix) {
                    *chosen = !*chosen;
                }
                cx.notify();
            }))
        });
        let set_all = |id: &'static str, text: &'static str, choose: fn(bool) -> bool| {
            spork_workbench::Button::new(id, text).on_click(cx.listener(move |this, _, _, cx| {
                for chosen in &mut this.chosen {
                    *chosen = choose(*chosen);
                }
                cx.notify();
            }))
        };
        let ready = self.ready(cx);
        v_flex()
            .id("components-dialog")
            .role(Role::Group)
            .aria_label(self.title)
            .gap(px(8.))
            .child(spork_workbench::controls::form_label(label))
            .child(
                v_flex()
                    .id("components")
                    .role(Role::List)
                    .aria_label("Components")
                    .h(px(220.))
                    .p(px(6.))
                    .gap(px(4.))
                    .overflow_y_scroll()
                    .border_1()
                    .border_color(tokens.control_border)
                    .rounded(px(spork_workbench::tokens::metrics::RADIUS))
                    .children(rows),
            )
            .children(self.path.as_ref().map(|path| {
                v_flex()
                    .gap(px(4.))
                    .child(spork_workbench::controls::form_label("Export settings to:"))
                    .child(spork_workbench::TextField::new(path).aria_label("Export settings to:"))
            }))
            .child(
                h_flex()
                    .pt(px(8.))
                    .gap(px(8.))
                    .child(set_all("components-select-all", "Select All", |_| true))
                    .child(set_all("components-select-none", "Select None", |_| false))
                    .child(set_all("components-invert", "Invert", |chosen| !chosen))
                    .child(div().flex_1())
                    .child(
                        spork_workbench::Button::new("components-cancel", "Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        spork_workbench::Button::new("components-ok", "OK")
                            .primary(true)
                            .disabled(!ready)
                            .on_click(cx.listener(|this, _, window, cx| this.ok(window, cx))),
                    ),
            )
    }
}

fn open_components_dialog(
    title: &'static str,
    components: Vec<(&'static str, &'static str)>,
    export_to: Option<PathBuf>,
    on_ok: impl Fn(Vec<&'static str>, Option<PathBuf>, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let dialog = cx.new(|cx| {
        let path = export_to.map(|to| {
            cx.new(|cx| {
                let mut input = InputState::new(window, cx);
                input.set_value(to.to_string_lossy().into_owned(), window, cx);
                input
            })
        });
        let _subscription = path.as_ref().map(|path| {
            cx.subscribe_in(
                path,
                window,
                |this: &mut ComponentsDialog, _, event, window, cx| match event {
                    InputEvent::PressEnter { .. } => this.ok(window, cx),
                    InputEvent::Change => cx.notify(),
                    _ => {}
                },
            )
        });
        ComponentsDialog {
            title,
            chosen: vec![true; components.len()],
            components,
            path,
            on_ok: Rc::new(on_ok),
            _subscription,
        }
    });
    window.open_dialog(cx, move |modal, _, _| {
        modal
            .title(spork_workbench::controls::dialog_title(title))
            .on_ok(|_, _, _| false)
            .w(px(480.))
            .child(dialog.clone())
    });
}

/// IntelliJ's message dialog (`Messages.showDialog`): a title, the message, and its buttons, the
/// first the default one; a button runs its action once the dialog closes.
pub(crate) fn message(
    title: &'static str,
    text: String,
    buttons: Vec<(&'static str, Option<Rc<dyn Fn(&mut Window, &mut App)>>)>,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_dialog(cx, move |dialog, _, _| {
        let buttons = buttons.clone();
        dialog
            .title(spork_workbench::controls::dialog_title(title))
            .on_ok(|_, _, _| false)
            .close_button(false)
            .w(px(420.))
            .child(
                v_flex()
                    .id("message-dialog")
                    .role(Role::Group)
                    .aria_label(title)
                    .gap(px(16.))
                    .child(
                        div()
                            .id("message-text")
                            .role(Role::Label)
                            .aria_label(SharedString::from(text.clone()))
                            .child(text.clone()),
                    )
                    .child(h_flex().gap(px(8.)).justify_end().children(
                        buttons.into_iter().enumerate().map(|(ix, (label, run))| {
                            spork_workbench::Button::new(("message-button", ix), label)
                                .primary(ix == 0)
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    if let Some(run) = &run {
                                        run(window, cx);
                                    }
                                })
                        }),
                    )),
            )
    });
}

/// IntelliJ's message after a failed step.
pub(crate) fn report(title: &'static str, text: String, window: &mut Window, cx: &mut App) {
    message(title, text, vec![("OK", None)], window, cx);
}

/// Writes the zip, then tells how it went.
fn write_and_tell(
    folder: PathBuf,
    chosen: Vec<&'static str>,
    to: PathBuf,
    window: &mut Window,
    cx: &mut App,
) {
    window
        .spawn(cx, async move |cx| {
            let written = write_zip(folder, chosen, to.clone()).await;
            cx.update(|window, cx| match written {
                Ok(()) => {
                    let reveal: Rc<dyn Fn(&mut Window, &mut App)> =
                        Rc::new(move |_, cx| cx.reveal_path(&to));
                    message(
                        "Export Complete",
                        "Your settings have been successfully exported.\nYou can import settings using 'File | Manage IDE Settings | Import Settings...'.".into(),
                        vec![("Reveal in Finder", Some(reveal)), ("Close", None)],
                        window,
                        cx,
                    )
                }
                Err(error) => report(
                    "Error Writing File",
                    format!("Error writing settings.\n\n{error:#}"),
                    window,
                    cx,
                ),
            })
        })
        .detach_and_log_err(cx);
}

fn export_settings(window: &mut Window, cx: &mut App) {
    let folder = paths::config_dir().clone();
    let components = present_components(&folder);
    let default = folder.join("settings.zip");
    open_components_dialog(
        "Export Settings",
        components,
        Some(default),
        move |chosen, to, window, cx| {
            let Some(to) = to else { return };
            let folder = folder.clone();
            if !to.exists() {
                write_and_tell(folder, chosen, to, window, cx);
                return;
            }
            let shown = to.display().to_string();
            let overwrite: Rc<dyn Fn(&mut Window, &mut App)> = Rc::new(move |window, cx| {
                write_and_tell(folder.clone(), chosen.clone(), to.clone(), window, cx)
            });
            message(
                "File Already Exists",
                format!("Overwrite '{shown}'?"),
                vec![("Overwrite", Some(overwrite)), ("Cancel", None)],
                window,
                cx,
            );
        },
        window,
        cx,
    );
}

fn import_settings(window: &mut Window, cx: &mut App) {
    let picked = spork_workbench::prompt_for_paths(
        PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        },
        cx,
    );
    window
        .spawn(cx, async move |cx| {
            let Some(zip) = picked.await??.and_then(|paths| paths.into_iter().next()) else {
                return anyhow::Ok(());
            };
            let shown = zip.display().to_string();
            if !zip.is_file() {
                cx.update(|window, cx| {
                    report("File Not Found", format!("Cannot find file {shown}"), window, cx)
                })?;
                return Ok(());
            }
            // the zip's files in a folder of their own, then those chosen into the configuration
            let unpacked = tempfile::tempdir()?;
            let extracted = async {
                let file = smol::fs::File::open(&zip).await?;
                util::archive::extract_zip(unpacked.path(), file).await
            }
            .await;
            let components = present_components(unpacked.path());
            if extracted.is_err() || !unpacked.path().join(MARKER).exists() || components.is_empty() {
                let message = match extracted {
                    Err(error) => format!(
                        "Error reading file {shown}.\nPlease make sure it was generated by 'File | Manage IDE Settings | Export Settings...' feature.\n\n{error:#}"
                    ),
                    Ok(()) => format!(
                        "The file {shown} contains no settings to import.\nPlease make sure it was generated by 'File | Manage IDE Settings | Export Settings...' feature."
                    ),
                };
                cx.update(|window, cx| report("Invalid File", message, window, cx))?;
                return Ok(());
            }
            let unpacked = Rc::new(unpacked);
            cx.update(|window, cx| {
                open_components_dialog(
                    "Select Components to Import",
                    components,
                    None,
                    move |chosen, _, window, cx| {
                        let unpacked = unpacked.clone();
                        let copied: anyhow::Result<()> = chosen.iter().try_for_each(|component| {
                            for file in component_files(unpacked.path(), component) {
                                let to = paths::config_dir().join(&file);
                                if let Some(parent) = to.parent() {
                                    std::fs::create_dir_all(parent)?;
                                }
                                std::fs::copy(unpacked.path().join(&file), &to)
                                    .with_context(|| format!("copying {}", file.display()))?;
                            }
                            Ok(())
                        });
                        if let Err(error) = copied {
                            report(
                                "Error Reading File",
                                format!("Error reading file {shown}.\n\n{error:#}"),
                                window,
                                cx,
                            );
                        }
                    },
                    window,
                    cx,
                )
            })?;
            Ok(())
        })
        .detach_and_log_err(cx);
}

#[cfg(test)]
mod tests {
    use super::{component_files, present_components};

    #[test]
    fn lists_the_components_there() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("settings.json"), "{}").unwrap();
        std::fs::create_dir_all(folder.path().join("themes/a")).unwrap();
        std::fs::write(folder.path().join("themes/a/t.json"), "{}").unwrap();
        let names: Vec<&str> = present_components(folder.path())
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, ["Settings", "Themes"]);
        assert_eq!(
            component_files(folder.path(), "themes"),
            [std::path::PathBuf::from("themes/a/t.json")]
        );
    }
}
