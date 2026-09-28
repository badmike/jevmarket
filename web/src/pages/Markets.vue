<script setup lang="ts">
import { useNow } from '@vueuse/core'

import { useLive } from '~/api/live'
import { useRecommendations } from '~/api/queries'
import type { Recommendation } from '~/api/types'
import DecideDialog from '~/components/DecideDialog.vue'
import Icon from '~/components/Icon.vue'
import MarketVerdict from '~/components/MarketVerdict.vue'
import QueryError from '~/components/QueryError.vue'
import RecommendationSheet from '~/components/RecommendationSheet.vue'
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
import { Tabs, TabsList, TabsTrigger } from '~/components/ui/tabs'
import { SimpleTooltip } from '~/components/ui/tooltip'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useReadState } from '~/composables/useReadState'
import { useThresholds } from '~/composables/useSettings'
import { ago, percent, points } from '~/lib/format'
import type { TableSort } from '~/lib/table'

type Filter = 'all' | 'trade' | 'skip'

const route = useRoute()
const router = useRouter()
const { data, error, isPending, refetch } = useRecommendations()
const { touched } = useLive()
const { isRead, markRead } = useReadState('markets')
const limits = useThresholds()
const now = useNow({ interval: 30_000 })

const search = ref('')
const filter = ref<Filter>('all')
/** Settled markets (resolved or past their end date) are history, kept out of the way. */
const view = ref<'open' | 'settled'>('open')
const inView = computed(() => (data.value ?? []).filter((r) => r.settled === (view.value === 'settled')))
const sort = ref<TableSort>({ column: 'edge', direction: 'desc' })

const selected = computed(() => (typeof route.params.slug === 'string' ? route.params.slug : null))
const open = (slug: string | null) =>
  router.replace({ name: 'markets', params: slug ? { slug } : {} })

const isTrade = (r: Recommendation) => r.action.startsWith('trade')

const columns: Record<string, (r: Recommendation) => number> = {
  edge: (r) => r.edge ?? -Infinity,
  p_yes: (r) => r.p_yes ?? -Infinity,
  answerable: (r) => r.answerable ?? -Infinity,
  ts: (r) => r.ts,
}

const edgeNeeded = computed(() => `${Math.round(limits.value.minEdge * 100)} points`)

/** Column headers in table order, each with a one-line explanation for newcomers. */
const heads = computed(() => {
  const { minAnswerable, minClarity } = limits.value
  return [
    { column: 'question', label: 'Market', hint: '', class: 'w-full min-w-64', align: 'start' as const },
    { column: 'p_yes', label: 'Jev', hint: 'How likely Jev, the AI model, thinks YES is.' },
    {
      column: 'midpoint',
      label: 'Price',
      hint: 'What YES costs on the market, roughly how likely traders think YES is.',
    },
    {
      column: 'edge',
      label: 'Edge',
      hint: `How far Jev's estimate beats the price on the better side, YES or NO. The bot needs ${edgeNeeded.value}.`,
    },
    {
      column: 'answerable',
      label: 'Answerable',
      hint: `How sure Jev is that public information can settle the question today. The bot needs ${percent(minAnswerable)}.`,
    },
    {
      column: 'clarity',
      label: 'Clarity',
      hint: `How clear the market's rules are, from 0 to 4. The bot needs ${minClarity}.`,
    },
    {
      column: 'verdict',
      label: 'Verdict',
      hint: 'What the bot did and why. Hover a verdict for the technical reason.',
      align: 'start' as const,
    },
    { column: 'ts', label: 'Updated', hint: 'When Jev last looked at the market.' },
  ].map((h) => ({ align: 'end' as const, ...h, sortable: h.column in columns }))
})

const rows = computed(() => {
  const q = search.value.trim().toLowerCase()
  const key = columns[sort.value.column] ?? columns.ts!
  const dir = sort.value.direction === 'asc' ? 1 : -1
  return inView.value
    .filter((r) => filter.value === 'all' || (filter.value === 'trade') === isTrade(r))
    .filter((r) => !q || r.question.toLowerCase().includes(q) || r.slug.includes(q))
    .toSorted((a, b) => (key(a) - key(b)) * dir)
})

const unread = computed(() => rows.value.filter((r) => !isRead(r.slug, r.ts)))
const markAllRead = () => markRead(unread.value.map((r) => ({ key: r.slug, ts: r.ts })))

const counts = computed(() => {
  const all = inView.value
  const trade = all.filter(isTrade).length
  const settled = (data.value ?? []).filter((r) => r.settled).length
  return { all: all.length, trade, skip: all.length - trade, open: (data.value?.length ?? 0) - settled, settled }
})

const emptyMessage = computed(() => {
  if (search.value.trim()) return 'No market matches your search.'
  if (!inView.value.length) return view.value === 'open' ? 'No open markets right now.' : 'No settled markets yet.'
  return filter.value === 'trade' ? 'No trades here yet.' : 'Nothing skipped here.'
})
</script>

