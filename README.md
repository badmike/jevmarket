# jevmarket

<div align="center">

**A Polymarket trading bot priced by Jev.** A web-search researcher writes a dated evidence brief, [Jev](https://typesafe.ai) turns it into a calibrated probability, and the bot buys where that probability beats the order book, inside hard caps you set. One Rust binary, one OpenRouter key.

[![CI](https://github.com/badmike/jevmarket/actions/workflows/ci.yml/badge.svg)](https://github.com/badmike/jevmarket/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](Cargo.toml)

[How it decides](#how-a-decision-is-made) · [Install](#install) · [Configure](#configuration) · [Commands](#commands) · [Safety](#safety-model)

</div>

> [!WARNING]
> **This software places real orders with real money.** `run` is live by default. Read [Safety model](#safety-model) and [Disclaimer](#disclaimer) first, start with `--dry-run`, and use a wallet holding only what you are willing to lose.

jevmarket is a Rust port of [jevymarket](https://github.com/markusbug/jevymarket) by Markus Haas. The commands, the prompts, the trading math and the database schema are the same. The port is a single static binary with no Python toolchain, keeps its settings in a JSON file in your OS config directory, and does the same pass in less wall-clock time. [What changed in the port](#what-changed-in-the-port) lists every difference.

## Why Jev needs a researcher

Jev is TypeSafe AI's "System One" decision model, served through [OpenRouter's Decisions API](https://openrouter.ai/typesafe). It is not a chat model. You send a JSON `state` and typed questions, and it answers with probabilities: no prose, no chain of thought, around 100 ms and a few thousandths of a cent per decision.

Jev has no browsing and a training cutoff. Asked about a news-driven market on its own, it correctly says it cannot tell: on a live market in September 2026 it rated the question `answerable=0.27`. With a sourced brief in the state, the same question scored `0.78`. So jevmarket pairs Jev with a cheap generative model that searches the web and reports dated facts, never a probability. Jev makes the call, the researcher only supplies the evidence.

## How a decision is made

```
 scan ──▶ research ──▶ state ──▶ Jev ──▶ evaluate ──▶ execute ──▶ log
 Gamma +   chat model   compact   3 typed  edge, band,   GTC limit   SQLite
 CLOB      + web search JSON      answers  Kelly, gates  + hard caps
```

1. **Scan** (`markets.rs`). Open markets by 24h volume from the Gamma API, filtered by liquidity, volume, days to resolution, spread, price band and Yes/No outcome labels. Order books for a whole page of markets arrive in one batched CLOB request. The survivors are ranked by where research can pay off: markets with no ask inside the trade band go last, then sooner resolution and thinner books (under about $100k liquidity) come first.
2. **Research** (`research.rs`). Before paying for a brief, one clarity-only Jev call on the state without evidence (about $0.00003) screens out markets whose resolution criteria score below `min_clarity`. Then a chat model with OpenRouter's web plugin returns strict JSON: as-of date, summary, dated key facts (newest first), latest development, scheduled events that could settle the question, what the named resolution source currently shows, considerations for and against YES, sources. Prediction-market and odds sites are excluded from search, so market prices never reach Jev as "evidence". Briefs are cached in SQLite for `research_ttl_hours` together with the midpoint at the time; once the midpoint moves more than `research_max_price_move`, the market has likely seen news and is researched again.
3. **State** (`markets.rs`). `question`, truncated `description`, `today`, `days_until_resolution` (pre-computed, Jev is bad at date math), `resolution_source`, `market_implied_probability_yes` (unless `jev_sees_market_price` is off) and `evidence`. Nothing else: Jev reads literally and gets worse with irrelevant context. When the evidence is over `research_max_chars`, considerations are trimmed first, for and against YES in turn so the trim never tilts the brief, then the oldest key facts, then the latest scheduled events.
4. **Ask Jev** (`signal.rs`). One call, three questions:
   - `resolves_yes` (noul): P(YES)
   - `answerable` (noul): does the state hold enough current information for a well-informed estimate? This is information sufficiency, not certainty.
   - `clarity` (score 0 to 4): how objective the resolution criteria are
5. **Evaluate** (`signal.rs`, a pure function). Skip unless answerable and clear. Consider only contracts whose ask sits inside `[min_trade_price, max_trade_price]`. Pick the side with the larger edge and require `p − ask ≥ min_edge`. Size with fractional Kelly against `max_open_exposure_usd`, capped at `max_usd_per_trade`, rounded to the market's tick and minimum size.
   A trade signal on a cached brief is researched again and re-evaluated, so orders only go out on evidence from this pass (`pipeline.rs`). Edges above `suspicious_edge` are logged and skipped: after fresh research that big a gap is more often the model than the market.
6. **Execute** (`executor.rs`). A GTC limit buy at the best ask, placed only if every cap passes. This is the only module that can spend money.
7. **Log** (`store.rs`). Every brief, decision and order goes to SQLite. Each `run` pass first fetches the outcomes of logged markets that have resolved (one Gamma request per 50 markets), so `stats` can score Jev against real results.

## Install

From source (Rust 1.88 or newer):

```bash
cargo install --git https://github.com/badmike/jevmarket
```

Or clone and build:

```bash
git clone https://github.com/badmike/jevmarket && cd jevmarket
cargo build --release          # binary at target/release/jevmarket
```

Tagged releases attach prebuilt binaries for Linux, macOS (Intel and Apple Silicon) and Windows.

You need two things before it can trade:

- **An OpenRouter key.** One key covers Jev and the researcher. Jev has no free tier, so the account needs prepaid credits.
- **A funded Polymarket wallet.** See [Funding](#funding). Raw EOAs run `jevmarket setup` once for the on-chain approvals.

## Quick start

```bash
jevmarket init
```

`init` walks you through everything in about five minutes and checks each answer live:

1. **OpenRouter.** Links to sign-up, credits and key creation. The key is verified against OpenRouter right away, and you are warned if the account has never bought credits (Jev has no free tier).
2. **Models.** Pick the Jev version (pinned or latest) and the researcher from the tested models, shown with live per-token prices, or search OpenRouter's whole catalog. One optional test decision (about $0.00003) proves Jev answers.
3. **Polymarket.** Checks whether Polymarket allows trading from your location, then guides you through either a polymarket.com account (key export, wallet address, wallet type detected and explained) or your own wallet (USDC.e, wrapping to pUSD, gas). Optionally connects and shows your pUSD balance. You can also skip this and only dry-run for now.
4. **Risk limits.** Per-trade cap, total exposure cap, orders per run, and dry-run mode, which is always offered as the default.

Nothing is written until you confirm the summary at the end; Esc or Ctrl-C leaves your config untouched. Keys go into the config file (owner-only permissions) or, if you prefer, stay in environment variables. Re-run `init` any time: current values become the defaults and Enter keeps them.

Then:

```bash
jevmarket scan -n 15                      # what the bot would look at right now
jevmarket decide <slug|url>               # research + Jev + proposed trade, never trades
jevmarket run --dry-run                   # the full pipeline, orders only logged
jevmarket setup                           # raw EOA only: exchange approvals
jevmarket config set dry_run false        # when `stats` convinces you
jevmarket run --max-trades 1              # LIVE: at most one real order
```

Prefer scripting? Every answer is a plain config key, see [Configuration](#configuration).

## Configuration

Settings live in one JSON file in the standard location for command-line tools:

| OS | Config file | Database |
|---|---|---|
| Linux | `~/.config/jevmarket/config.json` | `~/.local/share/jevmarket/jevmarket.db` |
| macOS | `~/.config/jevmarket/config.json` | `~/.local/share/jevmarket/jevmarket.db` |
| Windows | `%APPDATA%\coderscantina\jevmarket\config\config.json` | `%APPDATA%\coderscantina\jevmarket\data\jevmarket.db` |

`XDG_CONFIG_HOME` and `XDG_DATA_HOME` are honored. `--config <path>` or `JEVMARKET_CONFIG` points at a different file, which is handy for keeping a dry-run profile next to a live one.

```bash
jevmarket config path                     # where the files are
jevmarket config init                     # write a file with every default, to edit by hand
jevmarket config set min_edge 0.1         # values are parsed as JSON, else taken as text
jevmarket config set polymarket_wallet null    # null removes a key: back to its default
jevmarket config show                     # effective settings, secrets masked
```

`config set` validates the whole file before writing it: an unknown key or a wrong type is rejected with the list of valid keys, so a typo cannot silently fall back to a default. It writes only the keys you set, so everything else keeps following the built-in defaults when they change. The file is created with mode `0600` because it can hold a private key.

Secrets can stay out of the file entirely. These environment variables override the matching keys:

| Variable | Key |
|---|---|
| `OPENROUTER_API_KEY` | `openrouter_api_key` |
| `POLYMARKET_PRIVATE_KEY` | `polymarket_private_key` |
| `POLYMARKET_WALLET` | `polymarket_wallet` |

### All settings

| Key | Default | Meaning |
|---|---|---|
| **Keys** | | |
| `openrouter_api_key` | `""` | OpenRouter key for Jev and the researcher |
| `polymarket_private_key` | `""` | 32-byte hex key of the signer. Without it, only dry runs work |
| `polymarket_wallet` | `null` | Your Polymarket wallet if you funded via polymarket.com; `null` for a raw EOA |
| **Endpoints** | | |
| `openrouter_base_url` | `https://openrouter.ai/api` | |
| `clob_host` | `https://clob.polymarket.com` | Polymarket order book API |
| `polygon_rpc_url` | `https://polygon-rpc.com` | Only used by `setup` |
| **Jev** | | |
| `jev_model` | `typesafe/jev-1.13` | Pin a Jev version; `typesafe/jev-latest` also works |
| `jev_sees_market_price` | `true` | Put the market midpoint into the Jev state. `stats` compares Brier scores with and without it |
| **Researcher** | | |
| `research_enabled` | `true` | `false` runs Jev alone (expect near-zero trades) |
| `research_model` | `deepseek/deepseek-v4-pro-0813` | Any OpenRouter chat model, see below |
| `research_ttl_hours` | `6` | Reuse a cached brief for this long |
| `max_research_per_run` | `20` | Hard cap on researcher calls per `run` pass |
| `research_max_results` | `8` | Web search results per brief |
| `research_max_chars` | `2500` | Size limit of the evidence block in the Jev state |
| `research_exclude_domains` | 12 odds sites | Domains the web search must never return |
| `concurrency` | `4` | Markets researched and priced in parallel during `run` |
| `research_max_price_move` | `0.05` | Research a cached brief again once the midpoint moved more than this since it was written |
| **Signal** | | |
| `min_edge` | `0.08` | Required `P_jev − best ask` |
| `min_answerable` | `0.70` | Jev's belief that the state has enough information |
| `min_clarity` | `2` | 0 to 4 score of the resolution criteria |
| `min_trade_price` / `max_trade_price` | `0.10` / `0.90` | Only buy contracts priced in this band |
| `suspicious_edge` | `0.25` | Skip edges above this even on fresh evidence, as likely model errors |
| **Market filter** | | |
| `min_liquidity_usd` | `5000` | |
| `min_volume_usd` | `10000` | |
| `max_days_to_resolution` | `60` | |
| `max_spread` | `0.06` | Checked against Gamma and again against the live book |
| `min_market_price` / `max_market_price` | `0.03` / `0.97` | Skip markets already priced at the extremes |
| `description_max_chars` | `1500` | Description length in the Jev state |
| **Hard caps** | | |
| `max_usd_per_trade` | `5` | Per-order notional cap |
| `max_open_exposure_usd` | `50` | Positions plus open buy orders may not exceed this; also the Kelly bankroll |
| `max_trades_per_run` | `3` | Orders per `run` pass |
| `kelly_fraction` | `0.25` | Fraction of full Kelly used for sizing |
| **Misc** | | |
| `dry_run` | `false` | Make every `run` a dry run |
| `db_path` | `null` | SQLite file; `null` uses the data directory above |
| `log_level` | `info` | Log filter for jevmarket; `RUST_LOG` wins when set |

**Researcher models.** Any OpenRouter chat model works. Models without native search get OpenRouter's Exa search ($0.007 per search), which honors the odds-site exclusion list. The original project tested `deepseek/deepseek-v4-pro-0813` (default), `z-ai/glm-5.3`, `qwen/qwen3.8-max-0902` and `moonshotai/kimi-k3`, plus `anthropic/claude-sonnet-5` for native search at about twice the cost.

**Costs** (September 2026 prices). Jev: about $0.00006 per decision with evidence. A brief: $0.010 to $0.020. A 20-market pass: $0.20 to $0.40 before caching, a fraction of that when briefs are still fresh. `run` prints the spend after every pass.

## Commands

| Command | What it does |
|---|---|
| `init` | Guided setup: OpenRouter key, models, Polymarket wallet, risk limits. See [Quick start](#quick-start) |
| `jev-test [--raw]` | One hard-coded Jev call. Prints the raw response, then the parsed answers. Re-run it after a Jev version bump |
| `scan [-n 15] [--pages 5]` | Candidate markets that pass the filters, with live best bid and ask |
| `research <slug\|url> [--fresh]` | Researcher only: print the full evidence brief with sources |
| `decide <slug\|url> [--show-state] [--no-research] [--fresh]` | Research, ask Jev, show the proposed trade. Never places orders, always logs the decision |
| `run [--dry-run] [--max-trades N] [-n 20] [--loop SECS] [--no-research]` | The full pipeline. **Live by default** |
| `daemon [--dry-run] [--loop SECS] [--bind ADDR] [--base-path PATH]` | The `run` loop as a long-running process with a live web console. Live by default, but a live daemon starts paused until you resume it in the console. See [docs/daemon.md](docs/daemon.md) |
| `resolve` | Fetch outcomes of decided or ordered markets that resolved since the last check. `run` does this at the start of every pass |
| `setup` | One-time pUSD and outcome-token approvals for the exchange contracts (raw EOA only) |
| `positions` | pUSD balance, open positions, open orders, current exposure against the cap |
| `stats` | Decision and order counts, spend, Jev P(yes) buckets, calibration on resolved markets, and PnL. See below |
| `config path\|init\|show\|set` | See [Configuration](#configuration) |

Markets can be given as a slug or a polymarket.com URL: `/event/<event>/<market>`, `/market/<market>`, or `/event/<event>` when the event has a single market. For multi-market events the error lists the market slugs to choose from.

`stats` scores Jev on resolved markets, using each market's latest decision with a Jev probability so markets seen on every pass count once. It reports the Brier score of Jev's P(yes) and of the market midpoint at decision time (lower is better), and the hit rate of the side with the larger edge, split by `jev_sees_market_price` variant, by edge, by `answerable` and by clarity. PnL is reported for live orders and, separately, for the hypothetical dry-run orders, both assuming each order filled at its limit price. `positions` shows what the exchange actually filled.

`run --loop 900` repeats every 15 minutes until Ctrl-C. A failed pass (an API hiccup, a network drop) is reported and the loop carries on; a rejected OpenRouter key or exhausted credits stop it, since every further call would fail the same way.

## Safety model

Every order has to pass all of these, in this order:

1. **Signal gates** (`signal.rs`): `answerable ≥ min_answerable`, `clarity ≥ min_clarity`, ask inside the trade band, `edge ≥ min_edge`.
2. **Fresh evidence** (`pipeline.rs`): a signal on a cached brief is researched again and has to pass the gates a second time; `edge ≤ suspicious_edge`.
3. **Sizing**: fractional Kelly, capped at `max_usd_per_trade`. If the market's minimum order size would push the order past 1.5× that cap, the trade is refused.
4. **Executor checks** (`executor.rs`), re-evaluated immediately before each order:
   - no more than `max_trades_per_run` orders this pass
   - no second order on a market the database already has a live order for
   - no order on a market where the wallet already holds a position or an open order
   - positions plus open buy orders plus this order stay under `max_open_exposure_usd`

Orders are GTC limit buys at the best ask. There is no selling, no stop-loss and no re-pricing: positions are held to resolution. The caps are the only brake, so set them to amounts you can lose.

`--dry-run` runs everything except the order itself and logs the intended order with `dry_run = 1`. Without `polymarket_private_key`, dry runs still work and read exposure from `polymarket_wallet` if you set it.

## Funding

Since the April 2026 exchange upgrade Polymarket settles in **pUSD** (`0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB` on Polygon), a 1:1 USDC-backed token. Not USDC.e, not native USDC.

1. **Via polymarket.com (easiest).** Deposit on the site; it arrives as pUSD in your Polymarket wallet. Set `polymarket_private_key` to the key of the wallet you log in with and `polymarket_wallet` to your Polymarket wallet address. jevmarket works out the wallet type by itself: it compares the address against the Safe and proxy wallets Polymarket derives from your key, and treats anything else as a deposit wallet. No `setup` needed, Polymarket manages the approvals.
2. **Raw EOA.** Leave `polymarket_wallet` empty. Hold USDC.e (`0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174`), approve the CollateralOnramp (`0x93070a847efEf7F70739046A929D47a521F5B8ee`) and call `wrap(USDC_E, you, amount)`. Keep a little POL for gas, then run `jevmarket setup` once.

`jevmarket positions` shows the pUSD balance the exchange sees.

## What changed in the port

Same behavior:

- Command names, flags and defaults, including `run` being live by default.
- The Jev questions and the clarity rubric, word for word.
- Edge, band, Kelly and tick-rounding math. The Python test cases are ported one to one.
- The SQLite schema, extended only by adding: a `resolutions` table and a `midpoint` column on `research`, created on first open. Point `db_path` at an existing `jevymarket.db` and `stats` keeps counting, cached briefs included.

Different, on purpose:

- **`jevmarket init`** replaces copying `.env.example`: a guided setup that checks the OpenRouter key, Jev, your location and your wallet as you go.
- **Configuration is a JSON file** in the OS config directory instead of a `.env` in the working directory, so the binary behaves the same from any folder. `config set` validates keys and types.
- **Faster passes.** Order books for a page of up to 50 markets come from one batched request instead of one request per market, and Gamma is asked to drop markets resolving after `max_days_to_resolution` instead of paging through them. During `run`, research and Jev calls for the next `concurrency` markets run while the current one is evaluated. Orders are still placed one at a time in scan order, so every cap sees every earlier fill. The look-ahead can spend up to `concurrency − 1` extra briefs after the trade cap is hit; they are cached and reused on the next pass.
- **One HTTP client** with connection reuse for Jev and the researcher. Network errors are retried with backoff like 429 and 5xx responses.
- **`jev-test` makes one call** instead of two (the original called Jev again to parse what it had already printed), and uses today's date.
- **A rejected order no longer ends the run.** An exchange error on one order is logged as `rejected` and the pass continues.
- **`--loop` survives a failed pass** and stops cleanly on Ctrl-C.
- **Wallet type detection** from the address, as described in [Funding](#funding).
- **`setup` skips approvals already granted** and refreshes the exchange's balance cache afterwards.
- **polymarket.com URLs** for single-market events resolve to the market, and multi-market events list their market slugs.
- **Research spent where it can pay.** A clarity pre-screen before each paid brief, and candidates ranked by opportunity instead of 24h volume.
- **Richer briefs.** The researcher also reports scheduled events and the current state of the resolution source, and returns key facts newest first. Trimming for size is balanced between for and against YES; the original dropped every point against YES before any point for it.
- **No trades on stale evidence.** Cached briefs are dropped when the price moves, trade signals on cached briefs are researched again, and very large edges are skipped.
- **Real calibration.** `resolve` records outcomes; `stats` reports Brier scores, hit rates and PnL.

## Known limitations

- **Longshot bias.** Jev tends to be less confident than the market at the extremes, which shows up as "edge" on 5-cent contracts. The trade band is the guard; widen it only with evidence from `stats`.
- **Edge is not profit.** A limit order at the ask takes liquidity. Adverse selection eats into an 8-point threshold. Run `--dry-run`, watch `stats` once markets resolve, then trade small.
- **No exits.** Positions are held to resolution.
- **Beta endpoint.** OpenRouter's Decisions API is in beta. Answers are parsed leniently (`noul`, `probability`, `p` or `value` all work), but re-run `jev-test` after a Jev version bump.
- **Jev reads literally** and degrades with irrelevant context. Keep the state small; every new field needs a reason.

## Architecture

```
src/
  main.rs        CLI (clap): commands, output, the run loop
  config.rs      JSON settings, OS paths, env overrides, `config set` validation
  onboard.rs     `init`: the guided setup wizard
  openrouter.rs  shared HTTP client, retries, error type
  jev.rs         Decisions API client, typed questions, lenient answer parsing
  research.rs    researcher prompt, Brief, JSON extraction from model output
  signal.rs      the three questions, evaluate(): gates, edge, Kelly (pure)
  markets.rs     Gamma scan, filters, batched order books, Jev state
  pipeline.rs    pre-screen, brief cache -> state -> Jev -> evaluate, stale-evidence guard, logging
  executor.rs    caps, order placement, positions, approvals
  store.rs       SQLite log, research cache, resolutions, calibration and PnL
  ui.rs          tables and colored output
```

Polymarket access goes through [`polymarket_client_sdk_v2`](https://crates.io/crates/polymarket_client_sdk_v2), Polymarket's own Rust SDK (Gamma, Data API, CLOB, EIP-712 order signing). On-chain approvals use [`alloy`](https://alloy.rs). OpenRouter is plain HTTPS through `reqwest`; there is no other AI provider.

## Development

```bash
cargo test                      # all tests are offline: OpenRouter is mocked with wiremock
cargo clippy --all-targets
cargo fmt
```

Pure logic (edge, sizing, gates) belongs in `signal.rs` with a test. Anything that can spend money lives in `executor.rs` and goes through `Executor::check`. See [CONTRIBUTING.md](CONTRIBUTING.md).

## Troubleshooting

- **`error sending request for url (https://clob-v2.polymarket.com/...)`**: that migration-era host no longer resolves. Run `jevmarket config set clob_host https://clob.polymarket.com` (the default) or remove the key.
- **"a 0x…40-hex value is an address, not a key"**: you pasted an address into `polymarket_private_key`. A key is 32 bytes, 64 hex characters.
- **`setup` fails with RPC errors**: the public Polygon RPC is flaky. `setup` retries six times and skips approvals already granted. A private RPC in `polygon_rpc_url` helps.
- **`no OpenRouter key`**: run `jevmarket init`, or `jevmarket config set openrouter_api_key ...`, or export `OPENROUTER_API_KEY`.
- **More detail**: `RUST_LOG=jevmarket=debug jevmarket scan` shows why each market was skipped.

## Disclaimer

This software is provided for educational and research purposes. It is not financial advice. Prediction markets carry a risk of total loss. You alone are responsible for your trades, for complying with Polymarket's terms of service and with the laws of your jurisdiction. The authors accept no liability for losses. See [LICENSE](LICENSE).

## Security

See [SECURITY.md](SECURITY.md). The short version: a dedicated low-balance wallet, never share or commit your config file, and the exposure caps are the only brake.

## License

MIT. See [LICENSE](LICENSE).

## Acknowledgements

- Markus Haas for [jevymarket](https://github.com/markusbug/jevymarket), the Python original this port follows
- [TypeSafe AI](https://typesafe.ai) for Jev, and [OpenRouter](https://openrouter.ai) for serving it
- Polymarket for the [Rust SDK](https://github.com/Polymarket/rs-clob-client-v2)

---

<div align="center">
  <p>Maintained by <a href="https://github.com/badmike">Michael Wallner</a> · <a href="https://github.com/coderscantina">Coder's Cantina</a></p>
</div>
