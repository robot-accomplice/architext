import { watch } from "node:fs";

export function createDataWatchHub({
  target,
  dataDir,
  validateTarget,
  settleMs = 300,
  watchFn = watch,
  setTimer = setTimeout,
  clearTimer = clearTimeout
}) {
  const clients = new Set();
  let timer = null;
  let watcher = null;
  let version = 0;

  const broadcast = (payload) => {
    const body = `data: ${JSON.stringify(payload)}\n\n`;
    for (const client of clients) client.write(body);
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
    response.writeHead(200, {
      "content-type": "text/event-stream",
      "cache-control": "no-cache, no-transform",
      connection: "keep-alive"
    });
    response.write("\n");
    clients.add(response);
    response.on("close", () => clients.delete(response));
  };

  const close = () => {
    if (timer) clearTimer(timer);
    timer = null;
    watcher?.close?.();
    watcher = null;
    for (const client of clients) client.end();
    clients.clear();
  };

  return { attach, close, schedule, start };
}
