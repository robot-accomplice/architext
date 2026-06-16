//! `write_json_string` — byte-identical reproduction of the JS `writeJson`
//! on-disk format.
//!
//! The JS contract (src/adapters/cli/runtime.mjs):
//!   `${JSON.stringify(value, null, 2)}\n`
//!
//! Key semantics replicated:
//! - 2-space indent; `": "` key-value separator; `,\n` item separator.
//! - Empty containers render as `{}` / `[]` (no inner whitespace).
//! - Numbers: integers as-is; floats via ECMAScript Number::toString
//!   (`js_number_to_string`). `-0` → `"0"`.
//! - Strings: escape `"` `\` and C0 control chars; pass non-ASCII through raw.
//!   `/` is NOT escaped. U+2028/U+2029 are NOT escaped.
//! - Returns the stringified value with a trailing `\n` appended.

use architext_routing::js_compat::js_number_to_string;
use serde_json::Value;

/// Serialise `value` to the on-disk format produced by `JSON.stringify(v, null, 2) + "\n"`.
///
/// The result is byte-identical to what the JS `writeJson` helper writes.
pub fn write_json_string(value: &Value) -> String {
    let mut out = String::new();
    write_value(value, 0, &mut out);
    out.push('\n');
    out
}

fn write_value(value: &Value, depth: usize, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => write_number(n, out),
        Value::String(s) => write_string(s, out),
        Value::Array(arr) => write_array(arr, depth, out),
        Value::Object(obj) => write_object(obj, depth, out),
    }
}

