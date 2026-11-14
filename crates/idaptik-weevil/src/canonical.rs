//! Canonical bytes and their SHA-256, for documents another process verifies.
//!
//! The estate's interchange profile is RFC 8785 (JCS) over integer-only
//! I-JSON: object members sorted by the UTF-16 code units of their keys, no
//! insignificant whitespace, one escape for each string character, and no
//! number that is not an integer in `-(2^53 − 1) ..= 2^53 − 1`. A document
//! written here is compared byte for byte and hashed by a validator in another
//! repository, so every rule is enforced rather than approximated: a
//! non-integer or out-of-range number is an error, never a rounded value.
//!
//! This is the subset the replay driver's documents use. It is written for
//! this crate on purpose; IDApTIK's ADR-0009 forbids a dependency edge to the
//! validator's crates, so the bytes are produced here and checked there.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

/// The largest integer magnitude the profile admits: `2^53 − 1`.
pub const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// A value outside the integer-only I-JSON subset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotCanonical {
    /// The offending number, as serde_json printed it.
    pub number: String,
}

impl std::fmt::Display for NotCanonical {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "number {} is not an integer in -(2^53 - 1) ..= 2^53 - 1",
            self.number
        )
    }
}

impl std::error::Error for NotCanonical {}

/// The JCS text of `v`, without a trailing newline.
pub fn to_jcs(v: &Value) -> Result<String, NotCanonical> {
    let mut out = String::new();
    write_value(&mut out, v)?;
    Ok(out)
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn write_value(out: &mut String, v: &Value) -> Result<(), NotCanonical> {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(u) = n.as_u64().filter(|u| *u <= MAX_SAFE_INTEGER) {
                let _ = write!(out, "{u}");
            } else if let Some(i) = n.as_i64().filter(|i| i.unsigned_abs() <= MAX_SAFE_INTEGER) {
                let _ = write!(out, "{i}");
            } else {
                return Err(NotCanonical {
                    number: n.to_string(),
                });
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
            // JCS orders by UTF-16 code units, which differs from byte order
            // only outside the BMP; sorting on code units is right for both.
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

/// JCS strings: `"` and `\` escaped, the five short escapes, `\u00xx` in
/// lowercase hex for the other C0 controls, and every other character as is.
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
    fn members_are_sorted_and_whitespace_is_dropped() {
        let v = json!({"b": [1, {"z": true, "a": null}], "a": "x"});
        assert_eq!(
            to_jcs(&v).unwrap(),
            r#"{"a":"x","b":[1,{"a":null,"z":true}]}"#
        );
    }

    #[test]
    fn keys_sort_by_utf16_code_units_not_bytes() {
        // U+FF61 is one UTF-16 unit (0xFF61); U+1F600 is a surrogate pair
        // starting 0xD83D. Byte order puts U+FF61 first (EF.. < F0..);
        // UTF-16 order puts the emoji first.
        let v = json!({"\u{ff61}": 1, "\u{1f600}": 2});
        assert_eq!(to_jcs(&v).unwrap(), "{\"\u{1f600}\":2,\"\u{ff61}\":1}");
    }

    #[test]
    fn strings_use_the_one_jcs_escape() {
        let v = json!("q\"b\\n\nt\tc\u{1}é");
        assert_eq!(to_jcs(&v).unwrap(), "\"q\\\"b\\\\n\\nt\\tc\\u0001é\"");
    }

    #[test]
    fn integers_at_the_bound_pass_and_beyond_it_fail() {
        assert_eq!(
            to_jcs(&json!(MAX_SAFE_INTEGER)).unwrap(),
            "9007199254740991"
        );
        assert_eq!(
            to_jcs(&json!(-(MAX_SAFE_INTEGER as i64))).unwrap(),
            "-9007199254740991"
        );
        assert!(to_jcs(&json!(MAX_SAFE_INTEGER + 1)).is_err());
        assert!(to_jcs(&json!(-((MAX_SAFE_INTEGER + 1) as i64))).is_err());
        assert!(to_jcs(&json!(0.5)).is_err());
    }

    #[test]
    fn sha256_matches_the_fips_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
