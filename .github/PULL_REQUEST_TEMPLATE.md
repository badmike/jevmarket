## Summary

<!-- What does this PR change, and why? Describe the intent, not just the diff. -->

## Linked issue

<!-- Closes #123 / Relates to #456, or "none" for small standalone fixes. -->

## How was this tested?

<!-- Tests added, commands run (`decide --show-state`, `run --dry-run`, ...), edge cases considered. -->

## AI assistance

<!-- If any part of this contribution was meaningfully shaped by an AI tool, say so.
     If you'd credit a human pair-programmer, credit the AI.
     Example: AI assistance: Used Claude to draft the batched order-book fetch, then reviewed and tested it. -->

## Checklist

- [ ] Commits follow the [gitmoji format](../CONTRIBUTING.md#commit-messages-gitmoji-required)
- [ ] `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass
- [ ] Anything that can spend money goes through `Executor::check`
- [ ] AI usage is disclosed above (or not applicable)
- [ ] I understand and can explain the code I'm submitting
