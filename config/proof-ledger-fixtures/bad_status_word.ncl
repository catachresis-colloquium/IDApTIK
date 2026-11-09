# "verified" is not in the closed status vocabulary.
# expect: contract broken by the value of `status`
let S = import "../proof-ledger-schema.ncl" in
let B = import "base.ncl" in
{ schema_version = "1", policy = B.policy, rows = [B.proven & { status | force = "verified" }, B.verus] } | S.Ledger
