//! WASM boundary. `plan` takes the JSON plan-input and returns the JSON plan,
//! matching what /api/plan returns today. Native callers use `plan_value`.
use crate::model::Plan;

/// Native-facing entry: takes a parsed input value, returns a Plan. For Phase 1A
/// this is an echo that returns a baked plan fixture; Phase 1B implements routing.
pub fn plan_value(_input: &serde_json::Value) -> Plan {
    let raw = include_str!("../tests/fixtures/plan-sample.json");
    serde_json::from_str(raw).expect("baked fixture")
}

#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

/// Browser entry: JSON in, JSON out. Mirrors the /api/plan body contract.
#[cfg(feature = "wasm")]
#[wasm_bindgen]
pub fn plan(input_json: &str) -> Result<String, JsError> {
    let input: serde_json::Value =
        serde_json::from_str(input_json).map_err(|e| JsError::new(&e.to_string()))?;
    let plan = plan_value(&input);
    serde_json::to_string(&plan).map_err(|e| JsError::new(&e.to_string()))
}

#[cfg(test)]
mod wasm_tests {
    use super::plan_value;

    #[test]
    fn echo_returns_a_plan_with_routes() {
        let input = serde_json::json!({});
        let plan = plan_value(&input);
        assert!(!plan.routes.is_empty());
    }
}
