//! Commit-graph lane layout (docs/specs/M05 §2). Pure: commits in display order in, drawing instructions out.
//!
//! Each lane remembers the commit it is waiting for. A commit lands in the first lane waiting for it
//! (or a free one), other lanes waiting for it merge into it, its first parent continues straight down,
//! and extra parents (merges) take free lanes. A lane keeps its colour for as long as it lives.
//!
//! As in IntelliJ, a line longer than [`LONG_EDGE`] rows is cut: it shows [`EDGE_STUB`] row at
//! each end, ending in an arrow, and its lane is free in between, so the graph stays narrow.

use std::collections::HashMap;

/// Lines spanning more rows than this are cut (IntelliJ's long edges).
pub const LONG_EDGE: usize = 30;
/// Rows a cut line still shows at each end.
pub const EDGE_STUB: usize = 1;

/// Vertical anchor inside a row: top edge, the node's centre line, or bottom edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pos {
    Top,
    Center,
    Bottom,
}

/// A line to draw inside one row, from `(lane, pos)` to `(lane, pos)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub from: (usize, Pos),
    pub to: (usize, Pos),
    pub color: usize,
}

/// Where a cut line stops: at the bottom of the row, pointing down to the rest of it, or at the
/// top, pointing up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arrow {
    pub lane: usize,
    pub pos: Pos,
    pub color: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Lane of this commit's node.
    pub node: usize,
    /// Colour index of the node (an index into the UI palette, taken modulo its length).
    pub color: usize,
    pub edges: Vec<Edge>,
    pub arrows: Vec<Arrow>,
    /// Number of lanes this row needs to draw.
    pub width: usize,
}

#[derive(Clone)]
struct Lane<'a> {
    waits_for: &'a str,
    color: usize,
    /// A long line: the row whose bottom it stops at, its lane freed after.
    cut_after: Option<usize>,
    /// A long line's lower end, reopened in this row: an arrow at the top.
    reopened: bool,
}

/// Lays out commits given newest-first in display order as `(id, parent ids)`.
pub fn layout<'a, P>(commits: impl IntoIterator<Item = (&'a str, P)>) -> Vec<Row>
where
    P: IntoIterator<Item = &'a str>,
{
    layout_with(commits, false)
}

