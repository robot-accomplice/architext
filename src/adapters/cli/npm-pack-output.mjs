export function parseNpmPackJson(output) {
  const lines = output.split(/\r?\n/);
  const starts = lines
    .map((line, index) => line.trim() === "[" ? index : -1)
    .filter((index) => index >= 0);
  for (const start of starts.reverse()) {
    try {
      const parsed = JSON.parse(lines.slice(start).join("\n").trim());
      if (Array.isArray(parsed) && parsed[0]?.filename) return parsed;
    } catch {
      // Keep scanning; npm lifecycle scripts may print bracketed non-JSON text.
    }
  }
  throw new Error("Unable to parse npm pack --json output.");
}
