<script lang="ts">
import type { Recommendation, SkipCode } from '~/api/types'
import { percent, points } from '~/lib/format'

/** The limits a skip sentence may name. */
export interface Limits {
  minEdge: number
  suspiciousEdge: number
}

const pts = (x: number) => `${Math.round(x * 100)} points`

/** Every skip code in plain words: a short label for the table, and the reason for a sentence. */
const skips: Record<SkipCode, { label: string; because: (r: Recommendation, l: Limits) => string }> = {
  unclear: {
    label: 'Unclear rules',
    because: (r) =>
      r.p_yes == null
        ? 'The rules looked too unclear to pay for research'
        : 'The rules are too unclear to bet on',
  },
  unanswerable: {
    label: 'Not enough info',
    because: () => 'There is not enough public information to call it yet',
  },
  no_asks: { label: 'Nobody selling', because: () => 'Nobody is selling either side right now' },
  outside_band: {
    label: 'Price too extreme',
    because: () => 'The price is outside the range the bot trades',
  },
  small_edge: {
    label: 'Gap too small',
    because: (_, l) => `The gap is below the ${pts(l.minEdge)} the bot needs`,
  },
  suspicious_edge: {
    label: 'Gap too big to trust',
    because: (_, l) => `A gap above ${pts(l.suspiciousEdge)} is more likely Jev being wrong than the market`,
  },
  zero_stake: { label: 'Stake too small', because: () => 'The bet size worked out to zero' },
  min_order_too_big: {
    label: 'Minimum order too big',
    because: () => 'The smallest order allowed is above the per-trade cap',
  },
  stale_brief: {
    label: 'Research failed',
    because: () => 'Fresh research could not be fetched before trading',
  },
}

const skipOf = (r: Recommendation) => (r.action === 'skip' && r.skip_code ? skips[r.skip_code] : undefined)

/** A few words on why, for the table. Empty when the badge says it all. */
export const verdictLabel = (r: Recommendation): string =>
  r.action === 'trade_unexecuted' ? 'Not placed' : (skipOf(r)?.label ?? '')

/** One plain sentence on what Jev concluded and what the bot did about it. */
export function verdictSentence(r: Recommendation, limits: Limits): string {
  const view =
    r.p_yes == null
      ? 'Jev only checked how clear the rules are.'
      : `Jev thinks YES is ${percent(r.p_yes)} likely, the market prices it at ${percent(r.midpoint)}.`
  const side = r.trade?.outcome ?? r.side ?? 'a side'

  if (r.action === 'trade') {
    const dryRun = r.trade?.dry_run ? ' (dry run)' : ''
    return `${view} Its estimate for ${side} beats the price by ${points(r.edge)}, so the bot bought ${side}${dryRun}.`
  }
  if (r.action === 'trade_unexecuted') return `${view} The bot wanted to buy ${side}, but the order was not placed.`
  const skip = skipOf(r)
  return skip ? `${view} ${skip.because(r, limits)}, so the bot skipped it.` : `${view} The bot skipped it.`
}
</script>

<script setup lang="ts">
import VerdictBadge from '~/components/VerdictBadge.vue'

/** The verdict badge with a short plain reason; the raw reason is the hover title. */
const props = defineProps<{ recommendation: Recommendation }>()

const label = computed(() => verdictLabel(props.recommendation))
</script>

<template>
  <span
    class="inline-flex items-center gap-2 whitespace-nowrap"
    :title="recommendation.reason"
  >
    <VerdictBadge :action="recommendation.action" />
    <span
      v-if="label"
      class="text-xs text-muted"
      >{{ label }}</span
    >
  </span>
</template>
