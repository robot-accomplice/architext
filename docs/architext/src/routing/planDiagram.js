import { routeEdges } from "./routeEdges.js";

function estimatedLabelBox(route, relationship) {
  return labelBoxAt(route.labelX, route.labelY, relationship);
}

function labelBoxAt(x, y, relationship) {
  const text = relationship.label ?? relationship.id ?? "";
  const width = Math.max(24, Math.min(180, text.length * 6 + 12));
  const height = relationship.relationshipType === "flow" || relationship.stepId ? 24 : 18;
  return {
    x: x - width / 2,
    y: y - height / 2,
    width,
    height
  };
}

function rectsOverlap(a, b, padding = 0) {
  return (
    a.x < b.x + b.width + padding &&
    a.x + a.width > b.x - padding &&
    a.y < b.y + b.height + padding &&
    a.y + a.height > b.y - padding
  );
}

function labelPlacementCandidates(route) {
  const offsets = [];
  for (const y of [0, -24, 24, -48, 48, -72, 72, -96, 96, -120, 120, -144, 144, -168, 168, -192, 192]) {
    for (const x of [0, 36, -36, 64, -64, 96, -96, 128, -128]) {
      offsets.push([x, y]);
    }
  }
  return offsets
    .sort((a, b) => Math.hypot(a[0], a[1]) - Math.hypot(b[0], b[1]))
    .map(([x, y]) => ({ x: route.labelX + x, y: route.labelY + y }));
}

function placeLabel(route, relationship, nodeRects, placedLabels, canvasWidth, canvasHeight) {
  const candidates = labelPlacementCandidates(route);
  const scored = candidates.map((candidate, index) => {
    const box = labelBoxAt(candidate.x, candidate.y, relationship);
    const qualityCosts = {
      labelMovementCost: Math.hypot(candidate.x - route.labelX, candidate.y - route.labelY),
      labelSearchOrderCost: index * 4,
      labelBoundaryCost: 0,
      labelNodeConflictCost: 0,
      labelConflictCost: 0
    };
    if (box.x < 8 || box.y < 8 || box.x + box.width > canvasWidth - 8 || box.y + box.height > canvasHeight - 8) {
      qualityCosts.labelBoundaryCost += 100000;
    }
    for (const [nodeId, rect] of nodeRects) {
      if (nodeId === relationship.from || nodeId === relationship.to) continue;
      if (rectsOverlap(box, rect, 4)) qualityCosts.labelNodeConflictCost += 80000;
    }
    for (const placed of placedLabels) {
      if (rectsOverlap(box, placed, 2)) qualityCosts.labelConflictCost += 20000;
    }
    return {
      candidate,
      box,
      cost: Object.values(qualityCosts).reduce((sum, value) => sum + value, 0),
      qualityCosts
    };
  });
  return scored.sort((a, b) => a.cost - b.cost)[0];
}

