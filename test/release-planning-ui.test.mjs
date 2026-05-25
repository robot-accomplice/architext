import assert from "node:assert/strict";
import test from "node:test";
import {
  releaseDetailsForSelectedRelease,
  selectedReleaseIdForReload
} from "../docs/architext/src/adapters/fetchArchitectureData.js";
import {
  dataRefreshNoticeForDirtyEditors,
  releasePlanActionDisabled,
  releasePlanProposalPayload
} from "../docs/architext/src/presentation/releasePlanningModel.js";
import { releaseItemSummaryText } from "../docs/architext/src/presentation/releaseTruth.js";

function response(body) {
  return {
    ok: true,
    status: 200,
    statusText: "OK",
    text: async () => JSON.stringify(body)
  };
}

test("release planning approval is gated by actionable inputs only", () => {
  assert.equal(releasePlanActionDisabled({ pending: false, version: "1.4.8", selectedCount: 1 }), false);
  assert.equal(releasePlanActionDisabled({ pending: true, version: "1.4.8", selectedCount: 1 }), true);
  assert.equal(releasePlanActionDisabled({ pending: false, version: " ", selectedCount: 1 }), true);
  assert.equal(releasePlanActionDisabled({ pending: false, version: "1.4.8", selectedCount: 0 }), true);
});

test("release planning reload preserves selected draft release detail", async () => {
  const releaseModel = {
    detailBasePath: "releases/",
    index: {
      currentReleaseId: "v1-4-7",
      releases: [
        { id: "v1-4-7", file: "v1-4-7.json" },
        { id: "v1-4-8", file: "v1-4-8.json" }
      ]
    },
    details: [{ id: "v1-4-7" }]
  };
  const requested = [];
  const selectedReleaseId = selectedReleaseIdForReload("v1-4-8", releaseModel);
  const details = await releaseDetailsForSelectedRelease(async (path) => {
    requested.push(path);
    return response({ id: "v1-4-8" });
  }, releaseModel, selectedReleaseId);

  assert.equal(selectedReleaseId, "v1-4-8");
  assert.equal(details.has("v1-4-8"), true);
  assert.deepEqual(requested, ["/data/releases/v1-4-8.json"]);
});

test("release truth path projects item summaries into central view text", () => {
  assert.equal(releaseItemSummaryText({ summary: "Keep the detail visible." }), "Keep the detail visible.");
  assert.equal(releaseItemSummaryText({}), "");
});

test("valid data refreshes do not replace dirty editor changes", () => {
  assert.equal(dataRefreshNoticeForDirtyEditors({ releasePlanningDirty: false, rulesEditorDirty: false }), null);
  assert.equal(
    dataRefreshNoticeForDirtyEditors({ releasePlanningDirty: true, rulesEditorDirty: false }),
    "Architext data changed. Save or discard editor changes before refreshing."
  );
  assert.equal(
    dataRefreshNoticeForDirtyEditors({ releasePlanningDirty: false, rulesEditorDirty: true }),
    "Architext data changed. Save or discard editor changes before refreshing."
  );
});

test("release planning preserves persisted ad hoc item ids while omitting temporary ids", () => {
  const payload = releasePlanProposalPayload({
    dryRun: false,
    action: "approve",
    version: "1.4.8",
    theme: " Hardening ",
    selectedRoadmapIds: ["lock-http-writes"],
    itemScopes: { "lock-http-writes": "required" },
    adHocItems: [
      {
        id: "persisted-item",
        persisted: true,
        title: "Persisted item",
        kind: "test",
        priority: "medium",
        section: "Release Planning",
        scope: "planned"
      },
      {
        id: "ad-hoc-123",
        title: "Temporary item",
        kind: "test",
        priority: "medium",
        section: "Release Planning",
        scope: "planned"
      }
    ]
  });

  assert.equal(payload.theme, "Hardening");
  assert.deepEqual(payload.selectedRoadmapItemIds, ["lock-http-writes"]);
  assert.equal(payload.adHocItems[0].id, "persisted-item");
  assert.equal("id" in payload.adHocItems[1], false);
});
