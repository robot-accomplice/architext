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
