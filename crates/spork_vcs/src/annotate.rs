//! IntelliJ's annotations (Annotate with Git Blame) in the editor's gutter: a date and an author
//! column (and the revision, from View ▸), the lines of the file's last commit brighter and bold,
//! each revision's rows shaded by how recent it is (IntelliJ's default Colors ▸ Order), a tooltip
//! with the commit, and the annotations' own menu. The blame is Zed's (`editor::GitBlame`, git
//! blame of the buffer as it is, unsaved edits included), the editor drawing it through the
//! renderer the app sets (`zed::git_blame`), which hands the gutter's part to this module.

use std::collections::HashMap;
use std::rc::Rc;

use editor::display_map::DisplayRow;
use editor::{AnnotationMenu, Editor};
use git::Oid;
use git::blame::BlameEntry;
use git::commit::ParsedCommitMessage;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, FontWeight, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render,
    StatefulInteractiveElement as _, Styled as _, TextStyle, Window, div, px,
};
use multi_buffer::MultiBufferRow;
use settings::Settings as _;
use theme_settings::ThemeSettings;
use ui::{ContextMenu, h_flex, v_flex};

/// The longest author the column makes room for, in characters.
pub const MAX_AUTHOR: usize = 20;
/// The date column's characters: `dd/mm/yyyy`, wider than Today and Yesterday.
const DATE_CHARS: usize = 10;
/// The gap before, between and after the columns.
const GAP: Pixels = px(8.);

/// How the author's name shows (IntelliJ's View ▸ Names).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Names {
    Initials,
    FirstName,
    LastName,
    FullName,
    Email,
}

/// The columns shown and the names' form, IntelliJ's defaults (Revision hidden, Last Name), kept
/// for the session as IntelliJ keeps them for the app.
// shortcut: not saved across launches; persist them with the other VCS settings when Settings ›
// Version Control comes to the IDE
#[derive(Clone, Copy, Debug)]
struct Options {
    revision: bool,
    date: bool,
    author: bool,
    names: Names,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            revision: false,
            date: true,
            author: true,
            names: Names::LastName,
        }
    }
}

thread_local! {
    static OPTIONS: std::cell::Cell<Options> = std::cell::Cell::new(Options::default());
}

fn options() -> Options {
    OPTIONS.get()
}

/// Changes the options and draws the annotations again.
fn set_options(change: impl FnOnce(&mut Options), cx: &mut App) {
    let mut options = OPTIONS.get();
    change(&mut options);
    OPTIONS.set(options);
    cx.refresh_windows();
}

/// Of the file being drawn: its newest revision and each revision's shade, by its order among the
/// file's revisions, the newest first.
struct Revisions {
    newest: Option<Oid>,
    shades: HashMap<Oid, Hsla>,
}

thread_local! {
    /// The annotations' menu while it shows: their tooltips wait until it closes.
    static MENU: std::cell::RefCell<Option<gpui::WeakEntity<ContextMenu>>> =
        std::cell::RefCell::default();
    /// Each annotated editor's [`Revisions`], worked out when its first visible row is drawn.
    // shortcut: walks every row's blame once a frame; keep it on the blame when big files show
    // it in profiles
    static REVISIONS: std::cell::RefCell<HashMap<gpui::EntityId, Rc<Revisions>>> =
        std::cell::RefCell::default();
}

pub(crate) fn init(cx: &mut App) {
    cx.set_global(AnnotationMenu(Rc::new(|editor, row, window, cx| {
        menu(editor, row, window, cx)
    })));
}

/// The characters the author column makes room for: none while it is hidden.
pub fn author_chars() -> usize {
    match options().author {
        true => MAX_AUTHOR,
        false => 0,
    }
}

/// The characters of the date column: none while it is hidden.
pub fn date_chars() -> usize {
    match options().date {
        true => DATE_CHARS,
        false => 0,
    }
}

/// The annotations' width besides their text: the gaps, less the revision's characters the
/// editor counts while the Revision column is hidden.
pub fn non_text_width(window: &Window, cx: &App) -> Pixels {
    let options = options();
    let columns = [options.revision, options.date, options.author]
        .into_iter()
        .filter(|shown| *shown)
        .count();
    let gaps = GAP * (columns + 1) as f32;
    if options.revision {
        return gaps;
    }
    let settings = ThemeSettings::get_global(cx);
    let font_id = window.text_system().resolve_font(&settings.buffer_font);
    let advance = window
        .text_system()
        .ch_advance(font_id, settings.buffer_font_size(cx))
        .unwrap_or(px(0.));
    gaps - advance * git::SHORT_SHA_LENGTH as f32
}

