# SPDX-License-Identifier: MPL-2.0
# Nickel schema and runner policy paired with must.k9.ncl.
{
  component = "must.k9.ncl",
  input_path = "declaration",
  schema = {
    declaration | {
      schema_version | String,
      verb | String,
      authority | [| 'blocking |],
      source_of_truth | String,
      runner | String,
      clauses | Array {
        id | String,
        severity | String,
        description | String,
        probe | String,
        scope | String,
        evidence | String,
        ..
      },
      ..
    },
    ..
  },
  run = {
    mode = "repository-probes",
    adapter = "../../../scripts/must_check.sh",
    on_pass = "continue",
    on_critical_failure = "exit-nonzero",
    on_warning_failure = "continue-with-warning",
    report_format = "text",
    emit_summary = true,
  },
}
