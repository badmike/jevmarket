# Changelog

All notable changes to jevmarket are documented here.
Commits follow the [Gitmoji](https://gitmoji.dev/) convention.

## [Unreleased]

- ⚡️ Cut researcher tokens by about 30%: a terser brief that fits `research_max_chars`, source URLs from citations, `research_max_results` defaults to 5
- ✨ Add `resolve` and fetch market outcomes at the start of every `run` pass; `stats` now reports Brier scores, hit rates by edge, answerable and clarity, and live and dry-run PnL
- 🐛 Trim brief considerations for and against YES in turn; before, every point against YES was dropped before any point for it
- ✨ Research a cached brief again before trading on it or once the midpoint moved more than `research_max_price_move`, and skip edges above `suspicious_edge`
- ⚡️ Pre-screen clarity with one cheap Jev call before paying for research, and rank candidates by opportunity instead of 24h volume
- ✨ Ask the researcher for scheduled events and the current state of the resolution source
- ✨ Add `jev_sees_market_price` to compare Jev with and without the market price in its state
- ✨ Add `jevmarket daemon`: the trading loop plus a live web console for markets, briefings, positions, stats and settings; a live daemon starts paused until resumed in the console
- ✨ Add `jevmarket init`, a guided setup for OpenRouter, models, the Polymarket wallet and risk limits, with live checks
- 🔒 Create the config file with owner-only permissions before writing keys to it
- 🎉 Port jevymarket to Rust as a single `jevmarket` binary with the same commands, prompts, math and SQLite schema
- 🔧 Store settings as JSON in the OS config directory, with `config path|init|show|set` and env overrides for secrets
- ⚡️ Fetch order books for a whole page of markets in one request and bound the market scan by end date on the server
- ⚡️ Research and price the next markets concurrently during `run` while orders stay sequential
- ✨ Detect Safe, proxy and deposit wallets from the configured address
- ✨ Resolve polymarket.com event URLs and list market slugs for multi-market events
- 🐛 Keep `run` going when the exchange rejects a single order
- 🐛 Keep `run --loop` alive through a failed pass and stop cleanly on Ctrl-C
- ♻️ Share one OpenRouter client, retry policy and error type between Jev and the researcher
