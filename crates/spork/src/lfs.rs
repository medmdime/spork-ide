//! Repository ▸ Git LFS: pull, fetch, track, untrack, list files (docs/specs/M14).

use crate::notices::Notification;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;
use spork_git::lfs;

use crate::RepoView;

// shortcut: a dialog lists this many LFS files; switch to a virtual list if bigger repos need it.
const MAX_LISTED: usize = 500;

impl RepoView {
    /// Items of the repository menu's "Git LFS" submenu.
    /// Downloads the LFS files of the checked-out commit (`pull` also puts them in the working
    /// copy) from the default remote.
    pub(crate) fn lfs_transfer(&mut self, pull: bool, window: &mut Window, cx: &mut Context<Self>) {
        let remote = self.default_remote().unwrap_or_else(|| "origin".into());
        self.run_op(
            window,
            cx,
            move |root| {
                if pull {
                    lfs::pull(root, &remote).map(|()| format!("Pulled LFS files from {remote}"))
                } else {
                    lfs::fetch(root, &remote).map(|()| format!("Fetched LFS files from {remote}"))
                }
            },
            notify,
        )
    }

    /// Git ▸ Git LFS ▸ Track Pattern… / Untrack Pattern….
    pub(crate) fn lfs_track_dialog(
        &mut self,
        track: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match track {
            true => self.input_dialog(
                "Track with LFS".into(),
                "Files matching this pattern are stored with LFS, e.g. *.psd. .gitattributes is updated and staged.".into(),
                String::new(),
                |root, pattern| lfs::track(root, &pattern).map(|()| format!("Tracking {pattern} with LFS")),
                window,
                cx,
            ),
            false => self.input_dialog(
                "Stop tracking with LFS".into(),
                "New files matching this pattern are stored by git itself again.".into(),
                String::new(),
                |root, pattern| lfs::untrack(root, &pattern).map(|()| format!("No longer tracking {pattern}")),
                window,
                cx,
            ),
        }
    }

    pub(crate) fn lfs_files_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run_op(window, cx, lfs::files, |_, files, window, cx| {
            let muted = cx.theme().muted_foreground;
            let total = files.len();
            window.open_alert_dialog(cx, move |dialog, _, _| {
                let rows = files.iter().take(MAX_LISTED).map(|f| {
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .child(div().flex_1().min_w_0().truncate().child(f.path.clone()))
                        .child(div().text_xs().text_color(muted).child(if f.downloaded {
                            "downloaded"
                        } else {
                            "pointer only"
                        }))
                });
                dialog.title(format!("LFS files ({total})")).description(
                    v_flex()
                        .id("lfs-files")
                        .max_h(px(360.))
                        .gap_1()
                        .children(rows)
                        .overflow_y_scrollbar(),
                )
            });
        });
    }
}

fn notify(_: &mut RepoView, message: String, window: &mut Window, cx: &mut Context<RepoView>) {
    window.push_notification(Notification::success(message), cx);
}
