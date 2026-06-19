// Pure side-selection logic for decision-diamond branches, extracted from main.tsx
// so it is unit-testable (the corpus fitness gate can't see decision rendering,
// which let a latent branch-routing bug ship unexercised until the first decision
// example existed).
//
// A decision step (kind:"decision") A->D renders a diamond at D; each outcome
// branch leaves the diamond on `branchSide` and enters its target on `branchEndSide`.

/** @typedef {"left"|"right"|"top"|"bottom"} RouteSide */

export function nodeLanePosition(view, nodeId) {
  for (let laneIndex = 0; laneIndex < view.lanes.length; laneIndex += 1) {
    const rowIndex = view.lanes[laneIndex].nodeIds.indexOf(nodeId);
    if (rowIndex >= 0) return { laneIndex, rowIndex };
  }
  return null;
}

/** @returns {RouteSide} */
export function oppositeSide(side) {
  if (side === "left") return "right";
  if (side === "right") return "left";
  if (side === "top") return "bottom";
  return "top";
}

// Which side of the diamond a branch toward `targetNodeId` leaves from.
// Forward branches FAN vertically: a target below the decision row leaves the
// bottom tip, others the right tip. Without this, every forward branch mounted
// the single right tip (fixedPorts) and shared one segment, forcing the lower
// branch to dogleg/staircase around it. Fanning keeps the diamond in place (it
// can't move — the gutter carries other traffic).
/** @returns {RouteSide} */
export function preferredDecisionBranchSide(view, decisionNode, targetNodeId) {
  const target = nodeLanePosition(view, targetNodeId);
  if (!target) return "right";
  const deltaLane = target.laneIndex - decisionNode.laneIndex;
  const deltaRow = target.rowIndex - decisionNode.rowIndex;
  if (deltaLane > 0) return deltaRow > 0 ? "bottom" : "right";
  if (deltaLane < 0) return "left";
  if (deltaRow < 0) return "left";
  if (deltaRow > 0) return "bottom";
  return "right";
}

// Which face of the target the branch enters — the side FACING the decision's
// column. A forward target (next column) is entered on its left face, a backward
// target on its right; same-lane targets keep their dedicated faces. (The old
// code mounted the far face for right-start branches, forcing a wrap-around
// overshoot.)
/** @returns {RouteSide} */
export function preferredDecisionBranchEndSide(view, decisionNode, targetNodeId, startSide) {
  const target = nodeLanePosition(view, targetNodeId);
  if (target) {
    if (target.laneIndex > decisionNode.laneIndex) return "left";
    if (target.laneIndex < decisionNode.laneIndex) return "right";
    if (target.rowIndex < decisionNode.rowIndex) return "left";
    if (target.rowIndex > decisionNode.rowIndex) return "top";
  }
  return oppositeSide(startSide);
}
