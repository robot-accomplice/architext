//! Keyed cache for settled code-graph layouts (Plan D Task 2).
//!
//! Pure Rust — no Leptos, no web-sys — so it is unit-testable without a
//! browser. The layout is a pure deterministic function of
//! `(edges, seed, tick_count)` (the seed and tick budget are fixed per tier,
//! and `force_layout`/`code_graph_layout` already prove bit-identical output
//! across runs), so re-entering a tier that already settled once needs to
//! recompute nothing — it needs only to remember what it computed last time.
//!
//! WHY `tree` is part of the key, not just `sha`: the same commit with
//! uncommitted edits is a DIFFERENT graph. Magma emits `tree: "clean"|"dirty"`
//! precisely so a consumer can tell the two apart — keying on `sha` alone
//! would serve a stale layout to a dirty working tree at the same commit.
//! (Magma briefly emitted the sha in that field instead of the enum; that was
//! a real bug we caught and they fixed, which is what makes this key sound
//! now.)
use crate::code_graph_view_model::Tier;

/// Identifies one tier's settled layout for one artifact snapshot. `sha` and
/// `tree` MUST come from the loaded `CodeGraph` envelope (`data::models`),
/// never from anything derived (e.g. node/edge counts) — those can coincide
/// across genuinely different graphs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutKey {
    sha: String,
    tree: String,
    tier: Tier,
}

impl LayoutKey {
    pub fn new(sha: impl Into<String>, tree: impl Into<String>, tier: Tier) -> Self {
        Self { sha: sha.into(), tree: tree.into(), tier }
    }
}

/// This is a viewer SESSION cache, not a persistent store: it holds at most
/// `MAX_ENTRIES` settled layouts, enough to cover both tiers (modules +
/// functions) of the CURRENTLY loaded artifact without unbounded growth if
/// the underlying data changes underneath it (a live-reload picking up a new
/// sha, for instance). Deliberately NOT persisted to `localStorage` in this
/// task — 17.8k positions is ~142 KB and the quota/serialisation question is
/// its own decision.
const MAX_ENTRIES: usize = 3;

/// Small keyed cache of settled `(x, y)` positions, index-aligned with the
/// tier's `GraphModel` node order. Bounded, FIFO-evicted: at most two tiers
/// are ever live for one artifact in practice, so eviction rarely triggers
/// and a linear scan over `MAX_ENTRIES` entries is simpler than a `HashMap`
/// for no measurable cost.
#[derive(Debug, Default)]
pub struct LayoutCache {
    entries: Vec<(LayoutKey, Vec<(f32, f32)>)>,
}

