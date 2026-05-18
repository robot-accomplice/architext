import { clamp } from "./routeGeometry.js";

export const SIDES = ["left", "right", "top", "bottom"];
export const PORT_STUB = 18;
export const PORT_SPACING = 6;

export function anchorFor(rect, side) {
  if (side === "left") return { x: rect.x, y: rect.y + rect.height / 2 };
  if (side === "right") return { x: rect.x + rect.width, y: rect.y + rect.height / 2 };
  if (side === "top") return { x: rect.x + rect.width / 2, y: rect.y };
  return { x: rect.x + rect.width / 2, y: rect.y + rect.height };
}

export function sideVector(side) {
  if (side === "left") return { x: -1, y: 0 };
  if (side === "right") return { x: 1, y: 0 };
  if (side === "top") return { x: 0, y: -1 };
  return { x: 0, y: 1 };
}

export function tangentVector(side) {
  return side === "left" || side === "right"
    ? { x: 0, y: 1 }
    : { x: 1, y: 0 };
}

export function offsetForEndpointOrder(order) {
  const lane = order % 7;
  const band = Math.floor(order / 7);
  return (lane - 3) * PORT_SPACING + band * PORT_SPACING * 7;
}

export function portFor(rect, side, distance = PORT_STUB, rawOffset = 0) {
  const anchor = anchorFor(rect, side);
  const vector = sideVector(side);
  const maxOffset = (side === "left" || side === "right" ? rect.height : rect.width) / 2 - 8;
  const offset = clamp(rawOffset, -maxOffset, maxOffset);
  const tangent = tangentVector(side);
  const offsetAnchor = {
    x: anchor.x + tangent.x * offset,
    y: anchor.y + tangent.y * offset
  };
  return {
    anchor: offsetAnchor,
    port: {
      x: offsetAnchor.x + vector.x * distance,
      y: offsetAnchor.y + vector.y * distance
    }
  };
}

export function portCandidatesFor(rect, side, offsets) {
  const maxOffset = (side === "left" || side === "right" ? rect.height : rect.width) / 2 - 8;
  return [...new Set(offsets.map((offset) => Math.round(clamp(offset, -maxOffset, maxOffset))))].map((offset) => portFor(rect, side, PORT_STUB, offset));
}
