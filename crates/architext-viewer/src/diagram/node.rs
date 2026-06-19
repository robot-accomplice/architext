//! Node-card and decision-diamond renderers (one node per call).
//!
//! A node card is an 8px-radius rect (`--node-card-radius`) with a 2px category
//! top-bar in the node's `--c4-{type}` role color, the node name (Hanken
//! Grotesk via `.flow-node__name`), and the type as a chip (JetBrains Mono via
//! `.flow-node__type`). The SELECTED node gets the `--accent` state treatment
//! (ring/glow) — a STATE class, never a role hue.

use architext_routing::model::Rect;
use leptos::*;

use super::role_color_var;
use crate::components::node_icon::node_icon_path;

/// Corner type-glyph geometry (canvas px). The 24×24 line glyph is scaled to
/// `ICON_SIZE` and inset from the card's top-left by `ICON_PAD` (clear of the
/// 2px top-bar and the centered name).
const ICON_SIZE: f64 = 15.0;
const ICON_PAD: f64 = 5.0;

/// Name/type vertical metrics (canvas px) for centering the wrapped name block
/// plus the type chip within the card. `NAME_LINE_H` is the line box for the
/// 12px name; `TYPE_*` reserve room for the 9px type chip below it.
const NAME_LINE_H: f64 = 13.0;
const TYPE_GAP: f64 = 3.0;
const TYPE_H: f64 = 11.0;
/// Horizontal padding inside the card reserved from the name text, and the
/// approximate advance width of the 12px Hanken name (proportional, ~0.58em) —
/// good enough to greedily wrap to the card width without a DOM text measure.
const NAME_PAD_X: f64 = 6.0;
const NAME_CHAR_W: f64 = 12.0 * 0.58;
/// Cap wrapped names at two lines so the block + type chip fit the default 54px
/// card; an over-long name truncates with an ellipsis on the second line.
const NAME_MAX_LINES: usize = 2;

