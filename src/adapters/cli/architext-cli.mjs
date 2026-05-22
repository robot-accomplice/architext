import { existsSync } from "node:fs";
import { copyFile, cp, mkdir, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { createInterface } from "node:readline/promises";
import path from "node:path";
import { stdin as input, stdout as output } from "node:process";
import { fileURLToPath } from "node:url";
import { createCommandHandlers, routeCommand } from "./command-router.mjs";
import { parseArgs, usage } from "./command-line.mjs";
import { assertDirectory, git, gitAvailable, readJson, run, tryRun, writeJson } from "./runtime.mjs";
import { printStatus } from "./terminal-presenter.mjs";
import { createDataWatchHub } from "../http/data-watch-hub.mjs";
import { approveReleasePlanRequest as approveReleasePlanApiRequest } from "../http/release-planning-api.mjs";
import { updateRulesRequest as updateRulesApiRequest } from "../http/rules-api.mjs";
import { c4DrilldownIssues, c4IssuesForView, repairC4Views } from "../../domain/architecture-model/c4-quality.mjs";
import { generatedReleaseIndex, releaseIndexGenerationChanges } from "../../domain/architecture-model/release-history.mjs";
import { doctorRepairCategories, doctorRepairsForStatus } from "../../domain/lifecycle/doctor-repairs.mjs";
import { instructionRuleFiles, plannedInstructionRuleMigration } from "../../domain/lifecycle/instruction-rule-migration.mjs";
import { schemaMigrationPlan } from "../../domain/lifecycle/schema-migrations.mjs";
import {
  architextDir,
  copiedInstallCandidatePaths,
  dataDir,
  generatedIgnores,
  instructionFiles,
  legacyMetadataPath,
  metadataPath,
  rootScripts
} from "../../domain/lifecycle/target-layout.mjs";

const packageRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
const viewerDir = path.join(packageRoot, "docs", "architext");
const viewerDistDir = path.join(viewerDir, "dist");
const schemaDir = path.join(viewerDir, "schema");
const validatorPath = path.join(viewerDir, "tools", "validate-architext.mjs");
const appendixPath = path.join(viewerDir, "AGENTS_APPENDIX.md");
const dataSchemaVersion = "1.4.0";

async function packageVersion() {
  return (await readJson(path.join(packageRoot, "package.json"))).version;
}

async function assertTarget(target) {
  await assertDirectory(target, "Target");
}

function copiedInstallPaths(target) {
  if (path.resolve(target) === packageRoot) return [];
  return copiedInstallCandidatePaths(target).filter((entryPath) => existsSync(entryPath));
}

async function readMetadata(target) {
  const current = metadataPath(target);
  const legacy = legacyMetadataPath(target);
  if (existsSync(current)) return readJson(current).catch(() => null);
  if (existsSync(legacy)) return readJson(legacy).catch(() => null);
  return null;
}

async function validateTarget(target) {
  if (!existsSync(path.join(dataDir(target), "manifest.json"))) {
    return { ok: false, output: `Architext data is not installed at ${dataDir(target)}` };
  }
  return tryRun(process.execPath, [validatorPath, "--data-dir", dataDir(target), "--schema-dir", schemaDir], packageRoot);
}

async function collectC4Status(target) {
  const targetDataDir = dataDir(target);
  const viewsPath = path.join(targetDataDir, "views.json");
  const nodesPath = path.join(targetDataDir, "nodes.json");
  if (!existsSync(viewsPath) || !existsSync(nodesPath)) {
    return { available: false, issues: [], repairChanges: [], remainingIssues: [] };
  }

  const viewsDocument = await readJson(viewsPath);
  const nodeMap = new Map((await readJson(nodesPath)).nodes.map((node) => [node.id, node]));
  const issues = viewsDocument.views.flatMap((view) => view.type?.startsWith("c4-") ? c4IssuesForView(view, nodeMap) : []);
  const drilldownIssues = c4DrilldownIssues(viewsDocument.views, nodeMap);
  const repaired = repairC4Views(viewsDocument.views, nodeMap);
  const remainingIssues = repaired.views.flatMap((view) => view.type?.startsWith("c4-") ? c4IssuesForView(view, nodeMap) : []);
  return { available: true, issues, drilldownIssues, repairChanges: repaired.changes, remainingIssues };
}

async function repairC4Data(target, dryRun) {
  const viewsPath = path.join(dataDir(target), "views.json");
  const nodesPath = path.join(dataDir(target), "nodes.json");
  if (!existsSync(viewsPath) || !existsSync(nodesPath)) return { repairChanges: [] };
  const viewsDocument = await readJson(viewsPath);
  const nodeMap = new Map((await readJson(nodesPath)).nodes.map((node) => [node.id, node]));
  const repaired = repairC4Views(viewsDocument.views, nodeMap);
  if (repaired.changes.length && !dryRun) await writeJson(viewsPath, { ...viewsDocument, views: repaired.views });
  return { repairChanges: repaired.changes };
}

async function collectReleaseTruthStatus(target) {
  const manifestPath = path.join(dataDir(target), "manifest.json");
  if (!existsSync(manifestPath)) return null;
  const manifest = await readJson(manifestPath);
  const configured = Boolean(manifest.files?.releases);
  const indexPath = configured ? path.join(dataDir(target), manifest.files.releases) : path.join(dataDir(target), "releases", "index.json");
  const indexExists = existsSync(indexPath);
  const repairChanges = configured
    ? await generatedReleaseHistoryChanges(indexPath, indexExists)
    : ["add starter Release Truth data and manifest.files.releases"];
  return { configured, indexPath, indexExists, repairChanges };
}

async function collectManifestStatus(target) {
  const manifestPath = path.join(dataDir(target), "manifest.json");
  if (!existsSync(manifestPath)) return null;
  const manifest = await readJson(manifestPath);
  const currentSchemaVersion = manifest.schemaVersion ?? "";
  const migrationPlan = schemaMigrationPlan({
    currentVersion: currentSchemaVersion,
    targetVersion: dataSchemaVersion
  });
  return {
    path: manifestPath,
    schemaVersion: currentSchemaVersion,
    expectedSchemaVersion: dataSchemaVersion,
    migrationPlan,
    repairChanges: migrationPlan.pending.map((migration) => migration.summary)
  };
}

async function repairManifestData(target, dryRun) {
  const manifestPath = path.join(dataDir(target), "manifest.json");
  if (!existsSync(manifestPath)) return { repairChanges: [] };
  const manifest = await readJson(manifestPath);
  const status = await collectManifestStatus(target);
  if (!status?.repairChanges.length) return { repairChanges: [] };
  if (!dryRun) await writeJson(manifestPath, { ...manifest, schemaVersion: dataSchemaVersion });
  return { repairChanges: status.repairChanges };
}

async function releaseDetailEntries(indexPath, index) {
  const releaseDir = path.dirname(indexPath);
  return Promise.all(index.releases.map(async (summary) => ({
    file: summary.file,
    detail: await readJson(path.join(releaseDir, summary.file))
  })));
}

async function generatedReleaseHistoryChanges(indexPath, indexExists) {
  if (!indexExists) return ["create missing Release Truth history index"];
  const index = await readJson(indexPath);
  const generated = generatedReleaseIndex(index, await releaseDetailEntries(indexPath, index));
  return releaseIndexGenerationChanges(index, generated);
}

async function repairReleaseTruthData(target, dryRun) {
  const targetDataDir = dataDir(target);
  const manifestPath = path.join(targetDataDir, "manifest.json");
  if (!existsSync(manifestPath)) return { repairChanges: [] };
  const manifest = await readJson(manifestPath);
  if (manifest.files?.releases) {
    const indexPath = path.join(targetDataDir, manifest.files.releases);
    if (!existsSync(indexPath)) return { repairChanges: [] };
    const index = await readJson(indexPath);
    const generated = generatedReleaseIndex(index, await releaseDetailEntries(indexPath, index));
    const repairChanges = releaseIndexGenerationChanges(index, generated);
    if (repairChanges.length && !dryRun) await writeJson(indexPath, generated);
    return { repairChanges };
  }
  if (!dryRun) {
    await writeJson(manifestPath, {
      ...manifest,
      files: {
        ...manifest.files,
        releases: "releases/index.json"
      }
    });
    await writeStarterReleaseData(targetDataDir);
  }
  return { repairChanges: ["add starter Release Truth data and manifest.files.releases"] };
}

async function cursorRuleFilePaths(target) {
  const cursorRulesDir = path.join(target, ".cursor", "rules");
  if (!existsSync(cursorRulesDir)) return [];
  const entries = await readdir(cursorRulesDir, { withFileTypes: true }).catch(() => []);
  return entries
    .filter((entry) => entry.isFile() && /\.(md|mdc|txt)$/.test(entry.name))
    .map((entry) => path.join(cursorRulesDir, entry.name));
}

async function instructionRuleSourceFiles(target) {
  const explicitFiles = instructionRuleFiles.map((fileName) => path.join(target, fileName));
  const cursorFiles = await cursorRuleFilePaths(target);
  const files = [];
  for (const filePath of [...explicitFiles, ...cursorFiles]) {
    if (!existsSync(filePath)) continue;
    files.push({
      path: path.relative(target, filePath),
      absolutePath: filePath,
      text: await readFile(filePath, "utf8")
    });
  }
  return files;
}

async function collectInstructionRuleStatus(target) {
  const rulesPath = path.join(dataDir(target), "rules.json");
  if (!existsSync(rulesPath)) return null;
  const rulesDocument = await readJson(rulesPath).catch(() => ({ rules: [] }));
  return plannedInstructionRuleMigration({
    files: await instructionRuleSourceFiles(target),
    existingRules: rulesDocument.rules ?? []
  });
}

async function repairInstructionRules(target, dryRun) {
  const rulesPath = path.join(dataDir(target), "rules.json");
  if (!existsSync(rulesPath)) return { repairChanges: [] };
  const rulesDocument = await readJson(rulesPath);
  const migration = plannedInstructionRuleMigration({
    files: await instructionRuleSourceFiles(target),
    existingRules: rulesDocument.rules ?? []
  });
  if (!migration.repairChanges.length) return { repairChanges: [] };
  if (!dryRun) {
    if (migration.candidateRules.length) {
      await writeJson(rulesPath, { ...rulesDocument, rules: [...(rulesDocument.rules ?? []), ...migration.candidateRules] });
    }
    for (const rewrite of migration.rewriteFiles) {
      await writeFile(path.join(target, rewrite.path), rewrite.replacement.endsWith("\n") ? rewrite.replacement : `${rewrite.replacement}\n`, "utf8");
    }
  }
  return { repairChanges: migration.repairChanges };
}

const doctorRepairHandlers = {
  c4: repairC4Data,
  manifest: repairManifestData,
  "release-truth": repairReleaseTruthData,
  "instruction-rules": repairInstructionRules
};

function slugify(value) {
  const slug = value.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  return slug || "target-project";
}

async function writeStarterReleaseData(targetDataDir) {
  const releaseDir = path.join(targetDataDir, "releases");
  const releaseId = "initial-architext-buildout";
  const lastUpdated = new Date().toISOString();
  await mkdir(releaseDir, { recursive: true });
  await writeJson(path.join(releaseDir, "index.json"), {
    currentReleaseId: releaseId,
    releases: [
      {
        id: releaseId,
        version: "0.1.0",
        name: "Initial Architext build-out",
        status: "planned",
        posture: "at-risk",
        targetWindow: "Before claiming architecture documentation is current",
        lastUpdated,
        summary: "Replace starter architecture and release data with project-specific facts.",
        counts: {
          features: 0,
          bugFixes: 0,
          workstreams: 1,
          blockers: 1,
          complete: 0,
          inProgress: 0,
          planned: 1,
          stretch: 0
        },
        file: `${releaseId}.json`
      }
    ]
  });
  await writeJson(path.join(releaseDir, `${releaseId}.json`), {
    id: releaseId,
    version: "0.1.0",
    name: "Initial Architext build-out",
    status: "planned",
    posture: "at-risk",
    summary: "Replace starter architecture and release data with project-specific facts.",
    targetWindow: "Before claiming architecture documentation is current",
    lastUpdated,
    updateSource: "architext sync starter data",
    scope: {
      required: [
        {
          id: "replace-starter-architecture-data",
          title: "Replace starter architecture data",
          kind: "documentation",
          status: "planned",
          summary: "Inspect the repository and replace starter Architext JSON with source-backed project facts.",
          owner: "Project maintainers",
          workstreamId: "architecture-buildout",
          dependsOn: [],
          evidence: ["architext validate"]
        }
      ],
      planned: [],
      stretch: [],
      deferred: [],
      outOfScope: []
    },
    workstreams: [
      {
        id: "architecture-buildout",
        name: "Architecture build-out",
        owner: "Project maintainers",
        status: "planned",
        posture: "at-risk",
        summary: "Replace starter architecture facts, release facts, and validation evidence before relying on Architext.",
        progress: 0,
        itemIds: ["replace-starter-architecture-data"],
        evidence: ["architext validate"]
      }
    ],
    blockers: [
      {
        id: "starter-data-not-replaced",
        title: "Starter data is not project truth",
        severity: "high",
        status: "blocked",
        owner: "Project maintainers",
        summary: "The project has validating starter data, but it has not yet been replaced with source-backed architecture and release facts.",
        nextAction: "Run the agent-assisted Architext build-out workflow and review the JSON diff.",
        itemIds: ["replace-starter-architecture-data"],
        evidenceNeeded: ["Source-backed JSON updates", "architext validate"]
      }
    ],
    milestones: [
      {
        id: "starter-replaced",
        label: "Starter data replaced",
        status: "planned",
        targetWindow: "Initial documentation pass",
        order: 1,
        itemIds: ["replace-starter-architecture-data"]
      }
    ],
    dependencies: [],
    evidence: [
      {
        id: "starter-validation",
        label: "architext validate",
        kind: "test",
        status: "planned"
      }
    ]
  });
}

async function writeStarterData(target, version) {
  const targetDataDir = dataDir(target);
  const projectName = path.basename(target);
  const projectId = slugify(projectName);
  const systemId = `${projectId}-system`;
  const containerId = `${projectId}-container`;
  const componentId = `${projectId}-component`;
  const actorId = "project-team";
  const dataId = "architecture-knowledge";
  const flowId = "architecture-buildout";

  await writeJson(path.join(targetDataDir, "manifest.json"), {
    schemaVersion: dataSchemaVersion,
    project: {
      id: projectId,
      name: projectName,
      summary: "Architext has been installed. Replace this starter model with the real project architecture."
    },
    generatedAt: new Date().toISOString(),
    defaultViewId: "system-map",
    files: {
      nodes: "nodes.json",
      flows: "flows.json",
      views: "views.json",
      dataClassification: "data-classification.json",
      decisions: "decisions.json",
      risks: "risks.json",
      glossary: "glossary.json",
      rules: "rules.json",
      releases: "releases/index.json"
    },
    notes: [
      "Starter data only. Ask an agent to inspect the codebase and build out docs/architext/data/**/*.json.",
      "Do not treat this starter model as architecture documentation for the target project."
    ]
  });

  await writeJson(path.join(targetDataDir, "nodes.json"), {
    nodes: [
      {
        id: actorId,
        type: "actor",
        name: "Project Team",
        summary: "Placeholder actor for the team or user initiating the Architext build-out.",
        responsibilities: ["Replace starter data with real architecture facts"],
        owner: "Project maintainers",
        sourcePaths: [],
        runtime: "Repository workflow",
        interfaces: ["Architext JSON"],
        dependencies: [systemId],
        dataHandled: [dataId],
        security: ["Unknown until architecture build-out is complete"],
        observability: ["Unknown until architecture build-out is complete"],
        relatedFlows: [flowId],
        relatedDecisions: [],
        knownRisks: ["architext-starter-data"],
        verification: ["architext validate"]
      },
      {
        id: systemId,
        type: "software-system",
        name: projectName,
        summary: "Placeholder system boundary. Replace with the real project systems, services, stores, flows, and dependencies.",
        responsibilities: ["Pending architecture discovery"],
        owner: "Project maintainers",
        sourcePaths: [],
        runtime: "Unknown until architecture build-out is complete",
        interfaces: ["Unknown until architecture build-out is complete"],
        dependencies: [],
        dataHandled: [dataId],
        security: ["Unknown until architecture build-out is complete"],
        observability: ["Unknown until architecture build-out is complete"],
        relatedFlows: [flowId],
        relatedDecisions: ["architext-buildout-required"],
        knownRisks: ["architext-starter-data"],
        verification: ["architext validate"]
      },
      {
        id: containerId,
        type: "service",
        name: `${projectName} service placeholder`,
        summary: "Placeholder container inside the system boundary. Replace with real deployable units during architecture build-out.",
        responsibilities: ["Pending container discovery"],
        owner: "Project maintainers",
        sourcePaths: [],
        runtime: "Unknown until architecture build-out is complete",
        interfaces: ["Unknown until architecture build-out is complete"],
        dependencies: [],
        dataHandled: [dataId],
        security: ["Unknown until architecture build-out is complete"],
        observability: ["Unknown until architecture build-out is complete"],
        relatedFlows: [flowId],
        relatedDecisions: ["architext-buildout-required"],
        knownRisks: ["architext-starter-data"],
        verification: ["architext validate"]
      },
      {
        id: componentId,
        type: "module",
        name: `${projectName} component placeholder`,
        summary: "Placeholder component. Replace with real components inside a selected container during architecture build-out.",
        responsibilities: ["Pending component discovery"],
        owner: "Project maintainers",
        sourcePaths: [],
        runtime: "Unknown until architecture build-out is complete",
        interfaces: ["Unknown until architecture build-out is complete"],
        dependencies: [],
        dataHandled: [dataId],
        security: ["Unknown until architecture build-out is complete"],
        observability: ["Unknown until architecture build-out is complete"],
        relatedFlows: [flowId],
        relatedDecisions: ["architext-buildout-required"],
        knownRisks: ["architext-starter-data"],
        verification: ["architext validate"]
      }
    ]
  });

  await writeJson(path.join(targetDataDir, "flows.json"), {
    flows: [
      {
        id: flowId,
        name: "Architext build-out required",
        status: "planned",
        summary: "Starter flow showing that architecture data still needs to be generated from the target repository.",
        trigger: "Architext installed into the project",
        actors: [actorId],
        steps: [
          {
            id: "inspect-project",
            from: actorId,
            to: systemId,
            action: "inspectCodebaseAndReplaceStarterData",
            summary: "An agent should inspect the repository and replace every starter JSON file with real architecture data.",
            data: [dataId]
          }
        ],
        guarantees: ["Validation passes for starter data"],
        failureBehavior: ["Rendered site is not useful until project-specific data replaces the starter model"],
        observability: ["Validation output"],
        verification: ["architext validate"],
        knownGaps: ["All project architecture facts are pending discovery"]
      }
    ]
  });

  await writeJson(path.join(targetDataDir, "views.json"), {
    views: [
      { id: "system-map", name: "System Map", type: "system-map", summary: "Starter view. Replace with the real project system map.", lanes: [{ id: "people", name: "People", nodeIds: [actorId] }, { id: "system", name: "System", nodeIds: [systemId] }] },
      { id: "dataflow", name: "Dataflow", type: "dataflow", summary: "Starter dataflow. Replace with real data movement.", lanes: [{ id: "source", name: "Source", nodeIds: [actorId] }, { id: "target", name: "Target", nodeIds: [systemId] }] },
      { id: "sequence", name: "Sequence", type: "sequence", summary: "Starter sequence for the build-out flow.", lanes: [{ id: "participants", name: "Participants", nodeIds: [actorId, systemId] }] },
      { id: "deployment", name: "Deployment", type: "deployment", summary: "Starter deployment view. Replace with real runtime placement.", lanes: [{ id: "unknown", name: "Unknown", nodeIds: [systemId] }] },
      { id: "c4-context", name: "C4 Context", type: "c4-context", summary: "Starter C4 context. Replace with real actors, system boundary, and external systems.", lanes: [{ id: "people", name: "People", nodeIds: [actorId] }, { id: "system", name: "System", nodeIds: [systemId] }] },
      { id: "c4-container", name: "C4 Container", type: "c4-container", summary: "Starter C4 container view. Replace with deployable units and dependencies.", scopeNodeId: systemId, lanes: [{ id: "containers", name: "Containers", nodeIds: [containerId] }] },
      { id: "c4-component", name: "C4 Component", type: "c4-component", summary: "Starter C4 component view. Replace with components inside a selected container.", scopeNodeId: containerId, lanes: [{ id: "components", name: "Components", nodeIds: [componentId] }] }
    ]
  });

  await writeJson(path.join(targetDataDir, "data-classification.json"), {
    classes: [{ id: dataId, name: "Architecture Knowledge", sensitivity: "medium", handling: "Review generated architecture facts before treating them as project documentation." }]
  });
  await writeJson(path.join(targetDataDir, "decisions.json"), {
    decisions: [{ id: "architext-buildout-required", status: "planned", title: "Replace starter Architext data", context: "Architext was installed with neutral starter data.", decision: "An agent must inspect the target repository and replace docs/architext/data/*.json with project-specific architecture facts.", consequences: ["The site validates immediately", "The starter model is intentionally not useful as final documentation"], relatedNodes: [systemId], relatedFlows: [flowId] }]
  });
  await writeJson(path.join(targetDataDir, "risks.json"), {
    risks: [{ id: "architext-starter-data", title: "Starter data is not project architecture", category: "technical", severity: "high", status: "open", summary: "The installed Architext data is a placeholder until an agent builds out the real architecture model.", mitigations: ["Run the agent-assisted JSON build-out workflow", "Review generated JSON diffs", "Run architext validate"], relatedNodes: [systemId], relatedFlows: [flowId] }]
  });
  await writeJson(path.join(targetDataDir, "glossary.json"), {
    terms: [{ term: "Architext starter data", definition: "A neutral validating placeholder installed into new projects before real architecture data is generated." }]
  });
  await writeJson(path.join(targetDataDir, "rules.json"), {
    rules: [
      {
        id: "replace-starter-data",
        title: "Replace starter data",
        summary: "Replace neutral starter data with source-backed architecture, release, and project rules before treating Architext as current.",
        category: "project",
        criticality: "critical",
        order: 10,
        source: "maintainer",
        rationale: "Fresh installs validate immediately, but starter data is not project-specific documentation.",
        appliesTo: ["initial build-out", "agent maintenance", "validation"],
        protection: { edit: true, delete: true }
      }
    ]
  });
  await writeStarterReleaseData(targetDataDir);
}

async function appendixMarkdown() {
  const appendix = await readFile(appendixPath, "utf8");
  const start = appendix.indexOf("```markdown");
  const end = appendix.lastIndexOf("\n```");
  if (start === -1 || end === -1 || end <= start) return appendix.trim();
  return appendix.slice(start + "```markdown".length, end).trim();
}

function replaceArchitextSection(existing, appendix) {
  const heading = "## Architext Architecture Documentation";
  const start = existing.indexOf(heading);
  if (start === -1) {
    const prefix = existing.trimEnd();
    return `${prefix}${prefix ? "\n\n" : ""}${appendix}\n`;
  }
  const nextHeading = existing.slice(start + heading.length).search(/\n## (?!#)/);
  const end = nextHeading === -1 ? existing.length : start + heading.length + nextHeading;
  return `${existing.slice(0, start).trimEnd()}${existing.slice(0, start).trimEnd() ? "\n\n" : ""}${appendix}\n${existing.slice(end).replace(/^\n+/, "\n")}`.replace(/\n{3,}/g, "\n\n");
}

async function upsertInstructionFile({ target, fileName, dryRun }) {
  const destination = path.join(target, fileName);
  const appendix = await appendixMarkdown();
  const existing = existsSync(destination) ? await readFile(destination, "utf8") : "";
  const next = replaceArchitextSection(existing, appendix);
  if (next === existing) return { destination, changed: false, reason: "already current" };
  if (!dryRun) {
    await mkdir(path.dirname(destination), { recursive: true });
    await writeFile(destination, next, "utf8");
  }
  return { destination, changed: true, created: !existing };
}

async function packageJsonInfo(target) {
  const file = path.join(target, "package.json");
  if (!existsSync(file)) return { path: file, exists: false, packageJson: null };
  return { path: file, exists: true, packageJson: await readJson(file) };
}

async function upsertRootScripts({ target, dryRun }) {
  const info = await packageJsonInfo(target);
  if (!info.exists) return { destination: info.path, changed: false, reason: "missing package.json", missing: [] };
  const existingScripts = info.packageJson.scripts ?? {};
  const missing = Object.entries(rootScripts).filter(([name, value]) => existingScripts[name] !== value);
  if (missing.length === 0) return { destination: info.path, changed: false, reason: "already present", missing: [] };
  if (!dryRun) {
    await writeJson(info.path, { ...info.packageJson, scripts: { ...existingScripts, ...Object.fromEntries(missing) } });
  }
  return { destination: info.path, changed: true, missing: missing.map(([name]) => name) };
}

async function upsertGitignore({ target, dryRun }) {
  const destination = path.join(target, ".gitignore");
  const existing = existsSync(destination) ? await readFile(destination, "utf8") : "";
  const lines = existing.split(/\r?\n/);
  const missing = generatedIgnores.filter((entry) => !lines.includes(entry));
  if (missing.length === 0) return { destination, changed: false, reason: "already present", missing: [] };
  if (!dryRun) {
    const prefix = existing.trimEnd();
    await writeFile(destination, `${prefix}${prefix ? "\n\n" : ""}# Architext generated static builds.\n${missing.join("\n")}\n`, "utf8");
  }
  return { destination, changed: true, missing };
}

async function writeMetadata(target, patch) {
  const existing = await readMetadata(target);
  const next = {
    schemaVersion: 2,
    installedAt: existing?.installedAt ?? new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    ...existing,
    ...patch
  };
  await writeJson(metadataPath(target), next);
  return next;
}

async function collectStatus(target, version, { runValidation = false } = {}) {
  const targetDataDir = dataDir(target);
  const manifestPath = path.join(targetDataDir, "manifest.json");
  const copiedPaths = copiedInstallPaths(target);
  const packageSelf = path.resolve(target) === packageRoot;
  const metadata = await readMetadata(target);
  const installed = existsSync(manifestPath);
  const validation = runValidation ? await validateTarget(target) : null;
  const c4 = installed ? await collectC4Status(target) : null;
  const releaseTruth = installed ? await collectReleaseTruthStatus(target) : null;
  const manifest = installed ? await collectManifestStatus(target) : null;
  const instructionRules = installed ? await collectInstructionRuleStatus(target) : null;
  const gitignoreText = existsSync(path.join(target, ".gitignore")) ? await readFile(path.join(target, ".gitignore"), "utf8") : "";
  const gitignoreMissing = generatedIgnores.filter((entry) => !gitignoreText.split(/\r?\n/).includes(entry));
  const instructionStatus = {};
  for (const fileName of instructionFiles) {
    const filePath = path.join(target, fileName);
    const text = existsSync(filePath) ? await readFile(filePath, "utf8") : "";
    instructionStatus[fileName] = {
      exists: existsSync(filePath),
      hasArchitextSection: text.includes("## Architext Architecture Documentation"),
      mentionsCopiedTemplate: /docs\/architext\/(src|schema|tools|package\.json|node_modules)|npm run validate|cd docs\/architext/.test(text)
    };
  }
  const packageInfo = await packageJsonInfo(target);
  const rootScriptStatus = {};
  for (const [name, value] of Object.entries(rootScripts)) {
    const actual = packageInfo.packageJson?.scripts?.[name] ?? "";
    rootScriptStatus[name] = { present: Boolean(actual), recommended: actual === value, value: actual || null };
  }
  const trackedGenerated = gitAvailable(target)
    ? tryRun("git", ["ls-files", "docs/architext/dist"], target).output.split(/\r?\n/).filter(Boolean)
    : [];

  const status = {
    target,
    cliVersion: version,
    installed,
    dataDir: targetDataDir,
    metadata,
    copiedInstallDetected: !packageSelf && (copiedPaths.length > 0 || existsSync(legacyMetadataPath(target))),
    copiedInstallPaths: copiedPaths.map((item) => path.relative(target, item)),
    needsMigration: !packageSelf && (copiedPaths.length > 0 || existsSync(legacyMetadataPath(target))),
    gitignoreMissing,
    instructionStatus,
    rootPackageExists: packageInfo.exists,
    rootScripts: rootScriptStatus,
    trackedGenerated,
    manifest,
    instructionRules,
    c4,
    releaseTruth,
    validation
  };
  status.doctorRepairs = doctorRepairsForStatus(status);
  return status;
}

async function promptYesNo(rl, question, defaultValue) {
  const suffix = defaultValue ? "Y/n" : "y/N";
  const answer = (await rl.question(`${question} [${suffix}] `)).trim().toLowerCase();
  if (!answer) return defaultValue;
  return ["y", "yes"].includes(answer);
}

function nonInteractiveSync(options) {
  return options.yes || options.quiet;
}

function assertSyncPromptOptions(options) {
  if (options.prompt && nonInteractiveSync(options)) {
    throw new Error("--prompt cannot be combined with --yes or --quiet");
  }
}

function normalizeInstructionFiles(files) {
  return instructionFiles.filter((fileName) => files?.includes(fileName));
}

function defaultSyncChoices(rootPackageExists) {
  return {
    branch: "current",
    instructionFiles,
    manageGitignore: true,
    manageRootScripts: rootPackageExists,
    applyDoctorRepairs: true,
    proceedWithChanges: true,
    promptBeforeProceed: false
  };
}

function rememberedSyncChoices(metadata) {
  const choices = metadata?.syncChoices;
  if (!choices || typeof choices !== "object") return null;
  return {
    branch: ["current", "new", "none"].includes(choices.branch) ? choices.branch : "current",
    instructionFiles: normalizeInstructionFiles(choices.instructionFiles),
    manageGitignore: Boolean(choices.manageGitignore),
    manageRootScripts: Boolean(choices.manageRootScripts),
    applyDoctorRepairs: choices.applyDoctorRepairs !== false,
    proceedWithChanges: choices.proceedWithChanges !== false,
    promptBeforeProceed: false
  };
}

function applyExplicitSyncOptions(choices, options) {
  const next = { ...choices };
  if (options.branch) next.branch = options.branch;
  if (options.noAgents) next.instructionFiles = [];
  else if (options.appendAgents) next.instructionFiles = instructionFiles;
  if (options.noGitignore) next.manageGitignore = false;
  else if (options.updateGitignore) next.manageGitignore = true;
  if (options.noRootScripts) next.manageRootScripts = false;
  else if (options.rootScripts) next.manageRootScripts = true;
  return next;
}

function persistedSyncChoices(choices) {
  return {
    branch: choices.branch,
    instructionFiles: choices.instructionFiles,
    manageGitignore: choices.manageGitignore,
    manageRootScripts: choices.manageRootScripts,
    applyDoctorRepairs: choices.applyDoctorRepairs,
    proceedWithChanges: choices.proceedWithChanges
  };
}

async function chooseBranchChoice({ target, options, rl }) {
  if (options.dryRun || !gitAvailable(target)) return "none";
  if (options.branch) return options.branch;
  if (nonInteractiveSync(options)) return "current";
  return await promptYesNo(rl, "Create a new git branch for Architext changes?", false) ? "new" : "current";
}

async function chooseApplyDoctorRepairs(options, rl, doctorRepairAvailable) {
  if (!doctorRepairAvailable) return true;
  if (nonInteractiveSync(options)) return true;
  return promptYesNo(rl, "Apply deterministic doctor repairs during sync?", true);
}

async function promptSyncChoices({ target, options, rl, doctorRepairAvailable }) {
  return applyExplicitSyncOptions({
    branch: await chooseBranchChoice({ target, options, rl }),
    instructionFiles: await chooseInstructionFiles(options, rl),
    manageGitignore: await chooseGitignore(options, rl),
    manageRootScripts: await chooseRootScripts(target, options, rl),
    applyDoctorRepairs: await chooseApplyDoctorRepairs(options, rl, doctorRepairAvailable),
    proceedWithChanges: true,
    promptBeforeProceed: true
  }, options);
}

async function selectSyncChoices({ target, options, rl, metadata, doctorRepairAvailable, rootPackageExists }) {
  const defaults = applyExplicitSyncOptions(defaultSyncChoices(rootPackageExists), options);
  if (nonInteractiveSync(options)) return defaults;

  const savedChoices = options.prompt ? null : rememberedSyncChoices(metadata);
  if (savedChoices && await promptYesNo(rl, "Reuse saved sync choices from the last run?", true)) {
    return applyExplicitSyncOptions({ ...defaults, ...savedChoices }, options);
  }

  return promptSyncChoices({ target, options, rl, doctorRepairAvailable });
}

async function handleBranch({ target, options, version, branchChoice }) {
  if (options.dryRun || branchChoice === "none" || !gitAvailable(target)) return;
  if (branchChoice === "current") return;
  if (branchChoice !== "new") throw new Error("--branch must be current, new, or none");
  const branchName = options.branchName || `architext/data-only-${version.replaceAll(".", "-")}`;
  git(target, ["checkout", "-b", branchName]);
  console.log(`Created and switched to branch ${branchName}`);
}

async function chooseInstructionFiles(options, rl) {
  if (options.noAgents) return [];
  if (options.appendAgents || options.yes) return instructionFiles;
  const selected = [];
  for (const fileName of instructionFiles) {
    if (await promptYesNo(rl, `Create/update ${fileName} Architext instructions?`, true)) selected.push(fileName);
  }
  return selected;
}

async function chooseGitignore(options, rl) {
  if (options.noGitignore) return false;
  if (options.updateGitignore || options.yes) return true;
  return promptYesNo(rl, "Ensure .gitignore excludes Architext generated builds?", true);
}

async function chooseRootScripts(target, options, rl) {
  if (options.noRootScripts) return false;
  if (options.rootScripts) return true;
  if (!(await packageJsonInfo(target)).exists) return false;
  if (options.yes) return true;
  return promptYesNo(rl, "Add root package.json Architext convenience scripts?", true);
}

async function removeCopiedInstallFiles(target, dryRun) {
  const removed = [];
  for (const entryPath of copiedInstallPaths(target)) {
    removed.push(path.relative(target, entryPath));
    if (!dryRun) await rm(entryPath, { recursive: true, force: true });
  }
  if (existsSync(legacyMetadataPath(target))) {
    removed.push(path.relative(target, legacyMetadataPath(target)));
    if (!dryRun) await rm(legacyMetadataPath(target), { force: true });
  }
  return removed;
}

async function applyDoctorRepairs(target, status, dryRun, { skipInstructionRules = false } = {}) {
  const applied = [];
  const categories = doctorRepairCategories(status.doctorRepairs)
    .filter((category) => !(skipInstructionRules && category === "instruction-rules"));
  for (const category of categories) {
    const handler = doctorRepairHandlers[category];
    if (!handler) continue;
    const result = await handler(target, dryRun);
    const repairFiles = {
      c4: path.join(dataDir(target), "views.json"),
      manifest: path.join(dataDir(target), "manifest.json"),
      "release-truth": path.join(dataDir(target), "releases", "index.json"),
      "instruction-rules": path.join(dataDir(target), "rules.json")
    };
    const file = repairFiles[category] ?? dataDir(target);
    for (const change of result.repairChanges ?? []) {
      applied.push({
        category,
        file,
        summary: change
      });
    }
  }
  return applied;
}

async function runDoctor(target, options, version) {
  const status = await collectStatus(target, version, { runValidation: true });
  if (options.json) {
    console.log(JSON.stringify(status, null, 2));
    return;
  }

  printStatus(status, { verbose: true });
  if (!status.installed || status.needsMigration) {
    console.log("Next: architext sync");
    return;
  }
  if (!status.doctorRepairs.length) {
    if (status.validation && !status.validation.ok) {
      console.log("Next: architext prompt --mode repair-validation");
      return;
    }
    console.log("Next: architext serve");
    return;
  }

  if (options.dryRun) {
    console.log("Dry run: no doctor repairs applied.");
    return;
  }

  const rl = createInterface({ input, output });
  try {
    const apply = options.yes || await promptYesNo(rl, "Apply deterministic doctor repairs?", true);
    if (!apply) {
      console.log("No doctor repairs applied.");
      return;
    }
    const repairs = await applyDoctorRepairs(target, status, false);
    console.log("Applied doctor repairs:");
    repairs.forEach((repair) => console.log(`- ${repair.file}: ${repair.summary}`));
    const validation = options.skipValidate ? { ok: true, output: "Validation skipped." } : await validateTarget(target);
    console.log(validation.output);
    if (!validation.ok) process.exit(1);
  } finally {
    rl.close();
  }
}

async function syncTarget(target, options, version) {
  assertSyncPromptOptions(options);
  const status = await collectStatus(target, version, { runValidation: !options.skipValidate });
  const installing = !status.installed || options.overwriteData;
  const migrating = status.needsMigration;
  const effectiveDoctorRepairs = options.noAgents
    ? status.doctorRepairs.filter((repair) => repair.category !== "instruction-rules")
    : status.doctorRepairs;
  const doctorRepairAvailable = Boolean(effectiveDoctorRepairs.length);

  console.log(`Target: ${target}`);
  console.log(`Architext CLI: ${version}`);
  printStatus(status, { verbose: true });
  if (migrating) {
    console.log(`Copied install detected: ${status.copiedInstallPaths.length} package-owned paths`);
  }

  const rl = createInterface({ input, output });
  try {
    const syncChoices = await selectSyncChoices({
      target,
      options,
      rl,
      metadata: status.metadata,
      doctorRepairAvailable,
      rootPackageExists: status.rootPackageExists
    });
    const doctorRepairsSelected = doctorRepairAvailable && syncChoices.applyDoctorRepairs;
    const shouldWrite = installing || migrating || doctorRepairsSelected || options.force || syncChoices.instructionFiles.length > 0 || syncChoices.manageGitignore || syncChoices.manageRootScripts;

    console.log(`Operation: ${installing ? "install" : migrating ? "migrate" : "sync"}${shouldWrite ? "" : " (current)"}`);

    if (!shouldWrite) {
      console.log("No lifecycle changes needed.");
      return;
    }

    await handleBranch({ target, options, version, branchChoice: syncChoices.branch });
    if (!nonInteractiveSync(options) && syncChoices.promptBeforeProceed && !options.dryRun) {
      const proceed = syncChoices.proceedWithChanges && await promptYesNo(rl, "Proceed with selected Architext changes in this branch?", true);
      if (!proceed) {
        console.log("Aborted.");
        return;
      }
    }

    if (installing) {
      console.log(`${options.dryRun ? "Would write" : "Writing"} starter data to ${dataDir(target)}`);
      if (!options.dryRun) await writeStarterData(target, version);
    } else {
      console.log("Preserving target-owned docs/architext/data/**/*.json");
    }

    const removed = migrating ? await removeCopiedInstallFiles(target, options.dryRun) : [];
    if (removed.length) {
      console.log(`${options.dryRun ? "Would remove" : "Removed"} copied package-owned files:`);
      removed.forEach((item) => console.log(`- ${item}`));
    }

    if (!installing && doctorRepairsSelected) {
      const repairs = await applyDoctorRepairs(target, status, options.dryRun, { skipInstructionRules: options.noAgents });
      console.log(`${options.dryRun ? "Would apply" : "Applied"} doctor repairs:`);
      repairs.forEach((repair) => console.log(`- ${repair.file}: ${repair.summary}`));
    } else if (!installing && doctorRepairAvailable) {
      console.log("Skipped doctor repairs.");
    }

    const managedInstructions = [];
    for (const fileName of syncChoices.instructionFiles) {
      const result = await upsertInstructionFile({ target, fileName, dryRun: options.dryRun });
      console.log(result.changed ? `${options.dryRun ? "Would update" : "Updated"} ${result.destination}` : `Skipped ${result.destination}: ${result.reason}`);
      if (result.changed) managedInstructions.push(fileName);
    }

    let gitignoreManaged = false;
    if (syncChoices.manageGitignore) {
      const result = await upsertGitignore({ target, dryRun: options.dryRun });
      console.log(result.changed ? `${options.dryRun ? "Would update" : "Updated"} ${result.destination}` : `Skipped ${result.destination}: ${result.reason}`);
      gitignoreManaged = result.changed || result.reason === "already present";
    }

    let rootScriptsManaged = false;
    if (syncChoices.manageRootScripts) {
      const result = await upsertRootScripts({ target, dryRun: options.dryRun });
      console.log(result.changed ? `${options.dryRun ? "Would update" : "Updated"} ${result.destination} with ${result.missing.length} scripts` : `Skipped ${result.destination}: ${result.reason}`);
      rootScriptsManaged = result.changed || result.reason === "already present";
    }

    const validation = options.skipValidate || (options.dryRun && installing)
      ? null
      : await validateTarget(target);
    if (validation) console.log(`Validation: ${validation.ok ? "passed" : "failed"}`);

    if (!options.dryRun) {
      await writeMetadata(target, {
        source: "architext-cli",
        cliVersion: version,
        operation: installing ? "install" : migrating ? "migrate" : "sync",
        dataPolicy: installing ? "starter-written" : "preserved",
        copiedInstallMigrated: migrating,
        instructionFiles: Object.fromEntries(instructionFiles.map((fileName) => [fileName, syncChoices.instructionFiles.includes(fileName)])),
        managedInstructions,
        gitignoreManaged,
        rootScriptsManaged,
        syncChoices: persistedSyncChoices(syncChoices),
        lastValidation: validation ? { ok: validation.ok, at: new Date().toISOString() } : undefined
      });
    }
  } finally {
    rl.close();
  }
}

async function printPrompt(target, mode) {
  const manifestPath = path.join(dataDir(target), "manifest.json");
  const manifest = existsSync(manifestPath) ? await readJson(manifestPath) : null;
  const projectName = manifest?.project?.name ?? path.basename(target);
  const modes = new Set(["initial-buildout", "architecture-change", "repair-validation", "source-extraction"]);
  const promptMode = modes.has(mode) ? mode : "initial-buildout";
  const lead = {
    "initial-buildout": `Build out Architext for ${projectName}. Replace neutral starter data with source-backed architecture facts.`,
    "architecture-change": `Update Architext for the architecture changes just made in ${projectName}. Keep existing stable IDs where concepts already exist.`,
    "repair-validation": `Repair Architext JSON validation failures for ${projectName}. Do not change application code for this task.`,
    "source-extraction": `Inspect ${projectName} source files and draft proposed Architext data changes. Do not apply the draft silently.`
  }[promptMode];

  console.log(`${lead}

First read AGENTS.md/CLAUDE.md if present, then docs/architext/data/**/*.json.

Rules:
- Update only docs/architext/data/**/*.json unless the Architext package itself is being changed.
- Treat manifest.schemaVersion as the Architext data schema contract version, not the installed CLI/package version. Update it only when the data contract changes or when architext doctor/sync applies a schema repair.
- Reuse stable IDs, create nodes before references, keep flows ordered, and prefer source-path-backed claims.
- Keep Release Truth data current when release scope, blockers, milestones, evidence, target dates, dependencies, or posture changes.
- Treat Release Truth as reviewed release state, not a planning scratchpad: update detail files for completed, deferred, blocked, reprioritized, or newly scoped work, then refresh the generated release index from those facts.
- Keep Release Path labels concise; put rationale, blocker explanation, evidence, dependencies, and next actions in detail data for the selected release item.
- Use docs/architext/data/roadmap.json for release planning source items. Selected roadmap scope uses source: "roadmap"; manually entered scope uses source: "ad-hoc" and must be promoted into roadmap.json when approved.
- Use docs/architext/data/rules.json for project rules. Rule categories are maintainer-defined classifications, not a fixed Architext taxonomy. Respect edit/delete protection and rank rules by criticality and order instead of alphabetizing them.
- Build C4 drilldown chains with explicit scopeNodeId metadata for decomposable Context, Container, and Component nodes; leave actors and external dependencies without child views.
- For source extraction, return a reviewable draft of proposed JSON changes with source paths and confidence notes before editing data files. Validation remains required after any accepted edit.
- Mark uncertainty and known gaps explicitly.
- Do not edit copied viewer, schema, package, Vite, or local tool files in the target repository.
- Run architext validate ${target} before claiming completion.

Required finish:
- Summarize changed data files.
- Summarize covered architecture areas.
- Summarize remaining uncertainty.
- Report validation result.`);
}

async function cleanGenerated(target, options) {
  const candidates = [path.join(architextDir(target), "dist")];
  if (options.nodeModules) candidates.push(path.join(architextDir(target), "node_modules"));
  const removed = [];
  for (const candidate of candidates) {
    if (existsSync(candidate)) {
      removed.push(candidate);
      if (!options.dryRun) await rm(candidate, { recursive: true, force: true });
    }
  }
  console.log(removed.length ? `${options.dryRun ? "Would remove" : "Removed"}:\n${removed.map((item) => `- ${item}`).join("\n")}` : "No generated Architext artifacts found.");
}

async function explainTopic(topic) {
  const normalized = (topic || "overview").toLowerCase();
  const schemaMap = {
    manifest: "manifest.schema.json",
    nodes: "nodes.schema.json",
    node: "nodes.schema.json",
    flows: "flows.schema.json",
    flow: "flows.schema.json",
    views: "views.schema.json",
    view: "views.schema.json",
    data: "data-classification.schema.json",
    risks: "risks.schema.json",
    risk: "risks.schema.json",
    decisions: "decisions.schema.json",
    decision: "decisions.schema.json",
    glossary: "glossary.schema.json",
    releases: "release-index.schema.json",
    release: "release-detail.schema.json"
  };
  const schemaFile = schemaMap[normalized];
  if (!schemaFile) {
    console.log("Architext data is split across manifest, nodes, flows, views, data classification, decisions, risks, glossary, and optional releases JSON files.");
    return;
  }
  const schema = await readJson(path.join(schemaDir, schemaFile));
  console.log(`${normalized}: package schema ${schemaFile}`);
  if (schema.required?.length) console.log(`Required fields: ${schema.required.join(", ")}`);
}

async function buildStatic(target, options) {
  const outDir = path.resolve(target, options.out || path.join("docs", "architext", "dist"));
  if (!existsSync(path.join(viewerDistDir, "index.html"))) {
    throw new Error("Package viewer assets are missing. Run npm run build before packing Architext.");
  }
  await rm(outDir, { recursive: true, force: true });
  await cp(viewerDistDir, outDir, { recursive: true });
  await mkdir(path.join(outDir, "data"), { recursive: true });
  await cp(dataDir(target), path.join(outDir, "data"), { recursive: true });
  console.log(`Copied target data to ${path.join(outDir, "data")}`);
}

const contentTypes = {
  ".css": "text/css; charset=utf-8",
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml; charset=utf-8"
};

function safeJoin(root, requestPath) {
  const decoded = decodeURIComponent(requestPath);
  const resolved = path.resolve(root, decoded.replace(/^\/+/, ""));
  if (resolved !== root && !resolved.startsWith(`${root}${path.sep}`)) return "";
  return resolved;
}

async function sendFile(response, file) {
  const body = await readFile(file);
  response.writeHead(200, { "content-type": contentTypes[path.extname(file)] || "application/octet-stream" });
  response.end(body);
}

function sendJson(response, status, payload) {
  response.writeHead(status, { "content-type": "application/json; charset=utf-8" });
  response.end(`${JSON.stringify(payload, null, 2)}\n`);
}

function sendServerError(response, error, requestPath) {
  console.error(`Architext serve failed for ${requestPath}: ${error.message}`);
  response.writeHead(500, { "content-type": "text/plain; charset=utf-8" });
  response.end("Architext could not serve this request. Run `architext doctor [path]` and check the terminal output for details.");
}

async function requestJson(request) {
  const chunks = [];
  let total = 0;
  for await (const chunk of request) {
    total += chunk.length;
    if (total > 1024 * 1024) throw new Error("Request body is too large");
    chunks.push(chunk);
  }
  return JSON.parse(Buffer.concat(chunks).toString("utf8") || "{}");
}

async function approveReleasePlanRequest(target, payload) {
  return approveReleasePlanApiRequest({
    target,
    payload,
    dataDir,
    readJson,
    writeJson,
    validateTarget
  });
}

async function updateRulesRequest(target, payload) {
  return updateRulesApiRequest({
    target,
    payload,
    dataDir,
    readJson,
    writeJson,
    validateTarget
  });
}

async function serveTarget(target) {
  if (!existsSync(path.join(viewerDistDir, "index.html"))) {
    throw new Error("Package viewer assets are missing. Run npm run build before serving Architext.");
  }
  const targetDataDir = dataDir(target);
  const watchHub = createDataWatchHub({ target, dataDir, validateTarget });
  watchHub.start();
  const server = createServer(createViewerRequestHandler({ target, targetDataDir, watchHub }));

  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(4317, "127.0.0.1", resolve);
  });
  server.once("close", () => watchHub.close());
  console.log(`Serving Architext for ${target}`);
  console.log("Open http://127.0.0.1:4317");
}

