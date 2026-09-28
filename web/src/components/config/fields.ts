/**
 * Labels, help and grouping for the settings, mirroring the table in README.md. A key the
 * server sends that is not listed here still shows up, under "Other", with its input picked
 * from the type of its default.
 */
export interface FieldMeta {
  label: string
  /** What the setting does, in a sentence or two. */
  help: string
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
      openrouter_api_key: {
        label: 'OpenRouter API key',
        help: 'Pays for Jev and the researcher, the two AI models the bot uses. Create one at openrouter.ai with prepaid credits.',
      },
      polymarket_private_key: {
        label: 'Polymarket private key',
        help: 'The key that signs orders and transfers: 64 hex characters, exported from your Polymarket login. Without it, only dry runs work.',
      },
      polymarket_deposit_wallet: {
        label: 'Deposit wallet',
        help: 'The wallet the bot trades from. Polymarket only takes API orders from a deposit wallet; `jevmarket setup` creates it and fills this in.',
        nullable: true,
      },
      polymarket_proxy_wallet: {
        label: 'Polymarket.com wallet',
        help: 'Your account wallet on polymarket.com, the other end of transfers. Leave empty to use the one that belongs to your key.',
        nullable: true,
      },
      polymarket_builder_api_key: {
        label: 'Builder API key',
        help: "Lets Polymarket's relayer run wallet setup and transfers without gas. Create it on polymarket.com under Settings, Builder.",
      },
      polymarket_builder_secret: {
        label: 'Builder secret',
        help: 'Shown once when you create the builder API key. Signs relayer requests.',
      },
      polymarket_builder_passphrase: {
        label: 'Builder passphrase',
        help: 'Shown once when you create the builder API key. Sent with every relayer request.',
      },
    },
  },
  {
    title: 'Hard caps',
    description: 'Enforced right before every order. They are the only brake.',
    fields: {
      max_usd_per_trade: {
        label: 'Max per trade (USD)',
        help: 'The most a single order may cost. Manual orders from the console obey it too.',
      },
      max_open_exposure_usd: {
        label: 'Max open exposure (USD)',
        help: 'The most money that may be at risk at once: positions plus buy orders still waiting to fill. Bets are sized as a share of this amount.',
      },
      max_trades_per_run: {
        label: 'Max trades per research cycle',
        help: 'The bot stops ordering after this many orders until the next research cycle starts. The price watch counts toward it.',
        integer: true,
      },
      kelly_fraction: {
        label: 'Bet size (Kelly fraction)',
        help: 'How boldly to size bets. The Kelly formula picks a bet size from the edge and the money available; 1 bets its full amount, 0.25 a quarter of it. Lower is safer.',
      },
    },
  },
  {
    title: 'Price watch',
    description: 'Between research cycles, the daemon checks the prices of markets Jev already has a view on.',
    fields: {
      watch_interval_secs: {
        label: 'Check prices every (seconds)',
        help: 'How often the daemon compares recent Jev views with the live order books. A check costs nothing unless a price crosses the buy line. 0 turns the watch off.',
        integer: true,
      },
      trade_brief_max_age_minutes: {
        label: 'Max brief age for a trade (minutes)',
        help: 'When a price crosses the buy line, a brief older than this is researched again before the bot buys.',
        integer: true,
      },
      sell_early: {
        label: 'Sell early',
        help: 'Let the price watch sell a position before the market resolves, when selling is clearly the better deal.',
      },
      min_exit_edge: {
        label: 'Sell when the price beats Jev by',
        help: 'Sell only when buyers pay at least this much more per share than Jev thinks the share is worth, e.g. 0.05 is 5 cents.',
      },
      min_exit_profit: {
        label: 'Minimum profit to sell',
        help: 'Sell only when selling returns at least this share of what the position cost, e.g. 0.2 is 20%.',
      },
    },
  },
  {
    title: 'Signal',
    description: 'When a market is worth a trade.',
    fields: {
      min_edge: {
        label: 'Min edge',
        help: "How much better Jev's probability must be than the price before the bot buys, e.g. 0.08 is 8 percentage points.",
      },
      min_answerable: {
        label: 'Min answerable',
        help: 'How sure Jev must be that it has enough information to judge the question, from 0 (guessing) to 1 (sure).',
      },
      min_clarity: {
        label: 'Min clarity',
        help: 'How clear the resolution rules must be, from 0 (vague) to 4 (precise). Unclear markets are skipped before any research is paid for.',
        integer: true,
      },
      min_trade_price: {
        label: 'Min trade price',
        help: 'Never buy shares cheaper than this. Longshots below it are more often model error than opportunity.',
      },
      max_trade_price: {
        label: 'Max trade price',
        help: 'Never buy shares more expensive than this: little to win, a lot to lose.',
      },
      suspicious_edge: {
        label: 'Suspicious edge',
        help: 'An edge this large is more likely a mistake than a bargain, so the bot skips it.',
      },
    },
  },
  {
    title: 'Jev',
    description: 'The decision model that turns evidence into a probability.',
    fields: {
      jev_model: {
        label: 'Jev model',
        help: 'The Jev version on OpenRouter. Pick a fixed one for stable behaviour, or use typesafe/jev-latest.',
      },
      jev_sees_market_price: {
        label: 'Jev sees the market price',
        help: "Show Jev the market's current price as part of the question. Stats compare how accurate Jev is with and without it.",
      },
    },
  },
  {
    title: 'Researcher',
    description: 'The web-search model that writes the evidence briefs.',
    fields: {
      research_enabled: {
        label: 'Research',
        help: 'Look up current news before asking Jev. Off runs Jev alone, which almost never finds a trade.',
      },
      research_model: {
        label: 'Research model',
        help: 'Any OpenRouter chat model. It writes the brief; its price and quality set most of the research cost.',
      },
      research_ttl_hours: {
        label: 'Brief lifetime (hours)',
        help: 'Reuse a brief for this long before researching the market again.',
      },
      max_research_per_run: {
        label: 'Max briefs per research cycle',
        help: 'At most this many new briefs until the next research cycle starts, including the ones the price watch asks for: a cap on research spend.',
        integer: true,
      },
      research_max_results: {
        label: 'Search results per brief',
        help: 'Web pages the researcher reads per brief. More pages cost more tokens.',
        integer: true,
      },
      research_max_chars: {
        label: 'Evidence size (characters)',
        help: 'The most brief text Jev sees. Longer briefs are trimmed, balanced between the case for and against.',
        integer: true,
      },
      research_exclude_domains: {
        label: 'Excluded domains',
        help: 'Sites the web search never uses, one per line: odds and prediction-market sites, so Jev never sees market prices as evidence.',
      },
      research_max_price_move: {
        label: 'Max price move',
        help: 'Research a market again once its price moved more than this since the brief was written: it has likely seen news.',
      },
      concurrency: {
        label: 'Concurrency',
        help: 'How many markets are researched and priced at the same time. Orders still go out one at a time.',
        integer: true,
      },
    },
  },
  {
    title: 'Market filter',
    description: 'Which markets a pass looks at.',
    fields: {
      min_liquidity_usd: {
        label: 'Min liquidity (USD)',
        help: 'Skip markets with less money waiting in buy and sell offers than this: hard to trade at a fair price.',
      },
      min_volume_usd: {
        label: 'Min volume (USD)',
        help: 'Skip markets that have traded less than this in total.',
      },
      max_days_to_resolution: {
        label: 'Max days to resolution',
        help: 'Skip markets that resolve later than this. Money in long markets is tied up for months.',
        integer: true,
      },
      max_spread: {
        label: 'Max spread',
        help: 'Skip markets where the gap between the best buy and sell offer is wider than this, e.g. 0.05 is 5 cents.',
      },
      min_market_price: {
        label: 'Min market price',
        help: 'Skip markets whose YES price is below this: already decided, no room for an edge.',
      },
      max_market_price: {
        label: 'Max market price',
        help: 'Skip markets whose YES price is above this, for the same reason.',
      },
      exclude_tags: {
        label: 'Excluded tags',
        help: 'Polymarket tags to skip, one per line. The defaults are settled by a live price, a post count or a single game, which research cannot inform.',
      },
      description_max_chars: {
        label: 'Description size (characters)',
        help: "How much of the market's rules and description Jev and the researcher see.",
        integer: true,
      },
    },
  },
  {
    title: 'Endpoints',
    description: 'Where jevmarket connects. The defaults are right for almost everyone.',
    fields: {
      openrouter_base_url: {
        label: 'OpenRouter base URL',
        help: 'The OpenRouter API. Change it only for a proxy or a compatible gateway.',
      },
      clob_host: {
        label: 'Order book URL (CLOB host)',
        help: "Polymarket's trading API, where the bot reads prices and sends orders.",
      },
      polygon_rpc_url: {
        label: 'Polygon RPC URL',
        help: 'The Polygon blockchain server the bot asks for wallet balances. A private one is more reliable than the public default.',
      },
    },
  },
  {
    title: 'Misc',
    fields: {
      dry_run: {
        label: 'Dry run',
        help: 'The bot pretends to trade: it logs the orders it would place and places none. Turning it off needs a typed confirmation.',
      },
      db_path: {
        label: 'Database file',
        help: 'Where decisions, briefs and orders are stored. Empty uses the data directory; the daemon keeps its file until restarted.',
        nullable: true,
      },
      log_level: {
        label: 'Log level',
        help: 'How much the daemon logs: error, warn, info or debug. Takes effect on restart; RUST_LOG wins when set.',
      },
    },
  },
]
