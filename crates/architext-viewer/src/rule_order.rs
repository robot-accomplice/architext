//! Pure model for the Rules surface: order rules by criticality → order →
//! title, and map criticality to its severity-scale CSS token.
//!
//! Faithful port of the JS comparator in
//! `src/domain/architecture-model/rules.mjs` (`orderedRules`) and the tone map
//! in `viewer/src/presentation/rules.js` (`ruleCriticalityTone`). The native
//! port lives in `architext-core` (`domain::rules::ordered_rules`) but operates
//! on `serde_json::Value`; the viewer carries a typed `Rule`, so the SAME
//! ordering is ported here over the typed model (single comparator, no fork).

use crate::data::models::Rule;

/// Criticality → sort rank (unknown → 99). Matches the JS `criticalityRank`.
fn criticality_rank(c: Option<&str>) -> u32 {
    match c {
        Some("critical") => 0,
        Some("high") => 1,
        Some("medium") => 2,
        Some("low") => 3,
        _ => 99,
    }
}

/// Return `rules` sorted by the JS comparator: criticality rank, then numeric
/// `order` (missing → 0, as JS coerces `undefined - n` paths via the default),
/// then `title` (lexicographic). Returns indices into the input slice so the
/// caller keeps ownership of the rules.
pub fn ordered_rule_indices(rules: &[Rule]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..rules.len()).collect();
    idx.sort_by(|&a, &b| {
        let (la, lb) = (&rules[a], &rules[b]);
        let rank = criticality_rank(la.criticality.as_deref())
            .cmp(&criticality_rank(lb.criticality.as_deref()));
        if rank != std::cmp::Ordering::Equal {
            return rank;
        }
        let order = la.order.unwrap_or(0).cmp(&lb.order.unwrap_or(0));
        if order != std::cmp::Ordering::Equal {
            return order;
        }
        la.title.cmp(&lb.title)
    });
    idx
}

/// The severity-scale CSS `var(...)` for a rule's criticality (its OWN ordinal
/// ramp — `--sev-*` — NOT a `--c4-*` role hue). Delegates to the shared
/// [`crate::severity::severity_color_var`] so rule criticality and risk severity
/// resolve through one place.
pub fn criticality_color_var(criticality: Option<&str>) -> &'static str {
    crate::severity::severity_color_var(criticality)
}

/// The protection badge label, or `None` when the rule is freely editable.
/// Port of JS `ruleProtectionLabel` (display only; no editing this slice).
pub fn protection_label(rule: &Rule) -> Option<&'static str> {
    match (rule.protection.edit, rule.protection.delete) {
        (true, true) => Some("edit/delete protected"),
        (true, false) => Some("edit protected"),
        (false, true) => Some("delete protected"),
        (false, false) => None,
    }
}

/// Distinct categories present in `rules`, in criticality-then-order-then-name
/// priority (so the chip row leads with the most critical group), for the
/// "All + per-category" filter. Port of the ordering in JS `ruleCategories`.
pub fn ordered_categories(rules: &[Rule]) -> Vec<String> {
    use std::collections::BTreeMap;
    // Per category track the best (lowest) criticality rank and order seen.
    let mut best: BTreeMap<String, (u32, i64)> = BTreeMap::new();
    for rule in rules {
        let Some(cat) = rule.category.as_deref().map(str::trim).filter(|c| !c.is_empty()) else {
            continue;
        };
        let rank = criticality_rank(rule.criticality.as_deref());
        let order = rule.order.unwrap_or(i64::MAX);
        let entry = best.entry(cat.to_string()).or_insert((u32::MAX, i64::MAX));
        entry.0 = entry.0.min(rank);
        entry.1 = entry.1.min(order);
    }
    let mut cats: Vec<(String, u32, i64)> =
        best.into_iter().map(|(name, (rank, order))| (name, rank, order)).collect();
    cats.sort_by(|a, b| {
        a.1.cmp(&b.1).then(a.2.cmp(&b.2)).then(a.0.cmp(&b.0))
    });
    cats.into_iter().map(|(name, _, _)| name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::models::RuleProtection;

    fn rule(title: &str, criticality: &str, order: i64, category: &str) -> Rule {
        Rule {
            id: title.to_lowercase().replace(' ', "-"),
            title: title.to_string(),
            summary: None,
            category: Some(category.to_string()),
            criticality: Some(criticality.to_string()),
            order: Some(order),
            source: None,
            protection: RuleProtection::default(),
        }
    }

    #[test]
    fn ordered_rule_indices_sort_by_criticality_then_order_then_title() {
        let rules = vec![
            rule("Zebra", "high", 5, "A"),     // 0
            rule("Apple", "critical", 9, "A"), // 1
            rule("Mango", "critical", 2, "A"), // 2
            rule("Alpha", "high", 5, "A"),     // 3 — same crit+order as 0, title tiebreak
        ];
        let ordered = ordered_rule_indices(&rules);
        // critical first, by order: Mango(2, order 2) then Apple(1, order 9).
        // then high, order 5, title tiebreak: Alpha(3) before Zebra(0).
        assert_eq!(ordered, vec![2, 1, 3, 0]);
    }

    #[test]
    fn unknown_criticality_sorts_last() {
        let mut a = rule("A", "critical", 1, "X");
        let mut b = rule("B", "low", 1, "X");
        let mut c = rule("C", "weird", 1, "X");
        a.order = Some(1);
        b.order = Some(1);
        c.order = Some(1);
        let rules = vec![c, b, a];
        let ordered = ordered_rule_indices(&rules);
        // critical(idx2) → low(idx1) → unknown(idx0)
        assert_eq!(ordered, vec![2, 1, 0]);
    }

    #[test]
    fn criticality_color_is_a_sev_token_never_a_c4_role_hue() {
        for c in ["critical", "high", "medium", "low", "mystery"] {
            let v = criticality_color_var(Some(c));
            assert!(v.starts_with("var(--sev-"), "{c} -> {v}");
            assert!(!v.contains("--c4-"), "criticality must not borrow a role hue");
        }
    }

    #[test]
    fn protection_label_reflects_flags() {
        let mut r = rule("R", "high", 1, "X");
        assert_eq!(protection_label(&r), None);
        r.protection = RuleProtection { edit: true, delete: true };
        assert_eq!(protection_label(&r), Some("edit/delete protected"));
        r.protection = RuleProtection { edit: true, delete: false };
        assert_eq!(protection_label(&r), Some("edit protected"));
        r.protection = RuleProtection { edit: false, delete: true };
        assert_eq!(protection_label(&r), Some("delete protected"));
    }
}
