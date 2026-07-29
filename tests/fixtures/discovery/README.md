# Discovery accuracy corpus

`v0.1/corpus.json` is a deterministic, MIT-licensed set of 100 labeled Git
workspaces:

- 20 single-package JavaScript/TypeScript projects;
- 20 single-package Python projects;
- 20 single-package Rust projects;
- 20 single-package Go projects;
- 10 mixed-root projects; and
- 10 monorepos with explicit, argv-only nested-package checks.

The cases cover positive and negative manifest evidence, package-manager
selection, safe and unmodeled GitHub Actions steps, and discovery tripwires.
Expected check IDs and coverage gaps are hand-labeled from the declared
verification intent. The generator serializes those labels; it does not derive
them from TaskAttest output.

The evaluator creates a fresh committed Git workspace for every case and runs
only `taskattest discover --format json`. It independently calculates overall
and per-ecosystem true positives, false positives, false negatives, precision,
and recall. It also requires exact coverage-gap output and verifies that
package scripts, build scripts, tests, and workflow fragments did not create
their declared tripwire paths.

`v0.1/metrics.json` pins the reproducible result. CI requires at least 95%
precision and 90% recall overall and rejects any corpus-generation drift,
metrics drift, unexpected gap, or executed tripwire.

Regenerate and evaluate from the repository root:

```bash
python3 tests/fixtures/discovery/v0.1/generate_corpus.py
cargo build --locked
python3 tests/fixtures/discovery/v0.1/evaluate.py \
  --binary target/debug/taskattest \
  --corpus tests/fixtures/discovery/v0.1/corpus.json \
  --output tests/fixtures/discovery/v0.1/metrics.json
```

The monorepo fixtures measure the documented explicit-configuration path.
Automatic package-graph discovery and changed-path dependency propagation
remain separate roadmap work and are not inferred from these results.
