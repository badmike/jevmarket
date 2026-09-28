<script setup lang="ts">
import { useNow } from '@vueuse/core'

import type { BriefRecord } from '~/api/types'
import BriefStatus from '~/components/BriefStatus.vue'
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { ago, at, datedLine, hostname, points, prob, usd } from '~/lib/format'

const props = defineProps<{
  record: BriefRecord
  /** `research_max_price_move`: beyond it the pipeline researches again. */
  maxMove?: number
}>()

const now = useNow({ interval: 30_000 })
const nowSecs = computed(() => now.value.getTime() / 1000)
const brief = computed(() => props.record.brief)
const latest = computed(() => datedLine(brief.value.latest_development))

/** Key facts newest first, grouped under their date; undated facts come last. */
const timeline = computed(() => {
  const groups = new Map<string | null, string[]>()
  const sorted = brief.value.key_facts
    .map(datedLine)
    .toSorted((a, b) => (b.date ?? '').localeCompare(a.date ?? ''))
  for (const { date, text } of sorted) groups.set(date, [...(groups.get(date) ?? []), text])
  return [...groups].map(([date, items]) => ({ date, items }))
})

const events = computed(() =>
  brief.value.scheduled_events
    .map(datedLine)
    .toSorted((a, b) => (a.date ?? '￿').localeCompare(b.date ?? '￿'))
    .map((e) => ({ ...e, when: e.date ? ago(Date.parse(`${e.date}T00:00:00`) / 1000, nowSecs.value) : null }))
)

const sources = computed(() =>
  brief.value.sources.map((url) => ({ url, host: hostname(url), path: url.replace(/^https?:\/\/[^/]+/, '') }))
)

const sides = computed(() => [
  { title: 'For YES', icon: 'lucide:thumbs-up', tone: 'text-success', points: brief.value.for_yes },
  { title: 'Against YES', icon: 'lucide:thumbs-down', tone: 'text-destructive', points: brief.value.against_yes },
])

const move = computed(() => {
  const { midpoint_then: before, midpoint_now: current } = props.record
  if (before == null || current == null) return null
  const delta = current - before
  return { before, current, delta, stale: props.maxMove != null && Math.abs(delta) > props.maxMove }
})
</script>

