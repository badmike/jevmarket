# Changelog

All notable changes to jevmarket are documented here.
Commits follow the [Gitmoji](https://gitmoji.dev/) convention.

## [Unreleased]

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
