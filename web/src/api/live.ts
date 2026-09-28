import { createSharedComposable } from '@vueuse/core'
import { useQueryClient } from '@tanstack/vue-query'

import { api, apiUrl } from '~/api/client'
import { keys } from '~/api/queries'
import type { BriefSummary, Event, Recommendation } from '~/api/types'

export type ActivityEvent = Extract<
  Event,
  { type: 'pass_started' | 'pass_finished' | 'decision' | 'order' | 'brief' | 'log' }
>

export interface ActivityItem {
  id: number
  event: ActivityEvent
}

export type Connection = 'connecting' | 'open' | 'reconnecting'

/** Reconnect delays double from one second up to this. */
const MAX_BACKOFF_MS = 10_000
const MAX_ACTIVITY = 200

const ACTIVITY_TYPES = new Set<Event['type']>([
  'pass_started',
  'pass_finished',
  'decision',
  'order',
  'brief',
  'log',
])

const isActivity = (e: Event): e is ActivityEvent => ACTIVITY_TYPES.has(e.type)

/** Replace the entry with the same key, or put the new one first. */
const upsert = <T>(list: T[] | undefined, item: T, key: (x: T) => string | number): T[] => {
  const rest = (list ?? []).filter((x) => key(x) !== key(item))
  return [item, ...rest]
}

/**
 * The one connection to `api/events`. Events patch the query cache in place where they carry
 * the new state, and invalidate it where they only say what changed. Every (re)connect reads
 * all snapshots again, because events sent while disconnected are gone.
 */
const useLiveBase = () => {
  const client = useQueryClient()
  const connection = ref<Connection>('connecting')
  const retryAt = ref<number | null>(null)
  const activity = shallowRef<ActivityItem[]>([])
  /** Slugs updated live in the last moments, for a brief row highlight. */
  const touched = ref(new Set<string>())

  let source: EventSource | null = null
  /** The daemon build this page was loaded from; another one means stale code and asset names. */
  let build: string | null = null
  let attempt = 0
  let timer: ReturnType<typeof setTimeout> | undefined
  let nextId = 0

  const push = (events: ActivityEvent[]) => {
    const items = events.map((event) => ({ id: nextId++, event }))
    activity.value = [...items.reverse(), ...activity.value].slice(0, MAX_ACTIVITY)
  }

  const touch = (slug: string) => {
    touched.value = new Set(touched.value).add(slug)
    setTimeout(() => {
      const next = new Set(touched.value)
      next.delete(slug)
      touched.value = next
    }, 2500)
  }

  const resync = async () => {
    const status = await api.status().catch(() => null)
    if (status) {
      if (build !== null && status.build !== build) {
        window.location.reload()
        return
      }
      build = status.build
      client.setQueryData(keys.status, status)
    }
    await client.invalidateQueries()
    try {
      const recent = (await api.activity()).filter(isActivity)
      nextId = 0
      activity.value = []
      push(recent)
    } catch {
      /* the feed starts empty; live events still arrive */
    }
  }

  const apply = (event: Event) => {
    if (isActivity(event)) push([event])
    switch (event.type) {
      case 'status':
        client.setQueryData(keys.status, event.status)
        break
      case 'decision': {
        const r = event.recommendation
        client.setQueryData<Recommendation[]>(keys.recommendations, (list) =>
          upsert(list, r, (x) => x.slug)
        )
        void client.invalidateQueries({ queryKey: keys.recommendation(r.slug) })
        touch(r.slug)
        break
      }
      case 'brief':
        client.setQueryData<BriefSummary[]>(keys.briefs, (list) =>
          upsert(list, event.brief, (x) => x.slug)
        )
        break
      case 'order':
        void client.invalidateQueries({ queryKey: keys.orders })
        break
      case 'positions_changed':
        void client.invalidateQueries({ queryKey: keys.positions })
        void client.invalidateQueries({ queryKey: keys.wallets })
        void client.invalidateQueries({ queryKey: keys.orders })
        break
      case 'stats_changed':
        void client.invalidateQueries({ queryKey: keys.stats })
        break
      case 'watchlist_changed':
        void client.invalidateQueries({ queryKey: keys.watchlist })
        break
      case 'config_changed':
        void client.invalidateQueries({ queryKey: keys.config })
        break
      case 'resync':
        void resync()
        break
    }
  }

  const connect = () => {
    retryAt.value = null
    const current = new EventSource(apiUrl('events'))
    source = current
    current.onopen = () => {
      attempt = 0
      connection.value = 'open'
      void resync()
    }
    current.onmessage = (message: MessageEvent<string>) => apply(JSON.parse(message.data) as Event)
    // EventSource retries on its own at a fixed pace; take over to back off instead.
    current.onerror = () => {
      if (source !== current) return
      current.close()
      source = null
      connection.value = 'reconnecting'
      const delay = Math.min(MAX_BACKOFF_MS, 1000 * 2 ** attempt++)
      retryAt.value = Date.now() + delay
      timer = setTimeout(connect, delay)
    }
  }

  /** Skip the wait after a dropped connection. */
  const reconnect = () => {
    clearTimeout(timer)
    source?.close()
    source = null
    attempt = 0
    connection.value = 'connecting'
    connect()
  }

  /** Back from sleep, a hidden tab or a network change: try now instead of waiting out the backoff. */
  const retrySoon = () => {
    if (connection.value !== 'open') reconnect()
  }
  const onVisible = () => {
    if (document.visibilityState === 'visible') retrySoon()
  }
  /** A lazy route chunk of an older build is gone from the server: load the current build. */
  const onPreloadError = () => window.location.reload()

  connect()
  window.addEventListener('online', retrySoon)
  document.addEventListener('visibilitychange', onVisible)
  window.addEventListener('vite:preloadError', onPreloadError)
  onScopeDispose(() => {
    clearTimeout(timer)
    source?.close()
    window.removeEventListener('online', retrySoon)
    document.removeEventListener('visibilitychange', onVisible)
    window.removeEventListener('vite:preloadError', onPreloadError)
  })

  return {
    connection: readonly(connection),
    retryAt: readonly(retryAt),
    activity,
    touched,
    reconnect,
  }
}

export const useLive = createSharedComposable(useLiveBase)
