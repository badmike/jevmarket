<script setup lang="ts">
import type { Exposure } from '~/api/types'
import { Progress } from '~/components/ui/progress'
import { usd } from '~/lib/format'

const props = defineProps<{ exposure: Exposure }>()

const share = computed(() =>
  props.exposure.cap_usd > 0 ? (props.exposure.total_usd / props.exposure.cap_usd) * 100 : 0
)
const variant = computed(() => (share.value >= 100 ? 'destructive' : share.value >= 80 ? 'warning' : 'default'))
</script>

<template>
  <div class="grid gap-2">
    <p class="text-xl font-semibold text-primary tabular-nums">
      {{ usd(exposure.total_usd) }}
      <span class="text-sm font-normal text-muted">of {{ usd(exposure.cap_usd) }}</span>
    </p>
    <Progress
      :model-value="share"
      :variant="variant"
      :aria-label="`Exposure ${Math.round(share)}% of the cap`"
    />
    <p class="text-xs text-muted tabular-nums">
      Positions {{ usd(exposure.positions_usd) }} · open orders {{ usd(exposure.open_orders_usd) }}
    </p>
  </div>
</template>
