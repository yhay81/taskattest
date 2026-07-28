# TaskAttest performance baseline

This directory defines the reproducible, observation-only baseline used to
calibrate TaskAttest's v1.0 performance thresholds. Timing and memory are not
yet required pull-request checks.

## Workload

`generate_workspace.py` creates a clean Git repository with exactly 10,000
tracked files, including `.taskattest.toml`, and 100 explicit argv-only checks.
Each check emits one unique short line so the resulting receipt references 100
distinct stdout blobs plus the shared empty stderr blob. The synthetic fixture,
generator, and outputs are covered by the repository's MIT license.

The untimed setup runs all 100 checks and writes a receipt to an external state
directory. Measurements then run once, without warm-up, in this fixed order:

1. discover and select all 100 checks while hashing the 10,000-file workspace;
2. verify the 100-check receipt and all 200 stdout/stderr references offline.

The harness records wall-clock time and maximum resident memory from GNU
`time`, output and receipt bytes, fixture identity and counts, verification
evidence, runner identity, and the exact TaskAttest commit.

## Run

The supported measurement environment is the `ubuntu-latest` GitHub-hosted
runner selected by `.github/workflows/benchmark.yml`. Run it manually with the
**Benchmark** workflow, or on a compatible Linux machine:

```bash
benchmarks/run.sh benchmark-results.json
jq . benchmark-results.json
```

GNU `time`, GNU `stat`, `timeout`, `jq`, Python 3, Git, Cargo, and the locked
Rust dependency graph are required. Fixture generation, release building, and
receipt setup are excluded from measurements. The generated repository,
receipt, and log blobs are temporary and are not uploaded.

The workflow retains raw JSON for 90 days. Shared hosted runners are noisy, so
a single run is not a regression. Before enabling v1.0 gates, publish the
runner image, warm-up policy, sample count, p95 calculation, baseline window,
and noise-aware regression rule with the raw measurements.
