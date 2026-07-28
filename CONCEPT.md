# TaskAttest concept

## One-line thesis

TaskAttest gives humans and software agents a compact, verifiable receipt that a
software change was checked against the right evidence.

## Problem

Agents can run tests, linters, type checkers, and build commands, but the final
claim—"this change was verified"—is usually an unstructured summary. A reviewer
cannot easily tell:

- which checks were selected and why;
- which source state and configuration they ran against;
- whether relevant checks were omitted;
- where the full evidence lives; or
- whether a receipt was altered after the run.

CI systems retain logs, but they are optimized for workflows and web UIs rather
than a small, local, agent-readable verification contract.

## Target users and jobs

- Coding agents that must prove a change is ready.
- Maintainers reviewing agent-authored pull requests.
- Local automation and CI jobs that need the same evidence format.
- Tool builders that need verification results without integrating every test
  runner separately.

The primary job is: **given a workspace or commit, select relevant checks, run
them, and return a bounded receipt that can be independently verified.**

## Product principles

1. Evidence, not confidence language.
2. Structured output is the primary interface.
3. Discovery is explainable and overrideable.
4. Receipts bind checks to an exact source and configuration state.
5. Full evidence is retained by reference; normal output stays bounded.
6. Non-hermetic conditions and missing coverage are explicit.
7. The CLI never requires an agent to scrape prose or terminal decoration.

## Proposed command contract

```text
taskattest schema --document brief --format json
taskattest discover --changed --format json
taskattest run --changed --format json
taskattest run --check unit --check lint --format json
taskattest receipt show <receipt-id> --format json
taskattest verify <receipt-file> --format json
```

All commands support:

- `--format json` for one bounded result;
- `--format ndjson` for progress plus a final result;
- `--quiet` to emit only the result;
- stable exit codes and machine-readable error codes;
- `schema --brief` as the low-token capability contract.

The brief schema should describe commands, required arguments, result shapes,
error codes, and safety-relevant defaults in a few kilobytes.

## Discovery model

Discovery starts with repository evidence rather than assumptions:

- package manifests and workspace files;
- task runner configuration;
- CI workflow definitions;
- language-specific test and lint configuration;
- changed paths and dependency relationships;
- an optional checked-in `.taskattest.toml`.

Each proposed check includes `reason`, `source`, `confidence`, and
`coverage_paths`. Users and agents can accept the set, add checks, or explicitly
record omissions.

## Receipt model

A receipt contains at least:

- receipt schema version and TaskAttest version;
- source identity: commit, dirty-state digest, and relevant file digests;
- normalized argument vector and working directory;
- selected checks and discovery reasons;
- configuration and toolchain digests;
- start time, duration, exit status, and check-level results;
- counts and bounded diagnostic summaries;
- hashes and content-addressed references for complete logs and artifacts;
- redaction policy and names of relevant environment variables;
- declared coverage gaps and non-hermetic inputs;
- a canonical receipt digest.

The receipt can be verified offline for internal consistency. Re-running a
receipt is a separate operation because environment equivalence cannot be
assumed.

## Initial scope

Version 0.1 implements:

- support Git workspaces;
- discover common JavaScript/TypeScript, Python, Rust, and Go checks;
- read common CI workflow and package configuration;
- execute local checks with time and output budgets;
- store complete logs locally by content digest;
- emit and verify versioned JSON receipts;
- run unchanged in local development and CI.

## Non-goals

- Replacing test frameworks, linters, build systems, or hosted CI.
- Claiming that passing checks proves semantic correctness.
- Generating tests with an LLM.
- Uploading source, logs, or receipts by default.
- Treating a dirty workspace as equivalent to a commit.

## Differentiation and defensibility

The moat is not another test runner. It is a portable evidence protocol plus
high-quality, explainable check discovery across ecosystems. As integrations
grow, TaskAttest can become the common verification boundary between coding
agents, local tools, CI, and review systems.

## Success measures

- Median tokens and commands required for an agent to verify a change.
- Share of relevant repository checks discovered in a benchmark corpus.
- False inclusion and false omission rates.
- Percentage of receipts that verify independently.
- Difference between local and CI results for the same source state.
- Maintainer adoption in agent-authored pull requests.

## Key risks and open questions

- Repository conventions are diverse; discovery can create false confidence.
- External services and mutable caches make checks non-hermetic.
- Logs may contain secrets despite redaction.
- A receipt can prove what ran, not that the chosen checks were sufficient.
- The project needs a policy for receipt signing without making key management
  mandatory for local use.

The UI and documentation must consistently distinguish **evidence captured**
from **correctness proven**.
