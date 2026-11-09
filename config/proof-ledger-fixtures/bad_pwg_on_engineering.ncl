# Proof Witness Grades are for theory theorems only; rows are closed records.
# expect: extra field `pwg`
let S = import "../proof-ledger-schema.ncl" in
let B = import "base.ncl" in
{ schema_version = "1", policy = B.policy, rows = [B.proven & { pwg = "3" }, B.verus] } | S.Ledger
