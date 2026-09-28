<script setup lang="ts">
import { useStats } from '~/api/queries'
import type { Group, Pnl } from '~/api/types'
import PnlChart from '~/components/charts/PnlChart.vue'
import ProbabilityChart from '~/components/charts/ProbabilityChart.vue'
import QueryError from '~/components/QueryError.vue'
import StatTile from '~/components/StatTile.vue'
import { Card } from '~/components/ui/card'
import { EmptyState } from '~/components/ui/empty-state'
import { Skeleton } from '~/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '~/components/ui/table'
import { Tabs, TabsList, TabsTrigger } from '~/components/ui/tabs'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { count, percent, prob, signedUsd, usd } from '~/lib/format'

const { data, error, isPending, refetch } = useStats()

type GroupKey = 'by_edge' | 'by_answerable' | 'by_clarity' | 'by_variant'
const groupBy = ref<GroupKey>('by_edge')
const groupTabs: { value: GroupKey; label: string; hint: string }[] = [
  { value: 'by_edge', label: 'Edge', hint: "Edge: how much better Jev's estimate is than the market price." },
  {
    value: 'by_answerable',
    label: 'Answerable',
    hint: 'Answerable: how sure Jev was that it had enough information to judge the question.',
  },
  { value: 'by_clarity', label: 'Clarity', hint: "Clarity: how clear the market's resolution rules are, from 0 to 4." },
  { value: 'by_variant', label: 'Market price in state', hint: 'Whether Jev saw the market price when it decided.' },
]
const groupHint = computed(() => groupTabs.find((t) => t.value === groupBy.value)?.hint)

const all = computed(() => data.value?.calibration.all)
const groups = computed<Group[]>(() => data.value?.calibration[groupBy.value] ?? [])

const reliability = computed(() =>
  (data.value?.reliability ?? []).map((b) => ({
    x: b.avg_p,
    y: b.yes_rate,
    n: b.n,
    label: `Jev ${prob(b.avg_p)} on average, ${percent(b.yes_rate)} resolved YES, ${b.n} markets`,
  }))
)

const buckets = computed(() =>
  (data.value?.buckets ?? []).map((b) => ({
    x: b.avg_p,
    y: b.avg_market,
    n: b.n,
    label: `Jev ${prob(b.avg_p)} against market ${prob(b.avg_market)}, ${b.n} decisions`,
  }))
)

const verdict = computed(() => {
  const g = all.value
  if (!g?.n) return null
  const diff = g.brier_market - g.brier_jev
  if (Math.abs(diff) < 0.002) return 'Jev and the market are about even'
  return diff > 0 ? 'Jev is better calibrated than the market' : 'The market is better calibrated than Jev'
})

const pnlDetail = (p: Pnl) =>
  `${p.resolved} of ${p.orders} orders resolved, ${usd(p.staked_usd)} staked, ${usd(p.payout_usd)} paid out`
</script>

