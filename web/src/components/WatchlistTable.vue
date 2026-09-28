<script setup lang="ts">
import { useNow, useStorage } from '@vueuse/core'
import { toast } from 'vue-sonner'

import { api, errorMessage } from '~/api/client'
import { useConfig, useStatus, useWatchlist } from '~/api/queries'
import type { WatchItem, WatchPosition, WatchSide } from '~/api/types'
import Icon from '~/components/Icon.vue'
import MarketAvatar from '~/components/MarketAvatar.vue'
import MarketChoiceSelect from '~/components/MarketChoiceSelect.vue'
import QueryError from '~/components/QueryError.vue'
import SortSelect from '~/components/SortSelect.vue'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Input } from '~/components/ui/input'
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableRow,
  TableSortableHead,
} from '~/components/ui/table'
import { SimpleTooltip } from '~/components/ui/tooltip'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useMarketReference } from '~/composables/useMarketReference'
import { useThresholds } from '~/composables/useSettings'
import { ago, countdown, day, inDays, percent, prob } from '~/lib/format'
import { type SortKey, sortRows, type TableSort } from '~/lib/table'

/** The markets the price watch holds against the live book, closest to a buy or sale first. */
const props = defineProps<{ selected: string | null; search: string }>()
const emit = defineEmits<{ open: [slug: string] }>()

const { data, error, isPending, refetch } = useWatchlist()
const { data: status } = useStatus()
const { data: config } = useConfig()
const limits = useThresholds()
const now = useNow({ interval: 1000 })
const seconds = computed(() => now.value.getTime() / 1000)

const watch = computed(() => status.value?.watch ?? null)
const setting = (key: string, fallback: number) => {
  const v = config.value?.values[key]
  return typeof v === 'number' ? v : fallback
}
const maxBriefAge = computed(() => setting('trade_brief_max_age_minutes', 120) * 60)
const ttlHours = computed(() => setting('research_ttl_hours', 6))

const pts = (x: number) => {
  const n = Math.round(x * 100)
  return `${n} ${n === 1 ? 'pt' : 'pts'}`
}

/** The daemon's order, signals first and then the least left to move, is the "How close" sort. */
const columns: Record<string, SortKey<{ item: WatchItem; index: number }>> = {
  market: ({ item }) => item.question || item.slug,
  p_yes: ({ item }) => item.p_yes,
  midpoint: ({ item }) => item.midpoint ?? -Infinity,
  close: ({ index }) => index,
  end_date: ({ item }) => (item.end_date ? Date.parse(item.end_date) : Infinity),
  brief_at: ({ item }) => item.brief_at ?? -Infinity,
}
const sort = useStorage<TableSort>('jevmarket:sort:watchlist', { column: 'close', direction: 'asc' })

const heads = computed(() =>
  [
    { column: 'market', label: 'Market', hint: '', class: 'w-full min-w-64' },
    {
      column: 'p_yes',
      label: 'Jev',
      hint: 'How likely Jev thinks YES is, from its last look at the market.',
      class: 'text-right',
    },
    { column: 'midpoint', label: 'Price', hint: 'What YES costs on the market right now.', class: 'text-right' },
    {
      column: 'yes',
      label: 'Buys YES at',
      hint: `The highest YES price the bot would buy at: Jev's estimate minus the ${pts(limits.value.minEdge)} it needs, inside the trade band. Below it, the current price.`,
      class: 'text-right',
    },
    { column: 'no', label: 'Buys NO at', hint: 'The same for NO.', class: 'text-right' },
    {
      column: 'close',
      label: 'How close',
      hint: 'How far the price has to fall before the bot buys, or for shares it holds, rise before it sells them early.',
      class: '',
    },
    {
      column: 'end_date',
      label: 'Resolves',
      hint: 'When the market is scheduled to settle and pay out the winning side.',
      class: 'text-right',
    },
    {
      column: 'brief_at',
      label: 'Brief',
      hint: `How old the research behind Jev's view is. Older than ${Math.round(maxBriefAge.value / 60)} minutes, it is researched again before a trade.`,
      class: 'text-right',
    },
  ].map((h) => ({ ...h, sortable: h.column in columns }))
)
const sortOptions = computed(() => heads.value.filter((h) => h.sortable))

