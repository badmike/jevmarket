<script lang="ts">
import type { Recommendation } from '~/api/types'
import { percent, points } from '~/lib/format'

/** Skip reasons as the pipeline logs them (`src/signal.rs`, `src/pipeline.rs`), in plain words. */
const skips: { match: RegExp; label: string; because: (minEdge: number) => string }[] = [
  {
    match: /^clarity .*pre-screen/,
    label: 'Unclear rules',
    because: () => 'The rules looked too unclear to pay for research',
  },
  { match: /^clarity/, label: 'Unclear rules', because: () => 'The rules are too unclear to bet on' },
  {
    match: /^answerable/,
    label: 'Not enough info',
    because: () => 'There is not enough public information to call it yet',
  },
  {
    match: /suspicious_edge/,
    label: 'Gap too big to trust',
    because: () => 'A gap this big is more likely Jev being wrong than the market',
  },
  {
    match: /^best edge/,
    label: 'Gap too small',
    because: (minEdge) => `The gap is below the ${Math.round(minEdge * 100)} points the bot needs`,
  },
  {
    match: /^outside trade band/,
    label: 'Price too extreme',
    because: () => 'The price is outside the range the bot trades',
  },
  { match: /^no asks/, label: 'Nobody selling', because: () => 'Nobody is selling either side right now' },
  { match: /^kelly/, label: 'Stake too small', because: () => 'The bet size worked out to zero' },
  {
    match: /^min order size/,
    label: 'Minimum order too big',
    because: () => 'The smallest order allowed is above the per-trade cap',
  },
  {
    match: /^cached brief/,
    label: 'Research failed',
    because: () => 'Fresh research could not be fetched before trading',
  },
]

const skipRule = (r: Recommendation) =>
  r.action === 'skip' ? skips.find((s) => s.match.test(r.reason)) : undefined

/** A few words on why, for the table. Empty when the badge says it all. */
export const verdictLabel = (r: Recommendation): string =>
  r.action === 'trade_unexecuted' ? 'Not placed' : (skipRule(r)?.label ?? '')

/** One plain sentence on what Jev concluded and what the bot did about it. */
export function verdictSentence(r: Recommendation, minEdge: number): string {
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
  const skip = skipRule(r)
  return skip ? `${view} ${skip.because(minEdge)}, so the bot skipped it.` : `${view} The bot skipped it.`
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
