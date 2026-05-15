export function validateArchitectureReferences(model) {
  const errors = [];
  const nodeIds = new Set(model.nodes.map((node) => node.id));
  const flowIds = new Set(model.flows.map((flow) => flow.id));
  const dataIds = new Set(model.dataClasses.map((item) => item.id));
  const decisionIds = new Set(model.decisions.map((item) => item.id));
  const riskIds = new Set(model.risks.map((item) => item.id));
  const viewIds = new Set(model.views.map((item) => item.id));

  const requireKnown = (id, known, context) => {
    if (!known.has(id)) errors.push(`${context} references unknown id "${id}"`);
  };

  requireKnown(model.manifest.defaultViewId, viewIds, "manifest.defaultViewId");

  for (const node of model.nodes) {
    for (const id of node.dependencies) requireKnown(id, nodeIds, `node ${node.id}.dependencies`);
    for (const id of node.dataHandled) requireKnown(id, dataIds, `node ${node.id}.dataHandled`);
    for (const id of node.relatedFlows) requireKnown(id, flowIds, `node ${node.id}.relatedFlows`);
    for (const id of node.relatedDecisions) requireKnown(id, decisionIds, `node ${node.id}.relatedDecisions`);
    for (const id of node.knownRisks) requireKnown(id, riskIds, `node ${node.id}.knownRisks`);
  }

  for (const flow of model.flows) {
    for (const id of flow.actors) requireKnown(id, nodeIds, `flow ${flow.id}.actors`);
    for (const step of flow.steps) {
      requireKnown(step.from, nodeIds, `flow ${flow.id} step ${step.id}.from`);
      requireKnown(step.to, nodeIds, `flow ${flow.id} step ${step.id}.to`);
      for (const id of step.data) requireKnown(id, dataIds, `flow ${flow.id} step ${step.id}.data`);
    }
  }

  for (const view of model.views) {
    for (const lane of view.lanes) {
      for (const id of lane.nodeIds) requireKnown(id, nodeIds, `view ${view.id} lane ${lane.id}`);
    }
  }

  return errors;
}
