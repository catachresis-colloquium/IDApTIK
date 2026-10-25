# SPDX-License-Identifier: MPL-2.0
# Nickel schema and runner policy paired with trust.k9.ncl.
{
  component = "trust.k9.ncl",
  input_path = "declaration",
  schema = {
    declaration | {
      schema_version | String,
      verb | String,
      authority | [| 'blocking |],
      current_trust_level | String,
      trust_levels | Array String,
      permissions | Array String,
      approval_required | Array String,
      clauses | Array {
        id | String,
        severity | String,
        description | String,
        scope | String,
        evidence | String,
        ..
      },
      ..
    },
    ..
  },
  run = {
    mode = "integrity-and-provenance-review",
    on_pass = "continue",
    on_critical_failure = "exit-nonzero",
    report_format = "text",
    emit_summary = true,
  },
}
