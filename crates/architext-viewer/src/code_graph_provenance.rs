//! "How was this map produced?" — the code-graph provenance surface (Item 1).
//!
//! Split out of `code_graph_graph.rs` to keep that module scoped to the
//! graph/badges core. This file owns the `kind: "init"` weaker-evidence
//! tooltip wording and the fidelity → plain-language explanations. See
//! `code_graph_paths.rs` for the separate not-guaranteed-repo-relative
//! `file` handling (Item 2).
//!
//! Design is settled by the maintainer, not a judgment call made here:
//! - NO standalone fidelity display — the raw token (`rta`/`semantic`) must
//!   never appear in the UI, and `hir` must never be surfaced at all.
//! - Fidelity MODULATES the explanation of an edge kind the viewer already
//!   draws (dynamic vs. static calls), rather than getting its own label.
//! - A separate "how this map was made" affordance carries the facts that
//!   are not per-edge: generator + version, the analysis method in plain
//!   words, and — only when `executed_target_code` says so — that producing
//!   the map executed the analysed repository's own code.

use crate::code_graph_graph::Reach;

impl Reach {
    /// Hover text for this badge on a function of the given magma `kind`
    /// (`func` | `method` | `init`).
    ///
    /// A `kind: "init"` node is a Rust const/static initialiser, not a
    /// function — there is no `fn` in the source. Magma models CALLS, not
    /// USES: a `static` is used, never called, so no edge ever points at its
    /// initialiser. It therefore fails `reachable` (landing in `Dead`) or
    /// only shows up via test wiring (`TestOnly`) on structurally WEAKER
    /// evidence than a function in the same badge — a function's Dead badge
    /// means "no static caller found"; an init's Dead badge means "this
    /// analysis method cannot observe a caller for this shape of node at
    /// all". Reusing the generic reflection/cgo wording here would overstate
    /// the confidence, so `init` gets its own text instead. See the doc
    /// comment on `code_graph_accepts_magmas_forthcoming_fields` in
    /// `architext-core` for the full rationale.
    pub fn tooltip_for_kind(self, kind: &str) -> &'static str {
        if kind != "init" {
            return self.tooltip();
        }
        match self {
            Reach::Dead => "CANDIDATE — WEAKER EVIDENCE: a const/static initialiser, not a \
                function. Magma models calls, not uses, so an initialiser can never satisfy \
                reachability through any caller, used or not — this badge does not mean \
                'no caller was found', it means 'callers cannot be observed for this kind of \
                node'. Verify independently before treating this as dead.",
            Reach::TestOnly => "CANDIDATE — WEAKER EVIDENCE: a const/static initialiser reached \
                only from test wiring. Magma models calls, not uses, so its reachability is \
                inherently weaker evidence than a function's test-only badge — verify \
                independently.",
            _ => self.tooltip(),
        }
    }
}

/// Plain-language description of the analysis METHOD behind a fidelity
/// token, for the "how this map was made" affordance. NEVER echoes the raw
/// token (`rta`/`semantic`) — the maintainer's words: "Surfacing it as text
/// without providing the user with some means of understanding the value is
/// worthless." An unrecognised token gets a conservative fallback rather
/// than being printed verbatim, so a future fidelity value doesn't leak a
/// raw token into the UI before this viewer knows how to explain it.
pub fn fidelity_method_description(fidelity: &str) -> &'static str {
    match fidelity {
        "rta" => "Static call-graph analysis (Rapid Type Analysis): every call site is resolved \
            by reading the source, without running it.",
        "semantic" => "Semantic analysis using the compiler's own type inference to resolve \
            call sites to concrete targets.",
        _ => "Analysis method not recognised by this viewer version.",
    }
}

