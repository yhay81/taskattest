# Standalone receipt recovery

`--receipt-out` creates an additional portable copy. The primary receipt and
its content-addressed log evidence are stored under the selected TaskAttest
state directory.

TaskAttest validates the requested output path before checks start. Existing
targets, invalid parents, and other detectable failures return before a check
or state store is created. The requested path is never overwritten.

## Publication race after checks

Another process or a check can occupy the requested path after preflight. If
that happens, TaskAttest:

1. keeps the command-created or concurrently created path unchanged;
2. keeps the integrity-bound receipt in
   `<state-dir>/receipts/<receipt-id>.json`;
3. writes the same receipt document to stdout;
4. writes a `taskattest.receipt-recovery.v1` document to stderr;
5. exits with code 7.

The recovery document contains:

- `action: "do_not_retry_run"`;
- `command_state: "checks_completed"`;
- the receipt ID and `receipt_persisted: true`;
- the stored and requested receipt paths;
- the underlying publication error code.

## Reconciliation procedure

1. Do not rerun `taskattest run`. Checks may execute arbitrary repository code,
   and repeating them can duplicate effects.
2. Parse the receipt from stdout and the recovery document from stderr.
3. Require the receipt IDs to match.
4. Using the same `--workspace` and `--state-dir`, verify the stored receipt:

   ```bash
   taskattest verify rcpt_0123... --format json
   ```

5. Inspect the requested path independently. TaskAttest deliberately leaves it
   untouched because it may belong to the completed check or another process.
6. If a portable copy is still needed, choose a new absent destination and
   copy the verified stored receipt without rerunning checks.

Receipt integrity and check outcome remain separate. A recovered receipt can
be internally valid while its `outcome` is `failed`, `incomplete`, or
`cancelled`.
