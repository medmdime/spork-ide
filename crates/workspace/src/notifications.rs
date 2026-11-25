use crate::{MultiWorkspace, Toast, Workspace, workspace_error::WorkspaceError};
use anyhow::Context as _;
use gpui::{
    AnyView, App, AppContext as _, AsyncApp, AsyncWindowContext, Context, DismissEvent, Entity,
    EventEmitter, FocusHandle, Focusable, PromptLevel, Render, Task, TextStyleRefinement,
    UnderlineStyle, WeakEntity,
};
use markdown::MarkdownStyle;
use parking_lot::Mutex;
use project::{LanguageServerPromptRequest, project_settings::ProjectSettings};
use settings::Settings;
use spork_workbench::balloon::{self, Balloon, BalloonAction, BalloonType, Overflow};
use theme_settings::ThemeSettings;

use std::ops::Deref;
use std::rc::Rc;
use std::sync::{Arc, LazyLock};
use std::{any::TypeId, time::Duration};
use ui::prelude::*;
use util::ResultExt;

#[derive(Default)]
pub struct Notifications {
    notifications: Vec<(NotificationId, AnyView)>,
    /// Spork IDE: every notification shown, oldest first, for the Notifications tool window.
    log: Vec<LoggedNotification>,
    /// Spork IDE: the balloons pushed out of the stack, IntelliJ's "N more" stub.
    overflow: Overflow,
}

/// Spork IDE: a notification as IntelliJ's Notifications tool window lists it: what it said,
/// when, and its actions, which still work once its balloon is gone.
#[derive(Clone)]
pub struct LoggedNotification {
    pub level: PromptLevel,
    pub title: Option<SharedString>,
    pub text: SharedString,
    pub actions: Vec<(SharedString, Rc<dyn Fn(&mut Window, &mut App)>)>,
    pub at: std::time::SystemTime,
}

impl LoggedNotification {
    pub fn new(level: PromptLevel, text: impl Into<SharedString>) -> Self {
        Self {
            level,
            title: None,
            text: text.into(),
            actions: Vec::new(),
            at: std::time::SystemTime::now(),
        }
    }

    /// What a notification view says, for the ones that say it in text.
    fn of(view: &AnyView, cx: &App) -> Option<Self> {
        if let Ok(message) = view
            .clone()
            .downcast::<simple_message_notification::MessageNotification>()
        {
            return message.read(cx).logged(&message);
        }
        if let Ok(toast) = view.clone().downcast::<StatusToast>() {
            return Some(toast.read(cx).logged());
        }
        let prompt = view.clone().downcast::<LanguageServerPrompt>().ok()?;
        let request = prompt.read(cx).request.as_ref()?;
        Some(Self {
            title: Some(request.lsp_name.clone().into()),
            ..Self::new(request.level, request.message.clone())
        })
    }
}

impl Deref for Notifications {
    type Target = Vec<(NotificationId, AnyView)>;

    fn deref(&self) -> &Self::Target {
        &self.notifications
    }
}

impl std::ops::DerefMut for Notifications {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.notifications
    }
}

#[derive(Debug, Eq, PartialEq, Clone, Hash)]
pub enum NotificationId {
    Unique(TypeId),
    Composite(TypeId, ElementId),
    Named(SharedString),
}

impl NotificationId {
    /// Returns a unique [`NotificationId`] for the given type.
    pub const fn unique<T: 'static>() -> Self {
        Self::Unique(TypeId::of::<T>())
    }

    /// Returns a [`NotificationId`] for the given type that is also identified
    /// by the provided ID.
    pub fn composite<T: 'static>(id: impl Into<ElementId>) -> Self {
        Self::Composite(TypeId::of::<T>(), id.into())
    }

    /// Builds a `NotificationId` out of the given string.
    pub fn named(id: SharedString) -> Self {
        Self::Named(id)
    }
}

pub trait Notification:
    EventEmitter<DismissEvent> + EventEmitter<SuppressEvent> + Focusable + Render
{
}

pub struct SuppressEvent;

