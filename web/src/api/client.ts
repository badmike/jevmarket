import type {
  BriefRecord,
  BriefSummary,
  ConfigValue,
  ConfigView,
  ErrorBody,
  Event,
  ManualOrder,
  MarketChoice,
  OrderEvent,
  Positions,
  Recommendation,
  RecommendationDetail,
  StatsView,
  Status,
  Transfer,
  WalletKind,
  Wallets,
  WatchItem,
} from '~/api/types'

/** A non-2xx answer, with the server's message, and the confirmation it wants, if any. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly body: ErrorBody
  ) {
    super(body.error)
  }

  /** The action places real orders and needs `confirm` set to the phrase in `body.confirm`. */
  get needsConfirm(): boolean {
    return this.status === 428
  }

  /** The reference was an event with several open markets: these are the ones to pick from. */
  get choices(): MarketChoice[] {
    return this.body.choices ?? []
  }
}

/**
 * Every URL is relative to `<base href>`, which the daemon points at its `--base-path`, so the
 * console works at the root and behind a proxy prefix alike.
 */
export const apiUrl = (path: string): string => new URL(`api/${path}`, document.baseURI).href

async function call(method: string, path: string, body?: unknown): Promise<Response> {
  const response = await fetch(apiUrl(path), {
    method,
    headers: body === undefined ? undefined : { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!response.ok) {
    const error: ErrorBody = await response
      .json()
      .catch(() => ({ error: `${response.status} ${response.statusText}` }))
    throw new ApiError(response.status, error)
  }
  return response
}

async function json<T>(method: string, path: string, body?: unknown): Promise<T> {
  return (await call(method, path, body)).json() as Promise<T>
}

async function send(path: string, body: object = {}): Promise<void> {
  await call('POST', path, body)
}

const get = <T>(path: string) => json<T>('GET', path)

export const api = {
  status: () => get<Status>('status'),
  activity: () => get<Event[]>('activity'),
  config: () => get<ConfigView>('config'),
  patchConfig: (changes: Record<string, ConfigValue>, confirm?: string) =>
    json<ConfigView>('PATCH', 'config', { changes, confirm }),
  recommendations: () => get<Recommendation[]>('recommendations'),
  recommendation: (slug: string) =>
    get<RecommendationDetail>(`recommendations/${encodeURIComponent(slug)}`),
  briefs: () => get<BriefSummary[]>('briefs'),
  brief: (id: number) => get<BriefRecord>(`briefs/${id}`),
  orders: () => get<OrderEvent[]>('orders'),
  positions: () => get<Positions>('positions'),
  wallets: () => get<Wallets>('wallets'),
  /** `amountUsd` null moves everything. */
  transfer: (from: WalletKind, amountUsd: number | null) =>
    json<Transfer>('POST', 'transfer', { from, amount_usd: amountUsd }),
  stats: () => get<StatsView>('stats'),
  watchlist: () => get<WatchItem[]>('watchlist'),
  /** Put a market on the watchlist; one Jev never priced is decided first. */
  watch: (reference: string) => json<Recommendation>('POST', 'watchlist', { reference }),
  unwatch: async (slug: string) => {
    await call('DELETE', `watchlist/${encodeURIComponent(slug)}`)
  },
  runPass: () => send('pass'),
  pause: () => send('loop/pause'),
  resume: (confirm?: string) => send('loop/resume', { confirm }),
  decide: (reference: string, fresh = false) =>
    json<Recommendation>('POST', 'decide', { reference, fresh }),
  refreshBrief: (reference: string) => json<BriefRecord>('POST', 'briefs/refresh', { reference }),
  placeOrder: (order: ManualOrder, confirm?: string) =>
    json<OrderEvent>('POST', 'orders', { ...order, confirm }),
}

export const errorMessage = (e: unknown): string =>
  e instanceof Error ? e.message : typeof e === 'string' ? e : 'Something went wrong'
