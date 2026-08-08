//! Slop Ferret sweep viewer: coverage banner + findings list.

use leptos::*;

use crate::data::models::{SlopFerret, SlopFerretCandidate, SlopFerretFinding};
use crate::components::enrichment_empty_state::{Enrichment, EnrichmentEmptyState};
use crate::state::use_app_state;

const SEV_ORDER: &[&str] = &["blocking", "fix-or-file", "note"];

#[component]
pub fn SlopFerretPanel() -> impl IntoView {
    let state = use_app_state();

    let snapshot = move || {
        state
            .data
            .get()
            .slop_ferret
            .as_ref()
            .and_then(|res| res.as_ref().ok().cloned())
    };

    let error = move || {
        state
            .data
            .get()
            .slop_ferret
            .as_ref()
            .and_then(|res| res.as_ref().err().map(|e| e.to_string()))
    };

    view! {
        <div class="slop-ferret-panel">
            {move || {
                if let Some(err) = error() {
                    return view! {
                        <div class="slop-ferret-panel__empty">
                            <span class="overline">"SLOP FERRET"</span>
                            <p>{err}</p>
                        </div>
                    }.into_view();
                }
                match snapshot() {
                    Some(doc) => render_snapshot(&doc),
                    None => view! {
                        <EnrichmentEmptyState kind=Enrichment::SlopDetection/>
                    }.into_view(),
                }
            }}
        </div>
    }
}

