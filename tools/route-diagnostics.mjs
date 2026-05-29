#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { planDiagram } from "../viewer/src/routing/planDiagram.js";
import { diagramLayoutFor } from "../viewer/src/presentation/diagramLayout.js";
import { diagnosePlannedRoutes } from "../viewer/src/routing/routeDiagnostics.js";

function usage() {
  console.error("Usage: node tools/route-diagnostics.mjs --data-dir <path> --view <view-id-or-name> [--flow <flow-id-or-name>] [--json]");
  process.exit(2);
}

function parseArgs(argv) {
  const args = {};
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--json") {
      args.json = true;
    } else if (arg.startsWith("--")) {
      const key = arg.slice(2);
      const value = argv[index + 1];
      if (!value || value.startsWith("--")) usage();
      args[key] = value;
      index += 1;
    } else {
      usage();
    }
  }
  if (!args["data-dir"] || !args.view) usage();
  return args;
}

function readJson(dataDir, name) {
  return JSON.parse(readFileSync(resolve(dataDir, `${name}.json`), "utf8"));
}

function matchByIdOrName(items, query, label) {
  const item = items.find((candidate) => candidate.id === query || candidate.name === query);
  if (!item) {
    throw new Error(`Unknown ${label}: ${query}`);
  }
  return item;
}

function relationshipsForFlow(flow, view) {
  const visible = new Set(view.lanes.flatMap((lane) => lane.nodeIds));
  return flow.steps
    .map((step, index) => ({
      id: step.id,
      from: step.from,
      to: step.to,
      label: `${index + 1}. ${step.action}`,
      relationshipType: "flow",
      stepId: step.id,
      displayIndex: index + 1,
      kind: step.kind,
      returnOf: step.returnOf,
      outcome: step.outcome
    }))
    .filter((relationship) => visible.has(relationship.from) && visible.has(relationship.to));
}

function structuralRelationships(nodes, view) {
  const visible = new Set(view.lanes.flatMap((lane) => lane.nodeIds));
  const nodesById = new Map(nodes.map((node) => [node.id, node]));
  return [...visible].flatMap((nodeId) => {
    const node = nodesById.get(nodeId);
    return (node?.dependencies ?? [])
      .filter((dependencyId) => visible.has(dependencyId))
      .map((dependencyId) => ({
        id: `${nodeId}->${dependencyId}`,
        from: nodeId,
        to: dependencyId,
        label: `${node.name ?? nodeId} -> ${nodesById.get(dependencyId)?.name ?? dependencyId}`
      }));
  });
}

function printText(view, flow, diagnostics) {
  console.log(`Route diagnostics: ${view.name}${flow ? ` / ${flow.name}` : " / structural"}`);
  console.log(`Routes: ${diagnostics.metrics.routes}`);
  console.log(`Findings: ${diagnostics.metrics.findings}`);
  console.log(`Bends: ${diagnostics.metrics.bends}`);
  console.log(`Shared segments: ${diagnostics.metrics.sharedSegments}`);
  console.log(`Repeated crossings: ${diagnostics.metrics.repeatedCrossings}`);
  console.log(`Close parallel runs: ${diagnostics.metrics.closeParallelRuns}`);
  console.log(`Hops: ${diagnostics.metrics.hops}`);
  console.log("");
  for (const route of diagnostics.routes) {
    const step = route.step ? `${route.step}. ` : "";
    console.log(`${step}${route.relationshipId}: ${route.from}(${route.sourceSide}) -> ${route.to}(${route.targetSide})`);
    console.log(`  expected: ${route.expectedSourceSide} -> ${route.expectedTargetSide}; uses: ${route.sourceSideUseCount}/${route.sourceSideCapacity} -> ${route.targetSideUseCount}/${route.targetSideCapacity}; offsets: ${route.sourceOffset}, ${route.targetOffset}`);
    console.log(`  bends=${route.bends} crossings=${route.crossings} repeated=${route.repeatedCrossings} shared=${route.sharedSegments} self=${route.selfOverlappingSegments} hops=${route.hopCount}`);
    for (const constraint of route.constraints ?? []) {
      console.log(`  - ${constraint.code}: ${constraint.message}`);
    }
    for (const finding of route.findings) {
      console.log(`  ! ${finding.code}: ${finding.message}`);
    }
  }
  const globalFindings = diagnostics.findings.filter((finding) => !finding.relationshipId);
  for (const finding of globalFindings) {
    console.log(`! ${finding.code}: ${finding.message}`);
  }
}

try {
  const args = parseArgs(process.argv.slice(2));
  const dataDir = resolve(args["data-dir"]);
  const views = readJson(dataDir, "views").views;
  const nodes = readJson(dataDir, "nodes").nodes;
  const view = matchByIdOrName(views, args.view, "view");
  const flow = args.flow ? matchByIdOrName(readJson(dataDir, "flows").flows, args.flow, "flow") : null;
  const relationships = flow ? relationshipsForFlow(flow, view) : structuralRelationships(nodes, view);
  const layout = diagramLayoutFor(view, relationships.length);
  const plan = planDiagram({
    view,
    relationships,
    visibleNodeIds: new Set(view.lanes.flatMap((lane) => lane.nodeIds)),
    ...layout,
    style: "orthogonal",
    diagnostics: true
  });
  const diagnostics = plan.diagnostics ?? diagnosePlannedRoutes(plan, relationships);
  if (args.json) {
    console.log(JSON.stringify({ view: view.id, flow: flow?.id, ...diagnostics }, null, 2));
  } else {
    printText(view, flow, diagnostics);
  }
} catch (error) {
  console.error(error.message);
  process.exit(1);
}
