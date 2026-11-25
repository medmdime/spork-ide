//! Commit list with graph (docs/specs/M05): one row per commit, graph painted per row from `spork_graph::Row`.

use std::collections::HashMap;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_git::{Commit, Ref, RefKind};
use spork_graph::{Pos, Row};

/// In rems: rows grow with the text size. 26 px, IntelliJ's log rows; lists use the same.
pub const ROW_HEIGHT: Rems = rems(1.75);
/// IntelliJ's graph, at 26 px rows: lanes 18 px apart, 9 px dots, thin lines.
pub const LANE_WIDTH: f32 = 18.;
const NODE_RADIUS: f32 = 4.5;
const LINE_WIDTH: f32 = 1.25;
/// Graph column stops growing after this many lanes (M05 §2 "compress when lanes > 40" comes later).
const MAX_VISIBLE_LANES: usize = 16;

/// Everything the list needs, computed off the UI thread.
pub struct History {
    pub commits: Vec<Commit>,
    pub rows: Vec<Row>,
    /// Commit id → ref badges pointing at it.
    pub labels: HashMap<String, Vec<Label>>,
    pub graph_lanes: usize,
    /// Merges whose branch is folded away (`spork.folded`), with how many commits it hides.
    pub folded: HashMap<String, usize>,
    /// Stash commits drawn in the list, by id.
    pub stashes: HashMap<String, spork_git::Stash>,
    /// Each graph color index's IntelliJ color id: the hash of the best ref at that branch's
    /// head, or the index itself for a branch without one.
    pub color_ids: Vec<i32>,
}

#[derive(Clone)]
pub struct Label {
    pub name: String,
    pub kind: RefKind,
    pub is_head: bool,
}

/// `list` filling its box, with a vertical scroll bar beside it.
pub(crate) fn scrolled(list: UniformList, handle: &UniformListScrollHandle) -> Div {
    div()
        .relative()
        .size_full()
        .child(list.track_scroll(handle).size_full())
        .vertical_scrollbar(handle)
}

/// A commit row being dragged onto another to reorder the branch (M06 §2).
#[derive(Clone)]
pub struct DraggedCommit {
    pub id: String,
    pub subject: String,
}

impl Render for DraggedCommit {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_sm()
            .child(self.subject.clone())
    }
}

/// A label being dragged onto another (M03 §3) shows as itself under the pointer.
impl Render for Label {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let kind = match (self.is_head, self.kind) {
            (true, _) => spork_workbench::RefLabel::Head,
            (_, RefKind::Local) => spork_workbench::RefLabel::Local,
            (_, RefKind::Remote) => spork_workbench::RefLabel::Remote,
            (_, RefKind::Tag) => spork_workbench::RefLabel::Tag,
        };
        label_badge(&[kind], &self.name, cx)
    }
}

/// A commit's labels as IntelliJ shows them: a local branch and its copies on remotes at the
/// same commit make one label, `origin & main`. Each comes with the label it acts as.
pub fn grouped(labels: &[Label]) -> Vec<(&Label, String)> {
    let local = |name: &str| {
        labels.iter().any(|l| {
            l.kind == RefKind::Local && name.split_once('/').is_some_and(|(_, b)| b == l.name)
        })
    };
    labels
        .iter()
        .filter(|l| !(l.kind == RefKind::Remote && local(&l.name)))
        .map(|l| {
            let remotes: Vec<&str> = labels
                .iter()
                .filter(|r| l.kind == RefKind::Local && r.kind == RefKind::Remote)
                .filter_map(|r| r.name.split_once('/').filter(|(_, b)| *b == l.name))
                .map(|(remote, _)| remote)
                .collect();
            let text = match remotes.is_empty() {
                true => l.name.clone(),
                false => format!("{} & {}", remotes.join(" & "), l.name),
            };
            (l, text)
        })
        .collect()
}

impl History {
    pub fn new(commits: Vec<Commit>, refs: &[Ref]) -> Self {
        let rows = spork_graph::layout(
            commits
                .iter()
                .map(|c| (c.id.as_str(), c.parents.iter().map(String::as_str))),
        );
        let mut labels: HashMap<String, Vec<Label>> = HashMap::new();
        for r in refs {
            labels.entry(r.commit.clone()).or_default().push(Label {
                name: r.name.clone(),
                kind: r.kind,
                is_head: r.is_head,
            });
        }
        // current branch first, then local, remote, tags
        for list in labels.values_mut() {
            list.sort_by_key(|l| (!l.is_head, l.kind as u8));
        }
        let graph_lanes = rows
            .iter()
            .map(|r| r.width)
            .max()
            .unwrap_or(1)
            .min(MAX_VISIBLE_LANES);
        let color_ids = color_ids(&commits, &rows, &labels);
        Self {
            commits,
            rows,
            labels,
            graph_lanes,
            folded: HashMap::new(),
            stashes: HashMap::new(),
            color_ids,
        }
    }