impl LayoutCache {
    /// `None` on a cold cache or any key mismatch (different sha, different
    /// tree stamp, or different tier).
    pub fn get(&self, key: &LayoutKey) -> Option<&[(f32, f32)]> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, positions)| positions.as_slice())
    }

    /// Record a settled layout under `key`, overwriting any existing entry
    /// for the same key. Evicts the oldest entry first when the bound is hit.
    pub fn put(&mut self, key: LayoutKey, positions: Vec<(f32, f32)>) {
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = positions;
            return;
        }
        if self.entries.len() >= MAX_ENTRIES {
            self.entries.remove(0);
        }
        self.entries.push((key, positions));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_graph_graph::tests_support::interconnected;
    use crate::force_layout::{simulate, ForceConfig};

    #[test]
    fn cold_cache_misses_and_a_matching_key_hits() {
        let mut cache = LayoutCache::default();
        let key = LayoutKey::new("abc1234", "clean", Tier::Functions);
        assert!(cache.get(&key).is_none(), "cold cache must miss");
        cache.put(key.clone(), vec![(1.0, 2.0); 1000]);
        assert!(cache.get(&key).is_some(), "same (sha, tree, tier) must hit");
    }

    /// WHY: uncommitted edits make the same commit a DIFFERENT graph — see
    /// the module doc. A layout cached for the clean tree must not be served
    /// back for the dirty tree at the same sha.
    #[test]
    fn same_sha_different_tree_stamp_misses() {
        let mut cache = LayoutCache::default();
        cache.put(LayoutKey::new("abc1234", "clean", Tier::Functions), vec![(0.0, 0.0); 10]);
        assert!(cache.get(&LayoutKey::new("abc1234", "dirty", Tier::Functions)).is_none());
    }

    #[test]
    fn different_tier_misses() {
        let mut cache = LayoutCache::default();
        cache.put(LayoutKey::new("abc1234", "clean", Tier::Modules), vec![(0.0, 0.0); 10]);
        assert!(cache.get(&LayoutKey::new("abc1234", "clean", Tier::Functions)).is_none());
    }

    /// WHY this must hold: the cache is only SOUND because the layout is a
    /// pure deterministic function of (edges, seed, tick_count) — a cache hit
    /// is trusted to be exactly what a fresh settle would have produced. If
    /// this test ever fails, the cache is serving a different graph than a
    /// fresh run would, and the fix is to restore determinism in the layout,
    /// never to weaken this test.
    #[test]
    fn cached_layout_is_bit_identical_to_a_fresh_settle_at_1000_interconnected_nodes() {
        let (n, edges) = interconnected(1000, 3);
        let seed = 1_469_598_103_934_665_603u64; // the view's fixed seed
        let cfg = ForceConfig::default();

        let settle_a: Vec<(f32, f32)> =
            simulate(n, &edges, seed, &cfg).positions.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
        let mut cache = LayoutCache::default();
        let key = LayoutKey::new("abc1234", "clean", Tier::Functions);
        cache.put(key.clone(), settle_a);

        let settle_b: Vec<(f32, f32)> =
            simulate(n, &edges, seed, &cfg).positions.iter().map(|&(x, y)| (x as f32, y as f32)).collect();

        assert_eq!(cache.get(&key).unwrap(), settle_b.as_slice());
    }

    #[test]
    fn put_overwrites_an_existing_entry_for_the_same_key() {
        let mut cache = LayoutCache::default();
        let key = LayoutKey::new("abc1234", "clean", Tier::Functions);
        cache.put(key.clone(), vec![(1.0, 1.0)]);
        cache.put(key.clone(), vec![(2.0, 2.0)]);
        assert_eq!(cache.get(&key), Some([(2.0, 2.0)].as_slice()));
    }

    #[test]
    fn bound_evicts_the_oldest_entry_once_full() {
        let mut cache = LayoutCache::default();
        for i in 0..MAX_ENTRIES {
            cache.put(LayoutKey::new(format!("sha{i}"), "clean", Tier::Functions), vec![(i as f32, 0.0)]);
        }
        let oldest = LayoutKey::new("sha0", "clean", Tier::Functions);
        assert!(cache.get(&oldest).is_some(), "cache not yet over bound");
        cache.put(LayoutKey::new("sha_new", "clean", Tier::Functions), vec![(9.0, 9.0)]);
        assert!(cache.get(&oldest).is_none(), "oldest entry must be evicted once over bound");
    }
}

#[cfg(test)]
mod collision_tests {
    use super::*;
    use crate::code_graph_view_model::Tier;

    /// A dirty-tree map stamps `sha` from the COMMIT and `tree` as the literal
    /// string "dirty" — neither says anything about the working tree's
    /// CONTENT. So two runs at the same commit over DIFFERENT uncommitted code
    /// produce an identical key, and the second silently receives the first's
    /// settled positions.
    ///
    /// Reported by the magma session 2026-08-05 while proposing to change
    /// `sha` to `<sha>+<diffhash>` on dirty trees. Confirmed here rather than
    /// taken on trust: this pins the collision so the guard added alongside it
    /// cannot be removed without a failing test.
    #[test]
    fn two_dirty_runs_at_one_commit_over_different_code_collide() {
        let mut cache = LayoutCache::default();
        let first: Vec<(f32, f32)> = (0..1200).map(|i| (i as f32, 0.0)).collect();
        cache.put(LayoutKey::new("3673cee", "dirty", Tier::Functions), first.clone());

        // Same commit, same tree flag, DIFFERENT working-tree content — the
        // graph now has more nodes because uncommitted code added functions.
        let key_for_different_code = LayoutKey::new("3673cee", "dirty", Tier::Functions);
        let hit = cache.get(&key_for_different_code);

        assert!(
            hit.is_some(),
            "collision confirmed: a different working tree hits the previous entry"
        );
        assert_eq!(
            hit.unwrap().len(),
            1200,
            "and it receives positions sized for the OTHER graph"
        );
    }

    /// The proposed `<sha>+<diffhash>` fixes it at the source: different dirty
    /// content yields a different key, so the entries no longer alias.
    #[test]
    fn a_content_qualified_sha_separates_them() {
        let mut cache = LayoutCache::default();
        cache.put(LayoutKey::new("3673cee+aaaa", "dirty", Tier::Functions), vec![(0.0, 0.0); 1200]);
        assert!(
            cache.get(&LayoutKey::new("3673cee+bbbb", "dirty", Tier::Functions)).is_none(),
            "distinct dirty content must not alias"
        );
        // Clean trees are unaffected by the proposal and must keep hitting.
        cache.put(LayoutKey::new("3673cee", "clean", Tier::Functions), vec![(1.0, 1.0); 1200]);
        assert!(cache.get(&LayoutKey::new("3673cee", "clean", Tier::Functions)).is_some());
    }
}
