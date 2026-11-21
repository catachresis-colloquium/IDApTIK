# Security Policy

## Reporting a vulnerability

Do not report vulnerabilities through public issues, pull requests,
discussions, or social media.

Use GitHub's private
[security-advisory form](https://github.com/catachresis-colloquium/IDApTIK/security/advisories/new).
If that is unavailable, email
[developer@joshuajewell.dev](mailto:developer@joshuajewell.dev) with the
affected component, reproduction steps, potential impact, and any suggested
remediation.

The same channels are published as RFC 9116 metadata at
[`/.well-known/security.txt`](https://idaptik.net/.well-known/security.txt),
which is the file a researcher's tooling will find first. It names this address
too. That agreement is enforced, not assumed:
`tests/ci_security_config_test.sh` compares the contact sets named here, in
`CODE_OF_CONDUCT.md`, in `.well-known/security.txt`, and in
`.well-known/humans.txt`, and fails the build if they differ.

Until 2026-09-27 they did differ. `security.txt` carried a personal gmail
address while this file and `CODE_OF_CONDUCT.md` carried the role address above,
and neither document mentioned the other — so the RFC 9116 path and the policy
path routed reports to different inboxes with no way to tell from inside the
repository which one was monitored. The role address won because it appears in
more places, it belongs to a project-owned domain rather than a personal
mailbox, and it survives a change of maintainer. **Which inbox is actually
monitored was not verifiable from here** and is recorded as an open owner
ruling in `docs/decisions-pending/DR-0002-security-contact.adoc`. If the answer is
the gmail, change all four files in one commit; the check will reject a partial
edit.

## Scope

Security reports may cover the repository's code, dependencies, build and
deployment configuration, and published releases. The most sensitive areas
are:

- `crates/idaptik-ffi`: the unsafe C-ABI boundary used by non-Rust consumers;
- `crates/idaptik-net`: the network-facing client over burble game-session fabric;
- `.github/workflows/`: privileged automation and supply-chain configuration;
- snapshot, package, and multiplayer wire validation at trust boundaries.

Note: the session relay itself now lives in the burble repository
(`metadatastician/burble/server/lib/burble_web/channels/game_channel.ex`).

Do not perform denial-of-service testing, social engineering, or testing
against infrastructure or accounts you do not own.

## Response and disclosure

IDApTIK is currently maintained by one maintainer, so response times are
best-effort. Reports will be acknowledged privately, triaged, remediated, and
disclosed through a coordinated GitHub Security Advisory where appropriate.
No independent security audit has yet been performed; current evidence and
limitations are recorded in `docs/PROJECT-ASSURANCE-PROFILE.adoc`.
