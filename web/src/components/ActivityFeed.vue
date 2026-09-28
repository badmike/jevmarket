<script setup lang="ts">
import { type ActivityEvent, useLive } from '~/api/live'
import Icon from '~/components/Icon.vue'
import { EmptyState } from '~/components/ui/empty-state'
import { clockTime, points, prob, usd } from '~/lib/format'

type Filter = 'all' | 'decisions' | 'orders' | 'log'

const { activity } = useLive()
const filter = ref<Filter>('all')

const filters: { value: Filter; label: string }[] = [
  { value: 'all', label: 'All' },
  { value: 'decisions', label: 'Decisions' },
  { value: 'orders', label: 'Orders' },
  { value: 'log', label: 'Log' },
]

const matches: Record<Filter, (e: ActivityEvent) => boolean> = {
  all: () => true,
  decisions: (e) => e.type === 'decision',
  orders: (e) => e.type === 'order',
  log: (e) => e.type === 'log' || e.type === 'pass_started' || e.type === 'pass_finished',
}

interface Line {
  ts: number | null
  icon: string
  tone: string
  text: string
  to?: { name: string; params: Record<string, string | number> }
}

const describe = (e: ActivityEvent): Line => {
  switch (e.type) {
    case 'pass_started':
      return {
        ts: e.pass.started_at,
        icon: 'lucide:play',
        tone: 'text-accent',
        text: `Pass ${e.pass.number} started${e.pass.dry_run ? ' (dry run)' : ''}`,
      }
    case 'pass_finished':
      return {
        ts: e.pass.finished_at,
        icon: e.pass.error ? 'lucide:circle-x' : 'lucide:circle-check',
        tone: e.pass.error ? 'text-destructive' : 'text-success',
        text: e.pass.error
          ? `Pass ${e.pass.number} failed: ${e.pass.error}`
          : `Pass ${e.pass.number} finished: ${e.pass.assessed} assessed, ${e.pass.trades} trades, ${usd(e.pass.spend.jev_usd + e.pass.spend.research_usd, true)}`,
      }
    case 'decision': {
      const r = e.recommendation
      const view = r.p_yes == null ? 'pre-screen' : `Jev ${prob(r.p_yes)} vs ${prob(r.midpoint)}, edge ${points(r.edge)}`
      return {
        ts: r.ts,
        icon: r.action === 'skip' ? 'lucide:circle-minus' : 'lucide:target',
        tone: r.action === 'skip' ? 'text-muted' : 'text-success',
        text: `${r.action === 'skip' ? 'Skip' : 'Trade signal'}: ${r.question} (${view})`,
        to: { name: 'markets', params: { slug: r.slug } },
      }
    }
    case 'order':
      return {
        ts: e.order.ts,
        icon: 'lucide:receipt',
        tone: e.order.status === 'rejected' || e.order.status === 'refused' ? 'text-destructive' : 'text-accent',
        text: `${e.order.dry_run ? 'Dry-run order' : 'Order'} ${e.order.status}: BUY ${e.order.outcome} ${e.order.size} @ ${prob(e.order.price, 3)} (${usd(e.order.usd)}) on ${e.order.slug}${e.order.message ? `, ${e.order.message}` : ''}`,
      }
    case 'brief':
      return {
        ts: e.brief.ts,
        icon: 'lucide:newspaper',
        tone: 'text-ai',
        text: `New brief: ${e.brief.question ?? e.brief.slug}`,
        to: { name: 'briefings', params: { id: e.brief.id } },
      }
    case 'log':
      return {
        ts: e.ts,
        icon: e.level === 'info' ? 'lucide:info' : 'lucide:triangle-alert',
        tone: e.level === 'error' ? 'text-destructive' : e.level === 'warn' ? 'text-warning' : 'text-muted',
        text: e.message,
      }
  }
}

const lines = computed(() =>
  activity.value
    .filter((i) => matches[filter.value](i.event))
    .map((i) => ({ id: i.id, ...describe(i.event) }))
)
</script>

<template>
  <section
    class="grid gap-3"
    aria-labelledby="activity-title"
  >
    <div class="flex flex-wrap items-center justify-between gap-2">
      <h2
        id="activity-title"
        class="text-base font-semibold text-primary"
      >
        Activity
      </h2>
      <div
        role="radiogroup"
        aria-label="Show"
        class="inline-flex gap-1 rounded-xl bg-input p-1"
      >
        <button
          v-for="f in filters"
          :key="f.value"
          type="button"
          role="radio"
          :aria-checked="filter === f.value"
          class="cursor-pointer rounded-lg px-2 py-0.5 text-sm font-semibold transition-all hover:bg-background/75"
          :class="filter === f.value && 'bg-background text-primary shadow-sm'"
          @click="filter = f.value"
        >
          {{ f.label }}
        </button>
      </div>
    </div>
    <EmptyState
      v-if="!lines.length"
      icon="lucide:activity"
      title="Nothing yet"
      description="Passes, decisions, orders and log lines appear here as they happen."
    />
    <ol
      v-else
      class="divide-y divide-border overflow-hidden rounded-lg border border-border bg-card"
      aria-live="polite"
      aria-relevant="additions"
    >
      <TransitionGroup name="fade">
        <li
          v-for="line in lines"
          :key="line.id"
          class="flex items-start gap-3 px-3 py-2 text-sm"
        >
          <Icon
            :name="line.icon"
            size="16"
            class="mt-0.5 shrink-0"
            :class="line.tone"
            aria-hidden="true"
          />
          <RouterLink
            v-if="line.to"
            :to="line.to"
            class="min-w-0 flex-1 break-words hover:underline"
          >
            {{ line.text }}
          </RouterLink>
          <span
            v-else
            class="min-w-0 flex-1 break-words"
          >
            {{ line.text }}
          </span>
          <time class="shrink-0 text-xs text-muted tabular-nums">
            {{ line.ts ? clockTime(line.ts) : '' }}
          </time>
        </li>
      </TransitionGroup>
    </ol>
  </section>
</template>
