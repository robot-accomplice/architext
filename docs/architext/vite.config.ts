import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import fsSync from "node:fs";
import fs from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { approveReleasePlanRequest } from "../../src/adapters/http/release-planning-api.mjs";
import { readJson, tryRun, writeJson } from "../../src/adapters/cli/runtime.mjs";
import { dataDir as targetDataDir } from "../../src/domain/lifecycle/target-layout.mjs";

const dataDir = process.env.ARCHITEXT_DATA_DIR;
const configDir = path.dirname(fileURLToPath(import.meta.url));
const packageJson = JSON.parse(fsSync.readFileSync(path.resolve(configDir, "../..", "package.json"), "utf8")) as { version: string };
const packageRoot = path.resolve(configDir, "../..");
const schemaDir = path.join(configDir, "schema");
const validatorPath = path.join(configDir, "tools", "validate-architext.mjs");

async function requestJson(request: { on(event: string, listener: (chunk: Buffer) => void): void; once(event: string, listener: () => void): void }) {
  const chunks: Buffer[] = [];
  await new Promise<void>((resolve) => {
    request.on("data", (chunk) => chunks.push(chunk));
    request.once("end", resolve);
  });
  return JSON.parse(Buffer.concat(chunks).toString("utf8") || "{}");
}

function sendJson(response: { statusCode: number; setHeader(name: string, value: string): void; end(body: string): void }, status: number, value: unknown) {
  response.statusCode = status;
  response.setHeader("Content-Type", "application/json; charset=utf-8");
  response.end(JSON.stringify(value));
}

async function validateTarget(target: string) {
  if (!fsSync.existsSync(path.join(targetDataDir(target), "manifest.json"))) {
    return { ok: false, output: `Architext data is not installed at ${targetDataDir(target)}` };
  }
  return tryRun(process.execPath, [validatorPath, "--data-dir", targetDataDir(target), "--schema-dir", schemaDir], packageRoot);
}

async function approveReleasePlan(target: string, payload: unknown) {
  return approveReleasePlanRequest({
    target,
    payload,
    dataDir: targetDataDir,
    readJson,
    writeJson,
    validateTarget
  });
}

export default defineConfig({
  define: {
    __ARCHITEXT_VERSION__: JSON.stringify(packageJson.version)
  },
  plugins: [
    react(),
    {
      name: "architext-target-data",
      configureServer(server) {
        if (!dataDir) return;
        const target = path.resolve(dataDir, "../../..");
        server.middlewares.use(async (request, response, next) => {
          if (request.url?.startsWith("/api/release-plans") && request.method === "POST") {
            try {
              sendJson(response, 200, await approveReleasePlan(target, await requestJson(request)));
            } catch (error) {
              sendJson(response, 400, { error: error instanceof Error ? error.message : String(error) });
            }
            return;
          }

          if (!request.url?.startsWith("/data/")) {
            next();
            return;
          }

          const relativePath = decodeURIComponent(request.url.slice("/data/".length).split("?")[0] ?? "");
          const filePath = path.resolve(dataDir, relativePath);
          const root = path.resolve(dataDir);

          if (!filePath.startsWith(`${root}${path.sep}`)) {
            response.statusCode = 403;
            response.end("Forbidden");
            return;
          }

          try {
            const body = await fs.readFile(filePath);
            response.setHeader("Content-Type", "application/json; charset=utf-8");
            response.end(body);
          } catch {
            response.statusCode = 404;
            response.end("Not found");
          }
        });
      }
    }
  ],
  server: {
    host: "127.0.0.1",
    port: 4317
  }
});
