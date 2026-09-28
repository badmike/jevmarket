import { useEventListener } from '@vueuse/core'

import { navigation } from '~/router'

/** Focus is in a text field: single-key shortcuts must not fire. */
export const typing = (target: EventTarget | null): boolean =>
  target instanceof HTMLElement &&
  (target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName))

/**
 * `g` then a page's key jumps there (`g o` Overview, `g m` Markets, `g ,` Settings, ...), and `/` focuses
 * the page's search field. Ignored while typing.
 */
export function useShortcuts() {
  const router = useRouter()
  let pending = false
  let timer: ReturnType<typeof setTimeout> | undefined

  useEventListener(window, 'keydown', (event: KeyboardEvent) => {
    if (event.metaKey || event.ctrlKey || event.altKey || typing(event.target)) return
    if (pending) {
      pending = false
      clearTimeout(timer)
      const item = navigation.find((n) => n.key === event.key)
      if (item) {
        event.preventDefault()
        void router.push({ name: item.name })
      }
      return
    }
    if (event.key === 'g') {
      pending = true
      timer = setTimeout(() => (pending = false), 1200)
    } else if (event.key === '/') {
      const search = document.querySelector<HTMLInputElement>('#main-content input[type="search"]')
      if (search) {
        event.preventDefault()
        search.focus()
        search.select()
      }
    }
  })
}
