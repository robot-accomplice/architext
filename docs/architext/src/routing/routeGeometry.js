export function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

export function distanceToRect(point, rect) {
  const dx = Math.max(rect.x - point.x, 0, point.x - (rect.x + rect.width));
  const dy = Math.max(rect.y - point.y, 0, point.y - (rect.y + rect.height));
  return Math.hypot(dx, dy);
}

export function distanceToRectSquared(point, rect) {
  const dx = Math.max(rect.x - point.x, 0, point.x - (rect.x + rect.width));
  const dy = Math.max(rect.y - point.y, 0, point.y - (rect.y + rect.height));
  return dx * dx + dy * dy;
}

export function unitVector(from, to) {
  const dx = to.x - from.x;
  const dy = to.y - from.y;
  const length = Math.hypot(dx, dy);
  if (length === 0) return { x: 1, y: 0 };
  return { x: dx / length, y: dy / length };
}

export function lineSamples(points) {
  const samples = [];
  for (let index = 0; index < points.length - 1; index += 1) {
    const start = points[index];
    const end = points[index + 1];
    for (let step = 1; step <= 10; step += 1) {
      const t = step / 10;
      samples.push({
        x: start.x + (end.x - start.x) * t,
        y: start.y + (end.y - start.y) * t
      });
    }
  }
  return samples;
}

export function sampleLine(start, end, steps = 10) {
  const samples = [];
  for (let step = 1; step <= steps; step += 1) {
    const t = step / steps;
    samples.push({
      x: start.x + (end.x - start.x) * t,
      y: start.y + (end.y - start.y) * t
    });
  }
  return samples;
}

export function cubicPoint(start, controlA, controlB, end, t) {
  const inverse = 1 - t;
  return {
    x: inverse ** 3 * start.x + 3 * inverse ** 2 * t * controlA.x + 3 * inverse * t ** 2 * controlB.x + t ** 3 * end.x,
    y: inverse ** 3 * start.y + 3 * inverse ** 2 * t * controlA.y + 3 * inverse * t ** 2 * controlB.y + t ** 3 * end.y
  };
}

export function sampleCubic(start, controlA, controlB, end, steps = 16) {
  const samples = [];
  for (let step = 1; step <= steps; step += 1) {
    samples.push(cubicPoint(start, controlA, controlB, end, step / steps));
  }
  return samples;
}

export function nearestSample(samples, target) {
  return samples.reduce((nearest, sample) => {
    const nearestDistance = Math.hypot(nearest.x - target.x, nearest.y - target.y);
    const sampleDistance = Math.hypot(sample.x - target.x, sample.y - target.y);
    return sampleDistance < nearestDistance ? sample : nearest;
  }, samples[0] ?? target);
}

export function rectDistance(a, b) {
  const dx = Math.max(a.x - (b.x + b.width), b.x - (a.x + a.width), 0);
  const dy = Math.max(a.y - (b.y + b.height), b.y - (a.y + a.height), 0);
  return Math.hypot(dx, dy);
}

export function rectsOverlap(a, b, padding = 0) {
  return (
    a.x < b.x + b.width + padding &&
    a.x + a.width > b.x - padding &&
    a.y < b.y + b.height + padding &&
    a.y + a.height > b.y - padding
  );
}

export function boundsForPoints(points) {
  if (!points.length) return { x: 0, y: 0, width: 0, height: 0 };
  let minX = points[0].x;
  let maxX = points[0].x;
  let minY = points[0].y;
  let maxY = points[0].y;
  for (let index = 1; index < points.length; index += 1) {
    const point = points[index];
    minX = Math.min(minX, point.x);
    maxX = Math.max(maxX, point.x);
    minY = Math.min(minY, point.y);
    maxY = Math.max(maxY, point.y);
  }
  return {
    x: minX,
    y: minY,
    width: maxX - minX,
    height: maxY - minY
  };
}

export function segmentIntersectsRect(start, end, rect, padding = 0) {
  const left = rect.x - padding;
  const right = rect.x + rect.width + padding;
  const top = rect.y - padding;
  const bottom = rect.y + rect.height + padding;
  const minX = Math.min(start.x, end.x);
  const maxX = Math.max(start.x, end.x);
  const minY = Math.min(start.y, end.y);
  const maxY = Math.max(start.y, end.y);

  if (start.y === end.y) {
    return start.y > top && start.y < bottom && maxX > left && minX < right;
  }
  if (start.x === end.x) {
    return start.x > left && start.x < right && maxY > top && minY < bottom;
  }
  return false;
}

export function bendCount(points) {
  let bends = 0;
  for (let index = 1; index < points.length - 1; index += 1) {
    const previous = points[index - 1];
    const current = points[index];
    const next = points[index + 1];
    if (
      (previous.x === current.x && current.x !== next.x) ||
      (previous.y === current.y && current.y !== next.y)
    ) {
      bends += 1;
    }
  }
  return bends;
}

export function shallowJogCount(points) {
  let count = 0;
  for (let index = 1; index < points.length - 2; index += 1) {
    const before = points[index - 1];
    const start = points[index];
    const end = points[index + 1];
    const after = points[index + 2];
    const middleLength = Math.hypot(end.x - start.x, end.y - start.y);
    const horizontalJog = before.y === start.y && end.y === after.y && start.x === end.x;
    const verticalJog = before.x === start.x && end.x === after.x && start.y === end.y;
    if ((horizontalJog || verticalJog) && middleLength < 36) count += 1;
  }
  return count;
}

export function routeLength(samples) {
  let length = 0;
  for (let index = 0; index < samples.length - 1; index += 1) {
    length += Math.hypot(samples[index + 1].x - samples[index].x, samples[index + 1].y - samples[index].y);
  }
  return length;
}

export function pointAtDistance(samples, distance) {
  if (!samples.length) return null;
  let traveled = 0;
  for (let index = 0; index < samples.length - 1; index += 1) {
    const start = samples[index];
    const end = samples[index + 1];
    const segmentLength = Math.hypot(end.x - start.x, end.y - start.y);
    if (traveled + segmentLength >= distance) {
      const t = segmentLength === 0 ? 0 : (distance - traveled) / segmentLength;
      return {
        x: start.x + (end.x - start.x) * t,
        y: start.y + (end.y - start.y) * t
      };
    }
    traveled += segmentLength;
  }
  return samples.at(-1);
}

export function uniqueRounded(values) {
  return [...new Set(values.map((value) => Math.round(value)))];
}
