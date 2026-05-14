import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import fs from "node:fs/promises";
import path from "node:path";

const dataDir = process.env.ARCHITEXT_DATA_DIR;

export default defineConfig({
  plugins: [
    react(),
    {
      name: "architext-target-data",
      configureServer(server) {
        if (!dataDir) return;
        server.middlewares.use(async (request, response, next) => {
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
