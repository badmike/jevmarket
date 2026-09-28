import { useStorage } from '@vueuse/core'

/** Pinned market slugs, newest first. */
const store = useStorage<string[]>('jevmarket:pins', [])
const pinned = computed(() => new Set(store.value))

/** Markets this browser keeps at the top of its lists, keyed by slug. */
export function usePins() {
  const isPinned = (slug: string) => pinned.value.has(slug)

  const togglePin = (slug: string) => {
    store.value = isPinned(slug) ? store.value.filter((s) => s !== slug) : [slug, ...store.value]
  }

  return { isPinned, togglePin }
}
