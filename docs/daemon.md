# Daemon and web console

`jevmarket daemon` runs the trading loop as a long-running process and serves a web console for it: live status, recommendations with the evidence behind them, briefings, positions, calibration stats and the configuration. It is the same pipeline, store and settings as `run`, in one binary, plus a price watch between research cycles.

```bash
jevmarket daemon --dry-run                       # console on http://127.0.0.1:8787/
jevmarket daemon --loop 1800 -n 20               # a research cycle every 30 minutes, 20 markets each
jevmarket daemon --bind 127.0.0.1:8787 --base-path /jevmarket   # behind a proxy under /jevmarket/
```

| Flag | Default | Meaning |
|---|---|---|
| `--bind` | `127.0.0.1:8787` | Address to listen on. Keep it on localhost and put a TLS proxy in front for remote access |
| `--loop SECS` | `3600` | Seconds between research cycles. The price watch checks prices in between |
| `--dry-run` | off | Place no orders whatever the config says. The console shows the mode as forced |
| `--autostart` | off | When live, start the loop right away instead of waiting paused for a resume in the console. For services that should trade from boot |
| `-n, --limit` | `20` | Candidate markets per pass, as for `run` |
| `--base-path` | none | URL prefix when a proxy serves the console under a sub-path |

> [!WARNING]
> Like `run`, the daemon is **live unless `--dry-run` is given or `dry_run` is set**. A live daemon starts paused: no order goes out until you resume the loop in the console and type `LIVE` to confirm, unless it was started with `--autostart`. A dry-run daemon starts its first pass right after launch.

## Run it as a service

`jevmarket service install` runs the daemon at login and restarts it after a crash: a launchd agent on macOS, a systemd user unit on Linux. Everything after `install` is passed to `jevmarket daemon`, so the flags above work:

```bash
jevmarket service install --dry-run --loop 600   # install and start; run again to change the flags
jevmarket service restart                        # after upgrading jevmarket
jevmarket service status                         # running or not, and where the logs are
jevmarket service uninstall                      # stop and remove; config and database stay
```

| | macOS | Linux |
|---|---|---|
| Service file | `~/Library/LaunchAgents/com.coderscantina.jevmarket.plist` | `~/.config/systemd/user/jevmarket.service` |
| Logs | `~/Library/Logs/jevmarket.log` | `journalctl --user -u jevmarket -f` |

- **The service reads the config file only.** It does not see your shell's environment, so keys exported as `OPENROUTER_API_KEY` or `POLYMARKET_PRIVATE_KEY` are missing there. `install` warns about each one; store them with `jevmarket init` or `config set`. The config file in use when you run `install` (including `--config` or `JEVMARKET_CONFIG`) is the one the service gets.
- **A live service starts paused** after every start, reboot and restart, like any live daemon. Resume it in the console, or install it with `--autostart` to trade from boot.
- **On Linux**, a user service stops when you log out. `install` tells you when that applies; `sudo loginctl enable-linger $USER` keeps it running.
- **Installed with Homebrew**, the service points at the `bin/jevmarket` link, so upgrades keep working. The running process keeps the old binary until `jevmarket service restart`; open consoles reload themselves when it comes back.

## Install the console on a phone

The console is an installable web app: add it to the home screen and it opens full screen with its own icon, like a native app.

- **iOS and iPadOS**: open the console in Safari, Share, Add to Home Screen.
- **Android**: open it in Chrome, menu, Install app (or Add to home screen).

