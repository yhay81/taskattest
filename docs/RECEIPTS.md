# Receipt and evidence model

TaskAttest receipts are integrity documents, not signatures and not claims of
semantic correctness.

## Source binding

Before discovery, TaskAttest records:

- `HEAD` and the symbolic Git ref when available;
- the SHA-256 of Git porcelain status;
- a SHA-256 over every tracked and non-ignored untracked workspace entry,
  including path boundaries, file content, executable bits, symlink targets,
  and submodule HEAD/status;
- the changed path set and workspace file count.

The same identity is recomputed after every selected check. A mutation makes
the receipt fail even if all child commands returned zero.

## Canonical receipt

The receipt payload is serialized with deterministic Rust data structures and
hashed with SHA-256. `canonical_digest` contains the full digest and
`receipt_id` contains its first 128 bits:

```text
rcpt_<32 lowercase hexadecimal characters>
```

The ID is a compact content-derived handle, not a collision-proof replacement
for the full digest.

## Logs

stdout and stderr are captured separately. Their complete byte streams are
hashed while being written and published without overwriting under:

```text
<state-dir>/blobs/sha256/<64-character digest>
```

Receipts store the digest, byte count, encoding, sensitivity, and
`sha256:<digest>` handle. Repeated empty or identical logs share one blob.
Bounded tail summaries are included for diagnosis; common secret-assignment
lines are redacted there only.

The default state directory is `<git-dir>/taskattest`. Receipts are stored as
`receipts/<receipt-id>.json`.

An optional `--receipt-out` is an additional portable copy, not the primary
durable record. TaskAttest validates detectable destination failures before
checks start and never overwrites an existing path. A destination race can
still happen after checks complete. In that case the internally stored receipt
remains authoritative, stdout contains that receipt, stderr contains a
`taskattest.receipt-recovery.v1` document, and the process exits with code 7.
Follow [RECEIPT_RECOVERY.md](RECEIPT_RECOVERY.md); do not rerun the checks.

## Offline verification

`taskattest verify`:

1. requires the exact published receipt shape, rejecting unknown or omitted
   fields before they can disappear during deserialization;
2. checks the receipt schema version;
3. recomputes the canonical payload digest and derived ID;
4. validates source, selection, timing, log-reference, and outcome invariants;
5. hashes every referenced local log and compares its length and digest.

It does not rerun checks, contact a server, validate the current workspace, or
prove the author of a receipt. A failed-check or incomplete receipt can still
be internally valid: integrity and success are separate properties.

## Outcomes

- `passed`: every selected check passed, source stayed unchanged, and no
  coverage gap was recorded.
- `failed`: a check failed, timed out, exceeded its log budget, could not start,
  was skipped after fail-fast, or changed the source.
- `incomplete`: selected checks passed but no check ran or discovery found a
  coverage gap.
- `cancelled`: cancellation was observed.

Consumers must inspect both receipt integrity and outcome.

The versioned fixtures and self-consistent adversarial mutations in
[`tests/fixtures/contracts/`](../tests/fixtures/contracts/README.md) exercise
these guarantees without executing checks or contacting a network.

## Portability and retention

A standalone receipt is portable, but verification of log references also
requires the corresponding blob directory. Copy receipts and blobs together
when exporting evidence. Retention and access control are the operator's
responsibility; TaskAttest never uploads by default.
