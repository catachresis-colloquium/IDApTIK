# SPDX-License-Identifier: MPL-2.0
# Nickel schema and runner policy paired with dust.k9.ncl.
{
  component = "dust.k9.ncl",
  input_path = "declaration",
  schema = {
    declaration | {
      schema_version | String,
      verb | String,
      authority | [| 'advisory |],
      hygiene | Array {
        id | String,
        severity | String,
        description | String,
        tolerance | String,
        evidence | String,
        ..,
      },
      retirements | Array {
        id | String,
        status | String,
        disposition | String,
        evidence | String,
        ..,
      },
      ..,
    },
    ..,
  },
  run = {
    mode = "retirement-and-hygiene-report",
    on_pass = "continue",
    on_failure = "continue-with-warning",
    report_format = "text",
    emit_summary = true,
  },
}
