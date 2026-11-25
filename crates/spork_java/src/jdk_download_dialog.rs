//! IntelliJ's Download JDK dialog (`JdkDownloadDialog`): "Version:", "Vendor:" (the vendors of
//! the version by name, then under "Other Versions" the others' nearest release, which picking
//! switches the version to) and "Location:" (IntelliJ's folder for the vendor and version, its
//! archive's size under it), checked as IntelliJ checks it; Download starts the install in the
//! background (`jdk_download::start`).

use std::path::PathBuf;
use std::rc::Rc;

use collections::HashMap;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, Global, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Role, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div, px,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use spork_workbench::controls::{ComboBox, dialog_title, dropdown, form_label};
use spork_workbench::{Tokens, Typography as _};
use util::ResultExt as _;

use crate::jdk_download::{self, Catalog, Package};

const TITLE: &str = "Download JDK";

/// What Download does with the package and the folder chosen.
pub(crate) type OnDownload = Rc<dyn Fn(Package, PathBuf, &mut Window, &mut App)>;

/// The catalogue fetched once a session, as IntelliJ keeps its JDK list.
struct Fetched(Catalog);

impl Global for Fetched {}

/// Opens the dialog in `window` once the JDK list is read (IntelliJ's "Downloading JDK List"),
/// or tells why there is none.
pub(crate) fn open(on_download: OnDownload, window: &mut Window, cx: &mut App) {
    if let Some(Fetched(catalog)) = cx.try_global::<Fetched>() {
        let catalog = catalog.clone();
        show(catalog, on_download, window, cx);
        return;
    }
    let http = cx.http_client();
    let fetching = cx.background_spawn(async move { jdk_download::fetch_catalog(&http).await });
    window
        .spawn(cx, async move |cx| {
            let fetched = fetching.await;
            cx.update(|window, cx| match fetched {
                Ok(catalog) => {
                    cx.set_global(Fetched(catalog.clone()));
                    show(catalog, on_download, window, cx);
                }
                Err(error) => {
                    log::error!("reading the JDK list: {error:#}");
                    let answer = window.prompt(
                        gpui::PromptLevel::Critical,
                        TITLE,
                        Some("No JDK packages are available for download"),
                        &["OK"],
                        cx,
                    );
                    cx.background_spawn(async move { answer.await.log_err() })
                        .detach();
                }
            })
            .log_err();
        })
        .detach();
}

fn show(catalog: Catalog, on_download: OnDownload, window: &mut Window, cx: &mut App) {
    let Some(first) = catalog.default_package().cloned() else {
        return;
    };
    let dialog = cx.new(|cx| {
        let location = cx.new(|cx| InputState::new(window, cx));
        let _subscription = cx.subscribe_in(
            &location,
            window,
            |this: &mut DownloadDialog, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.error = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.download(window, cx),
                _ => {}
            },
        );
        let mut dialog = DownloadDialog {
            catalog,
            version: first.major,
            package: first.clone(),
            chosen: HashMap::default(),
            location,
            error: None,
            on_download,
            _subscription,
        };
        dialog.choose(first, window, cx);
        dialog
    });
    window.open_dialog(cx, move |modal, _, _| {
        modal
            .title(dialog_title(TITLE))
            .on_ok(|_, _, _| false)
            .w(px(DIALOG_WIDTH))
            .close_button(false)
            .child(dialog.clone())
    });
}

/// IntelliJ's dialog: its Location field is 36 columns, which sizes it.
const DIALOG_WIDTH: f32 = 520.;
/// The labels' column ("Location:" and the gap after it).
const LABEL_WIDTH: f32 = 72.;

struct DownloadDialog {
    catalog: Catalog,
    version: u32,
    package: Package,
    /// The vendor picked for each version, as IntelliJ keeps each version's selection.
    chosen: HashMap<u32, Package>,
    location: Entity<InputState>,
    /// IntelliJ's message for the Location, once Download found it wrong.
    error: Option<&'static str>,
    on_download: OnDownload,
    _subscription: Subscription,
}

impl DownloadDialog {
    /// Shows `version`'s vendor, the one picked before or the first of IntelliJ's list.
    fn set_version(&mut self, version: u32, window: &mut Window, cx: &mut Context<Self>) {
        let package = self
            .chosen
            .get(&version)
            .cloned()
            .or_else(|| self.catalog.default_for(version).cloned());
        if let Some(package) = package {
            self.choose(package, window, cx);
        }
    }

    /// Picks `package` (switching to its version) and sets the Location to IntelliJ's folder
    /// for it.
    fn choose(&mut self, package: Package, window: &mut Window, cx: &mut Context<Self>) {
        let target = jdk_download::install_dir(&jdk_download::default_folder(), &package);
        self.version = package.major;
        self.chosen.insert(package.major, package.clone());
        self.package = package;
        self.location.update(cx, |location, cx| {
            location.set_value(jdk_download::shown_path(&target), window, cx)
        });
        self.error = None;
        cx.notify();
    }