impl Workspace {
    pub fn has_notification(&self, id: &NotificationId) -> bool {
        self.notifications
            .iter()
            .any(|(existing_id, _)| existing_id == id)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn notification_ids(&self) -> Vec<NotificationId> {
        self.notifications
            .iter()
            .map(|(id, _)| id)
            .cloned()
            .collect()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn notification_views(&self) -> Vec<AnyView> {
        self.notifications
            .iter()
            .map(|(_, view)| view.clone())
            .collect()
    }

    pub fn show_notification<V: Notification>(
        &mut self,
        id: NotificationId,
        cx: &mut Context<Self>,
        build_notification: impl FnOnce(&mut Context<Self>) -> Entity<V>,
    ) {
        self.show_notification_without_handling_dismiss_events(&id, cx, |cx| {
            let notification = build_notification(cx);
            cx.subscribe(&notification, {
                let id = id.clone();
                move |this, _, _: &DismissEvent, cx| {
                    this.dismiss_notification(&id, cx);
                }
            })
            .detach();
            cx.subscribe(&notification, {
                let id = id.clone();
                move |workspace: &mut Workspace, _, _: &SuppressEvent, cx| {
                    workspace.suppress_notification(&id, cx);
                }
            })
            .detach();

            notification.into()
        });
    }

    /// Shows a notification in this workspace's window. Caller must handle dismiss.
    ///
    /// This exists so that the `build_notification` closures stored for app notifications can
    /// return `AnyView`. Subscribing to events from an `AnyView` is not supported, so instead that
    /// responsibility is pushed to the caller where the `V` type is known.
    pub(crate) fn show_notification_without_handling_dismiss_events(
        &mut self,
        id: &NotificationId,
        cx: &mut Context<Self>,
        build_notification: impl FnOnce(&mut Context<Self>) -> AnyView,
    ) {
        if self.suppressed_notifications.contains(id) {
            return;
        }
        self.dismiss_notification(id, cx);
        let view = build_notification(cx);
        // Spork IDE: the Notifications tool window keeps it; the oldest of three balloons goes
        // onto the stack's stub
        if let Some(logged) = LoggedNotification::of(&view, cx) {
            self.notifications.log.push(logged);
        }
        if self.notifications.overflow.push(self.notifications.len()) {
            self.notifications.notifications.remove(0);
        }
        self.notifications.push((id.clone(), view));
        cx.notify();
    }

    /// Spork IDE: the notifications shown in this workspace, oldest first.
    pub fn notification_log(&self) -> &[LoggedNotification] {
        &self.notifications.log
    }

    /// Spork IDE: adds a notification shown elsewhere (Spork's views') to the log.
    pub fn log_notification(&mut self, logged: LoggedNotification, cx: &mut Context<Self>) {
        self.notifications.log.push(logged);
        cx.notify();
    }

    /// Spork IDE: the Notifications tool window's Clear all.
    pub fn clear_notification_log(&mut self, cx: &mut Context<Self>) {
        self.notifications.log.clear();
        cx.notify();
    }

    pub fn show_error<E: WorkspaceError + 'static>(&mut self, err: E, cx: &mut Context<Self>) {
        self.show_notification(NotificationId::unique::<E>(), cx, |cx| {
            cx.new(|cx| {
                simple_message_notification::MessageNotification::from_workspace_error(err, cx)
            })
        });
    }

    pub fn dismiss_notification(&mut self, id: &NotificationId, cx: &mut Context<Self>) {
        // Spork IDE: the stack's stub goes with the oldest balloon
        if let Some(index) = self
            .notifications
            .iter()
            .position(|(existing_id, _)| existing_id == id)
        {
            self.notifications.remove(index);
            self.notifications.overflow.closed(index);
            cx.notify();
        }
    }

    // Spork IDE: a toast is a balloon, which hides after 10 s (IntelliJ's) when it autohides
    pub fn show_toast(&mut self, toast: Toast, cx: &mut Context<Self>) {
        self.dismiss_notification(&toast.id, cx);
        self.show_notification(toast.id.clone(), cx, |cx| {
            cx.new(|cx| {
                simple_message_notification::MessageNotification::new(toast.message, cx)
                    .sticky(!toast.autohide)
                    .when_some(toast.on_click, |this, (click_msg, on_click)| {
                        this.primary_message(click_msg)
                            .primary_on_click(move |window, cx| on_click(window, cx))
                    })
            })
        });
    }

    /// Spork IDE: shows a status toast as a balloon, or closes it if it shows.
    pub fn toggle_status_toast(&mut self, toast: Entity<StatusToast>, cx: &mut Context<Self>) {
        let id = NotificationId::composite::<StatusToast>(ElementId::View(toast.entity_id()));
        if self.has_notification(&id) {
            return self.dismiss_notification(&id, cx);
        }
        self.show_notification(id, cx, |_| toast);
    }

    /// Spork IDE: the balloons shown, each as the Notifications tool window lists it, oldest
    /// first, and how many the stack's stub counts.
    pub fn shown_notifications(&self, cx: &App) -> (Vec<LoggedNotification>, usize) {
        let shown = self
            .notifications
            .iter()
            .filter_map(|(_, view)| LoggedNotification::of(view, cx))
            .collect();
        (shown, self.notifications.overflow.count)
    }

    pub fn dismiss_toast(&mut self, id: &NotificationId, cx: &mut Context<Self>) {
        self.dismiss_notification(id, cx);
    }

    pub fn clear_all_notifications(&mut self, cx: &mut Context<Self>) {
        self.notifications.clear();
        self.notifications.overflow = Overflow::default();
        cx.notify();
    }

    /// Spork IDE: the workspace's balloons, between the stripes and above the status bar, in
    /// IntelliJ's corner; its stub, a click on which shows them all in the Notifications tool
    /// window.
    pub(crate) fn render_notifications(
        &self,
        stripes: (Pixels, Pixels),
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if self.notifications.is_empty() {
            return None;
        }
        let workspace = cx.entity().downgrade();
        let show_all = move |window: &mut Window, cx: &mut App| {
            workspace
                .update(cx, |workspace, cx| {
                    workspace.clear_all_notifications(cx);
                    workspace.show_tool_window_named("Notifications", window, cx);
                })
                .ok();
        };
        let balloons = self
            .notifications
            .iter()
            .map(|(_, notification)| notification.clone().into_any_element())
            .collect();
        Some(
            div()
                .absolute()
                .inset_0()
                .left(stripes.0)
                .right(stripes.1)
                .child(balloon::stack(
                    balloon::BalloonCorner::default(),
                    balloons,
                    self.notifications.overflow,
                    show_all,
                    crate::frame::tokens(cx),
                )),
        )
    }

    /// Hide all notifications matching the given ID
    pub fn suppress_notification(&mut self, id: &NotificationId, cx: &mut Context<Self>) {
        self.dismiss_notification(id, cx);
        self.suppressed_notifications.insert(id.clone());
    }

    pub fn is_notification_suppressed(&self, notification_id: NotificationId) -> bool {
        self.suppressed_notifications.contains(&notification_id)
    }

    pub fn show_initial_notifications(&mut self, cx: &mut Context<Self>) {
        // Allow absence of the global so that tests don't need to initialize it.
        let app_notifications = GLOBAL_APP_NOTIFICATIONS
            .lock()
            .app_notifications
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        for (id, build_notification) in app_notifications {
            self.show_notification_without_handling_dismiss_events(&id, cx, |cx| {
                build_notification(cx)
            });
        }
    }

    pub(crate) fn show_language_server_prompt(
        &mut self,
        request: &LanguageServerPromptRequest,
        cx: &mut Context<Self>,
    ) {
        if request.actions.is_empty()
            && self.notifications.iter().any(|(_, view)| {
                let Ok(prompt) = view.clone().downcast::<LanguageServerPrompt>() else {
                    return false;
                };
                prompt.read(cx).request.as_ref().is_some_and(|existing| {
                    existing.actions.is_empty()
                        && existing.level == request.level
                        && existing.lsp_name == request.lsp_name
                        && existing.message == request.message
                })
            })
        {
            return;
        }

        self.show_notification(
            NotificationId::composite::<LanguageServerPrompt>(request.id),
            cx,
            |cx| cx.new(|cx| LanguageServerPrompt::new(request.clone(), cx)),
        );
    }
}

pub struct LanguageServerPrompt {
    focus_handle: FocusHandle,
    request: Option<project::LanguageServerPromptRequest>,
    dismiss_task: Option<Task<()>>,
}

impl Focusable for LanguageServerPrompt {
    fn focus_handle(&self, _cx: &App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}

impl Notification for LanguageServerPrompt {}

impl LanguageServerPrompt {
    pub fn new(request: project::LanguageServerPromptRequest, cx: &mut Context<Self>) -> Self {
        let dismiss_task = if request.actions.is_empty() {
            ProjectSettings::get_global(cx)
                .global_lsp_settings
                .notifications
                .dismiss_timeout_ms
                .filter(|&duration| duration > 0)
                .map(|dismiss_duration_ms| {
                    cx.spawn(async move |prompt, cx| {
                        cx.background_executor()
                            .timer(Duration::from_millis(dismiss_duration_ms))
                            .await;
                        prompt
                            .update(cx, |prompt, cx| prompt.dismiss_notification(cx))
                            .ok();
                    })
                })
        } else {
            None
        };

        Self {
            focus_handle: cx.focus_handle(),
            request: Some(request),
            dismiss_task,
        }
    }

    async fn select_option(this: Entity<Self>, ix: usize, cx: &mut AsyncWindowContext) {
        util::maybe!(async move {
            let potential_future = this.update(cx, |this, _| {
                this.request.take().map(|request| request.respond(ix))
            });

            potential_future
                .context("Response already sent")?
                .await
                .context("Stream already closed")?;

            this.update(cx, |this, cx| {
                this.dismiss_notification(cx);
            });

            anyhow::Ok(())
        })
        .await
        .log_err();
    }

    fn dismiss_notification(&mut self, cx: &mut Context<Self>) {
        self.dismiss_task = None;
        cx.emit(DismissEvent);
    }
}

/// Spork IDE: a language server's message as IntelliJ's balloon: the server's name over its
/// text, its choices as action links; one asking for a choice stays (sticky).
impl Render for LanguageServerPrompt {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let Some(request) = &self.request else {
            return div().into_any_element();
        };
        let kind = match request.level {
            PromptLevel::Info => BalloonType::Information,
            PromptLevel::Warning => BalloonType::Warning,
            PromptLevel::Critical => BalloonType::Error,
        };
        let actions: Vec<BalloonAction> = request
            .actions
            .iter()
            .enumerate()
            .map(|(ix, action)| {
                let this = this.clone();
                BalloonAction::new(action.title.clone(), move |window, cx| {
                    let this = this.clone();
                    window
                        .spawn(cx, async move |cx| {
                            LanguageServerPrompt::select_option(this, ix, cx).await
                        })
                        .detach()
                })
            })
            .collect();
        let close = {
            let this = this.clone();
            move |_: &mut Window, cx: &mut App| {
                this.update(cx, |this, cx| this.dismiss_notification(cx))
            }
        };
        let suppress =
            move |_: &mut Window, cx: &mut App| this.update(cx, |_, cx| cx.emit(SuppressEvent));
        Balloon::new(("language-server-prompt", cx.entity_id()), kind)
            .tokens(crate::frame::tokens(cx))
            .title(request.lsp_name.clone())
            .text(request.message.clone())
            .actions(actions)
            .menu_item(Some(BalloonAction::new(
                "Don’t Show Again For This Project",
                suppress,
            )))
            .sticky(!request.actions.is_empty())
            .on_close(close)
            .on_close_all(close_all_balloons)
            .into_any_element()
    }
}

impl EventEmitter<DismissEvent> for LanguageServerPrompt {}
impl EventEmitter<SuppressEvent> for LanguageServerPrompt {}

/// Spork IDE: ⌥-clicking a balloon's close button closes every balloon of its window.
fn close_all_balloons(window: &mut Window, cx: &mut App) {
    if let Some(workspace) = Workspace::for_window(window, cx) {
        workspace.update(cx, |workspace, cx| workspace.clear_all_notifications(cx));
    }
}

/// Spork IDE: runs the action of the newest status toast that has one (`toast::RunAction`).
pub(crate) fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        workspace.register_action(|workspace, _: &zed_actions::toast::RunAction, window, cx| {
            let run = workspace.notifications.iter().rev().find_map(|(_, view)| {
                let toast = view.clone().downcast::<StatusToast>().ok()?;
                toast
                    .read(cx)
                    .actions
                    .first()
                    .map(|action| action.run.clone())
            });
            if let Some(run) = run {
                // the action may update the workspace
                cx.defer_in(window, move |_, window, cx| run(window, cx));
            }
        });
    })
    .detach();
}

