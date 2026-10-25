# SPDX-License-Identifier: MPL-2.0
# Nickel schema and runner policy paired with intend.k9.ncl.
{
  component = "intend.k9.ncl",
  input_path = "declaration",
  schema = {
    declaration | {
      schema_version | String,
      verb | String,
      authority | [| 'reporting |],
      north_star | String,
      anti_purpose | Array String,
      if_in_doubt | Array String,
      committed | Array {
        id | String,
        status | String,
        description | String,
        evidence | String,
        ..
      },
      horizon | Array {
        id | String,
        horizon | String,
        status | String,
        description | String,
        evidence | String,
        ..
      },
      ..
    },
    ..
  },
  run = {
    mode = "status-report",
    on_pass = "continue",
    on_failure = "continue-with-warning",
    report_format = "text",
    emit_summary = true,
  },
}
