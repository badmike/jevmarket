import { useConfig } from '~/api/queries'

/** Numeric settings the views compare against, with the built-in defaults until the config loads. */
export function useThresholds() {
  const { data } = useConfig()
  const num = (key: string, fallback: number) => {
    const v = data.value?.values[key]
    return typeof v === 'number' ? v : fallback
  }
  return computed(() => ({
    minEdge: num('min_edge', 0.08),
    minAnswerable: num('min_answerable', 0.7),
    minClarity: num('min_clarity', 2),
    maxPriceMove: num('research_max_price_move', 0.05),
  }))
}
