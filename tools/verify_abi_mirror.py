#!/usr/bin/env python3
"""Prove `crates/idaptik-weevil/src/abi.rs` mirrors the generated `ee.h`.

The header is self-describing: it emits `EE_SIZEOF_*` / `EE_OFFSET_*` defines and
carries its own `_Static_assert`s. This script parses both the header and the
Rust mirror, lays each struct out with the platform's alignment rules, and
diffs field order, field offsets and total size.

It is deliberately independent of both compilers: if the Rust mirror drifted
from the header, this fails here rather than at a host's link step.

    python3 tools/verify_abi_mirror.py <ee.h> <abi.rs>
"""
import re
import sys

# Fundamental types -> (size, alignment) on x86-64 SysV.
C_TYPES = {
    "uint8_t": (1, 1), "int8_t": (1, 1), "char": (1, 1),
    "uint16_t": (2, 2), "int16_t": (2, 2),
    "uint32_t": (4, 4), "int32_t": (4, 4), "uint": (4, 4), "int": (4, 4),
    "uint64_t": (8, 8), "int64_t": (8, 8),
}

RUST_TYPES = {
    "u8": (1, 1), "i8": (1, 1),
    "u16": (2, 2), "i16": (2, 2),
    "u32": (4, 4), "i32": (4, 4),
    "u64": (8, 8), "i64": (8, 8),
    "bool": (1, 1), "f32": (4, 4), "f64": (8, 8),
}

C_TO_RUST = {
    "ee_version": "EeVersion", "ee_init_desc": "EeInitDesc",
    "ee_contact": "EeContact", "ee_snapshot": "EeSnapshot",
    "ee_agent": "EeAgent", "ee_intent": "EeIntent", "ee_context": "EeContext",
}


def align_up(value, alignment):
    return (value + alignment - 1) // alignment * alignment


def parse_structs(text, kinds, field_re):
    """Return {name: [(field, type), ...]} preserving source order."""
    structs = {}
    for m in re.finditer(r"#\[repr\(C\)\]\s*(?:#\[derive\([^\]]*\)\]\s*)*pub struct (\w+) \{(.*?)\}",
                         text, re.S):
        structs[m.group(1)] = [(f.group(1), f.group(2))
                               for f in re.finditer(field_re, m.group(2))]
    if not structs:  # C header
        for m in re.finditer(r"typedef struct \{(.*?)\}\s+(\w+)\s*;", text, re.S):
            structs[m.group(2).strip()] = [(f.group(2), f.group(1))
                                           for f in re.finditer(field_re, m.group(1))]
    return structs


def lay_out(structs, kinds):
    """Lay out every struct, iterating until nested sizes settle."""
    layouts = {}
    for _ in range(len(structs) + 1):
        changed = False
        for name, fields in structs.items():
            offset = 0
            max_align = 1
            rec = []
            for field, ftype in fields:
                if ftype in kinds:
                    size, align = kinds[ftype]
                elif ftype in layouts:
                    size, align = layouts[ftype].size, layouts[ftype].align
                else:
                    size, align = (8, 8)  # unknown; replaced on a later pass
                offset = align_up(offset, align)
                rec.append((field, offset, size))
                offset += size
                max_align = max(max_align, align)
            total = align_up(offset, max_align)
            prev = layouts.get(name)
            if prev is None or prev.fields != rec or prev.size != total or prev.align != max_align:
                layouts[name] = Layout(name, rec, total, max_align)
                changed = True
        if not changed:
            break
    return layouts


class Layout:
    def __init__(self, name, fields, size, align):
        self.name = name
        self.fields = fields  # (field, offset, size)
        self.size = size
        self.align = align

    def offset_of(self, field):
        for name, offset, _ in self.fields:
            if name == field:
                return offset
        return None


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    header = parse_structs(open(sys.argv[1]).read(), C_TYPES,
                           r"\s*(\w+)\s+(\w+)\s*;")
    rust = parse_structs(open(sys.argv[2]).read(), RUST_TYPES,
                         r"pub\s+(\w+)\s*:\s*(\w+),")

    # The fixed-point type: the header defines it as a signed 32-bit.
    header_text = open(sys.argv[1]).read()
    if re.search(r"typedef\s+int32_t\s+ee_fx\s*;", header_text):
        C_TYPES["ee_fx"] = (4, 4)
    hl = lay_out(header, C_TYPES)
    rl = lay_out(rust, RUST_TYPES)

    failures = 0
    for cname, rname in C_TO_RUST.items():
        c, r = hl.get(cname), rl.get(rname)
        bad = 0
        if c is None or r is None:
            print(f"FAIL {cname}/{rname}: missing on one side")
            failures += 1
            continue
        if c.size != r.size:
            print(f"FAIL {cname} size {c.size} != {rname} size {r.size}")
            bad += 1
        if len(c.fields) != len(r.fields):
            print(f"FAIL {cname} has {len(c.fields)} fields, {rname} has {len(r.fields)}")
            bad += 1
        for (cf, co, _), (rf, ro, _) in zip(c.fields, r.fields):
            if cf != rf:
                print(f"FAIL {cname}.{cf} vs {rname}.{rf}: field name/order drift")
                bad += 1
            elif co != ro:
                print(f"FAIL {cname}.{cf} offset {co} != {rname}.{rf} offset {ro}")
                bad += 1
        if bad:
            failures += bad
        else:
            print(f"ok   {cname:16s} size={c.size:4d} fields={len(c.fields):2d}")

    if failures:
        print(f"\n{failures} mismatch(es)")
        return 1
    print("\nABI mirror matches the generated header on every field, offset and size")
    return 0


if __name__ == "__main__":
    sys.exit(main())
