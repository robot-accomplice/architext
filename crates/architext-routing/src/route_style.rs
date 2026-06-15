//! Faithful port of `viewer/src/routing/routeStyle.js`.

/// Port of JS `normalizeRouteStyle(style)`.
///
/// - `"spline"` or `"curved"` → `"spline"`
/// - `"straight"` → `"straight"`
/// - anything else → `"orthogonal"`
pub fn normalize_route_style(style: &str) -> &'static str {
    if style == "spline" || style == "curved" {
        return "spline";
    }
    if style == "straight" {
        return "straight";
    }
    "orthogonal"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spline_and_curved_normalize_to_spline() {
        assert_eq!(normalize_route_style("spline"), "spline");
        assert_eq!(normalize_route_style("curved"), "spline");
    }

    #[test]
    fn straight_normalizes_to_straight() {
        assert_eq!(normalize_route_style("straight"), "straight");
    }

    #[test]
    fn unknown_falls_back_to_orthogonal() {
        assert_eq!(normalize_route_style("orthogonal"), "orthogonal");
        assert_eq!(normalize_route_style(""), "orthogonal");
        assert_eq!(normalize_route_style("anything"), "orthogonal");
    }
}
