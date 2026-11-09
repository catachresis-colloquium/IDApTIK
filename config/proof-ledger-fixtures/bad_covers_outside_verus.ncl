# Only a Verus row may cover other rows.
# expect: only a verus row may cover other rows
let S = import "../proof-ledger-schema.ncl" in
let B = import "base.ncl" in
{ schema_version = "1", policy = B.policy, rows = [B.proven & { covers = ["VE-ONE"] }, B.verus] } | S.Ledger