/// A short message about what just happened, with an icon and an action or two (Zed's status
/// toast). Spork IDE: an IntelliJ balloon with that icon, its actions links that close it, hiding
/// after 10 s unless it doesn't `auto_dismiss`.
pub struct StatusToast {
    icon: Option<Icon>,
    text: SharedString,
    actions: Vec<BalloonAction>,
    auto_dismiss: bool,
    this_handle: Entity<Self>,
    focus_handle: FocusHandle,
}

impl StatusToast {
    pub fn new(
        text: impl Into<SharedString>,
        cx: &mut App,
        f: impl FnOnce(Self, &mut Context<Self>) -> Self,
    ) -> Entity<Self> {
        cx.new(|cx| {
            let focus_handle = cx.focus_handle();
            f(
                Self {
                    text: text.into(),
                    icon: None,
                    actions: Vec::new(),
                    auto_dismiss: true,
                    this_handle: cx.entity(),
                    focus_handle,
                },
                cx,
            )
        })
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn auto_dismiss(mut self, auto_dismiss: bool) -> Self {
        self.auto_dismiss = auto_dismiss;
        self
    }

    /// An action link; choosing it closes the toast.
    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        f: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        let this_handle = self.this_handle.clone();
        self.actions
            .push(BalloonAction::new(label, move |window, cx| {
                this_handle.update(cx, |_, cx| cx.emit(DismissEvent));
                f(window, cx);
            }));
        self
    }

    /// IntelliJ's balloon always has its close button, on hover.
    pub fn dismiss_button(self, _show: bool) -> Self {
        self
    }

    fn logged(&self) -> LoggedNotification {
        LoggedNotification {
            actions: self
                .actions
                .iter()
                .map(|action| (action.label.clone(), action.run.clone()))
                .collect(),
            ..LoggedNotification::new(PromptLevel::Info, self.text.clone())
        }
    }
}

impl Render for StatusToast {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = self.this_handle.clone();
        Balloon::new(("status-toast", cx.entity_id()), BalloonType::Information)
            .tokens(crate::frame::tokens(cx))
            .when_some(self.icon.clone(), |balloon, icon| {
                balloon.icon(icon.size(IconSize::Small))
            })
            .text(self.text.clone())
            .actions(self.actions.iter().cloned())
            .sticky(!self.auto_dismiss)
            .on_close(move |_, cx| this.update(cx, |_, cx| cx.emit(DismissEvent)))
            .on_close_all(close_all_balloons)
    }
}

