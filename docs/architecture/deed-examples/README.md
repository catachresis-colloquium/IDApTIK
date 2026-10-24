# DEED examples for the building infrastructure design

These are **examples** of the DEED metadata/policy surface described in
[../building-infrastructure-design.md](../building-infrastructure-design.md)
§10. They are *not* live repository metadata: the estate's a2ml→deed
conversion (standards#837) has not yet reached this repository, and live
deed files land in their conventional root/`.machine_readable` locations at
design stage 5.

## What is here

- `idaptik-building-authoring_praxis.deed` — a `praxis-deed` (filename
  dispatch `*_praxis.deed`): the building authoring and validation policy,
  as prioritised rules. Lint-clean against the estate's
  `deed_lint.py` (normative grammar `deed.abnf`,
  `hyperpolymath/standards` `1-formats/deed/spec/abnf/deed.abnf`).

## What is only sketched in the design doc

The `IDApTIK_chora.deed` repo-deed example in the design doc uses the `#u8`
repo-uuid form, which the normative ABNF v1.2.0 accepts but the currently
bundled linter (ABNF v1.0.0) does not parse yet. It is deliberately **not**
committed here as a file: it will land as part of the estate migration,
validated by whichever linter version the estate pins at that time.

## Checking

```
python3 <path-to>/hyperpolymath/standards/1-formats/deed/tools/deed_lint.py \
  idaptik-building-authoring_praxis.deed
```
