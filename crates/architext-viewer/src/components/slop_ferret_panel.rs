//! Slop Ferret sweep viewer: coverage banner + findings list.

use leptos::*;

use crate::data::models::{SlopFerret, SlopFerretFinding};
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
                        <div class="slop-ferret-panel__empty">
                            <span class="overline">"SLOP FERRET"</span>
                            <p>"No slop-ferret snapshot is registered in this project. Run `architext slop-ferret --plan ... --discharge ... --findings ...` to create one."</p>
                        </div>
                    }.into_view(),
                }
            }}
        </div>
    }
}

fn render_snapshot(doc: &SlopFerret) -> View {
    let mut findings = doc.findings.clone();
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
        let verified_a = a.status == "VERIFIED";
        let verified_b = b.status == "VERIFIED";
        if verified_a != verified_b {
            return if verified_a { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
        }
        a.title.cmp(&b.title)
    });

    view! {
        <div class="slop-ferret-panel__content">
            <div class="slop-ferret-panel__header">
                <span class="overline">"SLOP FERRET"</span>
                <h2>"Sweep findings"</h2>
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
            <div class="slop-ferret-panel__findings">
                {findings.into_iter().map(finding_card).collect_view()}
            </div>
        </div>
    }
    .into_view()
}

fn finding_card(f: SlopFerretFinding) -> View {
    let sev_class = format!("slop-ferret-panel__severity slop-ferret-panel__severity--{}", f.severity.to_lowercase());
    let status_class = format!("slop-ferret-panel__status slop-ferret-panel__status--{}", f.status.to_lowercase());
    view! {
        <div class="slop-ferret-panel__finding">
            <div class="slop-ferret-panel__finding-header">
                <span class=sev_class>{f.severity.clone()}</span>
                <span class=status_class>{f.status.clone()}</span>
                <span class="slop-ferret-panel__class">{f.class.clone()}</span>
            </div>
            <h3 class="slop-ferret-panel__finding-title">{f.title}</h3>
            <div class="slop-ferret-panel__meta">{f.file.clone()}</div>
            {(!f.claim.is_empty()).then(|| view! {
                <p class="slop-ferret-panel__claim">{f.claim.clone()}</p>
            })}
            {(!f.evidence.is_empty()).then(|| view! {
                <div class="slop-ferret-panel__section">
                    <span class="overline">"EVIDENCE"</span>
                    <p>{f.evidence.clone()}</p>
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
        let mut findings = vec![
            sample_finding("Cosmetic", "note", "VERIFIED"),
            sample_finding("Auth bug", "blocking", "VERIFIED"),
            sample_finding("Dupe", "fix-or-file", "VERIFIED"),
        ];
        findings.sort_by(|a, b| {
            let rank = |s: &str| match s {
                "blocking" => 0,
                "fix-or-file" => 1,
                "note" => 2,
                _ => 3,
            };
            rank(&a.severity).cmp(&rank(&b.severity))
        });
        assert_eq!(findings[0].title, "Auth bug");
        assert_eq!(findings[1].title, "Dupe");
        assert_eq!(findings[2].title, "Cosmetic");
    }
}
