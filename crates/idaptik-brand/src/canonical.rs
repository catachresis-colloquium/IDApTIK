//! The estate's I-JSON/JCS profile for the subset of JSON the tokens use.
//!
//! RFC 8785 (JCS) fixes one byte sequence for a JSON value: object members
//! sorted by their keys' UTF-16 code units, no insignificant whitespace,
//! strings escaped one way, numbers in ES6 `Number::toString` form. RFC 7493
//! (I-JSON) forbids duplicate keys and numbers outside IEEE double exactness.
//!
//! This module implements exactly the part of that the token file needs and
//! **refuses the rest** rather than approximating it: a non-integer number, or
//! an integer beyond 2^53, is an error, because rendering it would need the
//! ES6 shortest-round-trip algorithm and this file has no business containing
//! one. Keys here are ASCII, where UTF-16 order is byte order; the sort is
//! still done on code units so that a non-ASCII key would be ordered
//! correctly rather than silently by bytes.
//!
//! The kyq gate this repository is migrating to (docs/standards-conformance.md)
//! is not yet deployable; until it is, this is the transitional canonicaliser
//! for this one file, and it is tested against hand-written vectors below.

use serde_json::Value;
use std::fmt::Write as _;

/// Why a value is outside the subset this module canonicalises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unsupported {
    /// A number that is not an integer, or not exactly representable.
    Number(String),
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unsupported::Number(n) => write!(f, "number {n} is outside the I-JSON integer subset"),
        }
    }
}

impl std::error::Error for Unsupported {}

/// The JCS bytes of `v`, without a trailing newline.
pub fn to_jcs(v: &Value) -> Result<String, Unsupported> {
    let mut out = String::new();
    write_value(&mut out, v)?;
    Ok(out)
}

/// 2^53: the largest integer magnitude I-JSON lets a number carry exactly.
const MAX_EXACT: u64 = 1 << 53;

fn write_value(out: &mut String, v: &Value) -> Result<(), Unsupported> {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(u) = n.as_u64().filter(|u| *u <= MAX_EXACT) {
                let _ = write!(out, "{u}");
            } else if let Some(i) = n.as_i64().filter(|i| i.unsigned_abs() <= MAX_EXACT) {
                let _ = write!(out, "{i}");
            } else {
                return Err(Unsupported::Number(n.to_string()));
            }
        }
        Value::String(s) => write_string(out, s),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, item)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut members: Vec<(&String, &Value)> = map.iter().collect();
            // JCS: sort by the key's UTF-16 code units.
            members.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
            out.push('{');
            for (i, (k, item)) in members.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(out, k);
                out.push(':');
                write_value(out, item)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

/// JCS string serialisation: `"`, `\` and the C0 controls escaped, the five
/// short escapes where they exist, `\u00xx` (lowercase hex) otherwise, and
/// nothing else escaped.
fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn members_are_sorted_and_whitespace_is_gone() {
        let v = json!({"b": 1, "a": [true, null, "x"], "c": {"z": 0, "y": -2}});
        assert_eq!(
            to_jcs(&v).unwrap(),
            r#"{"a":[true,null,"x"],"b":1,"c":{"y":-2,"z":0}}"#
        );
    }

    #[test]
    fn strings_escape_the_jcs_way() {
        let v = json!("q\" bs\\ nl\n tab\t ctl\u{1} slash/ é");
        assert_eq!(
            to_jcs(&v).unwrap(),
            "\"q\\\" bs\\\\ nl\\n tab\\t ctl\\u0001 slash/ é\""
        );
    }

    #[test]
    fn utf16_order_not_byte_order() {
        // RFC 8785's own example: U+1F600 (surrogates D83D DE00) sorts BEFORE
        // U+FF5E (one code unit FF5E) because D83D < FF5E -- while in UTF-8
        // byte order (F0 .. vs EF ..) it would come after.
        let v = json!({"\u{FF5E}": 2, "\u{1F600}": 1});
        assert_eq!(to_jcs(&v).unwrap(), "{\"\u{1F600}\":1,\"\u{FF5E}\":2}");
    }

    #[test]
    fn non_integers_are_refused_not_approximated() {
        assert!(matches!(to_jcs(&json!(1.5)), Err(Unsupported::Number(_))));
        assert!(matches!(
            to_jcs(&json!(9007199254740993u64)),
            Err(Unsupported::Number(_))
        ));
        assert_eq!(
            to_jcs(&json!(9007199254740992u64)).unwrap(),
            "9007199254740992"
        );
        assert_eq!(to_jcs(&json!(-7)).unwrap(), "-7");
    }
}
