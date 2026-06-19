import { segmentIntersectsRect } from "./routeGeometry.js";

function rectCenter(rect) {
  return {
    x: rect.x + rect.width / 2,
    y: rect.y + rect.height / 2
  };
}

function relationshipRole(relationship) {
  if (relationship.kind) return relationship.kind;
  if (relationship.returnOf) return "return";
  if (relationship.outcome) return "decision-outcome";
  return "process";
}

function laneDirection(fromLaneIndex, toLaneIndex) {
  if (fromLaneIndex === toLaneIndex) return "same";
  return fromLaneIndex < toLaneIndex ? "forward" : "backward";
}

function rowDirection(fromRowIndex, toRowIndex) {
  if (fromRowIndex === toRowIndex) return "same";
  return fromRowIndex < toRowIndex ? "down" : "up";
}

export function expectedFacingSides(fromRect, toRect) {
  const from = rectCenter(fromRect);
  const to = rectCenter(toRect);
  if (Math.abs(to.x - from.x) >= Math.abs(to.y - from.y)) {
    return to.x >= from.x
      ? { source: "right", target: "left" }
      : { source: "left", target: "right" };
  }
  return to.y >= from.y
    ? { source: "bottom", target: "top" }
    : { source: "top", target: "bottom" };
}

export function deriveRouteIntent(input) {
  const lane = laneDirection(input.fromLaneIndex, input.toLaneIndex);
  const row = rowDirection(input.fromRowIndex, input.toRowIndex);
  const expected = expectedRouteSides(input.fromRect, input.toRect, lane, row);
  return {
    relationshipId: input.relationship.id,
    role: relationshipRole(input.relationship),
    returnOf: input.relationship.returnOf,
    outcome: input.relationship.outcome,
    laneDirection: lane,
    rowDirection: row,
    expectedSourceSide: expected.source,
    expectedTargetSide: expected.target
  };
}

export function semanticSurfaceOptions({ expectedSides, relationship, fromRect, toRect, blockerRects = [], canvasWidth, canvasHeight }) {
  const source = new Set([expectedSides.source]);
  const target = new Set([expectedSides.target]);
  const horizontalIntent = (expectedSides.source === "right" && expectedSides.target === "left") ||
    (expectedSides.source === "left" && expectedSides.target === "right");
  const verticalIntent = (expectedSides.source === "bottom" && expectedSides.target === "top") ||
    (expectedSides.source === "top" && expectedSides.target === "bottom");
  // A perpendicular (horizontal) escape is a cheap local hop, worth it for a single
  // blocker. A sideways gutter escape (vertical intent) is a full detour, only worth
  // it when staying facing would dogleg around two or more intermediaries.
  const verticalGutterEscape = verticalIntent &&
    corridorBlockerCount(fromRect, toRect, blockerRects) >= 1;
  if ((horizontalIntent || verticalGutterEscape) && semanticPrimaryCorridorBlocked({ relationship, fromRect, toRect, blockerRects }, expectedSides)) {
    const sourceEscape = escapeSideFor(fromRect, expectedSides.source, canvasWidth, canvasHeight);
    const targetEscape = escapeSideFor(toRect, expectedSides.target, canvasWidth, canvasHeight);
    const isReturn = relationship?.kind === "return" || Boolean(relationship?.returnOf);
    // Coplanar = same row for horizontal intent, same column for vertical intent.
    const coplanar = horizontalIntent ? fromRect.y === toRect.y : fromRect.x === toRect.x;
    if (coplanar) {
      // Same-plane endpoints separated by an intermediary: both ends escape to a
      // parallel (perpendicular) surface so the edge routes over/under the blocker
      // instead of forcing one end to exit into the blocked corridor and dogleg.
      // blockedPrimarySurfaceUseCount then steers each end off its blocked facing side.
      if (sourceEscape) source.add(sourceEscape);
      if (targetEscape) target.add(targetEscape);
    } else {
      // Blocker sits near one end only: escape the end that arrives past it.
      if (isReturn && sourceEscape) source.add(sourceEscape);
      if (!isReturn && targetEscape) target.add(targetEscape);
    }
  }
  return { source, target };
}

