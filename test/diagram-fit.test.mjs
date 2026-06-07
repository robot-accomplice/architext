import assert from "node:assert/strict";
import test from "node:test";
import { calculateFitZoom, measuredDiagramFitZoom } from "../viewer/src/presentation/diagramFit.js";

test("fit zoom chooses the largest zoom that fits both canvas dimensions", () => {
  assert.equal(calculateFitZoom({
    viewportWidth: 1000,
    viewportHeight: 500,
    canvasWidth: 1200,
    canvasHeight: 400
  }), 0.83);
});

test("fit zoom can shrink below readability presets to avoid scrolling", () => {
  assert.equal(calculateFitZoom({
    viewportWidth: 420,
    viewportHeight: 260,
    canvasWidth: 1600,
    canvasHeight: 1200
  }), 0.22);
});

test("fit zoom clamps tiny diagrams to the supported maximum", () => {
  assert.equal(calculateFitZoom({
    viewportWidth: 1200,
    viewportHeight: 900,
    canvasWidth: 300,
    canvasHeight: 200
  }), 1.6);
});

test("measured fit uses the active shell and unscaled canvas dataset", () => {
  const canvas = { dataset: { canvasWidth: "1000", canvasHeight: "800" } };
  const shell = {
    clientWidth: 750,
    clientHeight: 360,
    querySelector: (selector) => selector === ".scaled-canvas-extent" ? canvas : null
  };
  const viewport = {
    querySelector: (selector) => selector === ".map-shell" ? shell : null
  };

  assert.equal(measuredDiagramFitZoom(viewport), 0.45);
});