<template>
  <div class="mx-auto grid max-w-7xl gap-6">
    <header class="grid gap-1">
      <h1 class="text-2xl font-semibold text-primary">Stats</h1>
      <p class="text-sm text-muted">
        Whether Jev beats the market, measured on markets that have resolved. Resolutions are checked at the start of
        every pass.
      </p>
    </header>

    <QueryError
      v-if="error"
      :error="error"
      @retry="refetch()"
    />
    <div
      v-else-if="isPending"
      class="grid gap-3 sm:grid-cols-2 xl:grid-cols-4"
    >
      <Skeleton
        v-for="i in 4"
        :key="i"
        class="h-26 rounded-xl"
      />
    </div>
    <template v-else-if="data">
      <div class="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <StatTile
          title="Decisions"
          hint="A decision is Jev's estimate for one market, with a verdict to buy or skip. Trade signals are the buys."
          icon="lucide:scale"
          :detail="`${count(data.trade_signals)} trade signals`"
        >
          {{ count(data.decisions) }}
        </StatTile>
        <StatTile
          title="Briefs"
          hint="A brief is the researcher's summary of recent news and facts on a market. Jev prices from it."
          icon="lucide:newspaper"
          :detail="`${usd(data.research_cost_usd, true)} on research`"
        >
          {{ count(data.briefs) }}
        </StatTile>
        <StatTile
          title="OpenRouter spend"
          hint="What the AI models cost: Jev's price estimates plus the web research for briefs."
          icon="lucide:coins"
          :detail="`Jev ${usd(data.jev_cost_usd, true)}, research ${usd(data.research_cost_usd, true)}`"
        >
          {{ usd(data.jev_cost_usd + data.research_cost_usd, true) }}
        </StatTile>
        <StatTile
          title="Live orders"
          hint="Real orders placed on Polymarket. Dry runs are not counted."
          icon="lucide:receipt"
          :detail="`${usd(data.live_usd)} placed`"
        >
          {{ count(data.live_orders) }}
        </StatTile>
      </div>

      <Card class="grid gap-4 p-4 sm:p-6">
        <div class="grid gap-1">
          <h2 class="text-base font-semibold text-primary">Calibration</h2>
          <p class="text-sm text-muted">
            Whether Jev's odds come true as often as it says: of the markets it called 70% likely, about 7 in 10
            should happen.
          </p>
        </div>
        <EmptyState
          v-if="!all?.n"
          icon="lucide:hourglass"
          title="No resolved markets yet"
          description="Calibration needs markets that were decided and have since resolved. Check back after the first ones close."
        />
        <template v-else>
          <dl class="grid gap-3 sm:grid-cols-4">
            <div>
              <dt class="text-xs text-muted">Resolved markets</dt>
              <dd class="text-xl font-semibold text-primary tabular-nums">{{ count(all.n) }}</dd>
            </div>
            <div>
              <dt class="flex items-center gap-1.5 text-xs text-muted">
                <span
                  class="size-2 rounded-full bg-viz-1"
                  aria-hidden="true"
                />
                <SimpleTooltip
                  tooltip="Brier score: how far predictions were from what happened. Lower is better; always guessing 50% scores 0.25."
                  as-child
                >
                  <span class="underline decoration-muted/50 decoration-dotted underline-offset-4">Brier, Jev</span>
                </SimpleTooltip>
              </dt>
              <dd class="text-xl font-semibold text-primary tabular-nums">{{ prob(all.brier_jev, 3) }}</dd>
            </div>
            <div>
              <dt class="flex items-center gap-1.5 text-xs text-muted">
                <span
                  class="size-2 rounded-full bg-viz-2"
                  aria-hidden="true"
                />
                Brier, market
              </dt>
              <dd class="text-xl font-semibold text-primary tabular-nums">{{ prob(all.brier_market, 3) }}</dd>
            </div>
            <div>
              <dt class="text-xs text-muted">
                <SimpleTooltip
                  tooltip="Hit rate: how often the side Jev favoured over the market price won."
                  as-child
                >
                  <span class="underline decoration-muted/50 decoration-dotted underline-offset-4">Hit rate</span>
                </SimpleTooltip>
              </dt>
              <dd class="text-xl font-semibold text-primary tabular-nums">{{ percent(all.hit_rate) }}</dd>
            </div>
          </dl>
          <p class="text-sm font-medium text-primary">{{ verdict }}.</p>
        </template>
      </Card>

      <div class="grid gap-4 lg:grid-cols-2">
        <Card class="grid content-start gap-3 p-4 sm:p-6">
          <div class="grid gap-1">
            <h2 class="text-base font-semibold text-primary">Reliability</h2>
            <p class="text-sm text-muted">
              How often markets resolved YES for each band of Jev's probability. On the diagonal, Jev meant exactly
              what it said.
            </p>
          </div>
          <ProbabilityChart
            v-if="reliability.length"
            :points="reliability"
            x-label="Jev P(YES)"
            y-label="Resolved YES"
          />
          <EmptyState
            v-else
            icon="lucide:chart-scatter"
            title="Waiting for resolutions"
          />
        </Card>
        <Card class="grid content-start gap-3 p-4 sm:p-6">
          <div class="grid gap-1">
            <h2 class="text-base font-semibold text-primary">Jev against the market</h2>
            <p class="text-sm text-muted">
              Every decision, grouped by Jev's probability. Points off the diagonal are where Jev disagrees with the
              market price.
            </p>
          </div>
          <ProbabilityChart
            v-if="buckets.length"
            :points="buckets"
            x-label="Jev P(YES)"
            y-label="Market midpoint"
            :series="2"
          />
          <EmptyState
            v-else
            icon="lucide:chart-scatter"
            title="No decisions yet"
          />
        </Card>
      </div>

      <Card class="grid gap-4 p-4 sm:p-6">
        <div class="grid gap-1">
          <h2 class="text-base font-semibold text-primary">Profit and loss</h2>
          <p class="text-sm text-muted">Orders on resolved markets, each assumed filled at its limit price.</p>
        </div>
        <dl class="grid gap-3 sm:grid-cols-2">
          <div>
            <dt class="flex items-center gap-1.5 text-xs text-muted">
              <span
                class="size-2 rounded-full bg-viz-1"
                aria-hidden="true"
              />
              Live
            </dt>
            <dd
              class="text-xl font-semibold tabular-nums"
              :class="data.live_pnl.pnl_usd < 0 ? 'text-destructive' : 'text-primary'"
            >
              {{ signedUsd(data.live_pnl.pnl_usd) }}
            </dd>
            <dd class="text-xs text-muted">{{ pnlDetail(data.live_pnl) }}</dd>
          </div>
          <div>
            <dt class="flex items-center gap-1.5 text-xs text-muted">
              <span
                class="size-2 rounded-full bg-viz-2"
                aria-hidden="true"
              />
              <SimpleTooltip
                tooltip="Dry run orders were only pretended, scored as if they had filled."
                as-child
              >
                <span class="underline decoration-muted/50 decoration-dotted underline-offset-4">Dry run</span>
              </SimpleTooltip>
            </dt>
            <dd
              class="text-xl font-semibold tabular-nums"
              :class="data.dry_run_pnl.pnl_usd < 0 ? 'text-destructive' : 'text-primary'"
            >
              {{ signedUsd(data.dry_run_pnl.pnl_usd) }}
            </dd>
            <dd class="text-xs text-muted">{{ pnlDetail(data.dry_run_pnl) }}</dd>
          </div>
        </dl>
        <PnlChart
          v-if="data.pnl_series.length"
          :points="data.pnl_series"
        />
        <EmptyState
          v-else
          icon="lucide:chart-line"
          title="No resolved orders yet"
          description="The curve starts when the first market with an order resolves."
        />
      </Card>

      <section class="grid gap-3">
        <div class="flex flex-wrap items-end justify-between gap-2">
          <div class="grid gap-1">
            <h2 class="text-base font-semibold text-primary">Hit rate and Brier by group</h2>
            <p class="text-sm text-muted">{{ groupHint }}</p>
          </div>
          <Tabs v-model="groupBy">
            <TabsList aria-label="Group by">
              <TabsTrigger
                v-for="t in groupTabs"
                :key="t.value"
                :value="t.value"
              >
                {{ t.label }}
              </TabsTrigger>
            </TabsList>
          </Tabs>
        </div>
        <Table label="Calibration by group">
          <TableHeader>
            <TableRow>
              <TableHead>Group</TableHead>
              <TableHead class="text-right">Markets</TableHead>
              <TableHead class="w-full min-w-40">Hit rate</TableHead>
              <TableHead class="text-right">Brier Jev</TableHead>
              <TableHead class="text-right">Brier market</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow
              v-for="g in groups"
              :key="g.label"
            >
              <TableCell class="font-medium whitespace-nowrap text-primary">{{ g.label }}</TableCell>
              <TableCell
                label="Markets"
                class="text-right tabular-nums"
              >
                {{ g.n }}
              </TableCell>
              <TableCell label="Hit rate">
                <div class="flex items-center gap-2">
                  <div class="h-2 flex-1 rounded-full bg-secondary">
                    <div
                      class="h-full rounded-full bg-viz-1"
                      :style="{ width: `${g.hit_rate * 100}%` }"
                    />
                  </div>
                  <span class="w-10 text-right tabular-nums">{{ percent(g.hit_rate) }}</span>
                </div>
              </TableCell>
              <TableCell
                label="Brier Jev"
                class="text-right tabular-nums"
                :class="g.brier_jev < g.brier_market && 'font-semibold text-primary'"
              >
                {{ prob(g.brier_jev, 3) }}
              </TableCell>
              <TableCell
                label="Brier market"
                class="text-right tabular-nums"
                :class="g.brier_market < g.brier_jev && 'font-semibold text-primary'"
              >
                {{ prob(g.brier_market, 3) }}
              </TableCell>
            </TableRow>
            <TableRow v-if="!groups.length">
              <TableCell
                :colspan="5"
                class="py-8 text-center text-sm text-muted"
              >
                No resolved markets in any group yet.
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </section>
    </template>
  </div>
</template>
