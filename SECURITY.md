# Security policy

## Supported versions

TaskAttest is pre-1.0. Security fixes are applied to the latest tagged release.
Older pre-1.0 releases are unsupported after a newer release is available.

| Version | Supported |
| --- | --- |
| Latest tagged release | Yes |
| Older pre-1.0 releases | No |
| Unreleased development builds | Best effort |

## Reporting a vulnerability

Use
[GitHub private vulnerability reporting](https://github.com/yhay81/taskattest/security/advisories/new).
Do not open a public issue for command execution, path traversal, environment
leakage, process cleanup, resource-limit bypass, no-clobber, source-identity,
log-store, or receipt-integrity vulnerabilities.

Include the TaskAttest version, operating system, repository shape with private
content removed, command, structured error or receipt, and a minimal synthetic
reproduction. Do not attach real logs: complete check logs may contain secrets.

An acknowledgement is targeted within 7 days. The maintainer will validate the
report, coordinate disclosure, add a regression test, and publish a GitHub
Security Advisory when appropriate. Targets are goals, not a service-level
agreement for this volunteer project.

## Security boundaries

- TaskAttest executes repository-defined code. Use it only for repositories
  whose code and configuration you are prepared to run.
- TaskAttest spawns argument arrays and never interpolates a shell command.
  npm, pnpm, Yarn, and Bun may themselves invoke package scripts through a
  platform shell.
- Environment inheritance is minimized, but explicitly forwarded variables are
  available to child processes.
- Runtime and log budgets are enforced, and Unix process groups or Windows
  process-tree termination are used on cancellation. They are resource
  controls, not an operating-system sandbox.
- Network and filesystem access are not sandboxed.
- Full log blobs are unredacted and `potentially_sensitive`. Summary redaction
  is best-effort and must not be treated as a data-loss-prevention control.
- Receipt digests detect mutation. They do not authenticate an author, attest a
  reproducible environment, or prove that discovery was sufficient.

## Release and dependency policy

Dependabot monitors Rust and GitHub Actions dependencies. CI checks
`Cargo.lock` against RustSec advisories. Tagged releases are built only by the
release workflow and include checksums, SBOMs, and GitHub/Sigstore
attestations. See [RELEASING.md](RELEASING.md).
