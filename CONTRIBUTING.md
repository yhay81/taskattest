# Contributing to TaskAttest

Contributions of code, discovery fixtures, security hardening, documentation,
benchmark repositories, and reproducible bug reports are welcome.

## Before opening an issue

- Use GitHub Discussions for usage questions and design exploration.
- Search existing issues and include a minimal synthetic repository when
  reporting discovery behavior.
- Report security-sensitive behavior privately through [SECURITY.md](SECURITY.md).
- Remove secrets, private paths, and complete logs from public reports.

## Development setup

TaskAttest requires Rust 1.85 or newer and Git on `PATH`.

```bash
git clone https://github.com/yhay81/taskattest.git
cd taskattest
cargo test --all-targets --locked
```

Some tests use portable Git repositories. Unix-only process-group tests skip on
Windows.

Receipt parsing is continuously fuzzed. See [FUZZING.md](FUZZING.md) for the
reproducible local command and crash-handling rules.

## Making a change

1. Open an issue first for a receipt/schema change, new ecosystem, discovery
   heuristic, or security-boundary change. Small fixes do not require one.
2. Keep discovery evidence-based. A false “passed” result is more harmful than
   an explicit coverage gap.
3. Preserve argument arrays, bounded inputs/outputs, no-clobber publication,
   source re-identification, and offline verification.
4. Add success and failure tests. Discovery changes need representative
   positive and negative fixtures.
5. Update schemas, configuration/receipt documentation, and the changelog for
   public behavior changes.
6. Run:

   ```bash
   cargo fmt --check
   cargo clippy --all-targets --locked -- -D warnings
   cargo test --all-targets --locked
   cargo package --locked --allow-dirty
   ```

## Public contracts

Discovery reports, receipts, progress events, verification reports, errors,
exit codes, check IDs, configuration fields, and JSON Schemas are public
interfaces. Within a stable major version, changes must remain compatible or
ship with explicit migration notes.

## Pull requests

Explain the user problem, smallest complete scope, false-positive/false-negative
tradeoff, exact verification, and failure paths exercised. By contributing,
you agree that your contribution is licensed under MIT and follows the
[Code of Conduct](CODE_OF_CONDUCT.md).
