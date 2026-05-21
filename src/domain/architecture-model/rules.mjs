const criticalityRank = {
  critical: 0,
  high: 1,
  medium: 2,
  low: 3
};

export function orderedRules(rules = []) {
  return [...rules].sort((left, right) => {
    const criticalityDelta = (criticalityRank[left.criticality] ?? 99) - (criticalityRank[right.criticality] ?? 99);
    if (criticalityDelta !== 0) return criticalityDelta;
    const orderDelta = left.order - right.order;
    if (orderDelta !== 0) return orderDelta;
    return left.title.localeCompare(right.title);
  });
}

function protectedFromReorder(rule) {
  return rule.protection.edit || rule.protection.delete;
}

export function upsertRule(rulesDocument, rule) {
  const existing = rulesDocument.rules.find((candidate) => candidate.id === rule.id);
  if (existing?.protection.edit) throw new Error(`Rule "${rule.id}" is edit protected`);
  const nextRule = existing ? { ...existing, ...rule, protection: rule.protection ?? existing.protection } : rule;
  const exists = Boolean(existing);
  return {
    ...rulesDocument,
    rules: exists
      ? rulesDocument.rules.map((candidate) => (candidate.id === rule.id ? nextRule : candidate))
      : [...rulesDocument.rules, nextRule]
  };
}

export function deleteRule(rulesDocument, id) {
  const existing = rulesDocument.rules.find((candidate) => candidate.id === id);
  if (!existing) throw new Error(`Rule "${id}" was not found`);
  if (existing.protection.delete) throw new Error(`Rule "${id}" is delete protected`);
  return {
    ...rulesDocument,
    rules: rulesDocument.rules.filter((candidate) => candidate.id !== id)
  };
}

export function moveRule(rulesDocument, id, direction) {
  const existing = rulesDocument.rules.find((candidate) => candidate.id === id);
  if (!existing) throw new Error(`Rule "${id}" was not found`);
  if (protectedFromReorder(existing)) throw new Error(`Rule "${id}" is protected from reordering`);
  const peers = orderedRules(rulesDocument.rules)
    .filter((candidate) => candidate.criticality === existing.criticality && !protectedFromReorder(candidate));
  const index = peers.findIndex((candidate) => candidate.id === id);
  const targetIndex = direction === "up" ? index - 1 : index + 1;
  const target = peers[targetIndex];
  if (!target) return rulesDocument;
  return {
    ...rulesDocument,
    rules: rulesDocument.rules.map((candidate) => {
      if (candidate.id === existing.id) return { ...candidate, order: target.order };
      if (candidate.id === target.id) return { ...candidate, order: existing.order };
      return candidate;
    })
  };
}

export function moveRuleBefore(rulesDocument, id, beforeId) {
  if (id === beforeId) return rulesDocument;
  const existing = rulesDocument.rules.find((candidate) => candidate.id === id);
  const target = rulesDocument.rules.find((candidate) => candidate.id === beforeId);
  if (!existing) throw new Error(`Rule "${id}" was not found`);
  if (!target) throw new Error(`Rule "${beforeId}" was not found`);
  if (protectedFromReorder(existing)) throw new Error(`Rule "${id}" is protected from reordering`);
  if (protectedFromReorder(target)) throw new Error(`Rule "${beforeId}" is protected from reordering`);
  if (existing.criticality !== target.criticality) throw new Error("Rules can only be reordered within the same criticality group");

  const peers = orderedRules(rulesDocument.rules)
    .filter((candidate) => candidate.criticality === existing.criticality && !protectedFromReorder(candidate));
  const orderSlots = peers.map((candidate) => candidate.order).sort((left, right) => left - right);
  const reordered = peers.filter((candidate) => candidate.id !== id);
  const targetIndex = reordered.findIndex((candidate) => candidate.id === beforeId);
  reordered.splice(targetIndex, 0, existing);
  const orderById = new Map(reordered.map((candidate, index) => [candidate.id, orderSlots[index]]));

  return {
    ...rulesDocument,
    rules: rulesDocument.rules.map((candidate) => (
      orderById.has(candidate.id) ? { ...candidate, order: orderById.get(candidate.id) } : candidate
    ))
  };
}
