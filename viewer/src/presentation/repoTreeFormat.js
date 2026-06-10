// Pure formatting + file-type labeling helpers for the Repo Tree view.
// Kept free of React/DOM so they are unit-testable in isolation.

// Canonical short labels for common extensions; the badge stays legible at a
// fixed width. Anything unknown falls back to the uppercased extension.
const TYPE_LABELS = {
  ts: "TS", tsx: "TSX", mts: "TS", cts: "TS",
  js: "JS", jsx: "JSX", mjs: "JS", cjs: "JS",
  json: "JSON", jsonc: "JSON",
  md: "MD", mdx: "MDX", txt: "TXT", rst: "RST",
  css: "CSS", scss: "SCSS", sass: "SASS", less: "LESS",
  html: "HTML", htm: "HTML", xml: "XML", svg: "SVG",
  yml: "YML", yaml: "YML", toml: "TOML", ini: "INI", env: "ENV",
  sh: "SH", bash: "SH", zsh: "SH", fish: "SH",
  py: "PY", rb: "RB", go: "GO", rs: "RS", java: "JAVA", kt: "KT",
  c: "C", h: "H", cpp: "CPP", cc: "CPP", hpp: "HPP", cs: "CS",
  php: "PHP", swift: "SWIFT", sql: "SQL", graphql: "GQL", gql: "GQL",
  png: "IMG", jpg: "IMG", jpeg: "IMG", gif: "IMG", webp: "IMG", ico: "IMG",
  lock: "LOCK", log: "LOG", csv: "CSV", pdf: "PDF"
};

// Broad category for an extension — lets the UI keep one subtle visual family
// per kind of file without inventing a color per extension.
const TYPE_CATEGORIES = {
  code: ["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs", "py", "rb", "go", "rs", "java", "kt", "c", "h", "cpp", "cc", "hpp", "cs", "php", "swift", "sh", "bash", "zsh", "fish"],
  data: ["json", "jsonc", "yml", "yaml", "toml", "ini", "env", "xml", "sql", "csv", "graphql", "gql", "lock"],
  doc: ["md", "mdx", "txt", "rst", "pdf", "log"],
  style: ["css", "scss", "sass", "less"],
  markup: ["html", "htm"],
  asset: ["svg", "png", "jpg", "jpeg", "gif", "webp", "ico"]
};

function extensionOf(name) {
  const base = String(name ?? "");
  const dot = base.lastIndexOf(".");
  // A leading dot (".gitignore") is a dotfile, not an extension.
  if (dot <= 0) return "";
  return base.slice(dot + 1).toLowerCase();
}

// Short uppercase type label for a file, or "" when there is no usable
// extension (LICENSE, Dockerfile, .gitignore) — the caller shows a plain
// file glyph in that case.
export function fileTypeLabel(name) {
  const ext = extensionOf(name);
  if (!ext) return "";
  return TYPE_LABELS[ext] ?? ext.toUpperCase().slice(0, 4);
}

export function fileCategory(name) {
  const ext = extensionOf(name);
  if (!ext) return "other";
  for (const [category, exts] of Object.entries(TYPE_CATEGORIES)) {
    if (exts.includes(ext)) return category;
  }
  return "other";
}

// Human-readable byte size. null/undefined size renders as "".
export function formatSize(bytes) {
  if (bytes == null || Number.isNaN(bytes)) return "";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unitIndex]}`;
}

const MINUTE = 60 * 1000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

// Compact relative time ("3d", "2mo", "1y"). `now` is injected for testability.
// null/undefined mtime renders as "".
export function formatRelativeTime(mtime, now = Date.now()) {
  if (mtime == null || Number.isNaN(mtime)) return "";
  const delta = Math.max(0, now - mtime);
  if (delta < MINUTE) return "just now";
  if (delta < HOUR) return `${Math.floor(delta / MINUTE)}m`;
  if (delta < DAY) return `${Math.floor(delta / HOUR)}h`;
  const days = Math.floor(delta / DAY);
  if (days < 30) return `${days}d`;
  if (days < 365) return `${Math.floor(days / 30)}mo`;
  return `${Math.floor(days / 365)}y`;
}
