# Support

## Where to ask

- Use [GitHub Discussions](https://github.com/yhay81/taskattest/discussions) for
  installation, configuration, and workflow questions.
- Use a structured [GitHub issue](https://github.com/yhay81/taskattest/issues/new/choose)
  for reproducible bugs or scoped feature requests.
- Follow [SECURITY.md](SECURITY.md) for vulnerabilities.

TaskAttest is maintained by volunteers. Reports with a minimal synthetic Git
repository, exact version, operating system, command, and redacted structured
output are the easiest to investigate.

Never post complete log blobs, environment values, proprietary source, or
private absolute paths.

## Supported environment

The latest tagged pre-1.0 release supports:

- Linux x86-64;
- macOS x86-64 and Apple silicon;
- Windows x86-64;
- Rust 1.85 or newer when building from source;
- Git workspaces with supported project evidence.

Collect:

```bash
taskattest --version
taskattest schema --document brief --format json
git --version
```

For discovery reports, review `changed_paths`, command arguments, summaries,
and workflow observations before posting.

## Scope

Support does not cover running untrusted repository code safely, arbitrary
shell workflow emulation, guarantees that checks are sufficient, hosted log
retention, environment reproducibility, or authentication of receipt authors.
