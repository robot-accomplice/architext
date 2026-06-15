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

/// V8 `Number.prototype.toString` (radix 10), as used by template interpolation
/// in the SVG `d`-path builder. Reproduced exactly via the `ryu-js` crate.
pub fn js_number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-Infinity" } else { "Infinity" }.to_string();
    }
    let mut buffer = ryu_js::Buffer::new();
    buffer.format(x).to_string()
}

#[cfg(test)]
mod num_str_tests {
    use super::js_number_to_string;

    #[test]
    fn matches_v8_string_goldens() {
        // (input, Node String(x)).
        let cases: &[(f64, &str)] = &[
            (120.0, "120"),
            (1.5, "1.5"),
            (0.1 + 0.2, "0.30000000000000004"),
            (8.0 * 1.6, "12.8"),
            (-3.5, "-3.5"),
            (0.0, "0"),
            (-0.0, "0"),        // V8: -0 stringifies to "0"
            (1e21, "1e+21"),    // V8 switches to exponential at >=1e21
            (1e-7, "1e-7"),     // V8 switches to exponential at <1e-6
            (1e20, "100000000000000000000"),
        ];
        for (input, expected) in cases {
            assert_eq!(&js_number_to_string(*input), expected, "js_number_to_string({input})");
        }
    }
}
