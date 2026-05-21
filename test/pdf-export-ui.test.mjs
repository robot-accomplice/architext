import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const mainSource = readFileSync(new URL("../docs/architext/src/main.tsx", import.meta.url), "utf8");
const styleSource = readFileSync(new URL("../docs/architext/src/styles.css", import.meta.url), "utf8");

test("PDF export is exposed through shared diagram controls", () => {
  assert.match(mainSource, /function DiagramControls/);
  assert.match(mainSource, /onExportPdf/);
  assert.match(mainSource, /window\.print\(\)/);
  assert.match(mainSource, />PDF</);
});

test("PDF export print styles preserve the active diagram artifact", () => {
  assert.match(styleSource, /@media print/);
  assert.match(styleSource, /\.diagram-area/);
  assert.match(styleSource, /\.diagram-viewport/);
  assert.match(styleSource, /\.topbar,[\s\S]*?\.diagram-controls[\s\S]*?display: none !important/);
});
