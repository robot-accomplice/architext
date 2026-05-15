const targetSpecificLabels = {
  "api-server": "calls API",
  "websocket-control-plane": "uses websocket/control",
  "daemon-runtime": "uses runtime",
  "unified-pipeline": "runs turn",
  "llm-service": "requests model",
  "memory-system": "retrieves memory",
  "mcp-system": "uses MCP/tools",
  "skill-plugin-system": "uses skills/plugins",
  "scheduler": "schedules work",
  "config-keystore": "reads config",
  "observability-system": "records telemetry",
  "external-channel-adapters": "uses channel"
};

export function relationshipLabel(from, to) {
  if (!from || !to) return "relates to";
  if (targetSpecificLabels[to.id]) return targetSpecificLabels[to.id];
  if (to.type === "data-store") return "reads/writes";
  if (to.type === "queue") return "publishes";
  if (to.type === "external-service") return "uses provider";
  if (from.type === "actor") return "uses";
  if (from.type === "client") return "calls";
  return "depends on";
}
