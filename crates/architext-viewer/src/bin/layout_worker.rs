//! Plan D Task 3 — the code-graph layout worker.
//!
//! A SECOND Rust/wasm entry point: a separate Cargo `[[bin]]` in the SAME
//! crate as the main Leptos app (auto-discovered from `src/bin/`), built by
//! Trunk as a Web Worker (`index.html`'s `data-type="worker"` link) — zero
//! hand-written JavaScript. Trunk's `data-loader-shim` generates the tiny
//! classic-worker bootstrap script (`importScripts(...);wasm_bindgen(...)`)
//! itself; nothing here authors any `.js`.
//!
//! This file is a thin message-passing shell around `force_layout::simulate`
//! — already worker-shaped (pure Rust, no Leptos, no DOM; see that module's
//! docs) — decode the request with the SAME wire-format helpers the main
//! thread encodes with (`architext_viewer::layout_worker_client`, so the two
//! sides cannot drift), run the deterministic settle to completion, encode
//! and post back the result.
//!
//! DETERMINISM: this calls the exact same `force_layout::simulate` the
//! main-thread progressive settle uses (`code_graph_layout::LayoutDriver`),
//! with the same seed and `ForceConfig` — same inputs into the same pure,
//! deterministic function, so the answer is bit-identical to whichever side
//! computes it (see `layout_cache`'s module docs for why that is what makes
//! caching sound at all).
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{DedicatedWorkerGlobalScope, MessageEvent};

use architext_viewer::force_layout::simulate;
use architext_viewer::layout_worker_client::{decode_request, encode_result};

fn main() {
    console_error_panic_hook::set_once();

    let scope = DedicatedWorkerGlobalScope::from(JsValue::from(js_sys::global()));
    let scope_for_reply = scope.clone();

    let onmessage = Closure::wrap(Box::new(move |ev: MessageEvent| {
        let (node_count, edges, seed, cfg) = match decode_request(&ev.data()) {
            Ok(req) => req,
            Err(e) => {
                web_sys::console::error_2(&"[layout-worker] bad request:".into(), &e);
                return;
            }
        };

        let result = simulate(node_count as usize, &edges, seed, &cfg);
        let positions: Vec<(f32, f32)> =
            result.positions.iter().map(|&(x, y)| (x as f32, y as f32)).collect();

        match encode_result(&positions, result.ticks_run) {
            Ok((msg, transfer)) => {
                if let Err(e) = scope_for_reply.post_message_with_transfer(&msg, &transfer) {
                    web_sys::console::error_2(&"[layout-worker] post_message failed:".into(), &e);
                }
            }
            Err(e) => web_sys::console::error_2(&"[layout-worker] encode_result failed:".into(), &e),
        }
    }) as Box<dyn FnMut(MessageEvent)>);
    scope.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget(); // one-shot worker — never torn down early, nothing to unregister

    // Signal readiness. A wasm-bindgen worker's `main()` yields to the JS
    // event loop while its wasm loads; the main thread must not post the
    // real request until it sees this, or the browser silently drops it
    // instead of queueing it (see `layout_worker_client`'s module docs).
    let _ = scope.post_message(&js_sys::Array::new().into());
}
