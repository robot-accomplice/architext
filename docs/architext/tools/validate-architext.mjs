import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";
import { validateReleaseReferences } from "../../../src/domain/architecture-model/references.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function parseArgs(argv) {
  const options = {
    dataDir: path.join(root, "data"),
    schemaDir: path.join(root, "schema")
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--data-dir") {
      options.dataDir = path.resolve(argv[++index] ?? "");
    } else if (arg === "--schema-dir") {
      options.schemaDir = path.resolve(argv[++index] ?? "");
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }

  return options;
}

const options = parseArgs(process.argv.slice(2));
const dataDir = options.dataDir;
const schemaDir = options.schemaDir;

const schemaFiles = {
  manifest: "manifest.schema.json",
  nodes: "nodes.schema.json",
  flows: "flows.schema.json",
  views: "views.schema.json",
  dataClassification: "data-classification.schema.json",
  decisions: "decisions.schema.json",
  risks: "risks.schema.json",
  glossary: "glossary.schema.json"
};

const releaseSchemaFiles = {
  releaseIndex: "release-index.schema.json",
  releaseDetail: "release-detail.schema.json"
};

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function requireUnique(items, label, errors) {
  const seen = new Set();
  for (const item of items) {
    if (seen.has(item.id)) {
      errors.push(`${label} contains duplicate id "${item.id}"`);
    }
    seen.add(item.id);
  }
}

function requireKnown(id, known, context, errors) {
  if (!known.has(id)) {
    errors.push(`${context} references unknown id "${id}"`);
  }
}

function validateReferences(model, errors) {
  const nodeIds = new Set(model.nodes.nodes.map((node) => node.id));
  const flowIds = new Set(model.flows.flows.map((flow) => flow.id));
  const dataIds = new Set(model.dataClassification.classes.map((item) => item.id));
  const decisionIds = new Set(model.decisions.decisions.map((item) => item.id));
  const riskIds = new Set(model.risks.risks.map((item) => item.id));
  const viewIds = new Set(model.views.views.map((item) => item.id));

  requireKnown(model.manifest.defaultViewId, viewIds, "manifest.defaultViewId", errors);

  for (const node of model.nodes.nodes) {
    for (const dependencyId of node.dependencies) requireKnown(dependencyId, nodeIds, `node ${node.id}.dependencies`, errors);
    for (const dataId of node.dataHandled) requireKnown(dataId, dataIds, `node ${node.id}.dataHandled`, errors);
    for (const flowId of node.relatedFlows) requireKnown(flowId, flowIds, `node ${node.id}.relatedFlows`, errors);
    for (const decisionId of node.relatedDecisions) requireKnown(decisionId, decisionIds, `node ${node.id}.relatedDecisions`, errors);
    for (const riskId of node.knownRisks) requireKnown(riskId, riskIds, `node ${node.id}.knownRisks`, errors);
  }

  for (const flow of model.flows.flows) {
    for (const actorId of flow.actors) requireKnown(actorId, nodeIds, `flow ${flow.id}.actors`, errors);
    const stepIds = new Set();
    for (const step of flow.steps) {
      if (stepIds.has(step.id)) errors.push(`flow ${flow.id} contains duplicate step id "${step.id}"`);
      stepIds.add(step.id);
      requireKnown(step.from, nodeIds, `flow ${flow.id} step ${step.id}.from`, errors);
      requireKnown(step.to, nodeIds, `flow ${flow.id} step ${step.id}.to`, errors);
      for (const dataId of step.data) requireKnown(dataId, dataIds, `flow ${flow.id} step ${step.id}.data`, errors);
    }
  }

  for (const view of model.views.views) {
    const laneIds = new Set();
    for (const lane of view.lanes) {
      if (laneIds.has(lane.id)) errors.push(`view ${view.id} contains duplicate lane id "${lane.id}"`);
      laneIds.add(lane.id);
      for (const nodeId of lane.nodeIds) requireKnown(nodeId, nodeIds, `view ${view.id} lane ${lane.id}`, errors);
    }
  }

  for (const decision of model.decisions.decisions) {
    for (const nodeId of decision.relatedNodes) requireKnown(nodeId, nodeIds, `decision ${decision.id}.relatedNodes`, errors);
    for (const flowId of decision.relatedFlows) requireKnown(flowId, flowIds, `decision ${decision.id}.relatedFlows`, errors);
  }

  for (const risk of model.risks.risks) {
    for (const nodeId of risk.relatedNodes) requireKnown(nodeId, nodeIds, `risk ${risk.id}.relatedNodes`, errors);
    for (const flowId of risk.relatedFlows) requireKnown(flowId, flowIds, `risk ${risk.id}.relatedFlows`, errors);
  }
}

