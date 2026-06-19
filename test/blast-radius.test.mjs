import { test } from "node:test";
import assert from "node:assert/strict";
import { searchRepository, blastRadiusForNode } from "../viewer/src/presentation/blastRadius.js";

const model = {
  nodes: [
    { id: "auth", name: "Auth", type: "service", sourcePaths: ["src/services/auth"], summary: "Issues tokens", responsibilities: ["login"], dependencies: ["cache"], relatedFlows: ["login"], relatedDecisions: ["d1"], knownRisks: ["r1"], dataHandled: ["pii"] },
    { id: "cache", name: "Cache", type: "data-store", sourcePaths: ["src/db/cache"], dependencies: [] },
    { id: "gateway", name: "Gateway", type: "service", sourcePaths: ["src/services/gateway"], dependencies: ["auth"] },
    { id: "orphan", name: "Orphan", type: "module", sourcePaths: [], dependencies: [] }
  ],
  flows: [
    { id: "login", name: "Login", steps: [{ from: "gateway", to: "auth" }, { from: "auth", to: "cache" }] },
    { id: "unrelated", name: "Unrelated", steps: [{ from: "orphan", to: "orphan" }] }
  ],
  decisions: [{ id: "d1", title: "Token TTL", relatedNodes: [] }, { id: "d2", title: "Other", relatedNodes: ["auth"] }],
  risks: [{ id: "r1", title: "Token leak", severity: "high", relatedNodes: [] }],
  dataClasses: [{ id: "pii", name: "PII", sensitivity: "high" }],
  views: [{ id: "v1", name: "System", type: "system-map", lanes: [{ id: "l", nodeIds: ["auth", "cache"] }] }]
};
const files = [
  { path: "src/services/auth/index.ts", size: 100, mtime: 5 },
  { path: "src/services/auth/login.ts", size: 50, mtime: 6 },
  { path: "src/db/cache/lru.ts", size: 20, mtime: 7 },
  { path: "README.md", size: 10, mtime: 1 }
];

test("searchRepository ranks component name hits above summary hits and resolves file owners", () => {
  const { components, files: fileHits } = searchRepository(model, files, "auth");
  assert.equal(components[0].id, "auth"); // name match wins
  // file under src/services/auth resolves to the auth owner
  const loginFile = fileHits.find((f) => f.path.endsWith("login.ts"));
  assert.equal(loginFile.ownerId, "auth");
});

test("searchRepository matches concepts in summary/responsibilities and blanks empty query", () => {
  const byConcept = searchRepository(model, files, "tokens"); // only in auth.summary
  assert.equal(byConcept.components[0].id, "auth");
  assert.deepEqual(searchRepository(model, files, "  "), { components: [], files: [] });
});

test("blastRadiusForNode resolves owned files, forward + reverse deps, and participation", () => {
  const br = blastRadiusForNode(model, files, "auth");
  assert.deepEqual(br.ownedFiles.map((f) => f.path).sort(), ["src/services/auth/index.ts", "src/services/auth/login.ts"]);
  assert.deepEqual(br.dependsOn.map((d) => d.id), ["cache"]);       // auth -> cache
  assert.deepEqual(br.dependents.map((d) => d.id), ["gateway"]);    // gateway -> auth
  assert.deepEqual(br.flows.map((f) => f.id).sort(), ["login"]);    // related + step participation (deduped)
  assert.deepEqual(br.decisions.map((d) => d.id).sort(), ["d1", "d2"]); // declared + reverse relatedNodes
  assert.deepEqual(br.risks.map((r) => r.id), ["r1"]);
  assert.deepEqual(br.dataHandled.map((d) => d.id), ["pii"]);
  assert.deepEqual(br.views.map((v) => v.id), ["v1"]);
});

test("blastRadiusForNode returns null for an unknown node", () => {
  assert.equal(blastRadiusForNode(model, files, "nope"), null);
});

test("blastRadiusForNode dedupes a flow reached both by relatedFlows and step participation", () => {
  const br = blastRadiusForNode(model, files, "auth");
  assert.equal(br.flows.filter((f) => f.id === "login").length, 1);
});
