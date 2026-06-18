//! architext-viewer — Leptos CSR app (wasm32), built with Trunk (no Node).
//!
//! V2 added the data layer; V3 adds the FLOWS-mode diagram canvas: the selected
//! (view, flow) is turned into a routing `Plan` IN-PROCESS (no worker) and
//! rendered as a fluid, pan/zoom SVG. The shell renders loading / error /
//! loaded states. Non-flows modes keep their data surfaces (diagram is V4).
//!
//! Module layout (clean separation):
//! - `data`      — serde models + same-origin async fetch
//! - `state`     — `AppState` (signals) provided via Leptos context
//! - `selection` — thin adapter over `architext_routing` view-selection
//! - `diagram`   — in-process plan compute + SVG render (flows mode)
//! - `components`— one component per file
//! - `theme`     — enumerated design facts (the nine modes)
pub mod blast_radius;
pub mod components;
pub mod data;
pub mod diagram;
pub mod flow_step_display;
pub mod release_truth;
pub mod repo_tree_model;
pub mod rule_order;
pub mod selection;
pub mod severity;
pub mod state;
pub mod theme;

use leptos::*;

use crate::components::shell::Shell;
use crate::components::spinner::Spinner;
use crate::data::live::start_live_reload;
use crate::data::{fetch_cli_version, load_architecture_data, FetchError};
use crate::state::AppState;

/// Root view — loads the dataset, then mounts the data-bound shell.
///
/// The data load runs once on mount (`create_local_resource` with a unit
/// source). Loading and error states render explicit surfaces; the blank-screen
/// failure mode is treated as a defect, not an option.
#[component]
pub fn App() -> impl IntoView {
    let data = create_local_resource(
        || (),
        |_| async move { load_architecture_data().await },
    );

    view! {
        <Suspense fallback=move || view! { <LoadingScreen/> }>
            {move || data.get().map(|result| match result {
                Ok(loaded) => {
                    let state = AppState::new(loaded);
                    provide_context(state);
                    // The CLI version is a separate, non-fatal fetch (display-only
                    // header eyebrow); load it into state once, after mount.
                    let cli_version = state.cli_version;
                    spawn_local(async move {
                        if let Some(v) = fetch_cli_version().await {
                            cli_version.set(Some(v));
                        }
                    });
                    // Open the live-reload SSE stream: on a validated on-disk data
                    // change the dataset re-fetches and the diagram re-renders in
                    // place (selection preserved); an invalid change shows a notice.
                    start_live_reload(state);
                    view! { <Shell/> }.into_view()
                }
                Err(err) => view! { <ErrorScreen err=err/> }.into_view(),
            })}
        </Suspense>
    }
}

/// Loading surface shown while the dataset is fetched.
#[component]
fn LoadingScreen() -> impl IntoView {
    view! {
        <div class="boot-screen">
            <Spinner label="Loading"/>
            <p class="boot-screen__msg">"Fetching architecture data…"</p>
        </div>
    }
}

/// Error surface shown when the dataset cannot be loaded (never a blank screen).
#[component]
fn ErrorScreen(err: FetchError) -> impl IntoView {
    view! {
        <div class="boot-screen boot-screen--error">
            <div class="overline">"DATA LOAD FAILED"</div>
            <div class="accent-surface boot-screen__error-card">
                <p class="boot-screen__msg">"Could not load architecture data."</p>
                <p class="boot-screen__detail">{err.url}</p>
                <p class="boot-screen__detail">{err.message}</p>
            </div>
        </div>
    }
}