/// Plain-language explanation of what a DYNAMIC call edge means under this
/// fidelity. This is the core of the design: fidelity does not get its own
/// standalone label, it MODULATES the explanation of an edge kind the viewer
/// already draws (`calls[].kind`). Static edges are exact under both
/// methods; only the dynamic explanation changes:
///
///   - `rta`: the dynamic edge is this method's over-approximation — it may
///     not occur at runtime.
///   - `semantic`: the dynamic edge was resolved to a concrete target by
///     type inference — not an over-approximation.
///
/// Mirrors the reachability badges: a candidate with its blind spot named,
/// never a verdict. Never echoes the raw fidelity token (same rule as
/// [`fidelity_method_description`]); an unrecognised fidelity gets a
/// conservative fallback that asks the reader to treat the edge as
/// unverified either way.
pub fn dynamic_edge_explanation(fidelity: &str) -> &'static str {
    match fidelity {
        "rta" => "Dynamic calls are this analysis method's over-approximation: this edge may \
            not occur at runtime.",
        "semantic" => "Dynamic calls were resolved to a concrete target by type inference — \
            not an over-approximation.",
        _ => "Dynamic-call resolution method not recognised by this viewer — treat dynamic \
            edges as unverified.",
    }
}

/// Whether the "producing this map executed the analysed repository's code"
/// trust-boundary disclosure should render. Driven by the FIELD
/// (`executed_target_code`), never by a `language == "rust"` rule — magma's
/// own rationale for adding the field is that equivalence breaks once their
/// deferred sandboxed mode lands. `None` (older artifacts, or a producer
/// that hasn't shipped the field yet) is treated as "don't know", not as
/// "didn't execute" — it stays silent rather than asserting either way.
pub fn discloses_executed_target_code(executed_target_code: Option<bool>) -> bool {
    executed_target_code == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fidelity_method_description_never_echoes_the_raw_token() {
        // WHY: "the raw token (rta, semantic) must never appear in the UI" —
        // maintainer mandate. Guard both known tokens and the unknown fallback.
        for fidelity in ["rta", "semantic", "", "hir", "some-future-token"] {
            let d = fidelity_method_description(fidelity);
            assert!(!d.is_empty());
            assert!(
                !d.to_lowercase().contains("rta") || fidelity == "rta",
                "must not leak the raw token for {fidelity:?}: {d}"
            );
            assert_ne!(d, fidelity, "must be a description, not the token itself");
        }
        assert!(fidelity_method_description("rta").to_lowercase().contains("static"));
        assert!(fidelity_method_description("semantic").to_lowercase().contains("type inference"));
        // Unknown token: conservative fallback, no leaked token text.
        let unknown = fidelity_method_description("some-future-token");
        assert!(!unknown.contains("some-future-token"));
    }

    #[test]
    fn dynamic_edge_explanation_differs_by_fidelity_and_never_echoes_the_token() {
        // WHY: this is the core surfacing mechanism — fidelity modulates the
        // explanation of an edge kind already drawn, rather than a standalone
        // label. rta's dynamic edges are an over-approximation; semantic's are
        // not. The two must read differently, and neither may print "rta" or
        // "semantic" as a bare token.
        let rta = dynamic_edge_explanation("rta");
        let semantic = dynamic_edge_explanation("semantic");
        assert_ne!(rta, semantic, "the two fidelities must read differently");
        assert!(rta.to_lowercase().contains("over-approximation") || rta.to_lowercase().contains("may not occur"));
        assert!(!semantic.to_lowercase().contains("may not occur"), "semantic edges are not an over-approximation");
        assert!(!rta.contains("\"rta\"") && !semantic.contains("\"semantic\""));

        // Unknown fidelity: a sensible, conservative fallback — never panics,
        // never echoes the unrecognised token.
        let unknown = dynamic_edge_explanation("quantum-fidelity");
        assert!(!unknown.is_empty());
        assert!(!unknown.contains("quantum-fidelity"));
    }

    #[test]
    fn executed_target_code_disclosure_is_driven_by_the_field_only() {
        // WHY: the trust-boundary disclosure must key off the FIELD, never a
        // `language == "rust"` rule — magma added the field precisely because
        // that equivalence breaks once their deferred sandboxed mode lands.
        assert!(discloses_executed_target_code(Some(true)));
        assert!(!discloses_executed_target_code(Some(false)));
        // Unknown (absent field / older artifact) is "don't know", not "no".
        assert!(!discloses_executed_target_code(None));
    }

    #[test]
    fn init_kind_gets_weaker_evidence_wording_dead_and_test_only() {
        // WHY: a `kind: "init"` node is a const/static initialiser, never
        // called (magma models calls, not uses) — its Dead/TestOnly badge is
        // structurally weaker evidence than a function's and must say so
        // rather than reuse the generic reflection/cgo wording.
        let dead_init = Reach::Dead.tooltip_for_kind("init");
        assert_ne!(dead_init, Reach::Dead.tooltip(), "init must not reuse the generic wording");
        assert!(dead_init.to_lowercase().contains("initialiser"));
        assert!(dead_init.to_lowercase().contains("candidate"));

        let test_only_init = Reach::TestOnly.tooltip_for_kind("init");
        assert_ne!(test_only_init, Reach::TestOnly.tooltip());
        assert!(test_only_init.to_lowercase().contains("initialiser"));

        // A ordinary function/method kind is unaffected — verbatim passthrough.
        for kind in ["func", "method"] {
            assert_eq!(Reach::Dead.tooltip_for_kind(kind), Reach::Dead.tooltip());
            assert_eq!(Reach::TestOnly.tooltip_for_kind(kind), Reach::TestOnly.tooltip());
        }
        // Root/Generated badges are unaffected by kind (init can be root/generated too).
        assert_eq!(Reach::Root.tooltip_for_kind("init"), Reach::Root.tooltip());
        assert_eq!(Reach::Generated.tooltip_for_kind("init"), Reach::Generated.tooltip());
    }
}

