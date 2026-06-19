//! Release trend chart (F14) — faithful port of the React `ReleaseTrendChart`.
//!
//! An SVG area/line chart of per-release feature vs bug-fix counts over time:
//! filled feature/fix areas, trend lines, per-release dots toned by posture, a
//! dashed marker on the selected release, and y-ticks. Pure DISPLAY plus a
//! select interaction — clicking a release drives the shared `selected_release`
//! signal, so the chart and the left-nav selector stay in lockstep.

use leptos::*;

use crate::data::models::ReleaseSummary;
use crate::release_truth::{release_tone, Tone};
use crate::state::use_app_state;

// SVG canvas geometry — matches the React chart's viewBox + padding exactly.
const WIDTH: f64 = 1200.0;
const HEIGHT: f64 = 240.0;
const PAD_TOP: f64 = 38.0;
const PAD_RIGHT: f64 = 24.0;
const PAD_BOTTOM: f64 = 78.0;
const PAD_LEFT: f64 = 36.0;

/// Tone → the circle CSS modifier (`release-history circle.<class>`); Neutral
/// keeps the default (blue) circle with no modifier.
fn tone_class(t: Tone) -> &'static str {
    match t {
        Tone::Healthy => "healthy",
        Tone::Progressing => "progressing",
        Tone::Blocked => "blocked",
        Tone::Inactive => "inactive",
        Tone::Neutral => "",
    }
}

/// Sort key: releasedAt ?? targetDate ?? targetWindow ?? "" — ISO strings sort
/// lexicographically, matching the React `localeCompare` on these ASCII dates.
fn sort_key(r: &ReleaseSummary) -> String {
    r.released_at
        .clone()
        .or_else(|| r.target_date.clone())
        .or_else(|| r.target_window.clone())
        .unwrap_or_default()
}

#[component]
pub fn ReleaseTrendChart() -> impl IntoView {
    let state = use_app_state();
    let selected = state.selected_release;

    view! {
        <div class="release-history">
            {move || {
                let data = state.data.get();
                let mut sorted: Vec<ReleaseSummary> = data
                    .release_index
                    .as_ref()
                    .map(|idx| idx.releases.clone())
                    .unwrap_or_default();
                sorted.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
                if sorted.is_empty() {
                    return view! {
                        <p class="release-panel__hint">"No release history to chart."</p>
                    }
                    .into_view();
                }

                let n = sorted.len();
                let baseline = HEIGHT - PAD_BOTTOM;
                let x_label_y = baseline + 22.0;
                let max_count = sorted
                    .iter()
                    .flat_map(|r| [r.counts.features, r.counts.bug_fixes])
                    .max()
                    .unwrap_or(1)
                    .max(1) as f64;

                let active = selected.get();
                let marker_index = active
                    .as_ref()
                    .and_then(|id| sorted.iter().position(|r| &r.id == id))
                    .unwrap_or(0);

                let x_for = |i: usize| -> f64 {
                    if n == 1 {
                        WIDTH / 2.0
                    } else {
                        PAD_LEFT + (i as f64) * (WIDTH - PAD_LEFT - PAD_RIGHT) / ((n - 1) as f64)
                    }
                };
                let y_for = |count: i64| -> f64 {
                    baseline - (count as f64) * (baseline - PAD_TOP) / max_count
                };

                // Trend lines (and the closed areas down to the baseline).
                let mut feat_line = String::new();
                let mut fix_line = String::new();
                for (i, r) in sorted.iter().enumerate() {
                    let cmd = if i == 0 { "M" } else { "L" };
                    let sep = if i == 0 { "" } else { " " };
                    let x = x_for(i);
                    feat_line.push_str(&format!("{sep}{cmd} {x} {}", y_for(r.counts.features)));
                    fix_line.push_str(&format!("{sep}{cmd} {x} {}", y_for(r.counts.bug_fixes)));
                }
                let (first_x, last_x) = (x_for(0), x_for(n - 1));
                let feat_area = format!("{feat_line} L {last_x} {baseline} L {first_x} {baseline} Z");
                let fix_area = format!("{fix_line} L {last_x} {baseline} L {first_x} {baseline} Z");

                // y ticks: unique [0, ceil(max/2), max], order-preserving.
                let half = (max_count / 2.0).ceil() as i64;
                let mut seen = std::collections::BTreeSet::new();
                let ticks: Vec<i64> = [0, half, max_count as i64]
                    .into_iter()
                    .filter(|t| seen.insert(*t))
                    .collect();

                let axis = format!(
                    "M {PAD_LEFT} {PAD_TOP} V {baseline} H {}",
                    WIDTH - PAD_RIGHT
                );
                let active_line = format!("M {} {PAD_TOP} V {baseline}", x_for(marker_index));

                let tick_views = ticks
                    .iter()
                    .map(|&t| {
                        let y = y_for(t);
                        view! {
                            <g>
                                <path
                                    class="release-chart-tick"
                                    d=format!("M {} {y} H {}", PAD_LEFT - 3.0, WIDTH - PAD_RIGHT)
                                ></path>
                                <text
                                    class="release-chart-y-label"
                                    x=PAD_LEFT - 7.0
                                    y=y + 3.0
                                    text-anchor="end"
                                >
                                    {t.to_string()}
                                </text>
                            </g>
                        }
                    })
                    .collect_view();

                let dot_views = sorted
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        let cx = x_for(i);
                        let cy = y_for(r.counts.features);
                        let id = r.id.clone();
                        let is_active = active.as_deref() == Some(id.as_str());
                        let tone = tone_class(release_tone(r.posture.as_deref()));
                        let circle_class =
                            format!("{}{tone}", if is_active { "active " } else { "" });
                        let ver = r.version.clone().unwrap_or_else(|| r.id.clone());
                        let title = format!(
                            "{ver} · {} features · {} bug fixes",
                            r.counts.features, r.counts.bug_fixes
                        );
                        let on_pick = move |_| selected.set(Some(id.clone()));
                        view! {
                            <g role="listitem" tabindex="0" on:click=on_pick>
                                <title>{title}</title>
                                <circle class=circle_class cx=cx cy=cy r="3.5"></circle>
                                <text
                                    class="release-chart-x-label"
                                    x=cx
                                    y=x_label_y
                                    text-anchor="end"
                                    transform=format!("rotate(-65 {cx} {x_label_y})")
                                >
                                    {ver}
                                </text>
                            </g>
                        }
                    })
                    .collect_view();

                view! {
                    <svg
                        viewBox=format!("0 0 {WIDTH} {HEIGHT}")
                        preserveAspectRatio="xMinYMin meet"
                        role="img"
                        aria-label="Release feature and bug-fix count trend"
                    >
                        <path class="release-chart-axis" d=axis></path>
                        {tick_views}
                        <path class="release-chart-area feature" d=feat_area></path>
                        <path class="release-chart-area fix" d=fix_area></path>
                        <path class="release-chart-line feature" d=feat_line></path>
                        <path class="release-chart-line fix" d=fix_line></path>
                        <path class="release-chart-active-line" d=active_line></path>
                        {dot_views}
                    </svg>
                    <div class="release-chart-legend">
                        <span>
                            <i class="feature"></i>
                            "Features"
                        </span>
                        <span>
                            <i class="fix"></i>
                            "Bug fixes"
                        </span>
                    </div>
                }
                .into_view()
            }}
        </div>
    }
}
