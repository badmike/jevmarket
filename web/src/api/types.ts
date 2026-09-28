/**
 * Wire types of the daemon API. Mirrors `src/daemon/api.rs` (and the `Stats` and `Brief` it
 * embeds from `src/store.rs` and `src/research.rs`) by hand: change both together.
 * Timestamps are unix seconds.
 */

export type LoopState = 'waiting' | 'running' | 'stopping' | 'paused'

export interface Spend {
  jev_calls: number
  jev_usd: number
  briefs: number
  research_usd: number
}

export interface Pass {
  number: number
  started_at: number
  finished_at: number | null
  dry_run: boolean
  candidates: number
  assessed: number
  trades: number
  spend: Spend
  error: string | null
}

export interface Status {
  version: string
  /** Fingerprint of the embedded console; the page reloads when it changes. */
  build: string
  started_at: number
  state: LoopState
  loop_secs: number
  next_pass_at: number | null
  dry_run: boolean
  dry_run_forced: boolean
  pass: Pass | null
  last_pass: Pass | null
  /** All time, as logged in the database, plus everything since the daemon started. */
  spend_total: Spend
  last_error: string | null
  watch: Watch
}

/** The price watch between research cycles. */
export interface Watch {
  /** Seconds between ticks; 0 when the watch is off. */
  interval_secs: number
  last_tick_at: number | null
  /** `null` while the watch is off, the loop paused or a research cycle running. */
  next_tick_at: number | null
  /** Markets checked in the last tick. */
  watched: number
  /** Trade signals in the last tick. */
  signals: number
  last_signal: WatchSignal | null
  /** Why the last tick failed. */
  error: string | null
}

/** A market where the watch found a trade or exit signal. */
export interface WatchSignal {
  ts: number
  slug: string
  question: string
  /** An exit from a held position rather than a buy. */
  sell: boolean
}

/** One outcome of a watched market. */
export interface WatchSide {
  ask: number | null
  /** The highest ask the signal buys at; `null` when no ask in the trade band could trigger. */
  trigger: number | null
  /** `ask - trigger`: how far the ask has to fall. Zero or below is a signal. */
  distance: number | null
}

/** One watched market as the last tick saw it. */
export interface WatchItem {
  slug: string
  question: string
  /** Jev's stored probability of YES. */
  p_yes: number
  /** When Jev gave it. */
  view_at: number
  /** When the brief behind the view was written; `null` without one. */
  brief_at: number | null
  /** Live midpoint. */
  midpoint: number | null
  /** When the market is scheduled to resolve, `YYYY-MM-DD`. */
  end_date: string | null
  /** Buy sides; empty for a held position. */
  yes: WatchSide
  no: WatchSide
  /** The tick found a trade or exit signal here. */
  signal: boolean
  /** A person put the market on the watchlist. */
  pinned: boolean
  /** The wallet holds shares here: the watch looks for an exit instead of a buy. */
  position: WatchPosition | null
  /** The market's thumbnail URL. */
  image: string | null
}

/** A held position the watch may sell early. */
export interface WatchPosition {
  outcome: Side
  size: number
  avg_price: number
  /** Best bid of the held outcome. */
  bid: number | null
  /** The lowest bid the watch sells at; `null` when none below 1 would. */
  trigger: number | null
  /** `trigger - bid`: how far the bid has to rise. Zero or below is a signal. */
  distance: number | null
}

export type Side = 'YES' | 'NO'

/** Why a skip was a skip. Mirrors `SkipCode` in `src/signal.rs`. */
export type SkipCode =
  | 'unclear'
  | 'unanswerable'
  | 'no_asks'
  | 'outside_band'
  | 'small_edge'
  | 'suspicious_edge'
  | 'zero_stake'
  | 'min_order_too_big'
  | 'stale_brief'
  | 'stale_view'

/**
 * `trade`, `trade_unexecuted` and `skip` from buy decisions; `sell`, `sell_unexecuted` and `hold`
 * from the price watch's exits. The pipeline may log others.
 */
export type Action =
  | 'trade'
  | 'trade_unexecuted'
  | 'skip'
  | 'sell'
  | 'sell_unexecuted'
  | 'hold'
  | (string & {})

export type OrderSide = 'BUY' | 'SELL'

export interface Trade {
  side: OrderSide
  outcome: string
  price: number
  size: number
  usd: number
  status: string | null
  dry_run: boolean
}

export interface Recommendation {
  slug: string
  question: string
  condition_id: string
  ts: number
  p_yes: number | null
  answerable: number | null
  clarity: number | null
  yes_ask: number | null
  no_ask: number | null
  midpoint: number | null
  edge: number | null
  side: Side | null
  action: Action
  /** `null` for trades, and for skips of a kind this console does not know yet. */
  skip_code: SkipCode | null
  reason: string
  trade: Trade | null
  jev_cost: number
  research_cost: number | null
  /** The market resolved or its end date passed. */
  settled: boolean
  /** When the market is scheduled to resolve, `YYYY-MM-DD`. */
  end_date: string | null
  /** On the watchlist a person keeps. */
  pinned: boolean
  /** The market's thumbnail URL. */
  image: string | null
}

export interface Brief {
  summary: string
  key_facts: string[]
  latest_development: string
  scheduled_events: string[]
  resolution_source_status: string
  as_of: string
  for_yes: string[]
  against_yes: string[]
  sources: string[]
  model: string
  cost: number
}

