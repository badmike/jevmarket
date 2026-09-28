<script setup lang="ts">
import { prob } from '~/lib/format'

/** One value on the 0 to 1 scale, with an optional threshold tick. */
defineProps<{
  label: string
  value: number | null
  threshold?: number
  /** `1` for Jev, `2` for the market: the two chart series. */
  series?: 1 | 2
}>()
</script>

<template>
  <div class="grid grid-cols-[8rem_1fr_3rem] items-center gap-3 text-sm">
    <span class="text-muted">{{ label }}</span>
    <div
      class="relative h-2 rounded-full bg-secondary"
      role="meter"
      :aria-label="label"
      aria-valuemin="0"
      aria-valuemax="1"
      :aria-valuenow="value ?? undefined"
    >
      <div
        v-if="value != null"
        class="h-full rounded-full"
        :class="series === 2 ? 'bg-viz-2' : 'bg-viz-1'"
        :style="{ width: `${Math.max(0, Math.min(1, value)) * 100}%` }"
      />
      <div
        v-if="threshold != null"
        class="absolute -top-1 h-4 w-0.5 rounded bg-primary/60"
        :style="{ left: `${threshold * 100}%` }"
        :title="`Threshold ${prob(threshold)}`"
      />
    </div>
    <span class="text-right font-semibold text-primary tabular-nums">{{ prob(value) }}</span>
  </div>
</template>
