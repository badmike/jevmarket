<script setup lang="ts">
import { useNow } from '@vueuse/core'

import { useLive } from '~/api/live'
import { useRecommendations } from '~/api/queries'
import type { Recommendation } from '~/api/types'
import DecideDialog from '~/components/DecideDialog.vue'
import QueryError from '~/components/QueryError.vue'
import RecommendationSheet from '~/components/RecommendationSheet.vue'
import VerdictBadge from '~/components/VerdictBadge.vue'
import { EmptyState } from '~/components/ui/empty-state'
import { Input } from '~/components/ui/input'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
  TableSortableHead,
} from '~/components/ui/table'
import { Tabs, TabsList, TabsTrigger } from '~/components/ui/tabs'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useThresholds } from '~/composables/useSettings'
import { ago, points, prob } from '~/lib/format'
import type { TableSort } from '~/lib/table'

type Filter = 'all' | 'trade' | 'skip'

const route = useRoute()
const router = useRouter()
const { data, error, isPending, refetch } = useRecommendations()
const { touched } = useLive()
const limits = useThresholds()
const now = useNow({ interval: 30_000 })

const search = ref('')
const filter = ref<Filter>('all')
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

const rows = computed(() => {
  const q = search.value.trim().toLowerCase()
  const key = columns[sort.value.column] ?? columns.ts!
  const dir = sort.value.direction === 'asc' ? 1 : -1
  return (data.value ?? [])
    .filter((r) => filter.value === 'all' || (filter.value === 'trade') === isTrade(r))
    .filter((r) => !q || r.question.toLowerCase().includes(q) || r.slug.includes(q))
    .toSorted((a, b) => (key(a) - key(b)) * dir)
})

const counts = computed(() => {
  const all = data.value ?? []
  const trade = all.filter(isTrade).length
  return { all: all.length, trade, skip: all.length - trade }
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

    <div class="flex flex-wrap items-center gap-2">
      <Input
        v-model="search"
        type="search"
        placeholder="Search markets  /"
        aria-label="Search markets"
        class="max-w-xs"
      />
      <Tabs v-model="filter">
        <TabsList aria-label="Filter by verdict">
          <TabsTrigger value="all">All {{ counts.all }}</TabsTrigger>
          <TabsTrigger value="trade">Trade {{ counts.trade }}</TabsTrigger>
          <TabsTrigger value="skip">Skip {{ counts.skip }}</TabsTrigger>
        </TabsList>
      </Tabs>
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
          <TableHead class="w-full min-w-64">Market</TableHead>
          <TableSortableHead
            v-model="sort"
            column="p_yes"
            align="end"
          >
            Jev
          </TableSortableHead>
          <TableHead class="text-right">Market</TableHead>
          <TableSortableHead
            v-model="sort"
            column="edge"
            align="end"
          >
            Edge
          </TableSortableHead>
          <TableSortableHead
            v-model="sort"
            column="answerable"
            align="end"
          >
            Answerable
          </TableSortableHead>
          <TableHead class="text-right">Clarity</TableHead>
          <TableHead>Verdict</TableHead>
          <TableSortableHead
            v-model="sort"
            column="ts"
            align="end"
          >
            Updated
          </TableSortableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        <TableLoadingRow
          v-if="isPending"
          :colspan="8"
          :rows="8"
        />
        <TableRow
          v-for="r in rows"
          v-else
          :key="r.slug"
          class="cursor-pointer"
          :class="touched.has(r.slug) && 'bg-accent/10'"
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
            {{ prob(r.p_yes) }}
          </TableCell>
          <TableCell
            label="Market"
            labelled
            class="text-right text-muted tabular-nums"
          >
            {{ prob(r.midpoint) }}
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
            {{ prob(r.answerable) }}
          </TableCell>
          <TableCell
            label="Clarity"
            labelled
            class="text-right tabular-nums"
            :class="r.clarity != null && r.clarity < limits.minClarity && 'text-muted'"
          >
            {{ r.clarity ?? '–' }}
          </TableCell>
          <TableCell label="Verdict">
            <VerdictBadge
              :action="r.action"
              :title="r.reason"
            />
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
            :colspan="8"
            class="py-10 text-center text-sm text-muted"
          >
            No market matches.
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
