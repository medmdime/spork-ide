//! The Notifications tool window (the right stripe's bell), IntelliJ's (`NotificationsToolWindow`,
//! `NotificationsPanel`): the project's notifications newest first, each with its icon, title,
//! text, actions and time, under "Timeline" and its "Clear all". It lists what the workspace
//! showed (Zed's notifications, `Workspace::notification_log`) and what Spork's views pushed
//! (gpui-kit's, which `NotificationHost` makes the workspace's balloons); its stripe button's dot
//! while one is unread.

use chrono::{DateTime, Local};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, PromptLevel, Render, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div, px,
};
use gpui_kit::component::{h_flex, v_flex};
use spork_workbench::Tokens;
use spork_workbench::balloon::{self, BalloonSpec, BalloonType};
use workspace::Workspace;
use workspace::dock::{DockPosition, GearItem, Panel, PanelEvent};
use workspace::notifications::simple_message_notification::MessageNotification;
use workspace::notifications::{LoggedNotification, NotificationId};

use crate::problems::accessible_text;

pub mod notifications_panel {
    gpui::actions!(
        notifications_panel,
        [
            /// Opens the Notifications tool window and focuses it, or gives the focus back to the
            /// editor.
            ToggleFocus,
            /// Clears the Notifications tool window.
            ClearAllNotifications,
            /// Marks every notification read: no highlight, and their actions done with.
            MarkNotificationsAsRead
        ]
    );
}

/// Shows what Spork's views say in a window as its workspace's balloons, which the Notifications
/// tool window logs: an error stays as Spork's VCS errors do (IntelliJ's "VCS important messages",
/// a sticky balloon). A window without a workspace (a dialog's) shows them itself.
pub(crate) fn init(cx: &mut App) {
    balloon::host_notifications(
        |note, window, cx| {
            let spec = BalloonSpec::from_notification(&note)?;
            let Some(workspace) = Workspace::for_window(window, cx) else {
                return Some(spec);
            };
            // a notification may be pushed while the workspace updates
            cx.defer(move |cx| {
                workspace.update(cx, |workspace, cx| show_spec(workspace, spec, cx))
            });
            None
        },
        cx,
    );
}

/// A balloon of Spork's in the workspace, as a notification of its own.
fn show_spec(workspace: &mut Workspace, spec: BalloonSpec, cx: &mut Context<Workspace>) {
    struct SporkNotification;
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (icon, color) = match spec.kind {
        BalloonType::Error => (ui::IconName::XCircle, ui::Color::Error),
        BalloonType::Warning => (ui::IconName::Warning, ui::Color::Warning),
        BalloonType::Information => (ui::IconName::Info, ui::Color::Muted),
    };
    let sticky = spec.sticky || spec.kind == BalloonType::Error;
    workspace.show_notification(
        NotificationId::composite::<SporkNotification>(id),
        cx,
        |cx| {
            cx.new(|cx| {
                let mut notification = MessageNotification::new(spec.text, cx)
                    .content_icon(icon, color)
                    .sticky(sticky);
                if let Some(title) = spec.title {
                    notification = notification.with_title(title);
                }
                spec.actions
                    .into_iter()
                    .fold(notification, |notification, action| {
                        let run = action.run;
                        notification
                            .secondary_menu_entry(action.label, move |window, cx| run(window, cx))
                    })
            })
        },
    );
}

pub(crate) fn add_panel(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    workspace.register_action(
        |workspace, _: &notifications_panel::ToggleFocus, window, cx| {
            workspace.toggle_panel_focus::<NotificationsPanel>(window, cx);
        },
    );
    workspace.register_action(
        |workspace, _: &notifications_panel::ClearAllNotifications, _, cx| {
            // IntelliJ expires them all: their balloons go too
            workspace.clear_all_notifications(cx);
            workspace.clear_notification_log(cx)
        },
    );
    workspace.register_action(
        |workspace, _: &notifications_panel::MarkNotificationsAsRead, _, cx| {
            // expired, as IntelliJ's: their balloons go
            workspace.clear_all_notifications(cx);
            if let Some(panel) = workspace.panel::<NotificationsPanel>(cx) {
                let count = workspace.notification_log().len();
                panel.update(cx, |panel, cx| {
                    panel.read = count;
                    panel.expired = count;
                    cx.notify();
                });
            }
        },
    );
    let handle = cx.entity();
    let panel = cx.new(|cx| NotificationsPanel::new(&handle, cx));
    workspace.add_panel(panel, window, cx);
}

