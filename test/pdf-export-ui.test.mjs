import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
  pdfExportControlLabel,
  pdfExportReadyMessage,
  pdfExportUnavailableMessage,
  requestPdfExport
} from "../docs/architext/src/presentation/pdfExportModel.js";

function cssBlock(source, marker) {
  const markerIndex = source.indexOf(marker);
  assert.notEqual(markerIndex, -1, `${marker} should exist`);
  const openIndex = source.indexOf("{", markerIndex);
  let depth = 0;
  for (let index = openIndex; index < source.length; index += 1) {
    if (source[index] === "{") depth += 1;
    if (source[index] === "}") depth -= 1;
    if (depth === 0) return source.slice(openIndex + 1, index);
  }
  throw new Error(`${marker} block is not closed`);
}

function cssRules(source) {
  const rules = [];
  let index = 0;
  while (index < source.length) {
    const openIndex = source.indexOf("{", index);
    if (openIndex === -1) return rules;
    const closeIndex = source.indexOf("}", openIndex);
    if (closeIndex === -1) throw new Error("CSS rule is not closed");
    const selectors = source.slice(index, openIndex).split(",").map((selector) => selector.trim()).filter(Boolean);
    const declarations = Object.fromEntries(source.slice(openIndex + 1, closeIndex)
      .split(";")
      .map((entry) => entry.trim())
      .filter(Boolean)
      .map((entry) => {
        const separator = entry.indexOf(":");
        return [entry.slice(0, separator).trim(), entry.slice(separator + 1).trim()];
      }));
    rules.push({ selectors, declarations });
    index = closeIndex + 1;
  }
  return rules;
}

function ruleForSelector(rules, selector) {
  return rules.find((rule) => rule.selectors.includes(selector));
}

test("PDF export exposes a shared browser print request", () => {
  const calls = [];
  const result = requestPdfExport({
    print: () => calls.push("print"),
    requestAnimationFrame: (callback) => {
      calls.push("frame");
      callback();
    }
  });

  assert.equal(pdfExportControlLabel, "PDF");
  assert.equal(result.ok, true);
  assert.equal(result.message, pdfExportReadyMessage);
  assert.deepEqual(calls, ["frame", "print"]);
});

test("PDF export reports unavailable browser print support", () => {
  const frames = [];
  const result = requestPdfExport({
    print: undefined,
    requestAnimationFrame: (callback) => frames.push(callback)
  });

  assert.equal(result.ok, false);
  assert.equal(result.message, pdfExportUnavailableMessage);
  assert.deepEqual(frames, []);
});

test("PDF export print styles preserve the active diagram artifact", () => {
  const styleSource = readFileSync(new URL("../docs/architext/src/styles.css", import.meta.url), "utf8");
  const rules = cssRules(cssBlock(styleSource, "@media print"));
  const hiddenChrome = ruleForSelector(rules, ".diagram-controls");
  const diagramArea = ruleForSelector(rules, ".diagram-area");
  const diagramViewport = ruleForSelector(rules, ".diagram-viewport");

  assert.ok(hiddenChrome?.selectors.includes(".topbar"));
  assert.ok(hiddenChrome?.selectors.includes(".left-nav"));
  assert.ok(hiddenChrome?.selectors.includes(".details"));
  assert.equal(hiddenChrome?.declarations.display, "none !important");
  assert.equal(diagramArea?.declarations.border, "0");
  assert.equal(diagramArea?.declarations.overflow, "visible");
  assert.equal(diagramViewport?.declarations.height, "auto");
  assert.equal(diagramViewport?.declarations.overflow, "visible");
});
