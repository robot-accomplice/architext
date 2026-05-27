const routeClasses = {
  flow: "flow-step-route",
  sequence: "sequence-step-route"
};

const sequenceMessageKinds = new Set(["request", "return", "async", "persistence", "self"]);

export function stepRouteClassName(kind) {
  const className = routeClasses[kind];
  if (!className) throw new Error(`Unknown step route kind "${kind}"`);
  return className;
}

export function stepRouteMarkerClassName(className = "") {
  return `route-step-marker ${className}`.trim();
}

export function stepRouteLabelClassName(className = "") {
  return `route-step-label ${className}`.trim();
}

export function sequenceStepMessageKind(step, fromX, toX) {
  if (sequenceMessageKinds.has(step.kind)) return step.kind;
  const text = `${step.action ?? ""} ${step.summary ?? ""}`.toLowerCase();
  if (text.match(/\b(return|respond|response|ack|acknowledge|result)\b/)) return "return";
  if (step.to?.includes("queue")) return "async";
  if (step.to?.includes("db") || step.to?.includes("store")) return "persistence";
  if (step.from === step.to) return "self";
  if (toX < fromX) return "return";
  return "request";
}

export function sequenceReturnSourceStep(step, priorSteps) {
  if (step.returnOf) {
    return priorSteps.find((candidate) => candidate.id === step.returnOf) ?? null;
  }
  return [...priorSteps].reverse().find((candidate) => (
    candidate.from === step.to && candidate.to === step.from
  )) ?? null;
}

export function sequenceActivationSpans(steps, rowHeight = 56) {
  const indexById = new Map(steps.map((step, index) => [step.id, index]));
  const returnBySourceId = new Map();
  steps.forEach((step, index) => {
    if (sequenceStepMessageKind(step, index, index) !== "return") return;
    const sourceStep = sequenceReturnSourceStep(step, steps.slice(0, index));
    if (sourceStep && !returnBySourceId.has(sourceStep.id)) {
      returnBySourceId.set(sourceStep.id, step);
    }
  });

  const baseSpans = steps.flatMap((step, index) => {
    const kind = sequenceStepMessageKind(step, index, index);
    if (kind === "return") return [];
    const returnStep = returnBySourceId.get(step.id);
    const endIndex = returnStep ? indexById.get(returnStep.id) : undefined;
    return [{
      id: `activation-${step.id}`,
      participantId: kind === "self" ? step.from : step.to,
      startIndex: index,
      endIndex: endIndex ?? index,
      y1: index * rowHeight - 10,
      y2: (endIndex ?? index) * rowHeight + (returnStep ? 10 : rowHeight * 0.65)
    }];
  });

  return baseSpans.map((span, index) => {
    const depth = baseSpans
      .slice(0, index)
      .filter((candidate) => (
        candidate.participantId === span.participantId &&
        candidate.startIndex <= span.startIndex &&
        candidate.endIndex >= span.startIndex
      )).length;
    return { ...span, depth };
  });
}
