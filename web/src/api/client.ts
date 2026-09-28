import type {
  BriefRecord,
  BriefSummary,
  ConfigValue,
  ConfigView,
  ErrorBody,
  Event,
  OrderEvent,
  Positions,
  Recommendation,
  RecommendationDetail,
  StatsView,
  Status,
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
  stats: () => get<StatsView>('stats'),
  runPass: (confirm?: string) => send('pass', { confirm }),
  pause: () => send('loop/pause'),
  resume: (confirm?: string) => send('loop/resume', { confirm }),
  decide: (reference: string, fresh = false) =>
    json<Recommendation>('POST', 'decide', { reference, fresh }),
  refreshBrief: (reference: string) => json<BriefRecord>('POST', 'briefs/refresh', { reference }),
}

export const errorMessage = (e: unknown): string =>
  e instanceof Error ? e.message : typeof e === 'string' ? e : 'Something went wrong'
