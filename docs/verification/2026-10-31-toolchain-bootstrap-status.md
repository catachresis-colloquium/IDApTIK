<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->
# Verification tooling bootstrap status

Date: 2026-10-31. Project baseline:
`dfe0f7b4591c1fc4f96bdaa762f39995ee3c9b57`.

## Completed

- Confirmed x86-64 Linux, GCC/make/git/gh/curl present, passwordless sudo
  available, approximately 20 GB disk free.
- Retrieved the pons-asinorum source to the sandbox tool cache, outside the
  project's tracked tree, and checked out exactly
  `84f58f910de7020b79450f2c2643599fe7760f73`.
  The cache is disposable and not guaranteed to persist between sessions.
- Inspected the scanner's toolchain and package manifest: Rust 1.97.1,
  package `pons-cli`, binary `pons`.
- Located an existing successful Creusot workflow for the **exact baseline**:
  https://github.com/catachresis-colloquium/IDApTIK/actions/runs/40521243424
  GitHub's job/step API reports successful translation, proof, planted-mutant
  rejection, restored-tree proof and kernel tests. This is historical CI metadata,
  not a fresh local replay or an inspection of the detailed prover logs.
- Confirmed that run advertises a nonexpired `creusot-proof-evidence` artifact
  (32,990 bytes). Its content has not been inspected.

## Installation blockers observed

- `rustc`, `cargo`, `pons`, `just`, `mise`, and `opam` are not on PATH.
- HTTPS to `static.rust-lang.org` and `deb.debian.org` failed immediately with
  `SSL_ERROR_SYSCALL`. These hosts are outside the sandbox egress allowlist.
- `gh run view ... --log` could not retrieve the historical logs because the
  GitHub log redirect targets `results-receiver.actions.githubusercontent.com`,
  which is also outside the allowlist. Do not treat an empty local log file as
  proof output.
- No scanner release binaries were available in the inspected upstream release
  listing. Recent upstream artifacts were reports, not a scanner executable.

No tools have been falsely marked installed; no compilation, pons execution or
new Creusot proof has occurred. Sudo permission is not the obstacle. Do not
attempt an unreviewed third-party compiler mirror to bypass distribution access.

## Preferred unblocking options

1. Provision the tools in this sandbox, or enable standard distribution access.
   Rust installation/build needs the official Rust distribution and crates
   registry/download hosts. Creusot additionally needs system packages and the
   opam/compiler/prover sources used by the existing workflow. Follow that
   workflow rather than assuming a small host list is sufficient for all opam
   transitive downloads.
2. Alternatively, run the work on GitHub Actions, where this project's pinned
   Creusot workflow already operates successfully. Add a separate pinned pons
   build/test/scan job, push only the session branch, and use a pull request to
   trigger the existing proof jobs. No credentials need to be supplied in chat.
   This requires publishing branch changes/starting CI; it does not require
   merging them. Returning artifacts to the sandbox may still require allowing
   GitHub's artifact storage hosts, or a small durable textual evidence summary
   made available through the GitHub API.

Once provisioned, build and test pons with the upstream lockfile, execute the
complete-inventory scans, and reproduce the kernel proof controls before any
migration. Until then, keep the executable-scan and local-reproduction gates
blocked in the ultraplan.
