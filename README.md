# TaskAttest

Evidence-backed verification receipts for software changes.

> Status: concept stage. This repository defines the product contract before implementation.

TaskAttest discovers the checks a repository already trusts, runs the relevant subset, and emits a portable receipt that binds the result to the exact source state, command, toolchain, and artifacts.

## Why

An agent saying “tests passed” is not durable evidence. The source may have changed, the wrong command may have run, output may have been truncated, or the result may no longer be reproducible.

TaskAttest turns verification into a small deterministic CLI contract:

```bash
taskattest discover
taskattest run --changed
taskattest receipt show rcpt_01J...
taskattest verify rcpt_01J...
```

## Product principles

- No LLM in the execution path.
- Read-only discovery before execution.
- Bounded structured output with full logs retained by handle.
- Receipts bind checks to source, configuration, tool versions, and artifacts.
- Non-interactive and CI-safe by default.
- Human-readable terminal output and stable JSON from the same contract.

## Initial scope

The first release targets JavaScript/TypeScript, Python, Rust, and Go repositories. It discovers test, lint, type-check, and build commands from project files and CI configuration.

See [CONCEPT.md](CONCEPT.md) for the proposed contract, MVP, non-goals, and success criteria.

## License

MIT
