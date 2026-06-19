// Reciprocal flow pairs matched by displayIndex adjacency.
//
// Match each request, in flow order, with the nearest not-yet-paired opposite-direction edge
// that follows it. A node pair carrying 2+ round trips (e.g. query AND ingest between the same
// two stores) must pair each request with ITS own return, not the other trip's return — pairing
// every opposite-direction combo mis-pairs them (and, for crossing classification, roughly
// doubles the count). This is the canonical pairing shared by the route-crossing diagnostics and
// the straightening pass; `reciprocalParallelMoves` in routeMountModel.js applies the same rule
// inline against its live route map, so keep the three in sync if the rule changes.
export function reciprocalPairsByAdjacency(relationships) {
  const byPair = new Map();
  for (const relationship of relationships) {
    const key = [relationship.from, relationship.to].slice().sort().join("\u0000");
    if (!byPair.has(key)) byPair.set(key, []);
    byPair.get(key).push(relationship);
  }
  const pairs = [];
  for (const group of byPair.values()) {
    const sorted = group.slice().sort((a, b) => (a.displayIndex ?? 0) - (b.displayIndex ?? 0));
    const paired = new Set();
    for (const request of sorted) {
      if (paired.has(request.id)) continue;
      const ret = sorted.find((other) =>
        !paired.has(other.id) && other.id !== request.id &&
        other.from === request.to && other.to === request.from &&
        (other.displayIndex ?? 0) >= (request.displayIndex ?? 0));
      if (!ret) continue;
      paired.add(request.id);
      paired.add(ret.id);
      pairs.push([request.id, ret.id]);
    }
  }
  return pairs;
}
