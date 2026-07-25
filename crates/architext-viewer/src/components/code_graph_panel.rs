//! The Code Graph mode: renders Magma's `code-graph.json` as a layered
//! node-link diagram.
//!
//! Mirrors the `BlastRadiusPanel` shape — a mode component that renders its own
//! centre-canvas surface rather than a routed diagram. Selection is PANEL-LOCAL:
//! code-graph ids live in their own id-space and must never be written into
//! `AppState::selected_node`, which the inspector resolves against `data.nodes`.
//!
//! Four surfaces, in precedence order:
//!   1. no document registered  → how-to-generate empty state
//!   2. document unreadable     → explicit error surface (never a blank canvas)
//!   3. refusal (`computable:false`) → the producer's reason, verbatim
//!   4. a graph                 → the diagram (Task 4 onward)
use leptos::*;

use crate::code_graph_model::{
    build_function_layout, build_module_layout, format_signature, reach_badges, GraphConfig,
};
use crate::components::code_graph_svg::CodeGraphSvg;
use crate::state::use_app_state;

// Zoom bounds + step (mirrors `canvas_panel.rs` — centralized, not magic
// literals at call sites).
const ZOOM_MIN: f64 = 0.1;
const ZOOM_MAX: f64 = 4.0;
const ZOOM_STEP: f64 = 1.2;

