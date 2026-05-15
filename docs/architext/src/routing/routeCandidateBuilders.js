import {
  bendCount,
  clamp,
  lineSamples,
  pointAtDistance,
  routeLength,
  sampleCubic,
  sampleLine,
  segmentIntersectsRect,
  shallowJogCount,
  unitVector
} from "./routeGeometry.js";
import { pathToSvg, simplifyOrthogonalPoints } from "./routeRendering.js";
import { sideVector } from "./routePorts.js";
import { withQualityCosts } from "./routeScoring.js";
import { withReadableLabel } from "./routeLabels.js";
import { createMinHeap } from "./priorityQueue.js";
import { CORRIDOR_PADDING } from "./routeCorridors.js";

function monotonicBacktrackCost(points, fromRect, toRect) {
  const fromCenter = {
    x: fromRect.x + fromRect.width / 2,
    y: fromRect.y + fromRect.height / 2
  };
  const toCenter = {
    x: toRect.x + toRect.width / 2,
    y: toRect.y + toRect.height / 2
  };
  const xDirection = Math.sign(toCenter.x - fromCenter.x);
  const yDirection = Math.sign(toCenter.y - fromCenter.y);
  let cost = 0;
  for (let index = 0; index < points.length - 1; index += 1) {
    const start = points[index];
    const end = points[index + 1];
    const dx = end.x - start.x;
    const dy = end.y - start.y;
    if (xDirection !== 0 && Math.sign(dx) === -xDirection) cost += Math.abs(dx) * 18;
    if (yDirection !== 0 && Math.sign(dy) === -yDirection) cost += Math.abs(dy) * 18;
  }
  return cost;
}

