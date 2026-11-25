//! Repository ▸ Statistics: commits per month and who wrote what (docs/specs/M16 §3).
// shortcut: the current branch, all time; the date-range picker and treemap come later.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::chart::BarChart;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;
use spork_git::stats::Stats;

use crate::RepoView;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const TOP_AUTHORS: usize = 20;

/// `(year, month 1-12)` of a Unix time, UTC (days-to-civil, H. Hinnant).
fn year_month(secs: i64) -> (i64, u32) {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month)
}

/// Commits in each of the 12 months up to and including the one of `now`, oldest first.
fn last_year(times: &[i64], now: i64) -> Vec<(String, usize)> {
    let (y, m) = year_month(now);
    let index = |(year, month): (i64, u32)| year * 12 + i64::from(month) - 1;
    let end = index((y, m));
    let mut counts = [0usize; 12];
    for &t in times {
        let i = end - index(year_month(t));
        if (0..12).contains(&i) {
            counts[11 - i as usize] += 1;
        }
    }
    (0..12)
        .map(|k| {
            let ix = end - 11 + k;
            (
                MONTHS[ix.rem_euclid(12) as usize].to_owned(),
                counts[k as usize],
            )
        })
        .collect()
}

impl RepoView {
    /// Repository ▸ Details: where it is, where it's hosted, how big it is (M02 §3).
    pub(crate) fn details_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let (root, remotes) = (repo.root.clone(), repo.remotes.clone());
        let web = self.project_web_url();
        self.run_op(
            window,
            cx,
            spork_git::stats::details,
            move |_, d, window, cx| {
                let muted = cx.theme().muted_foreground;
                window.open_dialog(cx, move |dialog, _, _| {
                    let row = |label: &str, value: String| {
                        h_flex()
                            .gap_3()
                            .text_sm()
                            .child(
                                div()
                                    .w(px(120.))
                                    .flex_none()
                                    .text_color(muted)
                                    .child(label.to_owned()),
                            )
                            .child(div().flex_1().min_w_0().truncate().child(value))
                    };
                    let name = root
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    dialog.title(format!("{name} details")).w(px(560.)).child(
                        v_flex()
                            .gap_1p5()
                            .child(row("Folder", root.display().to_string()))
                            .children(
                                remotes
                                    .iter()
                                    .map(|r| row(&format!("Remote {}", r.name), r.url.clone())),
                            )
                            .child(row("Commits", d.commits.to_string()))
                            .child(row("Branches", d.branches.to_string()))
                            .child(row("Tags", d.tags.to_string()))
                            .child(row("Size on disk", human_size(d.size)))
                            .child(row(
                                "First commit",
                                d.first_commit.clone().unwrap_or_else(|| "none yet".into()),
                            ))
                            .children(web.clone().map(|(url, service)| {
                                Button::new("open-web")
                                    .ghost()
                                    .small()
                                    .label(format!("Open on {service}"))
                                    .on_click(move |_, _, cx| cx.open_url(&url))
                            })),
                    )
                });
            },
        );
    }

    pub(crate) fn stats_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run_op(
            window,
            cx,
            spork_git::stats::stats,
            |_, stats, window, cx| show(stats, window, cx),
        );
    }
}

fn show(stats: Stats, window: &mut Window, cx: &mut App) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let months = last_year(&stats.times, now);
    let theme = cx.theme();
    let (muted, fill, border) = (theme.muted_foreground, theme.chart_1, theme.border);
    let headline = format!(
        "{} commits by {} author(s) on this branch, merges left out",
        stats.times.len(),
        stats.authors.len()
    );
    window.open_dialog(cx, move |dialog, _, _| {
        let chart = BarChart::new(months.clone())
            .id("commits-per-month")
            .interactive(true)
            .band(|m: &(String, usize)| m.0.clone())
            .value(|m: &(String, usize)| m.1 as f64)
            .fill(move |_: &(String, usize), _, _, _| fill)
            .corner_radii(Corners {
                top_left: px(4.),
                top_right: px(4.),
                bottom_left: px(0.),
                bottom_right: px(0.),
            })
            .grid_dashed(true)
            .value_tick_count(3)
            .tooltip_title(|m: &(String, usize)| m.0.clone().into())
            .tooltip_value(|_, v| format!("{v:.0} commits").into());
        let rows = stats.authors.iter().take(TOP_AUTHORS).map(|a| {
            h_flex()
                .gap_3()
                .py_0p5()
                .text_sm()
                .child(div().flex_1().min_w_0().truncate().child(a.name.clone()))
                .child(div().w(px(70.)).text_right().child(a.commits.to_string()))
                .child(
                    div()
                        .w(px(80.))
                        .text_right()
                        .text_color(muted)
                        .child(format!("+{}", a.added)),
                )
                .child(
                    div()
                        .w(px(80.))
                        .text_right()
                        .text_color(muted)
                        .child(format!("−{}", a.deleted)),
                )
        });
        dialog.title("Repository statistics").w(px(640.)).child(
            v_flex()
                .gap_3()
                .child(div().text_sm().text_color(muted).child(headline.clone()))
                .child(div().text_sm().child("Commits per month, last 12 months"))
                .child(div().h(px(180.)).child(chart))
                .child(
                    h_flex()
                        .gap_3()
                        .pb_1()
                        .border_b_1()
                        .border_color(border)
                        .text_xs()
                        .text_color(muted)
                        .child(div().flex_1().child("Author"))
                        .child(div().w(px(70.)).text_right().child("Commits"))
                        .child(div().w(px(80.)).text_right().child("Lines added"))
                        .child(div().w(px(80.)).text_right().child("Lines removed")),
                )
                .child(
                    v_flex()
                        .id("authors")
                        .max_h(px(260.))
                        .children(rows)
                        .overflow_y_scrollbar(),
                ),
        )
    });
}

fn human_size(bytes: u64) -> String {
    match bytes {
        0..1_048_576 => format!("{:.0} KB", bytes as f64 / 1024.0),
        1_048_576..1_073_741_824 => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
        _ => format!("{:.2} GB", bytes as f64 / 1_073_741_824.0),
    }
}

#[cfg(test)]
mod tests {
    // explicit: `super::*` would pull in gpui's `test` attribute, which shadows #[test]
    use super::{last_year, year_month};

    #[test]
    fn buckets_by_month() {
        assert_eq!(year_month(0), (1970, 1));
        assert_eq!(year_month(951_782_400), (2000, 2)); // 2000-02-29
        assert_eq!(year_month(1_798_761_600), (2027, 1)); // 2027-01-01
        let now = 1_794_960_000; // 2026-11-18
        let months = last_year(&[now, now - 86_400, 1_764_547_200 /* 2025-12-01 */, 0], now);
        assert_eq!(months.len(), 12);
        assert_eq!(months[11], ("Nov".into(), 2));
        assert_eq!(months[0], ("Dec".into(), 1));
        assert_eq!(months.iter().map(|m| m.1).sum::<usize>(), 3);
    }
}
