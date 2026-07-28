# Changelog

All notable TaskAttest changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and releases use
semantic versioning.

## [Unreleased]

### Added

- Added a privacy-conscious adoption report form that captures evaluation,
  repeat-use, limitations, evidence, and public-listing permission.
- Added a monthly maintainer-continuity drill that recovers the public Git
  mirror and verifies signed tags, release checksums, build/SBOM attestations,
  and the released native binary without repository write access.

## [0.2.0] - 2026-07-29

### Compatibility

- Preserved the public v0.1 CLI, discovery, receipt, and verification
  contracts. The v0.2 verifier accepts the digest-pinned v0.1 receipt corpus
  byte-for-byte; no migration is required.

### Added

- Added a digest-pinned v1 receipt compatibility corpus with exact round-trip
  checks and ten strict-reader or offline-semantic mutation cases.
- Added a deterministic 10,000-file, 100-check benchmark workspace with weekly
  raw discovery, offline-verification, output-size, and peak-memory artifacts.

### Fixed

- Avoided reporting a successful or timed-out Unix check as an execution error
  when its process group disappears during post-exit descendant cleanup.

### Changed

- Upgraded `sha2` to 0.11 and centralized lowercase hexadecimal encoding while
  preserving workspace, log, and receipt digest contracts.
- Receipt loading now rejects unknown or omitted fields before they can
  disappear during deserialization and evade canonical verification.
- Defined measurable v1.0 compatibility, discovery accuracy, lifecycle
  correctness, security, performance, delivery, maintenance, contribution, and
  repeat-adoption gates.

## [0.1.0] - 2026-07-28

### Added

- Git workspace identity before and after verification, including dirty files,
  symlinks, executable bits, and submodule state.
- Explainable JavaScript/TypeScript, Python, Rust, Go, GitHub Actions, and
  `.taskattest.toml` check discovery.
- Time, combined-log, environment, cancellation, and process-tree controls.
- Content-addressed complete logs, bounded redacted summaries, canonical
  receipts, durable local storage, and offline integrity verification.
- Versioned JSON results, JSON Schema documents, stable error/exit contracts,
  NDJSON progress, completion generation, and cross-platform CLI tests.
- Explicit argv-only replacements for otherwise unmodeled CI verification
  steps.

[Unreleased]: https://github.com/yhay81/taskattest/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/yhay81/taskattest/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/yhay81/taskattest/releases/tag/v0.1.0
