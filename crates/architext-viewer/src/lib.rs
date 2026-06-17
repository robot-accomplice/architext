//! architext-viewer — Leptos CSR app (wasm32), built with Trunk (no Node).
//!
//! V2 adds the data layer: same-origin fetch of the Architext documents
//! (`/data/**` + `/api/config`), serde models, reactive app state, and the
//! ported routing view-selection driving mode/view/flow selection. The shell
//! renders loading / error / loaded states; diagram rendering arrives in V3.
//!
//! Module layout (clean separation):
//! - `data`      — serde models + same-origin async fetch
//! - `state`     — `AppState` (signals) provided via Leptos context
//! - `selection` — thin adapter over `architext_routing` view-selection
//! - `components`— one component per file
//! - `theme`     — enumerated design facts (the nine modes)
pub mod components;
pub mod data;
pub mod selection;
pub mod state;
pub mod theme;

use leptos::*;

use crate::components::shell::Shell;
use crate::data::{load_architecture_data, FetchError};
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
            <div class="overline">"LOADING"</div>
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

/// Touches the wasm-exported planner entry so the dependency is linked into the
/// bundle. `architext_routing::plan` is `#[cfg(feature = "wasm")]` and only
/// exists for wasm32, so this linkage is gated to that target; V3 replaces it
/// with the real diagram-rendering call.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub fn _routing_linked() -> fn(&str) -> Result<String, wasm_bindgen::JsValue> {
    |input| architext_routing::wasm::plan(input).map_err(Into::into)
}