    fn download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.location.read(cx).value().to_string();
        match jdk_download::validate_install_dir(&text) {
            Ok(target) => {
                window.close_dialog(cx);
                (self.on_download)(self.package.clone(), target, window, cx);
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }
}

/// A row of the form: its label, then its control.
fn row(label: &'static str, control: impl IntoElement) -> gpui::Div {
    h_flex()
        .gap(px(6.))
        .child(form_label(label).w(px(LABEL_WIDTH)))
        .child(div().flex_1().min_w_0().child(control))
}

/// A vendor's item: its name, its version and architecture greyed (IntelliJ's renderer).
fn vendor_item(package: &Package) -> impl Fn(&mut Window, &mut App) -> gpui::AnyElement + use<> {
    let (name, version, arch) = (
        package.vendor.name,
        SharedString::from(package.version.clone()),
        package
            .shown_arch()
            .map(|arch| SharedString::from(arch.to_owned())),
    );
    move |_, cx| {
        let muted = Tokens::get(cx).text_secondary;
        h_flex()
            .id(name)
            .role(Role::Label)
            .aria_label(name)
            .gap(px(4.))
            .child(name)
            .child(div().text_color(muted).child(version.clone()))
            .children(arch.clone().map(|arch| div().text_color(muted).child(arch)))
            .into_any_element()
    }
}

impl Render for DownloadDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let this = cx.entity();
        let versions = {
            let (this, versions) = (this.clone(), self.catalog.versions());
            dropdown(
                ComboBox::new("jdk-version", crate::jdk::feature_text(self.version))
                    .aria_label("Version:")
                    .w_full(),
                false,
                move |menu, _, _| {
                    versions.iter().fold(menu, |menu, version| {
                        let (this, version) = (this.clone(), *version);
                        menu.item(
                            PopupMenuItem::new(crate::jdk::feature_text(version)).on_click(
                                move |_, window, cx| {
                                    this.update(cx, |this, cx| {
                                        this.set_version(version, window, cx)
                                    })
                                },
                            ),
                        )
                    })
                },
            )
        };
        let vendors = {
            let (included, excluded) = self.catalog.vendors(self.version);
            let lists: Rc<(Vec<Package>, Vec<Package>)> = Rc::new((
                included.into_iter().cloned().collect(),
                excluded.into_iter().cloned().collect(),
            ));
            let detail = match self.package.shown_arch() {
                Some(arch) => format!("{} {arch}", self.package.version),
                None => self.package.version.clone(),
            };
            dropdown(
                ComboBox::new("jdk-vendor", self.package.vendor.name)
                    .aria_label("Vendor:")
                    .detail(detail)
                    .w_full(),
                false,
                move |mut menu, _, _| {
                    let item = |package: &Package| {
                        let (this, package) = (this.clone(), package.clone());
                        PopupMenuItem::element(vendor_item(&package)).on_click(
                            move |_, window, cx| {
                                this.update(cx, |this, cx| this.choose(package.clone(), window, cx))
                            },
                        )
                    };
                    for package in &lists.0 {
                        menu = menu.item(item(package));
                    }
                    if !lists.1.is_empty() {
                        menu = menu.item(PopupMenuItem::label("Other Versions"));
                        for package in &lists.1 {
                            menu = menu.item(item(package));
                        }
                    }
                    menu
                },
            )
        };
        let browse = {
            let location = self.location.clone();
            spork_workbench::ActionButton::new("jdk-location-browse", "nodes/folder")
                .tooltip("Browse")
                .on_click(move |_, window, cx| {
                    let picked = spork_workbench::prompt_for_paths(
                        gpui::PathPromptOptions {
                            files: false,
                            directories: true,
                            multiple: false,
                            prompt: Some("Select Path to Install JDK".into()),
                        },
                        cx,
                    );
                    let location = location.clone();
                    window
                        .spawn(cx, async move |cx| {
                            let Ok(Ok(Some(mut paths))) = picked.await else {
                                return;
                            };
                            let Some(folder) = paths.pop() else { return };
                            cx.update(|window, cx| {
                                location.update(cx, |location, cx| {
                                    location.set_value(
                                        jdk_download::shown_path(&folder),
                                        window,
                                        cx,
                                    )
                                })
                            })
                            .log_err();
                        })
                        .detach();
                })
        };
        v_flex()
            .id("download-jdk")
            .role(Role::Group)
            .aria_label(TITLE)
            .gap(px(10.))
            .child(row("Version:", versions))
            .child(row("Vendor:", vendors))
            .child(
                v_flex()
                    .gap(px(4.))
                    .child(row(
                        "Location:",
                        spork_workbench::TextField::new(&self.location)
                            .aria_label("Location:")
                            .suffix(browse),
                    ))
                    .child(
                        div()
                            .id("jdk-archive-size")
                            .role(Role::Label)
                            .aria_label(SharedString::from(self.package.size_text()))
                            .pl(px(LABEL_WIDTH + 6.))
                            .text_ui_small()
                            .text_color(tokens.text_secondary)
                            .child(self.package.size_text()),
                    )
                    .when_some(self.error, |form, error| {
                        form.child(
                            div()
                                .id("jdk-location-error")
                                .role(Role::Label)
                                .aria_label(error)
                                .pl(px(LABEL_WIDTH + 6.))
                                .text_ui_small()
                                .text_color(tokens.text_error)
                                .child(error),
                        )
                    }),
            )
            .child(
                h_flex()
                    .pt(px(10.))
                    .gap(px(8.))
                    .justify_end()
                    .child(
                        spork_workbench::Button::new("jdk-download-cancel", "Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        spork_workbench::Button::new("jdk-download", "Download")
                            .primary(true)
                            .on_click(cx.listener(|this, _, window, cx| this.download(window, cx))),
                    ),
            )
    }
}

/// Download JDK… for a project (the banner's Setup SDK): the JDK downloaded becomes its JDK.
pub(crate) fn download_for_project(
    java: &Entity<crate::JavaProject>,
    window: &mut Window,
    cx: &mut App,
) {
    let (java, workspace) = (java.downgrade(), java.read(cx).workspace.clone());
    open(
        Rc::new(move |package, target, _, cx| {
            let java = java.clone();
            jdk_download::start(
                package,
                target,
                Box::new(move |jdk, cx| {
                    java.update(cx, |java, cx| java.set_jdk(jdk, cx)).log_err();
                }),
                Some(workspace.clone()),
                cx,
            )
        }),
        window,
        cx,
    );
}
