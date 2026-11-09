# Two rows may not share an id.
# expect: two rows share an id
let S = import "../proof-ledger-schema.ncl" in
let B = import "base.ncl" in
{ schema_version = "1", policy = B.policy, rows = [B.proven, B.verus, B.verus] } | S.Ledger
