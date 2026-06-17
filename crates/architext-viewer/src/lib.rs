//! architext-viewer — Leptos CSR app (wasm32), built with Trunk (no Node).
//!
//! V1 is the chrome scaffold only: design tokens + the fixed-fluid console
//! shell (left nav / fluid canvas / right inspector). No data, no diagrams yet.
//!
//! The routing engine (`architext_routing`) is wired as a dependency now so
//! later slices call `plan()` in-process; `_routing_linked` keeps the linkage
//! live without yet invoking it.
pub mod components;
pub mod theme;

use leptos::*;

use crate::components::shell::Shell;

/// Root view — mounts the app shell.
#[component]
pub fn App() -> impl IntoView {
    view! { <Shell/> }
}

/// Touches the wasm-exported planner entry so the dependency is linked into the
/// bundle from V1. `architext_routing::plan` is `#[cfg(feature = "wasm")]` and
/// only exists for wasm32, so this linkage is gated to that target; the later
/// data slice replaces it with a real call.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub fn _routing_linked() -> fn(&str) -> Result<String, wasm_bindgen::JsValue> {
    |input| architext_routing::wasm::plan(input).map_err(Into::into)
}
