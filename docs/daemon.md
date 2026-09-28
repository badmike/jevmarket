# Daemon and web console

`jevmarket daemon` runs the trading loop as a long-running process and serves a web console for it: live status, recommendations with the evidence behind them, briefings, positions, calibration stats and the configuration. It is the same pipeline, store and settings as `run`, in one binary.

```bash
jevmarket daemon --dry-run                       # console on http://127.0.0.1:8787/
jevmarket daemon --loop 900 -n 20                # a pass every 15 minutes, 20 markets each
jevmarket daemon --bind 127.0.0.1:8787 --base-path /jevmarket   # behind a proxy under /jevmarket/
```

| Flag | Default | Meaning |
|---|---|---|
| `--bind` | `127.0.0.1:8787` | Address to listen on. Keep it on localhost and put a TLS proxy in front for remote access |
| `--loop SECS` | `900` | Seconds between passes |
| `--dry-run` | off | Place no orders whatever the config says. The console shows the mode as forced |
| `-n, --limit` | `20` | Candidate markets per pass, as for `run` |
| `--base-path` | none | URL prefix when a proxy serves the console under a sub-path |

> [!WARNING]
> Like `run`, the daemon is **live unless `--dry-run` is given or `dry_run` is set**. A live daemon starts paused: no order goes out until you resume the loop in the console and type `LIVE` to confirm. A dry-run daemon starts its first pass right after launch.

## What it does

- **Passes** run every `--loop` seconds, exactly as `run --loop` does: resolutions are refreshed, markets scanned and ranked, decided `concurrency` at a time, and orders placed one at a time under the hard caps. Settings are read from the config file at the start of every pass, so changes made in the console apply to the next one.
- **A failed pass** is shown in the console and the loop carries on. A rejected OpenRouter key or exhausted credits pause the loop instead, since every further pass would fail the same way; resume it once the cause is fixed.
- **Pause** stops scheduling and ends a running pass after its current market. An order in flight is always finished and logged.
- **Shutdown** on Ctrl-C or SIGTERM works the same way: the current market finishes, the event streams close, the process exits.
- **Decide** and **Refresh brief** in the console do what `decide` and `research --fresh` do. Deciding never places an order.
- **Order** on a market's sheet places a manual limit BUY on the current book, whatever the signal said: pick the outcome, limit price and amount. It overrides the one-order-per-market rule and the per-pass trade count, never `max_usd_per_trade` or `max_open_exposure_usd`, and follows the effective dry-run setting. Manual orders are logged in `orders` with `source = 'manual'` (the bot writes `bot`) and marked in the order log and activity feed.
- Cancelling open orders is not offered: the executor cannot cancel orders.

Without an OpenRouter key the console, positions (for a configured `polymarket_deposit_wallet`) and all stored data still work; passes and decisions fail with the missing-key message until one is set.

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

All endpoints are under `<base-path>/api/`, take and return JSON, and report errors as `{"error": "...", "confirm"?: "LIVE", "fields"?: {"key": "message"}}`. The types live in `src/daemon/api.rs` and are mirrored in `web/src/api/types.ts`.

| Method and path | What it does |
|---|---|
| `GET status` | Loop state, next pass time, current and last pass with spend, all-time spend (from the database, so it survives restarts), dry-run mode, last error |
| `GET events` | Server-sent events, see below |
| `GET activity` | The last 200 events of the kinds the activity feed shows |
| `GET config` | Effective settings without secrets, their defaults, which keys the environment overrides |
| `PATCH config` | `{"changes": {"key": value or null}, "confirm"?: "LIVE"}`. `null` returns a key to its default |
| `GET recommendations` | The latest decision per market, with best edge, side and the order placed for it |
| `GET recommendations/{slug}` | One decision with the state Jev saw and the brief behind it |
| `GET briefs` | The latest brief per market, as summaries |
| `GET briefs/{id}` | One brief in full, with freshness and the midpoint then and now |
| `POST briefs/refresh` | `{"reference": "slug or URL"}`: research the market again |
| `POST decide` | `{"reference": "slug or URL", "fresh"?: bool}`: research, ask Jev, log. Never trades |
| `GET orders` | Orders the bot and the console logged, live and dry-run, with `manual` set for console orders |
| `POST orders` | `{"reference": "slug or URL", "outcome": "YES" or "NO", "price": 0.42, "usd": 5, "confirm"?: "LIVE"}`: place a manual limit BUY. Refused and rejected orders answer `422` |
| `GET positions` | Balance, positions, open orders and exposure against the cap, read from Polymarket |
| `GET wallets` | The key's deposit wallet and polymarket.com wallet with their pUSD, read on-chain, plus warnings when the config disagrees |
| `POST transfer` | `{"from": "deposit" or "proxy", "amount_usd": 5 or null}`: move pUSD to the other wallet, `null` moves everything. No typed confirmation: both ends are derived from the key |
| `GET stats` | Everything `stats` shows, plus reliability bins and cumulative PnL over time |
| `POST pass` | `{}`: start a pass now. No confirmation: it brings the next scheduled pass forward |
| `POST loop/pause`, `POST loop/resume` | `{}` and `{"confirm"?: "LIVE"}` |

### Events

`GET events` is a standard `text/event-stream`. Every message is one JSON object in `data`, tagged by `type`: `status`, `pass_started`, `pass_finished`, `decision`, `order`, `brief`, `positions_changed`, `stats_changed`, `config_changed`, `log` (jevmarket's own log lines at info and above) and `resync`. A client that falls too far behind receives `resync` and should read every snapshot again; the console does the same after each reconnect, which it retries with backoff from one second up to thirty.

## Building the console

The console is a Vue app in `web/`. Its build, `web/dist`, is committed and embedded into the binary at compile time, so `cargo build` and `cargo install --git` never need Bun. After changing anything in `web/`, rebuild and commit `web/dist` with it; CI fails when the two drift apart:

```bash
cd web && bun install && bun run build
```

For development, run the daemon and `bun run dev` in `web/`; Vite proxies `/api` to `http://127.0.0.1:8787` (or `JEVMARKET_URL`).
