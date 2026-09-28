<script setup lang="ts">
import { useNow } from '@vueuse/core'

import { useOrders } from '~/api/queries'
import type { OpenOrder, OrderEvent } from '~/api/types'
import OrderStatus from '~/components/OrderStatus.vue'
import QueryError from '~/components/QueryError.vue'
import { Badge } from '~/components/ui/badge'
import { EmptyState } from '~/components/ui/empty-state'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '~/components/ui/table'
import { Tabs, TabsList, TabsTrigger } from '~/components/ui/tabs'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { ago, at, prob, usd } from '~/lib/format'

/** Open orders on the exchange and the log of every order placed, or in dry runs, not placed. */
defineProps<{ openOrders: OpenOrder[] | undefined }>()

type LogFilter = 'all' | 'live' | 'dry' | 'rejected'

const { data: log, error, isPending, refetch } = useOrders()
const now = useNow({ interval: 60_000 })
const filter = ref<LogFilter>('all')

const FAILED = ['rejected', 'failed', 'refused']
const isRejected = (o: OrderEvent) => !o.dry_run && FAILED.includes(o.status)
const matches: Record<LogFilter, (o: OrderEvent) => boolean> = {
  all: () => true,
  live: (o) => !o.dry_run,
  dry: (o) => o.dry_run,
  rejected: isRejected,
}

const counts = computed(() => {
  const all = log.value ?? []
  return {
    all: all.length,
    live: all.filter(matches.live).length,
    dry: all.filter(matches.dry).length,
    rejected: all.filter(isRejected).length,
  }
})
const rows = computed(() => (log.value ?? []).filter(matches[filter.value]))

/** `BUY` and `NO` read as "Buy No"; mixed-case outcomes like team names stay as they are. */
const word = (s: string) => (s === s.toUpperCase() ? s.charAt(0) + s.slice(1).toLowerCase() : s)
const shares = (n: number) => `${n.toFixed(2)} shares`
</script>

