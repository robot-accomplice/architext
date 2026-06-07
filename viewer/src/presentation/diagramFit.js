export const MIN_FIT_ZOOM = 0.15;
export const MAX_FIT_ZOOM = 1.6;

export function calculateFitZoom({
  viewportWidth,
  viewportHeight,
  canvasWidth,
  canvasHeight,
  minZoom = MIN_FIT_ZOOM,
  maxZoom = MAX_FIT_ZOOM
}) {
  const width = Number(canvasWidth);
  const height = Number(canvasHeight);
  const availableWidth = Number(viewportWidth);
  const availableHeight = Number(viewportHeight);
  if (![width, height, availableWidth, availableHeight].every((value) => Number.isFinite(value) && value > 0)) {
    return 1;
  }
  const fit = Math.min(availableWidth / width, availableHeight / height);
  return Math.max(minZoom, Math.min(maxZoom, Number(fit.toFixed(2))));
}

export function measuredDiagramFitZoom(viewportElement) {
  const shell = viewportElement?.querySelector(".map-shell");
  const canvas = shell?.querySelector(".scaled-canvas-extent");
  // Fit the drawn CONTENT, not the full canvas (which carries outer margins and empty lanes);
  // fall back to the canvas extent when content bounds are unavailable.
  return calculateFitZoom({
    viewportWidth: shell?.clientWidth,
    viewportHeight: shell?.clientHeight,
    canvasWidth: canvas?.dataset.contentWidth ?? canvas?.dataset.canvasWidth,
    canvasHeight: canvas?.dataset.contentHeight ?? canvas?.dataset.canvasHeight
  });
}
