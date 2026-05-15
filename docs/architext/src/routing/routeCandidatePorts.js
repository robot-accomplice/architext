import { portCandidatesFor, SIDES, tangentVector } from "./routePorts.js";

export function candidatePorts(fromRect, toRect, startSide, endSide, endpointOffsets, scope = "cheap") {
  const fromCenter = {
    x: fromRect.x + fromRect.width / 2,
    y: fromRect.y + fromRect.height / 2
  };
  const toCenter = {
    x: toRect.x + toRect.width / 2,
    y: toRect.y + toRect.height / 2
  };
  const startTangent = tangentVector(startSide);
  const endTangent = tangentVector(endSide);
  const targetAlignedStartOffset = startTangent.y !== 0 ? toCenter.y - fromCenter.y : toCenter.x - fromCenter.x;
  const targetAlignedEndOffset = endTangent.y !== 0 ? fromCenter.y - toCenter.y : fromCenter.x - toCenter.x;

  const sharedOffsets = scope === "grid"
    ? [
        0,
        endpointOffsets.from,
        endpointOffsets.to
      ]
    : [
        0,
        endpointOffsets.from,
        endpointOffsets.to,
        endpointOffsets.from + targetAlignedStartOffset,
        endpointOffsets.to + targetAlignedEndOffset
      ];

  return {
    starts: portCandidatesFor(fromRect, startSide, sharedOffsets),
    ends: portCandidatesFor(toRect, endSide, sharedOffsets)
  };
}

export function sidePairsFor(fromRect, toRect) {
  const fromCenter = {
    x: fromRect.x + fromRect.width / 2,
    y: fromRect.y + fromRect.height / 2
  };
  const toCenter = {
    x: toRect.x + toRect.width / 2,
    y: toRect.y + toRect.height / 2
  };
  const horizontal = toCenter.x >= fromCenter.x ? ["right", "left"] : ["left", "right"];
  const vertical = toCenter.y >= fromCenter.y ? ["bottom", "top"] : ["top", "bottom"];
  const pairs = [
    Math.abs(toCenter.x - fromCenter.x) >= Math.abs(toCenter.y - fromCenter.y) ? horizontal : vertical,
    Math.abs(toCenter.x - fromCenter.x) >= Math.abs(toCenter.y - fromCenter.y) ? vertical : horizontal,
    ["left", "right"],
    ["right", "left"],
    ["top", "bottom"],
    ["bottom", "top"],
    ["left", "left"],
    ["right", "right"],
    ["top", "top"],
    ["bottom", "bottom"]
  ];
  const seen = new Set();
  return pairs.filter(([startSide, endSide]) => {
    const key = `${startSide}:${endSide}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

export function allSidePairs() {
  return SIDES.flatMap((startSide) => SIDES.map((endSide) => [startSide, endSide]));
}

export function portPairsFor(ports) {
  const pairs = [];
  const seen = new Set();
  const add = (start, end) => {
    if (!start || !end) return;
    const key = `${start.anchor.x},${start.anchor.y}:${end.anchor.x},${end.anchor.y}`;
    if (seen.has(key)) return;
    seen.add(key);
    pairs.push([start, end]);
  };
  for (const start of ports.starts) {
    for (const end of ports.ends) {
      add(start, end);
    }
  }
  return pairs;
}