/// One row of the annotations: its columns on its revision's shade, the commit as its tooltip.
pub fn render_entry(
    style: &TextStyle,
    entry: BlameEntry,
    details: Option<ParsedCommitMessage>,
    editor: Entity<Editor>,
    ix: usize,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let options = options();
    let revisions = revisions(&editor, ix == 0, cx);
    let tokens = spork_workbench::Tokens::get(cx);
    let newest = revisions.newest == Some(entry.sha);
    let font_id = window.text_system().resolve_font(&style.font());
    let advance = window
        .text_system()
        .ch_advance(font_id, style.font_size.to_pixels(window.rem_size()))
        .unwrap_or(px(0.));
    let date = entry
        .author_time
        .map(|time| pretty_date(time, chrono::Local::now().timestamp()))
        .unwrap_or_default();
    let author = name(&entry, options.names);
    let tooltip = tooltip_text(&entry, details.as_ref());
    let menu_open =
        MENU.with_borrow(|menu| menu.as_ref().is_some_and(|menu| menu.upgrade().is_some()));
    // the columns line up from row to row: each as wide as its longest text can be
    let column = |text: String, chars: usize| {
        div()
            .flex_none()
            .w(advance * chars as f32)
            .whitespace_nowrap()
            .overflow_hidden()
            .child(text)
    };
    Some(
        h_flex()
            .id(("annotation", ix))
            .w_full()
            .pl(GAP)
            .gap(GAP)
            .font(style.font())
            .line_height(style.line_height)
            .text_color(match newest {
                true => tokens.annotation_last_commit,
                false => tokens.annotation,
            })
            .when(newest, |row| row.font_weight(FontWeight::BOLD))
            .when_some(revisions.shades.get(&entry.sha), |row, shade| {
                row.bg(*shade)
            })
            .when(options.revision, |row| {
                row.child(column(entry.sha.display_short(), git::SHORT_SHA_LENGTH))
            })
            .when(options.date, |row| row.child(column(date, DATE_CHARS)))
            .when(options.author, |row| {
                row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .child(author),
                )
            })
            .when(!menu_open, |row| {
                row.tooltip(move |_, cx| cx.new(|_| Tooltip(tooltip.clone())).into())
            })
            .into_any_element(),
    )
}

/// The revisions of the file `editor` shows, worked out again on its first visible row.
fn revisions(editor: &Entity<Editor>, fresh: bool, cx: &mut App) -> Rc<Revisions> {
    let id = editor.entity_id();
    if !fresh && let Some(found) = REVISIONS.with_borrow(|cache| cache.get(&id).cloned()) {
        return found;
    }
    let rows: Vec<_> = editor
        .read(cx)
        .buffer()
        .read(cx)
        .snapshot(cx)
        .row_infos(MultiBufferRow(0))
        .collect();
    let mut times: HashMap<Oid, i64> = HashMap::new();
    if let Some(blame) = editor.read(cx).blame().cloned() {
        blame.update(cx, |blame, cx| {
            for (_, entry) in blame.blame_for_rows(&rows, cx).flatten() {
                times.insert(entry.sha, entry.committer_time.unwrap_or_default());
            }
        });
    }
    let mut order: Vec<(Oid, i64)> = times.into_iter().collect();
    order.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.to_string().cmp(&b.0.to_string()))
    });
    let palette = order_palette(spork_workbench::Tokens::get(cx).annotation_order);
    // IntelliJ shades nothing when every line comes from one revision
    let shades = match order.len() > 1 {
        true => order
            .iter()
            .enumerate()
            .map(|(ix, (sha, _))| (*sha, palette[palette.len() * ix / order.len()]))
            .collect(),
        false => HashMap::new(),
    };
    let revisions = Rc::new(Revisions {
        newest: order.first().map(|(sha, _)| *sha),
        shades,
    });
    REVISIONS.with_borrow_mut(|cache| cache.insert(id, revisions.clone()));
    revisions
}

