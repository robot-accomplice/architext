import { readFileSync } from "node:fs";

const docs = ["README.md", "docs/architext/README.md"];
const scopedPackage = "@robotaccomplice/architext";

const forbiddenPatterns = [
  {
    label: "unscoped npm install",
    pattern: /\bnpm\s+(?:install|i)\b[^\n#]*?(?<![@/\w-])architext(?![/\w-])/g,
  },
  {
    label: "unscoped npx invocation",
    pattern: /\bnpx\s+architext\b/g,
  },
];

const failures = [];

for (const docPath of docs) {
  const text = readFileSync(docPath, "utf8");

  if (!text.includes(scopedPackage)) {
    failures.push(`${docPath}: missing scoped package ${scopedPackage}`);
  }

  for (const { label, pattern } of forbiddenPatterns) {
    for (const match of text.matchAll(pattern)) {
      const line = text.slice(0, match.index).split("\n").length;
      failures.push(`${docPath}:${line}: ${label}: ${match[0].trim()}`);
    }
  }
}

if (failures.length > 0) {
  console.error("Install documentation check failed:");
  for (const failure of failures) {
    console.error(`- ${failure}`);
  }
  process.exit(1);
}

console.log("Install documentation uses the scoped npm package.");
