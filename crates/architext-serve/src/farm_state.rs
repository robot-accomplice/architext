//! Shared plan-farm state for the serve layer.
//!
//! The plan farm is built at startup by calling `enumerate_flow_plan_requests`
//! from `architext-routing`, then stored in a `HashMap<hash, plan_json>` behind
//! an `Arc<RwLock<_>>` so handlers can read without blocking.
//!
//! This is the native precompute farm counterpart: it runs synchronously at
//! startup (no background worker thread for this slice — that optimisation
//! belongs to a later slice when the full watch-hub is ported).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use architext_routing::diagram_config::resolve_diagram_config_defaults;
use architext_routing::precompute::warm_flow_plans;

/// Per-plan entry stored in the farm.
#[derive(Clone)]
pub struct PlanEntry {
    /// The raw serialised plan JSON (without the `{"plan":...}` wrapper).
    pub plan_json: String,
}

/// Shared farm: hash → PlanEntry.
pub type Farm = Arc<RwLock<HashMap<String, PlanEntry>>>;

/// Initialise the farm by computing all plans synchronously.
///
/// Returns an empty farm (not an error) if `data_dir` doesn't contain
/// flows.json or views.json yet — this matches the JS behaviour where the
/// farm starts empty and fills in asynchronously.
pub fn build_farm(data_dir: &Path) -> Farm {
    let farm = empty_farm();
    refresh_farm(&farm, data_dir);
    farm
}

/// An empty farm. The serve startup uses this so it can bind + listen
/// immediately, then warm the farm on a background task ([`refresh_farm`]).
/// Lookups against an empty/warming farm simply miss, and the viewer falls back
/// to its in-process plan compute until the farm is populated.
pub fn empty_farm() -> Farm {
    Arc::new(RwLock::new(HashMap::new()))
}

/// (Re)populate the farm from `data_dir`. Inserts each plan into the live farm
/// the moment it is computed, so the viewer (which polls the farm) gets a HIT for
/// the diagram it opened as soon as that one plan is ready — instead of waiting
/// for the entire corpus to finish warming. Plan hashes are config-keyed, so a
/// later config re-warm inserts new hashes without disturbing in-flight reads
/// (the superseded entries are simply no longer requested).
pub fn refresh_farm(farm: &Farm, data_dir: &Path) {
    let config = resolve_diagram_config_defaults();
    let result = warm_flow_plans(data_dir, &config, |entry| {
        if let Ok(mut map) = farm.write() {
            map.insert(entry.hash, PlanEntry { plan_json: entry.plan_json });
        }
    });
    match result {
        Ok(count) => {
            // One line per refresh — fires once at startup and once per explicit
            // config write, NOT on data-watch events (the farm is deliberately
            // not wired to the watch hub, so it can't loop the way the legacy
            // Node farm did when its refresh was bound to every watcher event).
            tracing::info!("plan farm: warmed {count} diagram plan(s) incrementally");
        }
        Err(err) => {
            tracing::warn!("plan farm warm failed: {err}");
            // Leave the farm as-is on error.
        }
    }
}

/// Lookup a plan by its sha256 hash.  Returns `Some(plan_json)` on hit.
pub fn farm_lookup(farm: &Farm, hash: &str) -> Option<String> {
    farm.read()
        .expect("farm read lock")
        .get(hash)
        .map(|e| e.plan_json.clone())
}

/// The resolved paths the server uses.
pub struct ServePaths {
    pub data_dir: PathBuf,
    pub dist_dir: PathBuf,
}