/// IntelliJ's ordered colors: its five anchors with four steps between each two.
fn order_palette(anchors: [Hsla; 5]) -> Vec<Hsla> {
    const BETWEEN: usize = 4;
    let mut palette = Vec::with_capacity(anchors.len() + BETWEEN * (anchors.len() - 1));
    for pair in anchors.windows(2) {
        let (from, to) = (gpui::Rgba::from(pair[0]), gpui::Rgba::from(pair[1]));
        palette.push(pair[0]);
        for step in 1..=BETWEEN {
            let t = step as f32 / (BETWEEN + 1) as f32;
            let mix = |a: f32, b: f32| a + (b - a) * t;
            palette.push(
                gpui::Rgba {
                    r: mix(from.r, to.r),
                    g: mix(from.g, to.g),
                    b: mix(from.b, to.b),
                    a: 1.,
                }
                .into(),
            );
        }
    }
    palette.push(anchors[anchors.len() - 1]);
    palette
}

/// IntelliJ's pretty date: Today, Yesterday, else the date.
fn pretty_date(then: i64, now: i64) -> String {
    use chrono::{Local, TimeZone as _};
    let (Some(at), Some(today)) = (
        Local.timestamp_opt(then, 0).single(),
        Local.timestamp_opt(now, 0).single(),
    ) else {
        return String::new();
    };
    match (today.date_naive() - at.date_naive()).num_days() {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        _ => at.format("%d/%m/%Y").to_string(),
    }
}

/// The author as View ▸ Names has it (IntelliJ's `ShortNameType`).
fn name(entry: &BlameEntry, names: Names) -> String {
    let email = entry.author_mail.as_deref().unwrap_or_default();
    short_name(entry.author.as_deref().unwrap_or_default(), email, names)
}

fn short_name(full: &str, email: &str, names: Names) -> String {
    let parts: Vec<&str> = full.split_whitespace().collect();
    match names {
        Names::FullName => full.to_owned(),
        Names::Email => email.trim_matches(['<', '>']).to_owned(),
        Names::FirstName => parts.first().copied().unwrap_or_default().to_owned(),
        Names::LastName => parts.last().copied().unwrap_or_default().to_owned(),
        Names::Initials => parts
            .iter()
            .filter_map(|part| part.chars().next())
            .flat_map(char::to_uppercase)
            .collect(),
    }
}

/// IntelliJ's annotation tooltip: the commit, its author and date, then its message.
fn tooltip_text(entry: &BlameEntry, details: Option<&ParsedCommitMessage>) -> String {
    let author = match (&entry.author, &entry.author_mail) {
        (Some(name), Some(mail)) => format!("{name} {mail}"),
        (Some(name), None) => name.clone(),
        (None, mail) => mail.clone().unwrap_or_default(),
    };
    let date = entry
        .author_time
        .and_then(|time| {
            use chrono::TimeZone as _;
            chrono::Local.timestamp_opt(time, 0).single()
        })
        .map(|at| at.format("%d/%m/%Y, %H:%M").to_string())
        .unwrap_or_default();
    let message = match details {
        Some(details) => details.message.trim_end().to_owned(),
        None => format!("{}\n...", entry.summary.clone().unwrap_or_default()),
    };
    format!(
        "commit {}\nAuthor: {author}\nDate: {date}\n\n{message}",
        entry.sha
    )
}

/// The annotation tooltip, IntelliJ's text in Zed's tooltip.
struct Tooltip(String);

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text = self.0.clone();
        ui::tooltip_container(cx, move |container, _| {
            container.child(
                v_flex().children(
                    text.lines()
                        .map(|line| div().min_h(px(14.)).child(line.to_owned())),
                ),
            )
        })
    }
}

/// The annotation's row clicked: its blame entry, for the menu.
fn entry_at(editor: &mut Editor, row: DisplayRow, cx: &mut Context<Editor>) -> Option<BlameEntry> {
    let display = editor.display_snapshot(cx);
    let rows: Vec<_> = display.row_infos(row).take(1).collect();
    let blame = editor.blame()?.clone();
    blame.update(cx, |blame, cx| {
        blame
            .blame_for_rows(&rows, cx)
            .next()
            .flatten()
            .map(|(_, entry)| entry)
    })
}

