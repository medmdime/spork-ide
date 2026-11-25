//! The editor's git blame: the gutter and inline annotations and the status bar entry, kept from
//! `git_ui` when it was removed. Opening a commit, the hover popover and avatars went with it; the
//! gutter is IntelliJ's annotations (`spork_vcs::annotate`).

use editor::{BlameRenderer, Editor};
use git::{blame::BlameEntry, commit::ParsedCommitMessage};
use gpui::{AnyElement, Entity, Hsla, Pixels, ScrollHandle, Subscription, TextStyle, WeakEntity};
use markdown::Markdown;
use project::{
    git_store::Repository,
    project_settings::{GitDateFormat, InlineBlameLocation, ProjectSettings},
};
use settings::{GitDateStyle, Settings as _};
use time::{OffsetDateTime, UtcOffset};
use time_format::TimestampFormat;
use ui::{Tooltip, prelude::*};
use workspace::Workspace;

pub struct GitBlameRenderer;

fn format_blame_text(blame_entry: &BlameEntry, style: GitDateStyle, cx: &App) -> String {
    let timestamp = blame_entry_timestamp(blame_entry, style, cx);
    let author = blame_entry.author.as_deref().unwrap_or_default();
    let summary_enabled = ProjectSettings::get_global(cx)
        .git
        .inline_blame
        .show_commit_summary;

    match blame_entry.summary.as_ref() {
        Some(summary) if summary_enabled => {
            format!("{author}, {timestamp} - {summary}")
        }
        _ => format!("{author}, {timestamp}"),
    }
}

fn blame_entry_timestamp(blame_entry: &BlameEntry, style: GitDateStyle, cx: &App) -> String {
    match blame_entry.author_offset_date_time() {
        Ok(timestamp) => format_git_date(timestamp, style, cx),
        Err(_) => "Error parsing date".to_string(),
    }
}

#[derive(Default)]
pub struct GitBlameStatus {
    text: Option<(SharedString, SharedString)>,
    _subscriptions: Vec<Subscription>,
}

impl GitBlameStatus {
    fn update(&mut self, editor: Entity<Editor>, _window: &mut Window, cx: &mut Context<Self>) {
        let inline_blame = ProjectSettings::get_global(cx).git.inline_blame;
        let text =
            if inline_blame.enabled && inline_blame.location == InlineBlameLocation::StatusBar {
                editor
                    .update(cx, |editor, cx| editor.active_git_blame_entry(cx))
                    .map(|blame_entry| {
                        (
                            format_blame_text(&blame_entry, inline_blame.date_style, cx).into(),
                            format_blame_text(&blame_entry, GitDateStyle::Absolute, cx).into(),
                        )
                    })
            } else {
                None
            };

        if text != self.text {
            self.text = text;
            cx.notify();
        }
    }
}

impl Render for GitBlameStatus {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let inline_blame = ProjectSettings::get_global(cx).git.inline_blame;
        if !inline_blame.enabled || inline_blame.location != InlineBlameLocation::StatusBar {
            return div();
        }

        div().when_some(self.text.clone(), |el, (text, tooltip)| {
            el.child(
                Button::new("git-blame-status", text)
                    .label_size(LabelSize::Small)
                    .start_icon(
                        Icon::new(IconName::FileGit)
                            .size(IconSize::Small)
                            .color(Color::Hint),
                    )
                    .tooltip(Tooltip::text(tooltip)),
            )
        })
    }
}

impl workspace::StatusItemView for GitBlameStatus {
    fn set_active_pane_item(
        &mut self,
        active_pane_item: Option<&dyn workspace::item::ItemHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = active_pane_item.and_then(|item| item.act_as::<Editor>(cx)) {
            self._subscriptions = vec![cx.observe_in(&editor, window, Self::update)];
            self.update(editor, window, cx);
        } else {
            self.text = None;
            self._subscriptions.clear();
            cx.notify();
        }
    }

    fn hide_setting(&self, _: &App) -> Option<workspace::HideStatusItem> {
        None
    }
}

impl BlameRenderer for GitBlameRenderer {
    fn max_author_length(&self) -> usize {
        spork_vcs::annotate::author_chars()
    }

    fn max_timestamp_width(&self, _: &App) -> usize {
        spork_vcs::annotate::date_chars()
    }

    fn blame_entry_non_text_width(&self, window: &Window, cx: &App) -> Pixels {
        spork_vcs::annotate::non_text_width(window, cx)
    }

    fn render_blame_entry(
        &self,
        style: &TextStyle,
        blame_entry: BlameEntry,
        details: Option<ParsedCommitMessage>,
        _tag_names: Vec<SharedString>,
        _repository: Entity<Repository>,
        _workspace: WeakEntity<Workspace>,
        editor: Entity<Editor>,
        ix: usize,
        _sha_color: Hsla,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        spork_vcs::annotate::render_entry(style, blame_entry, details, editor, ix, window, cx)
    }

