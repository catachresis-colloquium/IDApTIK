# A proven row may not rest on a step that is only proven with postulates.
# expect: a proven row depends on a step that is not proven
let S = import "../proof-ledger-schema.ncl" in
let B = import "base.ncl" in
{ schema_version = "1", policy = B.policy, rows = [B.proven & { depends_on = ["CR-AXIOM"] }, B.verus & { covers | force = ["CR-ONE", "CR-AXIOM"] }, B.proven & { id | force = "CR-AXIOM", status | force = "proven-with-postulates" }] } | S.Ledger
