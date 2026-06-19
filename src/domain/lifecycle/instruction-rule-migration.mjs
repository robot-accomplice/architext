export const instructionRuleFiles = [
  "AGENTS.md",
  "CLAUDE.md",
  ".cursorrules"
];

const architextRulePointerHeading = "## Architext Project Rules";
const architextInstructionHeading = "## Architext Architecture Documentation";
const pointerBody = [
  architextRulePointerHeading,
  "",
  "Project rules are maintained in `docs/architext/data/rules.json`.",
  "Read that file before changing code, update it when project rules change, and run `architext validate [path]` after Architext data edits.",
  "Do not duplicate long-lived project rules across model-specific instruction files."
].join("\n");

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function managedSectionPattern(heading, flags = "") {
  return new RegExp(`\\n?${escapeRegExp(heading)}[\\s\\S]*?(?=\\n## |$)`, flags);
}

function stripMarkdown(value) {
  return value
    .replace(/\*\*(.*?)\*\*/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\[(.*?)\]\([^)]*\)/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}

function titleForRule(summary) {
  const firstSentence = summary.split(/[.;:]/)[0]?.trim() || summary;
  return firstSentence.length <= 54 ? firstSentence : `${firstSentence.slice(0, 51).trim()}...`;
}

function slugForRule(summary, existingIds) {
  const base = stripMarkdown(summary)
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48)
    .replace(/-+$/g, "") || "project-rule";
  let id = base;
  let suffix = 2;
  while (existingIds.has(id)) {
    id = `${base}-${suffix}`;
    suffix += 1;
  }
  existingIds.add(id);
  return id;
}

function withoutArchitextManagedSections(text) {
  return text
    .replace(managedSectionPattern(architextRulePointerHeading, "g"), "\n")
    .replace(managedSectionPattern(architextInstructionHeading, "g"), "\n")
    .trim();
}

function ruleLinesFromText(text) {
  return withoutArchitextManagedSections(text)
    .split(/\r?\n/)
    .map((line) => line.match(/^\s*(?:[-*]|\d+[.)])\s+(.+?)\s*$/)?.[1])
    .filter(Boolean)
    .map(stripMarkdown)
    .filter((line) => line.length >= 16)
    .filter((line) => !/^architext\b/i.test(line))
    .filter((line) => !/^project rules are maintained\b/i.test(line));
}

function isSimpleRuleList(text) {
  const body = withoutArchitextManagedSections(text);
  if (!body) return true;
  const meaningful = body.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  if (!meaningful.length) return true;
  const ruleLike = meaningful.filter((line) => /^([-*]|\d+[.)])\s+/.test(line) || /^#{1,3}\s+/.test(line));
  return ruleLike.length === meaningful.length;
}

function normalizeRuleText(value) {
  return stripMarkdown(value).toLowerCase().replace(/\s+/g, " ");
}

export function plannedInstructionRuleMigration({ files, existingRules }) {
  const existingIds = new Set(existingRules.map((rule) => rule.id));
  const existingSummaries = new Set(existingRules.map((rule) => normalizeRuleText(rule.summary)));
  const nextOrderBase = existingRules.reduce((max, rule) => Math.max(max, Number(rule.order) || 0), 0);
  const candidateRules = [];
  const rewriteFiles = [];
  const ambiguousFiles = [];

  for (const file of files) {
    const ruleLines = [...new Set(ruleLinesFromText(file.text))];
    if (!ruleLines.length) continue;

    const simpleRuleList = isSimpleRuleList(file.text);
    rewriteFiles.push({ path: file.path, replacement: simpleRuleList ? pointerBody : upsertRulePointer(file.text) });
    if (!simpleRuleList) ambiguousFiles.push({ path: file.path, reason: "preserving non-list prose outside the Architext rule pointer" });

    for (const summary of ruleLines) {
      const normalized = normalizeRuleText(summary);
      if (existingSummaries.has(normalized)) continue;
      existingSummaries.add(normalized);
      candidateRules.push({
        id: slugForRule(summary, existingIds),
        title: titleForRule(summary),
        summary,
        category: "Agent Collaboration",
        criticality: "medium",
        order: nextOrderBase + candidateRules.length * 10 + 10,
        source: "maintainer",
        rationale: `Migrated from ${file.path} so model-specific instruction files can point at one model-agnostic rules source.`,
        appliesTo: ["agent instructions", "project rules"],
        protection: { edit: false, delete: false }
      });
    }
  }

  const repairChanges = [
    ...candidateRules.map((rule) => `migrate instruction rule: ${rule.title}`),
    ...rewriteFiles.map((file) => `rewrite ${file.path} to point at docs/architext/data/rules.json`)
  ];

  return { candidateRules, rewriteFiles, ambiguousFiles, repairChanges };
}

export function upsertRulePointer(text) {
  const existing = text.trimEnd();
  if (existing.includes(architextRulePointerHeading)) {
    return existing.replace(managedSectionPattern(architextRulePointerHeading), pointerBody).trimEnd() + "\n";
  }
  return `${existing}${existing ? "\n\n" : ""}${pointerBody}\n`;
}
