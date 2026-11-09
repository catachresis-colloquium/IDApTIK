# depends_on must name a row that exists.
# expect: names a row that does not exist
let S = import "../proof-ledger-schema.ncl" in
let B = import "base.ncl" in
{ schema_version = "1", policy = B.policy, rows = [B.proven & { depends_on = ["TE-NOWHERE"] }, B.verus] } | S.Ledger
