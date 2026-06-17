//! The ONE shared wordmark component (DESIGN.md rule 2).
//!
//! Renders the literal "ARCHITEXT" everywhere it appears — never a truncation.
//! Because every header pulls this single component, the "HITEXT" mockup
//! artifact from the Stitch screens cannot occur by construction.
use leptos::*;

/// The canonical wordmark text. One definition — do not inline elsewhere.
const WORDMARK: &str = "ARCHITEXT";

#[component]
pub fn Wordmark() -> impl IntoView {
    view! {
        <div class="wordmark">
            <span class="wordmark__rail"></span>
            <span>{WORDMARK}</span>
        </div>
    }
}
