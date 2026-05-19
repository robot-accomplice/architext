import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";

const mainSource = readFileSync(path.resolve(import.meta.dirname, "../docs/architext/src/main.tsx"), "utf8");

test("flow and sequence render step lines through the shared StepRoute primitive", () => {
  assert.match(mainSource, /import \{ StepRoute \} from "\.\/presentation\/StepRoute\.js";/);
  assert.match(mainSource, /className="flow-step-route"/);
  assert.match(mainSource, /className="sequence-step-route"/);
  assert.doesNotMatch(mainSource, /<rect className="route-step-marker/);
  assert.doesNotMatch(mainSource, /<text className="route-step-label/);
});
