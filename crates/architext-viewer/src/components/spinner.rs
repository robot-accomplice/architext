//! A small, on-language progress indicator — one component reused for the
//! initial data load and for the async diagram (re)compute.
//!
//! DESIGN.md fidelity: a 1px-stroke `--accent` ring that spins (the only motion;
//! the rest of the chrome is static hairline + mono), a mono uppercase label.
//! It animates because the work it covers is async (the main thread is free):
//! the data fetch (`/data/**`) and the plan fetch (`/api/plan/{hash}`) both run
//! off the main thread, so the ring actually turns instead of freezing.

use leptos::*;

/// A spinner ring + mono label. `label` is the short status word ("Routing",
/// "Loading") rendered as an `.overline`.
#[component]
pub fn Spinner(#[prop(into)] label: String) -> impl IntoView {
    view! {
        <div class="spinner" role="status" aria-live="polite">
            <span class="spinner__ring" aria-hidden="true"></span>
            <span class="spinner__label overline">{label}</span>
        </div>
    }
}

/// The spinner centered as an overlay inside a relatively-positioned host (the
/// canvas stage). Shown only while a (re)compute is in flight; it does not
/// intercept pointer events, so pan/zoom stay live, and it is removed once the
/// render bundle is ready (never obstructs the loaded diagram).
#[component]
pub fn CanvasSpinner(#[prop(into)] label: String) -> impl IntoView {
    view! {
        <div class="canvas-panel__progress">
            <Spinner label=label/>
        </div>
    }
}