    fn render_inline_blame_entry(
        &self,
        style: &TextStyle,
        blame_entry: BlameEntry,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let text = format_blame_text(
            &blame_entry,
            ProjectSettings::get_global(cx).git.inline_blame.date_style,
            cx,
        );

        Some(
            h_flex()
                .id("inline-blame")
                .w_full()
                .font(style.font())
                .text_color(cx.theme().status().hint)
                .line_height(style.line_height)
                .child(Icon::new(IconName::FileGit).color(Color::Hint))
                .child(text)
                .gap_2()
                .into_any(),
        )
    }

    fn render_blame_entry_popover(
        &self,
        _blame: BlameEntry,
        _scroll_handle: ScrollHandle,
        _details: Option<ParsedCommitMessage>,
        _tag_names: Vec<SharedString>,
        _markdown: Entity<Markdown>,
        _repository: Entity<Repository>,
        _workspace: WeakEntity<Workspace>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<AnyElement> {
        None
    }

    fn open_blame_commit(
        &self,
        _blame_entry: BlameEntry,
        _repository: Entity<Repository>,
        _workspace: WeakEntity<Workspace>,
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }
}

fn format_git_date(timestamp: OffsetDateTime, style: GitDateStyle, cx: &App) -> String {
    let format = match style {
        GitDateStyle::Relative => TimestampFormat::Relative,
        GitDateStyle::Absolute => TimestampFormat::EnhancedAbsolute,
    };
    format_timestamp(
        timestamp,
        OffsetDateTime::now_utc(),
        UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC),
        format,
        &ProjectSettings::get_global(cx).git.date_format,
    )
}

fn format_timestamp(
    timestamp: OffsetDateTime,
    reference: OffsetDateTime,
    offset: UtcOffset,
    format: TimestampFormat,
    date_format: &GitDateFormat,
) -> String {
    if !matches!(format, TimestampFormat::Relative)
        && let Some(timestamp) = date_format.format(timestamp.to_offset(offset))
    {
        return timestamp;
    }
    time_format::format_localized_timestamp(timestamp, reference, offset, format)
}

#[cfg(test)]
mod tests {
    use super::format_timestamp;
    use project::project_settings::GitDateFormat;
    use time::UtcOffset;
    use time::macros::{datetime, offset};
    use time_format::TimestampFormat;

    #[test]
    fn absolute_formats_use_local_time() {
        let timestamp = datetime!(2024-06-08 23:45:12 UTC);
        for (format, expected) in [
            ("iso", "2024-06-09 08:45"),
            ("%Y-%m-%d %H:%M:%S %z", "2024-06-09 08:45:12 +0900"),
            ("%a %b %d %%", "Sun Jun 09 %"),
        ] {
            for fallback in [
                TimestampFormat::Absolute,
                TimestampFormat::EnhancedAbsolute,
                TimestampFormat::MediumAbsolute,
            ] {
                assert_eq!(
                    format_timestamp(
                        timestamp,
                        timestamp,
                        offset!(+9),
                        fallback,
                        &GitDateFormat::parse(format)
                    ),
                    expected
                );
            }
        }
    }

    #[test]
    fn relative_dates_ignore_absolute_format() {
        let timestamp = datetime!(2024-06-08 23:45:12 UTC);
        let reference = timestamp + time::Duration::days(3);
        let expected = time_format::format_localized_timestamp(
            timestamp,
            reference,
            UtcOffset::UTC,
            TimestampFormat::Relative,
        );
        for format in ["locale", "iso", "%Y", "", "%Q"] {
            assert_eq!(
                format_timestamp(
                    timestamp,
                    reference,
                    UtcOffset::UTC,
                    TimestampFormat::Relative,
                    &GitDateFormat::parse(format)
                ),
                expected
            );
        }
    }

    #[test]
    fn locale_and_invalid_formats_preserve_surface_defaults() {
        let timestamp = datetime!(2024-06-08 23:45:12 UTC);
        let reference = timestamp + time::Duration::days(3);
        for format in ["locale", "", " ", "%Q", "%"] {
            assert!(matches!(
                GitDateFormat::parse(format),
                GitDateFormat::Locale
            ));
            for fallback in [
                TimestampFormat::Absolute,
                TimestampFormat::EnhancedAbsolute,
                TimestampFormat::MediumAbsolute,
            ] {
                assert_eq!(
                    format_timestamp(
                        timestamp,
                        reference,
                        UtcOffset::UTC,
                        fallback,
                        &GitDateFormat::parse(format)
                    ),
                    time_format::format_localized_timestamp(
                        timestamp,
                        reference,
                        UtcOffset::UTC,
                        fallback
                    )
                );
            }
        }
    }
}
