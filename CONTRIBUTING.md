# Contributing to jevmarket

Thanks for taking a look. jevmarket is a small, opinionated bot that spends real money, so PRs that keep it small and keep the brakes intact are the most welcome.

## Table of contents

- [Code of conduct](#code-of-conduct)
- [Getting started](#getting-started)
- [Ground rules for this codebase](#ground-rules-for-this-codebase)
- [Commit messages: gitmoji required](#commit-messages-gitmoji-required)
- [AI-assisted contributions](#ai-assisted-contributions)
- [Pull request process](#pull-request-process)
- [Ideas that would fit](#ideas-that-would-fit)

## Code of conduct

Be kind, patient and constructive. The full text lives in [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). Report violations to [hello@coderscantina.com](mailto:hello@coderscantina.com).

## Getting started

You need Rust 1.88 or newer.

```bash
git clone https://github.com/<your-username>/jevmarket.git
cd jevmarket
cargo test
```

The tests are fully offline: OpenRouter is mocked with `wiremock`, and nothing talks to Polymarket. You only need keys for live commands. Keep a separate config for development so you never touch your live settings:

```bash
export JEVMARKET_CONFIG=/tmp/jevmarket-dev.json
cargo run -- config set openrouter_api_key sk-or-...
cargo run -- decide <slug>
```

Branch names: `feat/`, `fix/`, `docs/`, `refactor/`, `test/`, `chore/`.

## Ground rules for this codebase

- **Anything that can spend money lives in `executor.rs`** and goes through `Executor::check`. No other module places orders or signs transactions.
- **Pure logic belongs in `signal.rs` with a test.** Edge, sizing, gates and rounding stay free of I/O.
- **Keep the Jev state small.** Every field in `markets::build_state` costs accuracy. Justify additions with a before/after from `decide --show-state`.
- **The researcher never estimates a probability.** It reports dated facts; Jev makes the call.
- **Keep the SQLite schema compatible** with existing databases. Add columns, don't rename them.
- **Never commit config files, databases, keys or wallet addresses.**

Before opening a PR:

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

## Commit messages: gitmoji required

Every commit starts with exactly one [gitmoji](https://gitmoji.dev/), then a short imperative subject under 72 characters:

```
<emoji> <short description in present tense>

[optional body]

[optional footer: closes #issue, co-authors, etc.]
```

| Emoji | Use for |
|---|---|
| ✨ | New feature |
| 🐛 | Bug fix |
| 📝 | Documentation |
| ♻️ | Refactoring |
| ⚡️ | Performance improvement |
| 🔒 | Security fix |
| 🧪 | Adding tests |
| 🔧 | Config or tooling |
| 🩹 | Minor fix |
| 🔥 | Removing dead code |

```
🐛 Refuse orders when the min size pushes past the per-trade cap
```

## AI-assisted contributions

AI-assisted contributions are welcome. Be transparent about them.

1. **Disclose meaningful AI usage in the PR description.** One line is enough: `AI assistance: Used Claude to draft the batched order-book fetch, then reviewed and tested it.` Autocomplete and one-liners need no disclosure. If you'd credit a human pair-programmer, credit the AI.
2. **Understand what you submit.** You are responsible for the code, not the model. Be ready to explain what it does, why it is correct and how you tested it.
3. **Review everything.** AI output looks plausible and hides subtle bugs. In a bot that places orders, a subtle bug costs money.
4. **No hallucinated APIs.** Check that every crate, function and endpoint exists at the version in `Cargo.lock`. The Polymarket SDK and OpenRouter's Decisions API both move quickly.

## Pull request process

1. Keep a PR to one concern.
2. Fill in the [PR template](.github/PULL_REQUEST_TEMPLATE.md), including how you tested and any AI assistance.
3. CI runs fmt, clippy and tests on Linux, macOS and Windows. It has to be green.
4. Changes to caps, sizing or order placement get an extra careful review. Expect questions.

## Ideas that would fit

- Exit logic: selling or re-pricing positions when new evidence moves Jev's estimate.
- A calibration report that joins Jev's `p_yes` with actual resolutions from the log.
- Alternative researchers (native-search models, news APIs) behind the same `Brief` shape.
