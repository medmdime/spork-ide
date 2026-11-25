//! Notifications as in IntelliJ: each one shows as a balloon (`spork_workbench::balloon`) and
//! stays in the Notifications tool window on the right (docs/reference-spec.md §2). `Notification`
//! stands in for gpui-kit's, so every `window.push_notification` call records what it said; the
//! standalone app shows what is pushed as balloons, by the Notifications settings page's options
//! (`install`).

use std::sync::Mutex;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification as Toast;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Root, Sizable as _, h_flex, v_flex};
use gpui_kit::*;
use spork_workbench::Tokens;
use spork_workbench::balloon::{self, BalloonAction, BalloonCorner, BalloonSpec, BalloonType};

use crate::RepoView;
use crate::settings_window::Store;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Info,
    Success,
    Warning,
    Error,
}

impl Kind {
    /// IntelliJ's group for a VCS notification of this kind (`VcsNotifier`): errors are "VCS
    /// important messages", a sticky balloon by default, the others "VCS notifications".
    fn group(self) -> (&'static str, &'static str) {
        match self {
            Kind::Error => ("vcs-important-notifications", "Sticky balloon"),
            _ => ("vcs-notifications", "Balloon"),
        }
    }
}

struct Entry {
    kind: Kind,
    text: SharedString,
    at: chrono::DateTime<chrono::Local>,
}

/// Every notification of this run the Notifications tool window keeps, oldest first.
static HISTORY: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

/// How many of `HISTORY` are read: those there when the tool window last hid. While it shows,
/// how many it showed.
static READ: Mutex<(usize, Option<usize>)> = Mutex::new((0, None));

/// A notification: build it with `success`, `info`, `warning` or `error` and push it as usual.
pub(crate) struct Notification {
    kind: Kind,
    text: SharedString,
}

impl Notification {
    pub(crate) fn info(text: impl Into<SharedString>) -> Self {
        Self {
            kind: Kind::Info,
            text: text.into(),
        }
    }
    pub(crate) fn success(text: impl Into<SharedString>) -> Self {
        Self {
            kind: Kind::Success,
            text: text.into(),
        }
    }
    pub(crate) fn warning(text: impl Into<SharedString>) -> Self {
        Self {
            kind: Kind::Warning,
            text: text.into(),
        }
    }
    pub(crate) fn error(text: impl Into<SharedString>) -> Self {
        Self {
            kind: Kind::Error,
            text: text.into(),
        }
    }
}

/// A setting of the Notifications page, saved as the Settings window saves it.
fn setting(key: &str) -> Option<String> {
    Store::load().get(key).map(str::to_owned)
}

/// Pushing it keeps it in the history, unless its group's "Show in tool window" is off; the
/// app shows it (`install`).
impl From<Notification> for Toast {
    fn from(n: Notification) -> Self {
        let (group, _) = n.kind.group();
        let logged = setting(&format!("notifications.{group}.log")).is_none_or(|log| log == "true");
        if logged && let Ok(mut history) = HISTORY.lock() {
            history.push(Entry {
                kind: n.kind,
                text: n.text.clone(),
                at: chrono::Local::now(),
            });
        }
        match n.kind {
            Kind::Info => Toast::info(n.text),
            Kind::Success => Toast::success(n.text),
            Kind::Warning => Toast::warning(n.text),
            Kind::Error => Toast::error(n.text),
        }
    }
}

/// Shows what is pushed to a window as a balloon there, as the Notifications page says: none
/// with "Display balloon notifications" off or its group's popup "No popup", sticky for "Sticky
/// balloon", in the corner "Notification location:" names. The balloon's ⋮ opens the page.
pub(crate) fn install(cx: &mut App) {
    balloon::host_notifications(
        |note, window, cx| {
            let mut spec = BalloonSpec::from_notification(&note)?;
            if setting("notifications.balloons").is_some_and(|on| on != "true") {
                return None;
            }
            let (group, default) = match spec.kind {
                BalloonType::Error => Kind::Error.group(),
                _ => Kind::Info.group(),
            };
            let popup = setting(&format!("notifications.{group}.popup"));
            // shortcut: a "Tool window balloon" shows as a balloon; IntelliJ points it at the
            // tool window's stripe button, which needs a balloon with a callout
            match popup.as_deref().unwrap_or(default) {
                "No popup" => return None,
                "Sticky balloon" => spec.sticky = true,
                _ => {}
            }
            spec.menu = vec![Some(BalloonAction::new("Settings…", |window, cx| {
                if let Some(view) = repo_view(window, cx) {
                    view.update(cx, |view, cx| {
                        view.settings_page(
                            Some("Appearance & Behavior/Notifications".into()),
                            window,
                            cx,
                        )
                    });
                }
            }))];
            layout(window, cx);
            Some(spec)
        },
        cx,
    );
}