export function createViewerRequestHandler({ target, targetDataDir = dataDir(target), watchHub }) {
  return async function viewerRequestHandler(request, response) {
    try {
      const url = new URL(request.url || "/", "http://127.0.0.1");
      if (url.pathname === "/api/data-events" && request.method === "GET") {
        watchHub.attach(response);
        return;
      }

      if (url.pathname === "/api/release-plans" && request.method === "POST") {
        try {
          const result = await approveReleasePlanRequest(target, await requestJson(request));
          sendJson(response, 200, result);
        } catch (error) {
          sendJson(response, 400, { error: error.message });
        }
        return;
      }

      if (url.pathname === "/api/rules" && request.method === "POST") {
        try {
          const result = await updateRulesRequest(target, await requestJson(request));
          sendJson(response, 200, result);
        } catch (error) {
          sendJson(response, 400, { error: error.message });
        }
        return;
      }

      if (url.pathname.startsWith("/api/")) {
        sendJson(response, 404, { error: `Unknown Architext API route: ${url.pathname}` });
        return;
      }

      if (url.pathname.startsWith("/data/")) {
        const dataFile = safeJoin(targetDataDir, url.pathname.slice("/data/".length));
        if (!dataFile || !(await stat(dataFile).catch(() => null))?.isFile()) {
          response.writeHead(404);
          response.end("Not found");
          return;
        }
        await sendFile(response, dataFile);
        return;
      }

      const assetPath = url.pathname === "/" ? "index.html" : url.pathname;
      const assetFile = safeJoin(viewerDistDir, assetPath);
      const assetStat = assetFile ? await stat(assetFile).catch(() => null) : null;
      await sendFile(response, assetStat?.isFile() ? assetFile : path.join(viewerDistDir, "index.html"));
    } catch (error) {
      sendServerError(response, error, request.url || "/");
    }
  };
}

