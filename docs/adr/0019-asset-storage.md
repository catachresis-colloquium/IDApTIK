# ADR-0019: Binary assets are plain Git blobs in this repository, built from `source/`

- Status: Accepted
- Date: 2026-11-21
- Closes: DR-0003 (where binary game assets live, and Git LFS)
- Relates to: ADR-0008 (Bevy frontend), `docs/verification/2026-11-21-bevy-interface-ultraplan.md` (workstream D)

## Context

DR-0003 asked 3 linked questions: where binary assets live, Git blobs or Git
LFS, and whether the loadable `assets/` tree is committed or built at package
time. It said the question became live when the first real asset landed, and
that a ruling after that point can need a history rewrite.

That point passed on 2026-10-14 (`cd7c71f`). On 2026-11-21 the repository
tracks 92 binary files under `crates/idaptik-bevy/assets` (PNG sprites and
backdrops, and fonts), 345,425 bytes in total. The largest is
`fonts/VT323-Regular.ttf` at 153,116 bytes. No `filter=lfs` attribute exists,
and no `source/` tree exists.

The owner ruled on all 3 questions on 2026-11-21.

## Decision

1. Binary assets live in this repository, not in a separate content
   repository or object store.
2. They are ordinary Git blobs. The repository does not use Git LFS.
3. The loadable assets are built from an authored `source/` tree at package
   time. The authored files are committed.
4. No committed file is larger than 1 MiB (1,048,576 bytes).
   `scripts/asset_size_check.sh` enforces this in CI. A larger file needs a
   new ruling, not a larger number in the script.

## Consequences

- A plain `git clone` gives the complete content, with no LFS client.
- Every asset revision stays in the history for ever. The 1 MiB limit keeps
  that cost bounded.
- The 92 files in `crates/idaptik-bevy/assets` are hand-committed loadable
  files, and no `source/` tree or build step exists. They do not follow
  decision 3 yet. The move to `source/` and a package-time build is follow-up
  work (Bevy interface ultraplan, workstream D).
- The `assets/** linguist-generated` marking in `.gitattributes` matches
  decision 3 and stays.
