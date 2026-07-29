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
- [x] Deterministic 10,000-file, 100-check performance fixture with weekly raw
  discovery, verification, output-size, and peak-memory artifacts.
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

## v1.0 quality criteria

TaskAttest reaches v1.0 only when every gate below has published,
reproducible evidence. More discovered commands, receipts, downloads, or stars
do not substitute for accurate discovery, bounded execution, or real use.

### Product and compatibility

- CLI, configuration, discovery, selection, progress, receipt, log-reference,
  verification, schema, error, and exit-code contracts remain compatible across
  at least two released pre-1.0 minor versions.
- Golden workspaces and receipts from every supported contract version are
  accepted by the current verifier or have a tested migration command and
  migration guide.
- Receipt verification remains offline and never reruns a check, resolves a
  tool from `PATH`, or trusts a stored outcome without validating its canonical
  payload and referenced blobs.
- Environment equivalence, non-hermetic inputs, source identity, and discovery
  gaps remain explicit rather than being promoted to stronger attestation.

Current evidence: v0.2 and v0.3 provide two released compatibility cycles. The
current v0.3 verifier accepts the digest-pinned v1 receipt corpus byte-for-byte
and preserves canonical digest, receipt-ID, and offline semantic verification.
Thirteen declared mutations cover strict reader shape, duplicate-key
ambiguity, identity, schema, source, timing, and outcome failures, including
self-consistent attacker-recomputed digests. The v0.2 and v0.3 release notes
record contract preservation; no migration is required.

Standalone receipt publication is preflighted before checks run. A late
destination race preserves the primary durable receipt, returns it on stdout,
emits a versioned no-retry recovery document on stderr, and uses a distinct
exit class. The published receipt v1 and normal error v1 documents are
unchanged.

### Discovery accuracy, correctness, and security

- A published labeled corpus contains at least 100 representative projects,
  with at least 20 each for JavaScript/TypeScript, Python, Rust, and Go plus
  mixed and monorepo fixtures.
- On that corpus, check discovery achieves at least 95% precision and 90%
  recall overall, publishes per-ecosystem results, and emits a coverage gap
  instead of guessing when a safe argv command cannot be modeled.
- The workflow corpus has zero arbitrary expression, shell fragment,
  configuration code, or package script execution during discovery.
- The adversarial receipt corpus has 100% rejection of payload, digest, source,
  log, selection, outcome, and schema mutations.
- Cross-platform stress completes 10,000 aggregate timeout, cancellation,
  log-limit, spawn-failure, normal-exit, and orphan-descendant lifecycle
  iterations without a surviving owned process, hung capture pipe, or passing
  receipt for changed source.
- An independent security review covers command discovery, argument handling,
  environment forwarding, process trees, cancellation races, log storage,
  redaction, no-clobber publication, receipt integrity, and offline
  verification; all critical and high findings are resolved.
- No known critical or high-severity vulnerability is open at release time.

### Performance and bounds

- Discovery and selection for the published 10,000-file, 100-check workspace
  complete below 2 seconds p95 on the documented GitHub-hosted runner.
- Offline verification of the published 100-check receipt completes below
  1 second p95, including all referenced-blob digests.
- Peak resident memory remains below 256 MiB for every published bounded
  fixture.
- Runtime, combined log bytes, diagnostic summaries, receipt size, environment
  names, and stored artifacts never exceed configured bounds without an
  explicit structured outcome.
- Corpus labels, runner images, raw measurements, and regression thresholds
  are versioned with the repository.

### Delivery and maintenance

- Required CI remains green on Linux, macOS, and Windows for 30 consecutive
  days before the v1.0 tag.
- Releases originate only from protected `main` and signed annotated tags; all
  native archives have verified checksums, GitHub-hosted provenance, and a
  CycloneDX SBOM attestation.
- The release and execution-incident runbooks are exercised by two maintainers,
  or governance records the single-maintainer continuity risk and a tested
  recovery procedure.
- Security reports are acknowledged within 3 business days and receive an
  initial assessment within 7.

### Adoption evidence

- At least three independent external workflows are recorded in
  [ADOPTERS.md](ADOPTERS.md) with the decision a receipt improved.
- At least two adopters report repeat use separated by 30 days.
- At least one public CI or agent workflow verifies and consumes a receipt
  rather than only printing or storing it.
- At least one non-maintainer issue, discussion, benchmark label,
  documentation change, test, ecosystem fixture, or code contribution is
  resolved and credited.

Maintainer-authored fixtures, automated downloads, stars, and synthetic
accounts cannot satisfy adoption gates.