export interface BriefRecord {
  id: number
  ts: number
  slug: string
  question: string | null
  cost: number
  fresh: boolean
  midpoint_then: number | null
  midpoint_now: number | null
  /** The market resolved or its end date passed. */
  settled: boolean
  /** The market's thumbnail URL. */
  image: string | null
  brief: Brief
}

export interface BriefSummary {
  id: number
  ts: number
  slug: string
  question: string | null
  as_of: string
  summary: string
  model: string
  cost: number
  fresh: boolean
  settled: boolean
  facts: number
  sources: number
  /** The market's thumbnail URL. */
  image: string | null
}

export interface RecommendationDetail extends Recommendation {
  /** The JSON state Jev was asked about. */
  state: Record<string, unknown>
  brief: BriefRecord | null
}

export interface Position {
  slug: string
  title: string
  outcome: string
  size: number
  avg_price: number
  cur_price: number
  value_usd: number
  pnl_usd: number
  redeemable: boolean
  /** When jevmarket first bought this outcome live, unix seconds; null when bought elsewhere. */
  ordered_at: number | null
  /** The market's thumbnail URL. */
  image: string | null
  /** When the market is scheduled to resolve, `YYYY-MM-DD`. */
  end_date: string | null
}

export interface OpenOrder {
  id: string
  /** Condition id. */
  market: string
  /** The market, when the daemon's log knows it. */
  slug: string | null
  title: string | null
  side: string
  outcome: string
  price: number
  size: number
  matched: number
  status: string
  created_at: number
}

export interface Exposure {
  positions_usd: number
  open_orders_usd: number
  total_usd: number
  cap_usd: number
}

export interface Positions {
  wallet: string | null
  wallet_type: string
  balance_usd: number | null
  positions: Position[]
  open_orders: OpenOrder[]
  exposure: Exposure
}

export type WalletKind = 'deposit' | 'proxy'

export interface WalletState {
  address: string
  balance_usd: number
}

/** The key's two wallets, read on-chain. */
export interface Wallets {
  signer: string
  /** Places the bot's orders. */
  deposit: WalletState
  /** The polymarket.com account. */
  proxy: WalletState
  /** Config entries that disagree with the wallets derived from the key. */
  warnings: string[]
}

export interface Transfer {
  from: WalletKind
  to: WalletKind
  amount_usd: number
  tx_hash: string
}

export interface OrderEvent {
  ts: number
  slug: string
  /** The market question; the slug when unknown. */
  title: string
  side: OrderSide
  outcome: string
  price: number
  size: number
  usd: number
  status: string
  dry_run: boolean
  order_id: string | null
  message: string | null
  /** Placed from the console rather than by the signal. */
  manual: boolean
}

/** A BUY placed from the console, whatever the signal said. The money caps still apply. */
export interface ManualOrder {
  /** Slug or polymarket.com URL. */
  reference: string
  outcome: Side
  /** Limit price. */
  price: number
  /** Roughly what to spend; the daemon rounds the size to the book. */
  usd: number
}

export interface Bucket {
  bucket: number
  n: number
  avg_p: number
  avg_market: number
}

export interface Group {
  label: string
  n: number
  hit_rate: number
  brier_jev: number
  brier_market: number
}

export interface Calibration {
  all: Group
  by_variant: Group[]
  by_edge: Group[]
  by_answerable: Group[]
  by_clarity: Group[]
}

export interface Pnl {
  orders: number
  resolved: number
  staked_usd: number
  payout_usd: number
  pnl_usd: number
}

export interface ReliabilityBin {
  bucket: number
  n: number
  avg_p: number
  avg_market: number
  yes_rate: number
}

export interface PnlPoint {
  ts: number
  dry_run: boolean
  pnl_usd: number
  cumulative_usd: number
}

export interface StatsView {
  decisions: number
  trade_signals: number
  jev_cost_usd: number
  briefs: number
  research_cost_usd: number
  live_orders: number
  live_usd: number
  buckets: Bucket[]
  calibration: Calibration
  live_pnl: Pnl
  dry_run_pnl: Pnl
  reliability: ReliabilityBin[]
  pnl_series: PnlPoint[]
}

export type ConfigValue = string | number | boolean | null | string[]

export interface ConfigView {
  path: string
  values: Record<string, ConfigValue>
  defaults: Record<string, ConfigValue>
  secrets: { key: string; set: boolean }[]
  env_overrides: string[]
  dry_run_forced: boolean
}

export type LogLevel = 'error' | 'warn' | 'info'

export type Event =
  | { type: 'status'; status: Status }
  | { type: 'pass_started'; pass: Pass }
  | { type: 'pass_finished'; pass: Pass }
  | { type: 'decision'; recommendation: Recommendation }
  | { type: 'order'; order: OrderEvent }
  | { type: 'brief'; brief: BriefSummary }
  | { type: 'positions_changed' }
  | { type: 'stats_changed' }
  | { type: 'watchlist_changed' }
  | { type: 'config_changed' }
  | { type: 'log'; ts: number; level: LogLevel; message: string }
  | { type: 'resync' }

export type EventType = Event['type']

/** The body of every non-2xx response. */
export interface ErrorBody {
  error: string
  /** The phrase to repeat in `confirm` when the action places real orders. */
  confirm?: string
  /** Validation errors by setting. */
  fields?: Record<string, string>
  /** The markets of an event link, to pick one from. */
  choices?: MarketChoice[]
}

/** One market of a Polymarket event. */
export interface MarketChoice {
  slug: string
  /** The short name Polymarket gives it inside the event, else its question. */
  label: string
  image: string | null
  /** The last YES price. */
  price: number | null
}