    /// Lays the graph out again, long lines whole or cut (View Options ▸ Long Edges).
    pub fn relayout(&mut self, long_edges: bool) {
        self.rows = spork_graph::layout_with(
            self.commits
                .iter()
                .map(|c| (c.id.as_str(), c.parents.iter().map(String::as_str))),
            long_edges,
        );
        self.graph_lanes = self
            .rows
            .iter()
            .map(|r| r.width)
            .max()
            .unwrap_or(1)
            .min(MAX_VISIBLE_LANES);
        self.color_ids = color_ids(&self.commits, &self.rows, &self.labels);
    }
}

/// Java's `String.hashCode`, which IntelliJ colors branches by.
fn java_hash(s: &str) -> i32 {
    s.encode_utf16()
        .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

/// How much IntelliJ prefers a ref to name (and color) the branch it sits on; lower is better.
fn ref_rank(l: &Label) -> u8 {
    let main = |n: &str| n == "main" || n == "master";
    match l.kind {
        RefKind::Remote
            if l.name
                .split_once('/')
                .is_some_and(|(r, n)| r == "origin" && main(n)) =>
        {
            0
        }
        RefKind::Remote => 1,
        RefKind::Local if l.is_head => 5,
        RefKind::Local if main(&l.name) => 2,
        RefKind::Local => 3,
        RefKind::Tag => 4,
    }
}

/// Each color index's id: going down the log, the first commit drawn in that color with refs
/// names it after its best ref; a color never named keeps its index.
fn color_ids(commits: &[Commit], rows: &[Row], labels: &HashMap<String, Vec<Label>>) -> Vec<i32> {
    let count = rows
        .iter()
        .flat_map(|r| std::iter::once(r.color).chain(r.edges.iter().map(|e| e.color)))
        .max()
        .map_or(0, |m| m + 1);
    let mut ids: Vec<Option<i32>> = vec![None; count];
    for (commit, row) in commits.iter().zip(rows) {
        if ids[row.color].is_some() {
            continue;
        }
        if let Some(best) = labels
            .get(&commit.id)
            .and_then(|l| l.iter().min_by_key(|l| ref_rank(l)))
        {
            ids[row.color] = Some(java_hash(&best.name));
        }
    }
    ids.into_iter()
        .enumerate()
        .map(|(ix, id)| id.unwrap_or(ix as i32))
        .collect()
}

/// IntelliJ's branch color for a color id: a hue from the id, muted (saturation 0.6,
/// brightness 0.6 in dark themes, 0.7 in light ones).
pub fn lane_color(id: i32, dark: bool) -> Hsla {
    let channel = |n: i32| (n % 100).abs() + 70;
    let (r, g, b) = (
        channel(id.wrapping_mul(200).wrapping_add(30)) as f32,
        channel(id.wrapping_mul(130).wrapping_add(50)) as f32,
        channel(id.wrapping_mul(90).wrapping_add(100)) as f32,
    );
    // the hue as java.awt.Color.RGBtoHSB works it out
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let hue = if max == min {
        0.
    } else {
        let (rc, gc, bc) = (
            (max - r) / (max - min),
            (max - g) / (max - min),
            (max - b) / (max - min),
        );
        let h = if r == max {
            bc - gc
        } else if g == max {
            2. + rc - bc
        } else {
            4. + gc - rc
        } / 6.;
        if h < 0. { h + 1. } else { h }
    };
    let value: f32 = if dark { 0.6 } else { 0.7 };
    // HSB to HSL, which gpui takes
    let lightness = value * (1. - 0.6 / 2.);
    let saturation = if lightness == 0. || lightness == 1. {
        0.
    } else {
        (value - lightness) / lightness.min(1. - lightness)
    };
    hsla(hue, saturation, lightness, 1.)
}

/// When a commit was made, as IntelliJ's log says it: "5 minutes ago", "Today 18:56",
/// "Yesterday 18:56", else the date and time.
pub fn date_text(then: i64, now: i64) -> String {
    use chrono::{Local, TimeZone as _};
    let minutes = (now - then).max(0) / 60;
    match minutes {
        0 => return "Moments ago".into(),
        1 => return "A minute ago".into(),
        2..60 => return format!("{minutes} minutes ago"),
        60 => return "1 hour ago".into(),
        _ => {}
    }
    let (Some(at), Some(today)) = (
        Local.timestamp_opt(then, 0).single(),
        Local.timestamp_opt(now, 0).single(),
    ) else {
        return String::new();
    };
    let days = (today.date_naive() - at.date_naive()).num_days();
    match days {
        0 => at.format("Today %H:%M").to_string(),
        1 => at.format("Yesterday %H:%M").to_string(),
        _ => at.format("%d/%m/%Y, %H:%M").to_string(),
    }
}

/// The one label a commit shows in IntelliJ's compact view: the name of its first group
/// (the current branch, else a branch with its remote copies, "origin & main"), shortened, and
/// the colors of every ref on the commit. A commit with only tags shows only their icon.
/// `tag_names`: a commit with only tags names its first one, as IntelliJ's "Tag Names".
pub fn compact_label(
    labels: &[Label],
    tag_names: bool,
) -> Option<(String, Vec<spork_workbench::RefLabel>)> {
    use spork_workbench::RefLabel;
    let groups = grouped(labels);
    let first = groups
        .iter()
        .find(|(l, _)| l.is_head)
        .or_else(|| groups.iter().find(|(l, _)| l.kind != RefKind::Tag))
        .or_else(|| groups.first().filter(|_| tag_names))
        .map(|(_, text)| text.clone());
    let mut kinds: Vec<RefLabel> = Vec::new();
    for l in labels {
        let kind = match (l.is_head, l.kind) {
            (true, _) => RefLabel::Head,
            (_, RefKind::Local) => RefLabel::Local,
            (_, RefKind::Remote) => RefLabel::Remote,
            (_, RefKind::Tag) => RefLabel::Tag,
        };
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    if kinds.is_empty() {
        return None;
    }
    Some((first.map(|t| shorten(&t)).unwrap_or_default(), kinds))
}

/// IntelliJ's long-name rule: past 22 characters, "../" and what follows the first "/", then
/// cut with "…".
fn shorten(name: &str) -> String {
    const MAX: usize = 22;
    if name.chars().count() <= MAX {
        return name.to_owned();
    }
    let name = match name.split_once('/') {
        Some((_, rest)) => format!("../{rest}"),
        None => name.to_owned(),
    };
    match name.chars().count() <= MAX {
        true => name,
        false => name.chars().take(MAX - 1).chain(['…']).collect(),
    }
}

/// The graph cell of one row, drawn as IntelliJ does: thin lines in each branch's color, a
/// solid dot per commit, HEAD's dot ringed.
pub fn graph_cell(row: &Row, ids: &[i32], head: bool, lanes: usize, cx: &App) -> impl IntoElement {
    let row = row.clone();
    let dark = cx.theme().mode.is_dark();
    let background = cx.theme().background;
    let ids = ids.to_vec();
    let color = move |i: usize| lane_color(ids.get(i).copied().unwrap_or(i as i32), dark);
    let cell = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let x = |lane: usize| bounds.origin.x + px(LANE_WIDTH * lane as f32 + LANE_WIDTH / 2.);
            let y = |pos: Pos| match pos {
                Pos::Top => bounds.origin.y,
                Pos::Center => bounds.origin.y + bounds.size.height / 2.,
                Pos::Bottom => bounds.origin.y + bounds.size.height,
            };
            for edge in &row.edges {
                let (a, b) = (
                    point(x(edge.from.0), y(edge.from.1)),
                    point(x(edge.to.0), y(edge.to.1)),
                );
                let mut path = PathBuilder::stroke(px(LINE_WIDTH));
                path.move_to(a);
                if a.x == b.x {
                    path.line_to(b);
                } else {
                    // S-curve with vertical tangents at both ends
                    let mid = (a.y + b.y) / 2.;
                    path.cubic_bezier_to(b, point(a.x, mid), point(b.x, mid));
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, color(edge.color));
                }
            }
            // a cut line's end: IntelliJ's small arrow, pointing to where the line goes on
            for arrow in &row.arrows {
                let (tip, dir) = match arrow.pos {
                    Pos::Bottom => (y(Pos::Bottom) - px(2.), -1.),
                    _ => (y(Pos::Top) + px(2.), 1.),
                };
                let ax = x(arrow.lane);
                let mut path = PathBuilder::stroke(px(LINE_WIDTH));
                path.move_to(point(ax - px(3.5), tip + px(4. * dir)));
                path.line_to(point(ax, tip));
                path.line_to(point(ax + px(3.5), tip + px(4. * dir)));
                if let Ok(path) = path.build() {
                    window.paint_path(path, color(arrow.color));
                }
            }
            let center = point(x(row.node), y(Pos::Center));
            let disc = |r: f32, fill: Hsla, window: &mut Window| {
                let r = px(r);
                let at = Bounds::new(point(center.x - r, center.y - r), size(r * 2., r * 2.));
                window.paint_quad(quad(at, r, fill, px(0.), fill, BorderStyle::default()));
            };
            let own = color(row.color);
            if head {
                // a 13 px disc, ringed in the row's color, with a 5 px dot inside
                disc(6.5, own, window);
                disc(4.5, background, window);
                disc(2.5, own, window);
            } else {
                disc(NODE_RADIUS, own, window);
            }
        },
    )
    .size_full();
    div()
        .w(graph_width(lanes))
        .h_full()
        .flex_none()
        .overflow_hidden()
        .child(cell)
}

