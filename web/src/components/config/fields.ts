/**
 * Labels, help and grouping for the settings, mirroring the table in README.md. A key the
 * server sends that is not listed here still shows up, under "Other", with its input picked
 * from the type of its default.
 */
export interface FieldMeta {
  label: string
  help?: string
  /** Whole numbers only. */
  integer?: boolean
  /** `null` when left empty. */
  nullable?: boolean
}

export interface FieldGroup {
  title: string
  description?: string
  fields: Record<string, FieldMeta>
}

export const groups: FieldGroup[] = [
  {
    title: 'Keys',
    description: 'Write-only. The console shows whether a key is set, never the key.',
    fields: {
      openrouter_api_key: { label: 'OpenRouter API key', help: 'For Jev and the researcher' },
      polymarket_private_key: {
        label: 'Polymarket private key',
        help: '32-byte hex key of the signer. Without it, only dry runs work',
      },
      polymarket_wallet: {
        label: 'Polymarket wallet',
        help: 'Your Polymarket wallet if you funded via polymarket.com; empty for a raw EOA',
        nullable: true,
      },
    },
  },
  {
    title: 'Hard caps',
    description: 'Enforced right before every order. They are the only brake.',
    fields: {
      max_usd_per_trade: { label: 'Max per trade (USD)', help: 'Per-order notional cap' },
      max_open_exposure_usd: {
        label: 'Max open exposure (USD)',
        help: 'Positions plus open buy orders may not exceed this; also the Kelly bankroll',
      },
      max_trades_per_run: { label: 'Max trades per pass', integer: true },
      kelly_fraction: { label: 'Kelly fraction', help: 'Fraction of full Kelly used for sizing' },
    },
  },
  {
    title: 'Signal',
    fields: {
      min_edge: { label: 'Min edge', help: 'Required P(Jev) minus best ask' },
      min_answerable: { label: 'Min answerable', help: "Jev's belief that the state has enough information" },
      min_clarity: { label: 'Min clarity', help: '0 to 4 score of the resolution criteria', integer: true },
      min_trade_price: { label: 'Min trade price', help: 'Only buy contracts priced at or above this' },
      max_trade_price: { label: 'Max trade price', help: 'Only buy contracts priced at or below this' },
      suspicious_edge: {
        label: 'Suspicious edge',
        help: 'Skip edges above this even on fresh evidence, as likely model errors',
      },
    },
  },
  {
    title: 'Jev',
    fields: {
      jev_model: { label: 'Jev model', help: 'Pin a Jev version; typesafe/jev-latest also works' },
      jev_sees_market_price: {
        label: 'Jev sees the market price',
        help: 'Put the market midpoint into the Jev state. Stats compare Brier scores with and without it',
      },
    },
  },
  {
    title: 'Researcher',
    fields: {
      research_enabled: { label: 'Research', help: 'Off runs Jev alone: expect near-zero trades' },
      research_model: { label: 'Research model', help: 'Any OpenRouter chat model' },
      research_ttl_hours: { label: 'Brief lifetime (hours)', help: 'Reuse a cached brief for this long' },
      max_research_per_run: { label: 'Max briefs per pass', integer: true },
      research_max_results: { label: 'Search results per brief', integer: true },
      research_max_chars: {
        label: 'Evidence size (characters)',
        help: 'Size limit of the evidence block in the Jev state',
        integer: true,
      },
      research_exclude_domains: {
        label: 'Excluded domains',
        help: 'One per line. Odds sites the web search must never return',
      },
      research_max_price_move: {
        label: 'Max price move',
        help: 'Research a cached brief again once the midpoint moved more than this since it was written',
      },
      concurrency: { label: 'Concurrency', help: 'Markets researched and priced in parallel', integer: true },
    },
  },
  {
    title: 'Market filter',
    fields: {
      min_liquidity_usd: { label: 'Min liquidity (USD)' },
      min_volume_usd: { label: 'Min volume (USD)' },
      max_days_to_resolution: { label: 'Max days to resolution', integer: true },
      max_spread: { label: 'Max spread', help: 'Checked against Gamma and again against the live book' },
      min_market_price: { label: 'Min market price', help: 'Skip markets already priced at the extremes' },
      max_market_price: { label: 'Max market price' },
      description_max_chars: {
        label: 'Description size (characters)',
        help: 'Description length in the Jev state',
        integer: true,
      },
    },
  },
  {
    title: 'Endpoints',
    fields: {
      openrouter_base_url: { label: 'OpenRouter base URL' },
      clob_host: { label: 'CLOB host', help: 'Polymarket order book API' },
      polygon_rpc_url: { label: 'Polygon RPC URL', help: 'Only used by setup' },
    },
  },
  {
    title: 'Misc',
    fields: {
      dry_run: { label: 'Dry run', help: 'Log orders instead of placing them' },
      db_path: {
        label: 'Database file',
        help: 'Empty uses the data directory. The daemon keeps the file it started with until restarted',
        nullable: true,
      },
      log_level: { label: 'Log level', help: 'Takes effect on restart; RUST_LOG wins when set' },
    },
  },
]
