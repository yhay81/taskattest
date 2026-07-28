## User problem

<!-- What user-visible evidence or workflow problem does this solve? -->

## Scope and tradeoffs

<!-- Include discovery precision/recall and compatibility tradeoffs. -->

## Verification

<!-- Exact commands, fixtures, platforms, negative cases, and dogfood receipts. -->

## Release-safety checklist

- [ ] Discovery remains evidence-based and reports uncertainty as a coverage gap.
- [ ] Commands are argument arrays and working directories stay in the repository.
- [ ] I tested failure, timeout/cancellation, mutation, and cleanup paths when relevant.
- [ ] I preserved bounded output, no-clobber storage, and offline verification.
- [ ] I updated schemas, docs, examples, and the changelog for public changes.
- [ ] I ran formatting, clippy, tests, and package validation.
- [ ] I did not commit secrets, private source, machine paths, receipts, or logs.
- [ ] I documented compatibility or migration impact.
