//! Bisect: start from a known-good commit, then mark each checked-out commit good or bad
//! until git names the first bad one (docs/specs/M05 §8).

use crate::notices::Notification;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_git::bisect::{self, Progress, Verdict};

use crate::RepoView;

impl RepoView {
    /// Commit menu: bisect between HEAD (bad) and `good`.
    pub(crate) fn start_bisect(
        &mut self,
        good: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_op(
            window,
            cx,
            move |root| bisect::start(root, "HEAD", &good),
            Self::bisect_progress,
        );
    }

    fn bisect_progress(&mut self, progress: Progress, window: &mut Window, cx: &mut Context<Self>) {
        self.bisect = Some(progress.clone());
        if let Progress::Found(id) = progress {
            let subject = match &self.repo {
                Some(Ok(repo)) => repo
                    .history
                    .commits
                    .iter()
                    .find(|c| c.id.starts_with(&id))
                    .map(|c| c.subject.clone()),
                _ => None,
            };
            let short: String = id.chars().take(7).collect();
            window.push_notification(
                Notification::success(format!(
                    "First bad commit: {short} {}",
                    subject.unwrap_or_default()
                )),
                cx,
            );
        }
    }

    fn bisect_step(
        &mut self,
        verdict: Option<Verdict>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match verdict {
            Some(verdict) => self.run_op(
                window,
                cx,
                move |root| bisect::mark(root, verdict),
                Self::bisect_progress,
            ),
            None => self.run_op(window, cx, bisect::reset, |this, (), _, cx| {
                this.bisect = None;
                cx.notify();
            }),
        }
    }

    /// Shown while a bisect runs (also one started in a terminal).
    pub(crate) fn bisect_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        let Some(Ok(repo)) = &self.repo else {
            return None;
        };
        if !repo.bisecting {
            return None;
        }
        let (title, testing) = match &self.bisect {
            Some(Progress::Found(id)) => (
                format!(
                    "Found the first bad commit: {}",
                    id.chars().take(7).collect::<String>()
                ),
                false,
            ),
            Some(Progress::Testing { steps }) => {
                (format!("Bisecting, about {steps} more step(s)"), true)
            }
            None => ("Bisecting".to_owned(), true),
        };
        let theme = cx.theme();
        let button = |id: &'static str, label: &'static str, verdict: Option<Verdict>| {
            Button::new(id)
                .small()
                .ghost()
                .label(label)
                .disabled(self.busy)
                .on_click(
                    cx.listener(move |this, _, window, cx| this.bisect_step(verdict, window, cx)),
                )
        };
        Some(
            h_flex()
                .flex_none()
                .px_4()
                .py_2()
                .gap_3()
                .justify_between()
                .bg(theme.info.opacity(0.12))
                .border_b_1()
                .border_color(theme.info.opacity(0.3))
                .child(
                    v_flex()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(title),
                        )
                        .child(div().text_xs().text_color(theme.muted_foreground).child(
                            if testing {
                                "Test the checked-out commit, then say whether it works."
                            } else {
                                "Stop to go back to the branch you started from."
                            },
                        )),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .when(testing, |h| {
                            h.child(button("bisect-good", "Good", Some(Verdict::Good)))
                                .child(button("bisect-bad", "Bad", Some(Verdict::Bad)))
                                .child(button("bisect-skip", "Skip", Some(Verdict::Skip)))
                        })
                        .child(button("bisect-reset", "Stop bisect", None)),
                ),
        )
    }
}
