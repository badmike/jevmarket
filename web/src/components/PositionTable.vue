<script setup lang="ts">
import type { Position } from '~/api/types'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
  TableSortableHead,
} from '~/components/ui/table'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { prob, signedUsd, usd } from '~/lib/format'
import type { TableSort } from '~/lib/table'

/** The wallet's positions. Redeemable ones stay on top whatever the sort, they want action. */
const props = defineProps<{ positions: Position[] | undefined }>()

const sort = ref<TableSort>({ column: 'value', direction: 'desc' })

const cost = (p: Position) => p.size * p.avg_price
/** PnL against what the position cost. */
const pnlShare = (p: Position) => (cost(p) > 0 ? p.pnl_usd / cost(p) : 0)
const signedPercent = (v: number) => `${v > 0 ? '+' : ''}${(v * 100).toFixed(1)}%`
const tone = (v: number) => (v < 0 ? 'text-destructive' : v > 0 ? 'text-success' : 'text-muted')

const columns: Record<string, (p: Position) => number> = {
  size: (p) => p.size,
  value: (p) => p.value_usd,
  pnl: (p) => p.pnl_usd,
}

const rows = computed(() => {
  const key = columns[sort.value.column] ?? columns.value!
  const dir = sort.value.direction === 'asc' ? 1 : -1
  return (props.positions ?? []).toSorted(
    (a, b) => Number(b.redeemable) - Number(a.redeemable) || (key(a) - key(b)) * dir
  )
})
</script>

<template>
  <EmptyState
    v-if="positions && !positions.length"
    icon="lucide:layers"
    title="No open positions"
    description="The bot buys when Jev finds an edge. Markets shows its latest calls."
  >
    <Button
      as-child
      size="sm"
      variant="outline"
    >
      <RouterLink :to="{ name: 'markets' }">Open markets</RouterLink>
    </Button>
  </EmptyState>
  <Table
    v-else
    label="Open positions"
  >
    <TableHeader>
      <TableRow>
        <TableHead class="w-full min-w-64">Market</TableHead>
        <TableHead>Outcome</TableHead>
        <TableSortableHead
          v-model="sort"
          column="size"
          align="end"
        >
          Shares
        </TableSortableHead>
        <TableHead class="text-right whitespace-nowrap">Avg / now</TableHead>
        <TableSortableHead
          v-model="sort"
          column="value"
          align="end"
        >
          Value
        </TableSortableHead>
        <TableSortableHead
          v-model="sort"
          column="pnl"
          align="end"
        >
          PnL
        </TableSortableHead>
      </TableRow>
    </TableHeader>
    <TableBody>
      <TableLoadingRow
        v-if="!positions"
        :colspan="6"
        :rows="3"
      />
      <TableRow
        v-for="p in rows"
        :key="`${p.slug}-${p.outcome}`"
        :class="p.redeemable && 'bg-success-background/40'"
      >
        <TableCell>
          <div class="grid justify-items-start gap-1">
            <RouterLink
              :to="{ name: 'markets', params: { slug: p.slug } }"
              class="font-medium text-primary hover:underline"
            >
              {{ p.title || p.slug }}
            </RouterLink>
            <Badge
              v-if="p.redeemable"
              variant="success"
              size="sm"
            >
              Resolved, ready to redeem
            </Badge>
          </div>
        </TableCell>
        <TableCell label="Outcome">
          <Badge
            variant="surface"
            size="sm"
          >
            {{ p.outcome }}
          </Badge>
        </TableCell>
        <TableCell
          label="Shares"
          labelled
          class="text-right tabular-nums"
        >
          {{ p.size.toFixed(2) }}
        </TableCell>
        <TableCell
          label="Avg / now"
          labelled
          class="text-right whitespace-nowrap tabular-nums"
        >
          <span class="text-muted">{{ prob(p.avg_price, 3) }}</span>
          <span
            class="px-1 text-muted"
            aria-hidden="true"
            >→</span
          >
          <span class="text-primary">{{ prob(p.cur_price, 3) }}</span>
        </TableCell>
        <TableCell
          label="Value"
          labelled
          class="text-right font-medium text-primary tabular-nums"
        >
          {{ usd(p.value_usd) }}
        </TableCell>
        <TableCell
          label="PnL"
          labelled
          class="text-right whitespace-nowrap tabular-nums"
          :class="tone(p.pnl_usd)"
        >
          {{ signedUsd(p.pnl_usd) }}
          <span class="text-xs">({{ signedPercent(pnlShare(p)) }})</span>
        </TableCell>
      </TableRow>
    </TableBody>
  </Table>
</template>
