<script setup lang="ts">
import { useElementSize } from '@vueuse/core'

import type { PnlPoint } from '~/api/types'
import { at, signedUsd, usd } from '~/lib/format'

/** Cumulative PnL over resolution time, live and dry-run orders as two stepped lines. */
const props = defineProps<{ points: PnlPoint[] }>()

/** Drawn at the container's width, so labels keep their size instead of scaling with the SVG. */
const root = useTemplateRef('root')
const { width } = useElementSize(root)
const W = computed(() => Math.max(320, Math.round(width.value)))
const H = 240
const pad = { left: 56, right: 16, top: 12, bottom: 28 }
const plotW = computed(() => W.value - pad.left - pad.right)
const plotH = H - pad.top - pad.bottom

const series = computed(() =>
  (
    [
      { name: 'Live', dryRun: false, cls: 'stroke-viz-1', dot: 'fill-viz-1' },
      { name: 'Dry run', dryRun: true, cls: 'stroke-viz-2', dot: 'fill-viz-2' },
    ] as const
  )
    .map((s) => ({ ...s, points: props.points.filter((p) => p.dry_run === s.dryRun) }))
    .filter((s) => s.points.length)
)

const domain = computed(() => {
  const ts = props.points.map((p) => p.ts)
  const ys = [0, ...props.points.map((p) => p.cumulative_usd)]
  const t0 = Math.min(...ts)
  const t1 = Math.max(...ts)
  const air = Math.max(1, (Math.max(...ys) - Math.min(...ys)) * 0.1)
  return { t0, t1: t1 > t0 ? t1 : t0 + 86400, y0: Math.min(...ys) - air, y1: Math.max(...ys) + air }
})

const sx = (t: number) =>
  pad.left + ((t - domain.value.t0) / (domain.value.t1 - domain.value.t0)) * plotW.value
const sy = (v: number) =>
  pad.top + (1 - (v - domain.value.y0) / (domain.value.y1 - domain.value.y0)) * plotH

/** Four round ticks across the value range. */
const yTicks = computed(() => {
  const { y0, y1 } = domain.value
  const raw = (y1 - y0) / 4
  const mag = 10 ** Math.floor(Math.log10(raw))
  const step = ([1, 2, 5, 10].find((m) => m * mag >= raw) ?? 10) * mag
  const out: number[] = []
  for (let v = Math.ceil(y0 / step) * step; v <= y1; v += step) out.push(Number(v.toFixed(6)))
  return out
})

/** Step after each resolution: PnL changes when a market resolves, not in between. */
const path = (points: PnlPoint[]) =>
  points
    .map((p, i) => {
      const prev = i === 0 ? 0 : points[i - 1]!.cumulative_usd
      return `${i === 0 ? `M${sx(p.ts)},${sy(0)}` : ''} L${sx(p.ts)},${sy(prev)} L${sx(p.ts)},${sy(p.cumulative_usd)}`
    })
    .join(' ')
</script>

<template>
  <div
    ref="root"
    class="grid gap-2"
  >
    <ul
      v-if="series.length > 1"
      class="flex gap-4 text-xs text-muted"
    >
      <li
        v-for="s in series"
        :key="s.name"
        class="flex items-center gap-1.5"
      >
        <span
          class="h-0.5 w-4 rounded"
          :class="s.dryRun ? 'bg-viz-2' : 'bg-viz-1'"
          aria-hidden="true"
        />
        {{ s.name }}
      </li>
    </ul>
    <svg
      :viewBox="`0 0 ${W} ${H}`"
      class="h-auto w-full text-muted"
      role="img"
      aria-label="Cumulative profit and loss by resolution date"
    >
      <g
        v-for="t in yTicks"
        :key="t"
      >
        <line
          :x1="pad.left"
          :x2="W - pad.right"
          :y1="sy(t)"
          :y2="sy(t)"
          :class="t === 0 ? 'stroke-border-strong' : 'stroke-border'"
          stroke-width="1"
        />
        <text
          :x="pad.left - 6"
          :y="sy(t) + 3"
          text-anchor="end"
          class="fill-current text-[10px] tabular-nums"
        >
          {{ usd(t) }}
        </text>
      </g>
      <text
        :x="pad.left"
        :y="H - 6"
        class="fill-current text-[10px]"
      >
        {{ at(domain.t0) }}
      </text>
      <text
        :x="W - pad.right"
        :y="H - 6"
        text-anchor="end"
        class="fill-current text-[10px]"
      >
        {{ at(domain.t1) }}
      </text>
      <g
        v-for="s in series"
        :key="s.name"
      >
        <path
          :d="path(s.points)"
          fill="none"
          :class="s.cls"
          stroke-width="2"
          stroke-linejoin="round"
          stroke-linecap="round"
        />
        <g
          v-for="(p, i) in s.points"
          :key="i"
          tabindex="0"
          role="img"
          :aria-label="`${s.name}, ${at(p.ts)}: ${signedUsd(p.pnl_usd)}, total ${signedUsd(p.cumulative_usd)}`"
          class="focus:outline-none"
        >
          <title>{{ s.name }}, {{ at(p.ts) }}: {{ signedUsd(p.pnl_usd) }}, total {{ signedUsd(p.cumulative_usd) }}</title>
          <circle
            :cx="sx(p.ts)"
            :cy="sy(p.cumulative_usd)"
            r="10"
            fill="transparent"
          />
          <circle
            :cx="sx(p.ts)"
            :cy="sy(p.cumulative_usd)"
            r="4"
            :class="s.dot"
            class="stroke-card"
            stroke-width="2"
          />
        </g>
      </g>
    </svg>
  </div>
</template>
