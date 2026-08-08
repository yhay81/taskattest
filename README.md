# TaskAttest

Evidence-backed verification receipts for software changes.

> Status: 0.3 release. The end-to-end local workflow is implemented
> and covered by unit and CLI tests on Linux, macOS, and Windows.

TaskAttest discovers the checks a repository already trusts, explains why each
check is selected, executes argument vectors under explicit resource limits,
and stores a portable receipt bound to the exact Git workspace state.

```bash
taskattest discover --format json
taskattest run --changed --format json
taskattest receipt show rcpt_0123... --format json
taskattest verify rcpt_0123... --format json
```

## Why

“Tests passed” is not durable evidence. The source may have changed, the wrong
command may have run, relevant CI checks may have been omitted, or retained
logs may no longer match the summary.

TaskAttest records:

- the commit, ref, dirty-state digest, and full tracked/untracked workspace
  digest before and after execution;
- discovered checks, their configuration sources, selection reasons, and
  coverage gaps;
- normalized command argument vectors, working directories, tool identities,
  environment policy, timing, and outcomes;
- bounded redacted summaries plus complete local logs addressed by SHA-256;
- a canonical receipt digest and an ID derived from that digest.

`verify` checks receipt semantics, the canonical digest, the derived ID, and
every referenced local log without rerunning or accessing the network.

The checked-in
[receipt compatibility corpus](tests/fixtures/contracts/README.md) freezes the
published v1 receipt shape and digest derivation. CI checks exact round trips,
offline verification, byte-level fixture digests, and declared reader and
semantic tampering cases on every supported operating system.

The versioned
[discovery accuracy corpus](tests/fixtures/discovery/README.md) contains 100
hand-labeled projects: 20 single-package fixtures for each initial ecosystem,
10 mixed projects, and 10 explicitly configured monorepos. Its independent
evaluator publishes overall and per-ecosystem TP/FP/FN, enforces at least 95%
precision and 90% recall, checks exact coverage gaps, and uses tripwires to
prove discovery did not execute repository commands. The pinned v0.1 result is
100% precision and 100% recall over 262 expected checks.

Performance observations use a generated 10,000-file, 100-check Git workspace
and a complete 100-check receipt. The
[benchmark methodology](benchmarks/README.md) documents fixture construction,
measurement boundaries, raw artifacts, and the distinction between the current
baseline and future v1.0 regression thresholds.

## Supported discovery

TaskAttest 0.1 supports Git workspaces and common checks from:

- JavaScript/TypeScript `package.json` scripts, with npm, pnpm, Yarn, or Bun
  selected from the lockfile;
- Python `pyproject.toml`, requirements files, and `tox.ini`, including
  declared pytest, Ruff, mypy, and tox checks;
- Rust `Cargo.toml` and `Cargo.lock`;
- Go `go.mod` and `go.sum`;
- safe single-command GitHub Actions `run` steps;
- explicit checked-in `.taskattest.toml` checks.

Interactive, watch, server, update, and auto-fix JavaScript scripts are not
selected automatically. Shell expressions and dynamic GitHub Actions steps are
reported as coverage gaps unless an explicit argument-vector replacement is
declared.

## Install

Download a native archive from
[GitHub Releases](https://github.com/yhay81/taskattest/releases), or install
from a source checkout with Rust 1.85 or newer:

```bash
cargo install --path . --locked
```

See [INSTALL.md](INSTALL.md) for platform-specific, checksum- and
provenance-verified native installation, updating, and removal.

Generate completion scripts with `taskattest completions bash` (also `zsh`,
`fish`, `powershell`, and `elvish`).

## Try it

Start with read-only discovery:

```bash
taskattest discover --changed --format json
```

Run all discovered checks and write an additional standalone receipt:

```bash
taskattest run \
  --receipt-out verification-receipt.json \
  --format json
```

By default, receipts and logs are stored under
`<git-dir>/taskattest/{receipts,blobs}` so they do not alter the workspace.
`--state-dir` selects another local store. Existing receipt output files and
content-addressed evidence are never overwritten. Detectable
`--receipt-out` failures are rejected before checks start. If the destination
changes after preflight, TaskAttest returns exit 7, writes the durable receipt
to stdout, and emits a `taskattest.receipt-recovery.v1` document on stderr.
Do not rerun the checks in that state; reconcile the stored receipt instead.

Use `--format ndjson` for progress events on stderr and a final receipt on
stdout. `--quiet` suppresses progress. `taskattest schema --document brief
--format json` emits the bounded machine contract; full JSON Schemas are
available for discovery, receipt, progress, verification, error, and
receipt-recovery documents.

See [docs/CONFIGURATION.md](docs/CONFIGURATION.md) for explicit check
configuration and [docs/RECEIPTS.md](docs/RECEIPTS.md) for the trust model,
storage layout, and verification limits. See
[docs/RECEIPT_RECOVERY.md](docs/RECEIPT_RECOVERY.md) for the no-retry
reconciliation procedure.

## Safety boundaries

- TaskAttest itself never constructs a shell command; every check is spawned
  from a program and argument array.
- Package-manager script runners may invoke project-defined shell scripts.
  Their hashed `package.json` is part of the discovery evidence.
- Checks execute repository code. TaskAttest minimizes inherited environment
  variables and enforces time and combined-log budgets, but it does not sandbox
  network or filesystem access.
- Complete logs are retained unredacted and marked `potentially_sensitive`.
  Diagnostic summaries redact common secret assignments, but this is not a
  substitute for preventing secrets from reaching logs.
- A valid receipt proves internal integrity of captured evidence. It does not
  prove that the selected checks were sufficient or that the environment is
  reproducible.

Read [SECURITY.md](SECURITY.md) before using TaskAttest with untrusted
repositories.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
cargo package --locked --allow-dirty
```

The test suite covers source mutation, failed checks, timeout and process-tree
cleanup, log retention, receipt tampering, pre-execution output refusal,
post-execution no-retry recovery, no-clobber publication, CI replacement
mapping, structured Python dependency evidence, and labeled discovery accuracy
across all four initial ecosystems.

## Release integrity

CI tests Linux, macOS, Windows, and the declared Rust 1.85 MSRV. Tagged releases
contain native archives, documentation, completions, SHA-256 checksums,
CycloneDX SBOMs, and GitHub/Sigstore build provenance and SBOM attestations.
See [RELEASING.md](RELEASING.md).

## Community

Use [GitHub Discussions](https://github.com/yhay81/taskattest/discussions) for
questions and workflow examples, and structured issues for reproducible bugs
and feature proposals. See [CONTRIBUTING.md](CONTRIBUTING.md),
[SUPPORT.md](SUPPORT.md), [GOVERNANCE.md](GOVERNANCE.md), and the
[Code of Conduct](CODE_OF_CONDUCT.md). Report security issues privately.

Verified, opt-in usage is recorded in [ADOPTERS.md](ADOPTERS.md).

## License

MIT