impl Focusable for StatusToast {
    fn focus_handle(&self, _cx: &App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for StatusToast {}
impl EventEmitter<SuppressEvent> for StatusToast {}
impl Notification for StatusToast {}

fn workspace_error_notification_id() -> NotificationId {
    struct WorkspaceErrorNotification;
    NotificationId::unique::<WorkspaceErrorNotification>()
}

pub fn markdown_style(window: &Window, cx: &App) -> MarkdownStyle {
    let settings = ThemeSettings::get_global(cx);
    let ui_font_family = settings.ui_font.family.clone();
    let ui_font_fallbacks = settings.ui_font.fallbacks.clone();
    let buffer_font_family = settings.buffer_font.family.clone();
    let buffer_font_fallbacks = settings.buffer_font.fallbacks.clone();

    let mut base_text_style = window.text_style();
    base_text_style.refine(&TextStyleRefinement {
        font_family: Some(ui_font_family),
        font_fallbacks: ui_font_fallbacks,
        color: Some(cx.theme().colors().text),
        ..Default::default()
    });

    MarkdownStyle {
        base_text_style,
        selection_background_color: cx.theme().colors().element_selection_background,
        inline_code: TextStyleRefinement {
            background_color: Some(cx.theme().colors().editor_background.opacity(0.5)),
            font_family: Some(buffer_font_family),
            font_fallbacks: buffer_font_fallbacks,
            ..Default::default()
        },
        link: TextStyleRefinement {
            underline: Some(UnderlineStyle {
                thickness: px(1.),
                color: Some(cx.theme().colors().text_accent),
                wavy: false,
            }),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub mod simple_message_notification {
    use std::rc::Rc;
    use std::sync::Arc;

    use gpui::{
        AnyElement, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, ParentElement,
        PromptLevel, Render, SharedString,
    };
    use spork_workbench::balloon::{Balloon, BalloonAction, BalloonType};
    use ui::prelude::*;

    use crate::workspace_error::{ErrorAction, ErrorActionHandler, ErrorSeverity, WorkspaceError};

    use super::{Notification, SuppressEvent};

    /// Spork IDE: a notification shown as IntelliJ's balloon (`spork_workbench::balloon`): its
    /// buttons are the balloon's action links, each closing it once chosen; it stays 5 minutes
    /// (a sticky balloon) unless made to hide sooner.
    #[derive(RegisterComponent)]
    pub struct MessageNotification {
        focus_handle: FocusHandle,
        build_content: Box<dyn Fn(&mut Window, &mut Context<Self>) -> AnyElement>,
        content_icon: Option<IconName>,
        content_icon_color: Option<Color>,
        secondary_content: Option<SharedString>,
        copy_text: Option<SharedString>,
        primary_message: Option<SharedString>,
        primary_on_click: Option<Arc<dyn Fn(&mut Window, &mut Context<Self>)>>,
        secondary_message: Option<SharedString>,
        secondary_on_click: Option<Arc<dyn Fn(&mut Window, &mut Context<Self>)>>,
        secondary_menu_entries: Vec<(SharedString, Arc<dyn Fn(&mut Window, &mut Context<Self>)>)>,
        more_info_message: Option<SharedString>,
        more_info_url: Option<Arc<str>>,
        show_close_button: bool,
        show_suppress_button: bool,
        title: Option<SharedString>,
        sticky: bool,
        /// Spork IDE: the message, when it is text, for the Notifications tool window.
        message: Option<SharedString>,
    }

    impl Focusable for MessageNotification {
        fn focus_handle(&self, _: &App) -> FocusHandle {
            self.focus_handle.clone()
        }
    }

    impl EventEmitter<DismissEvent> for MessageNotification {}
    impl EventEmitter<SuppressEvent> for MessageNotification {}

    impl Notification for MessageNotification {}

    impl FluentBuilder for MessageNotification {}

    impl MessageNotification {
        pub fn new<S>(message: S, cx: &mut App) -> MessageNotification
        where
            S: Into<SharedString>,
        {
            let message = message.into();
            let mut notification = Self::new_from_builder(cx, {
                let message = message.clone();
                move |_, _| Label::new(message.clone()).into_any_element()
            });
            notification.message = Some(message);
            notification
        }

        /// Spork IDE: its type, from the icon it was given (an error's is red).
        pub(crate) fn kind(&self) -> BalloonType {
            match (self.content_icon, self.content_icon_color) {
                (_, Some(Color::Error)) | (Some(IconName::XCircle), _) => BalloonType::Error,
                (Some(IconName::Warning), _) => BalloonType::Warning,
                _ => BalloonType::Information,
            }
        }

        /// Spork IDE: what the Notifications tool window lists of it: its title, its text and
        /// its buttons, which run on `this`.
        pub(crate) fn logged(&self, this: &Entity<Self>) -> Option<super::LoggedNotification> {
            let text = self
                .message
                .clone()
                .or_else(|| self.secondary_content.clone())?;
            let level = match self.kind() {
                BalloonType::Error => PromptLevel::Critical,
                BalloonType::Warning => PromptLevel::Warning,
                BalloonType::Information => PromptLevel::Info,
            };
            let mut actions: Vec<(SharedString, Rc<dyn Fn(&mut Window, &mut App)>)> = Vec::new();
            for (label, on_click) in [
                (&self.primary_message, &self.primary_on_click),
                (&self.secondary_message, &self.secondary_on_click),
            ] {
                if let (Some(label), Some(on_click)) = (label.clone(), on_click.clone()) {
                    let this = this.clone();
                    actions.push((
                        label,
                        Rc::new(move |window, cx| {
                            this.update(cx, |_, cx| on_click(window, cx));
                        }),
                    ));
                }
            }
            Some(super::LoggedNotification {
                title: self.title.clone(),
                actions,
                ..super::LoggedNotification::new(level, text)
            })
        }

        pub fn new_from_builder<F>(cx: &mut App, content: F) -> MessageNotification
        where
            F: 'static + Fn(&mut Window, &mut Context<Self>) -> AnyElement,
        {
            Self {
                build_content: Box::new(content),
                content_icon: None,
                content_icon_color: None,
                secondary_content: None,
                copy_text: None,
                primary_message: None,
                primary_on_click: None,
                secondary_message: None,
                secondary_on_click: None,
                secondary_menu_entries: Vec::new(),
                more_info_message: None,
                more_info_url: None,
                show_close_button: true,
                show_suppress_button: true,
                title: None,
                sticky: true,
                focus_handle: cx.focus_handle(),
                message: None,
            }
        }

        /// Spork IDE: an IntelliJ balloon's action links have no style of their own.
        pub fn button_style(self, _style: ButtonStyle) -> Self {
            self
        }

        pub fn primary_message<S>(mut self, message: S) -> Self
        where
            S: Into<SharedString>,
        {
            self.primary_message = Some(message.into());
            self
        }

        /// Spork IDE: an IntelliJ balloon's action links have no icons.
        pub fn primary_icon(self, _icon: IconName) -> Self {
            self
        }

        pub fn primary_end_icon(self, _icon: IconName) -> Self {
            self
        }

        pub fn primary_icon_color(self, _color: Color) -> Self {
            self
        }

        pub fn primary_on_click<F>(mut self, on_click: F) -> Self
        where
            F: 'static + Fn(&mut Window, &mut Context<Self>),
        {
            self.primary_on_click = Some(Arc::new(on_click));
            self
        }

        pub fn primary_on_click_arc<F>(mut self, on_click: Arc<F>) -> Self
        where
            F: 'static + Fn(&mut Window, &mut Context<Self>),
        {
            self.primary_on_click = Some(on_click);
            self
        }

        pub fn secondary_message<S>(mut self, message: S) -> Self
        where
            S: Into<SharedString>,
        {
            self.secondary_message = Some(message.into());
            self
        }

        pub fn secondary_icon(self, _icon: IconName) -> Self {
            self
        }

        pub fn secondary_end_icon(self, _icon: IconName) -> Self {
            self
        }

        pub fn secondary_icon_color(self, _color: Color) -> Self {
            self
        }

        pub fn secondary_on_click<F>(mut self, on_click: F) -> Self
        where
            F: 'static + Fn(&mut Window, &mut Context<Self>),
        {
            self.secondary_on_click = Some(Arc::new(on_click));
            self
        }

        /// One more action. Spork IDE: an action link after the others, folding into the
        /// balloon's "More" when the links don't fit. Like the others, it closes the
        /// notification.
        pub fn secondary_menu_entry<S, F>(mut self, label: S, on_click: F) -> Self
        where
            S: Into<SharedString>,
            F: 'static + Fn(&mut Window, &mut Context<Self>),
        {
            self.secondary_menu_entries
                .push((label.into(), Arc::new(on_click)));
            self
        }

        pub fn more_info_message<S>(mut self, message: S) -> Self
        where
            S: Into<SharedString>,
        {
            self.more_info_message = Some(message.into());
            self
        }

        pub fn more_info_url<S>(mut self, url: S) -> Self
        where
            S: Into<Arc<str>>,
        {
            self.more_info_url = Some(url.into());
            self
        }

        pub fn dismiss(&mut self, cx: &mut Context<Self>) {
            cx.emit(DismissEvent);
        }

        pub fn show_close_button(mut self, show: bool) -> Self {
            self.show_close_button = show;
            self
        }

        /// Determines whether the given notification ID should be suppressible
        /// Suppressed notifications will not be shown anymor
        pub fn show_suppress_button(mut self, show: bool) -> Self {
            self.show_suppress_button = show;
            self
        }

        pub fn with_title<S>(mut self, title: S) -> Self
        where
            S: Into<SharedString>,
        {
            self.title = Some(title.into());
            self
        }

        pub fn content_icon(mut self, icon: IconName, color: Color) -> Self {
            self.content_icon = Some(icon);
            self.content_icon_color = Some(color);
            self
        }

        pub fn secondary_content<S: Into<SharedString>>(mut self, text: S) -> Self {
            self.secondary_content = Some(text.into());
            self
        }

        pub fn copy_text<S: Into<SharedString>>(mut self, text: S) -> Self {
            self.copy_text = Some(text.into());
            self
        }

        /// The text copied to the clipboard; for error notifications, the full message.
        #[cfg(any(test, feature = "test-support"))]
        pub fn copied_text(&self) -> Option<&SharedString> {
            self.copy_text.as_ref()
        }

        /// Spork IDE: whether it stays 5 minutes (IntelliJ's sticky balloon) rather than 10
        /// seconds.
        pub fn sticky(mut self, sticky: bool) -> Self {
            self.sticky = sticky;
            self
        }

        pub fn from_workspace_error<E: WorkspaceError>(error: E, cx: &mut Context<Self>) -> Self {
            let primary_message = error.primary_message();
            let severity = error.severity();
            // Spork IDE: an action that only dismisses is the balloon's close button
            let run_of = |handler: ErrorActionHandler| match handler {
                ErrorActionHandler::Action(action) => {
                    Some(move |window: &mut Window, cx: &mut Context<Self>| {
                        window.dispatch_action(action.boxed_clone(), cx);
                    })
                }
                ErrorActionHandler::Dismiss => None,
            };
            let ErrorAction {
                label,
                handler: primary,
                ..
            } = error.primary_action();
            let secondary = error
                .secondary_action()
                .and_then(|ErrorAction { label, handler, .. }| Some((label, run_of(handler)?)));
            Self::new(primary_message.clone(), cx)
                .content_icon(IconName::Warning, Color::Error)
                .copy_text(primary_message)
                .show_suppress_button(false)
                // Spork IDE: an error that Zed hides after a while hides as a balloon does
                .sticky(severity.auto_dismiss_delay().is_none())
                .when_some(error.secondary_message(), |this, text| {
                    this.secondary_content(text)
                })
                .when_some(run_of(primary), |this, on_click| {
                    this.primary_message(label).primary_on_click(on_click)
                })
                .when_some(secondary, |this, (label, on_click)| {
                    this.secondary_message(label).secondary_on_click(on_click)
                })
        }

        /// Spork IDE: its buttons as the balloon's links, each closing it once chosen.
        fn balloon_actions(&self, cx: &mut Context<Self>) -> Vec<BalloonAction> {
            type OnClick = Arc<dyn Fn(&mut Window, &mut Context<MessageNotification>)>;
            let this = cx.entity();
            let expiring = |label: &SharedString, on_click: Option<OnClick>| {
                let this = this.clone();
                BalloonAction::new(label.clone(), move |window, cx| {
                    this.update(cx, |this, cx| {
                        if let Some(on_click) = &on_click {
                            on_click(window, cx);
                        }
                        this.dismiss(cx);
                    })
                })
            };
            let mut actions = Vec::new();
            if let Some(label) = &self.primary_message {
                actions.push(expiring(label, self.primary_on_click.clone()));
            }
            if let Some(label) = &self.secondary_message {
                actions.push(expiring(label, self.secondary_on_click.clone()));
            }
            actions.extend(
                self.secondary_menu_entries
                    .iter()
                    .map(|(label, on_click)| expiring(label, Some(on_click.clone()))),
            );
            if let (Some(label), Some(url)) = (&self.more_info_message, &self.more_info_url) {
                let url = url.clone();
                actions.push(BalloonAction::new(label.clone(), move |_, cx| {
                    cx.open_url(&url)
                }));
            }
            actions
        }
    }

    impl Render for MessageNotification {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let this = cx.entity();
            let content = match self.message {
                Some(_) => None,
                None => Some((self.build_content)(window, cx)),
            };
            // its text, with what Zed shows under it after a line break
            let text = match (&self.message, &self.secondary_content) {
                (Some(message), Some(secondary)) => {
                    Some(SharedString::from(format!("{message}\n{secondary}")))
                }
                (message, secondary) => message
                    .clone()
                    .or_else(|| secondary.clone().filter(|_| content.is_none())),
            };
            let close = {
                let this = this.clone();
                move |_: &mut Window, cx: &mut App| this.update(cx, |this, cx| this.dismiss(cx))
            };
            let suppress = {
                let this = this.clone();
                move |_: &mut Window, cx: &mut App| this.update(cx, |_, cx| cx.emit(SuppressEvent))
            };
            Balloon::new(("notification", this.entity_id()), self.kind())
                .tokens(crate::frame::tokens(cx))
                .when_some(self.title.clone(), Balloon::title)
                .when_some(text, Balloon::text)
                .when_some(content, Balloon::content)
                .actions(self.balloon_actions(cx))
                .when(self.show_suppress_button, |balloon| {
                    balloon.menu_item(Some(BalloonAction::new(
                        "Don’t Show Again For This Project",
                        suppress,
                    )))
                })
                .sticky(self.sticky)
                .closable(self.show_close_button)
                .on_close(close)
                .on_close_all(super::close_all_balloons)
        }
    }

    impl Component for MessageNotification {
        fn scope() -> ComponentScope {
            ComponentScope::Notification
        }

        fn description() -> &'static str {
            "A workspace notification that surfaces a message in a framed container, with an \
            optional title, secondary message, copy button, and primary/secondary action buttons."
        }

        fn preview(_window: &mut Window, cx: &mut App) -> AnyElement {
            let normal =
                cx.new(|cx| MessageNotification::new("A regular informational notification.", cx));

            let with_title = cx.new(|cx| {
                MessageNotification::new("Some informational content for the user.", cx)
                    .with_title("Notification Title")
            });

            let with_primary_action = cx.new(|cx| {
                MessageNotification::new(
                    "A new version of Spork IDE is available for download.",
                    cx,
                )
                .with_title("Update Available")
                .primary_message("Restart Now")
                .primary_icon(IconName::ArrowCircle)
            });

            let with_end_icon_action = cx.new(|cx| {
                MessageNotification::new("Release notes for this version are available online.", cx)
                    .with_title("What’s New")
                    .primary_message("Read Release Notes")
                    .primary_end_icon(IconName::ArrowUpRight)
            });

            // Mirrors the shape of notifications such as the keymap parse error: a long,
            // multi-line message followed by a primary action button. Useful for catching
            // regressions where the action row overlaps or is clipped by the content above.
            let with_long_content_and_action = cx.new(|cx| {
                let long_message = "Errors in user keymap file. In section with context = \
                    \"Workspace\":\n\
                    • In binding \"ctrl-r\", expected two-element array of [name, input], \
                    found [\"editor::Apply\"].\n\
                    • In binding \"ctrl-shift-r\", action \"editor::Reload\" is not registered.";
                MessageNotification::new(long_message, cx)
                    .primary_message("Open Keymap File")
                    .primary_icon(IconName::Settings)
            });

            struct PreviewError;
            impl WorkspaceError for PreviewError {
                fn primary_message(&self) -> SharedString {
                    "Something went wrong while loading your project.".into()
                }

                fn primary_action(&self) -> ErrorAction {
                    ErrorAction::dismiss()
                }

                fn secondary_message(&self) -> Option<SharedString> {
                    Some("Check your network connection and try again.".into())
                }
                fn severity(&self) -> ErrorSeverity {
                    ErrorSeverity::Error
                }
            }
            let error_state =
                cx.new(|cx| MessageNotification::from_workspace_error(PreviewError, cx));

            let close_only = cx
                .new(|cx| MessageNotification::new("Default header with just a close button.", cx));

            let copy_and_close = cx.new(|cx| {
                let msg: SharedString = "This message can be copied to the clipboard.".into();
                MessageNotification::new(msg.clone(), cx).copy_text(msg)
            });

            let no_close = cx.new(|cx| {
                MessageNotification::new("This notification can't be closed manually.", cx)
                    .show_close_button(false)
            });

            // --- Workspace errors ---
            // These showcase common shapes of [`WorkspaceError`]. They are intentionally
            // [`ErrorSeverity::Critical`] so they never auto-dismiss in the preview, which
            // would otherwise make them disappear mid-inspection.

            struct BasicError;
            impl WorkspaceError for BasicError {
                fn primary_message(&self) -> SharedString {
                    "Failed to save the file.".into()
                }
                fn primary_action(&self) -> ErrorAction {
                    ErrorAction::dismiss()
                }
                fn severity(&self) -> ErrorSeverity {
                    ErrorSeverity::Critical
                }
            }

            struct LanguageServerError;
            impl WorkspaceError for LanguageServerError {
                fn primary_message(&self) -> SharedString {
                    "Error: Prepare rename via rust-analyzer failed: No references found at position"
                        .into()
                }
                fn secondary_message(&self) -> Option<SharedString> {
                    None
                }
                fn primary_action(&self) -> ErrorAction {
                    ErrorAction::dismiss()
                }
                fn severity(&self) -> ErrorSeverity {
                    ErrorSeverity::Critical
                }
            }

            // Mirrors the shape of [`super::super::PortalError`]: a critical error with a
            // documentation link as its primary action.
            struct PortalSetupError;
            impl WorkspaceError for PortalSetupError {
                fn primary_message(&self) -> SharedString {
                    "Linux desktop portal initialization failed.".into()
                }
                fn secondary_message(&self) -> Option<SharedString> {
                    Some(
                        "Spork IDE needs an xdg-desktop-portal implementation to open files."
                            .into(),
                    )
                }
                fn severity(&self) -> ErrorSeverity {
                    ErrorSeverity::Critical
                }
                fn primary_action(&self) -> ErrorAction {
                    ErrorAction::link(
                        "See Docs",
                        "https://zed.dev/docs/linux#i-cant-open-any-files",
                    )
                }
            }

            // Has both a primary action (link) and a secondary action (dismiss), so the
            // preview exercises the full button row.
            struct UpdateRequiredError;
            impl WorkspaceError for UpdateRequiredError {
                fn primary_message(&self) -> SharedString {
                    "An update is required to continue using Zed AI.".into()
                }
                fn severity(&self) -> ErrorSeverity {
                    ErrorSeverity::Critical
                }
                fn primary_action(&self) -> ErrorAction {
                    ErrorAction::link("Update Zed", "https://zed.dev/releases")
                }
                fn secondary_action(&self) -> Option<ErrorAction> {
                    Some(ErrorAction::dismiss())
                }
            }

            let basic_error =
                cx.new(|cx| MessageNotification::from_workspace_error(BasicError, cx));
            let detailed_error =
                cx.new(|cx| MessageNotification::from_workspace_error(LanguageServerError, cx));
            let docs_error =
                cx.new(|cx| MessageNotification::from_workspace_error(PortalSetupError, cx));
            let update_error =
                cx.new(|cx| MessageNotification::from_workspace_error(UpdateRequiredError, cx));

            let container = || div().w(px(440.));

            v_flex()
                .gap_6()
                .p_4()
                .children(vec![
                    example_group_with_title(
                        "States",
                        vec![
                            single_example("Normal", container().child(normal).into_any_element()),
                            single_example(
                                "With Title",
                                container().child(with_title).into_any_element(),
                            ),
                            single_example(
                                "With Primary Action (start icon)",
                                container().child(with_primary_action).into_any_element(),
                            ),
                            single_example(
                                "With Primary Action (end icon)",
                                container().child(with_end_icon_action).into_any_element(),
                            ),
                            single_example(
                                "Long Content + Primary Action",
                                container()
                                    .child(with_long_content_and_action)
                                    .into_any_element(),
                            ),
                            single_example(
                                "Error",
                                container().child(error_state).into_any_element(),
                            ),
                        ],
                    ),
                    example_group_with_title(
                        "Header Actions (top right)",
                        vec![
                            single_example(
                                "Close Only",
                                container().child(close_only).into_any_element(),
                            ),
                            single_example(
                                "Copy + Close",
                                container().child(copy_and_close).into_any_element(),
                            ),
                            single_example(
                                "No Close",
                                container().child(no_close).into_any_element(),
                            ),
                        ],
                    ),
                    example_group_with_title(
                        "Workspace Errors",
                        vec![
                            single_example(
                                "Basic",
                                container().child(basic_error).into_any_element(),
                            ),
                            single_example(
                                "With Secondary Message",
                                container().child(detailed_error).into_any_element(),
                            ),
                            single_example(
                                "With Documentation Link",
                                container().child(docs_error).into_any_element(),
                            ),
                            single_example(
                                "With Primary + Secondary Action",
                                container().child(update_error).into_any_element(),
                            ),
                        ],
                    ),
                ])
                .into_any_element()
        }
    }
}

static GLOBAL_APP_NOTIFICATIONS: LazyLock<Mutex<AppNotifications>> = LazyLock::new(|| {
    Mutex::new(AppNotifications {
        app_notifications: Vec::new(),
    })
});

/// Stores app notifications so that they can be shown in new workspaces.
struct AppNotifications {
    app_notifications: Vec<(
        NotificationId,
        Arc<dyn Fn(&mut Context<Workspace>) -> AnyView + Send + Sync>,
    )>,
}

impl AppNotifications {
    pub fn insert(
        &mut self,
        id: NotificationId,
        build_notification: Arc<dyn Fn(&mut Context<Workspace>) -> AnyView + Send + Sync>,
    ) {
        self.remove(&id);
        self.app_notifications.push((id, build_notification))
    }

    pub fn remove(&mut self, id: &NotificationId) {
        self.app_notifications
            .retain(|(existing_id, _)| existing_id != id);
    }
}

/// Shows a notification in all workspaces. New workspaces will also receive the notification - this
/// is particularly to handle notifications that occur on initialization before any workspaces
/// exist. If the notification is dismissed within any workspace, it will be removed from all.
pub fn show_app_notification<V: Notification + 'static>(
    id: NotificationId,
    cx: &mut App,
    build_notification: impl Fn(&mut Context<Workspace>) -> Entity<V> + 'static + Send + Sync,
) {
    // Defer notification creation so that windows on the stack can be returned to GPUI
    cx.defer(move |cx| {
        // Handle dismiss events by removing the notification from all workspaces.
        let build_notification: Arc<dyn Fn(&mut Context<Workspace>) -> AnyView + Send + Sync> =
            Arc::new({
                let id = id.clone();
                move |cx| {
                    let notification = build_notification(cx);
                    cx.subscribe(&notification, {
                        let id = id.clone();
                        move |_, _, _: &DismissEvent, cx| {
                            dismiss_app_notification(&id, cx);
                        }
                    })
                    .detach();
                    cx.subscribe(&notification, {
                        let id = id.clone();
                        move |workspace: &mut Workspace, _, _: &SuppressEvent, cx| {
                            workspace.suppress_notification(&id, cx);
                        }
                    })
                    .detach();
                    notification.into()
                }
            });

        // Store the notification so that new workspaces also receive it.
        GLOBAL_APP_NOTIFICATIONS
            .lock()
            .insert(id.clone(), build_notification.clone());

        for window in cx.windows() {
            if let Some(multi_workspace) = window.downcast::<MultiWorkspace>() {
                multi_workspace
                    .update(cx, |multi_workspace, _window, cx| {
                        for workspace in multi_workspace.workspaces() {
                            workspace.update(cx, |workspace, cx| {
                                workspace.show_notification_without_handling_dismiss_events(
                                    &id,
                                    cx,
                                    |cx| build_notification(cx),
                                );
                            });
                        }
                    })
                    .ok(); // Doesn't matter if the windows are dropped
            }
        }
    });
}

pub fn dismiss_app_notification(id: &NotificationId, cx: &mut App) {
    let id = id.clone();
    // Defer notification dismissal so that windows on the stack can be returned to GPUI
    cx.defer(move |cx| {
        GLOBAL_APP_NOTIFICATIONS.lock().remove(&id);
        for window in cx.windows() {
            if let Some(multi_workspace) = window.downcast::<MultiWorkspace>() {
                let id = id.clone();
                multi_workspace
                    .update(cx, |multi_workspace, _window, cx| {
                        for workspace in multi_workspace.workspaces() {
                            workspace.update(cx, |workspace, cx| {
                                workspace.dismiss_notification(&id, cx)
                            });
                        }
                    })
                    .ok();
            }
        }
    });
}

pub trait NotifyResultExt {
    type Ok;

    fn notify_err(self, workspace: &mut Workspace, cx: &mut Context<Workspace>)
    -> Option<Self::Ok>;

    fn notify_workspace_async_err(
        self,
        workspace: WeakEntity<Workspace>,
        cx: &mut AsyncApp,
    ) -> Option<Self::Ok>;

    /// Notifies the active workspace if there is one, otherwise notifies all workspaces.
    fn notify_app_err(self, cx: &mut App) -> Option<Self::Ok>;
}

impl<T, E> NotifyResultExt for std::result::Result<T, E>
where
    E: std::fmt::Debug + std::fmt::Display,
{
    type Ok = T;

    fn notify_err(self, workspace: &mut Workspace, cx: &mut Context<Workspace>) -> Option<T> {
        match self {
            Ok(value) => Some(value),
            Err(err) => {
                log::error!("Showing error notification in workspace: {err:?}");
                workspace.show_error(format!("Error: {err}"), cx);
                None
            }
        }
    }

    fn notify_workspace_async_err(
        self,
        workspace: WeakEntity<Workspace>,
        cx: &mut AsyncApp,
    ) -> Option<T> {
        match self {
            Ok(value) => Some(value),
            Err(err) => {
                log::error!("{err:?}");
                let message = format!("Error: {err}");
                workspace
                    .update(cx, |workspace, cx| workspace.show_error(message, cx))
                    .ok();
                None
            }
        }
    }

    fn notify_app_err(self, cx: &mut App) -> Option<T> {
        match self {
            Ok(value) => Some(value),
            Err(err) => {
                let message = format!("Error: {err}");
                log::error!("Showing error notification in app: {message}");
                show_app_notification(workspace_error_notification_id(), cx, {
                    move |cx| {
                        cx.new({
                            let message = message.clone();
                            move |cx| {
                                simple_message_notification::MessageNotification::from_workspace_error(message, cx)
                            }
                        })
                    }
                });

                None
            }
        }
    }
}

pub trait NotifyTaskExt {
    fn detach_and_notify_err(
        self,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut App,
    );
}

impl<R, E> NotifyTaskExt for Task<std::result::Result<R, E>>
where
    E: std::fmt::Debug + std::fmt::Display + Sized + 'static,
    R: 'static,
{
    fn detach_and_notify_err(
        self,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut App,
    ) {
        window
            .spawn(cx, async move |mut cx| {
                self.await.notify_workspace_async_err(workspace, &mut cx)
            })
            .detach();
    }
}

pub trait DetachAndPromptErr<R> {
    fn prompt_err(
        self,
        msg: &str,
        window: &Window,
        cx: &App,
        f: impl FnOnce(&anyhow::Error, &mut Window, &mut App) -> Option<String> + 'static,
    ) -> Task<Option<R>>;

    fn detach_and_prompt_err(
        self,
        msg: &str,
        window: &Window,
        cx: &App,
        f: impl FnOnce(&anyhow::Error, &mut Window, &mut App) -> Option<String> + 'static,
    );
}

impl<R> DetachAndPromptErr<R> for Task<anyhow::Result<R>>
where
    R: 'static,
{
    fn prompt_err(
        self,
        msg: &str,
        window: &Window,
        cx: &App,
        f: impl FnOnce(&anyhow::Error, &mut Window, &mut App) -> Option<String> + 'static,
    ) -> Task<Option<R>> {
        let msg = msg.to_owned();
        window.spawn(cx, async move |cx| {
            let result = self.await;
            if let Err(err) = result.as_ref() {
                log::error!("{err:#}");
                if let Ok(prompt) = cx.update(|window, cx| {
                    let mut display = format!("{err:#}");
                    if !display.ends_with('\n') {
                        display.push('.');
                    }
                    let detail = f(err, window, cx).unwrap_or(display);
                    window.prompt(PromptLevel::Critical, &msg, Some(&detail), &["OK"], cx)
                }) {
                    prompt.await.ok();
                }
                return None;
            }
            Some(result.unwrap())
        })
    }

    fn detach_and_prompt_err(
        self,
        msg: &str,
        window: &Window,
        cx: &App,
        f: impl FnOnce(&anyhow::Error, &mut Window, &mut App) -> Option<String> + 'static,
    ) {
        self.prompt_err(msg, window, cx, f).detach();
    }
}

#[cfg(test)]
mod tests {
    use fs::FakeFs;
    use gpui::{TestAppContext, VisualTestContext};
    use lsp::MessageActionItem;
    use project::{LanguageServerPromptRequest, Project};
    use std::collections::HashMap;

    use crate::tests::init_test;

    use super::*;

    // Spork IDE: the Notifications tool window's log keeps what a notification said and its
    // buttons after it is dismissed, until Clear all
    #[gpui::test]
    async fn test_notification_log_keeps_text_and_actions(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (workspace, cx) =
            cx.add_window_view(|window, cx| Workspace::test_new(project.clone(), window, cx));
        let clicked = Rc::new(std::cell::Cell::new(false));
        workspace.update(cx, |workspace, cx| {
            let clicked = clicked.clone();
            workspace.show_notification(NotificationId::named("pushed".into()), cx, |cx| {
                cx.new(|cx| {
                    simple_message_notification::MessageNotification::new("Pushed to origin", cx)
                        .primary_message("View")
                        .primary_on_click(move |_, _| clicked.set(true))
                })
            });
            workspace.dismiss_notification(&NotificationId::named("pushed".into()), cx);
        });
        let logged = workspace.read_with(cx, |workspace, _| workspace.notification_log().to_vec());
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].text.as_ref(), "Pushed to origin");
        assert_eq!(logged[0].actions.len(), 1);
        assert_eq!(logged[0].actions[0].0.as_ref(), "View");
        let run = logged[0].actions[0].1.clone();
        cx.update(|window, cx| run(window, cx));
        assert!(clicked.get());
        workspace.update(cx, |workspace, cx| workspace.clear_notification_log(cx));
        workspace.read_with(cx, |workspace, _| {
            assert!(workspace.notification_log().is_empty())
        });
    }

