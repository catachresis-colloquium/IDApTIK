# The secret gate: first-party, history-complete, content-addressed

Status: **Delivered** (engine, control suite, workflow) / **Observed** (numbers
below, measured in this repository) / **Unverified** (anything requiring the
real `gitleaks` binary, which this environment cannot download).

## The failure being fixed

The branch protection rule on `main` requires a status check named
`scan / gitleaks`. That context was produced by
`metadatastician/standards/.github/workflows/secret-scanner-reusable.yml`,
called from `.github/workflows/secret-scanner.yml`.

That caller was added by #212 on 2026-10-31 and **never started once**. Every
run reports `startup_failure`: no job runs, and no check run is created at all.
`gh pr checks` shows nothing, `gh pr view` waits forever, and every pull request
in the repository is unmergeable — including ones whose own changes are
unrelated.

What is *observed*:

- The caller file and its pin are byte-identical to the version that last
  succeeded on 2026-10-27 (`dfe0f7b`, `ce6fdbf`).
- The pinned reusable revision (`01c9b0c9`) is the already-repaired
  least-privilege one. It declares `permissions: contents: read` per job,
  consumes only `actions/checkout@3d3c42e5`, and every ref it uses is declared
  in this repository's `actions.lock` — verified by extraction, not assumed.
- The workflow's own header says reusable calls do not follow repository-owner
  redirects, and `standards` moved owner (`hyperpolymath` → `metadatistician`)
  on 2026-10-29, between the last success and the first failure.
- A sibling repository (`catachresis-colloquium/enaction-engine`) fails with the
  same shape.

What is *inferred, not proven*: the org's Actions policy rejects reusable
workflows from another owner, or an allowlist does not name it. The org settings
API returns 403 for this repository's token, so this cannot be settled from
here. It does not need to be: a **required** gate must not depend on another
repository's settings page at all, which is why the fix is local.

## What replaced it

`.github/workflows/secret-scanner.yml` now runs gitleaks itself, in a job whose
display name is literally `scan / gitleaks`, so the existing protection rule
keeps matching and no ruleset edit is needed. The binary is the same pinned,
checksum-verified 8.18.4 release the estate reusable used, so the ruleset — and
therefore every finding — is unchanged by the migration.

The scan itself is `scripts/secret_scan.sh`.

### It scans history, not the working tree

The reusable ran `gitleaks detect --source .`: a working-tree scan. A secret
committed in March and deleted in April is invisible to it, and that is the
common shape of a real leak — the string is gone, so nobody remembers to rotate
it. The new gate enumerates every blob reachable from any fetched ref
(`git rev-list --objects --all`), resolves object types in a single
`cat-file --batch-check` pass, and scans each distinct `(blob, path)` pair.
Deleting a file does not delete the blob, so a deleted secret stays findable.

### It does not re-scan bytes that have not changed

A blob is a content hash, and its verdict depends only on its bytes, its path,
and the scanner. `scripts/lib/content-cache.sh` keeps a ledger of
`blob-sha <TAB> path` pairs already proven clean, under an **epoch** built from
the scanner binary digest, the ruleset digest, and a format literal. Only pairs
absent from the ledger are materialised and scanned.

The epoch is the load-bearing part: a cache that survives a change of checker
turns "we proved this" into "we assumed this". Editing `.gitleaks.toml` or
replacing the binary invalidates the entire ledger by construction, and the
control suite asserts exactly that.

Findings are never recorded as clean, so a red run stays red until the secret is
rotated. A corrupt or unreadable ledger is a full rescan, never a pass. A
missing scanner or missing ruleset is exit 2, never a skip.

### Measured

This repository, 706 blobs in history, 2 vCPU, warm page cache, scanner that
does nothing (so the numbers are the engine's, not a scanner's):

| run | wall |
|-----|------|
| cold — enumerate, materialise 706 blobs, invoke the scanner once | 3.38 s |
| warm — enumerate, partition, invoke the scanner **not at all** | 0.10 s |

With `tests/fixtures/fake-gitleaks.sh` in place of that null scanner the cold
pass took 4 m 02 s. That is not a scanner benchmark — the stub is a bash loop
spawning `grep` per line — but it shows the shape that matters: the stub's cost
is proportional to the number of files scanned, and the cache removes that term
entirely. On a push touching a handful of files the real binary scans a handful
of files.

The cold number is also the *whole history of this repository*, which is a
one-time cost per cache epoch, not a per-push one.

