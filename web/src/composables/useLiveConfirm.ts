import { createSharedComposable } from '@vueuse/core'

import { ApiError } from '~/api/client'

export interface LiveConfirmRequest {
  title: string
  message: string
  /** The phrase to type, as the server asked for it. */
  phrase: string
  settle: (phrase: string | null) => void
}

const useLiveConfirmBase = () => {
  const current = shallowRef<LiveConfirmRequest | null>(null)

  /** Ask for the typed confirmation; resolves to the phrase, or `null` when cancelled. */
  const ask = (title: string, message: string, phrase: string): Promise<string | null> =>
    new Promise((resolve) => {
      current.value?.settle(null)
      current.value = {
        title,
        message,
        phrase,
        settle: (value) => {
          current.value = null
          resolve(value)
        },
      }
    })

  /**
   * Run `action`; if the server answers that it places real orders, ask for the confirmation
   * and run it once more with it. Resolves to `undefined` when the person cancels.
   */
  const guarded = async <T>(
    title: string,
    message: string,
    action: (confirm?: string) => Promise<T>
  ): Promise<T | undefined> => {
    try {
      return await action()
    } catch (e) {
      if (!(e instanceof ApiError && e.needsConfirm)) throw e
      const phrase = await ask(title, message, e.body.confirm ?? 'LIVE')
      return phrase === null ? undefined : action(phrase)
    }
  }

  return { current, guarded }
}

/** The one live-money confirmation, rendered by `LiveConfirmDialog.vue` in `app.vue`. */
export const useLiveConfirm = createSharedComposable(useLiveConfirmBase)
