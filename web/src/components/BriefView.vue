<script setup lang="ts">
import { useNow } from '@vueuse/core'

import type { BriefRecord } from '~/api/types'
import Icon from '~/components/Icon.vue'
import { Badge } from '~/components/ui/badge'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { ago, at, datedLine, hostname, points, prob, usd } from '~/lib/format'

const props = defineProps<{
  record: BriefRecord
  /** `research_max_price_move`: beyond it the pipeline researches again. */
  maxMove?: number
}>()

const now = useNow({ interval: 30_000 })
const brief = computed(() => props.record.brief)
const facts = computed(() => brief.value.key_facts.map(datedLine))
const events = computed(() => brief.value.scheduled_events.map(datedLine))
const latest = computed(() => datedLine(brief.value.latest_development))

const move = computed(() => {
  const { midpoint_then: before, midpoint_now: current } = props.record
  if (before == null || current == null) return null
  const delta = current - before
  return { before, current, delta, stale: props.maxMove != null && Math.abs(delta) > props.maxMove }
})
</script>

<template>
  <article class="grid gap-6">
    <div class="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-xs text-muted">
      <SimpleTooltip
        :tooltip="
          record.fresh
            ? 'Inside research_ttl_hours: passes reuse this brief'
            : 'Older than research_ttl_hours: the next decision researches again'
        "
        as-child
      >
        <Badge
          :variant="record.fresh ? 'success' : 'warning'"
          size="sm"
          tabindex="0"
        >
          {{ record.fresh ? 'Fresh' : 'Expired' }}
        </Badge>
      </SimpleTooltip>
      <SimpleTooltip
        v-if="move?.stale"
        :tooltip="`The midpoint moved more than ${points(maxMove)} since this was written: the market has likely seen news the brief lacks`"
        as-child
      >
        <Badge
          variant="warning"
          size="sm"
          tabindex="0"
        >
          Price moved
        </Badge>
      </SimpleTooltip>
      <span>Facts as of {{ brief.as_of || 'unknown' }}</span>
      <span :title="at(record.ts)">Researched {{ ago(record.ts, now.getTime() / 1000) }}</span>
      <span v-if="brief.model">{{ brief.model }}</span>
      <span class="tabular-nums">{{ usd(record.cost, true) }}</span>
      <span
        v-if="move"
        class="tabular-nums"
      >
        Midpoint {{ prob(move.before) }} then, {{ prob(move.current) }} now ({{ points(move.delta) }})
      </span>
    </div>

    <p class="max-w-prose text-base text-primary">
      {{ brief.summary || 'The researcher found nothing relevant.' }}
    </p>

    <div
      v-if="brief.resolution_source_status || brief.latest_development"
      class="grid gap-3 sm:grid-cols-2"
    >
      <section
        v-if="brief.resolution_source_status"
        class="rounded-lg bg-muted-background/60 p-3"
      >
        <h3 class="mb-1 text-xs font-semibold text-muted">Resolution source</h3>
        <p class="text-sm">{{ brief.resolution_source_status }}</p>
      </section>
      <section
        v-if="brief.latest_development"
        class="rounded-lg bg-muted-background/60 p-3"
      >
        <h3 class="mb-1 text-xs font-semibold text-muted">
          Latest development<template v-if="latest.date">, {{ latest.date }}</template>
        </h3>
        <p class="text-sm">{{ latest.text }}</p>
      </section>
    </div>

    <section v-if="events.length">
      <h3 class="mb-2 text-sm font-semibold text-primary">Scheduled before the end date</h3>
      <ol class="grid gap-1.5">
        <li
          v-for="(e, i) in events"
          :key="i"
          class="grid grid-cols-[6.5rem_1fr] gap-3 text-sm"
        >
          <span class="flex items-start gap-1.5 text-muted tabular-nums">
            <Icon
              name="lucide:calendar-clock"
              size="14"
              class="mt-0.5 shrink-0"
              aria-hidden="true"
            />
            {{ e.date ?? 'Undated' }}
          </span>
          <span>{{ e.text }}</span>
        </li>
      </ol>
    </section>

    <section v-if="facts.length">
      <h3 class="mb-2 text-sm font-semibold text-primary">Key facts</h3>
      <ol class="relative grid gap-3 border-l border-border pl-4">
        <li
          v-for="(f, i) in facts"
          :key="i"
          class="relative text-sm"
        >
          <span
            class="absolute top-1.5 -left-[1.3rem] size-2 rounded-full border-2 border-background bg-border-strong"
            aria-hidden="true"
          />
          <span class="block text-xs text-muted tabular-nums">{{ f.date ?? 'Undated' }}</span>
          {{ f.text }}
        </li>
      </ol>
    </section>

    <div class="grid gap-3 sm:grid-cols-2">
      <section class="rounded-lg border border-border p-3">
        <h3 class="mb-2 flex items-center gap-1.5 text-sm font-semibold text-primary">
          <Icon
            name="lucide:thumbs-up"
            size="14"
            class="text-success"
            aria-hidden="true"
          />
          For YES
        </h3>
        <ul
          v-if="brief.for_yes.length"
          class="grid list-disc gap-1 pl-4 text-sm"
        >
          <li
            v-for="(point, i) in brief.for_yes"
            :key="i"
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
      <section class="rounded-lg border border-border p-3">
        <h3 class="mb-2 flex items-center gap-1.5 text-sm font-semibold text-primary">
          <Icon
            name="lucide:thumbs-down"
            size="14"
            class="text-destructive"
            aria-hidden="true"
          />
          Against YES
        </h3>
        <ul
          v-if="brief.against_yes.length"
          class="grid list-disc gap-1 pl-4 text-sm"
        >
          <li
            v-for="(point, i) in brief.against_yes"
            :key="i"
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

    <section v-if="brief.sources.length">
      <h3 class="mb-2 text-sm font-semibold text-primary">Sources</h3>
      <ul class="grid gap-1">
        <li
          v-for="url in brief.sources"
          :key="url"
          class="min-w-0"
        >
          <a
            :href="url"
            target="_blank"
            rel="noopener noreferrer"
            class="group inline-flex max-w-full items-center gap-1.5 text-sm text-accent hover:underline"
          >
            <Icon
              name="lucide:external-link"
              size="12"
              class="shrink-0"
              aria-hidden="true"
            />
            <span class="font-medium">{{ hostname(url) }}</span>
            <span class="truncate text-muted">{{ url.replace(/^https?:\/\/[^/]+/, '') }}</span>
          </a>
        </li>
      </ul>
    </section>
  </article>
</template>
