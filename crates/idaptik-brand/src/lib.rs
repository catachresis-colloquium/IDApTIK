//! The IDApTIK brand tokens, twice: as **data** and as **constants**.
//!
//! * `config/brand-tokens.ncl` is the source of truth, typechecked by
//!   `config/brand-schema.ncl`.
//! * [`TOKENS_JSON`] (`tokens.json` beside this crate) is its export in the
//!   estate's I-JSON/JCS profile: sorted keys, no whitespace, one trailing
//!   LF. It is the file a design tool re-imports, and the committed golden
//!   that `just brand-check` compares against a fresh Nickel export.
//! * [`generated`] is rendered from that JSON by [`Brand::render_generated`]:
//!   `0xRRGGBB` colour constants and pixel constants that the Bevy palette
//!   (`idaptik_bevy::palette`) wraps. The committed file is compared against
//!   a fresh rendering on every `cargo test`, so it cannot drift.
//!
//! Nothing here reads the JSON at runtime: the constants are the runtime
//! surface, the JSON is the interchange surface, and the tests are what make
//! the two the same thing.
//!
//! The legibility rule (WCAG 2 contrast, 4.5:1 for text and 3:1 for large
//! text and UI glyphs) is decided exactly in `idaptik_kernel::contrast`; this
//! crate's [`srgb`] module does the one float step, linearising channels, and
//! tests that step against the transfer function it implements.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

pub mod canonical;
pub mod generated;
pub mod srgb;

/// The committed token export: I-JSON, JCS bytes, one trailing LF.
pub const TOKENS_JSON: &str = include_str!("../tokens.json");

/// One family of tokens, in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Family<T> {
    pub tokens: Vec<T>,
}

/// Per-theme values of a colour. One theme today.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Themes {
    pub night: String,
}

/// A named colour with its usage rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColourToken {
    pub name: String,
    /// Lowercase `#rrggbb`.
    pub value: String,
    pub usage: String,
    /// `ground`, `accent` or `scene`.
    pub group: String,
    /// The seat law: `hacker`, `infiltrator`, `threat` or `none`.
    pub seat: String,
    pub themes: Themes,
}

/// A bundled typeface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontToken {
    pub name: String,
    /// The family name.
    pub value: String,
    pub usage: String,
    pub file: String,
    pub licence: String,
    pub licence_file: String,
    pub grid_px: u32,
    pub default: bool,
}

/// A named pixel measure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PxToken {
    pub name: String,
    pub value: u32,
    pub usage: String,
}

/// The whole token set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Brand {
    pub schema_version: String,
    pub colour: Family<ColourToken>,
    pub font: Family<FontToken>,
    pub size: Family<PxToken>,
    pub space: Family<PxToken>,
    pub radius: Family<PxToken>,
    pub grid: Family<PxToken>,
}

impl ColourToken {
    /// The colour as `0xRRGGBB`.
    pub fn rgb(&self) -> u32 {
        parse_hex(&self.value).unwrap_or_else(|| panic!("token {} has a non-hex value", self.name))
    }
}

/// Parse lowercase `#rrggbb` to `0xRRGGBB`; `None` for anything else.
pub fn parse_hex(s: &str) -> Option<u32> {
    let digits = s.strip_prefix('#')?;
    if digits.len() != 6
        || !digits
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    u32::from_str_radix(digits, 16).ok()
}

impl Brand {
    /// The committed tokens.
    pub fn load() -> Brand {
        serde_json::from_str(TOKENS_JSON).expect("tokens.json is valid")
    }

