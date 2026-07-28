# TaskAttest receipt compatibility corpus

This corpus freezes receipts emitted by TaskAttest's published machine
contract. Current and future offline verifiers must continue to accept every
receipt under its version directory and must reject the mutations declared by
that directory's manifest.

The fixtures contain synthetic source identities and no log blobs or
third-party content. They are covered by the repository's MIT license.

When a contract intentionally changes:

1. keep the existing version directory unchanged;
2. add a new version directory and manifest;
3. retain an old-version verifier or add a tested migration command;
4. document the compatibility decision and migration in the changelog.

The integration test verifies byte-level fixture SHA-256, exact pretty-JSON
round trips, canonical payload and receipt-ID binding, offline semantic
verification, stable rejection signals, and manifest coverage. The repository
attributes force LF bytes in this directory on Windows, macOS, and Linux.
