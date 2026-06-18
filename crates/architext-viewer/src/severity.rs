//! Ordinal severity / sensitivity color scales.
//!
//! DESIGN.md: rule criticality, risk severity, and data-class sensitivity are
//! STATE/SEVERITY signals on their OWN ordinal ramps — never the `--c4-*`
//! component-type role palette. Centralized here so "critical" looks identical
//! across the Rules and Data/Risks surfaces, and the hue lives only in
//! `styles.css` (`--sev-*` / `--sens-*`); these return `var()` references.

/// Severity/criticality (critical > high > medium > low) → `--sev-*` token.
/// Unknown → the lowest/dim tone. Shared by rule criticality and risk severity.
pub fn severity_color_var(level: Option<&str>) -> &'static str {
    match level {
        Some("critical") => "var(--sev-critical)",
        Some("high") => "var(--sev-high)",
        Some("medium") => "var(--sev-medium)",
        Some("low") => "var(--sev-low)",
        _ => "var(--sev-low)",
    }
}

/// Data-classification sensitivity (high > medium > low) → `--sens-*` token.
pub fn sensitivity_color_var(level: Option<&str>) -> &'static str {
    match level {
        Some("high") => "var(--sens-high)",
        Some("medium") => "var(--sens-medium)",
        Some("low") => "var(--sens-low)",
        _ => "var(--sens-low)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_are_their_own_tokens_not_role_hues() {
        for l in ["critical", "high", "medium", "low", "x"] {
            assert!(severity_color_var(Some(l)).starts_with("var(--sev-"));
            assert!(!severity_color_var(Some(l)).contains("--c4-"));
        }
        for l in ["high", "medium", "low", "x"] {
            assert!(sensitivity_color_var(Some(l)).starts_with("var(--sens-"));
        }
    }
}
