import assert from "node:assert/strict";
import test from "node:test";
import { compareByRoutePriority } from "../viewer/src/routing/routeScoring.js";

// A baseline "clean" candidate with every cascade metric at zero.
function candidate(overrides = {}) {
  return {
    collisions: 0,
    endpointNodeTraversals: 0,
    selfOverlappingSegments: 0,
    selfOverlapLength: 0,
    repeatedCrossings: 0,
    semanticSurfaceMismatchCount: 0,
    blockedPrimarySurfaceUseCount: 0,
    surfaceDirectionMismatchCount: 0,
    sameLaneExteriorMismatchCount: 0,
    paddedCollisions: 0,
    sharedSegments: 0,
    sharedSegmentLength: 0,
    crossings: 0,
    bends: 2,
    qualityCosts: {},
    cost: 0,
    ...overrides
  };
}

// A short interior route that crosses two other routes should beat a long
// perimeter-fallback detour that crosses nothing. A perimeter detour is a worse
// outcome than a couple of honest crossings.
test("a short route with crossings beats a long perimeter-fallback detour", () => {
  const interiorWithCrossings = candidate({ crossings: 2, cost: 5000 });
  const perimeterDetour = candidate({
    crossings: 0,
    qualityCosts: { perimeterFallbackCost: 12000 },
    cost: 35000
  });

  assert.ok(
    compareByRoutePriority(interiorWithCrossings, perimeterDetour) < 0,
    "the crossing route should sort before the perimeter detour"
  );
});
