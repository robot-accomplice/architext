import { watch } from "node:fs";

export function createDataWatchHub({
  target,
  dataDir,
  validateTarget,
  settleMs = 300,
  maxClients = 32,
  heartbeatMs = 30000,
  watchFn = watch,
  setTimer = setTimeout,
  clearTimer = clearTimeout,
  setIntervalFn = setInterval,
  clearIntervalFn = clearInterval
}) {
  const clients = new Set();
  let timer = null;
  let heartbeatTimer = null;
  let watcher = null;
  let version = 0;

  const stopHeartbeat = () => {
    if (!heartbeatTimer) return;
    clearIntervalFn(heartbeatTimer);
    heartbeatTimer = null;
  };

  const closeClient = (client) => {
    if (!clients.delete(client)) return;
    client.end();
    if (clients.size === 0) stopHeartbeat();
  };

  const writeToClient = (client, body) => {
    if (!client.write(body)) closeClient(client);
  };

  const broadcast = (payload) => {
    const body = `data: ${JSON.stringify(payload)}\n\n`;
    for (const client of [...clients]) writeToClient(client, body);
  };

  const startHeartbeat = () => {
    if (heartbeatTimer || heartbeatMs <= 0) return;
    heartbeatTimer = setIntervalFn(() => {
      for (const client of [...clients]) writeToClient(client, ": heartbeat\n\n");
    }, heartbeatMs);
    heartbeatTimer?.unref?.();
  };

  const validateAndBroadcast = async () => {
    timer = null;
    const validation = await validateTarget(target);
    version += 1;
    broadcast({
      type: validation.ok ? "valid" : "invalid",
      version,
      output: validation.output
    });
  };

  const schedule = (fileName = "") => {
    if (fileName && !fileName.endsWith(".json")) return;
    if (timer) clearTimer(timer);
    timer = setTimer(() => {
      validateAndBroadcast().catch((error) => {
        version += 1;
        broadcast({
          type: "invalid",
          version,
          output: error instanceof Error ? error.message : String(error)
        });
      });
    }, settleMs);
  };

  const start = () => {
    if (watcher) return;
    watcher = watchFn(dataDir(target), { recursive: true }, (_eventType, fileName) => schedule(String(fileName ?? "")));
  };

  const attach = (response) => {
    if (clients.size >= maxClients) {
      response.writeHead(503, { "content-type": "application/json; charset=utf-8" });
      response.end(`${JSON.stringify({ error: "Too many Architext data event clients." })}\n`);
      return;
    }
    response.writeHead(200, {
      "content-type": "text/event-stream",
      "cache-control": "no-cache, no-transform",
      connection: "keep-alive"
    });
    clients.add(response);
    response.socket?.setTimeout?.(heartbeatMs * 3, () => response.destroy?.());
    response.on("close", () => {
      clients.delete(response);
      if (clients.size === 0) stopHeartbeat();
    });
    response.on("error", () => closeClient(response));
    writeToClient(response, "\n");
    startHeartbeat();
  };

  const close = () => {
    if (timer) clearTimer(timer);
    timer = null;
    watcher?.close?.();
    watcher = null;
    stopHeartbeat();
    for (const client of clients) client.end();
    clients.clear();
  };

  return { attach, close, schedule, start };
}
