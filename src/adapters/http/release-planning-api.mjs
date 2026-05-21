import path from "node:path";
import { validateReleaseReferences } from "../../domain/architecture-model/references.mjs";
import {
  approveReleasePlan,
  buildReleasePlan,
  mergeExistingReleasePlan,
  saveReleasePlanDraft
} from "../../domain/architecture-model/release-planning.mjs";

function releaseItems(detail) {
  return [
    ...detail.scope.required,
    ...detail.scope.planned,
    ...detail.scope.stretch,
    ...detail.scope.deferred,
    ...detail.scope.outOfScope
  ];
}

function releaseFileForVersion(version) {
  return `v${version.replaceAll(".", "-")}.json`;
}

async function readReleaseDetailForVersion({ payload, releaseIndex, releaseIndexPath, readJson }) {
  if (!payload.version) return null;
  const releaseFile = releaseIndex.releases.find((release) => release.version === payload.version)?.file
    ?? releaseFileForVersion(payload.version);
  const releasePath = path.join(path.dirname(releaseIndexPath), releaseFile);
  return readJson(releasePath).catch(() => null);
}

async function writeDeferredTransferMarkers({
  action,
  roadmap,
  releaseIndex,
  releaseIndexPath,
  releaseDetail,
  readJson,
  writeJson
}) {
  if (action === "save-draft") return;
  const selectedItemIds = new Set(releaseItems(releaseDetail).map((item) => item.id));
  const transferredItems = roadmap.items.filter((item) => (
    selectedItemIds.has(item.id)
    && item.status === "deferred"
    && item.targetReleaseId
    && item.targetReleaseId !== releaseDetail.id
  ));
  for (const item of transferredItems) {
    const sourceSummary = releaseIndex.releases.find((release) => release.id === item.targetReleaseId);
    if (!sourceSummary?.file) continue;
    const sourcePath = path.join(path.dirname(releaseIndexPath), sourceSummary.file);
    const sourceDetail = await readJson(sourcePath);
    let changed = false;
    for (const releaseItem of releaseItems(sourceDetail)) {
      if (releaseItem.id !== item.id) continue;
      releaseItem.deferredToReleaseId = releaseDetail.id;
      releaseItem.deferredToVersion = releaseDetail.version;
      changed = true;
    }
    if (changed) await writeJson(sourcePath, sourceDetail);
  }
}

export async function approveReleasePlanRequest({
  target,
  payload,
  dataDir,
  readJson,
  writeJson,
  validateTarget
}) {
  const targetDataDir = dataDir(target);
  const manifest = await readJson(path.join(targetDataDir, "manifest.json"));
  if (!manifest.files?.roadmap) throw new Error("Release Planning requires manifest.files.roadmap");
  if (!manifest.files?.releases) throw new Error("Release Planning requires manifest.files.releases");

  const roadmapPath = path.join(targetDataDir, manifest.files.roadmap);
  const releaseIndexPath = path.join(targetDataDir, manifest.files.releases);
  const roadmap = await readJson(roadmapPath);
  const releaseIndex = await readJson(releaseIndexPath);

  const action = payload.action ?? (payload.dryRun ? "preview" : "approve");
  if (!["preview", "approve", "save-draft"].includes(action)) {
    throw new Error(`Unknown release planning action "${action}"`);
  }
  const existingReleaseDetail = await readReleaseDetailForVersion({ payload, releaseIndex, releaseIndexPath, readJson });
  const selectedCount = (payload.selectedRoadmapItemIds ?? []).length + (payload.adHocItems ?? []).length;
  const releaseDetail = action === "approve" && selectedCount === 0 && existingReleaseDetail
    ? existingReleaseDetail
    : mergeExistingReleasePlan(existingReleaseDetail, buildReleasePlan({
    releaseIndex,
    roadmapItems: roadmap.items,
    selectedRoadmapItemIds: payload.selectedRoadmapItemIds ?? [],
    itemScopes: payload.itemScopes ?? {},
    adHocItems: payload.adHocItems ?? [],
    projectName: manifest.project.name,
    version: payload.version,
    theme: payload.theme
  }));
  const planned = action === "save-draft"
    ? saveReleasePlanDraft({ releaseIndex, roadmap, releaseDetail })
    : approveReleasePlan({ releaseIndex, roadmap, releaseDetail });
  const referenceErrors = validateReleaseReferences({
    index: planned.releaseIndex,
    details: [planned.releaseFile.detail]
  }, [], { requireAllDetails: false });
  const releaseIds = new Set(planned.releaseIndex.releases.map((release) => release.id));
  for (const item of planned.roadmap.items) {
    if (item.targetReleaseId && !releaseIds.has(item.targetReleaseId)) {
      referenceErrors.push(`roadmap item ${item.id}.targetReleaseId references unknown id "${item.targetReleaseId}"`);
    }
  }
  if (referenceErrors.length) {
    throw new Error(`Proposed release plan failed reference validation:\n${referenceErrors.join("\n")}`);
  }
  if (payload.dryRun) {
    return {
      release: planned.releaseIndex.releases.find((release) => release.id === releaseDetail.id),
      releaseDetail: planned.releaseFile.detail,
      roadmapItems: planned.roadmap.items,
      changes: planned.changes,
      validation: { ok: true, output: "Preview passed reference validation." }
    };
  }

  await writeJson(path.join(path.dirname(releaseIndexPath), planned.releaseFile.file), planned.releaseFile.detail);
  await writeJson(releaseIndexPath, planned.releaseIndex);
  if (action !== "save-draft") {
    await writeJson(roadmapPath, planned.roadmap);
    await writeDeferredTransferMarkers({
      action,
      roadmap,
      releaseIndex,
      releaseIndexPath,
      releaseDetail: planned.releaseFile.detail,
      readJson,
      writeJson
    });
  }

  const validation = await validateTarget(target);
  if (!validation.ok) {
    throw new Error(`Release plan did not validate:\n${validation.output}`);
  }
  return {
    release: planned.releaseIndex.releases.find((release) => release.id === releaseDetail.id),
    releaseDetail: planned.releaseFile.detail,
    roadmapItems: planned.roadmap.items,
    changes: planned.changes,
    validation
  };
}
