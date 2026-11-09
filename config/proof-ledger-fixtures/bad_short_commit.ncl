# A receipt commit is a full 40-character SHA, not an abbreviation.
# expect: contract broken by the value of `commit`
let S = import "../proof-ledger-schema.ncl" in
let B = import "base.ncl" in
{ schema_version = "1", policy = B.policy, rows = [B.proven & { receipt | force = B.receipt & { commit | force = "a62cc9f" } }, B.verus] } | S.Ledger
