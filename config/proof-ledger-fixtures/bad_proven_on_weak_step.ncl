# A proven row may not rest on a step that is only argued.
# expect: depends on a step that is not proven
let S = import "../proof-ledger-schema.ncl" in
let B = import "base.ncl" in
{ schema_version = "1", policy = B.policy, rows = [B.proven & { depends_on = ["BR-ARGUED"] }, B.verus, { id = "BR-ARGUED", obligation = "o", kind = "engineering", verifier = "tests", status = "argued", where = "w", checker = "-", trust_boundary = "t" }] } | S.Ledger
