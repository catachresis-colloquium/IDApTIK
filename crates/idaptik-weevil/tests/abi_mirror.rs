//! The ABI mirror gate: `src/abi.rs` against the kernel's generated `ee.h`.
//!
//! The header is the single source of truth for the wire format. This test
//! reads it from the evil-weevil checkout (`$EVIL_WEEVIL_ROOT`), reads the Rust
//! transcription as text, lays every struct out on both sides with the C
//! layout rules (x86-64 SysV, which is what `#[repr(C)]` follows there), and
//! fails on any difference in:
//!
//! * the set of structs (every header struct is mirrored, every `#[repr(C)]`
//!   struct in `abi.rs` is in the header, and the ten structs of ABI 1.2 are
//!   all present);
//! * field count, field name and order;
//! * field type (width and signedness, or the nested struct it names);
//! * field offset and field size;
//! * struct size and alignment;
//! * the header's own `EE_SIZEOF_*` / `EE_OFFSET_*` defines, which must agree
//!   with the layout of the header's own struct bodies, one define per field.
//!
//! A struct member that this parser does not recognise is an error, never a
//! skipped line, and so is an unknown type. A missing header is a failure
//! ("NO CHECK WAS PERFORMED"), never a pass.
//!
//! The check is deliberately independent of both compilers: it does not use
//! the crate (so it does not link the kernel archive) and it does not ask
//! rustc for the layout. The compile-time `offset_of!` assertions in
//! `src/abi.rs` are the second, compiler-side opinion.
//!
//! The tests below follow the estate test doctrine (proven-tests-and-benches
//! `docs/TEST-DOCTRINE.adoc`): the real pair is the silence fixture, and each
//! `firing_*` test presents one specimen of a declared drift class, built by a
//! single exact edit of the real files, and requires the check to fire for
//! that specimen's own reason. An edit that no longer applies is a harness
//! fault and fails loudly.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The structs ABI 1.2 defines. The bijection below already requires every
/// header struct to be mirrored; this floor stops two parsers that both
/// silently found nothing from agreeing on an empty set.
const REQUIRED_STRUCTS: [&str; 10] = [
    "ee_version",
    "ee_init_desc",
    "ee_contact",
    "ee_snapshot",
    "ee_agent",
    "ee_intent",
    "ee_context",
    "ee_mind",
    "ee_mind_trace",
    "ee_model_trace",
];

// ── Model ───────────────────────────────────────────────────────────────────

/// A field type, normalised so the two languages can be compared.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Ty {
    Int {
        bits: u32,
        signed: bool,
    },
    /// A nested struct, by its C name.
    Struct(String),
}

