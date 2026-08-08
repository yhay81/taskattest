# Fuzzing TaskAttest

TaskAttest continuously fuzzes its untrusted receipt boundary with
AddressSanitizer. The `receipt_document` target exercises the production size
bound, recursive duplicate-key rejection, JSON decoding, typed schema, and
lossless unknown-or-omitted-field check.

Install a current nightly toolchain and the pinned local runner, then run:

```bash
cargo install cargo-fuzz --version 0.13.2 --locked
mkdir -p fuzz/corpus/receipt_document
cp tests/fixtures/contracts/v1/receipt.incomplete.json \
  fuzz/corpus/receipt_document/
cargo +nightly fuzz run receipt_document
```

Pull requests receive a five-minute ClusterFuzzLite code-change run. A
15-minute batch run executes weekly on `main`, seeded by the versioned
compatibility receipt, and publishes machine-readable findings to GitHub code
scanning.
Each code-changing `main` update also saves a comparison build so later pull
requests can distinguish newly introduced crashes. The accumulated corpus is
pruned after every weekly batch.

Treat minimized crashes as potentially sensitive because receipts can contain
commands, paths, and captured evidence metadata. Add a deterministic regression
test before fixing the defect, and use the private process in
[SECURITY.md](SECURITY.md) for security-relevant findings.
