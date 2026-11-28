# Fuzzing TaskAttest

TaskAttest fuzzes its untrusted receipt boundary with
AddressSanitizer. The `receipt_document` target exercises the production size
bound, recursive duplicate-key rejection, JSON decoding, typed schema, and
lossless unknown-or-omitted-field check.

Install a current nightly toolchain and the pinned local runner, then run:

```bash
cargo install cargo-fuzz --version 0.13.2 --locked
mkdir -p fuzz/corpus/receipt_document
cp tests/fixtures/contracts/v1/receipt.incomplete.json \
  fuzz/corpus/receipt_document/
cargo +nightly fuzz run receipt_document -- -max_total_time=300
```

Fuzzing a change is optional. To test a pull request, check out its branch
and run the commands above. The five-minute limit applies to the fuzz run
after the target has compiled. Omit `-max_total_time=300` for a longer run.

The weekly ClusterFuzzLite batch on `main` uses the versioned compatibility
receipt as its seed.
Its 15-minute fuzz run exceeds the hosted job's one-minute limit. Completed
findings are published to GitHub code scanning. Corpus pruning and `main`
comparison builds have separate workflows.

Treat minimized crashes as potentially sensitive because receipts can contain
commands, paths, and captured evidence metadata. Add a deterministic regression
test before fixing the defect, and use the private process in
[SECURITY.md](SECURITY.md) for security-relevant findings.