#[component]
pub fn CodeGraphPanel() -> impl IntoView {
    let state = use_app_state();

    let pan_x = create_rw_signal(0.0_f64);
    let pan_y = create_rw_signal(0.0_f64);
    let zoom = create_rw_signal(1.0_f64);
    let selected = create_rw_signal::<Option<String>>(None);
    // None = coarse (module) tier; Some(module_id) = that module's functions.
    let drill = create_rw_signal::<Option<String>>(None);

    let zoom_by = move |factor: f64| {
        zoom.update(|z| *z = (*z * factor).clamp(ZOOM_MIN, ZOOM_MAX));
    };
    let on_wheel = move |ev: ev::WheelEvent| {
        ev.prevent_default();
        let factor = if ev.delta_y() < 0.0 { ZOOM_STEP } else { 1.0 / ZOOM_STEP };
        zoom_by(factor);
    };
    let dragging = create_rw_signal(false);
    let last = create_rw_signal((0.0_f64, 0.0_f64));
    let on_mouse_down = move |ev: ev::MouseEvent| {
        dragging.set(true);
        last.set((ev.client_x() as f64, ev.client_y() as f64));
    };
    let on_mouse_move = move |ev: ev::MouseEvent| {
        if !dragging.get() {
            return;
        }
        let (lx, ly) = last.get();
        let (cx, cy) = (ev.client_x() as f64, ev.client_y() as f64);
        pan_x.update(|p| *p += cx - lx);
        pan_y.update(|p| *p += cy - ly);
        last.set((cx, cy));
    };
    // mouseleave also ends the drag — otherwise it "sticks" when the pointer
    // exits the viewport mid-drag (same reason canvas_panel.rs does this).
    let end_drag = move |_: ev::MouseEvent| dragging.set(false);
    // Shared by the reset button, the "Modules" crumb, and drilling into a
    // module — each of those reframes the view for a new tier/level.
    let reset_pan_zoom = move || {
        pan_x.set(0.0);
        pan_y.set(0.0);
        zoom.set(1.0);
    };
    let reset_view = move |_| reset_pan_zoom();

    view! {
        <div class="code-graph">
            {move || {
                let data = state.data.get();
                match &data.code_graph {
                    // 1. Not registered in the manifest.
                    None => view! {
                        <div class="code-graph__empty">
                            <h2 class="code-graph__empty-title">"No code graph yet"</h2>
                            <p class="code-graph__empty-body">
                                "This project has no " <code>"code-graph.json"</code> ". \
                                 Generate one with Magma, then re-run " <code>"architext doctor"</code>
                                " to register it:"
                            </p>
                            <pre class="code-graph__empty-cmd">"magma --architext <repo> <name> <vault>"</pre>
                            <p class="code-graph__empty-note">
                                "Go modules only — other languages report why the graph could not be built."
                            </p>
                        </div>
                    }.into_view(),
                    // 2. Registered but unreadable/malformed — say so loudly.
                    Some(Err(err)) => view! {
                        <div class="code-graph__empty">
                            <h2 class="code-graph__empty-title">"Code graph could not be read"</h2>
                            <p class="code-graph__empty-body">{err.to_string()}</p>
                            <p class="code-graph__empty-note">
                                "The rest of the architecture loaded normally. Re-run the producer, \
                                 then " <code>"architext validate"</code> " to see the specific defect."
                            </p>
                        </div>
                    }.into_view(),
                    // 3. A refusal is a VALID document — surface the reason verbatim
                    //    and synthesize nothing.
                    Some(Ok(cg)) if !cg.computable => {
                        let reason = cg.not_computable_reason.clone()
                            .unwrap_or_else(|| "no reason given".to_string());
                        let lang = cg.language.clone();
                        view! {
                            <div class="code-graph__empty">
                                <h2 class="code-graph__empty-title">"No graph available"</h2>
                                <p class="code-graph__empty-body">{reason}</p>
                                <p class="code-graph__empty-note">
                                    {format!("The producer analysed this project as \"{lang}\" and \
                                              declined to guess. Nothing is inferred.")}
                                </p>
                            </div>
                        }.into_view()
                    }
                    // 4. A real graph — module tier by default, drilling into one
                    //    module's functions on click. The breadcrumb always
                    //    renders (even when a tier's layout is empty) so a user
                    //    who drills into an empty module is never stranded.
                    Some(Ok(cg)) => {
                        let layout = match drill.get() {
                            None => build_module_layout(cg, &GraphConfig::default()),
                            Some(ref m) => build_function_layout(cg, m, &GraphConfig::default()),
                        };
                        view! {
                            <nav class="code-graph__crumbs" aria-label="Code graph level">
                                <button
                                    class="code-graph__crumb"
                                    class:is-active=move || drill.get().is_none()
                                    on:click=move |_| {
                                        drill.set(None);
                                        selected.set(None);
                                        reset_pan_zoom();
                                    }
                                >
                                    "Modules"
                                </button>
                                {move || drill.get().map(|_| view! {
                                    <span class="code-graph__crumb-sep">"›"</span>
                                })}
                                {move || drill.get().map(|m| view! {
                                    <span class="code-graph__crumb is-active">{m}</span>
                                })}
                            </nav>
                            {if layout.nodes.is_empty() {
                                view! {
                                    <div class="code-graph__empty">
                                        <h2 class="code-graph__empty-title">"Nothing to show"</h2>
                                        <p class="code-graph__empty-body">
                                            "This level has no nodes to draw."
                                        </p>
                                    </div>
                                }.into_view()
                            } else {
                                view! {
                                    <div
                                        class="code-graph__viewport"
                                        on:wheel=on_wheel
                                        on:mousedown=on_mouse_down
                                        on:mousemove=on_mouse_move
                                        on:mouseup=end_drag
                                        on:mouseleave=end_drag
                                    >
                                        <CodeGraphSvg
                                            layout=layout
                                            pan_x=pan_x
                                            pan_y=pan_y
                                            zoom=zoom
                                            selected=selected
                                            on_select=Callback::new(move |id: String| {
                                                if drill.get_untracked().is_none() {
                                                    // Coarse tier: descend into the module and
                                                    // reset the view so the new tier is framed.
                                                    drill.set(Some(id));
                                                    selected.set(None);
                                                    reset_pan_zoom();
                                                } else {
                                                    selected.set(Some(id));
                                                }
                                            })
                                        />
                                    </div>
                                    <div class="code-graph__controls">
                                        <button on:click=move |_| zoom_by(1.0 / ZOOM_STEP)>"−"</button>
                                        <button on:click=reset_view title="Reset view">"⤢"</button>
                                        <button on:click=move |_| zoom_by(ZOOM_STEP)>"+"</button>
                                    </div>
                                }.into_view()
                            }}
                            <div class="cg-detail">
                                {move || {
                                    let Some(id) = selected.get() else {
                                        // Module tier: clicking a node DRILLS rather than
                                        // selects, so there is never a module selection to
                                        // show detail for — say so instead of the generic
                                        // function-tier hint.
                                        let hint = if drill.get().is_none() {
                                            "Click a module to drill into its functions."
                                        } else {
                                            "Select a node to see its detail."
                                        };
                                        return view! {
                                            <p class="cg-detail__hint">{hint}</p>
                                        }.into_view();
                                    };
                                    let data = state.data.get();
                                    let Some(Ok(cg)) = data.code_graph.as_ref() else {
                                        return ().into_view();
                                    };
                                    let Some(f) = cg.functions.as_ref()
                                        .and_then(|fs| fs.iter().find(|f| f.id == id))
                                    else {
                                        // Module-tier selection is impossible (clicking a
                                        // module drills, it never sets `selected`), so this
                                        // is reachable only when an SSE data reload swaps in
                                        // a new graph while `selected` still holds a function
                                        // id absent from it. Say so plainly rather than
                                        // printing the bare opaque id.
                                        return view! {
                                            <p class="cg-detail__hint">
                                                {format!("Function \"{id}\" is no longer in the code graph.")}
                                            </p>
                                        }.into_view();
                                    };
                                    let badges = reach_badges(f);
                                    let sig = format_signature(&f.signature);
                                    let loc = format!("{}:{}", f.file, f.line);
                                    let fan = format!("fan-in {} · fan-out {}", f.fan_in, f.fan_out);
                                    let doc = f.doc.clone();
                                    view! {
                                        <div class="cg-detail__body">
                                            <h3 class="cg-detail__symbol">{f.symbol.clone()}</h3>
                                            <code class="cg-detail__sig">{sig}</code>
                                            <p class="cg-detail__meta">{loc}</p>
                                            <p class="cg-detail__meta">{fan}</p>
                                            {doc.map(|d| view! { <p class="cg-detail__doc">{d}</p> })}
                                            <div class="cg-detail__badges">
                                                {badges.into_iter().map(|b| view! {
                                                    <span
                                                        class="chip chip--state cg-chip"
                                                        style=format!("color:{}", b.color_var())
                                                        title=b.tooltip()
                                                    >
                                                        {b.label()}
                                                    </span>
                                                }).collect_view()}
                                            </div>
                                        </div>
                                    }.into_view()
                                }}
                            </div>
                        }.into_view()
                    }
                }
            }}
        </div>
    }
}
