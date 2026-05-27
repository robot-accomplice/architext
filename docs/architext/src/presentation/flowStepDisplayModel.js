export function flowStepDisplayIndexes(steps) {
  const displayIndexes = new Map();
  const latestDecisionIndexByNode = new Map();
  steps.forEach((step, index) => {
    const decisionIndex = step.outcome ? latestDecisionIndexByNode.get(step.from) : undefined;
    displayIndexes.set(step.id, decisionIndex ?? index + 1);
    if (step.kind === "decision") latestDecisionIndexByNode.set(step.to, index + 1);
  });
  return displayIndexes;
}

export function decisionBranchTargets(steps) {
  const targets = new Map();
  for (const step of steps) {
    if (step.kind === "decision") targets.set(step.to, step);
  }
  const branchedTargets = new Set();
  for (const step of steps) {
    if (step.outcome && targets.has(step.from)) branchedTargets.add(step.from);
  }
  return branchedTargets;
}

export function isDecisionBranchSupportStep(steps, step, index) {
  const displayIndex = flowStepDisplayIndexes(steps).get(step.id) ?? index + 1;
  return Boolean(step.outcome && displayIndex !== index + 1);
}

export function isRenderedFlowRelationshipStep(steps, step, index) {
  return true;
}
