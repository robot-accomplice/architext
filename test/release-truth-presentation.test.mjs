import assert from "node:assert/strict";
import test from "node:test";
import {
  activeReleaseBlockersForItem,
  blockersGroupedByItem,
  formatReleaseDate,
  progressFill,
  progressTone,
  releaseItems,
  releaseLineCheckClass,
  releaseLineState,
  releasePathCompletionText,
  releasePathMilestoneStatus,
  releaseProgress,
  releaseScopeByItemId,
  releaseTone
} from "../viewer/src/presentation/releaseTruth.js";

const detail = {
  scope: {
    required: [
      { id: "contract", status: "complete" },
      { id: "workflow", status: "in-progress" }
    ],
    planned: [{ id: "watch", status: "planned" }],
    stretch: [{ id: "pdf", status: "stretch" }],
    deferred: [{ id: "hosted", status: "deferred" }],
    outOfScope: [{ id: "service", status: "cut" }]
  }
};

test("release truth presentation derives release progress from required scope only", () => {
  assert.equal(releaseProgress(detail), 50);
  assert.equal(releaseProgress({ ...detail, scope: { ...detail.scope, required: [] } }), 0);
});

test("release truth presentation keeps release item scope and flattened order consistent", () => {
  assert.deepEqual(releaseItems(detail).map((item) => item.id), ["contract", "workflow", "watch", "pdf", "hosted", "service"]);

  const scopeByItemId = releaseScopeByItemId(detail);
  assert.equal(scopeByItemId.get("contract"), "required");
  assert.equal(scopeByItemId.get("pdf"), "stretch");
  assert.equal(scopeByItemId.get("service"), "out of scope");
});

test("release truth presentation maps status into visual state without UI dependencies", () => {
  assert.equal(releaseTone("complete"), "healthy");
  assert.equal(releaseTone("implementing"), "progressing");
  assert.equal(releaseTone("blocked"), "blocked");
  assert.equal(releaseTone("cut"), "inactive");

  assert.equal(releaseLineState("planned"), "Not Blocked");
  assert.equal(releaseLineState("in-progress"), "Not Blocked");
  assert.equal(releaseLineState("planned", true), "Blocked");
  assert.equal(releaseLineState("complete"), "Complete");
  assert.equal(releaseLineState("cut"), "Deferred");
  assert.equal(releaseLineCheckClass("Complete"), "checked");
});

test("release truth presentation suppresses impossible blocker overlays", () => {
  const activeBlocker = { id: "active", status: "blocked", itemIds: ["contract"] };
  const retiredBlocker = { id: "retired", status: "complete", itemIds: ["workflow"] };

  assert.deepEqual(activeReleaseBlockersForItem({ id: "contract", status: "complete" }, [activeBlocker]), []);
  assert.deepEqual(activeReleaseBlockersForItem({ id: "hosted", status: "deferred" }, [activeBlocker]), []);
  assert.deepEqual(activeReleaseBlockersForItem({ id: "service", status: "cut" }, [activeBlocker]), []);
  assert.deepEqual(activeReleaseBlockersForItem({ id: "workflow", status: "in-progress" }, [retiredBlocker]), []);
  assert.deepEqual(activeReleaseBlockersForItem({ id: "workflow", status: "in-progress" }, [activeBlocker]), [activeBlocker]);
});

test("release truth presentation groups blockers by release item", () => {
  const grouped = blockersGroupedByItem([
    { id: "blocked-media", itemIds: ["camera", "voice"] },
    { id: "blocked-web", itemIds: ["browser"] }
  ]);

  assert.deepEqual(grouped.get("camera").map((blocker) => blocker.id), ["blocked-media"]);
  assert.deepEqual(grouped.get("voice").map((blocker) => blocker.id), ["blocked-media"]);
  assert.deepEqual(grouped.get("browser").map((blocker) => blocker.id), ["blocked-web"]);
});

test("release truth presentation summarizes release path milestone completion", () => {
  assert.equal(releasePathCompletionText([
    { id: "done", status: "complete" },
    { id: "blocked", status: "blocked" },
    { id: "deferred", status: "deferred" }
  ]), "1/3 complete");
});

test("release truth presentation derives completed milestone state from linked items", () => {
  assert.equal(releasePathMilestoneStatus({
    status: "in-progress",
    items: [
      { id: "proof-1", status: "complete" },
      { id: "proof-2", status: "complete" }
    ],
    blockedItems: []
  }), "complete");
});

test("release truth presentation keeps incomplete milestone blockers visible", () => {
  assert.equal(releasePathMilestoneStatus({
    status: "in-progress",
    items: [
      { id: "proof-1", status: "complete" },
      { id: "proof-2", status: "in-progress" }
    ],
    blockedItems: [{ id: "proof-2", status: "in-progress" }]
  }), "blocked");
});

test("release truth presentation keeps stored status for empty milestones", () => {
  assert.equal(releasePathMilestoneStatus({ status: "in-progress", items: [], blockedItems: [] }), "in-progress");
});

test("release truth presentation colors progress by completion state", () => {
  assert.equal(progressTone(0), "inactive");
  assert.equal(progressTone(73), "progressing");
  assert.equal(progressTone(100), "healthy");
  assert.equal(progressFill(0), "var(--line-strong)");
  assert.match(progressFill(73), /var\(--green\) 73%/);
});

test("release truth presentation formats stored release dates for display", () => {
  assert.equal(formatReleaseDate("2026-05-25T12:30:00.000Z"), "2026-05-25");
  assert.equal(formatReleaseDate("2026-05-25"), "2026-05-25");
  assert.equal(formatReleaseDate(), "");
});
