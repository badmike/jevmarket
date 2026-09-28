import { ApiError } from '~/api/client'
import type { MarketChoice } from '~/api/types'

/**
 * A market a person names by slug or polymarket.com link. An event link with several open markets
 * comes back as `choices`; the one picked is what `target` sends next. Typing clears the pick.
 */
export function useMarketReference() {
  const reference = ref('')
  const choices = ref<MarketChoice[]>([])
  const choice = ref<string>()

  watch(reference, () => {
    choices.value = []
    choice.value = undefined
  })

  const target = computed(() => choice.value ?? reference.value.trim())

  /** Keeps the choices of an event error. `false` for any other error, which the caller reports. */
  const offerChoices = (e: unknown): boolean => {
    if (!(e instanceof ApiError) || !e.choices.length) return false
    choices.value = e.choices
    choice.value = undefined
    return true
  }

  return { reference, choices, choice, target, offerChoices }
}
