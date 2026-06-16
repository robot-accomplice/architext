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

    /// Regression: reroute callbacks (separate_close_parallel_routes, optimize_mount_assignments)
    /// must inherit lane/row indices from the PlannerContext, not pass None and fall back to 0.
    ///
    /// `detect-copied-files` connects architext-cli (lane 1, row 0) → target-repository (lane 3, row 0).
    /// viewer-runtime (lane 2, row 0) blocks the primary right→left corridor.
    ///
    /// The route exits the RIGHT side of architext-cli (x=526) and the optimizer's trySideMoves
    /// pass reroutes via the top gutter (y=67) to avoid the blocked corridor. The JavaScript
    /// reference produces: M 526 122 L 544 122 L 544 67 L 792 67 L 792 131 L 810 131
    ///
    /// Before the lane/row fix, reroutes passed None indices so semantic scoring misfired.
    /// After the fix, the route matches the JS reference: exiting x=526 (right side of architext-cli).
    #[test]
    fn detect_copied_files_routes_right_side_exit() {
        let input_str = include_str!(
            "../tests/fixtures/plan-diagram-input-copied-install-migration.json"
        );
        let input: serde_json::Value = serde_json::from_str(input_str).unwrap();
        let plan = plan_value(&input).expect("plan_value");

        let route = plan
            .routes
            .get("detect-copied-files")
            .expect("detect-copied-files route must be present");

        // JS reference: M 526 122 L 544 122 L 544 67 L 792 67 L 792 131 L 810 131
        // architext-cli right edge x=526. A top-side exit would start at y=104 (top edge).
        let first_pt = route.points.first().expect("route must have points");
        assert_eq!(
            first_pt.x as i64, 526,
            "detect-copied-files must exit architext-cli's RIGHT side (x=526). \
             Got start_x={} — JS reference is x=526.",
            first_pt.x
        );
    }
}