/// [`layout`], drawing long lines whole when `long_edges` (IntelliJ's View Options ▸ Long Edges).
pub fn layout_with<'a, P>(
    commits: impl IntoIterator<Item = (&'a str, P)>,
    long_edges: bool,
) -> Vec<Row>
where
    P: IntoIterator<Item = &'a str>,
{
    let long_edge = if long_edges { usize::MAX } else { LONG_EDGE };
    let commits: Vec<(&'a str, Vec<&'a str>)> = commits
        .into_iter()
        .map(|(id, parents)| (id, parents.into_iter().collect()))
        .collect();
    let row_of: HashMap<&str, usize> = commits
        .iter()
        .enumerate()
        .map(|(r, (id, _))| (*id, r))
        .collect();
    let total = commits.len();
    // the lower ends of cut lines: (row, parent it waits for, colour)
    let mut reopen: HashMap<usize, Vec<(&'a str, usize)>> = HashMap::new();
    // a line from row `r` to `parent`: cut when long, its lower end scheduled
    let open = |r: usize,
                parent: &'a str,
                color: usize,
                reopen: &mut HashMap<usize, Vec<(&'a str, usize)>>| {
        let end = row_of.get(parent).copied();
        let cut_after = match end {
            Some(e) if e - r > long_edge => {
                reopen
                    .entry(e - EDGE_STUB)
                    .or_default()
                    .push((parent, color));
                Some(r + EDGE_STUB)
            }
            // the parent isn't loaded: the line runs off the end of the list
            None if total - r > long_edge => Some(r + EDGE_STUB),
            _ => None,
        };
        Lane {
            waits_for: parent,
            color,
            cut_after,
            reopened: false,
        }
    };
    let mut lanes: Vec<Option<Lane<'a>>> = Vec::new();
    let mut next_color = 0;
    let mut new_color = || {
        next_color += 1;
        next_color - 1
    };
    let mut rows = Vec::new();

    for (r, (id, parents)) in commits.into_iter().enumerate() {
        // long lines whose lower end shows from here: back in a free lane, unless one already
        // waits for that parent
        for (parent, color) in reopen.remove(&r).unwrap_or_default() {
            if lanes.iter().flatten().any(|l| l.waits_for == parent) {
                continue;
            }
            let lane = Lane {
                waits_for: parent,
                color,
                cut_after: None,
                reopened: true,
            };
            match lanes.iter().position(Option::is_none) {
                Some(i) => lanes[i] = Some(lane),
                None => lanes.push(Some(lane)),
            }
        }
        let before = lanes.len();
        let waiting = lanes.iter().enumerate().find_map(|(i, lane)| {
            lane.as_ref()
                .filter(|lane| lane.waits_for == id)
                .map(|lane| (i, lane.color))
        });

        let (node, color) = match waiting {
            Some(found) => found,
            None => {
                let i = lanes
                    .iter()
                    .position(Option::is_none)
                    .unwrap_or(lanes.len());
                if i == lanes.len() {
                    lanes.push(None);
                }
                (i, new_color())
            }
        };

        let mut edges = Vec::new();
        let mut arrows = Vec::new();
        for (i, lane) in lanes.iter_mut().enumerate() {
            let Some(l) = lane else { continue };
            if l.reopened {
                arrows.push(Arrow {
                    lane: i,
                    pos: Pos::Top,
                    color: l.color,
                });
                l.reopened = false;
            }
            if l.waits_for == id {
                // this lane ends here (it may be the node's own lane, or a branch merging into it)
                edges.push(Edge {
                    from: (i, Pos::Top),
                    to: (node, Pos::Center),
                    color: l.color,
                });
                *lane = None;
            } else {
                edges.push(Edge {
                    from: (i, Pos::Top),
                    to: (i, Pos::Bottom),
                    color: l.color,
                });
            }
        }

        if let Some((first, rest)) = parents.split_first() {
            lanes[node] = Some(open(r, first, color, &mut reopen));
            edges.push(Edge {
                from: (node, Pos::Center),
                to: (node, Pos::Bottom),
                color,
            });
            for p in rest {
                let target = lanes.iter().enumerate().find_map(|(j, lane)| {
                    lane.as_ref()
                        .filter(|lane| lane.waits_for == *p)
                        .map(|lane| (j, lane.color))
                });
                let (j, c) = match target {
                    Some(found) => found,
                    None => {
                        let j = lanes
                            .iter()
                            .position(Option::is_none)
                            .unwrap_or(lanes.len());
                        let c = new_color();
                        if j == lanes.len() {
                            lanes.push(None);
                        }
                        lanes[j] = Some(open(r, p, c, &mut reopen));
                        (j, c)
                    }
                };
                edges.push(Edge {
                    from: (node, Pos::Center),
                    to: (j, Pos::Bottom),
                    color: c,
                });
            }
        }

        let width = before.max(lanes.len()).max(node + 1);
        // long lines stop at this row's bottom, in an arrow; their lanes are free again
        for (i, lane) in lanes.iter_mut().enumerate() {
            if let Some(l) = lane
                && l.cut_after == Some(r)
            {
                arrows.push(Arrow {
                    lane: i,
                    pos: Pos::Bottom,
                    color: l.color,
                });
                *lane = None;
            }
        }
        while matches!(lanes.last(), Some(None)) {
            lanes.pop();
        }
        rows.push(Row {
            node,
            color,
            edges,
            arrows,
            width,
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use Pos::*;

    fn run(commits: &[(&'static str, &[&'static str])]) -> Vec<Row> {
        layout(commits.iter().map(|(id, ps)| (*id, ps.iter().copied())))
    }

    fn e(from: (usize, Pos), to: (usize, Pos), color: usize) -> Edge {
        Edge { from, to, color }
    }

    #[test]
    fn linear_history_is_one_straight_lane() {
        let rows = run(&[("c", &["b"]), ("b", &["a"]), ("a", &[])]);
        assert!(
            rows.iter()
                .all(|r| r.node == 0 && r.width == 1 && r.color == 0)
        );
        assert_eq!(rows[0].edges, vec![e((0, Center), (0, Bottom), 0)]);
        assert_eq!(
            rows[1].edges,
            vec![e((0, Top), (0, Center), 0), e((0, Center), (0, Bottom), 0)]
        );
        assert_eq!(rows[2].edges, vec![e((0, Top), (0, Center), 0)]); // root: nothing below
    }

    #[test]
    fn merge_opens_a_lane_that_closes_at_the_fork_point() {
        // m merges feature f into main; both come from base b
        let rows = run(&[("m", &["b", "f"]), ("f", &["b"]), ("b", &[])]);
        assert_eq!((rows[0].node, rows[0].width), (0, 2));
        assert_eq!(
            rows[0].edges,
            vec![
                e((0, Center), (0, Bottom), 0),
                e((0, Center), (1, Bottom), 1)
            ]
        );
        assert_eq!((rows[1].node, rows[1].color), (1, 1)); // f sits in the merge's lane, keeps its colour
        assert_eq!(
            rows[1].edges,
            vec![
                e((0, Top), (0, Bottom), 0),
                e((1, Top), (1, Center), 1),
                e((1, Center), (1, Bottom), 1)
            ]
        );
        // both lanes wait for b: they converge into one node and the graph narrows back
        assert_eq!(rows[2].node, 0);
        assert_eq!(
            rows[2].edges,
            vec![e((0, Top), (0, Center), 0), e((1, Top), (0, Center), 1)]
        );
        assert_eq!(rows[2].width, 2);
    }

    #[test]
    fn unrelated_tips_get_their_own_lanes_and_colours() {
        let rows = run(&[("a", &["base"]), ("b", &["base"]), ("base", &[])]);
        assert_eq!((rows[0].node, rows[0].color), (0, 0));
        assert_eq!((rows[1].node, rows[1].color), (1, 1));
        assert_eq!(rows[1].edges[0], e((0, Top), (0, Bottom), 0)); // a's lane passes by
        assert_eq!(rows[2].node, 0);
    }

    #[test]
    fn octopus_merge_takes_one_lane_per_extra_parent() {
        let rows = run(&[
            ("o", &["p1", "p2", "p3"]),
            ("p3", &[]),
            ("p2", &[]),
            ("p1", &[]),
        ]);
        assert_eq!(rows[0].width, 3);
        assert_eq!(rows[0].edges.len(), 3);
        assert_eq!((rows[1].node, rows[2].node, rows[3].node), (2, 1, 0));
    }

    #[test]
    fn freed_lane_is_reused() {
        // x ends (root) in lane 1, so the next new tip y reuses lane 1 instead of growing to lane 2
        let rows = run(&[("a", &["b"]), ("x", &[]), ("y", &["b"]), ("b", &[])]);
        assert_eq!(rows[1].node, 1);
        assert_eq!(rows[2].node, 1);
        assert_eq!(rows[2].width, 2);
    }

    #[test]
    fn parent_outside_the_loaded_range_keeps_its_lane_open() {
        let rows = run(&[("a", &["not-loaded"])]);
        assert_eq!(rows[0].edges, vec![e((0, Center), (0, Bottom), 0)]);
    }

    #[test]
    fn long_lines_are_cut_and_their_lane_reused() {
        // a merge whose branch forks far below: its line shows one row, an arrow, then nothing
        // until one row above the fork point, where it comes back with an arrow
        let mut commits: Vec<(String, Vec<String>)> =
            vec![("m".into(), vec!["0".into(), "f".into()])];
        for i in 0..40 {
            commits.push((i.to_string(), vec![(i + 1).to_string()]));
        }
        commits.push(("40".into(), vec!["base".into()]));
        commits.push(("f".into(), vec!["base".into()]));
        commits.push(("base".into(), vec![]));
        let rows = layout(
            commits
                .iter()
                .map(|(id, ps)| (id.as_str(), ps.iter().map(String::as_str))),
        );
        assert_eq!(
            rows[1].arrows,
            vec![Arrow {
                lane: 1,
                pos: Bottom,
                color: 1
            }]
        );
        // in between, one lane only
        assert!(
            rows[5..38]
                .iter()
                .all(|r| r.width == 1 && r.arrows.is_empty())
        );
        // the merge's line is back one row above f, with an arrow at its top
        let f = rows.len() - 2;
        assert_eq!(
            rows[f - 1].arrows,
            vec![Arrow {
                lane: 1,
                pos: Top,
                color: 1
            }]
        );
        assert_eq!(rows[f].node, 1);
        // Long Edges on: the same line runs whole, no arrows
        let whole = layout_with(
            commits
                .iter()
                .map(|(id, ps)| (id.as_str(), ps.iter().map(String::as_str))),
            true,
        );
        assert!(whole.iter().all(|r| r.arrows.is_empty()));
        assert!(whole[5..38].iter().all(|r| r.width == 2));
    }

    #[test]
    fn large_linear_history_lays_out_quickly() {
        let ids: Vec<String> = (0..200_000).map(|i| i.to_string()).collect();
        let start = std::time::Instant::now();
        let rows = layout(
            ids.iter()
                .enumerate()
                .map(|(i, id)| (id.as_str(), ids.get(i + 1).map(String::as_str))),
        );
        assert_eq!(rows.len(), 200_000);
        assert!(
            start.elapsed().as_secs() < 2,
            "layout too slow: {:?}",
            start.elapsed()
        );
    }
}
