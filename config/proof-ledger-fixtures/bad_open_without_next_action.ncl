# An open row must say what happens next.
# expect: an open row needs a next_action
let S = import "../proof-ledger-schema.ncl" in
let B = import "base.ncl" in
{ schema_version = "1", policy = B.policy, rows = [B.proven, std.record.remove "next_action" B.verus] } | S.Ledger
