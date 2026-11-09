# A proven Creusot row needs a Verus row covering it (PROOF-POLICY 1.2).
# expect: has no Verus row covering it
let S = import "../proof-ledger-schema.ncl" in
let B = import "base.ncl" in
{ schema_version = "1", policy = B.policy, rows = [B.proven] } | S.Ledger