/// Width of the graph column; never narrower than its "Graph" header.
pub fn graph_width(lanes: usize) -> Pixels {
    px(LANE_WIDTH * lanes as f32 + 4.)
}

/// A commit's label in the log, as in IntelliJ: a tag icon per ref color, stacked, then the
/// name in gray (no name for tags alone).
pub fn label_badge(kinds: &[spork_workbench::RefLabel], text: &str, cx: &App) -> Div {
    const STEP: f32 = 5.8;
    let icons = kinds.iter().enumerate().map(|(ix, kind)| {
        let icon = match kind {
            spork_workbench::RefLabel::Head => "dvcs/currentBranchLabel",
            _ => "dvcs/branchLabel",
        };
        div().absolute().left(px(STEP * ix as f32)).top_0().child(
            spork_workbench::icon(icon)
                .size(px(14.))
                .text_color(spork_workbench::ref_label_color(*kind, cx)),
        )
    });
    h_flex()
        .flex_none()
        .min_w_0()
        .px_1()
        .gap(px(2.))
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(
            div()
                .relative()
                .flex_none()
                .w(px(14. + STEP * kinds.len().saturating_sub(1) as f32))
                .h(px(14.))
                .children(icons),
        )
        .when(!text.is_empty(), |d| {
            d.child(div().min_w_0().truncate().child(text.to_owned()))
        })
}

