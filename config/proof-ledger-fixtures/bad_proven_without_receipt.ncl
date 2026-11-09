# A proven row must name the CI job and run that checked it.
# expect: a proven row needs a receipt
let S = import "../proof-ledger-schema.ncl" in
let B = import "base.ncl" in
{ schema_version = "1", policy = B.policy, rows = [(std.record.remove "receipt" B.proven), B.verus] } | S.Ledger