    #[gpui::test]
    async fn test_notification_auto_dismiss_with_notifications_from_multiple_language_servers(
        cx: &mut TestAppContext,
    ) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;

        let (workspace, cx) =
            cx.add_window_view(|window, cx| Workspace::test_new(project.clone(), window, cx));

        let count_notifications = |workspace: &Entity<Workspace>, cx: &mut TestAppContext| {
            workspace.read_with(cx, |workspace, _| workspace.notification_ids().len())
        };

        let show_notification = |workspace: &Entity<Workspace>,
                                 cx: &mut TestAppContext,
                                 lsp_name: &str| {
            workspace.update(cx, |workspace, cx| {
                let request = LanguageServerPromptRequest::test(
                    gpui::PromptLevel::Warning,
                    "Test notification".to_string(),
                    vec![], // Empty actions triggers auto-dismiss
                    lsp_name.to_string(),
                );
                let notification_id = NotificationId::composite::<LanguageServerPrompt>(request.id);
                workspace.show_notification(notification_id, cx, |cx| {
                    cx.new(|cx| LanguageServerPrompt::new(request, cx))
                });
            })
        };

        show_notification(&workspace, cx, "Lsp1");
        assert_eq!(count_notifications(&workspace, cx), 1);

        cx.executor().advance_clock(Duration::from_millis(1000));

