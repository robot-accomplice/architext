// The Rust model must deserialize the JS planCodec wire shape and re-serialize to
// the same logical structure. We assert structural round-trip: parse -> serialize
// -> parse, and compare the two parsed values (key order in entries arrays is
// preserved by IndexMap).
use architext_routing::model::Plan;

#[test]
fn roundtrips_js_wire_shape() {
    let raw = include_str!("fixtures/plan-sample.json");
    let plan: Plan = serde_json::from_str(raw).expect("deserialize JS wire shape");
    let reserialized = serde_json::to_string(&plan).expect("serialize");
    let plan2: Plan = serde_json::from_str(&reserialized).expect("re-deserialize");
    assert_eq!(plan, plan2, "round-trip changed the plan structurally");
    assert!(!plan.routes.is_empty(), "fixture should contain routes");
}
