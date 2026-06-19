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
use architext_routing::precompute::enumerate_flow_plan_requests;

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
    let farm: Farm = Arc::new(RwLock::new(HashMap::new()));
    refresh_farm(&farm, data_dir);
    farm
}

/// (Re)populate the farm from `data_dir`.  Swaps in the new map atomically.
pub fn refresh_farm(farm: &Farm, data_dir: &Path) {
    let config = resolve_diagram_config_defaults();
    match enumerate_flow_plan_requests(data_dir, &config) {
        Ok(entries) => {
            let mut map = HashMap::new();
            for entry in entries {
                map.insert(entry.hash, PlanEntry { plan_json: entry.plan_json });
            }
            let count = map.len();
            *farm.write().expect("farm write lock") = map;
            // One line per refresh — fires once at startup and once per explicit
            // config write, NOT on data-watch events (the farm is deliberately
            // not wired to the watch hub, so it can't loop the way the legacy
            // Node farm did when its refresh was bound to every watcher event).
            tracing::info!("plan farm: precomputed {count} diagram plan(s)");
        }
        Err(err) => {
            tracing::warn!("plan farm enumeration failed: {err}");
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
