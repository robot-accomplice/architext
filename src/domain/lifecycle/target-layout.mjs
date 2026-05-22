import path from "node:path";

export const metadataFile = ".architext.json";
export const legacyMetadataFile = ".architext-install.json";
export const instructionFiles = ["AGENTS.md", "CLAUDE.md"];
export const generatedIgnores = ["docs/architext/dist/", "docs/architext/.architext-write.lock/"];
export const copiedInstallEntries = [
  "AGENTS_APPENDIX.md",
  "LLM_ARCHITEXT.md",
  "README.md",
  "index.html",
  "dist",
  "node_modules",
  "package-lock.json",
  "package.json",
  "public",
  "schema",
  "src",
  "tools",
  "tsconfig.json",
  "vite.config.ts"
];
export const rootScripts = {
  architext: "architext serve .",
  "architext:build": "architext build .",
  "architext:clean": "architext clean .",
  "architext:doctor": "architext doctor .",
  "architext:prompt": "architext prompt .",
  "architext:validate": "architext validate ."
};

export function architextDir(target) {
  return path.join(target, "docs", "architext");
}

export function dataDir(target) {
  return path.join(architextDir(target), "data");
}

export function metadataPath(target) {
  return path.join(architextDir(target), metadataFile);
}

export function legacyMetadataPath(target) {
  return path.join(architextDir(target), legacyMetadataFile);
}

export function copiedInstallCandidatePaths(target) {
  return copiedInstallEntries.map((entry) => path.join(architextDir(target), entry));
}
