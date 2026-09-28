<script setup lang="ts">
/**
 * Points on the unit square against the diagonal, for calibration: a point on the diagonal
 * means the x value was exactly right on average. Dot area follows `n`.
 */
export interface ProbabilityPoint {
  x: number
  y: number
  n: number
  /** Tooltip and accessible name. */
  label: string
}

const props = defineProps<{
  points: ProbabilityPoint[]
  xLabel: string
  yLabel: string
  /** `1` Jev, `2` the market. */
  series?: 1 | 2
}>()

const W = 320
const H = 300
const pad = { left: 40, right: 12, top: 12, bottom: 36 }
const plotW = W - pad.left - pad.right
const plotH = H - pad.top - pad.bottom
const ticks = [0, 0.25, 0.5, 0.75, 1]

const sx = (v: number) => pad.left + v * plotW
const sy = (v: number) => pad.top + (1 - v) * plotH

const maxN = computed(() => Math.max(1, ...props.points.map((p) => p.n)))
const radius = (n: number) => 4 + 5 * Math.sqrt(n / maxN.value)
</script>

<template>
  <svg
    :viewBox="`0 0 ${W} ${H}`"
    class="h-auto w-full max-w-md text-muted"
    role="img"
    :aria-label="`${yLabel} against ${xLabel}`"
  >
    <g
      v-for="t in ticks"
      :key="t"
    >
      <line
        :x1="sx(t)"
        :x2="sx(t)"
        :y1="sy(0)"
        :y2="sy(1)"
        class="stroke-border"
        stroke-width="1"
      />
      <line
        :x1="sx(0)"
        :x2="sx(1)"
        :y1="sy(t)"
        :y2="sy(t)"
        class="stroke-border"
        stroke-width="1"
      />
      <text
        :x="sx(t)"
        :y="H - pad.bottom + 14"
        text-anchor="middle"
        class="fill-current text-[10px] tabular-nums"
      >
        {{ t }}
      </text>
      <text
        :x="pad.left - 6"
        :y="sy(t) + 3"
        text-anchor="end"
        class="fill-current text-[10px] tabular-nums"
      >
        {{ t }}
      </text>
    </g>
    <line
      :x1="sx(0)"
      :y1="sy(0)"
      :x2="sx(1)"
      :y2="sy(1)"
      class="stroke-border-strong"
      stroke-width="1"
    />
    <text
      :x="pad.left + plotW / 2"
      :y="H - 4"
      text-anchor="middle"
      class="fill-current text-[11px]"
    >
      {{ xLabel }}
    </text>
    <text
      :transform="`translate(11 ${pad.top + plotH / 2}) rotate(-90)`"
      text-anchor="middle"
      class="fill-current text-[11px]"
    >
      {{ yLabel }}
    </text>
    <g
      v-for="(p, i) in points"
      :key="i"
      tabindex="0"
      role="img"
      :aria-label="p.label"
      class="focus:outline-none [&:focus-visible>circle:last-child]:stroke-ring"
    >
      <title>{{ p.label }}</title>
      <circle
        :cx="sx(p.x)"
        :cy="sy(p.y)"
        r="12"
        fill="transparent"
      />
      <circle
        :cx="sx(p.x)"
        :cy="sy(p.y)"
        :r="radius(p.n)"
        :class="series === 2 ? 'fill-viz-2' : 'fill-viz-1'"
        class="stroke-card"
        stroke-width="2"
      />
    </g>
  </svg>
</template>