use crate::data::models::{CodeGraph, CodeGraphLimitation};

/// Effect value meaning "this analysis reports more code as live than really
/// is". Unpinned in the schema (producer vocabulary), so this constant is the
/// one place we recognise it; an unrecognised effect degrades to "disclosed but
/// unweighted" rather than being guessed at.
const EFFECT_OVER_APPROXIMATES_LIVE: &str = "over-approximates-live";

/// Does the producer disclose that it over-reports live code?
///
/// This is the question our reachability badges depend on. `dead` is computed
/// as `!reachable && !generated && !root`, so a backend that over-roots drives
/// BOTH the Dead and Test-only sets toward empty — and an empty result renders
/// as "this codebase has no dead code", which is a false reassurance rather
/// than a neutral absence. Magma measured 87% of nodes rooted on a derive-heavy
/// Rust crate with ZERO dead functions reported out of 156; on its own graph
/// the same ratio is 2.8%. When the producer says it over-approximates, the
/// viewer must say the counts are SUPPRESSED, not clean.
pub fn over_approximates_live(limitations: &[CodeGraphLimitation]) -> Option<&CodeGraphLimitation> {
    limitations.iter().find(|l| l.effect == EFFECT_OVER_APPROXIMATES_LIVE)
}

/// Sentence to attach to an empty/negative reachability badge set when the
/// producer has disclosed over-approximation. Returns `None` when nothing is
/// disclosed, so an honest sweep gains no caveat it did not earn.
pub fn reachability_caveat(cg: &CodeGraph) -> Option<String> {
    let lim = over_approximates_live(&cg.limitations)?;
    let evidence = lim
        .evidenced_by
        .as_deref()
        .and_then(|key| match key {
            // Only `root_ratio` is quantitatively meaningful to a reader here;
            // the rest are raw counts whose significance needs the ratio anyway.
            "root_ratio" => cg.disclosure.as_ref()?.root_ratio,
            _ => None,
        })
        .map(|r| format!(" ({:.0}% of nodes are roots)", r * 100.0))
        .unwrap_or_default();
    Some(format!(
        "Reachability is SUPPRESSED by a disclosed limitation of {}{}: {}. \
         An empty dead-code result here is absence of evidence, not evidence of absence.",
        lim.attribution, evidence, lim.description
    ))
}

