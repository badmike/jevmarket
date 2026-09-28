import { useStorage } from '@vueuse/core'

export type ReadKind = 'markets' | 'briefs'

/** Entries kept per kind; the oldest reads are dropped beyond this. */
const MAX_ENTRIES = 1000

/** Per kind: item key → timestamp of the version last opened. */
const stores = {
  markets: useStorage<Record<string, number>>('jevmarket:read:markets', {}),
  briefs: useStorage<Record<string, number>>('jevmarket:read:briefs', {}),
}

/**
 * What this browser has opened, so lists can dim it. Keyed by slug with the item's timestamp:
 * a newer decision or brief for the same market reads as unread again.
 */
export function useReadState(kind: ReadKind) {
  const store = stores[kind]

  const isRead = (key: string, ts: number) => (store.value[key] ?? 0) >= ts

  const markRead = (items: { key: string; ts: number }[]) => {
    const next = { ...store.value }
    for (const { key, ts } of items) next[key] = Math.max(next[key] ?? 0, ts)
    const entries = Object.entries(next)
    store.value =
      entries.length > MAX_ENTRIES
        ? Object.fromEntries(entries.toSorted((a, b) => b[1] - a[1]).slice(0, MAX_ENTRIES))
        : next
  }

  return { isRead, markRead }
}