/// `window`'s balloon layout, in the corner "Notification location:" names, its stub opening
/// the Notifications tool window.
fn layout(window: &Window, cx: &mut App) -> Entity<balloon::BalloonLayout> {
    let corner = setting("notifications.location")
        .map_or_else(BalloonCorner::default, |name| BalloonCorner::named(&name));
    let layout = balloon::layout_of(window, cx);
    layout.update(cx, |layout, cx| {
        layout.set_corner(corner, cx);
        layout.on_show_all(|window, cx| {
            if let Some(view) = repo_view(window, cx) {
                view.update(cx, |view, cx| {
                    if !view.show_notifications {
                        view.toggle_notifications(cx);
                    }
                });
            }
        });
    });
    layout
}

/// The project view of a project window, the one its tabs show.
fn repo_view(window: &mut Window, cx: &mut App) -> Option<Entity<RepoView>> {
    let root = window.root::<Root>()??;
    let workspace = root
        .read(cx)
        .view()
        .clone()
        .downcast::<crate::workspace::Workspace>()
        .ok()?;
    Some(workspace.read(cx).active_tab())
}

/// What the latest notification said, for the end-to-end tests (e2e.rs).
pub(crate) fn last_text() -> Option<SharedString> {
    HISTORY.lock().ok()?.last().map(|e| e.text.clone())
}

/// IntelliJ's dot on the Notifications stripe button while a notification is unread: red when
/// one is an error.
pub(crate) fn unread_badge(tokens: &Tokens) -> Option<Hsla> {
    let history = HISTORY.lock().ok()?;
    let (read, _) = *READ.lock().ok()?;
    let unread = history.get(read..).filter(|unread| !unread.is_empty())?;
    match unread.iter().any(|e| e.kind == Kind::Error) {
        true => Some(tokens.icon_badge_error),
        false => Some(tokens.icon_badge_info),
    }
}

impl RepoView {
    pub(crate) fn toggle_notifications(&mut self, cx: &mut Context<Self>) {
        self.show_notifications = !self.show_notifications;
        cx.notify();
    }

