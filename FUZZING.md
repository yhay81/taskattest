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

## Manual maintenance

Run the following commands individually from a checkout of `main` with the
corpus seeded as above. The comparison build uses a commit-specific directory
so a later build cannot replace it.

```bash
# Keep a comparison build for the checked-out commit.
cargo +nightly fuzz build --target-dir "$PWD/fuzz/target/baselines/$(git rev-parse HEAD)" receipt_document

# Run a 15-minute batch against the local corpus.
cargo +nightly fuzz run receipt_document -- -max_total_time=900

# Minimize the accumulated local corpus in place.
cargo +nightly fuzz cmin receipt_document
```

The ClusterFuzzLite comparison build, batch and pruning workflows can each
be started from GitHub Actions using **Run workflow** on `main`. Hosted jobs
still have a one-minute limit. Use the local commands for a full run. Local
builds, corpora and findings stay on the machine.

Treat minimized crashes as potentially sensitive because receipts can contain
commands, paths, and captured evidence metadata. Add a deterministic regression
test before fixing the defect, and use the private process in
[SECURITY.md](SECURITY.md) for security-relevant findings.