First real run in CI, which is the only place the real binary exists
(`d279555`, cold cache, 1773 blobs -- the runner's full history is bigger than
this sandbox's clone):

| step | wall |
|------|------|
| control, 12 cases against real gitleaks | 1 s |
| scan of 1773 blobs | 14 s |

and it found exactly one secret: `aws-access-token` in
`tests/secret_scan_self_test.sh:107` -- the control's own planted test vector,
committed as a literal. The gate was working correctly on its own fixture.

That is fixed in two moves, and the distinction matters:

- the control now **assembles** the value at run time from two halves, so the
  literal never enters the source again;
- the one historical occurrence is allowlisted by **exact value**
  (`^AKIAZ7Q2XK9MPL4RTUVW$`), not by pattern. It cannot blind the scanner to
  anything else, and the control now plants a different value precisely because
  a control that plants an allowlisted string proves nothing.

## The control suite

`tests/secret_scan_self_test.sh` (12 cases, offline, ~2 s) is what makes the
above a gate rather than a claim. It builds a throwaway repository and requires
the verdict to change under mutation:

1. a clean history passes; 2. the pass reports a non-zero denominator; 3. a
planted secret fails; 4. **deleting the file does not launder it** — the blob is
still in history; 5. a warm cache rescans only the blob that still fails;
6. adding one file scans exactly one extra blob; 7. a rules change rescans
everything; 8. a corrupt cache still fails; 9. a missing scanner is a hard
failure; 10. a missing ruleset is a hard failure; 11. a finding whose path the
ledger cannot attribute is refused rather than recorded clean; 12. rewriting the
secret out of history restores a pass.

Two of those cases exist because the control caught real defects that no amount
of local reasoning had:

- gitleaks 8.18.4 writes its JSON report only when it has findings. The engine
  required the file, so every clean history read as a broken gate.
- the report's `File` is the path the scanner was handed, not a repo-relative
  path. Comparing it to the ledger key without normalising matched nothing --
  and "nothing matched" meant every finding was recorded as CLEAN, a false green
  produced by a path-format difference. The engine now normalises the prefix and
  refuses to give a verdict it cannot attribute. The offline fixture had hidden
  this by emitting tidier paths than the real binary does; it now emits the path
  it was given, like the original.

It runs in `just fixture-tests`, in the `fixtures-and-must` CI job, and as a
step *inside* the secret workflow before the real scan.

## What this does not do

- **The estate's bespoke `rust-secrets` and `shell-secrets` jobs are not
  reproduced.** They were grep-shaped checks for `*_TOKEN=`-style literals in
  `*.rs` and `*.sh`. This gate scans every file type in history with gitleaks'
  full rule set, which is broader in file coverage and different in rule
  coverage; it is not a superset of those two bespoke checks. Reproducing them
  as extra gitleaks rules would need measuring against the real binary, which
  cannot be done here, and an unmeasured rule that fires on this repository's
  own fixtures would turn the required gate red. Left as a follow-up with that
  requirement stated.
- **`.gitleaks.toml` allowlists are otherwise untouched.** Every pre-existing
  entry was measured against a real binary (the file says so) and none was
  changed. Two edits were made and both are documented in place: one comment
  that described the reusable's staging step (which no longer exists) now says
  so, and one entry was **added** for the single historical test vector above,
  anchored to the whole value. Adding it *was* measured -- the CI run is the
  measurement: the same tree went from one finding to none.
- **The real scanner has not been executed here.** Release downloads are blocked
  in this environment (the same reason the site's `yq` had to be shimmed). The
  engine is exercised against a binary that speaks the same CLI and report
  format; the real one runs in CI, where the control must pass before the scan
  is even attempted.

## Why this is worth copying to the other gates

Nothing in `scripts/lib/content-cache.sh` is secret-specific. It takes a list of
work items, an epoch digest, and a ledger directory, and hands back the items
that need work. Any gate whose verdict for an item depends only on that item's
bytes plus a fixed checker can use it:

- the documentation-claims gate keys on document digests and the workflow set;
- the fixture suites key on the scripts under test and their fixtures;
- the Nickel scenario check keys on the `.ncl` inputs and the validator build.

The invariant to keep is the one this document keeps repeating: **the epoch must
cover everything that can change the answer** — toolchain, rules, and the gate's
own code. A cache is a claim about the past; the epoch is how the gate knows it
is still talking about the same past.

## Reproducing the evidence

```
just secret-scan-test                 # 11 control cases, offline
just secret-scan                      # real scan, needs gitleaks on PATH
bash tests/secret_scan_self_test.sh --scanner "$(command -v gitleaks)"
```