    /// The Notifications tool window: newest first, IntelliJ's empty text when there are none.
    /// Showing it closes the window's balloons; once it hides, what it showed is read.
    pub(crate) fn notifications_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let count = HISTORY.lock().map_or(0, |history| history.len());
        if let Ok(mut read) = READ.lock() {
            match (self.show_notifications, read.1) {
                (false, Some(shown)) => *read = (shown, None),
                (true, None) => {
                    *read = (read.0, Some(count));
                    if let Some(layout) = balloon::layout(window, cx) {
                        layout.update(cx, |layout, cx| layout.clear(cx));
                    }
                }
                (true, Some(_)) => read.1 = Some(count),
                (false, None) => {}
            }
        }
        if !self.show_notifications {
            return None;
        }
        let theme = cx.theme();
        let (secondary, border) = (theme.muted_foreground, theme.title_bar);
        let rows: Vec<Div> = HISTORY.lock().map_or_else(
            |_| Vec::new(),
            |history| {
                history
                    .iter()
                    .rev()
                    .map(|e| {
                        let icon = match e.kind {
                            Kind::Info => "status/info",
                            Kind::Success => "status/success",
                            Kind::Warning => "status/warning",
                            Kind::Error => "status/error",
                        };
                        h_flex()
                            .items_start()
                            .px_3()
                            .py_2()
                            .gap_2()
                            .border_b_1()
                            .border_color(border)
                            .child(spork_workbench::image(icon, cx))
                            .child(div().flex_1().min_w_0().child(e.text.clone()))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(secondary)
                                    .child(e.at.format("%H:%M").to_string()),
                            )
                    })
                    .collect()
            },
        );
        let empty = rows.is_empty();
        let clear = Button::new("notifications-clear")
            .ghost()
            .small()
            .icon(spork_workbench::icon("general/delete"))
            .tooltip("Clear All")
            .on_click(cx.listener(|_, _, window, cx| {
                if let Ok(mut history) = HISTORY.lock() {
                    history.clear();
                }
                if let Ok(mut read) = READ.lock() {
                    *read = (0, read.1.map(|_| 0));
                }
                // IntelliJ expires them all: their balloons go too
                if let Some(layout) = balloon::layout(window, cx) {
                    layout.update(cx, |layout, cx| layout.clear(cx));
                }
                cx.notify();
            }));
        let hide = Button::new("notifications-hide")
            .ghost()
            .small()
            .icon(spork_workbench::icon("general/hide"))
            .tooltip("Hide")
            .on_click(cx.listener(|this, _, _, cx| this.toggle_notifications(cx)));
        let body = match empty {
            true => div()
                .id("notifications-empty")
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .text_color(secondary)
                .child("Suggestions, events,")
                .child("and errors will appear here")
                .into_any_element(),
            false => div()
                .id("notifications-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .child(v_flex().children(rows))
                .into_any_element(),
        };
        Some(
            crate::island(cx)
                .capture_any_mouse_down(cx.listener(|this, _, _, cx| {
                    this.set_active_tool(Some(crate::ActiveTool::Notifications), cx)
                }))
                .w(px(360.))
                .flex_none()
                .ml(px(6.))
                .child(
                    spork_workbench::shell::tool_window_header("Notifications").child(
                        spork_workbench::shell::tool_window_actions()
                            .child(clear)
                            .child(hide),
                    ),
                )
                .child(body),
        )
    }
}

/// For the end-to-end tests: the balloons the project window shows (`spork_workbench`'s
/// layout), the unread dot and how many notifications the tool window keeps (`logged`).
pub(crate) fn e2e_state(window: &Window, cx: &App) -> serde_json::Value {
    let mut state = balloon::layout(window, cx).map_or_else(
        || serde_json::json!({"balloons": [], "more": 0}),
        |layout| layout.read(cx).e2e_state(),
    );
    let badge =
        unread_badge(Tokens::get(cx)).map(|dot| match dot == Tokens::get(cx).icon_badge_error {
            true => "error",
            false => "info",
        });
    state["badge"] = serde_json::json!(badge);
    state["logged"] = serde_json::json!(HISTORY.lock().map_or(0, |history| history.len()));
    state
}

/// For the end-to-end tests: shows a balloon in `window` as a notification pushed there would,
/// with `actions` (each logging "Ran <action>" in the tool window) and as `sticky` as asked.
pub(crate) fn e2e_notify(
    kind: &str,
    title: Option<String>,
    text: String,
    actions: Vec<String>,
    sticky: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let kind = match kind {
        "error" => BalloonType::Error,
        "warning" => BalloonType::Warning,
        _ => BalloonType::Information,
    };
    let actions = actions
        .into_iter()
        .map(|label| {
            let ran = format!("Ran {label}");
            BalloonAction::new(label, move |_, _| {
                if let Ok(mut history) = HISTORY.lock() {
                    history.push(Entry {
                        kind: Kind::Info,
                        text: ran.clone().into(),
                        at: chrono::Local::now(),
                    });
                }
            })
        })
        .collect();
    let spec = BalloonSpec {
        kind,
        title: title.map(Into::into),
        text: text.into(),
        actions,
        menu: Vec::new(),
        sticky,
    };
    layout(window, cx).update(cx, |layout, cx| layout.push(spec, cx));
}

#[cfg(test)]
mod tests {
    use super::{HISTORY, Kind, Notification, Toast};

    #[test]
    fn pushing_keeps_the_text() {
        let _toast: Toast = Notification::error("push rejected").into();
        let history = HISTORY.lock().unwrap();
        assert!(
            history
                .iter()
                .any(|e| e.text == "push rejected" && e.kind == Kind::Error)
        );
    }
}
