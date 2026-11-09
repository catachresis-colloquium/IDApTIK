# Contracts for the proof ledger (config/proof-ledger.ncl).
#
# The ledger is the repository's machine-readable proof record: one row per
# obligation the repository speaks about, in the closed status vocabulary of
# the estate proof policy (metadatastician/standards, 3-practice/
# PROOF-POLICY.adoc, read at 9bdf864cc7f2c8e24b4df84bf00d04158eee3ad5; its §5,
# and README-EXPLAINME-STANDARD's proof ledger). It is canonical: the
# rendered docs/status/PROOF-STATUS.adoc is derived from it, and when they
# disagree the ledger wins (README-EXPLAINME §5, rule 1).
#
# What this schema makes a typecheck failure rather than a review comment:
#
#   - a status word outside the closed set (`proven`, `proven-with-postulates`,
#     `tested-only`, `argued`, `open`);
#   - a `proven` or `proven-with-postulates` row without a receipt (the CI job
#     that checks it, and the run and commit it was last seen passing on);
#   - an `open` row without a next action: an unrun check is never silent;
#   - a duplicate id, or a `depends_on` / `covers` naming no row;
#   - a `proven` row resting on a step that is not itself `proven` (a
#     `proven-with-postulates` step included), or a `proven-with-postulates`
#     row resting on anything weaker: a row is only as strong as the weakest
#     step it depends on (README-EXPLAINME §5);
#   - a Creusot row that no Verus row covers: PROOF-POLICY §1.2 makes both
#     verifiers the default for first-party Rust, so each Creusot obligation
#     has a planned Verus leg, `open` with its reason until it runs;
#   - a Proof Witness Grade on an engineering row: PWG is for theory theorems
#     only (§8.1), and this ledger records `pwg` nowhere else.
#
# Records are closed: an unknown field is rejected, which catches typos.

let statuses = ["proven", "proven-with-postulates", "tested-only", "argued", "open"] in
let verifiers = ["creusot", "verus", "idris2", "tests", "nickel"] in
let kinds = ["engineering", "theory"] in
let strong = ["proven", "proven-with-postulates"] in

let one_of = fun options =>
  std.contract.from_predicate (fun v => std.is_string v && std.array.elem v options)
in

let has_duplicate = fun ids =>
  let sorted = std.array.sort (fun a b => std.string.compare a b) ids in
  let n = std.array.length sorted in
  if n < 2 then
    false
  else
    std.array.generate (fun i => std.array.at i sorted == std.array.at (i + 1) sorted) (n - 1)
    |> std.array.any (fun d => d)
in

let Id =
  std.contract.from_predicate (fun v =>
    std.is_string v && std.string.is_match "^[A-Z]{2}-[A-Z0-9]+(-[A-Z0-9]+)*$" v
  )
in

let Sha =
  std.contract.from_predicate (fun v =>
    std.is_string v && std.string.is_match "^[0-9a-f]{40}$" v
  )
in

let Receipt = {
  workflow | String,
  job | String,
  run | Number,
  commit | Sha,
}
in

let RowShape = {
  id | Id,
  obligation | String,
  kind | one_of kinds,
  verifier | one_of verifiers,
  status | one_of statuses,
  where | String,
  checker | String,
  trust_boundary | String,
  receipt | Receipt | optional,
  depends_on | Array Id | default = [],
  covers | Array Id | default = [],
  next_action | String | optional,
}
in

let check = fun ok msg =>
  if ok then 'Ok else 'Error { message = msg }
in

let Row =
  std.contract.Sequence [
    RowShape,
    std.contract.from_validator (fun r =>
      check
        (!(std.array.elem r.status strong) || std.record.has_field "receipt" r)
        "%{r.id}: a %{r.status} row needs a receipt"
    ),
    std.contract.from_validator (fun r =>
      check
        (r.status != "open" || std.record.has_field "next_action" r)
        "%{r.id}: an open row needs a next_action"
    ),
    std.contract.from_validator (fun r =>
      check
        (std.array.length r.covers == 0 || r.verifier == "verus")
        "%{r.id}: only a verus row may cover other rows"
    ),
  ]
in

let ids_of = fun rows => std.array.map (fun r => r.id) rows in
let status_of = fun rows => fun id =>
  (std.array.filter (fun r => r.id == id) rows |> std.array.first).status
in

# The first row failing `ok`, by id, or "" when every row passes.
let first_failing = fun ok => fun rows =>
  let bad = std.array.filter (fun r => !(ok r)) rows in
  if std.array.length bad == 0 then "" else (std.array.first bad).id
in

let all_rows = fun ok => fun message => fun rows =>
  let id = first_failing ok rows in
  check (id == "") "%{id}: %{message}"
in

let Rows =
  std.contract.Sequence [
    Array Row,
    std.contract.from_validator (fun rows =>
      check (!(has_duplicate (ids_of rows))) "two rows share an id"
    ),
    std.contract.from_validator (fun rows =>
      let ids = ids_of rows in
      all_rows
        (fun r => std.array.all (fun d => std.array.elem d ids) (r.depends_on @ r.covers))
        "depends_on or covers names a row that does not exist"
        rows
    ),
    std.contract.from_validator (fun rows =>
      all_rows
        (fun r =>
          # `proven` rests only on `proven`; `proven-with-postulates` may also
          # rest on another postulated step, whose axioms it then inherits.
          let allowed = if r.status == "proven" then ["proven"] else strong in
          !(std.array.elem r.status strong)
          || std.array.all (fun d => std.array.elem (status_of rows d) allowed) r.depends_on
        )
        "a proven row depends on a step that is not proven (README-EXPLAINME §5: a row is only as strong as its weakest step)"
        rows
    ),
    # PROOF-POLICY §1.2: every Creusot obligation claimed as proven has a
    # planned Verus leg. Debt rows (a Creusot row that proves nothing yet)
    # owe a Creusot proof first.
    std.contract.from_validator (fun rows =>
      let covered =
        std.array.flat_map (fun r => if r.verifier == "verus" then r.covers else []) rows
      in
      all_rows
        (fun r =>
          r.verifier != "creusot"
          || !(std.array.elem r.status strong)
          || std.array.elem r.id covered
        )
        "a proven Creusot row has no Verus row covering it (PROOF-POLICY §1.2)"
        rows
    ),
  ]
in

{
  Ledger
    | doc "The whole proof ledger."
    = {
      schema_version | String,
      policy
        | doc "The PROOF-POLICY revision the rows were written against."
        = {
          repository | String,
          path | String,
          commit | Sha,
        },
      rows | Rows,
    },
}
