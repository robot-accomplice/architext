import { MOUNT_COST, MIN_LEGIBLE_GAP } from "./routeConstants.js";

// positions: mount coordinates along the surface axis, expressed as distance
// from the surface start (0..length). count: total mounts on the surface.
export function surfaceSpacingCost(positions, length, count) {
  const sorted = [...positions].sort((a, b) => a - b);
  let cost = 0;
  // Deviation from the ideal evenly-spread slots (mounts may run to corners).
  sorted.forEach((pos, index) => {
    const ideal = ((index + 1) / (count + 1)) * length;
    cost += Math.abs(pos - ideal) * MOUNT_COST.spacingDeviation;
  });
  // Steep sub-penalty when any adjacent gap (incl. surface ends) is sub-legible.
  const guards = [0, ...sorted, length];
  for (let i = 0; i < guards.length - 1; i += 1) {
    const gap = guards[i + 1] - guards[i];
    if (gap < MIN_LEGIBLE_GAP) cost += (MIN_LEGIBLE_GAP - gap) * MOUNT_COST.cramped;
  }
  return cost;
}
