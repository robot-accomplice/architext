import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { approveReleasePlanRequest } from "../src/adapters/http/release-planning-api.mjs";
import {
  approveReleasePlan,
  buildReleasePlan,
  nextMinorVersion,
  releasePlanChanges,
  saveReleasePlanDraft
} from "../src/domain/architecture-model/release-planning.mjs";

function dataDir(target) {
  return path.join(target, "docs", "architext", "data");
}

async function readJson(file) {
  return JSON.parse(await readFile(file, "utf8"));
}

async function writeJson(file, value) {
  await mkdir(path.dirname(file), { recursive: true });
  await writeFile(file, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

test("nextMinorVersion prefills from the latest known release", () => {
  assert.equal(nextMinorVersion({
    releases: [
      { version: "1.1.2" },
      { version: "1.2.0" }
    ]
  }), "1.3.0");
});

test("buildReleasePlan combines selected roadmap items and ad hoc items", () => {
  const plan = buildReleasePlan({
    releaseIndex: { releases: [{ version: "1.2.0" }] },
    roadmapItems: [
      {
        id: "release-kanban-view",
        title: "Release Truth Kanban view",
        summary: "Show Release Truth items as Kanban.",
        kind: "feature",
        status: "planned",
        priority: "high",
        section: "Release Planning"
      },
      {
        id: "pdf-export",
        title: "PDF export",
        summary: "Export active view.",
        kind: "feature",
        status: "planned",
        priority: "low",
        section: "PDF Export"
      }
    ],
    selectedRoadmapItemIds: ["release-kanban-view"],
    adHocItems: [{
      title: "C4 container drilldown",
      summary: "Navigate from container to component view.",
      kind: "feature",
      priority: "high",
      section: "C4"
    }],
    projectName: "Architext",
    theme: "Release Planning",
    now: "2026-05-18T06:05:00.000Z"
  });

  assert.equal(plan.id, "v1-3-0");
  assert.equal(plan.version, "1.3.0");
  assert.equal(plan.name, "Architext 1.3.0 Release Planning");
  assert.deepEqual(plan.scope.planned.map((item) => item.id), ["release-kanban-view", "c4-container-drilldown"]);
  assert.deepEqual(plan.scope.planned.map((item) => item.source), ["roadmap", "ad-hoc"]);
  assert.deepEqual(plan.scope.planned.map((item) => item.dateAdded), ["2026-05-18T06:05:00.000Z", "2026-05-18T06:05:00.000Z"]);
  assert.deepEqual(plan.workstreams.map((workstream) => workstream.name), ["Release Planning", "C4"]);
  assert.deepEqual(plan.milestones[0].itemIds, ["release-kanban-view", "c4-container-drilldown"]);
});

test("buildReleasePlan accepts ad hoc items without summaries", () => {
  const plan = buildReleasePlan({
    releaseIndex: { releases: [{ version: "1.2.0" }] },
    roadmapItems: [],
    selectedRoadmapItemIds: [],
    adHocItems: [{
      title: "Release planning polish",
      kind: "feature",
      priority: "medium",
      section: "Release Planning"
    }],
    projectName: "Architext",
    now: "2026-05-18T06:05:00.000Z"
  });

  assert.equal(plan.scope.planned[0].title, "Release planning polish");
  assert.equal(plan.scope.planned[0].summary, "Release planning polish");
});

test("buildReleasePlan requires ad hoc kind and priority", () => {
  const base = {
    releaseIndex: { releases: [{ version: "1.2.0" }] },
    roadmapItems: [],
    selectedRoadmapItemIds: [],
    projectName: "Architext",
    now: "2026-05-18T06:05:00.000Z"
  };

  assert.throws(() => buildReleasePlan({
    ...base,
    adHocItems: [{ title: "Missing kind", priority: "medium", section: "Release Planning" }]
  }), /must include a kind/);
  assert.throws(() => buildReleasePlan({
    ...base,
    adHocItems: [{ title: "Missing priority", kind: "feature", section: "Release Planning" }]
  }), /must include a priority/);
});

test("buildReleasePlan keeps scope separate from lifecycle status", () => {
  const plan = buildReleasePlan({
    releaseIndex: { releases: [{ version: "1.2.0" }] },
    roadmapItems: [{
      id: "pdf-export",
      title: "PDF export",
      summary: "Export the active view.",
      kind: "feature",
      status: "planned",
      priority: "low",
      section: "Export"
    }],
    selectedRoadmapItemIds: ["pdf-export"],
    itemScopes: { "pdf-export": "stretch" },
    adHocItems: [{
      title: "Draft cleanup",
      kind: "chore",
      priority: "medium",
      section: "Release Planning",
      scope: "required"
    }],
    projectName: "Architext",
    now: "2026-05-18T06:05:00.000Z"
  });

  assert.deepEqual(plan.scope.required.map((item) => item.id), ["draft-cleanup"]);
  assert.deepEqual(plan.scope.stretch.map((item) => item.id), ["pdf-export"]);
  assert.equal(plan.scope.stretch[0].status, "planned");
});

test("buildReleasePlan rejects roadmap items already committed to another release", () => {
  assert.throws(() => buildReleasePlan({
    releaseIndex: { releases: [{ version: "1.2.0" }] },
    roadmapItems: [{
      id: "kanban",
      title: "Kanban",
      summary: "Board view.",
      kind: "feature",
      status: "planned",
      section: "Release Truth",
      targetReleaseId: "v1-2-0"
    }],
    selectedRoadmapItemIds: ["kanban"],
    projectName: "Architext",
    version: "1.3.0"
  }), /already committed/);
});


test("approveReleasePlan updates release history and roadmap targets", () => {
  const releaseDetail = buildReleasePlan({
    releaseIndex: { releases: [{ version: "1.2.0" }] },
    roadmapItems: [{
      id: "release-kanban-view",
      title: "Release Truth Kanban view",
      summary: "Show Release Truth items as Kanban.",
      kind: "feature",
      status: "planned",
      priority: "high",
      section: "Release Planning"
    }],
    selectedRoadmapItemIds: ["release-kanban-view"],
    adHocItems: [{
      title: "C4 container drilldown",
      summary: "Navigate from container to component view.",
      kind: "feature",
      priority: "high",
      section: "C4"
    }],
    projectName: "Architext",
    theme: "Release Planning",
    now: "2026-05-18T06:05:00.000Z"
  });

  const approved = approveReleasePlan({
    releaseIndex: {
      currentReleaseId: "v1-2-0",
      releases: [{
        id: "v1-2-0",
        version: "1.2.0",
        name: "Architext 1.2.0",
        status: "completed",
        posture: "shipped",
        lastUpdated: "2026-05-16T14:15:00.000Z",
        summary: "Release Truth.",
        counts: {
          features: 1,
          bugFixes: 0,
          workstreams: 1,
          blockers: 0,
          complete: 1,
          inProgress: 0,
          planned: 0,
          stretch: 0
        },
        file: "v1-2-0.json"
      }]
    },
    roadmap: {
      items: [{
        id: "release-kanban-view",
        title: "Release Truth Kanban view",
        summary: "Show Release Truth items as Kanban.",
        kind: "feature",
        status: "planned",
        priority: "high",
        section: "Release Planning"
      }]
    },
    releaseDetail
  });

  assert.equal(approved.releaseIndex.currentReleaseId, "v1-3-0");
  assert.equal(approved.releaseIndex.releases.at(-1).file, "v1-3-0.json");
  assert.equal(approved.releaseIndex.releases.at(-1).counts.features, 2);
  assert.equal(approved.releaseFile.file, "v1-3-0.json");
  assert.deepEqual(approved.releaseFile.detail, releaseDetail);

  assert.deepEqual(approved.roadmap.items.map((item) => item.id), [
    "release-kanban-view",
    "c4-container-drilldown"
  ]);
  assert.equal(approved.roadmap.items[0].targetReleaseId, "v1-3-0");
  assert.equal(approved.roadmap.items[1].section, "C4");
  assert.equal(approved.roadmap.items[1].targetReleaseId, "v1-3-0");
  assert.equal(approved.roadmap.items[1].dateAdded, "2026-05-18T06:05:00.000Z");
});

test("saveReleasePlanDraft persists draft release truth without changing roadmap targets or current release", () => {
  const releaseIndex = {
    currentReleaseId: "v1-2-0",
    releases: [{ id: "v1-2-0", version: "1.2.0" }]
  };
  const roadmap = {
    items: [{
      id: "release-kanban-view",
      title: "Release Truth Kanban view",
      summary: "Show Release Truth items as Kanban.",
      kind: "feature",
      status: "planned",
      priority: "high",
      section: "Release Planning"
    }]
  };
  const releaseDetail = buildReleasePlan({
    releaseIndex,
    roadmapItems: roadmap.items,
    selectedRoadmapItemIds: ["release-kanban-view"],
    adHocItems: [{
      title: "C4 container drilldown",
      summary: "Navigate from container to component view.",
      kind: "feature",
      priority: "high",
      section: "C4"
    }],
    projectName: "Architext",
    theme: "Release Planning",
    now: "2026-05-18T06:05:00.000Z"
  });

  const draft = saveReleasePlanDraft({ releaseIndex, roadmap, releaseDetail });

  assert.equal(draft.releaseFile.detail.status, "draft");
  assert.equal(draft.releaseIndex.currentReleaseId, "v1-2-0");
  assert.equal(draft.releaseIndex.releases.at(-1).status, "draft");
  assert.equal(draft.releaseIndex.releases.at(-1).id, "v1-3-0");
  assert.equal(draft.roadmap, roadmap);
  assert.equal(draft.roadmap.items[0].targetReleaseId, undefined);
  assert.deepEqual(draft.changes.releaseIndex, {
    action: "add-summary",
    currentReleaseId: "v1-2-0",
    releaseCount: 2
  });
  assert.deepEqual(draft.changes.roadmap, {
    add: 0,
    retarget: 0,
    unchanged: 0,
    changes: []
  });
});

test("releasePlanChanges previews writes without mutating source inputs", () => {
  const releaseIndex = {
    currentReleaseId: "v1-2-0",
    releases: [{ id: "v1-2-0", version: "1.2.0" }]
  };
  const roadmap = {
    items: [{
      id: "release-kanban-view",
      title: "Release Truth Kanban view",
      summary: "Show Release Truth items as Kanban.",
      kind: "feature",
      status: "planned",
      priority: "high",
      section: "Release Planning"
    }]
  };
  const releaseDetail = buildReleasePlan({
    releaseIndex,
    roadmapItems: roadmap.items,
    selectedRoadmapItemIds: ["release-kanban-view"],
    adHocItems: [{
      title: "C4 container drilldown",
      summary: "Navigate from container to component view.",
      kind: "feature",
      priority: "high",
      section: "C4"
    }],
    projectName: "Architext",
    theme: "Release Planning",
    now: "2026-05-18T06:05:00.000Z"
  });

  const changes = releasePlanChanges({ releaseIndex, roadmap, releaseDetail });

  assert.deepEqual(changes.releaseFile, {
    action: "create",
    file: "v1-3-0.json",
    id: "v1-3-0",
    name: "Architext 1.3.0 Release Planning"
  });
  assert.deepEqual(changes.releaseIndex, {
    action: "add-summary",
    currentReleaseId: "v1-3-0",
    releaseCount: 2
  });
  assert.equal(changes.roadmap.add, 1);
  assert.equal(changes.roadmap.retarget, 1);
  assert.equal(changes.roadmap.unchanged, 0);
  assert.equal(roadmap.items[0].targetReleaseId, undefined);
  assert.equal(releaseIndex.currentReleaseId, "v1-2-0");
});

test("release planning API previews without writing target repository files", async () => {
  const target = await mkdtemp(path.join(os.tmpdir(), "architext-release-plan-api-"));
  try {
    const targetDataDir = dataDir(target);
    await mkdir(path.join(targetDataDir, "releases"), { recursive: true });
    await writeJson(path.join(targetDataDir, "manifest.json"), {
      project: { id: "fixture", name: "Fixture" },
      files: {
        releases: "releases/index.json",
        roadmap: "roadmap.json"
      }
    });
    await writeJson(path.join(targetDataDir, "releases", "index.json"), {
      currentReleaseId: "v1-2-0",
      releases: [{ id: "v1-2-0", version: "1.2.0", file: "v1-2-0.json" }]
    });
    await writeJson(path.join(targetDataDir, "roadmap.json"), {
      items: [{
        id: "release-kanban-view",
        title: "Release Truth Kanban view",
        summary: "Show Release Truth items as Kanban.",
        kind: "feature",
        status: "planned",
        priority: "high",
        section: "Release Planning"
      }]
    });

    const result = await approveReleasePlanRequest({
      target,
      payload: {
        dryRun: true,
        version: "1.3.0",
        theme: "Planning",
        selectedRoadmapItemIds: ["release-kanban-view"],
        adHocItems: []
      },
      dataDir,
      readJson,
      writeJson,
      validateTarget: async () => ({ ok: true, output: "should not run for dry run" })
    });

    assert.equal(result.release.id, "v1-3-0");
    assert.equal(result.changes.releaseFile.action, "create");
    assert.equal(result.changes.roadmap.retarget, 1);
    assert.equal(result.validation.ok, true);
    assert.equal(existsSync(path.join(targetDataDir, "releases", "v1-3-0.json")), false);
    assert.equal((await readJson(path.join(targetDataDir, "releases", "index.json"))).currentReleaseId, "v1-2-0");
    assert.equal((await readJson(path.join(targetDataDir, "roadmap.json"))).items[0].targetReleaseId, undefined);
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("release planning API writes approved plans and validates the target", async () => {
  const target = await mkdtemp(path.join(os.tmpdir(), "architext-release-plan-approve-"));
  try {
    const targetDataDir = dataDir(target);
    await mkdir(path.join(targetDataDir, "releases"), { recursive: true });
    await writeJson(path.join(targetDataDir, "manifest.json"), {
      project: { id: "fixture", name: "Fixture" },
      files: {
        releases: "releases/index.json",
        roadmap: "roadmap.json"
      }
    });
    await writeJson(path.join(targetDataDir, "releases", "index.json"), {
      currentReleaseId: "v1-2-0",
      releases: [{ id: "v1-2-0", version: "1.2.0", file: "v1-2-0.json" }]
    });
    await writeJson(path.join(targetDataDir, "roadmap.json"), {
      items: [{
        id: "release-kanban-view",
        title: "Release Truth Kanban view",
        summary: "Show Release Truth items as Kanban.",
        kind: "feature",
        status: "planned",
        priority: "high",
        section: "Release Planning"
      }]
    });

    let validatedTarget = null;
    const result = await approveReleasePlanRequest({
      target,
      payload: {
        dryRun: false,
        version: "1.3.0",
        theme: "Planning",
        selectedRoadmapItemIds: ["release-kanban-view"],
        adHocItems: [{
          title: "C4 container drilldown",
          summary: "Navigate from container to component view.",
          kind: "feature",
          priority: "high",
          section: "C4"
        }]
      },
      dataDir,
      readJson,
      writeJson,
      validateTarget: async (value) => {
        validatedTarget = value;
        return { ok: true, output: "validation passed" };
      }
    });

    const releaseIndex = await readJson(path.join(targetDataDir, "releases", "index.json"));
    const roadmap = await readJson(path.join(targetDataDir, "roadmap.json"));
    const releaseDetail = await readJson(path.join(targetDataDir, "releases", "v1-3-0.json"));

    assert.equal(result.validation.output, "validation passed");
    assert.equal(validatedTarget, target);
    assert.equal(releaseIndex.currentReleaseId, "v1-3-0");
    assert.equal(releaseIndex.releases.at(-1).counts.features, 2);
    assert.deepEqual(roadmap.items.map((item) => item.targetReleaseId), ["v1-3-0", "v1-3-0"]);
    assert.deepEqual(releaseDetail.scope.planned.map((item) => item.id), [
      "release-kanban-view",
      "c4-container-drilldown"
    ]);
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("release planning API saves draft plans without changing roadmap targets", async () => {
  const target = await mkdtemp(path.join(os.tmpdir(), "architext-release-plan-draft-"));
  try {
    const targetDataDir = dataDir(target);
    await mkdir(path.join(targetDataDir, "releases"), { recursive: true });
    await writeJson(path.join(targetDataDir, "manifest.json"), {
      project: { id: "fixture", name: "Fixture" },
      files: {
        releases: "releases/index.json",
        roadmap: "roadmap.json"
      }
    });
    await writeJson(path.join(targetDataDir, "releases", "index.json"), {
      currentReleaseId: "v1-2-0",
      releases: [{ id: "v1-2-0", version: "1.2.0", file: "v1-2-0.json" }]
    });
    await writeJson(path.join(targetDataDir, "roadmap.json"), {
      items: [{
        id: "release-kanban-view",
        title: "Release Truth Kanban view",
        summary: "Show Release Truth items as Kanban.",
        kind: "feature",
        status: "planned",
        priority: "high",
        section: "Release Planning"
      }]
    });

    let validatedTarget = null;
    const result = await approveReleasePlanRequest({
      target,
      payload: {
        action: "save-draft",
        dryRun: false,
        version: "1.3.0",
        theme: "Planning",
        selectedRoadmapItemIds: ["release-kanban-view"],
        adHocItems: []
      },
      dataDir,
      readJson,
      writeJson,
      validateTarget: async (value) => {
        validatedTarget = value;
        return { ok: true, output: "validation passed" };
      }
    });

    const releaseIndex = await readJson(path.join(targetDataDir, "releases", "index.json"));
    const roadmap = await readJson(path.join(targetDataDir, "roadmap.json"));
    const releaseDetail = await readJson(path.join(targetDataDir, "releases", "v1-3-0.json"));

    assert.equal(result.validation.output, "validation passed");
    assert.equal(validatedTarget, target);
    assert.equal(releaseIndex.currentReleaseId, "v1-2-0");
    assert.equal(releaseIndex.releases.at(-1).status, "draft");
    assert.equal(roadmap.items[0].targetReleaseId, undefined);
    assert.equal(releaseDetail.status, "draft");
    assert.equal(result.changes.roadmap.retarget, 0);
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("release planning API marks source release items when deferred scope moves later", async () => {
  const target = await mkdtemp(path.join(os.tmpdir(), "architext-release-plan-deferred-"));
  try {
    const targetDataDir = dataDir(target);
    await mkdir(path.join(targetDataDir, "releases"), { recursive: true });
    await writeJson(path.join(targetDataDir, "manifest.json"), {
      project: { id: "fixture", name: "Fixture" },
      files: {
        releases: "releases/index.json",
        roadmap: "roadmap.json"
      }
    });
    await writeJson(path.join(targetDataDir, "releases", "index.json"), {
      currentReleaseId: "v1-2-0",
      releases: [{
        id: "v1-2-0",
        version: "1.2.0",
        file: "v1-2-0.json"
      }]
    });
    await writeJson(path.join(targetDataDir, "releases", "v1-2-0.json"), {
      id: "v1-2-0",
      version: "1.2.0",
      name: "Fixture 1.2.0",
      status: "completed",
      posture: "shipped",
      summary: "Prior release.",
      releasedAt: "2026-05-01T00:00:00.000Z",
      lastUpdated: "2026-05-01T00:00:00.000Z",
      scope: {
        required: [],
        planned: [],
        stretch: [],
        deferred: [{
          id: "pdf-export",
          title: "PDF export",
          summary: "Export active views.",
          kind: "feature",
          status: "deferred",
          source: "roadmap"
        }],
        outOfScope: []
      },
      workstreams: [],
      blockers: [],
      milestones: [],
      dependencies: [],
      evidence: []
    });
    await writeJson(path.join(targetDataDir, "roadmap.json"), {
      items: [{
        id: "pdf-export",
        title: "PDF export",
        summary: "Export active views.",
        kind: "feature",
        status: "deferred",
        priority: "low",
        section: "Export",
        targetReleaseId: "v1-2-0"
      }]
    });

    await approveReleasePlanRequest({
      target,
      payload: {
        dryRun: false,
        version: "1.3.0",
        selectedRoadmapItemIds: ["pdf-export"],
        itemScopes: { "pdf-export": "planned" },
        adHocItems: []
      },
      dataDir,
      readJson,
      writeJson,
      validateTarget: async () => ({ ok: true, output: "validation passed" })
    });

    const sourceRelease = await readJson(path.join(targetDataDir, "releases", "v1-2-0.json"));
    assert.equal(sourceRelease.scope.deferred[0].deferredToReleaseId, "v1-3-0");
    assert.equal(sourceRelease.scope.deferred[0].deferredToVersion, "1.3.0");
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});