Browsers only install web apps served over HTTPS, so put the daemon behind a TLS proxy as described in [Behind nginx with TLS](#behind-nginx-with-tls), and install from that address. The app needs the daemon to be reachable: it shows live data and does nothing offline.

## What it does

- **Research cycles** (passes) run every `--loop` seconds, exactly as `run --loop` does: resolutions are refreshed, markets scanned and ranked, decided `concurrency` at a time, and orders placed one at a time under the hard caps. Settings are read from the config file at the start of every cycle, so changes made in the console apply to the next one.
- **The price watch** checks prices every `watch_interval_secs` in between, see [Price watch](#price-watch).
- **A failed pass** is shown in the console and the loop carries on. A rejected OpenRouter key or exhausted credits pause the loop instead, since every further pass would fail the same way; resume it once the cause is fixed.
- **Pause** stops research cycles and every order, and ends a running pass or watch tick after its current market. An order in flight is always finished and logged. The price watch keeps checking prices while paused, so the watchlist stays current, but it acts on no signal.
- **Shutdown** on Ctrl-C or SIGTERM works the same way: the current market finishes, the event streams close, the process exits.
- **Decide** and **Refresh brief** in the console do what `decide` and `research --fresh` do. Deciding never places an order.
- **Order** on a market's sheet places a manual limit BUY on the current book, whatever the signal said: pick the outcome, limit price and amount. It overrides the one-order-per-market rule and the per-pass trade count, never `max_usd_per_trade` or `max_open_exposure_usd`, and follows the effective dry-run setting. Manual orders are logged in `orders` with `source = 'manual'` (the bot writes `bot`) and marked in the order log and activity feed.
- Cancelling open orders is not offered: the executor cannot cancel orders.

Without an OpenRouter key the console, positions (for a configured `polymarket_deposit_wallet`) and all stored data still work; passes and decisions fail with the missing-key message until one is set.

## Price watch

A research cycle is expensive and runs hourly by default. Prices move in between, so the daemon keeps checking them against what Jev already concluded:

- **What is watched.** Each market's latest decision, when it has a Jev probability, passed the `min_answerable` and `min_clarity` gates, is younger than `research_ttl_hours`, the market has not settled, and the bot holds no position or order on it. In dry runs a dry-run order counts too, so the watch does not buy the same market every tick. The views come from the database, so they survive restarts.
- **Your watchlist.** Markets you add in the console (Watch on a market's sheet, or a link on the Watchlist tab) are watched whatever their view's age and gates, until you take them off. A market Jev never priced is decided first. A market too unclear to price is refused.
- **Held positions.** With `sell_early` on, every tick also reads the wallet's open positions from Polymarket's public positions API and holds each one Jev priced against its best bid, see [Selling early](#selling-early).
- **A tick** every `watch_interval_secs` seconds (default 60) loads the watched markets from Gamma and their order books in batched requests, one each per 50 markets, and runs the same `evaluate` and suspicious-edge check a pass uses on the stored view. A tick without a trade signal calls no model at all.
- **On a trade signal**, in this order:
  1. The executor's checks run first. If the order would be refused anyway (trade cap, exposure cap, already ordered), nothing is logged or paid for.
  2. If the brief behind the view is older than `trade_brief_max_age_minutes` (default 120), or there is none, the market is researched again and put to Jev, like a pass does with a cached brief. When the research budget of this cycle is used up, the signal waits for the next cycle.
  3. Otherwise, if `jev_sees_market_price` is on and the midpoint moved by at least a tick (0.01) since the view, Jev is asked again with the live price (about $0.0001).
  4. The verdict is evaluated again, the order placed through the executor and the decision logged, so the console, stats and order log see it. A signal that was re-researched into a skip is logged too.
- **Only signals are logged.** A tick logs a decision only when Jev was asked again, research ran or an order was attempted. When research or Jev fails, for example without an OpenRouter key, the signal is logged as a skip (`stale_brief` or `stale_view`) and the market drops out of the watch until a research cycle decides it again.
- **Shared caps.** `max_trades_per_run` and `max_research_per_run` count per research-cycle window: the cycle and the watch share them until the next cycle starts. Per-trade and exposure caps apply to every order as always.
- **One at a time.** A tick never runs during a research cycle; the next one is due `watch_interval_secs` after the cycle ends. A cycle that falls due during a tick starts right after it. `0` turns the watch off. Changes to either setting apply from the next tick.

The watch is daemon-only. `jevmarket run --loop` keeps its single cadence.

### Selling early

A position bought on an edge normally pays out at resolution. The watch sells it earlier only when both hold for the held outcome:

- **The market pays more than Jev expects.** The best bid is at least `min_exit_edge` (default 0.05) above Jev's probability of that outcome. A share that Jev expects to pay $0.58 at resolution is worth selling at $0.63 now.
- **The sale is a real profit.** The bid returns at least `min_exit_profit` (default 20%) on the average price paid.

On an exit signal the view is renewed like a buy signal's (research when the brief is older than `trade_brief_max_age_minutes`, Jev again after a price move), then the rule is checked again. If it still holds, the whole position is sold with a GTC limit SELL at the bid and logged as a `sell` decision and a SELL order; if not, the fresh view is logged as `hold`. A position the bot sold (or dry-run sold) is not sold again. Sells do not count against `max_trades_per_run` or the money caps: they take risk off. Stats count a sale's proceeds against the stake of its market.

## Safety without a login

The console has no login. These rules stand in for one:

- **Secrets never leave the process.** The API reports only whether `openrouter_api_key` and `polymarket_private_key` are set. They can be replaced or cleared from the console, never read.
- **Real money needs a typed confirmation.** Switching `dry_run` off, resuming the loop or placing a manual order while dry runs are off is refused with `428` unless the request carries `"confirm": "LIVE"`. The console asks you to type it.
- **Writes must come from the console's own origin.** Requests that change anything are refused when `Origin` differs from the host the request was sent to, or when the browser marks them `Sec-Fetch-Site: cross-site`. They must also be JSON, which a page on another site cannot send without a CORS preflight the daemon never answers.
- **Unknown host names are refused.** Without a proxy, the daemon only answers to `localhost` and IP addresses, which stops DNS rebinding attacks from a web page. Behind a proxy, the proxy must set `X-Forwarded-Host`.
- **Config changes go through the same validation as `config set`**, with errors reported per setting.

Anyone who can reach the console can change the configuration and, after typing `LIVE`, trade. Restrict who can reach it.

## Behind nginx with TLS

Keep the daemon on `127.0.0.1` and let nginx terminate TLS. This block serves it under `/jevmarket/`; start the daemon with `--base-path /jevmarket`:

```nginx
server {
    listen 443 ssl;
    http2 on;
    server_name bot.example.com;

    ssl_certificate     /etc/letsencrypt/live/bot.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/bot.example.com/privkey.pem;

    # There is no login. Allow only your own addresses, or add basic auth below, or both.
    allow 203.0.113.7;
    deny all;
    # auth_basic "jevmarket";
    # auth_basic_user_file /etc/nginx/jevmarket.htpasswd;

    location /jevmarket/ {
        proxy_pass http://127.0.0.1:8787;
        proxy_http_version 1.1;
        proxy_set_header Connection "";
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-Host $host;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;

        # The event stream: no buffering, and a long read timeout. The daemon also sends
        # X-Accel-Buffering: no and a heartbeat every 15 seconds.
        proxy_buffering off;
        proxy_cache off;
        proxy_read_timeout 1h;
    }
}
```

`proxy_pass` has no path, so nginx forwards `/jevmarket/...` unchanged and the daemon strips the prefix itself. To serve the console at the root instead, use `location /` and leave out `--base-path`.

`X-Forwarded-Host` is required: without it the daemon sees a public host name and refuses the request. `X-Forwarded-Proto` must match the scheme the browser uses, or writes are refused as cross-origin.

## API

All endpoints are under `<base-path>/api/`, take and return JSON, and report errors as `{"error": "...", "confirm"?: "LIVE", "fields"?: {"key": "message"}, "choices"?: [...]}`. `choices` lists the open markets of an event link that names no single market (`slug`, `label`, `image`, `price`); send one of their slugs instead. The types live in `src/daemon/api.rs` and are mirrored in `web/src/api/types.ts`.

| Method and path | What it does |
|---|---|
| `GET status` | Loop state, next pass time, current and last pass with spend, all-time spend (from the database, so it survives restarts), dry-run mode, last error, and `watch`: the price watch's interval, last and next tick time, markets watched, signals in the last tick, the last signal and the last tick's error |
| `GET events` | Server-sent events, see below |
| `GET activity` | The last 200 events of the kinds the activity feed shows |
| `GET config` | Effective settings without secrets, their defaults, which keys the environment overrides |
| `PATCH config` | `{"changes": {"key": value or null}, "confirm"?: "LIVE"}`. `null` returns a key to its default |
| `GET recommendations` | The latest decision per market, with best edge, side, `skip_code` (why a skip was a skip), `settled`, and the order placed for it |
| `GET recommendations/{slug}` | One decision with the state Jev saw and the brief behind it |
| `GET briefs` | The latest brief per market, as summaries |
| `GET briefs/{id}` | One brief in full, with freshness and the midpoint then and now |
| `POST briefs/refresh` | `{"reference": "slug or URL"}`: research the market again |
| `POST decide` | `{"reference": "slug or URL", "fresh"?: bool}`: research, ask Jev, log. Never trades |
| `GET orders` | Orders the bot and the console logged, live and dry-run, with the market `title`, `manual` for console orders and the exchange's `message` for rejected ones |
| `POST orders` | `{"reference": "slug or URL", "outcome": "YES" or "NO", "price": 0.42, "usd": 5, "confirm"?: "LIVE"}`: place a manual limit BUY. Refused and rejected orders answer `422` |
| `GET positions` | Balance, positions, open orders (with `slug` and `title` when the local log knows the market) and exposure against the cap, read from Polymarket |
| `GET wallets` | The key's deposit wallet and polymarket.com wallet with their pUSD, read on-chain, plus warnings when the config disagrees |
| `POST transfer` | `{"from": "deposit" or "proxy", "amount_usd": 5 or null}`: move pUSD to the other wallet, `null` moves everything. No typed confirmation: both ends are derived from the key |
| `GET stats` | Everything `stats` shows, plus reliability bins and cumulative PnL over time |
| `GET watchlist` | The markets the last watch tick checked, closest to a signal first: slug, question, Jev's `p_yes`, `view_at`, `brief_at`, the live `midpoint`, `end_date`, `pinned` (on your watchlist), and per side (`yes`, `no`) the `ask`, the `trigger` (the highest ask the signal buys at, `null` when no ask in the trade band could trigger) and the `distance` (`ask - trigger`, zero or below is a signal). Held positions come with `position`: outcome, size, average price, `bid`, the `trigger` bid it sells at and the `distance` (`trigger - bid`) |
| `POST watchlist` | `{"reference": "slug or URL"}`: put a market on your watchlist, deciding it first when Jev never priced it |
| `DELETE watchlist/{slug}` | Take a market off your watchlist |
| `POST pass` | `{}`: start a pass now. No confirmation: it brings the next scheduled pass forward |
| `POST loop/pause`, `POST loop/resume` | `{}` and `{"confirm"?: "LIVE"}` |

### Events

`GET events` is a standard `text/event-stream`. Every message is one JSON object in `data`, tagged by `type`: `status`, `pass_started`, `pass_finished`, `decision`, `order`, `brief`, `positions_changed`, `stats_changed`, `watchlist_changed` (a watch tick finished; its summary is in `status`), `config_changed`, `log` (jevmarket's own log lines at info and above) and `resync`. A client that falls too far behind receives `resync` and should read every snapshot again; the console does the same after each reconnect, which it retries with backoff from one second up to thirty.

## Building the console

The console is a Vue app in `web/`. Its build, `web/dist`, is committed and embedded into the binary at compile time, so `cargo build` and `cargo install --git` never need Bun. After changing anything in `web/`, rebuild and commit `web/dist` with it; CI fails when the two drift apart:

```bash
cd web && bun install && bun run build
```

For development, run the daemon and `bun run dev` in `web/`; Vite proxies `/api` to `http://127.0.0.1:8787` (or `JEVMARKET_URL`).