<template>
  <section
    v-if="openOrders"
    class="grid gap-3"
  >
    <div class="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
      <h2 class="text-base font-semibold text-primary">Open orders</h2>
      <p class="text-sm text-muted">
        <template v-if="openOrders.length">Buy orders waiting for a seller at their price</template>
        <template v-else>Nothing waiting to fill</template>
      </p>
    </div>
    <Table
      v-if="openOrders.length"
      label="Open orders"
    >
      <TableHeader>
        <TableRow>
          <TableHead class="w-full min-w-64">Market</TableHead>
          <TableHead class="text-right">Price</TableHead>
          <TableHead class="text-right">Filled</TableHead>
          <TableHead class="text-right">Amount</TableHead>
          <TableHead>Status</TableHead>
          <TableHead class="text-right">Placed</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        <TableRow
          v-for="o in openOrders"
          :key="o.id"
        >
          <TableCell>
            <div class="grid gap-0.5">
              <RouterLink
                v-if="o.slug"
                :to="{ name: 'markets', params: { slug: o.slug } }"
                class="font-medium text-primary hover:underline"
              >
                {{ o.title ?? o.slug }}
              </RouterLink>
              <span
                v-else
                class="truncate font-mono text-xs text-muted"
                :title="o.market"
              >
                Market {{ o.market.slice(0, 10) }}…
              </span>
              <span class="text-xs text-muted">{{ word(o.side) }} {{ word(o.outcome) }}</span>
            </div>
          </TableCell>
          <TableCell
            label="Price"
            labelled
            class="text-right tabular-nums"
          >
            {{ prob(o.price, 3) }}
          </TableCell>
          <TableCell
            label="Filled"
            labelled
            class="text-right whitespace-nowrap tabular-nums"
          >
            {{ o.matched.toFixed(2) }} of {{ shares(o.size) }}
          </TableCell>
          <TableCell
            label="Amount"
            labelled
            class="text-right tabular-nums"
          >
            {{ usd(o.price * o.size) }}
          </TableCell>
          <TableCell label="Status">
            <Badge
              variant="info"
              size="sm"
              class="whitespace-nowrap"
            >
              {{ o.status.toLowerCase() }}
            </Badge>
          </TableCell>
          <TableCell
            label="Placed"
            class="text-right text-xs whitespace-nowrap text-muted"
            :title="at(o.created_at)"
          >
            {{ ago(o.created_at, now.getTime() / 1000) }}
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>
  </section>

  <section class="grid gap-3">
    <div class="flex flex-wrap items-end justify-between gap-3">
      <div class="grid gap-1">
        <h2 class="text-base font-semibold text-primary">Order log</h2>
        <p class="text-sm text-muted">Every order the bot or you placed, and in dry runs, would have placed.</p>
      </div>
      <Tabs
        v-if="log?.length"
        v-model="filter"
      >
        <TabsList aria-label="Filter the order log">
          <TabsTrigger value="all">All {{ counts.all }}</TabsTrigger>
          <TabsTrigger value="live">Live {{ counts.live }}</TabsTrigger>
          <TabsTrigger value="dry">Dry run {{ counts.dry }}</TabsTrigger>
          <TabsTrigger value="rejected">Rejected {{ counts.rejected }}</TabsTrigger>
        </TabsList>
      </Tabs>
    </div>
    <QueryError
      v-if="error"
      :error="error"
      @retry="refetch()"
    />
    <EmptyState
      v-else-if="log && !log.length"
      icon="lucide:scroll-text"
      title="No orders yet"
      description="Orders from the bot and from the console show up here, dry runs included."
    />
    <Table
      v-else
      label="Order log"
    >
      <TableHeader>
        <TableRow>
          <TableHead class="w-full min-w-64">Market</TableHead>
          <TableHead>Order</TableHead>
          <TableHead class="text-right">Amount</TableHead>
          <TableHead>Status</TableHead>
          <TableHead class="text-right">When</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        <TableLoadingRow
          v-if="isPending"
          :colspan="5"
          :rows="4"
        />
        <TableRow
          v-for="o in rows"
          :key="`${o.ts}-${o.slug}`"
        >
          <TableCell>
            <div class="grid justify-items-start gap-1">
              <RouterLink
                :to="{ name: 'markets', params: { slug: o.slug } }"
                class="hover:underline"
                :class="o.dry_run ? 'text-muted' : 'font-medium text-primary'"
              >
                {{ o.title }}
              </RouterLink>
              <Badge
                v-if="o.manual"
                variant="surface"
                size="sm"
              >
                Manual
              </Badge>
              <p
                v-if="o.message"
                class="text-xs break-words"
                :class="isRejected(o) ? 'text-destructive' : 'text-muted'"
              >
                {{ o.message }}
              </p>
            </div>
          </TableCell>
          <TableCell
            label="Order"
            class="whitespace-nowrap tabular-nums"
            :class="o.dry_run ? 'text-muted' : 'text-primary'"
          >
            Buy {{ word(o.outcome) }}
            <span class="text-muted">{{ shares(o.size) }} at {{ prob(o.price, 3) }}</span>
          </TableCell>
          <TableCell
            label="Amount"
            labelled
            class="text-right tabular-nums"
            :class="o.dry_run ? 'text-muted' : 'font-medium text-primary'"
          >
            {{ usd(o.usd) }}
          </TableCell>
          <TableCell label="Status">
            <OrderStatus
              :status="o.status"
              :dry-run="o.dry_run"
            />
          </TableCell>
          <TableCell
            label="When"
            class="text-right text-xs whitespace-nowrap text-muted"
            :title="at(o.ts)"
          >
            {{ ago(o.ts, now.getTime() / 1000) }}
          </TableCell>
        </TableRow>
        <TableRow v-if="log?.length && !rows.length">
          <TableCell
            :colspan="5"
            class="py-8 text-center text-sm text-muted"
          >
            No orders match this filter.
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>
  </section>
</template>
