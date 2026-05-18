export function selectedStepIdForSelection(selection) {
  if (selection?.kind === "step") return selection.stepId;
  if (selection?.kind === "relationship") return selection.stepId ?? null;
  return null;
}

export function selectedFlowIdForSelection(selection) {
  if (selection?.kind === "step") return selection.flowId;
  if (selection?.kind === "relationship") return selection.flowId ?? null;
  return null;
}

export function isSelectedStep(selection, flowId, stepId) {
  return selectedStepIdForSelection(selection) === stepId && selectedFlowIdForSelection(selection) === flowId;
}

export function orderSelectedLast(items, isSelected) {
  return [...items].sort((left, right) => Number(isSelected(left)) - Number(isSelected(right)));
}
