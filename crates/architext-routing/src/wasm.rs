//! WASM boundary. `plan` takes the JSON plan-input and returns the JSON plan,
//! matching what /api/plan returns today. Native callers use `plan_value`.
use crate::model::Plan;
use crate::plan_diagram::{plan_diagram, PlanDiagramInput};

/// Native-facing entry: deserializes the wire-form input and calls `plan_diagram`.
/// Returns a `Plan` struct ready for serialization.
pub fn plan_value(input: &serde_json::Value) -> Result<Plan, String> {
    let diagram_input: PlanDiagramInput = serde_json::from_value(input.clone())
        .map_err(|e| format!("deserialize plan input: {e}"))?;
    Ok(plan_diagram(&diagram_input))
}

#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

/// Browser entry: JSON in, JSON out. Mirrors the /api/plan body contract.
#[cfg(feature = "wasm")]
#[wasm_bindgen]
pub fn plan(input_json: &str) -> Result<String, JsError> {
    let input: serde_json::Value =
        serde_json::from_str(input_json).map_err(|e| JsError::new(&e.to_string()))?;
    let plan = plan_value(&input).map_err(|e| JsError::new(&e))?;
    serde_json::to_string(&plan).map_err(|e| JsError::new(&e.to_string()))
}

#[cfg(test)]
mod wasm_tests {
    use super::plan_value;

    #[test]
    fn plan_value_returns_a_plan_with_routes() {
        // Use the golden fixture (it's a valid PlanDiagramInput)
        let input_str = include_str!("../tests/fixtures/plan-diagram-input-fresh-install.json");
        let input: serde_json::Value = serde_json::from_str(input_str).unwrap();
        let plan = plan_value(&input).expect("plan_value");
        assert!(!plan.routes.is_empty());
    }
}