interface Closeness {
  text: string
  tone: string
}

/** How near a held position is to its early sale, in a sentence. */
const exitCloseness = (item: WatchItem, p: WatchPosition): Closeness => {
  if (item.signal) return { text: `Sell signal on ${p.outcome}`, tone: 'font-semibold text-success' }
  if (p.trigger == null) return { text: 'Holding until it resolves', tone: 'text-muted' }
  if (p.bid == null) return { text: `Nobody is buying ${p.outcome} right now`, tone: 'text-muted' }
  return { text: `${p.outcome} has to rise ${pts(p.distance ?? 0)} to ${prob(p.trigger)}`, tone: 'text-muted' }
}

/** The side nearest to its buy line, or a held position's sale, in a sentence. */
const closeness = (item: WatchItem): Closeness => {
  if (item.position) return exitCloseness(item, item.position)
  const sides: [string, number, WatchSide][] = [
    ['YES', item.p_yes, item.yes],
    ['NO', 1 - item.p_yes, item.no],
  ]
  if (item.signal) {
    const [side] = sides.toSorted(([, , a], [, , b]) => (a.distance ?? 1) - (b.distance ?? 1))[0] ?? ['a side']
    return { text: `Buy signal on ${side}`, tone: 'font-semibold text-success' }
  }
  const pending = sides
    .filter(([, , s]) => (s.distance ?? 0) > 0)
    .toSorted(([, , a], [, , b]) => (a.distance ?? 0) - (b.distance ?? 0))[0]
  if (pending) {
    const [side, , s] = pending
    return { text: `${side} has to fall ${pts(s.distance ?? 0)} to ${prob(s.trigger)}`, tone: 'text-muted' }
  }
  const past = sides.find(([, , s]) => s.distance != null && s.ask != null)
  if (past) {
    const [side, p, s] = past
    const ask = s.ask ?? 0
    const why =
      ask < limits.value.minTradePrice
        ? `at ${prob(ask)} is below the trade band`
        : p - ask > limits.value.suspiciousEdge
          ? 'is past its buy line, but the gap is too big to trust'
          : 'is past its buy line, but the order size does not work out'
    return { text: `${side} ${why}`, tone: 'text-muted' }
  }
  if (item.yes.trigger == null && item.no.trigger == null) {
    return { text: 'No price in the trade band leaves enough edge', tone: 'text-muted' }
  }
  return { text: 'Nobody is selling the side that could trigger', tone: 'text-muted' }
}

const researchOn = computed(() => config.value?.values.research_enabled !== false)

/** Research is on and the brief is missing or too old to trade on without researching again. */
const briefStale = (item: WatchItem) =>
  researchOn.value && (!item.brief_at || seconds.value - item.brief_at > maxBriefAge.value)

const rows = computed(() => {
  const q = props.search.trim().toLowerCase()
  const shown = (data.value ?? [])
    .map((item, index) => ({ item, index }))
    .filter(({ item }) => !q || item.question.toLowerCase().includes(q) || item.slug.includes(q))
  return sortRows(shown, columns, sort.value, 'close').map(({ item }) => ({
    item,
    close: closeness(item),
    sides: [
      ['YES', item.yes],
      ['NO', item.no],
    ] as const,
  }))
})

const checked = computed(() => {
  const w = watch.value
  if (!w || !w.last_tick_at) return null
  const next = w.next_tick_at ? `, next in ${countdown(w.next_tick_at, seconds.value)}` : ''
  return `Checked ${ago(w.last_tick_at, seconds.value)}${next}`
})

const empty = computed(() => {
  const w = watch.value
  if (w && w.interval_secs === 0) {
    return { title: 'The price watch is off', description: 'Set how often to check prices under Settings, Price watch.' }
  }
  return {
    title: 'Nothing to watch yet',
    description: `The watch follows markets Jev found answerable and clear in the last ${ttlHours.value} hours, markets you add, and the positions you hold.`,
  }
})

