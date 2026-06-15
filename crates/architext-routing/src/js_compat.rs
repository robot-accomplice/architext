//! Exact reproductions of the JS numeric/string/sort semantics the router relies
//! on. Each primitive is verified against Node golden values; getting one wrong
//! produces a different diagram, not a crash.

/// JS `Math.round`: round half toward +∞. Differs from Rust `f64::round`
/// (half away from zero) on negative halves, and yields -0.0 for x in (-0.5, 0].
pub fn js_round(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return x;
    }
    // floor(x + 0.5) reproduces V8's half-toward-+∞ for finite values.
    let r = (x + 0.5).floor();
    // Preserve JS negative-zero: Math.round(x) for x in (-0.5, 0] is -0.
    if r == 0.0 && x.is_sign_negative() {
        return -0.0;
    }
    r
}

#[cfg(test)]
mod round_tests {
    use super::js_round;

    #[test]
    fn matches_v8_goldens() {
        // (input, expected) from Node Math.round.
        let cases = [
            (2.5_f64, 3.0_f64),
            (2.4, 2.0),
            (-2.5, -2.0),
            (-2.6, -3.0),
            (0.5, 1.0),
            (-0.5, 0.0),   // JS: -0; value compares equal to 0.0
            (-0.3, 0.0),   // JS: -0
            (120.0, 120.0),
        ];
        for (input, expected) in cases {
            assert_eq!(js_round(input), expected, "js_round({input})");
        }
    }

    #[test]
    fn negative_small_is_negative_zero() {
        // JS Math.round(-0.3) is -0: sign bit set, value zero.
        let r = js_round(-0.3);
        assert!(r == 0.0 && r.is_sign_negative(), "expected -0.0, got {r}");
    }
}
