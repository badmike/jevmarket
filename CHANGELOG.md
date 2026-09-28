# Changelog

All notable changes to jevmarket are documented here.
Commits follow the [Gitmoji](https://gitmoji.dev/) convention.

## [Unreleased]

- ✨ The price watch sells held positions early when the best bid beats Jev's probability of the held side by `min_exit_edge` (0.05) and returns `min_exit_profit` (20%) on the entry, after renewing a stale view. Logged as `sell` or `hold` decisions and SELL orders; stats count sale proceeds. Off with `sell_early: false`
- ✨ Put any market on your watchlist from the console (`POST /api/watchlist`, `DELETE /api/watchlist/{slug}`): it is watched whatever its view's age, and decided first when Jev never priced it
- ✨ `daemon --autostart` starts a live loop right away instead of waiting paused for a resume
- ✨ The price watch keeps checking prices while the loop is paused, placing no orders
- ✨ Markets, the watchlist and positions show when a market resolves
- ✨ Pin briefings, icon-only loop controls with tooltips, and plain-English tooltips across the console
- 🐛 Buttons show a loading spinner or their icon, not both
- ✨ An event link with several open markets offers them in a picker when deciding or watching a market, instead of failing
- 💄 Market thumbnails from Polymarket on markets, the watchlist, positions and briefings (`decisions.image`)
- ✨ Sort markets and the watchlist by any column, on a phone too; the choice is remembered
- 💄 Briefings on a desktop scroll the list and the reader apart, and a newly opened brief starts at its top; read briefs are dimmed, without an unread dot

- ✨ The daemon watches prices between research cycles: every `watch_interval_secs` (60) the stored Jev views of recently researched markets meet the live order books, and a trade signal is researched again when its brief is older than `trade_brief_max_age_minutes` (120), put back to Jev when the price moved, then ordered under the same caps. Research cycles run hourly by default (`daemon --loop 3600`); the trade and research caps are shared per cycle. New `GET /api/watchlist`, a watch card on the Overview page and a Watchlist tab on the Markets page
- ✨ `jevmarket service install|uninstall|restart|status` runs the daemon as a login service, a launchd agent on macOS or a systemd user unit on Linux
- ✨ Install the web console as an app on iOS and Android: web app manifest, home screen icons, full-screen standalone mode
- 📦 Homebrew formula: `brew install badmike/tap/jevmarket`, updated by the release workflow
- ✨ Skips carry a reason code (`decisions.skip_code`, backfilled for older rows), so the console explains them without parsing log text
- ✨ Orders remember their market question; open orders and the order log always name their market
- 🐛 An open console reloads itself when it reconnects to a daemon running a newer build, and retries at once when the tab or network comes back
- ✨ Markets and briefings remember what you opened in this browser and dim it; a newer decision or brief counts as unread again
- 💄 Plain-language Markets page, a two-pane briefings reader with keyboard navigation, a clearer Positions overview, and settings with a description each and an inline reset
- ✨ Rejected orders keep the exchange's reason, shown in the order log
- ✨ `transfer` and the console's Positions page move pUSD between the deposit wallet and your polymarket.com wallet, both ways, without gas; only ever between the two wallets derived from the key
- ✨ Configure `polymarket_deposit_wallet` and `polymarket_proxy_wallet` apart; `polymarket_wallet` in existing files migrates to the first
- 💄 Settings save on leaving the field instead of through a save bar
- ⚡️ Skip markets tagged `Crypto Prices`, `Hit Price`, `Tweet Markets` or `Games` (`exclude_tags`): research never made them answerable, and they took most brief spend and candidate slots
- ⚡️ Ask the researcher for low reasoning effort: about half the output tokens
- ✨ The console's OpenRouter spend is all-time from the database and survives restarts
- ✨ Settle markets and briefs that resolved or passed their end date into their own tab in the console
- 💄 Live mode shows green instead of alarm red; the connection indicator is a dot; starting a pass needs no confirmation
- ✨ `setup` creates the deposit wallet Polymarket now requires for API orders: deploys and approves it through the gas-free relayer with builder API credentials, sets `polymarket_wallet` and prints a funding address. Replaces the raw-EOA approvals, which Polymarket refuses
- 🐛 Default `polygon_rpc_url` to PublicNode; polygon-rpc.com now demands an API key
- ✨ Place manual orders from the console, whatever the signal said, inside the money caps; logged with `source = 'manual'`
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
