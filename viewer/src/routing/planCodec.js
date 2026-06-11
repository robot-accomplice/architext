// JSON codec for planned diagrams. The serve-side precompute farm stores plans
// as JSON and the viewer rehydrates them before rendering, so the collection
// fields (Maps/Set) need an explicit, stable wire shape. Handled strictly by
// known field name — an unexpected shape should fail loudly in tests rather
// than round-trip silently wrong.

const MAP_FIELDS = ["laneIndexByNode", "rowIndexByNode", "nodeRects", "routes", "labelBoxes"];
const SET_FIELDS = ["visibleNodeIds"];

export function serializePlan(plan) {
  const { positionFor, ...rest } = plan;
  const wire = { ...rest };
  for (const field of MAP_FIELDS) {
    if (!(rest[field] instanceof Map)) throw new Error(`serializePlan: expected Map for ${field}`);
    wire[field] = Array.from(rest[field].entries());
  }
  for (const field of SET_FIELDS) {
    if (!(rest[field] instanceof Set)) throw new Error(`serializePlan: expected Set for ${field}`);
    wire[field] = Array.from(rest[field]);
  }
  return wire;
}

export function deserializePlan(wire) {
  const plan = { ...wire };
  for (const field of MAP_FIELDS) {
    if (!Array.isArray(wire[field])) throw new Error(`deserializePlan: expected entries array for ${field}`);
    plan[field] = new Map(wire[field]);
  }
  for (const field of SET_FIELDS) {
    if (!Array.isArray(wire[field])) throw new Error(`deserializePlan: expected array for ${field}`);
    plan[field] = new Set(wire[field]);
  }
  return plan;
}