        show_notification(&workspace, cx, "Lsp2");
        assert_eq!(count_notifications(&workspace, cx), 2);

        cx.executor().advance_clock(Duration::from_millis(1000));

        show_notification(&workspace, cx, "Lsp3");
        assert_eq!(count_notifications(&workspace, cx), 3);

        cx.executor().advance_clock(Duration::from_millis(3000));
        assert_eq!(count_notifications(&workspace, cx), 2);

        cx.executor().advance_clock(Duration::from_millis(1000));
        assert_eq!(count_notifications(&workspace, cx), 1);

        cx.executor().advance_clock(Duration::from_millis(1000));
        assert_eq!(count_notifications(&workspace, cx), 0);
    }

    #[gpui::test]
    async fn test_notification_auto_dismiss_with_multiple_notifications_from_single_language_server(
        cx: &mut TestAppContext,
    ) {
        init_test(cx);

        let lsp_name = "server1";

        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (workspace, cx) =
            cx.add_window_view(|window, cx| Workspace::test_new(project.clone(), window, cx));

        let count_notifications = |workspace: &Entity<Workspace>, cx: &mut TestAppContext| {
            workspace.read_with(cx, |workspace, _| workspace.notification_ids().len())
        };

        let show_notification = |lsp_name: &str,
                                 workspace: &Entity<Workspace>,
                                 cx: &mut TestAppContext| {
            workspace.update(cx, |workspace, cx| {
                let lsp_name = lsp_name.to_string();
                let request = LanguageServerPromptRequest::test(
                    gpui::PromptLevel::Warning,
                    "Test notification".to_string(),
                    vec![], // Empty actions triggers auto-dismiss
                    lsp_name,
                );
                let notification_id = NotificationId::composite::<LanguageServerPrompt>(request.id);

                workspace.show_notification(notification_id, cx, |cx| {
                    cx.new(|cx| LanguageServerPrompt::new(request, cx))
                });
            })
        };

        show_notification(lsp_name, &workspace, cx);
        assert_eq!(count_notifications(&workspace, cx), 1);

        cx.executor().advance_clock(Duration::from_millis(1000));

        show_notification(lsp_name, &workspace, cx);
        assert_eq!(count_notifications(&workspace, cx), 2);

        cx.executor().advance_clock(Duration::from_millis(4000));
        assert_eq!(count_notifications(&workspace, cx), 1);

        cx.executor().advance_clock(Duration::from_millis(1000));
        assert_eq!(count_notifications(&workspace, cx), 0);
    }

    #[gpui::test]
    async fn test_notification_auto_dismiss_turned_off(cx: &mut TestAppContext) {
        init_test(cx);

        cx.update(|cx| {
            let mut settings = ProjectSettings::get_global(cx).clone();
            settings
                .global_lsp_settings
                .notifications
                .dismiss_timeout_ms = Some(0);
            ProjectSettings::override_global(settings, cx);
        });

        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (workspace, cx) =
            cx.add_window_view(|window, cx| Workspace::test_new(project.clone(), window, cx));

        let count_notifications = |workspace: &Entity<Workspace>, cx: &mut TestAppContext| {
            workspace.read_with(cx, |workspace, _| workspace.notification_ids().len())
        };

        workspace.update(cx, |workspace, cx| {
            let request = LanguageServerPromptRequest::test(
                gpui::PromptLevel::Warning,
                "Test notification".to_string(),
                vec![], // Empty actions would trigger auto-dismiss if enabled
                "test_server".to_string(),
            );
            let notification_id = NotificationId::composite::<LanguageServerPrompt>(request.id);
            workspace.show_notification(notification_id, cx, |cx| {
                cx.new(|cx| LanguageServerPrompt::new(request, cx))
            });
        });

        assert_eq!(count_notifications(&workspace, cx), 1);

        // Advance time beyond the default auto-dismiss duration
        cx.executor().advance_clock(Duration::from_millis(10000));
        assert_eq!(count_notifications(&workspace, cx), 1);
    }

    #[gpui::test]
    async fn test_notification_auto_dismiss_with_custom_duration(cx: &mut TestAppContext) {
        init_test(cx);

        let custom_duration_ms: u64 = 2000;
        cx.update(|cx| {
            let mut settings = ProjectSettings::get_global(cx).clone();
            settings
                .global_lsp_settings
                .notifications
                .dismiss_timeout_ms = Some(custom_duration_ms);
            ProjectSettings::override_global(settings, cx);
        });

        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (workspace, cx) =
            cx.add_window_view(|window, cx| Workspace::test_new(project.clone(), window, cx));

        let count_notifications = |workspace: &Entity<Workspace>, cx: &mut TestAppContext| {
            workspace.read_with(cx, |workspace, _| workspace.notification_ids().len())
        };

        workspace.update(cx, |workspace, cx| {
            let request = LanguageServerPromptRequest::test(
                gpui::PromptLevel::Warning,
                "Test notification".to_string(),
                vec![], // Empty actions triggers auto-dismiss
                "test_server".to_string(),
            );
            let notification_id = NotificationId::composite::<LanguageServerPrompt>(request.id);
            workspace.show_notification(notification_id, cx, |cx| {
                cx.new(|cx| LanguageServerPrompt::new(request, cx))
            });
        });

        assert_eq!(count_notifications(&workspace, cx), 1);

        // Advance time less than custom duration
        cx.executor()
            .advance_clock(Duration::from_millis(custom_duration_ms - 500));
        assert_eq!(count_notifications(&workspace, cx), 1);

        // Advance time past the custom duration
        cx.executor().advance_clock(Duration::from_millis(1000));
        assert_eq!(count_notifications(&workspace, cx), 0);
    }

    #[gpui::test]
    async fn test_language_server_prompt_deduplication(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (workspace, cx) =
            cx.add_window_view(|window, cx| Workspace::test_new(project.clone(), window, cx));

        workspace.update(cx, |workspace, cx| {
            workspace.show_notification(NotificationId::unique::<()>(), cx, |cx| {
                cx.new(|cx| simple_message_notification::MessageNotification::new("Unrelated", cx))
            });
        });

        let long_message = "x".repeat(65536);
        let prompts = [
            ("server", PromptLevel::Warning, ""),
            ("server", PromptLevel::Info, ""),
            ("server", PromptLevel::Critical, ""),
            ("other", PromptLevel::Warning, ""),
            ("server", PromptLevel::Warning, "é🦀"),
            ("server", PromptLevel::Warning, long_message.as_str()),
            ("a", PromptLevel::Warning, "b:Warning:c"),
            ("a:Warning:b", PromptLevel::Warning, "c"),
        ];
        for (index, (server, level, message)) in prompts.into_iter().enumerate() {
            emit_language_server_prompt(&project, server, level, message, Vec::new(), cx);
            let original_views =
                workspace.read_with(cx, |workspace, _| workspace.notification_views());
            // Spork IDE: three balloons show at once, as IntelliJ's
            assert_eq!(original_views.len(), (index + 2).min(balloon::MAX_SHOWN));
            emit_language_server_prompt(&project, server, level, message, Vec::new(), cx);
            assert_eq!(
                workspace.read_with(cx, |workspace, _| workspace.notification_views()),
                original_views
            );
        }

        workspace.update(cx, |workspace, cx| workspace.clear_all_notifications(cx));
        let actions = vec![MessageActionItem {
            title: String::from("Retry"),
            properties: HashMap::default(),
        }];
        let mut expected_ids = Vec::new();
        for (actions, should_show) in [
            (actions.clone(), true),
            (Vec::new(), true),
            (Vec::new(), false),
            (actions, true),
        ] {
            let id = emit_language_server_prompt(
                &project,
                "server",
                PromptLevel::Warning,
                "same message",
                actions,
                cx,
            );
            if should_show {
                expected_ids.push(id);
            }
            assert_eq!(
                workspace.read_with(cx, |workspace, _| workspace.notification_ids()),
                expected_ids
            );
        }
    }

    #[gpui::test]
    async fn test_language_server_prompt_deduplication_preserves_dismissal(
        cx: &mut TestAppContext,
    ) {
        init_test(cx);
        cx.update(|cx| {
            let mut settings = ProjectSettings::get_global(cx).clone();
            settings
                .global_lsp_settings
                .notifications
                .dismiss_timeout_ms = Some(1);
            ProjectSettings::override_global(settings, cx);
        });
        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (workspace, cx) =
            cx.add_window_view(|window, cx| Workspace::test_new(project.clone(), window, cx));
        let emit_prompt = |cx: &mut VisualTestContext| {
            emit_language_server_prompt(
                &project,
                "server",
                PromptLevel::Warning,
                "same message",
                Vec::new(),
                cx,
            )
        };
        let notification_ids = |cx: &mut VisualTestContext| {
            workspace.read_with(cx, |workspace, _| workspace.notification_ids())
        };

        let original_id = emit_prompt(cx);
        cx.executor().advance_clock(Duration::from_micros(999));
        emit_prompt(cx);
        assert_eq!(notification_ids(cx), vec![original_id]);
        cx.executor().advance_clock(Duration::from_micros(1));
        assert_eq!(notification_ids(cx), Vec::new());

        let id = emit_prompt(cx);
        assert_eq!(notification_ids(cx), vec![id.clone()]);
        workspace.update(cx, |workspace, cx| workspace.dismiss_notification(&id, cx));
        let id = emit_prompt(cx);
        assert_eq!(notification_ids(cx), vec![id.clone()]);
        workspace.update(cx, |workspace, cx| workspace.suppress_notification(&id, cx));
        let id = emit_prompt(cx);
        assert_eq!(notification_ids(cx), vec![id]);

        workspace.update(cx, |workspace, cx| workspace.clear_all_notifications(cx));
        cx.update(|_, cx| {
            let mut settings = ProjectSettings::get_global(cx).clone();
            settings
                .global_lsp_settings
                .notifications
                .dismiss_timeout_ms = Some(0);
            ProjectSettings::override_global(settings, cx);
        });
        let id = emit_prompt(cx);
        emit_prompt(cx);
        cx.executor().advance_clock(Duration::from_millis(10000));
        assert_eq!(notification_ids(cx), vec![id]);
    }

    fn emit_language_server_prompt(
        project: &Entity<Project>,
        server: &str,
        level: PromptLevel,
        message: &str,
        actions: Vec<MessageActionItem>,
        cx: &mut VisualTestContext,
    ) -> NotificationId {
        let request = LanguageServerPromptRequest::test(
            level,
            message.to_owned(),
            actions,
            server.to_owned(),
        );
        let id = NotificationId::composite::<LanguageServerPrompt>(request.id);
        project.update(cx, |_, cx| {
            cx.emit(project::Event::LanguageServerPrompt(request));
        });
        cx.run_until_parked();
        id
    }
}