function commandHandlers(version) {
  return createCommandHandlers({
    sync: (target, options) => syncTarget(target, options, version),
    serve: (target) => serveTarget(target),
    validate: async (target) => {
      const validation = await validateTarget(target);
      console.log(validation.output);
      if (!validation.ok) process.exit(1);
    },
    build: (target, options) => buildStatic(target, options),
    prompt: (target, options) => printPrompt(target, options.mode),
    clean: (target, options) => cleanGenerated(target, options),
    explain: (_target, options) => explainTopic(options.topic),
    status: async (target, options) => {
      const status = await collectStatus(target, version);
      if (options.json) console.log(JSON.stringify(status, null, 2));
      else {
        printStatus(status);
        if (!status.installed || status.needsMigration) console.log("Next: architext sync");
        else if (status.doctorRepairs.length) console.log("Next: architext doctor");
        else console.log("Next: architext serve");
      }
    },
    doctor: (target, options) => runDoctor(target, options, version)
  });
}

export async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.command === "help") {
    console.log(usage());
    return;
  }

  const version = await packageVersion();
  if (options.command === "version") {
    console.log(version);
    return;
  }

  const target = path.resolve(options.target || process.cwd());
  if (options.command !== "explain") await assertTarget(target);

  return routeCommand({ options, target, handlers: commandHandlers(version) });
}