/// Greedily word-wrap a node name to fit `width`, capped at [`NAME_MAX_LINES`].
/// Pure string math (no DOM measure) using [`NAME_CHAR_W`]; if the name needs
/// more lines than the cap, the last kept line is ellipsized. Whitespace-only or
/// single over-long words are returned as-is (one line) rather than dropped.
fn wrap_name(name: &str, width: f64) -> Vec<String> {
    let max_chars = (((width - 2.0 * NAME_PAD_X) / NAME_CHAR_W).floor() as usize).max(1);
    let words: Vec<&str> = name.split_whitespace().collect();
    if words.is_empty() {
        return vec![name.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for w in words {
        let cand = if cur.is_empty() {
            w.to_string()
        } else {
            format!("{cur} {w}")
        };
        if cur.is_empty() || cand.chars().count() <= max_chars {
            cur = cand;
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = w.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.len() <= NAME_MAX_LINES {
        return lines;
    }
    // Overflow: keep the first NAME_MAX_LINES, ellipsize the last kept line.
    let mut kept: Vec<String> = lines.into_iter().take(NAME_MAX_LINES).collect();
    if let Some(last) = kept.last_mut() {
        while last.chars().count() > max_chars.saturating_sub(1) && !last.is_empty() {
            last.pop();
        }
        *last = format!("{}…", last.trim_end());
    }
    kept
}

/// A node ready to render: its placed rect plus the display fields resolved
/// from the loaded dataset (name + C4 type).
#[derive(Clone)]
pub struct NodeView {
    pub id: String,
    pub name: String,
    pub node_type: String,
    /// The placed top-left (`x`/`y`) and NATURAL card size (`width`/`height`).
    /// The card's inner content (icon, wrapped name, type chip) is laid out
    /// against this natural size, then the whole card is uniformly scaled by
    /// [`NodeView::scale`] at render time — so a parked card shrinks crisply
    /// instead of clipping a fixed-px name into a tiny rect.
    pub rect: Rect,
    /// Whether the node is part of the active flow (an endpoint of some flow
    /// step). `true` when there is no flow (structural C4/Deployment mode →
    /// every node is "in flow"). When `false` the card is dimmed,
    /// non-interactive, and parked beside the flow (the `flow-node--unrelated`
    /// state).
    pub in_flow: bool,
    /// Uniform render scale for the card (1.0 in-flow; ~0.62 for parked
    /// out-of-flow cards, so the flow dominates while the parked cluster stays
    /// compact). The rendered footprint is `width*scale` × `height*scale`.
    pub scale: f64,
    /// Whether clicking this card DRILLS DOWN to a scoped C4 child view (C4 mode,
    /// the node has a scoped child view). Drives the drilldown affordance — a
    /// pointer cursor plus a small corner chevron — so a decomposable node reads
    /// as open-able while a leaf/external node (no child) shows no such cue and
    /// merely selects for the inspector. `false` in every non-C4 mode.
    pub drillable: bool,
}

/// Render one node card. `selected` toggles the `--accent` state treatment;
/// `on_select` fires the node id on click so the inspector can bind to it.
#[component]
pub fn DiagramNode(
    node: NodeView,
    #[prop(into)] selected: Signal<bool>,
    #[prop(into)] on_select: Callback<String>,
) -> impl IntoView {
    let Rect { x, y, width, height } = node.rect;
    let role = role_color_var(&node.node_type);
    let id_for_click = node.id.clone();
    // Corner type-glyph: the node's type → DiagramIcon glyph, tinted to the
    // SAME `--c4-*` role token as the top-bar (single source). The 24×24 path
    // is scaled to ICON_SIZE and positioned just below the top-bar at the left.
    let icon_d = node_icon_path(&node.node_type);
    let icon_scale = ICON_SIZE / 24.0;
    let icon_transform = format!("translate({ICON_PAD} {ICON_PAD}) scale({icon_scale})");
    let icon_color = role.clone();

    // Card classes: base + selected state (rule 1: state ≠ role hue). An
    // out-of-flow ("unrelated") node carries `flow-node--unrelated` (CSS dims it
    // and disables pointer events) so the active flow stays the visual focus.
    let in_flow = node.in_flow;
    // A drillable card (C4 node with a scoped child) carries `flow-node--drillable`
    // so CSS gives it the pointer cursor + reveals the corner chevron — the
    // affordance that this card opens a child view. Non-drillable cards omit it,
    // so the *absence* of the cue tells the user drilldown is unavailable.
    let drillable = node.drillable;
    let group_class = move || match (in_flow, selected.get(), drillable) {
        (false, _, _) => "flow-node flow-node--unrelated",
        (true, true, true) => "flow-node flow-node--selected flow-node--drillable",
        (true, true, false) => "flow-node flow-node--selected",
        (true, false, true) => "flow-node flow-node--drillable",
        (true, false, false) => "flow-node",
    };

    // Wrap the name once; the line count drives both the name tspans and the
    // type chip's baseline (so the block + chip stay vertically centered).
    let name_lines = wrap_name(&node.name, width);
    let n_lines = name_lines.len() as f64;
    let block_top = height / 2.0 - (n_lines * NAME_LINE_H + TYPE_GAP + TYPE_H) / 2.0;
    let type_cy = block_top + n_lines * NAME_LINE_H + TYPE_GAP + TYPE_H / 2.0;

    // Parked cards render at NATURAL size then scale uniformly, so the inner
    // text/icon shrink proportionally rather than clipping (UX #2 parked look).
    let scale = node.scale;
    view! {
        <g
            class=group_class
            transform=format!("translate({x} {y}) scale({scale})")
            on:click=move |_| on_select.call(id_for_click.clone())
        >
            // Card body — 8px radius via --node-card-radius (set in CSS).
            <rect class="flow-node__card" width=width height=height rx="8" ry="8"></rect>
            // 2px category top-bar in the node's role color (single source).
            <rect class="flow-node__topbar" width=width height="2" fill=role.clone()></rect>
            // Upper-left type glyph, tinted to the SAME role token as the bar.
            <g class="flow-node__icon" transform=icon_transform style=("color", icon_color)>
                <path d=icon_d></path>
            </g>
            // Node name (Hanken Grotesk), word-wrapped to the card width so a
            // long name stacks into lines instead of overflowing the card. The
            // name block + type chip are vertically centered as a unit.
            <text class="flow-node__name" x=width / 2.0>
                {name_lines
                    .into_iter()
                    .enumerate()
                    .map(|(i, line)| {
                        let cy = block_top + NAME_LINE_H * (i as f64 + 0.5);
                        view! { <tspan x=width / 2.0 y=cy>{line}</tspan> }
                    })
                    .collect_view()}
            </text>
            // Type chip (JetBrains Mono), tinted with the role color, just below
            // the (possibly multi-line) name block.
            <text class="flow-node__type" x=width / 2.0 y=type_cy fill=role>
                {node.node_type.clone()}
            </text>
            // Drilldown affordance: a small bottom-right chevron, shown by CSS
            // only on `flow-node--drillable` cards. Signals "click to open the
            // scoped child view"; a node without a child has no chevron, so the
            // cue's absence reads as "no drilldown here".
            {drillable.then(|| {
                let gx = width - ICON_PAD - ICON_SIZE;
                let gy = height - ICON_PAD - ICON_SIZE;
                let g_transform = format!("translate({gx} {gy}) scale({icon_scale})");
                view! {
                    <g class="flow-node__drill" transform=g_transform>
                        <path d="M9 6l6 6-6 6"></path>
                    </g>
                }
            })}
        </g>
    }
}

/// Render a decision diamond (an extra decision-node rect, `fixedPorts`). It is
/// a rotated square centered on the rect; tinted with the affiliated component's
/// role color so the branch point reads as part of that node's lane.
#[component]
pub fn DecisionDiamond(rect: Rect, #[prop(into)] role_var: String) -> impl IntoView {
    let cx = rect.x + rect.width / 2.0;
    let cy = rect.y + rect.height / 2.0;
    let r = rect.width / 2.0;
    // Diamond points: top, right, bottom, left.
    let points = format!(
        "{cx},{} {},{cy} {cx},{} {},{cy}",
        cy - r,
        cx + r,
        cy + r,
        cx - r,
    );
    view! {
        <polygon class="flow-decision" points=points stroke=role_var></polygon>
    }
}

#[cfg(test)]
mod wrap_tests {
    use super::wrap_name;

    // Default card width is 136px → ~18 chars/line at the 12px name advance.
    const W: f64 = 136.0;

    #[test]
    fn short_name_stays_one_line() {
        assert_eq!(wrap_name("Architext CLI", W), vec!["Architext CLI"]);
    }

    #[test]
    fn long_name_wraps_to_two_lines() {
        // The overflowing case from the audit screenshot.
        let lines = wrap_name("Architecture model domain", W);
        assert_eq!(lines.len(), 2, "should wrap, got {lines:?}");
        // Every line fits the card; nothing overflows horizontally.
        assert!(lines.iter().all(|l| l.chars().count() <= 19), "{lines:?}");
    }

    #[test]
    fn over_long_name_is_capped_and_ellipsized() {
        let lines = wrap_name("Distributed event sourcing projection rebuild coordinator service", W);
        assert_eq!(lines.len(), 2, "capped at two lines");
        assert!(lines.last().unwrap().ends_with('…'), "last line ellipsized");
    }

    #[test]
    fn empty_or_single_word_never_dropped() {
        assert_eq!(wrap_name("", W), vec![""]);
        assert_eq!(wrap_name("Supercalifragilistic", W).len(), 1);
    }
}
