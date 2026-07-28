# TaskAttest roadmap

TaskAttest is built as vertical evidence slices. Every slice must preserve
explainable discovery, argument-vector execution, bounded output, source
binding, no-clobber evidence storage, and independent verification.

## 0.1 foundation

- [x] Git workspace and dirty-state identity before and after checks.
- [x] JavaScript/TypeScript, Python, Rust, and Go manifest discovery.
- [x] Safe GitHub Actions command discovery and explicit replacement mapping.
- [x] Relevant-check selection for full, changed, and explicit runs.
- [x] Runtime, log, environment, cancellation, and process-tree controls.
- [x] Content-addressed logs and canonical durable receipts.
- [x] Offline integrity and semantic verification.
- [x] Human, JSON, NDJSON, brief schema, full JSON Schema, and completions.
- [x] Unit and CLI failure-path coverage.
- [x] Linux, macOS, Windows, MSRV, audit, and package CI green on `main`.
- [x] Signed `v0.1.0` tag and independently verified release archives.

## 0.2 discovery quality

- Benchmark corpus with labeled expected checks across all four ecosystems.
- Monorepo/workspace package graphs and changed-path dependency propagation.
- More task runners and CI providers without arbitrary configuration
  evaluation.
- Precision/recall reports and regression fixtures for every heuristic.
- Platform and matrix conditions represented explicitly in observations.

## 0.3 portable evidence

- Explicit receipt bundle export/import with log retention policy.
- Optional signing profile separate from local digest integrity.
- Artifact capture with bounded, opt-in paths.
- Re-run planning that distinguishes environment equivalence from receipt
  verification.

## 1.0 gates

- Stable receipt and configuration compatibility policy.
- Published discovery benchmark and documented false-positive/false-negative
  targets.
- Reproducible cross-platform releases with provenance and SBOMs.
- Security review of execution, process cleanup, storage, and verification.
- At least three opt-in external workflows recorded in [ADOPTERS.md](ADOPTERS.md).
- Maintainer runbook, succession path, support policy, and two consecutive
  compatibility-preserving minor releases.
