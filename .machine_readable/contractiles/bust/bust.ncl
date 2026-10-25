# SPDX-License-Identifier: MPL-2.0
# Nickel schema and runner policy paired with bust.k9.ncl.
{
  component = "bust.k9.ncl",
  input_path = "declaration",
  schema = {
    declaration | {
      schema_version | String,
      verb | String,
      authority | [| 'alarm |],
      drills | Array {
        id | String,
        injection | String,
        expected | String,
        recovery | String,
        evidence | String,
        ..,
      },
      ..,
    },
    ..,
  },
  run = {
    mode = "on-demand-failure-drills",
    on_pass = "continue",
    on_failure = "exit-nonzero",
    report_format = "text",
    emit_summary = true,
  },
}