fn write_number(n: &serde_json::Number, out: &mut String) {
    // serde_json::Number distinguishes i64, u64, and f64 internally.
    // JSON.stringify prints integers without a decimal point.
    if let Some(i) = n.as_i64() {
        // Integer representation (covers all negative integers and small positives).
        // Note: -0 is not representable as i64/u64 in serde_json — it parses
        // as the f64 arm below.
        out.push_str(&i.to_string());
    } else if let Some(u) = n.as_u64() {
        out.push_str(&u.to_string());
    } else if let Some(f) = n.as_f64() {
        // ECMAScript Number::toString via ryu-js.
        // JSON.stringify(-0) → "0": ryu-js already normalises -0 → "0".
        // JSON.stringify(1e21) → "1e+21"; JSON.stringify(100.0) → "100" (whole).
        out.push_str(&js_number_to_string(f));
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\x08' => out.push_str("\\b"),
            '\x09' => out.push_str("\\t"),
            '\x0A' => out.push_str("\\n"),
            '\x0C' => out.push_str("\\f"),
            '\x0D' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => {
                // Other C0 control characters: \u00XX (lowercase hex)
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            // All other chars (incl. non-ASCII, '/', U+2028, U+2029) pass through raw.
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_array(arr: &[Value], depth: usize, out: &mut String) {
    if arr.is_empty() {
        out.push_str("[]");
        return;
    }
    out.push('[');
    let inner_depth = depth + 1;
    let indent = "  ".repeat(inner_depth);
    let close_indent = "  ".repeat(depth);
    for (i, item) in arr.iter().enumerate() {
        out.push('\n');
        out.push_str(&indent);
        write_value(item, inner_depth, out);
        if i + 1 < arr.len() {
            out.push(',');
        }
    }
    out.push('\n');
    out.push_str(&close_indent);
    out.push(']');
}

fn write_object(obj: &serde_json::Map<String, Value>, depth: usize, out: &mut String) {
    if obj.is_empty() {
        out.push_str("{}");
        return;
    }
    out.push('{');
    let inner_depth = depth + 1;
    let indent = "  ".repeat(inner_depth);
    let close_indent = "  ".repeat(depth);
    let len = obj.len();
    for (i, (key, val)) in obj.iter().enumerate() {
        out.push('\n');
        out.push_str(&indent);
        write_string(key, out);
        out.push_str(": ");
        write_value(val, inner_depth, out);
        if i + 1 < len {
            out.push(',');
        }
    }
    out.push('\n');
    out.push_str(&close_indent);
    out.push('}');
}

#[cfg(test)]
mod tests {
    use super::write_json_string;
    use serde_json::{json, Value};

    fn js(v: &Value) -> String {
        write_json_string(v)
    }

    // --- Trailing newline ---

    #[test]
    fn trailing_newline_always_appended() {
        let result = js(&json!({}));
        assert!(result.ends_with('\n'), "must end with newline; got: {result:?}");
    }

    // --- Empty containers ---

    #[test]
    fn empty_object_no_inner_whitespace() {
        assert_eq!(js(&json!({})), "{}\n");
    }

    #[test]
    fn empty_array_no_inner_whitespace() {
        assert_eq!(js(&json!([])), "[]\n");
    }

    #[test]
    fn nested_empties() {
        let v = json!({ "a": {}, "b": [] });
        assert_eq!(js(&v), "{\n  \"a\": {},\n  \"b\": []\n}\n");
    }

    // --- Simple scalars ---

    #[test]
    fn null_value() {
        assert_eq!(js(&json!(null)), "null\n");
    }

    #[test]
    fn bool_true() {
        assert_eq!(js(&json!(true)), "true\n");
    }

    #[test]
    fn bool_false() {
        assert_eq!(js(&json!(false)), "false\n");
    }

    // --- Numbers ---

    #[test]
    fn integer_zero() {
        assert_eq!(js(&json!(0)), "0\n");
    }

    #[test]
    fn integer_negative() {
        assert_eq!(js(&json!(-3)), "-3\n");
    }

    #[test]
    fn integer_large() {
        assert_eq!(js(&json!(9007199254740991_u64)), "9007199254740991\n");
    }

    #[test]
    fn float_half() {
        assert_eq!(js(&json!(0.5)), "0.5\n");
    }

    #[test]
    fn float_negative() {
        assert_eq!(js(&json!(-1.25)), "-1.25\n");
    }

    #[test]
    fn whole_number_float_no_decimal() {
        // JSON.stringify(100.0) → "100", NOT "100.0"
        // serde_json represents 100.0 as f64 with no fractional part;
        // ryu-js renders it as "100".
        let v: Value = serde_json::from_str("100.0").unwrap();
        // serde_json parses 100.0 as f64 → Number::from_f64
        assert_eq!(js(&v), "100\n");
    }

    #[test]
    fn float_very_large_exponential() {
        // JSON.stringify(1e21) → "1e+21"
        let v: Value = serde_json::from_str("1e21").unwrap();
        assert_eq!(js(&v), "1e+21\n");
    }

    #[test]
    fn float_very_small_exponential() {
        // JSON.stringify(1e-7) → "1e-7"
        let v: Value = serde_json::from_str("1e-7").unwrap();
        assert_eq!(js(&v), "1e-7\n");
    }

    #[test]
    fn negative_zero_float_serializes_as_zero() {
        // JSON.stringify(-0) → "0"
        // serde_json parses -0 as f64 -0.0; ryu-js formats -0.0 as "0".
        let v: Value = serde_json::from_str("-0").unwrap();
        assert_eq!(js(&v), "0\n");
    }

    // --- Strings: escape sequences ---

    #[test]
    fn string_plain_ascii() {
        assert_eq!(js(&json!("hello")), "\"hello\"\n");
    }

    #[test]
    fn string_double_quote_escaped() {
        assert_eq!(js(&json!("say \"hi\"")), "\"say \\\"hi\\\"\"\n");
    }

    #[test]
    fn string_backslash_escaped() {
        assert_eq!(js(&json!("a\\b")), "\"a\\\\b\"\n");
    }

    #[test]
    fn string_forward_slash_not_escaped() {
        // JSON.stringify does NOT escape forward slash
        assert_eq!(js(&json!("a/b")), "\"a/b\"\n");
    }

    #[test]
    fn string_control_chars() {
        // \b \t \n \f \r
        let s = "\x08\x09\x0A\x0C\x0D";
        let v = Value::String(s.to_string());
        assert_eq!(js(&v), "\"\\b\\t\\n\\f\\r\"\n");
    }

    #[test]
    fn string_other_c0_control_chars() {
        // C0 chars other than the named ones: \u00XX (lowercase)
        let s = "\x01\x1F";
        let v = Value::String(s.to_string());
        assert_eq!(js(&v), "\"\\u0001\\u001f\"\n");
    }

    #[test]
    fn string_unicode_passthrough() {
        // é, 中, emoji — all pass through raw (no escape)
        assert_eq!(js(&json!("é中🎉")), "\"é中🎉\"\n");
    }

    #[test]
    fn string_u2028_u2029_not_escaped() {
        // JSON.stringify does NOT escape U+2028 / U+2029 (only JS source contexts do)
        let s = "\u{2028}\u{2029}";
        let v = Value::String(s.to_string());
        // Raw UTF-8 bytes for U+2028 = E2 80 A8, U+2029 = E2 80 A9
        assert_eq!(js(&v), "\"\u{2028}\u{2029}\"\n");
    }

    // --- Indent and structure ---

    #[test]
    fn simple_object() {
        let v = json!({ "x": 1, "y": 2 });
        // serde_json preserves insertion order with preserve_order feature
        assert_eq!(js(&v), "{\n  \"x\": 1,\n  \"y\": 2\n}\n");
    }

    #[test]
    fn simple_array() {
        let v = json!([1, 2, 3]);
        assert_eq!(js(&v), "[\n  1,\n  2,\n  3\n]\n");
    }

    #[test]
    fn deep_nesting() {
        let v = json!({ "a": { "b": { "c": 42 } } });
        let expected = "{\n  \"a\": {\n    \"b\": {\n      \"c\": 42\n    }\n  }\n}\n";
        assert_eq!(js(&v), expected);
    }

    #[test]
    fn array_of_objects() {
        let v = json!([{ "id": "a", "v": 1 }, { "id": "b", "v": 2 }]);
        let expected = concat!(
            "[\n",
            "  {\n",
            "    \"id\": \"a\",\n",
            "    \"v\": 1\n",
            "  },\n",
            "  {\n",
            "    \"id\": \"b\",\n",
            "    \"v\": 2\n",
            "  }\n",
            "]\n"
        );
        assert_eq!(js(&v), expected);
    }
}
