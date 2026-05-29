export { orderedRules } from "../../../src/domain/architecture-model/rules.mjs";

const criticalityTones = {
  critical: "rule-critical",
  high: "rule-high",
  medium: "rule-medium",
  low: "rule-low"
};

const criticalityRank = {
  critical: 0,
  high: 1,
  medium: 2,
  low: 3
};
const unrankedCriticality = Number.POSITIVE_INFINITY;

export function ruleCriticalityTone(criticality) {
  return criticalityTones[criticality] ?? "rule-neutral";
}

export function ruleCategories(rules = []) {
  const categories = new Map();
  for (const rule of rules) {
    const category = rule.category?.trim();
    if (!category) continue;
    const current = categories.get(category) ?? {
      id: category,
      label: category,
      count: 0,
      criticalCount: 0,
      rank: unrankedCriticality,
      order: Number.POSITIVE_INFINITY
    };
    current.count += 1;
    if (rule.criticality === "critical") current.criticalCount += 1;
    current.rank = Math.min(current.rank, criticalityRank[rule.criticality] ?? unrankedCriticality);
    current.order = Math.min(current.order, rule.order ?? Number.POSITIVE_INFINITY);
    categories.set(category, current);
  }

  return [
    {
      id: "all",
      label: "All Rules",
      count: rules.length,
      criticalCount: rules.filter((rule) => rule.criticality === "critical").length
    },
    ...[...categories.values()].sort((left, right) => {
      const rankDelta = left.rank - right.rank;
      if (rankDelta !== 0) return rankDelta;
      const orderDelta = left.order - right.order;
      if (orderDelta !== 0) return orderDelta;
      return left.label.localeCompare(right.label);
    })
  ];
}

export function nextRuleCategoryName(rules = [], baseName = "New Category") {
  const existing = new Set(rules.map((rule) => rule.category?.trim()).filter(Boolean));
  if (!existing.has(baseName)) return baseName;
  let suffix = 2;
  while (existing.has(`${baseName} ${suffix}`)) {
    suffix += 1;
  }
  return `${baseName} ${suffix}`;
}

export function ruleProtectionLabel(rule) {
  if (rule.protection.edit && rule.protection.delete) return "edit/delete protected";
  if (rule.protection.edit) return "edit protected";
  if (rule.protection.delete) return "delete protected";
  return "editable";
}

const categoryAccents = [
  "var(--cyan)",
  "var(--purple)",
  "var(--c4-module)",
  "var(--pink)",
  "var(--orange)",
  "var(--c4-client)",
  "var(--section-accent)"
];

export function ruleCategoryAccent(category = "") {
  let hash = 0;
  for (const character of category) {
    hash = (hash * 31 + character.charCodeAt(0)) >>> 0;
  }
  return categoryAccents[hash % categoryAccents.length];
}
