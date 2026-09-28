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
  started_at: number
  state: LoopState
  loop_secs: number
  next_pass_at: number | null
  dry_run: boolean
  dry_run_forced: boolean
  pass: Pass | null
  last_pass: Pass | null
  spend_total: Spend
  last_error: string | null
}

export type Side = 'YES' | 'NO'

/** `trade`, `trade_unexecuted` and `skip` today; the pipeline may log others. */
export type Action = 'trade' | 'trade_unexecuted' | 'skip' | (string & {})

export interface Trade {
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
  reason: string
  trade: Trade | null
  jev_cost: number
  research_cost: number | null
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
  facts: number
  sources: number
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
}

export interface OpenOrder {
  id: string
  market: string
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

export interface OrderEvent {
  ts: number
  slug: string
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
}
