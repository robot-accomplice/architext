const nodeTypeIcons = {
  actor: "actor",
  "software-system": "system",
  client: "client",
  service: "service",
  module: "module",
  worker: "worker",
  queue: "queue",
  "data-store": "database",
  "external-service": "external",
  "deployment-unit": "package",
  "trust-boundary": "shield"
};

const stepKindIcons = {
  start: "start",
  stop: "stop",
  decision: "decision",
  async: "queue",
  persistence: "database",
  artifact: "artifact",
  return: "return",
  process: "process"
};

export function iconForNodeType(type) {
  return nodeTypeIcons[type] ?? "node";
}

export function iconForStep(step, index, totalSteps) {
  if (step?.kind && stepKindIcons[step.kind]) return stepKindIcons[step.kind];
  if (index === 0) return "start";
  if (index === totalSteps - 1) return "stop";
  return "process";
}

export function iconLabel(icon) {
  const labels = {
    actor: "Actor",
    artifact: "Artifact",
    client: "Client",
    database: "Data store",
    decision: "Decision",
    external: "External service",
    file: "File",
    folder: "Folder",
    "folder-open": "Folder",
    module: "Module",
    node: "Node",
    package: "Deployment unit",
    process: "Process",
    queue: "Queue",
    return: "Return",
    service: "Service",
    shield: "Trust boundary",
    start: "Start",
    stop: "Stop",
    system: "Software system",
    worker: "Worker"
  };
  return labels[icon] ?? icon;
}