pub(crate) struct NotificationsPanel {
    workspace: Entity<Workspace>,
    /// How many notifications the log had when last drawn.
    seen: usize,
    /// The notifications before this one in the log are read: IntelliJ highlights the others
    /// until the tool window hides.
    read: usize,
    /// The notifications before this one are expired (Mark All Notifications as Read): their
    /// actions are done with.
    expired: usize,
    /// Whether the tool window shows.
    active: bool,
    focus: FocusHandle,
    _subscription: Subscription,
}

impl NotificationsPanel {
    fn new(workspace: &Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let _subscription = cx.observe(workspace, |this, workspace, cx| {
            let count = workspace.read(cx).notification_log().len();
            if count != this.seen {
                // the log was cleared: what comes next is new
                if count < this.seen {
                    this.read = this.read.min(count);
                    this.expired = this.expired.min(count);
                }
                this.seen = count;
                cx.notify();
            }
        });
        Self {
            workspace: workspace.clone(),
            seen: 0,
            read: 0,
            expired: 0,
            active: false,
            focus: cx.focus_handle(),
            _subscription,
        }
    }

    fn entry(
        &self,
        ix: usize,
        logged: &LoggedNotification,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let icon = match logged.level {
            PromptLevel::Critical => "status/error",
            PromptLevel::Warning => "status/warning",
            PromptLevel::Info => "status/info",
        };
        let expired = ix < self.expired;
        let unread = ix >= self.read;
        let actions = logged
            .actions
            .iter()
            .enumerate()
            .map(|(action_ix, (label, run))| {
                let run = run.clone();
                let link = div()
                    .id(("notification-action", ix * 16 + action_ix))
                    .role(Role::Link)
                    .aria_label(label.clone())
                    .child(label.clone());
                let link = match expired {
                    true => link.text_color(tokens.text_disabled),
                    false => link
                        .text_color(tokens.text_link)
                        .cursor_pointer()
                        .on_click(move |_, window, cx| run(window, cx)),
                };
                spork_workbench::controls::A11yDisabled::new(link, expired)
            });
        let at: DateTime<Local> = logged.at.into();
        h_flex()
            .id(("notification", ix))
            .role(Role::Group)
            .aria_label(logged.text.clone())
            .items_start()
            .py(px(10.))
            .pl(px(10.))
            .gap(px(7.))
            .rounded(px(8.))
            .when(unread, |entry| entry.bg(tokens.notification_unread))
            .child(spork_workbench::image(icon, cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .pr(px(10.))
                    .gap(px(8.))
                    .when_some(logged.title.clone(), |entry, title| {
                        entry.child(div().font_weight(gpui::FontWeight::BOLD).child(title))
                    })
                    .child(div().child(logged.text.clone()))
                    .when(!logged.actions.is_empty(), |entry| {
                        entry.child(h_flex().gap(px(16.)).children(actions))
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .pt(px(3.))
                    .pr(px(10.))
                    .text_size(px(12.))
                    .text_color(tokens.text_secondary)
                    .child(shown_time(at)),
            )
    }
}

/// For the end-to-end tests: how many notifications the tool window lists, how many of them are
/// unread (highlighted) and expired (their actions done with); the balloons shown (`balloons`,
/// oldest first, each its title or text), the stub's count (`more`) and the stripe's dot
/// (`badge`: "error", "info" or null).
pub fn notifications_e2e_state(workspace: &Workspace, cx: &App) -> serde_json::Value {
    let count = workspace.notification_log().len();
    let (read, expired) = workspace
        .panel::<NotificationsPanel>(cx)
        .map(|panel| (panel.read(cx).read, panel.read(cx).expired))
        .unwrap_or_default();
    let unread = workspace.notification_log().get(read..).unwrap_or_default();
    let badge = match unread
        .iter()
        .any(|logged| logged.level == PromptLevel::Critical)
    {
        _ if unread.is_empty() => None,
        true => Some("error"),
        false => Some("info"),
    };
    let (shown, more) = workspace.shown_notifications(cx);
    let balloons: Vec<SharedString> = shown
        .into_iter()
        .map(|logged| logged.title.unwrap_or(logged.text))
        .collect();
    serde_json::json!({
        "count": count,
        "unread": count.saturating_sub(read),
        "expired": expired.min(count),
        "balloons": balloons,
        "more": more,
        "badge": badge,
    })
}

/// For the end-to-end tests: a notification of `kind` ("info", "warning", "error") as the
/// workspace's balloon, its `actions` links (each logging "Ran <action>" when chosen), `sticky`
/// or hiding after 10 s.
pub fn e2e_notify(
    workspace: &mut Workspace,
    kind: &str,
    title: Option<String>,
    text: String,
    actions: Vec<String>,
    sticky: bool,
    cx: &mut Context<Workspace>,
) {
    struct E2eNotification;
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (icon, color) = match kind {
        "error" => (ui::IconName::XCircle, ui::Color::Error),
        "warning" => (ui::IconName::Warning, ui::Color::Warning),
        _ => (ui::IconName::Info, ui::Color::Muted),
    };
    let workspace_handle = cx.entity().downgrade();
    workspace.show_notification(NotificationId::composite::<E2eNotification>(id), cx, |cx| {
        cx.new(|cx| {
            let notification = MessageNotification::new(text, cx)
                .content_icon(icon, color)
                .sticky(sticky);
            let notification = match title {
                Some(title) => notification.with_title(title),
                None => notification,
            };
            actions
                .into_iter()
                .fold(notification, |notification, label| {
                    let workspace = workspace_handle.clone();
                    let ran = format!("Ran {label}");
                    notification.secondary_menu_entry(label, move |_, cx| {
                        let ran = ran.clone();
                        workspace
                            .update(cx, |workspace, cx| {
                                workspace.log_notification(
                                    LoggedNotification::new(PromptLevel::Info, ran),
                                    cx,
                                )
                            })
                            .ok();
                    })
                })
        })
    });
}

/// The time an entry shows: `SPORK_E2E_TIME` when the end-to-end tests set it, so their
/// screenshots are the same at any hour of the day.
fn shown_time(at: DateTime<Local>) -> String {
    static FIXED: std::sync::LazyLock<Option<String>> =
        std::sync::LazyLock::new(|| std::env::var("SPORK_E2E_TIME").ok());
    FIXED
        .clone()
        .unwrap_or_else(|| pretty_time(at, Local::now()))
}

/// IntelliJ's `formatPrettyDateTime`: the time today, "Yesterday", the date before.
fn pretty_time(at: DateTime<Local>, now: DateTime<Local>) -> String {
    let (day, today) = (at.date_naive(), now.date_naive());
    if day == today {
        at.format("%H:%M").to_string()
    } else if today.pred_opt() == Some(day) {
        "Yesterday".to_owned()
    } else {
        at.format("%x").to_string()
    }
}

impl Render for NotificationsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let log: Vec<LoggedNotification> = self.workspace.read(cx).notification_log().to_vec();
        let body =
            match log.is_empty() {
                true => v_flex()
                    .id("notifications-empty")
                    .size_full()
                    .items_center()
                    .justify_center()
                    .text_color(tokens.text_secondary)
                    .child(accessible_text(
                        "notifications-empty-1",
                        "Suggestions, events,".into(),
                    ))
                    .child(accessible_text(
                        "notifications-empty-2",
                        "and errors will appear here".into(),
                    )),
                false => {
                    let entries: Vec<_> = log
                        .iter()
                        .enumerate()
                        .rev()
                        .map(|(ix, logged)| self.entry(ix, logged, cx).into_any_element())
                        .collect();
                    let workspace = self.workspace.clone();
                    v_flex()
                        .id("notifications-timeline")
                        .size_full()
                        .pt(px(8.))
                        .gap(px(8.))
                        .child(
                            h_flex()
                                .pl(px(18.))
                                .pr(px(20.))
                                .justify_between()
                                .child(div().text_color(tokens.text_secondary).child(
                                    accessible_text("notifications-timeline", "Timeline".into()),
                                ))
                                .child(
                                    div()
                                        .id("notifications-clear-all")
                                        .role(Role::Link)
                                        .aria_label("Clear all")
                                        .text_color(tokens.text_link)
                                        .cursor_pointer()
                                        .child("Clear all")
                                        .on_click(move |_, _, cx| {
                                            workspace.update(cx, |workspace, cx| {
                                                workspace.clear_notification_log(cx)
                                            })
                                        }),
                                ),
                        )
                        .child(
                            v_flex()
                                .id("notifications-list")
                                .role(Role::List)
                                .aria_label("Notifications")
                                .flex_1()
                                .min_h_0()
                                .pl(px(8.))
                                .pr(px(10.))
                                .gap(px(10.))
                                .overflow_y_scroll()
                                .children(entries),
                        )
                }
            };
        spork_vcs::spork_look(cx).size_full().child(
            div()
                .size_full()
                .bg(tokens.island)
                .track_focus(&self.focus)
                .key_context("NotificationsPanel")
                .child(body),
        )
    }
}

impl Focusable for NotificationsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EventEmitter<PanelEvent> for NotificationsPanel {}

impl Panel for NotificationsPanel {
    fn persistent_name() -> &'static str {
        "Notifications"
    }