function corridorBlockerCount(fromRect, toRect, blockerRects = []) {
  if (!fromRect || !toRect) return 0;
  const padding = 12;
  const loY = Math.min(fromRect.y + fromRect.height, toRect.y + toRect.height);
  const hiY = Math.max(fromRect.y, toRect.y);
  const left = Math.max(fromRect.x, toRect.x) - padding;
  const right = Math.min(fromRect.x + fromRect.width, toRect.x + toRect.width) + padding;
  if (hiY <= loY || right <= left) return 0;
  return blockerRects.filter((blocker) => (
    blocker.y >= loY && blocker.y + blocker.height <= hiY &&
    blocker.x < right && blocker.x + blocker.width > left
  )).length;
}

function semanticPrimaryCorridorBlocked(context, expectedSides) {
  if (!context.relationship?.relationshipType && !context.relationship?.kind && !context.relationship?.returnOf && !context.relationship?.outcome && !context.relationship?.stepId && !context.relationship?.flowId) return false;
  if (!context.fromRect || !context.toRect || !context.blockerRects) return false;
  const source = rectCenter(context.fromRect);
  const target = rectCenter(context.toRect);
  const horizontalIntent = (expectedSides.source === "right" && expectedSides.target === "left") ||
    (expectedSides.source === "left" && expectedSides.target === "right");
  const verticalIntent = (expectedSides.source === "bottom" && expectedSides.target === "top") ||
    (expectedSides.source === "top" && expectedSides.target === "bottom");
  if (!horizontalIntent && !verticalIntent) return false;
  return context.blockerRects.some((rect) => (
    primarySurfaceBandBlocked(context.fromRect, context.toRect, rect, horizontalIntent)
      || segmentIntersectsRect(source, target, rect, 12)
  ));
}

function primarySurfaceBandBlocked(fromRect, toRect, blocker, horizontalIntent) {
  const padding = 12;
  if (horizontalIntent) {
    const left = Math.min(fromRect.x + fromRect.width, toRect.x + toRect.width);
    const right = Math.max(fromRect.x, toRect.x);
    const top = Math.max(fromRect.y, toRect.y) - padding;
    const bottom = Math.min(fromRect.y + fromRect.height, toRect.y + toRect.height) + padding;
    if (right <= left || bottom <= top) return false;
    return blocker.x < right && blocker.x + blocker.width > left && blocker.y < bottom && blocker.y + blocker.height > top;
  }
  const top = Math.min(fromRect.y + fromRect.height, toRect.y + toRect.height);
  const bottom = Math.max(fromRect.y, toRect.y);
  const left = Math.max(fromRect.x, toRect.x) - padding;
  const right = Math.min(fromRect.x + fromRect.width, toRect.x + toRect.width) + padding;
  if (bottom <= top || right <= left) return false;
  return blocker.y < bottom && blocker.y + blocker.height > top && blocker.x < right && blocker.x + blocker.width > left;
}

function escapeSideFor(rect, expectedSide, canvasWidth, canvasHeight) {
  if (!rect) return "";
  const center = rectCenter(rect);
  if (expectedSide === "left" || expectedSide === "right") {
    if (!canvasHeight) return center.y < rect.height ? "bottom" : "top";
    return center.y < canvasHeight / 2 ? "bottom" : "top";
  }
  // Vertical intent escapes sideways toward the nearer outer gutter (left for the
  // left half, right for the right half), where free space lives, rather than
  // inward toward the crowded center.
  if (!canvasWidth) return center.x < rect.width ? "left" : "right";
  return center.x < canvasWidth / 2 ? "left" : "right";
}

export function expectedRouteSides(fromRect, toRect, laneDirectionValue, rowDirectionValue) {
  if (laneDirectionValue === "forward") return { source: "right", target: "left" };
  if (laneDirectionValue === "backward") return { source: "left", target: "right" };
  if (rowDirectionValue === "down") return { source: "bottom", target: "top" };
  if (rowDirectionValue === "up") return { source: "top", target: "bottom" };
  return expectedFacingSides(fromRect, toRect);
}
