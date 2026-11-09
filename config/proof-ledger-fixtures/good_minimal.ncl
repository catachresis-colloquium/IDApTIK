# A proven Creusot row with its planned Verus leg: the smallest valid ledger.
let S = import "../proof-ledger-schema.ncl" in
let B = import "base.ncl" in
{ schema_version = "1", policy = B.policy, rows = [B.proven, B.verus] } | S.Ledger