export function createRouteCandidateFactory(context) {
  const {
    blockerRects,
    canvasHeight,
    canvasWidth,
    rectFor,
    routeQualityFromSamples,
    stats
  } = context;

  const gridRoute = (relationship, fromId, toId, startSide, endSide, routeOffset, usedRoutes, startPort, endPort) => {
    if (stats) stats.gridRouteCalls = (stats.gridRouteCalls ?? 0) + 1;
    const start = startPort.port;
    const end = endPort.port;
    const fromRect = rectFor(fromId);
    const toRect = rectFor(toId);
    const blockers = blockerRects(fromId, toId);
    const padding = CORRIDOR_PADDING;
    const minX = 24;
    const maxX = canvasWidth - 24;
    const minY = 30;
    const maxY = canvasHeight - 24;
    const add = (set, value, min, max) => set.add(Math.min(max, Math.max(min, Math.round(value))));
    const xLines = new Set([Math.round(start.x), Math.round(end.x), minX, maxX]);
    const yLines = new Set([Math.round(start.y), Math.round(end.y), minY, maxY]);

    for (const rect of blockers) {
      add(xLines, rect.x - padding - routeOffset, minX, maxX);
      add(xLines, rect.x + rect.width + padding + routeOffset, minX, maxX);
      add(yLines, rect.y - padding - routeOffset, minY, maxY);
      add(yLines, rect.y + rect.height + padding + routeOffset, minY, maxY);
    }

    const xs = [...xLines].sort((a, b) => a - b);
    const ys = [...yLines].sort((a, b) => a - b);
    const points = [];
    const pointIndex = new Map();
    for (const x of xs) {
      for (const y of ys) {
        const key = `${x},${y}`;
        pointIndex.set(key, points.length);
        points.push({ x, y });
      }
    }

    const pointKey = (point) => `${Math.round(point.x)},${Math.round(point.y)}`;
    const startIndex = pointIndex.get(pointKey(start));
    const endIndex = pointIndex.get(pointKey(end));
    if (startIndex === undefined || endIndex === undefined) return null;

    const neighbors = Array.from({ length: points.length }, () => []);
    const horizontalBlockersByY = new Map(ys.map((y) => [
      y,
      blockers.filter((rect) => y > rect.y - padding && y < rect.y + rect.height + padding)
    ]));
    const verticalBlockersByX = new Map(xs.map((x) => [
      x,
      blockers.filter((rect) => x > rect.x - padding && x < rect.x + rect.width + padding)
    ]));
    const horizontalClear = (y, left, right) => {
      const minX = Math.min(left, right);
      const maxX = Math.max(left, right);
      return (horizontalBlockersByY.get(y) ?? []).every((rect) => maxX <= rect.x - padding || minX >= rect.x + rect.width + padding);
    };
    const verticalClear = (x, top, bottom) => {
      const minY = Math.min(top, bottom);
      const maxY = Math.max(top, bottom);
      return (verticalBlockersByX.get(x) ?? []).every((rect) => maxY <= rect.y - padding || minY >= rect.y + rect.height + padding);
    };

    for (const y of ys) {
      for (let index = 0; index < xs.length - 1; index += 1) {
        const a = pointIndex.get(`${xs[index]},${y}`);
        const b = pointIndex.get(`${xs[index + 1]},${y}`);
        if (horizontalClear(y, xs[index], xs[index + 1])) {
          const distance = Math.abs(xs[index + 1] - xs[index]);
          neighbors[a].push([b, distance]);
          neighbors[b].push([a, distance]);
        }
      }
    }
    for (const x of xs) {
      for (let index = 0; index < ys.length - 1; index += 1) {
        const a = pointIndex.get(`${x},${ys[index]}`);
        const b = pointIndex.get(`${x},${ys[index + 1]}`);
        if (verticalClear(x, ys[index], ys[index + 1])) {
          const distance = Math.abs(ys[index + 1] - ys[index]);
          neighbors[a].push([b, distance]);
          neighbors[b].push([a, distance]);
        }
      }
    }

    const distances = new Array(points.length).fill(Infinity);
    const previous = new Array(points.length).fill(-1);
    const visited = new Uint8Array(points.length);
    const queue = createMinHeap();
    distances[startIndex] = 0;
    queue.push({ index: startIndex, distance: 0 });

    while (queue.size > 0) {
      const nextItem = queue.pop();
      if (!nextItem || nextItem.distance !== distances[nextItem.index]) continue;
      const current = nextItem.index;
      if (current === endIndex) break;
      if (visited[current]) continue;
      visited[current] = 1;
      for (const [next, distance] of neighbors[current]) {
        if (visited[next]) continue;
        const turnPenalty = previous[current] >= 0
          ? (points[previous[current]].x !== points[current].x && points[current].x !== points[next].x) ||
            (points[previous[current]].y !== points[current].y && points[current].y !== points[next].y)
            ? 18
            : 0
          : 0;
        const nextDistance = distances[current] + distance + turnPenalty;
        if (nextDistance < distances[next]) {
          distances[next] = nextDistance;
          previous[next] = current;
          queue.push({ index: next, distance: nextDistance });
        }
      }
    }

    if (!Number.isFinite(distances[endIndex])) return null;

    const routePoints = [];
    for (let cursor = endIndex; cursor !== -1; cursor = previous[cursor]) {
      routePoints.unshift(points[cursor]);
    }
    const simplified = simplifyOrthogonalPoints([startPort.anchor, ...routePoints, endPort.anchor]);
    const samples = lineSamples(simplified);
    const label = samples[Math.floor(samples.length / 2)] ?? {
      x: (start.x + end.x) / 2,
      y: (start.y + end.y) / 2
    };
    const backtrackCost = monotonicBacktrackCost(simplified, fromRect, toRect);
    return withQualityCosts({
      d: pathToSvg(simplified),
      labelX: label.x,
      labelY: label.y,
      bends: bendCount(simplified),
      samples,
      points: simplified
    }, {
      ...routeQualityFromSamples(samples, label, fromId, toId, usedRoutes, relationship),
      pointCountCost: simplified.length * 24,
      bendCost: bendCount(simplified) * 420,
      doglegCost: shallowJogCount(simplified) * 14000,
      monotonicBacktrackCost: backtrackCost
    });
  };

  const splineCandidate = (relationship, fromId, toId, startSide, endSide, usedRoutes, startPort, endPort, pairIndex, curvatureOffset) => {
    const start = startPort.anchor;
    const end = endPort.anchor;
    const centerDistance = Math.hypot(end.x - start.x, end.y - start.y);
    const controlDistance = clamp(centerDistance * 0.32 + pairIndex * 12, 64, 190);
    const chord = unitVector(start, end);
    const normal = { x: -chord.y, y: chord.x };
    const startVector = sideVector(startSide);
    const endVector = sideVector(endSide);
    const direction = { x: end.x - start.x, y: end.y - start.y };
    const startDirection = startVector.x * direction.x + startVector.y * direction.y;
    const endDirection = endVector.x * direction.x + endVector.y * direction.y;
    const sideDirectionCost = (startDirection < 0 ? Math.abs(startDirection) * 260 : 0) +
      (endDirection > 0 ? Math.abs(endDirection) * 260 : 0);
    const controlA = {
      x: clamp(start.x + chord.x * controlDistance + normal.x * curvatureOffset, 24, canvasWidth - 24),
      y: clamp(start.y + chord.y * controlDistance + normal.y * curvatureOffset, 30, canvasHeight - 24)
    };
    const controlB = {
      x: clamp(end.x - chord.x * controlDistance + normal.x * curvatureOffset, 24, canvasWidth - 24),
      y: clamp(end.y - chord.y * controlDistance + normal.y * curvatureOffset, 30, canvasHeight - 24)
    };
    const samples = [start, ...sampleCubic(start, controlA, controlB, end, 32)];
    const label = pointAtDistance(samples, routeLength(samples) / 2) ?? {
      x: (start.x + end.x) / 2,
      y: (start.y + end.y) / 2
    };
    return withQualityCosts({
      d: `M ${start.x} ${start.y} C ${controlA.x} ${controlA.y} ${controlB.x} ${controlB.y} ${end.x} ${end.y}`,
      labelX: label.x,
      labelY: label.y,
      bends: 0,
      samples,
      points: [start, end],
      controls: [controlA, controlB],
      style: "spline"
    }, {
      ...routeQualityFromSamples(samples, label, fromId, toId, usedRoutes, relationship),
      lengthCost: routeLength(samples),
      pointCountCost: 48,
      directnessReward: -1400,
      splineSideDirectionCost: sideDirectionCost,
      splineStraightnessCost: Math.abs(curvatureOffset) < 1 ? 90000 : 0
    });
  };

  const perimeterRoute = (relationship, fromId, toId, side, routeOffset, usedRoutes, startPort, endPort) => {
    const start = startPort.port;
    const end = endPort.port;
    const gutter = side === "left"
      ? 24 + routeOffset
      : side === "right"
        ? canvasWidth - 24 - routeOffset
        : side === "top"
          ? 30 + routeOffset
          : canvasHeight - 24 - routeOffset;
    const points = side === "left" || side === "right"
      ? [
          startPort.anchor,
          start,
          { x: gutter, y: start.y },
          { x: gutter, y: end.y },
          end,
          endPort.anchor
        ]
      : [
          startPort.anchor,
          start,
          { x: start.x, y: gutter },
          { x: end.x, y: gutter },
          end,
          endPort.anchor
        ];
    const simplified = simplifyOrthogonalPoints(points);
    const samples = lineSamples(simplified);
    const label = samples[Math.floor(samples.length / 2)] ?? {
      x: (start.x + end.x) / 2,
      y: (start.y + end.y) / 2
    };
    return withQualityCosts({
      d: pathToSvg(simplified),
      labelX: label.x,
      labelY: label.y,
      bends: bendCount(simplified),
      samples,
      points: simplified
    }, {
      ...routeQualityFromSamples(samples, label, fromId, toId, usedRoutes, relationship),
      perimeterFallbackCost: 7000,
      perimeterLengthCost: routeLength(samples) * 8,
      pointCountCost: simplified.length * 24,
      bendCost: bendCount(simplified) * 420,
      doglegCost: shallowJogCount(simplified) * 14000
    });
  };

  const cornerPerimeterRoutes = (relationship, fromId, toId, routeOffset, usedRoutes, startPort, endPort) => {
    const boundaries = [
      { x: 24 + routeOffset, y: 30 + routeOffset },
      { x: canvasWidth - 24 - routeOffset, y: 30 + routeOffset },
      { x: 24 + routeOffset, y: canvasHeight - 24 - routeOffset },
      { x: canvasWidth - 24 - routeOffset, y: canvasHeight - 24 - routeOffset }
    ];

    const start = startPort.port;
    const end = endPort.port;
    return boundaries.flatMap((boundary) => [
      [
        startPort.anchor,
        start,
        { x: boundary.x, y: start.y },
        boundary,
        { x: boundary.x, y: end.y },
        end,
        endPort.anchor
      ],
      [
        startPort.anchor,
        start,
        { x: start.x, y: boundary.y },
        boundary,
        { x: end.x, y: boundary.y },
        end,
        endPort.anchor
      ]
    ]).map((points) => {
      const simplified = simplifyOrthogonalPoints(points);
      const samples = lineSamples(simplified);
      const start = simplified[0];
      const end = simplified[simplified.length - 1];
      const label = samples[Math.floor(samples.length / 2)] ?? {
        x: (start.x + end.x) / 2,
        y: (start.y + end.y) / 2
      };
      return withQualityCosts({
        d: pathToSvg(simplified),
        labelX: label.x,
        labelY: label.y,
        bends: bendCount(simplified),
        samples,
        points: simplified
      }, {
        ...routeQualityFromSamples(samples, label, fromId, toId, usedRoutes, relationship),
        perimeterFallbackCost: 12000,
        perimeterLengthCost: routeLength(samples) * 10,
        pointCountCost: simplified.length * 24,
        bendCost: bendCount(simplified) * 420,
        doglegCost: shallowJogCount(simplified) * 14000
      });
    });
  };

  const directPortCandidate = (relationship, fromId, toId, startSide, endSide, usedRoutes, startPort, endPort) => {
    const startVector = sideVector(startSide);
    const endVector = sideVector(endSide);
    const horizontal = startPort.port.y === endPort.port.y && startVector.y === 0 && endVector.y === 0;
    const vertical = startPort.port.x === endPort.port.x && startVector.x === 0 && endVector.x === 0;
    if (!horizontal && !vertical) return null;
    const points = simplifyOrthogonalPoints([startPort.anchor, startPort.port, endPort.port, endPort.anchor]);
    const samples = lineSamples(points);
    const label = samples[Math.floor(samples.length / 2)] ?? {
      x: (startPort.anchor.x + endPort.anchor.x) / 2,
      y: (startPort.anchor.y + endPort.anchor.y) / 2
    };
    return withQualityCosts({
      d: pathToSvg(points),
      labelX: label.x,
      labelY: label.y,
      bends: bendCount(points),
      samples,
      points
    }, {
      ...routeQualityFromSamples(samples, label, fromId, toId, usedRoutes, relationship),
      directnessReward: -2000,
      doglegCost: shallowJogCount(points) * 14000
    });
  };

  const straightCandidate = (relationship, fromId, toId, usedRoutes, startPort, endPort) => {
    const points = [startPort.anchor, endPort.anchor];
    const samples = sampleLine(points[0], points[1], 18);
    const label = samples[Math.floor(samples.length / 2)] ?? {
      x: (points[0].x + points[1].x) / 2,
      y: (points[0].y + points[1].y) / 2
    };
    return withReadableLabel(withQualityCosts({
      d: pathToSvg(points),
      labelX: label.x,
      labelY: label.y,
      bends: 0,
      samples,
      points,
      style: "straight"
    }, {
      ...routeQualityFromSamples(samples, label, fromId, toId, usedRoutes, relationship),
      lengthCost: routeLength(samples),
      pointCountCost: 24,
      directnessReward: -2200
    }));
  };

  const corridorCandidate = (relationship, fromId, toId, usedRoutes, startPort, endPort, corridor) => {
    const start = startPort.port;
    const end = endPort.port;
    const points = corridor.axis === "x"
      ? [
          startPort.anchor,
          start,
          { x: corridor.value, y: start.y },
          { x: corridor.value, y: end.y },
          end,
          endPort.anchor
        ]
      : [
          startPort.anchor,
          start,
          { x: start.x, y: corridor.value },
          { x: end.x, y: corridor.value },
          end,
          endPort.anchor
        ];
    const simplified = simplifyOrthogonalPoints(points);
    const blockers = blockerRects(fromId, toId);
    if (!blockers.every((rect) => simplified.slice(0, -1).every((point, index) => !segmentIntersectsRect(point, simplified[index + 1], rect, CORRIDOR_PADDING)))) {
      return null;
    }
    const samples = lineSamples(simplified);
    const label = samples[Math.floor(samples.length / 2)] ?? {
      x: (start.x + end.x) / 2,
      y: (start.y + end.y) / 2
    };
    return withQualityCosts({
      d: pathToSvg(simplified),
      labelX: label.x,
      labelY: label.y,
      bends: bendCount(simplified),
      samples,
      points: simplified
    }, {
      ...routeQualityFromSamples(samples, label, fromId, toId, usedRoutes, relationship),
      pointCountCost: simplified.length * 24,
      bendCost: bendCount(simplified) * 420,
      doglegCost: shallowJogCount(simplified) * 14000,
      monotonicBacktrackCost: monotonicBacktrackCost(simplified, rectFor(fromId), rectFor(toId))
    });
  };

  return {
    cornerPerimeterRoutes,
    corridorCandidate,
    directPortCandidate,
    gridRoute,
    perimeterRoute,
    splineCandidate,
    straightCandidate
  };
}
