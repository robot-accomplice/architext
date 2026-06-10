import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DIAGRAM_CONFIG_FIELDS,
  defaultDiagramConfig,
  diffDiagramConfigFromDefaults,
  normalizeDiagramConfigLayer,
  resolveDiagramConfig
} from "../src/domain/diagram-config/diagram-config.mjs";

test("defaults mirror the hardcoded viewer values so an absent config renders identically", () => {
  const config = defaultDiagramConfig();
  // These must match viewer/src/presentation/diagramLayout.js, diagramFit.js,
  // routeConstants.js, and the sequence renderer. If a viewer default changes,
  // this test must change with it — that coupling is intentional.
  assert.equal(config.layout.nodeWidth, 136);
  assert.equal(config.layout.laneWidth, 210);
  assert.equal(config.layout.rowGap, 102);
  assert.equal(config.layout.routeGutter, 132);
  assert.equal(config.sequence.participantWidth, 146);
  assert.equal(config.sequence.rowHeight, 56);
  assert.equal(config.zoom.minFitZoom, 0.15);
  assert.equal(config.zoom.maxFitZoom, 1.6);
  assert.equal(config.legibility.gapArrowheads, 0.5);
});

test("a null or empty layer contributes no overrides and no warnings", () => {
  assert.deepEqual(normalizeDiagramConfigLayer(null), { overrides: {}, warnings: [] });
  assert.deepEqual(normalizeDiagramConfigLayer({}), { overrides: {}, warnings: [] });
});

test("valid numeric fields become overrides for exactly the fields set", () => {
  const { overrides, warnings } = normalizeDiagramConfigLayer({
    layout: { laneWidth: 300 },
    legibility: { gapArrowheads: 0.75 }
  });
  assert.deepEqual(overrides, { layout: { laneWidth: 300 }, legibility: { gapArrowheads: 0.75 } });
  assert.equal(warnings.length, 0);
});

test("unknown sections and fields are ignored with a warning, never crash", () => {
  const { overrides, warnings } = normalizeDiagramConfigLayer({
    bogus: { x: 1 },
    layout: { notAField: 5, laneWidth: 250 }
  });
  assert.deepEqual(overrides, { layout: { laneWidth: 250 } });
  assert.equal(warnings.length, 2);
});

test("non-numeric values fall through (ignored) so the lower layer shows", () => {
  const { overrides, warnings } = normalizeDiagramConfigLayer({
    layout: { laneWidth: "wide", rowGap: 120 }
  });
  assert.deepEqual(overrides, { layout: { rowGap: 120 } });
  assert.equal(warnings.length, 1);
});

test("out-of-range numbers are clamped to the allowed band with a warning", () => {
  const { overrides, warnings } = normalizeDiagramConfigLayer({
    layout: { laneWidth: 99999 },
    legibility: { gapArrowheads: -1 }
  });
  assert.equal(overrides.layout.laneWidth, DIAGRAM_CONFIG_FIELDS.layout.laneWidth.max);
  assert.equal(overrides.legibility.gapArrowheads, 0);
  assert.equal(warnings.length, 2);
});

test("a non-object layer is rejected wholesale, not partially applied", () => {
  const { overrides, warnings } = normalizeDiagramConfigLayer([1, 2, 3], { source: "user" });
  assert.deepEqual(overrides, {});
  assert.equal(warnings.length, 1);
  assert.match(warnings[0], /user/);
});

test("resolution precedence: project overrides user overrides defaults", () => {
  const { config } = resolveDiagramConfig([
    { raw: { layout: { laneWidth: 300, rowGap: 150 } }, source: "user" },
    { raw: { layout: { laneWidth: 400 } }, source: "project" }
  ]);
  assert.equal(config.layout.laneWidth, 400); // project wins
  assert.equal(config.layout.rowGap, 150); // user shows through where project is silent
  assert.equal(config.layout.nodeWidth, 136); // default shows through where both are silent
});

test("warnings from every layer are aggregated", () => {
  const { warnings } = resolveDiagramConfig([
    { raw: { bogus: 1 }, source: "user" },
    { raw: { layout: { laneWidth: "x" } }, source: "project" }
  ]);
  assert.equal(warnings.length, 2);
});

test("diffFromDefaults keeps only changed fields and drops empty sections", () => {
  const full = defaultDiagramConfig();
  full.layout.laneWidth = 300; // changed
  const overrides = diffDiagramConfigFromDefaults(full);
  assert.deepEqual(overrides, { layout: { laneWidth: 300 } });
});

test("diffFromDefaults of an all-default config is empty", () => {
  assert.deepEqual(diffDiagramConfigFromDefaults(defaultDiagramConfig()), {});
});

test("an inverted fit-zoom window reverts the zoom section to defaults", () => {
  const { config, warnings } = resolveDiagramConfig([
    { raw: { zoom: { minFitZoom: 0.9, maxFitZoom: 0.5 } }, source: "user" }
  ]);
  assert.equal(config.zoom.minFitZoom, 0.15);
  assert.equal(config.zoom.maxFitZoom, 1.6);
  assert.ok(warnings.some((w) => /minFitZoom/.test(w)));
});