    fn panel_key() -> &'static str {
        "NotificationsToolWindow"
    }

    fn position(&self, _: &Window, _: &App) -> DockPosition {
        DockPosition::Right
    }

    // shortcut: the tool window stays where IntelliJ puts it, as Find's does
    fn position_is_valid(&self, position: DockPosition) -> bool {
        position == DockPosition::Right
    }

    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}

    fn default_size(&self, _: &Window, _: &App) -> Pixels {
        px(360.)
    }

    fn icon(&self, _: &Window, _: &App) -> Option<ui::IconName> {
        Some(ui::IconName::Bell)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some("Notifications")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(notifications_panel::ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        11
    }

    // the notifications shown are read once the tool window hides, as IntelliJ's; the workspace
    // is updating while it hides the tool window, so its count is the one seen last. Showing it
    // closes the balloons, once the workspace is done.
    fn set_active(&mut self, active: bool, _: &mut Window, cx: &mut Context<Self>) {
        if self.active && !active {
            self.read = self.seen;
            cx.notify();
        }
        if active && !self.active {
            let workspace = self.workspace.downgrade();
            cx.defer(move |cx| {
                workspace
                    .update(cx, |workspace, cx| workspace.clear_all_notifications(cx))
                    .ok();
            });
        }
        self.active = active;
    }

    // IntelliJ's dot while a notification is unread: red for an error
    fn stripe_badge(&self, _: &Window, cx: &App) -> Option<gpui::Hsla> {
        let workspace = self.workspace.read(cx);
        let unread = workspace.notification_log().get(self.read..)?;
        let tokens = Tokens::get(cx);
        match unread
            .iter()
            .any(|logged| logged.level == PromptLevel::Critical)
        {
            _ if unread.is_empty() => None,
            true => Some(tokens.icon_badge_error),
            false => Some(tokens.icon_badge_info),
        }
    }

    // IntelliJ's ⋮ (`NotificationsPanel`'s gear actions, Find left out)
    fn gear_items(&self, _: &Window, _: &App) -> Vec<GearItem> {
        vec![
            GearItem {
                label: "Clear All Notifications".into(),
                checked: None,
                action: Box::new(notifications_panel::ClearAllNotifications),
            },
            GearItem {
                label: "Mark All Notifications as Read".into(),
                checked: None,
                action: Box::new(notifications_panel::MarkNotificationsAsRead),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::pretty_time;
    use chrono::{Local, TimeZone as _};

    #[test]
    fn writes_the_time_today_yesterday_then_the_date() {
        let now = Local.with_ymd_and_hms(2026, 11, 22, 18, 30, 0).unwrap();
        let at = |day, hour| Local.with_ymd_and_hms(2026, 11, day, hour, 5, 0).unwrap();
        assert_eq!(pretty_time(at(22, 9), now), "09:05");
        assert_eq!(pretty_time(at(21, 23), now), "Yesterday");
        assert_eq!(pretty_time(at(20, 9), now), "11/20/26");
    }
}