const paused = computed(() => status.value?.state === 'paused')

const { reference, choices, choice, target, offerChoices } = useMarketReference()
const adding = ref(false)
const add = async () => {
  if (!target.value) return
  adding.value = true
  try {
    const added = await api.watch(target.value)
    toast.success(`Watching ${added.question || added.slug}`)
    reference.value = ''
  } catch (e) {
    if (!offerChoices(e)) toast.error(errorMessage(e))
  } finally {
    adding.value = false
  }
}

const removing = ref<string | null>(null)
const remove = async (slug: string) => {
  removing.value = slug
  try {
    await api.unwatch(slug)
    toast.success('Taken off the watchlist')
  } catch (e) {
    toast.error(errorMessage(e))
  } finally {
    removing.value = null
  }
}
</script>

<template>
  <form
    class="flex flex-wrap items-center gap-2"
    @submit.prevent="add"
  >
    <Input
      v-model="reference"
      placeholder="Market link or slug"
      aria-label="Market to watch, as a polymarket.com link or slug"
      class="max-w-sm"
      :disabled="adding"
    />
    <MarketChoiceSelect
      v-if="choices.length"
      v-model="choice"
      :choices="choices"
      :aria-label="`This event has ${choices.length} open markets. Pick one to watch`"
      class="max-w-sm"
    />
    <SimpleTooltip
      tooltip="Jev prices the market first if it never has, which costs a research brief. Then the watch checks its price every minute."
      as-child
    >
      <Button
        type="submit"
        size="sm"
        variant="outline"
        :loading="adding"
        :disabled="!target || (choices.length > 0 && !choice)"
      >
        <Icon name="lucide:eye" />
        Watch market
      </Button>
    </SimpleTooltip>
    <SortSelect
      v-if="data?.length"
      v-model="sort"
      :options="sortOptions"
      class="ml-auto sm:hidden"
    />
  </form>
  <QueryError
    v-if="error"
    :error="error"
    @retry="refetch()"
  />
  <EmptyState
    v-else-if="!isPending && !data?.length"
    icon="lucide:radar"
    :title="empty.title"
    :description="empty.description"
  />
  <section
    v-else
    class="grid gap-2"
  >
    <p class="text-xs text-muted">
      Between research cycles the bot compares Jev's last view with the live prices. It buys when a price falls to its
      buy line, and sells shares it holds early when buyers pay clearly more than Jev thinks they are worth and the sale
      locks in a profit.
      <template v-if="paused">While the loop is paused it only looks and places no orders.</template>
      <template v-if="checked">{{ checked }}.</template>
    </p>
    <Table label="Watchlist">
      <TableHeader>
        <TableRow>
          <TableSortableHead
            v-for="h in heads"
            :key="h.column"
            v-model="sort"
            :column="h.column"
            :sortable="h.sortable"
            :align="h.class === 'text-right' ? 'end' : 'start'"
            :class="h.class"
          >
            <SimpleTooltip
              v-if="h.hint"
              :tooltip="h.hint"
              as-child
            >
              <span class="underline decoration-muted/50 decoration-dotted underline-offset-4">{{ h.label }}</span>
            </SimpleTooltip>
            <template v-else>{{ h.label }}</template>
          </TableSortableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        <TableLoadingRow
          v-if="isPending"
          :colspan="heads.length"
          :rows="6"
        />
        <TableRow
          v-for="{ item, close, sides } in rows"
          v-else
          :key="item.slug"
          class="cursor-pointer"
          :class="item.signal && 'bg-success/10'"
          :data-state="item.slug === selected ? 'selected' : undefined"
          @click="emit('open', item.slug)"
        >
          <TableCell class="max-w-md">
            <div class="flex min-w-0 items-center gap-3">
              <MarketAvatar
                :src="item.image"
                :name="item.question || item.slug"
              />
              <div class="grid min-w-0 justify-items-start">
                <RouterLink
                  :to="{ name: 'markets', params: { slug: item.slug } }"
                  replace
                  class="line-clamp-2 font-medium text-primary hover:underline"
                  @click.stop.prevent="emit('open', item.slug)"
                >
                  {{ item.question || item.slug }}
                </RouterLink>
                <span
                  v-if="item.position || item.pinned"
                  class="mt-0.5 flex items-center gap-2 text-xs text-muted"
                >
                  <span
                    v-if="item.position"
                    class="inline-flex items-center gap-1"
                  >
                    <Icon
                      name="lucide:wallet"
                      aria-hidden="true"
                    />
                    You hold {{ item.position.size }} {{ item.position.outcome }} at {{ prob(item.position.avg_price) }}
                  </span>
                  <span
                    v-if="item.pinned"
                    class="inline-flex items-center gap-1"
                  >
                    <Icon
                      name="lucide:eye"
                      aria-hidden="true"
                    />
                    Added by you
                    <Button
                      size="xs"
                      variant="ghost"
                      class="h-5 px-1 text-muted hover:text-primary"
                      :loading="removing === item.slug"
                      :aria-label="`Stop watching ${item.question || item.slug}`"
                      @click.stop="remove(item.slug)"
                    >
                      <Icon name="lucide:x" />
                    </Button>
                  </span>
                </span>
              </div>
            </div>
          </TableCell>
          <TableCell
            label="Jev"
            labelled
            class="text-right tabular-nums"
          >
            {{ percent(item.p_yes) }}
          </TableCell>
          <TableCell
            label="Price"
            labelled
            class="text-right text-muted tabular-nums"
          >
            {{ percent(item.midpoint) }}
          </TableCell>
          <TableCell
            v-if="item.position"
            :colspan="2"
            :label="`Sells ${item.position.outcome} at`"
            labelled
            class="text-right whitespace-nowrap tabular-nums"
          >
            <SimpleTooltip
              :tooltip="`The lowest price the bot sells your ${item.position.outcome} shares at early: Jev's estimate plus a margin, and enough profit on what you paid. Below it, what buyers pay now.`"
              as-child
            >
              <span class="grid">
                <span :class="item.position.trigger == null ? 'text-muted' : 'text-primary'">
                  sells at {{ prob(item.position.trigger) }}
                </span>
                <span class="text-xs text-muted">bid now {{ prob(item.position.bid) }}</span>
              </span>
            </SimpleTooltip>
          </TableCell>
          <template v-else>
            <TableCell
              v-for="[side, s] in sides"
              :key="side"
              :label="`Buys ${side} at`"
              labelled
              class="text-right whitespace-nowrap tabular-nums"
            >
              <span class="grid">
                <span :class="s.trigger == null ? 'text-muted' : 'text-primary'">{{ prob(s.trigger) }}</span>
                <span class="text-xs text-muted">now {{ prob(s.ask) }}</span>
              </span>
            </TableCell>
          </template>
          <TableCell
            label="How close"
            class="text-sm whitespace-nowrap"
            :class="close.tone"
          >
            {{ close.text }}
          </TableCell>
          <TableCell
            label="Resolves"
            labelled
            class="text-right text-xs whitespace-nowrap"
          >
            <span
              v-if="item.end_date"
              class="grid"
            >
              <span class="text-primary">{{ day(item.end_date) }}</span>
              <span class="text-muted">{{ inDays(item.end_date, seconds) }}</span>
            </span>
            <span
              v-else
              class="text-muted"
              >–</span
            >
          </TableCell>
          <TableCell
            label="Brief"
            class="text-right text-xs whitespace-nowrap"
            :class="briefStale(item) ? 'text-warning' : 'text-muted'"
          >
            {{ item.brief_at ? ago(item.brief_at, seconds) : 'No brief' }}
          </TableCell>
        </TableRow>
        <TableRow v-if="!isPending && !rows.length">
          <TableCell
            :colspan="heads.length"
            class="py-10 text-center text-sm text-muted"
          >
            No watched market matches your search.
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>
  </section>
</template>