/// IntelliJ's annotation popup: Close Annotations, View ▸, Show Diff, Copy Revision Number.
fn menu(
    editor: &mut Editor,
    row: DisplayRow,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> Option<Entity<ContextMenu>> {
    let entry = entry_at(editor, row, cx)?;
    let weak_editor = cx.entity().downgrade();
    let path = editor
        .buffer()
        .read(cx)
        .as_singleton()
        .and_then(|buffer| buffer.read(cx).file().cloned())
        .and_then(|file| file.as_local().map(|file| file.abs_path(cx)));
    let options = options();
    let sha = entry.sha.to_string();
    let menu = ContextMenu::build(window, cx, move |menu, _, _| {
        let set =
            |change: fn(&mut Options)| move |_: &mut Window, cx: &mut App| set_options(change, cx);
        let current = options.names;
        let names = move |label: &'static str, kind: Names| {
            let checked = current == kind;
            (label, checked, move |_: &mut Window, cx: &mut App| {
                set_options(|options| options.names = kind, cx)
            })
        };
        let show_diff = {
            let (sha, path, editor) = (sha.clone(), path.clone(), weak_editor.clone());
            move |window: &mut Window, cx: &mut App| {
                let (Some(path), Some(editor)) = (path.clone(), editor.upgrade()) else {
                    return;
                };
                crate::revision_tabs::show_commit_file(&editor, &sha, &path, window, cx);
            }
        };
        menu.entry("Close Annotations", None, move |window, cx| {
            weak_editor
                .update(cx, |editor, cx| {
                    editor.toggle_git_blame(&git::Blame, window, cx)
                })
                .ok();
        })
        .submenu("View", move |menu, _, _| {
            let menu = menu
                .toggleable_entry(
                    "Revision",
                    options.revision,
                    ui::IconPosition::Start,
                    None,
                    set(|o| o.revision = !o.revision),
                )
                .toggleable_entry(
                    "Date",
                    options.date,
                    ui::IconPosition::Start,
                    None,
                    set(|o| o.date = !o.date),
                )
                .toggleable_entry(
                    "Author",
                    options.author,
                    ui::IconPosition::Start,
                    None,
                    set(|o| o.author = !o.author),
                )
                .separator();
            menu.submenu("Names", move |menu, _, _| {
                [
                    names("Initials", Names::Initials),
                    names("First Name", Names::FirstName),
                    names("Last Name", Names::LastName),
                    names("Full Name", Names::FullName),
                    names("Email", Names::Email),
                ]
                .into_iter()
                .fold(menu, |menu, (label, checked, handler)| {
                    menu.toggleable_entry(label, checked, ui::IconPosition::Start, None, handler)
                })
            })
        })
        .entry("Show Diff", None, show_diff)
        .entry("Copy Revision Number", None, move |_, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(sha.clone()))
        })
    });
    MENU.set(Some(menu.downgrade()));
    Some(menu)
}

#[cfg(test)]
mod tests {
    use super::{Names, order_palette, pretty_date, short_name};
    use gpui::Hsla;

    #[test]
    fn names_as_view_names_has_them() {
        let (full, mail) = ("Ada King Lovelace", "<ada@example.com>");
        assert_eq!(short_name(full, mail, Names::LastName), "Lovelace");
        assert_eq!(short_name(full, mail, Names::FirstName), "Ada");
        assert_eq!(short_name(full, mail, Names::Initials), "AKL");
        assert_eq!(short_name(full, mail, Names::FullName), full);
        assert_eq!(short_name(full, mail, Names::Email), "ada@example.com");
    }

    #[test]
    fn dates_say_today_and_yesterday() {
        let now = chrono::Local::now().timestamp();
        assert_eq!(pretty_date(now, now), "Today");
        assert_eq!(pretty_date(now - 86_400, now), "Yesterday");
        assert_eq!(pretty_date(0, now).len(), "01/01/1970".len());
    }

    #[test]
    fn the_palette_steps_between_its_anchors() {
        let anchors = [0., 0.25, 0.5, 0.75, 1.].map(|l| Hsla {
            h: 0.,
            s: 0.,
            l,
            a: 1.,
        });
        let palette = order_palette(anchors);
        assert_eq!(palette.len(), 21);
        assert_eq!(palette[0], anchors[0]);
        assert_eq!(palette[20], anchors[4]);
    }
}