/// "now", "5m", "3h", "4d", "6mo", "2y" — compact relative age.
pub fn relative_age(then: i64, now: i64) -> String {
    let s = (now - then).max(0);
    match s {
        0..60 => "now".into(),
        60..3_600 => format!("{}m", s / 60),
        3_600..86_400 => format!("{}h", s / 3_600),
        86_400..2_592_000 => format!("{}d", s / 86_400),
        2_592_000..31_536_000 => format!("{}mo", s / 2_592_000),
        _ => format!("{}y", s / 31_536_000),
    }
}

#[cfg(test)]
mod tests {
    use super::{Label, grouped, java_hash, lane_color, relative_age, shorten};

    fn hex(c: gpui_kit::Hsla) -> String {
        let c = c.to_rgb();
        let b = |v: f32| (v * 255.).round() as u8;
        format!("#{:02X}{:02X}{:02X}", b(c.r), b(c.g), b(c.b))
    }

    #[test]
    fn branch_colors_are_intellijs() {
        let named = |name: &str, dark| hex(lane_color(java_hash(name), dark));
        assert_eq!(named("origin/master", true), "#99783D");
        assert_eq!(named("origin/master", false), "#B38C47");
        assert_eq!(named("origin/main", true), "#4D3D99");
        assert_eq!(named("master", true), "#66993D");
        assert_eq!(named("main", true), "#3D8A99");
        let fragments: Vec<String> = (1..=5).map(|i| hex(lane_color(i, true))).collect();
        assert_eq!(
            fragments,
            ["#3D8A99", "#573D99", "#3D5499", "#3D9982", "#743D99"]
        );
    }

    #[test]
    fn long_label_names_are_shortened() {
        assert_eq!(shorten("origin & main"), "origin & main");
        assert_eq!(
            shorten("refactor/shared-nutrition-calculator"),
            "../shared-nutrition-c…"
        );
        assert_eq!(shorten("claude/kitchen-units"), "claude/kitchen-units");
    }
    use spork_git::RefKind;

    #[test]
    fn local_and_remote_labels_merge() {
        let label = |name: &str, kind| Label {
            name: name.into(),
            kind,
            is_head: false,
        };
        let labels = [
            label("main", RefKind::Local),
            label("origin/main", RefKind::Remote),
            label("upstream/main", RefKind::Remote),
            label("origin/feature", RefKind::Remote),
            label("v1.0", RefKind::Tag),
        ];
        let texts: Vec<String> = grouped(&labels).into_iter().map(|(_, t)| t).collect();
        assert_eq!(
            texts,
            ["origin & upstream & main", "origin/feature", "v1.0"]
        );
    }

    #[test]
    fn ages_are_compact() {
        let now = 1_000_000_000;
        assert_eq!(relative_age(now, now), "now");
        assert_eq!(relative_age(now - 120, now), "2m");
        assert_eq!(relative_age(now - 3 * 3_600, now), "3h");
        assert_eq!(relative_age(now - 4 * 86_400, now), "4d");
        assert_eq!(relative_age(now - 65 * 86_400, now), "2mo");
        assert_eq!(relative_age(now - 800 * 86_400, now), "2y");
        assert_eq!(relative_age(now + 50, now), "now"); // clock skew
    }
}
