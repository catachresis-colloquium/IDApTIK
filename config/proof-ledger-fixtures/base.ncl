# Shared pieces for the proof-ledger fixtures. Not a fixture itself: `just
# proof-ledger-check` exports good_*.ncl and expects every bad_*.ncl to be
# rejected by proof-ledger-schema.ncl.
let receipt_value = {
  workflow = "ci.yml",
  job = "proof",
  run = 1,
  commit = "a62cc9f9949f556f751597e71be55f18512a0287",
} in
{
  policy = {
    repository = "metadatastician/standards",
    path = "3-practice/PROOF-POLICY.adoc",
    commit = "9bdf864cc7f2c8e24b4df84bf00d04158eee3ad5",
  },
  receipt = receipt_value,
  proven = {
    id = "CR-ONE",
    obligation = "o",
    kind = "engineering",
    verifier = "creusot",
    status = "proven",
    where = "w",
    checker = "c",
    trust_boundary = "t",
    receipt = receipt_value,
  },
  verus = {
    id = "VE-ONE",
    obligation = "o",
    kind = "engineering",
    verifier = "verus",
    status = "open",
    where = "w",
    checker = "c",
    trust_boundary = "t",
    covers = ["CR-ONE"],
    next_action = "n",
  },
}