    /// Render [`generated`] from these tokens. Deterministic: the same tokens
    /// always give the same bytes, which is what the drift test relies on.
    pub fn render_generated(&self) -> String {
        let mut out = String::new();
        out.push_str(
            "//! GENERATED from `tokens.json` by `just brand-regen` (`Brand::render_generated`).\n\
             //! Do not edit: change `config/brand-tokens.ncl` and regenerate. The test\n\
             //! `generated_rs_matches_tokens_json` fails if this file and the JSON disagree.\n\n",
        );

        out.push_str(
            "/// Colours as `0xRRGGBB`, in source order.\n#[rustfmt::skip]\npub mod colour {\n",
        );
        for t in &self.colour.tokens {
            let _ = writeln!(out, "    /// {} ({}, seat: {})", t.usage, t.group, t.seat);
            let _ = writeln!(
                out,
                "    pub const {}: u32 = 0x{:06x};",
                upper(&t.name),
                t.rgb()
            );
        }
        out.push_str("    /// Every colour token: `(name, group, seat, value)`.\n");
        out.push_str("    pub const ALL: &[(&str, &str, &str, u32)] = &[\n");
        for t in &self.colour.tokens {
            let _ = writeln!(
                out,
                "        ({:?}, {:?}, {:?}, {}),",
                t.name,
                t.group,
                t.seat,
                upper(&t.name)
            );
        }
        out.push_str("    ];\n}\n\n");

        out.push_str("/// The bundled typefaces: family names and grids.\n#[rustfmt::skip]\npub mod font {\n");
        for t in &self.font.tokens {
            let _ = writeln!(out, "    /// {}", t.usage);
            let _ = writeln!(
                out,
                "    pub const {}: &str = {:?};",
                upper(&t.name),
                t.value
            );
            let _ = writeln!(
                out,
                "    pub const {}_FILE: &str = {:?};",
                upper(&t.name),
                t.file
            );
            let _ = writeln!(
                out,
                "    pub const {}_GRID_PX: u32 = {};",
                upper(&t.name),
                t.grid_px
            );
        }
        let default = self
            .font
            .tokens
            .iter()
            .find(|t| t.default)
            .expect("one font is the default");
        out.push_str("    /// The default face's family name.\n");
        let _ = writeln!(
            out,
            "    pub const DEFAULT: &str = {};",
            upper(&default.name)
        );
        out.push_str("    /// Every typeface: `(name, family, file, grid_px, default)`.\n");
        out.push_str("    pub const ALL: &[(&str, &str, &str, u32, bool)] = &[\n");
        for t in &self.font.tokens {
            let _ = writeln!(
                out,
                "        ({:?}, {}, {}_FILE, {}_GRID_PX, {}),",
                t.name,
                upper(&t.name),
                upper(&t.name),
                upper(&t.name),
                t.default
            );
        }
        out.push_str("    ];\n}\n");

        for (module, doc, family) in [
            ("size", "The type ramp as requested, in px.", &self.size),
            ("space", "Spacing, in px.", &self.space),
            ("radius", "Corner radius, in px.", &self.radius),
            ("grid", "The art grid.", &self.grid),
        ] {
            let _ = write!(out, "\n/// {doc}\n#[rustfmt::skip]\npub mod {module} {{\n");
            for t in &family.tokens {
                let _ = writeln!(out, "    /// {}", t.usage);
                let _ = writeln!(out, "    pub const {}: u32 = {};", upper(&t.name), t.value);
            }
            let _ = writeln!(out, "    /// Every {module} token: `(name, value)`.");
            out.push_str("    pub const ALL: &[(&str, u32)] = &[\n");
            for t in &family.tokens {
                let _ = writeln!(out, "        ({:?}, {}),", t.name, upper(&t.name));
            }
            out.push_str("    ];\n}\n");
        }
        out
    }
}