<template>
  <article class="@container grid gap-8">
    <div class="grid gap-4">
      <div class="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted">
        <BriefStatus
          :fresh="record.fresh"
          :settled="record.settled"
          tabindex="0"
        />
        <span :title="at(record.ts)">Researched {{ ago(record.ts, nowSecs) }}</span>
        <span aria-hidden="true">·</span>
        <span>facts as of {{ brief.as_of || 'unknown' }}</span>
        <template v-if="move">
          <span aria-hidden="true">·</span>
          <span class="tabular-nums">
            <SimpleTooltip
              tooltip="Midpoint: the market price, halfway between the best buy and sell offer."
              as-child
            >
              <span class="underline decoration-muted/50 decoration-dotted underline-offset-4">Midpoint</span>
            </SimpleTooltip>
            {{ move.delta ? `${prob(move.before)} then, ${prob(move.current)} now` : `${prob(move.current)}, unchanged` }}
          </span>
        </template>
      </div>

      <Alert
        v-if="move?.stale"
        color="warning"
        icon="lucide:triangle-alert"
        class="text-sm"
      >
        The midpoint moved {{ points(move.delta) }} since this brief was written, more than the
        {{ points(maxMove) }} limit. The market has likely seen news the brief lacks.
      </Alert>

      <p class="max-w-prose text-lg text-pretty text-primary">
        {{ brief.summary || 'The researcher found nothing relevant.' }}
      </p>
    </div>

    <dl
      v-if="brief.latest_development || brief.resolution_source_status"
      class="grid gap-x-6 gap-y-4 @lg:grid-cols-2"
    >
      <div
        v-if="brief.latest_development"
        class="grid content-start gap-1"
      >
        <dt class="text-xs font-medium text-muted">
          Latest development<template v-if="latest.date">, {{ latest.date }}</template>
        </dt>
        <dd class="text-sm text-primary">{{ latest.text }}</dd>
      </div>
      <div
        v-if="brief.resolution_source_status"
        class="grid content-start gap-1"
      >
        <dt class="text-xs font-medium text-muted">Resolution source</dt>
        <dd class="text-sm text-primary">{{ brief.resolution_source_status }}</dd>
      </div>
    </dl>

    <section
      v-if="events.length"
      class="grid gap-3"
    >
      <h3 class="text-sm font-semibold text-primary">Coming up</h3>
      <ol class="grid gap-2">
        <li
          v-for="(e, i) in events"
          :key="i"
          class="flex gap-3 rounded-lg bg-muted-background/50 px-3 py-2 text-sm"
        >
          <Icon
            name="lucide:calendar-clock"
            size="14"
            class="mt-0.75 shrink-0 text-muted"
            aria-hidden="true"
          />
          <div class="grid min-w-0 gap-0.5">
            <span class="text-primary">{{ e.text }}</span>
            <span class="text-xs text-muted tabular-nums">
              {{ e.date ?? 'Undated' }}<template v-if="e.when">, {{ e.when }}</template>
            </span>
          </div>
        </li>
      </ol>
    </section>

    <section
      v-if="timeline.length"
      class="grid gap-3"
    >
      <h3 class="text-sm font-semibold text-primary">Key facts</h3>
      <ol class="grid gap-4">
        <li
          v-for="group in timeline"
          :key="group.date ?? 'undated'"
          class="group/fact grid grid-cols-[auto_1fr] gap-x-3"
        >
          <span
            class="mt-1 size-2 rounded-full bg-border-strong"
            aria-hidden="true"
          />
          <span class="text-xs font-medium text-muted tabular-nums">{{ group.date ?? 'Undated' }}</span>
          <span
            class="mx-auto mt-1.5 -mb-3 w-px bg-border group-last/fact:mb-0"
            aria-hidden="true"
          />
          <ul class="mt-1 grid gap-2">
            <li
              v-for="(text, i) in group.items"
              :key="i"
              class="text-sm text-primary"
            >
              {{ text }}
            </li>
          </ul>
        </li>
      </ol>
    </section>

    <div class="grid gap-6 @lg:grid-cols-2">
      <section
        v-for="side in sides"
        :key="side.title"
        class="grid content-start gap-3"
      >
        <h3 class="flex items-center gap-1.5 text-sm font-semibold text-primary">
          <Icon
            :name="side.icon"
            size="14"
            :class="side.tone"
            aria-hidden="true"
          />
          {{ side.title }}
        </h3>
        <ul
          v-if="side.points.length"
          class="grid gap-2 text-sm text-primary"
        >
          <li
            v-for="(point, i) in side.points"
            :key="i"
            class="border-l-2 border-border pl-3"
          >
            {{ point }}
          </li>
        </ul>
        <p
          v-else
          class="text-sm text-muted"
        >
          Nothing reported.
        </p>
      </section>
    </div>

    <section
      v-if="sources.length"
      class="grid gap-2"
    >
      <h3 class="text-sm font-semibold text-primary">Sources</h3>
      <ul class="grid gap-0.5">
        <li
          v-for="s in sources"
          :key="s.url"
          class="min-w-0"
        >
          <a
            :href="s.url"
            target="_blank"
            rel="noopener noreferrer"
            class="group -mx-2 flex min-w-0 items-center gap-2 rounded-md px-2 py-1 text-sm transition-colors hover:bg-secondary/60"
          >
            <span class="shrink-0 font-medium text-primary group-hover:text-accent">{{ s.host }}</span>
            <span class="min-w-0 truncate text-xs text-muted">{{ s.path }}</span>
            <Icon
              name="lucide:arrow-up-right"
              size="12"
              class="ml-auto shrink-0 text-muted group-hover:text-accent"
              aria-hidden="true"
            />
          </a>
        </li>
      </ul>
    </section>

    <p
      v-if="brief.model || record.cost"
      class="text-xs text-muted"
    >
      Written by {{ brief.model || 'an unknown model' }} for <span class="tabular-nums">{{ usd(record.cost, true) }}</span>
    </p>
  </article>
</template>