/// Severity first, then VERIFIED before SUSPECTED, then title.
///
/// A free function rather than an inline closure so the ordering can be tested
/// against the code that actually runs. `severity` and `status` are NOT
/// schema-constrained (an unknown value must degrade, never reject the sweep —
/// see `slop-ferret.schema.json`), so an unrecognised severity sorts LAST
/// rather than being treated as `note`: a severity this build has never heard
/// of might outrank everything, and quietly filing it among the cosmetic ones
/// would be the worst guess available.
pub(crate) fn sort_findings(findings: &mut [SlopFerretFinding]) {
    findings.sort_by(|a, b| {
        let rank = |s: &str| {
            SEV_ORDER
                .iter()
                .position(|&r| r == s.to_lowercase().as_str())
                .unwrap_or(SEV_ORDER.len())
        };
        let ra = rank(&a.severity);
        let rb = rank(&b.severity);
        if ra != rb {
            return ra.cmp(&rb);
        }
        let verified_a = a.status.eq_ignore_ascii_case("VERIFIED");
        let verified_b = b.status.eq_ignore_ascii_case("VERIFIED");
        if verified_a != verified_b {
            return if verified_a { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
        }
        a.title.cmp(&b.title)
    });
}

/// A CSS modifier for a value this build may not recognise. Unknown values get
/// `--unknown` rather than an unstyled class name built from arbitrary producer
/// text, which would silently render as an unstyled chip.
fn modifier(known: &[&str], value: &str) -> String {
    let lower = value.to_lowercase();
    if known.contains(&lower.as_str()) { lower } else { "unknown".to_string() }
}

fn render_snapshot(doc: &SlopFerret) -> View {
    let mut findings = doc.findings.clone();
    sort_findings(&mut findings);

    view! {
        <div class="slop-ferret-panel__content">
            <div class="slop-ferret-panel__header">
                <span class="overline">"SLOP FERRET"</span>
                <h2>"Sweep findings"</h2>
                // WHICH commit, and WHEN. Without these a months-old sweep
                // renders identically to one taken this morning, and a reader
                // has no way to tell that the findings below describe code that
                // no longer exists. This is the single most important thing the
                // panel can say about a QA artifact.
                <p class="slop-ferret-panel__provenance">
                    {let sha = doc.sha.clone();
                     let short = if sha.len() > 12 { sha[..12].to_string() } else { sha };
                     format!("{} · swept {}", short, doc.date)}
                    {(!doc.tier.is_empty()).then(|| format!(" · tier {}", doc.tier))}
                </p>
            </div>
            <div class="slop-ferret-panel__banner">
                <div class="slop-ferret-panel__metric">
                    <span class="slop-ferret-panel__value">{doc.attested_repo.clone()}</span>
                    <span class="slop-ferret-panel__label">"repo read"</span>
                </div>
                <div class="slop-ferret-panel__metric">
                    <span class="slop-ferret-panel__value">{doc.attested_plan.clone()}</span>
                    <span class="slop-ferret-panel__label">"plan dispositioned"</span>
                </div>
                <div class="slop-ferret-panel__metric">
                    <span class="slop-ferret-panel__value">{doc.denominator.to_string()}</span>
                    <span class="slop-ferret-panel__label">"files"</span>
                </div>
                <div class="slop-ferret-panel__metric">
                    <span class={format!("slop-ferret-panel__badge slop-ferret-panel__badge--{}", doc.accounting.clone())}>
                        {doc.accounting.clone()}
                    </span>
                    <span class="slop-ferret-panel__label">"accounting"</span>
                </div>
            </div>
            {coverage_section(doc)}
            {candidates_section(&doc.candidates)}
            <div class="slop-ferret-panel__findings">
                {findings.into_iter().map(finding_card).collect_view()}
            </div>
        </div>
    }
    .into_view()
}

/// What the sweep did NOT cover.
///
/// A findings list on its own reads as the whole story, and an empty one reads
/// as "clean". Neither is safe: a sweep that skipped four check families, or
/// waived half the tree, found nothing in the part it never looked at. Since
/// this tool's own doctrine is that degradation must be loud, the gaps belong
/// next to the findings, not in a file nobody opens. Renders nothing when there
/// is genuinely nothing to disclose.
fn coverage_section(doc: &SlopFerret) -> Option<View> {
    let mut rows: Vec<(String, String)> = Vec::new();
    if !doc.families_not_run.is_empty() {
        rows.push(("not run".into(), doc.families_not_run.join(", ")));
    }
    if doc.waived > 0 {
        rows.push(("waived".into(), doc.waived.to_string()));
    }
    if !doc.near_misses.is_empty() {
        rows.push(("near misses".into(), doc.near_misses.join(", ")));
    }
    if !doc.checked_clean.is_empty() {
        // The substance behind the denominator: what was actually looked at and
        // found clean. "2 files" means little; "checked X by method Y" is the
        // claim a reader can weigh.
        let checked = doc
            .checked_clean
            .iter()
            .map(|c| format!("{} ({})", c.class, c.method))
            .collect::<Vec<_>>()
            .join(", ");
        rows.push(("checked clean".into(), checked));
    }
    if doc.unmatched_size > 0 {
        rows.push(("unmatched hypotheses".into(), doc.unmatched_size.to_string()));
    }
    if rows.is_empty() {
        return None;
    }
    Some(
        view! {
            <div class="slop-ferret-panel__coverage">
                <span class="overline">"COVERAGE"</span>
                {rows.into_iter().map(|(label, value)| view! {
                    <div class="slop-ferret-panel__coverage-row">
                        <span class="slop-ferret-panel__label">{label}</span>
                        <span class="slop-ferret-panel__coverage-value">{value}</span>
                    </div>
                }).collect_view()}
            </div>
        }
        .into_view(),
    )
}

/// Undispositioned candidates from the plan.
///
/// `ferret plan` locates and classifies these mechanically — no model involved
/// — so they are available the moment a plan exists, long before anyone has
/// swept anything. Rendering only the COUNT (which is all the bundle carried
/// before) threw away the entire actionable payload: file, line, symbol, class,
/// and the bar that settles it.
///
/// Presented as OPEN QUESTIONS, never as findings. The skill's own operating
/// rule is that a false accusation costs more than a missed one, so nothing
/// here may read as confirmed slop — each row states what would have to be
/// proven, which is also the reader's next action.
fn candidates_section(candidates: &[SlopFerretCandidate]) -> Option<View> {
    if candidates.is_empty() {
        return None;
    }
    let n = candidates.len();
    let rows = candidates.to_vec();
    Some(view! {
        <div class="slop-ferret-panel__candidates">
            <span class="overline">"OPEN CANDIDATES"</span>
            <p class="slop-ferret-panel__candidates-note">
                {format!("{n} raised by the plan and not yet dispositioned. \
                          These are leads, not findings \u{2014} nothing has verified them.")}
            </p>
            {rows.into_iter().map(candidate_row).collect_view()}
        </div>
    }.into_view())
}

fn candidate_row(c: SlopFerretCandidate) -> View {
    view! {
        <div class="slop-ferret-panel__candidate">
            <div class="slop-ferret-panel__candidate-head">
                <span class="slop-ferret-panel__class">{c.class.clone()}</span>
                <code class="slop-ferret-panel__symbol">{c.symbol.clone()}</code>
                {(!c.family.is_empty()).then(|| view! {
                    <span class="slop-ferret-panel__family">{format!("family {}", c.family)}</span>
                })}
            </div>
            <div class="slop-ferret-panel__meta">
                {if c.line > 0 { format!("{}:{}", c.file, c.line) } else { c.file.clone() }}
            </div>
            {(!c.bar.is_empty()).then(|| view! {
                <div class="slop-ferret-panel__section">
                    <span class="overline">"BAR TO SETTLE"</span>
                    <p>{c.bar.clone()}</p>
                </div>
            })}
        </div>
    }.into_view()
}

fn finding_card(f: SlopFerretFinding) -> View {
    let sev_class = format!(
        "slop-ferret-panel__severity slop-ferret-panel__severity--{}",
        modifier(SEV_ORDER, &f.severity)
    );
    let status_class = format!(
        "slop-ferret-panel__status slop-ferret-panel__status--{}",
        modifier(&["verified", "suspected"], &f.status)
    );
    view! {
        <div class="slop-ferret-panel__finding">
            <div class="slop-ferret-panel__finding-header">
                <span class=sev_class>{f.severity.clone()}</span>
                <span class=status_class>{f.status.clone()}</span>
                <span class="slop-ferret-panel__class">{f.class.clone()}</span>
            </div>
            <h3 class="slop-ferret-panel__finding-title">{f.title}</h3>
            // `occurrences` changes what the finding IS — one slip versus a
            // pattern repeated across the tree — so it belongs next to the
            // location, not dropped.
            <div class="slop-ferret-panel__meta">
                {f.file.clone()}
                {f.occurrences.filter(|n| *n > 1).map(|n| format!(" · {n} occurrences"))}
            </div>
            {(!f.claim.is_empty()).then(|| view! {
                <p class="slop-ferret-panel__claim">{f.claim.clone()}</p>
            })}
            {(!f.evidence.is_empty()).then(|| view! {
                <div class="slop-ferret-panel__section">
                    <span class="overline">"EVIDENCE"</span>
                    <p>{f.evidence.clone()}</p>
                </div>
            })}
            // For a SUSPECTED finding these two ARE the judgement. `bar` is what
            // the finding would have to clear to be confirmed, and `refutation`
            // is the attempt already made to knock it down. Rendering a
            // SUSPECTED badge while withholding both leaves the reader with a
            // scary label and no way to weigh it — which is how a suspected
            // finding becomes either ignored or over-trusted.
            {(!f.bar.is_empty()).then(|| view! {
                <div class="slop-ferret-panel__section">
                    <span class="overline">"BAR TO CONFIRM"</span>
                    <p>{f.bar.clone()}</p>
                </div>
            })}
            {(!f.refutation.is_empty()).then(|| view! {
                <div class="slop-ferret-panel__section">
                    <span class="overline">"REFUTATION ATTEMPT"</span>
                    <p>{f.refutation.clone()}</p>
                </div>
            })}
            {(!f.remediation.is_empty()).then(|| view! {
                <div class="slop-ferret-panel__section">
                    <span class="overline">"REMEDIATION"</span>
                    <p>{f.remediation.clone()}</p>
                </div>
            })}
        </div>
    }
    .into_view()
}

#[cfg(test)]
mod tests {
    use crate::data::models::SlopFerretFinding;

    fn sample_finding(title: &str, severity: &str, status: &str) -> SlopFerretFinding {
        SlopFerretFinding {
            title: title.to_string(),
            file: "main.go".to_string(),
            class: "H · latent defect".to_string(),
            severity: severity.to_string(),
            status: status.to_string(),
            claim: "claim".to_string(),
            refutation: String::new(),
            bar: String::new(),
            evidence: String::new(),
            remediation: String::new(),
            occurrences: None,
        }
    }

    #[test]
    fn severity_sorts_blocking_first() {
        // Calls the PRODUCTION sort. The previous version of this test
        // reimplemented the ranking inline and asserted its own copy worked,
        // so deleting the real sort left it green — it could not fail when the
        // behaviour it names changed.
        let mut findings = vec![
            sample_finding("Cosmetic", "note", "VERIFIED"),
            sample_finding("Auth bug", "blocking", "VERIFIED"),
            sample_finding("Dupe", "fix-or-file", "VERIFIED"),
        ];
        super::sort_findings(&mut findings);
        assert_eq!(findings[0].title, "Auth bug");
        assert_eq!(findings[1].title, "Dupe");
        assert_eq!(findings[2].title, "Cosmetic");
    }

    #[test]
    fn verified_precedes_suspected_at_equal_severity() {
        // WHY: within one severity band a confirmed defect is actionable and a
        // suspected one still needs judgement, so the reader should meet the
        // confirmed ones first. This tier was entirely untested before.
        let mut findings = vec![
            sample_finding("A suspected", "blocking", "SUSPECTED"),
            sample_finding("Z verified", "blocking", "VERIFIED"),
        ];
        super::sort_findings(&mut findings);
        assert_eq!(
            findings[0].title, "Z verified",
            "VERIFIED must outrank SUSPECTED even when the title sorts later"
        );
    }

    #[test]
    fn title_breaks_ties_so_the_order_is_stable_across_renders() {
        let mut findings = vec![
            sample_finding("Beta", "note", "VERIFIED"),
            sample_finding("Alpha", "note", "VERIFIED"),
        ];
        super::sort_findings(&mut findings);
        assert_eq!(findings[0].title, "Alpha");
    }

    #[test]
    fn an_unknown_severity_sorts_last_and_gets_a_known_css_modifier() {
        // `severity`/`status` are deliberately NOT schema-constrained, so this
        // build must survive a value slop-ferret adds later. Two requirements:
        // it must not outrank real severities on a guess, and it must not emit
        // an arbitrary producer string as a CSS class (which would render as an
        // unstyled chip).
        let mut findings = vec![
            sample_finding("Future", "catastrophic", "VERIFIED"),
            sample_finding("Known", "note", "VERIFIED"),
        ];
        super::sort_findings(&mut findings);
        assert_eq!(findings[0].title, "Known", "unknown severity must sort after known ones");
        assert_eq!(super::modifier(super::SEV_ORDER, "catastrophic"), "unknown");
        assert_eq!(super::modifier(super::SEV_ORDER, "BLOCKING"), "blocking", "case-insensitive");
        assert_eq!(super::modifier(&["verified", "suspected"], "REFUTED"), "unknown");
    }

    #[test]
    fn status_comparison_is_case_insensitive() {
        // The schema no longer pins the casing, so a producer emitting
        // "verified" must not be silently demoted below "SUSPECTED".
        let mut findings = vec![
            sample_finding("A suspected", "note", "SUSPECTED"),
            sample_finding("Z verified", "note", "verified"),
        ];
        super::sort_findings(&mut findings);
        assert_eq!(findings[0].title, "Z verified");
    }
}