#[derive(Clone, Debug)]
struct Field {
    name: String,
    ty: Ty,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Laid {
    name: String,
    ty: Ty,
    offset: usize,
    size: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Layout {
    fields: Vec<Laid>,
    size: usize,
    align: usize,
}

/// Structs by C name, each with its fields in source order.
type Structs = BTreeMap<String, Vec<Field>>;

fn int_ty(name: &str) -> Option<Ty> {
    let (signed, bits) = match name {
        "uint8_t" | "u8" => (false, 8),
        "int8_t" | "i8" => (true, 8),
        "uint16_t" | "u16" => (false, 16),
        "int16_t" | "i16" => (true, 16),
        "uint32_t" | "u32" => (false, 32),
        "int32_t" | "i32" => (true, 32),
        "uint64_t" | "u64" => (false, 64),
        "int64_t" | "i64" => (true, 64),
        _ => return None,
    };
    Some(Ty::Int { bits, signed })
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `ee_mind_trace` -> `EeMindTrace`.
fn rust_name(c_name: &str) -> String {
    c_name
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// `EeMindTrace` -> `ee_mind_trace`.
fn c_name(rust: &str) -> String {
    let mut out = String::new();
    for (i, ch) in rust.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

// ── The C header ────────────────────────────────────────────────────────────

struct Header {
    structs: Structs,
    sizeof: BTreeMap<String, usize>,
    offsets: BTreeMap<String, usize>,
}

fn strip_c_comments(text: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let block = rest.find("/*");
        let line = rest.find("//");
        match (block, line) {
            (None, None) => {
                out.push_str(rest);
                return Ok(out);
            }
            (Some(b), l) if l.is_none_or(|l| b < l) => {
                out.push_str(&rest[..b]);
                let end = rest[b + 2..]
                    .find("*/")
                    .ok_or("header: unterminated /* comment")?;
                out.push(' ');
                rest = &rest[b + 2 + end + 2..];
            }
            (_, Some(l)) => {
                out.push_str(&rest[..l]);
                rest = match rest[l..].find('\n') {
                    Some(nl) => &rest[l + nl..],
                    None => "",
                };
            }
            (Some(_), None) => unreachable!("covered by the guard above"),
        }
    }
}

fn parse_header(text: &str) -> Result<Header, String> {
    let text = strip_c_comments(text)?;
    let mut aliases: BTreeMap<String, Ty> = BTreeMap::new();
    let mut sizeof = BTreeMap::new();
    let mut offsets = BTreeMap::new();

    for line in text.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        // Scalar typedefs: `typedef int32_t ee_fx;`.
        if let ["typedef", base, alias] = tokens.as_slice()
            && let Some(alias) = alias.strip_suffix(';')
            && let Some(ty) = int_ty(base)
        {
            aliases.insert(alias.to_string(), ty);
        }
        if let ["#define", key, value] = tokens.as_slice() {
            let table = if let Some(name) = key.strip_prefix("EE_SIZEOF_") {
                Some((&mut sizeof, name))
            } else {
                key.strip_prefix("EE_OFFSET_")
                    .map(|name| (&mut offsets, name))
            };
            if let Some((table, name)) = table {
                let v = value
                    .parse::<usize>()
                    .map_err(|_| format!("header: {key} has a non-numeric value {value:?}"))?;
                if table.insert(name.to_string(), v).is_some() {
                    return Err(format!("header: {key} is defined twice"));
                }
            }
        }
    }

    let mut structs = Structs::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("typedef struct") {
        let after = &rest[start + "typedef struct".len()..];
        let open = after
            .find('{')
            .ok_or("header: `typedef struct` without a body")?;
        if !after[..open].trim().is_empty() {
            return Err(format!(
                "header: unsupported tagged struct `typedef struct {}`",
                after[..open].trim()
            ));
        }
        let close = after[open..]
            .find('}')
            .ok_or("header: unterminated struct body")?
            + open;
        let body = &after[open + 1..close];
        let tail = &after[close + 1..];
        let semi = tail
            .find(';')
            .ok_or("header: struct without a typedef name")?;
        let name = tail[..semi].trim();
        if !is_ident(name) {
            return Err(format!("header: bad struct typedef name {name:?}"));
        }
        let mut fields = Vec::new();
        for member in body.split(';') {
            let member = member.trim();
            if member.is_empty() {
                continue;
            }
            let tokens: Vec<&str> = member.split_whitespace().collect();
            let [ty, field] = tokens.as_slice() else {
                return Err(format!(
                    "header: {name}: unrecognised member {member:?} (only `type name;` is supported)"
                ));
            };
            if !is_ident(ty) || !is_ident(field) {
                return Err(format!("header: {name}: unrecognised member {member:?}"));
            }
            let ty = if let Some(t) = int_ty(ty) {
                t
            } else if let Some(t) = aliases.get(*ty) {
                t.clone()
            } else {
                Ty::Struct(ty.to_string())
            };
            fields.push(Field {
                name: field.to_string(),
                ty,
            });
        }
        if structs.insert(name.to_string(), fields).is_some() {
            return Err(format!("header: struct {name} is defined twice"));
        }
        rest = &tail[semi + 1..];
    }

    Ok(Header {
        structs,
        sizeof,
        offsets,
    })
}

// ── The Rust mirror ─────────────────────────────────────────────────────────

fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

/// Every `#[repr(C)]` struct in the mirror, keyed by its C name.
fn parse_mirror(text: &str) -> Result<Structs, String> {
    let mut structs = Structs::new();
    let mut lines = text.lines().enumerate();
    while let Some((_, line)) = lines.next() {
        if strip_line_comment(line).trim() != "#[repr(C)]" {
            continue;
        }
        // Further attributes, then the struct line.
        let (n, header) = loop {
            let (n, line) = lines.next().ok_or("abi.rs: #[repr(C)] at end of file")?;
            let line = strip_line_comment(line).trim();
            if line.starts_with("#[") {
                continue;
            }
            break (n + 1, line);
        };
        let name = header
            .strip_prefix("pub struct ")
            .and_then(|s| s.strip_suffix('{'))
            .map(str::trim)
            .filter(|s| is_ident(s))
            .ok_or_else(|| {
                format!("abi.rs:{n}: #[repr(C)] item is not `pub struct Name {{`: {header:?}")
            })?;
        let mut fields = Vec::new();
        loop {
            let (n, line) = lines
                .next()
                .ok_or_else(|| format!("abi.rs: struct {name} is not closed"))?;
            let line = strip_line_comment(line).trim();
            if line.is_empty() {
                continue;
            }
            if line == "}" {
                break;
            }
            let member = line
                .strip_prefix("pub ")
                .and_then(|s| s.strip_suffix(','))
                .and_then(|s| s.split_once(':'))
                .map(|(f, t)| (f.trim(), t.trim()))
                .filter(|(f, t)| is_ident(f) && is_ident(t));
            let Some((field, ty)) = member else {
                return Err(format!(
                    "abi.rs:{}: {name}: unrecognised member {line:?} (only `pub name: Type,` is supported)",
                    n + 1
                ));
            };
            let ty = match int_ty(ty) {
                Some(t) => t,
                None if ty.starts_with("Ee") => Ty::Struct(c_name(ty)),
                None => {
                    return Err(format!(
                        "abi.rs:{}: {name}.{field}: unknown type {ty}",
                        n + 1
                    ));
                }
            };
            fields.push(Field {
                name: field.to_string(),
                ty,
            });
        }
        if rust_name(&c_name(name)) != name {
            return Err(format!(
                "abi.rs: struct {name} does not follow the Ee<CamelCase> naming of ee_<snake_case>"
            ));
        }
        if structs.insert(c_name(name), fields).is_some() {
            return Err(format!("abi.rs: struct {name} is defined twice"));
        }
    }
    Ok(structs)
}

// ── Layout ──────────────────────────────────────────────────────────────────

fn align_up(value: usize, align: usize) -> usize {
    value.div_ceil(align) * align
}

fn lay_out(structs: &Structs, side: &str) -> Result<BTreeMap<String, Layout>, String> {
    fn one(
        name: &str,
        structs: &Structs,
        side: &str,
        done: &mut BTreeMap<String, Layout>,
        visiting: &mut BTreeSet<String>,
    ) -> Result<Layout, String> {
        if let Some(l) = done.get(name) {
            return Ok(l.clone());
        }
        let fields = structs
            .get(name)
            .ok_or_else(|| format!("{side}: unknown type {name}"))?;
        if !visiting.insert(name.to_string()) {
            return Err(format!("{side}: struct {name} contains itself"));
        }
        if fields.is_empty() {
            return Err(format!("{side}: struct {name} has no fields"));
        }
        let mut offset = 0;
        let mut max_align = 1;
        let mut laid = Vec::new();
        for f in fields {
            let (size, align) = match &f.ty {
                Ty::Int { bits, .. } => {
                    let bytes = (*bits / 8) as usize;
                    (bytes, bytes)
                }
                Ty::Struct(inner) => {
                    let l = one(inner, structs, side, done, visiting)?;
                    (l.size, l.align)
                }
            };
            offset = align_up(offset, align);
            laid.push(Laid {
                name: f.name.clone(),
                ty: f.ty.clone(),
                offset,
                size,
            });
            offset += size;
            max_align = max_align.max(align);
        }
        visiting.remove(name);
        let layout = Layout {
            fields: laid,
            size: align_up(offset, max_align),
            align: max_align,
        };
        done.insert(name.to_string(), layout.clone());
        Ok(layout)
    }

    let mut done = BTreeMap::new();
    for name in structs.keys() {
        one(name, structs, side, &mut done, &mut BTreeSet::new())?;
    }
    Ok(done)
}

// ── The check ───────────────────────────────────────────────────────────────

/// Compare the header text with the mirror text. `Ok` carries the per-struct
/// report; `Err` lists every mismatch found (all of them, not the first).
fn check(header_text: &str, mirror_text: &str) -> Result<String, String> {
    let header = parse_header(header_text)?;
    let mirror = parse_mirror(mirror_text)?;
    let c = lay_out(&header.structs, "header")?;
    let r = lay_out(&mirror, "abi.rs")?;

    let mut fail = Vec::new();
    for name in REQUIRED_STRUCTS {
        if !c.contains_key(name) {
            fail.push(format!("header has no struct {name}"));
        }
    }

    // The header against its own defines.
    let mut used_sizeof = BTreeSet::new();
    let mut used_offsets = BTreeSet::new();
    for (name, l) in &c {
        match header.sizeof.get(name) {
            Some(&s) if s == l.size => {}
            Some(&s) => fail.push(format!(
                "header: EE_SIZEOF_{name} is {s}, its struct body lays out to {}",
                l.size
            )),
            None => fail.push(format!("header: no EE_SIZEOF_{name}")),
        }
        used_sizeof.insert(name.clone());
        for f in &l.fields {
            let key = format!("{name}_{}", f.name);
            match header.offsets.get(&key) {
                Some(&o) if o == f.offset => {}
                Some(&o) => fail.push(format!(
                    "header: EE_OFFSET_{key} is {o}, the struct body puts {name}.{} at {}",
                    f.name, f.offset
                )),
                None => fail.push(format!("header: no EE_OFFSET_{key}")),
            }
            used_offsets.insert(key);
        }
    }
    for key in header.sizeof.keys().filter(|k| !used_sizeof.contains(*k)) {
        fail.push(format!("header: EE_SIZEOF_{key} names no struct"));
    }
    for key in header.offsets.keys().filter(|k| !used_offsets.contains(*k)) {
        fail.push(format!("header: EE_OFFSET_{key} names no struct field"));
    }

    // The mirror against the header.
    for name in r.keys().filter(|n| !c.contains_key(*n)) {
        fail.push(format!(
            "abi.rs: #[repr(C)] struct {} has no {name} in the header",
            rust_name(name)
        ));
    }
    let mut report = String::new();
    for (name, cl) in &c {
        let rname = rust_name(name);
        let Some(rl) = r.get(name) else {
            fail.push(format!(
                "abi.rs: header struct {name} is not mirrored as {rname}"
            ));
            continue;
        };
        let before = fail.len();
        if cl.size != rl.size {
            fail.push(format!(
                "{name} size {} != {rname} size {}",
                cl.size, rl.size
            ));
        }
        if cl.align != rl.align {
            fail.push(format!(
                "{name} align {} != {rname} align {}",
                cl.align, rl.align
            ));
        }
        if cl.fields.len() != rl.fields.len() {
            fail.push(format!(
                "{name} has {} fields, {rname} has {}",
                cl.fields.len(),
                rl.fields.len()
            ));
        }
        for (cf, rf) in cl.fields.iter().zip(&rl.fields) {
            if cf.name != rf.name {
                fail.push(format!(
                    "{name}.{} vs {rname}.{}: field name/order drift",
                    cf.name, rf.name
                ));
                continue;
            }
            if cf.ty != rf.ty {
                fail.push(format!(
                    "{name}.{} type {:?} != {rname}.{} type {:?}",
                    cf.name, cf.ty, rf.name, rf.ty
                ));
            }
            if cf.offset != rf.offset {
                fail.push(format!(
                    "{name}.{} offset {} != {rname}.{} offset {}",
                    cf.name, cf.offset, rf.name, rf.offset
                ));
            }
            if cf.size != rf.size {
                fail.push(format!(
                    "{name}.{} size {} != {rname}.{} size {}",
                    cf.name, cf.size, rf.name, rf.size
                ));
            }
        }
        if fail.len() == before {
            let _ = writeln!(
                report,
                "ok   {name:16} size={:4} fields={:2}",
                cl.size,
                cl.fields.len()
            );
        }
    }

    if fail.is_empty() {
        let _ = writeln!(
            report,
            "ABI mirror matches the generated header on every field, offset and size ({} structs)",
            c.len()
        );
        Ok(report)
    } else {
        Err(format!(
            "{} mismatch(es):\n  {}",
            fail.len(),
            fail.join("\n  ")
        ))
    }
}

// ── Inputs ──────────────────────────────────────────────────────────────────

/// Where the header is. An explicit `$EVIL_WEEVIL_ROOT` must hold it; with no
/// root given, the locations `scripts/weevil_kernel.sh locate` searches.
fn resolve_header(root: Option<&Path>, workspace: &Path) -> Result<PathBuf, String> {
    const REL: &str = "include/evil_weevil/ee.h";
    let candidates: Vec<PathBuf> = match root {
        Some(root) => vec![root.join(REL)],
        None => {
            let mut c = vec![
                workspace.join("../evil-weevil").join(REL),
                workspace.join("../../evil-weevil").join(REL),
            ];
            if let Some(home) = std::env::var_os("HOME") {
                c.push(PathBuf::from(home).join("evil-weevil").join(REL));
            }
            c
        }
    };
    candidates
        .iter()
        .find(|p| p.is_file())
        .cloned()
        .ok_or_else(|| {
            format!(
                "NO CHECK WAS PERFORMED: the generated header is not at any of {candidates:?}. \
                 Set EVIL_WEEVIL_ROOT to the evil-weevil checkout."
            )
        })
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn real_pair() -> (String, String) {
    let root = std::env::var_os("EVIL_WEEVIL_ROOT").map(PathBuf::from);
    let header = resolve_header(root.as_deref(), &workspace()).unwrap_or_else(|e| panic!("{e}"));
    let mirror = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/abi.rs");
    let read = |p: &Path| {
        std::fs::read_to_string(p)
            .unwrap_or_else(|e| panic!("NO CHECK WAS PERFORMED: cannot read {}: {e}", p.display()))
    };
    (read(&header), read(&mirror))
}

/// Apply one exact edit. The edit must match exactly once, or the fixture is
/// stale: that is a harness fault, reported as such rather than as a pass.
fn edit(text: &str, from: &str, to: &str) -> String {
    let hits = text.matches(from).count();
    assert_eq!(
        hits, 1,
        "harness fault: fixture edit {from:?} matches {hits} times, expected exactly 1"
    );
    text.replacen(from, to, 1)
}

/// Run a firing specimen: the check must fail, and for this specimen's reason.
fn fires(header: &str, mirror: &str, reason: &str) {
    match check(header, mirror) {
        Ok(report) => panic!("payload fault: drift was not detected\n{report}"),
        Err(e) => assert!(e.contains(reason), "fired, but not for {reason:?}:\n{e}"),
    }
}

// ── The gate (silence fixture: the real pair) ───────────────────────────────

#[test]
fn abi_rs_mirrors_the_generated_header() {
    let (header, mirror) = real_pair();
    match check(&header, &mirror) {
        Ok(report) => print!("{report}"),
        Err(e) => panic!("src/abi.rs does not mirror ee.h\n{e}"),
    }
}

// ── Harness controls ────────────────────────────────────────────────────────

#[test]
fn harness_lays_out_padding_like_c() {
    let header = "typedef struct {\n  uint8_t a;\n  uint32_t b;\n  uint16_t c;\n} ee_pad;\n\
                  #define EE_SIZEOF_ee_pad 12\n#define EE_OFFSET_ee_pad_a 0\n\
                  #define EE_OFFSET_ee_pad_b 4\n#define EE_OFFSET_ee_pad_c 8\n";
    let h = parse_header(header).expect("parse");
    let l = lay_out(&h.structs, "header").expect("lay out");
    let pad = &l["ee_pad"];
    let offsets: Vec<usize> = pad.fields.iter().map(|f| f.offset).collect();
    assert_eq!((offsets, pad.size, pad.align), (vec![0, 4, 8], 12, 4));
}

#[test]
fn harness_refuses_a_missing_header() {
    let nowhere = Path::new("/nonexistent/evil-weevil");
    let err = resolve_header(Some(nowhere), &workspace()).expect_err("no header there");
    assert!(err.starts_with("NO CHECK WAS PERFORMED"), "{err}");
}

// ── Firing fixtures: one per drift class ────────────────────────────────────

#[test]
fn firing_a_widened_field_moves_every_later_offset() {
    let (h, m) = real_pair();
    let m = edit(&m, "pub bearing_sin: u16,", "pub bearing_sin: u32,");
    fires(
        &h,
        &m,
        "ee_contact.distance offset 8 != EeContact.distance offset 12",
    );
}

#[test]
fn firing_reordered_fields_of_equal_width() {
    let (h, m) = real_pair();
    let m = edit(
        &m,
        "pub token: u8,\n    pub hesitate_left: u8,",
        "pub hesitate_left: u8,\n    pub token: u8,",
    );
    fires(&h, &m, "ee_mind.token vs EeMind.hesitate_left");
}

#[test]
fn firing_a_signedness_change_at_the_same_width() {
    let (h, m) = real_pair();
    let m = edit(&m, "pub distance: i32,", "pub distance: u32,");
    fires(&h, &m, "ee_contact.distance type");
}

#[test]
fn firing_a_dropped_field_hidden_by_tail_padding() {
    let (h, m) = real_pair();
    // 38 bytes round up to 40 either way, so only the field count catches it.
    let m = edit(&m, "    pub reserved1: u16,\n", "");
    fires(&h, &m, "ee_model_trace has 7 fields, EeModelTrace has 6");
}

#[test]
fn firing_a_struct_missing_from_the_mirror() {
    let (h, m) = real_pair();
    let m = edit(&m, "pub struct EeAgent {", "pub struct EeAgentRenamed {");
    fires(&h, &m, "header struct ee_agent is not mirrored as EeAgent");
}

#[test]
fn firing_an_unparsed_member_is_refused_not_skipped() {
    let (h, m) = real_pair();
    let m = edit(&m, "pub reserved1: u16,", "pub reserved1: [u8; 2],");
    fires(&h, &m, "unrecognised member");
}

#[test]
fn firing_a_header_define_that_disagrees_with_its_struct() {
    let (h, m) = real_pair();
    let h = edit(
        &h,
        "#define EE_OFFSET_ee_mind_flags 54\n",
        "#define EE_OFFSET_ee_mind_flags 55\n",
    );
    fires(&h, &m, "EE_OFFSET_ee_mind_flags is 55");
}

#[test]
fn firing_a_header_field_without_its_define() {
    let (h, m) = real_pair();
    let h = edit(&h, "#define EE_OFFSET_ee_intent_speed 28\n", "");
    fires(&h, &m, "no EE_OFFSET_ee_intent_speed");
}