<template>
  <div class="mx-auto grid max-w-7xl gap-4">
    <header class="flex flex-wrap items-end justify-between gap-3">
      <div class="grid gap-1">
        <h1 class="text-2xl font-semibold text-primary">Markets</h1>
        <p class="text-sm text-muted">The latest Jev view per market, from passes and single decisions.</p>
      </div>
      <DecideDialog />
    </header>

    <details class="group text-sm">
      <summary class="inline-flex cursor-pointer items-center gap-1 text-muted hover:text-primary">
        <Icon
          name="lucide:chevron-right"
          class="transition-transform group-open:rotate-90"
          aria-hidden="true"
        />
        How to read this
      </summary>
      <div class="mt-2 grid gap-3 rounded-lg bg-muted-background/60 p-3">
        <p>
          Jev, an AI model, reads the news on each market and estimates how likely YES is. The bot compares that to
          the price and buys YES or NO when Jev's estimate is at least {{ edgeNeeded }} better and the
          question is answerable and clear enough. Everything else is skipped.
        </p>
        <dl class="grid gap-x-6 gap-y-2 sm:grid-cols-2">
          <div
            v-for="h in heads.filter((h) => h.hint)"
            :key="h.column"
          >
            <dt class="mr-1 inline font-semibold text-primary">{{ h.label }}:</dt>
            <dd class="inline text-muted">{{ h.hint }}</dd>
          </div>
        </dl>
      </div>
    </details>

    <div class="flex flex-wrap items-center gap-2">
      <Input
        v-model="search"
        type="search"
        placeholder="Search markets  /"
        aria-label="Search markets"
        class="max-w-xs"
      />
      <Tabs v-model="view">
        <TabsList aria-label="Open or settled markets">
          <TabsTrigger value="open">Open {{ counts.open }}</TabsTrigger>
          <TabsTrigger value="settled">Settled {{ counts.settled }}</TabsTrigger>
        </TabsList>
      </Tabs>
      <Tabs v-model="filter">
        <TabsList aria-label="Filter by verdict">
          <TabsTrigger value="all">All {{ counts.all }}</TabsTrigger>
          <TabsTrigger value="trade">Trade {{ counts.trade }}</TabsTrigger>
          <TabsTrigger value="skip">Skip {{ counts.skip }}</TabsTrigger>
        </TabsList>
      </Tabs>
      <Button
        variant="ghost"
        size="sm"
        class="ml-auto text-muted"
        :disabled="!unread.length"
        @click="markAllRead"
      >
        <Icon name="lucide:check-check" />
        Mark all read
      </Button>
    </div>

    <QueryError
      v-if="error"
      :error="error"
      @retry="refetch()"
    />
    <EmptyState
      v-else-if="!isPending && !data?.length"
      icon="lucide:list-checks"
      title="No decisions yet"
      description="Each pass logs a decision per market. Decide one market by hand to see how it works."
    />
    <Table
      v-else
      label="Markets"
    >
      <TableHeader>
        <TableRow>
          <TableSortableHead
            v-for="h in heads"
            :key="h.column"
            v-model="sort"
            :column="h.column"
            :sortable="h.sortable"
            :align="h.align"
            :class="[h.class, !h.sortable && h.align === 'end' && 'text-right']"
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
          :rows="8"
        />
        <TableRow
          v-for="r in rows"
          v-else
          :key="r.slug"
          class="cursor-pointer"
          :class="[
            touched.has(r.slug) && 'bg-accent/10',
            r.slug !== selected &&
              isRead(r.slug, r.ts) &&
              'opacity-60 focus-within:opacity-100 hover:opacity-100',
          ]"
          :data-state="r.slug === selected ? 'selected' : undefined"
          @click="open(r.slug)"
        >
          <TableCell class="max-w-md">
            <RouterLink
              :to="{ name: 'markets', params: { slug: r.slug } }"
              replace
              class="line-clamp-2 font-medium text-primary hover:underline"
              @click.stop
            >
              {{ r.question || r.slug }}
            </RouterLink>
          </TableCell>
          <TableCell
            label="Jev"
            labelled
            class="text-right tabular-nums"
          >
            {{ percent(r.p_yes) }}
          </TableCell>
          <TableCell
            label="Price"
            labelled
            class="text-right text-muted tabular-nums"
          >
            {{ percent(r.midpoint) }}
          </TableCell>
          <TableCell
            label="Edge"
            labelled
            class="text-right whitespace-nowrap tabular-nums"
            :class="isTrade(r) && 'font-semibold text-success'"
          >
            {{ points(r.edge) }}
            <span class="text-xs text-muted">{{ r.side }}</span>
          </TableCell>
          <TableCell
            label="Answerable"
            labelled
            class="text-right tabular-nums"
            :class="r.answerable != null && r.answerable < limits.minAnswerable && 'text-muted'"
          >
            {{ percent(r.answerable) }}
          </TableCell>
          <TableCell
            label="Clarity"
            labelled
            class="text-right whitespace-nowrap tabular-nums"
            :class="r.clarity != null && r.clarity < limits.minClarity && 'text-muted'"
          >
            <template v-if="r.clarity != null">{{ r.clarity }}<span class="text-xs text-muted">/4</span></template>
            <template v-else>–</template>
          </TableCell>
          <TableCell label="Verdict">
            <MarketVerdict :recommendation="r" />
          </TableCell>
          <TableCell
            label="Updated"
            class="text-right text-xs whitespace-nowrap text-muted"
          >
            {{ ago(r.ts, now.getTime() / 1000) }}
          </TableCell>
        </TableRow>
        <TableRow v-if="!isPending && !rows.length">
          <TableCell
            :colspan="heads.length"
            class="py-10 text-center text-sm text-muted"
          >
            {{ emptyMessage }}
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>

    <RecommendationSheet
      :slug="selected"
      @close="open(null)"
    />
  </div>
</template>
