import assert from "node:assert/strict";
import test from "node:test";
import { nextRuleCategoryName, orderedRules, ruleCategories, ruleCriticalityTone, ruleProtectionLabel } from "../docs/architext/src/presentation/rules.js";
import { postRulesAction } from "../docs/architext/src/presentation/rulesClient.js";

test("rules are ordered by criticality before explicit order", () => {
  const rules = [
    { id: "medium-first", title: "Medium first", criticality: "medium", order: 1, protection: { edit: false, delete: false } },
    { id: "critical-later", title: "Critical later", criticality: "critical", order: 50, protection: { edit: false, delete: false } },
    { id: "critical-earlier", title: "Critical earlier", criticality: "critical", order: 10, protection: { edit: false, delete: false } }
  ];

  assert.deepEqual(orderedRules(rules).map((rule) => rule.id), [
    "critical-earlier",
    "critical-later",
    "medium-first"
  ]);
});

test("rule protection labels distinguish edit and delete protection", () => {
  assert.equal(ruleProtectionLabel({ protection: { edit: true, delete: true } }), "edit/delete protected");
  assert.equal(ruleProtectionLabel({ protection: { edit: true, delete: false } }), "edit protected");
  assert.equal(ruleProtectionLabel({ protection: { edit: false, delete: true } }), "delete protected");
  assert.equal(ruleProtectionLabel({ protection: { edit: false, delete: false } }), "editable");
});

test("rule criticality uses priority-specific tones instead of release health tones", () => {
  assert.deepEqual(
    ["critical", "high", "medium", "low", "unknown"].map(ruleCriticalityTone),
    ["rule-critical", "rule-high", "rule-medium", "rule-low", "rule-neutral"]
  );
});

test("rule categories are derived from user-defined rule data", () => {
  const categories = ruleCategories([
    { title: "Design A", category: "Design", criticality: "medium", order: 30 },
    { title: "Architecture A", category: "Architecture", criticality: "critical", order: 20 },
    { title: "Design B", category: "Design", criticality: "high", order: 10 }
  ]);

  assert.deepEqual(categories.map((category) => [category.id, category.count, category.criticalCount]), [
    ["all", 3, 1],
    ["Architecture", 1, 1],
    ["Design", 2, 0]
  ]);
});

test("new rule categories get a unique display name", () => {
  assert.equal(nextRuleCategoryName([]), "New Category");
  assert.equal(nextRuleCategoryName([{ category: "New Category" }]), "New Category 2");
  assert.equal(nextRuleCategoryName([{ category: "New Category" }, { category: "New Category 2" }]), "New Category 3");
});

test("rules client reports request failures with actionable copy", async () => {
  await assert.rejects(
    postRulesAction(async () => {
      throw new TypeError("Load failed");
    }, { action: "update" }),
    /Confirm architext serve is running/
  );
});

test("rules client reports invalid API responses without raw parser errors", async () => {
  await assert.rejects(
    postRulesAction(async () => ({
      ok: true,
      text: async () => "<html>not json</html>"
    }), { action: "update" }),
    /invalid server response/
  );
});
