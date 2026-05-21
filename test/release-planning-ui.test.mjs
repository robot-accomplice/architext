import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";

const releasePlanningSource = readFileSync(
  path.resolve(import.meta.dirname, "../docs/architext/src/presentation/ReleasePlanning.tsx"),
  "utf8"
);
const mainSource = readFileSync(path.resolve(import.meta.dirname, "../docs/architext/src/main.tsx"), "utf8");

test("release planning approval is not gated by transient preview state", () => {
  const approveButton = releasePlanningSource.match(
    /<button type="button" className="approve-action" onClick=\{\(\) => submitPlan\("approve"\)\} disabled=\{([^}]*)\}>/
  );

  assert.ok(approveButton, "approve button should remain present");
  assert.doesNotMatch(approveButton[1], /preview/);
});

test("release planning reload preserves selected draft release detail", () => {
  assert.match(mainSource, /const selectedReleaseId = activeReleaseId \|\| loaded\.releases\?\.index\.currentReleaseId \|\| "";/);
  assert.match(mainSource, /loadReleaseDetail\(fetch, loaded\.releases, selectedReleaseId\)/);
});

test("release truth path keeps item summaries visible in the central view", () => {
  assert.match(mainSource, /<span className="release-path-item-summary">\{item\.summary\}<\/span>/);
});

test("valid data refreshes do not replace dirty editor changes", () => {
  assert.match(mainSource, /if \(releasePlanningDirty \|\| rulesEditorDirty\) \{/);
  assert.match(mainSource, /Save or discard editor changes before refreshing/);
  assert.match(mainSource, /useUnsavedEditorGuard\(editorStates\)/);
  assert.match(releasePlanningSource, /const markEditing = \(\) => onEditingChange\(true\);/);
});

test("release planning preserves persisted ad hoc item ids while omitting temporary ids", () => {
  assert.match(releasePlanningSource, /persisted: true/);
  assert.match(releasePlanningSource, /\.\.\.\(persisted \? \{ id \} : \{\}\)/);
});