/// `snake_case` token name to `SCREAMING_SNAKE` constant name.
fn upper(name: &str) -> String {
    name.to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENERATED_RS: &str = include_str!("generated.rs");
    const GENERATED_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/generated.rs");
    const TOKENS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tokens.json");
    /// Where `just brand-check` / `just brand-regen` leave the Nickel export.
    const NICKEL_EXPORT: &str = "/tmp/brand_tokens_raw.json";

    #[test]
    fn tokens_json_is_canonical() {
        // The committed file must already be in the I-JSON/JCS profile:
        // re-canonicalising it is the identity, and it ends in exactly one LF.
        let v: serde_json::Value = serde_json::from_str(TOKENS_JSON).expect("parses");
        let want = canonical::to_jcs(&v).expect("within the I-JSON subset") + "\n";
        assert_eq!(
            TOKENS_JSON, want,
            "tokens.json is not canonical; run `just brand-regen`"
        );
    }

    #[test]
    fn tokens_json_round_trips_through_the_typed_model() {
        // `deny_unknown_fields` on every struct: a field the model does not
        // know is a schema drift, not something to ignore.
        let b = Brand::load();
        let v: serde_json::Value = serde_json::to_value(&b).expect("serialise");
        let back = canonical::to_jcs(&v).expect("subset") + "\n";
        assert_eq!(
            back, TOKENS_JSON,
            "the typed model loses or reorders something"
        );
    }

    #[test]
    fn generated_rs_matches_tokens_json() {
        let want = Brand::load().render_generated();
        assert_eq!(
            GENERATED_RS, want,
            "generated.rs is stale; run `just brand-regen`"
        );
    }

    #[test]
    fn every_colour_is_lowercase_hex_and_names_are_unique() {
        let b = Brand::load();
        let mut names = std::collections::BTreeSet::new();
        for t in &b.colour.tokens {
            assert!(parse_hex(&t.value).is_some(), "{}: {}", t.name, t.value);
            assert_eq!(t.value, t.themes.night, "{}: theme drifted", t.name);
            assert!(names.insert(t.name.clone()), "duplicate colour {}", t.name);
        }
        assert!(!names.is_empty());
    }

    #[test]
    fn hex_parser_rejects_what_the_schema_rejects() {
        assert_eq!(parse_hex("#0c0c0e"), Some(0x0c0c0e));
        assert_eq!(parse_hex("#0C0C0E"), None, "uppercase is not canonical");
        assert_eq!(parse_hex("0c0c0e"), None);
        assert_eq!(parse_hex("#0c0c0"), None);
        assert_eq!(parse_hex("#0c0c0eff"), None);
    }

    /// `just brand-check`: the committed golden equals a fresh Nickel export,
    /// canonicalised. Ignored by default because it needs that export on disk.
    #[test]
    #[ignore = "needs the Nickel export at /tmp/brand_tokens_raw.json (just brand-check)"]
    fn tokens_json_matches_nickel_export() {
        let raw = std::fs::read_to_string(NICKEL_EXPORT).expect("nickel export");
        let v: serde_json::Value = serde_json::from_str(&raw).expect("parse nickel json");
        let want = canonical::to_jcs(&v).expect("subset") + "\n";
        assert_eq!(
            TOKENS_JSON, want,
            "tokens.json drifted from config/brand-tokens.ncl; run `just brand-regen`"
        );
    }

    /// `just brand-regen`, step 1: rewrite the committed golden from the export.
    #[test]
    #[ignore = "regenerates the committed golden; run via just brand-regen"]
    fn regenerate_tokens_json() {
        let raw = std::fs::read_to_string(NICKEL_EXPORT).expect("nickel export");
        let v: serde_json::Value = serde_json::from_str(&raw).expect("parse nickel json");
        // Round-trip through the typed model first, so a field the model does
        // not know fails here rather than landing in the golden.
        let b: Brand = serde_json::from_value(v.clone()).expect("export fits the typed model");
        assert_eq!(serde_json::to_value(&b).expect("serialise"), v);
        let out = canonical::to_jcs(&v).expect("subset") + "\n";
        std::fs::write(TOKENS_PATH, out).expect("write tokens.json");
    }

    /// `just brand-regen`, step 2: rewrite `generated.rs` from the golden.
    #[test]
    #[ignore = "regenerates generated.rs; run via just brand-regen"]
    fn regenerate_generated_rs() {
        let fresh = std::fs::read_to_string(TOKENS_PATH).expect("tokens.json");
        let b: Brand = serde_json::from_str(&fresh).expect("tokens.json is valid");
        std::fs::write(GENERATED_PATH, b.render_generated()).expect("write generated.rs");
    }
}