const ajv = new Ajv2020({ allErrors: true, strict: true });
addFormats(ajv);

const schemas = Object.fromEntries(
  Object.entries(schemaFiles).map(([key, file]) => [key, readJson(path.join(schemaDir, file))])
);
const releaseSchemas = Object.fromEntries(
  Object.entries(releaseSchemaFiles).map(([key, file]) => [key, readJson(path.join(schemaDir, file))])
);

for (const schema of [...Object.values(schemas), ...Object.values(releaseSchemas)]) {
  ajv.addSchema(schema);
}

const manifest = readJson(path.join(dataDir, "manifest.json"));
const releases = manifest.files.releases ? readReleases(dataDir, manifest.files.releases) : null;
const model = {
  manifest,
  nodes: readJson(path.join(dataDir, manifest.files.nodes)),
  flows: readJson(path.join(dataDir, manifest.files.flows)),
  views: readJson(path.join(dataDir, manifest.files.views)),
  dataClassification: readJson(path.join(dataDir, manifest.files.dataClassification)),
  decisions: readJson(path.join(dataDir, manifest.files.decisions)),
  risks: readJson(path.join(dataDir, manifest.files.risks)),
  glossary: readJson(path.join(dataDir, manifest.files.glossary)),
  ...(releases ? { releases } : {})
};

const errors = [];

for (const [key, schema] of Object.entries(schemas)) {
  const validate = ajv.getSchema(schema.$id);
  const value = key === "dataClassification" ? model.dataClassification : model[key];
  if (!validate(value)) {
    for (const error of validate.errors ?? []) {
      errors.push(`${key}${error.instancePath}: ${error.message}`);
    }
  }
}

requireUnique(model.nodes.nodes, "nodes", errors);
requireUnique(model.flows.flows, "flows", errors);
requireUnique(model.views.views, "views", errors);
requireUnique(model.dataClassification.classes, "dataClassification.classes", errors);
requireUnique(model.decisions.decisions, "decisions", errors);
requireUnique(model.risks.risks, "risks", errors);
validateReferences(model, errors);

if (model.releases) {
  validateReleaseData(model.releases, errors);
}

if (errors.length > 0) {
  console.error("Architext validation failed:");
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}

console.log("Architext validation passed.");

function readReleases(baseDir, indexFile) {
  const indexPath = path.join(baseDir, indexFile);
  const index = readJson(indexPath);
  const detailBaseDir = path.dirname(indexPath);
  return {
    index,
    details: index.releases.map((release) => readJson(path.join(detailBaseDir, release.file)))
  };
}

function validateReleaseData(releases, errors) {
  const releaseIndexSchema = ajv.getSchema(releaseSchemas.releaseIndex.$id);
  if (!releaseIndexSchema(releases.index)) {
    for (const error of releaseIndexSchema.errors ?? []) {
      errors.push(`releaseIndex${error.instancePath}: ${error.message}`);
    }
  }

  const releaseDetailSchema = ajv.getSchema(releaseSchemas.releaseDetail.$id);
  for (const detail of releases.details) {
    if (!releaseDetailSchema(detail)) {
      for (const error of releaseDetailSchema.errors ?? []) {
        errors.push(`release ${detail.id ?? "(unknown)"}${error.instancePath}: ${error.message}`);
      }
    }
  }

  requireUnique(releases.index.releases, "releases.index", errors);
  requireUnique(releases.details, "releases.details", errors);
  validateReleaseReferences(releases, errors);
}