/// Plain-language reading of a limitation's `scope` — the question a reader
/// actually has, which is whether waiting for a newer version helps.
pub fn scope_meaning(scope: &str) -> &'static str {
    match scope {
        "language" => "inherent to the language; this will not change",
        "analyzer" => "a limit of the pinned analyser; moves when the pin moves",
        "backend" => "not implemented yet in this backend; fixable",
        _ => "this build cannot tell whether a newer version would lift this",
    }
}

#[cfg(test)]
mod limitation_tests {
    use super::*;
    use crate::data::models::{CodeGraph, CodeGraphDisclosure, CodeGraphLimitation};

    fn cg(limitations: Vec<CodeGraphLimitation>, root_ratio: Option<f64>) -> CodeGraph {
        CodeGraph {
            contract_version: "magma-code-graph/1".into(),
            generator: "magma/0.3.0".into(),
            language: "rust".into(),
            module: "m".into(),
            sha: "abc".into(),
            tree: "clean".into(),
            fidelity: "semantic".into(),
            computable: true,
            executed_target_code: Some(true),
            not_computable_reason: None,
            functions: None,
            calls: None,
            modules: None,
            module_calls: None,
            limitations,
            disclosure: root_ratio.map(|r| CodeGraphDisclosure {
                nodes: Some(156),
                roots: Some(136),
                generated: Some(0),
                dynamic_edges: Some(0),
                root_ratio: Some(r),
            }),
        }
    }

    fn over_rooting() -> CodeGraphLimitation {
        CodeGraphLimitation {
            id: "rust-derive-over-rooting".into(),
            scope: "backend".into(),
            attribution: "magma rust backend".into(),
            description: "derive-generated methods report as Public, so derive-heavy crates over-root".into(),
            effect: "over-approximates-live".into(),
            evidenced_by: Some("root_ratio".into()),
        }
    }

    #[test]
    fn an_empty_dead_set_is_caveated_when_the_producer_admits_over_rooting() {
        // WHY: this is the whole point of consuming `limitations`. Magma
        // measured 136 of 156 nodes rooted with ZERO dead functions found. The
        // viewer must not render that as a clean bill of health.
        let doc = cg(vec![over_rooting()], Some(0.872));
        let caveat = reachability_caveat(&doc).expect("over-approximation must produce a caveat");
        assert!(caveat.contains("SUPPRESSED"), "{caveat}");
        assert!(caveat.contains("87%"), "the ratio must be quantified: {caveat}");
        assert!(
            caveat.contains("absence of evidence"),
            "must distinguish absence of evidence from evidence of absence: {caveat}"
        );
    }

    #[test]
    fn an_honest_artifact_gains_no_caveat_it_did_not_earn() {
        // A producer that discloses nothing, or discloses a limitation erring
        // the OTHER way, must not have its reachability undermined.
        assert!(reachability_caveat(&cg(vec![], None)).is_none());
        let omits = CodeGraphLimitation { effect: "may-omit-edges".into(), ..over_rooting() };
        assert!(reachability_caveat(&cg(vec![omits], Some(0.028))).is_none());
    }

    #[test]
    fn the_caveat_survives_a_missing_or_dangling_evidence_pointer() {
        // `evidenced_by` is optional and only `root_ratio` is meaningful here.
        // Losing the number must not lose the WARNING — degrade the precision,
        // never the disclosure.
        let no_ev = CodeGraphLimitation { evidenced_by: None, ..over_rooting() };
        let c = reachability_caveat(&cg(vec![no_ev], Some(0.872))).expect("caveat still required");
        assert!(c.contains("SUPPRESSED") && !c.contains('%'), "{c}");
    }

    #[test]
    fn scope_answers_whether_waiting_helps_including_for_unknown_scopes() {
        assert!(scope_meaning("language").contains("will not change"));
        assert!(scope_meaning("analyzer").contains("pin"));
        assert!(scope_meaning("backend").contains("fixable"));
        // The forward-compat case: a scope this build predates.
        assert!(scope_meaning("sandbox-mode").contains("cannot tell"));
    }
}
