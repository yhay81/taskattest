# Configuration

TaskAttest prefers repository evidence and safe defaults. A checked-in
`.taskattest.toml` version 1 file can disable an automatically discovered check,
add an explicit check, forward named environment variables, and replace a CI
step that cannot be represented safely without a shell.

```toml
version = 1

disable_checks = ["rust-build"]

[[checks]]
id = "fixture-json"
label = "JSON fixtures"
kind = "test"
command = [
  "python3",
  "-c",
  "import json; [json.load(open(p, encoding='utf-8')) for p in ['one.json', 'two.json']]",
]
working_directory = "."
reason = "both checked-in JSON fixtures are part of the public contract"
coverage_paths = ["**/*.json", ".taskattest.toml", ".github/workflows/**"]
pass_environment = []
non_hermetic_inputs = ["installed Python interpreter"]
replaces_workflow_steps = [
  ".github/workflows/ci.yml#quality#Validate fixtures",
]
```

## Check fields

| Field | Required | Meaning |
| --- | --- | --- |
| `id` | Yes | Stable 1–64 character lowercase identifier. |
| `label` | Yes | Human-readable check name. |
| `kind` | Yes | `format`, `lint`, `test`, `type_check`, `build`, or `custom`. |
| `command` | Yes | Non-empty program and argument array. No shell parsing occurs. |
| `working_directory` | No | Repository-relative directory; defaults to `.`. |
| `reason` | Yes | Why the check is relevant evidence. |
| `coverage_paths` | No | Git-style glob patterns used by `--changed`. |
| `pass_environment` | No | Additional environment variable names to forward. Values are not recorded. |
| `non_hermetic_inputs` | No | Known external state that can affect the result. |
| `replaces_workflow_steps` | No | Exact unmodeled workflow observation IDs replaced by this check. |

Unknown fields, duplicate IDs, invalid environment names, unsafe working
directories, unknown replacement IDs, and two checks replacing the same
workflow step are configuration errors.

## Environment policy

Every process starts with an empty environment. TaskAttest forwards a small
cross-platform baseline when present: executable search paths, user-home paths,
temporary-directory paths, Rust toolchain paths, terminal/color indicators,
and `CI`. Additional names must be explicitly listed in
`pass_environment`.

Environment values are deliberately absent from receipts. A receipt records
only the forwarded names and declares the resulting environment as
non-hermetic.

Never forward a secret merely to make discovery pass. Prefer a separate,
non-secret verification target. Complete logs are not redacted.

## CI replacement mapping

GitHub Actions `run` steps containing pipes, redirects, multiple commands,
expressions, or unsupported tools are never silently rewritten. Verification-
looking steps become coverage gaps with IDs in this form:

```text
.github/workflows/<file>.yml#<job-id>#<step-name>
```

An explicit check may list that exact ID in `replaces_workflow_steps`. The
observation then records `replaced_by_explicit_check` and the replacement check
ID. Setup, caching, artifact upload, and release-delivery operations are not
treated as local verification gaps.

Replacement is a maintainer assertion that the argument-vector check captures
the workflow claim. Review replacement changes with the same care as CI
changes.