export function planDiagram(input) {
  const nodeWidth = input.nodeWidth;
  const nodeHeight = input.nodeHeight;
  const laneWidth = input.laneWidth;
  const rowGap = input.rowGap;
  const marginX = input.marginX;
  const marginY = input.marginY;
  const visibleNodeIds = new Set(input.visibleNodeIds);
  const laneIndexByNode = new Map();
  const rowIndexByNode = new Map();

  input.view.lanes.forEach((lane, laneIndex) => {
    lane.nodeIds.forEach((nodeId, rowIndex) => {
      if (!visibleNodeIds.has(nodeId)) return;
      laneIndexByNode.set(nodeId, laneIndex);
      rowIndexByNode.set(nodeId, rowIndex);
    });
  });

  const maxRows = Math.max(...input.view.lanes.map((lane) => lane.nodeIds.filter((nodeId) => visibleNodeIds.has(nodeId)).length), 1);
  const canvasWidth = Math.max(input.minCanvasWidth, marginX * 2 + input.view.lanes.length * laneWidth + input.canvasExtraWidth);
  const canvasHeight = Math.max(input.minCanvasHeight, marginY + maxRows * rowGap + input.canvasExtraHeight);
  const positionFor = (nodeId) => ({
    x: marginX + (laneIndexByNode.get(nodeId) ?? 0) * laneWidth,
    y: marginY + (rowIndexByNode.get(nodeId) ?? 0) * rowGap
  });
  const nodeRects = new Map(Array.from(visibleNodeIds).map((nodeId) => {
    const position = positionFor(nodeId);
    return [
      nodeId,
      {
        x: position.x,
        y: position.y,
        width: nodeWidth,
        height: nodeHeight
      }
    ];
  }));
  const routes = routeEdges({
    relationships: input.relationships,
    visibleNodeIds,
    nodeRects,
    laneIndexByNode,
    rowIndexByNode,
    canvasWidth,
    canvasHeight,
    marginY,
    style: input.style,
    stats: input.stats
  });
  const relationshipsById = new Map(input.relationships.map((relationship) => [relationship.id, relationship]));
  const plannedRoutes = new Map();
  const labelBoxes = new Map();
  const placedLabels = [];

  for (const [relationshipId, route] of routes) {
    const relationship = relationshipsById.get(relationshipId);
    if (relationship) {
      const labelPlacement = placeLabel(route, relationship, nodeRects, placedLabels, canvasWidth, canvasHeight);
      const labelQualityCosts = {
        ...route.qualityCosts,
        labelMovementCost: 0,
        labelSearchOrderCost: 0,
        labelBoundaryCost: 0,
        labelNodeConflictCost: 0,
        labelConflictCost: 0,
        ...labelPlacement.qualityCosts
      };
      const plannedRoute = {
        ...route,
        labelX: labelPlacement.candidate.x,
        labelY: labelPlacement.candidate.y,
        qualityCosts: labelQualityCosts,
        cost: Object.values(labelQualityCosts).reduce((sum, value) => sum + value, 0)
      };
      plannedRoutes.set(relationshipId, plannedRoute);
      labelBoxes.set(relationshipId, labelPlacement.box);
      placedLabels.push(labelPlacement.box);
    } else {
      plannedRoutes.set(relationshipId, route);
    }
  }

  const warnings = [];
  for (const [relationshipId, route] of plannedRoutes) {
    for (const warning of route.warnings ?? []) {
      warnings.push({ ...warning, relationshipId });
    }
  }
  for (const [relationshipId, labelBox] of labelBoxes) {
    const relationship = relationshipsById.get(relationshipId);
    for (const [nodeId, rect] of nodeRects) {
      if (nodeId === relationship?.from || nodeId === relationship?.to) continue;
      if (rectsOverlap(labelBox, rect, 4)) {
        warnings.push({
          code: "label-over-node",
          message: "Route label overlaps a non-endpoint node.",
          relationshipId,
          nodeId
        });
      }
    }
  }
  const labelEntries = [...labelBoxes];
  for (let index = 0; index < labelEntries.length; index += 1) {
    const [relationshipId, labelBox] = labelEntries[index];
    for (let otherIndex = index + 1; otherIndex < labelEntries.length; otherIndex += 1) {
      const [otherRelationshipId, otherLabelBox] = labelEntries[otherIndex];
      if (rectsOverlap(labelBox, otherLabelBox, 2)) {
        warnings.push({
          code: "label-over-label",
          message: "Route label overlaps another route label.",
          relationshipId,
          otherRelationshipId
        });
      }
    }
  }

  return {
    canvasWidth,
    canvasHeight,
    nodeWidth,
    nodeHeight,
    laneWidth,
    rowGap,
    marginX,
    marginY,
    visibleNodeIds,
    laneIndexByNode,
    rowIndexByNode,
    nodeRects,
    routes: plannedRoutes,
    labelBoxes,
    warnings,
    positionFor
  };
}
