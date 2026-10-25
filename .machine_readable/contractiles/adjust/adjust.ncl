# SPDX-License-Identifier: MPL-2.0
# Nickel schema and runner policy paired with adjust.k9.ncl.
{
  component = "adjust.k9.ncl",
  input_path = "declaration",
  schema = {
    declaration | {
      schema_version | String,
      verb | String,
      authority | [| 'advisory |],
      clauses | Array {
        id | String,
        severity | String,
        description | String,
        tolerance | String,
        corrective | String,
        evidence | String,
        ..
      },
      ..
    },
    ..
  },
  run = {
    mode = "advisory-drift-review",
    on_pass = "continue",
    on_failure = "continue-with-warning",
    report_format = "text",
    emit_summary = true,
  },
}
