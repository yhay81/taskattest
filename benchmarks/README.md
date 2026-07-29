# TaskAttest performance baseline

This directory defines and enforces TaskAttest's reproducible v1.0 performance
thresholds on pull requests and in the weekly scheduled benchmark.

## Workload

`generate_workspace.py` creates a clean Git repository with exactly 10,000
tracked files, including `.taskattest.toml`, and 100 explicit argv-only checks.
Each check emits one unique short line so the resulting receipt references 100
distinct stdout blobs plus the shared empty stderr blob. The synthetic fixture,
generator, and outputs are covered by the repository's MIT license.

Each raw sample performs untimed setup by running all 100 checks and writing a
receipt to an external state directory. The workflow performs one discarded
warm-up followed by 20 measured samples. Each sample measures, in this order:

1. discover and select all 100 checks while hashing the 10,000-file workspace;
2. verify the 100-check receipt and all 200 stdout/stderr references offline.

The harness records wall-clock time and maximum resident memory from GNU
`time`, output and receipt bytes, fixture identity and counts, verification
evidence, runner identity, and the exact TaskAttest commit. `evaluate.py`
requires every sample to use the same commit and runner image, calculates p95
with the nearest-rank method, and fails closed on missing or malformed evidence.

## Enforced thresholds

The versioned policy in `thresholds.json` enforces:

- discovery and selection below 2 seconds p95;
- offline receipt verification below 1 second p95;
- peak resident memory no greater than 256 MiB in every measured sample.

Twenty samples make nearest-rank p95 the second-slowest observation, preventing
one hosted-runner outlier from deciding the result. Once
`baseline-ubuntu24.json` is present, each metric must also remain within the
stricter of its absolute v1.0 limit and the versioned noise allowance: 1.5
times baseline or baseline plus 50 ms for time and 16 MiB for memory.

## Run

The supported measurement environment is the `ubuntu-24.04` x86_64
GitHub-hosted runner selected by `.github/workflows/benchmark.yml`. Run one raw
sample on a compatible Linux machine with:

```bash
benchmarks/run.sh benchmark-results.json
jq . benchmark-results.json
```

Run evaluator tests with:

```bash
python3 -m unittest benchmarks/test_evaluate.py
```

GNU `time`, GNU `stat`, `timeout`, `jq`, Python 3, Git, Cargo, and the locked
Rust dependency graph are required. Fixture generation, release building, and
receipt setup are excluded from measurements. The generated repository,
receipt, and log blobs are temporary and are not uploaded.

The workflow uploads all 20 raw samples and the aggregate evaluation for 90
days. The checked-in baseline is refreshed only from a successful protected
runner evaluation, so baseline changes remain reviewable rather than silently
moving with every run.
