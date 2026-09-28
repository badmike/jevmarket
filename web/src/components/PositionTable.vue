<script setup lang="ts">
import type { Position } from '~/api/types'
import MarketAvatar from '~/components/MarketAvatar.vue'
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
import { SimpleTooltip } from '~/components/ui/tooltip'
import { day, inDays, prob, signedUsd, usd } from '~/lib/format'
import type { TableSort } from '~/lib/table'

/** The wallet's positions. Redeemable ones stay on top whatever the sort, they want action. */
const props = defineProps<{ positions: Position[] | undefined }>()

const sort = ref<TableSort>({ column: 'value', direction: 'desc' })

const cost = (p: Position) => p.size * p.avg_price
/** PnL against what the position cost. */
const pnlShare = (p: Position) => (cost(p) > 0 ? p.pnl_usd / cost(p) : 0)
const signedPercent = (v: number) => `${v > 0 ? '+' : ''}${(v * 100).toFixed(1)}%`
const tone = (v: number) => (v < 0 ? 'text-destructive' : v > 0 ? 'text-success' : 'text-muted')

/** Sort keys; `null` (no end date) sorts last in both directions. */
const columns: Record<string, (p: Position) => number | null> = {
  resolves: (p) => (p.end_date ? Date.parse(p.end_date) : null),
  size: (p) => p.size,
  value: (p) => p.value_usd,
  pnl: (p) => p.pnl_usd,
}

const rows = computed(() => {
  const key = columns[sort.value.column] ?? columns.value!
  const dir = sort.value.direction === 'asc' ? 1 : -1
  const byKey = (a: Position, b: Position) => {
    const [x, y] = [key(a), key(b)]
    if (x === null || y === null) return Number(x === null) - Number(y === null)
    return (x - y) * dir
  }
  return (props.positions ?? []).toSorted((a, b) => Number(b.redeemable) - Number(a.redeemable) || byKey(a, b))
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
          column="resolves"
        >
          <SimpleTooltip
            tooltip="When the market is scheduled to settle. Money in the position is tied up until then."
            as-child
          >
            <span class="underline decoration-muted/50 decoration-dotted underline-offset-4">Resolves</span>
          </SimpleTooltip>
        </TableSortableHead>
        <TableSortableHead
          v-model="sort"
          column="size"
          align="end"
        >
          <SimpleTooltip
            tooltip="Each share pays $1 if its outcome wins and nothing if it loses."
            as-child
          >
            <span class="underline decoration-muted/50 decoration-dotted underline-offset-4">Shares</span>
          </SimpleTooltip>
        </TableSortableHead>
        <TableHead class="text-right whitespace-nowrap">
          <SimpleTooltip
            tooltip="The average price paid per share, then what a share sells for now."
            as-child
          >
            <span class="underline decoration-muted/50 decoration-dotted underline-offset-4">Avg / now</span>
          </SimpleTooltip>
        </TableHead>
        <TableSortableHead
          v-model="sort"
          column="value"
          align="end"
        >
          <SimpleTooltip
            tooltip="What the shares are worth at the current price."
            as-child
          >
            <span class="underline decoration-muted/50 decoration-dotted underline-offset-4">Value</span>
          </SimpleTooltip>
        </TableSortableHead>
        <TableSortableHead
          v-model="sort"
          column="pnl"
          align="end"
        >
          <SimpleTooltip
            tooltip="Profit and loss: the value now minus what the shares cost."
            as-child
          >
            <span class="underline decoration-muted/50 decoration-dotted underline-offset-4">PnL</span>
          </SimpleTooltip>
        </TableSortableHead>
      </TableRow>
    </TableHeader>
    <TableBody>
      <TableLoadingRow
        v-if="!positions"
        :colspan="7"
        :rows="3"
      />
      <TableRow
        v-for="p in rows"
        :key="`${p.slug}-${p.outcome}`"
        :class="p.redeemable && 'bg-success-background/40'"
      >
        <TableCell>
          <div class="flex min-w-0 items-center gap-3">
            <MarketAvatar
              :src="p.image"
              :name="p.title || p.slug"
            />
            <div class="grid min-w-0 justify-items-start gap-1">
              <RouterLink
                :to="{ name: 'markets', params: { slug: p.slug } }"
                class="font-medium text-primary hover:underline"
              >
                {{ p.title || p.slug }}
              </RouterLink>
              <SimpleTooltip
                v-if="p.redeemable"
                tooltip="The market has ended. Winning shares can now be cashed in at $1 each."
                as-child
              >
                <Badge
                  variant="success"
                  size="sm"
                >
                  Resolved, ready to redeem
                </Badge>
              </SimpleTooltip>
            </div>
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
          label="Resolves"
          labelled
          class="whitespace-nowrap"
        >
          <template v-if="p.end_date">
            <span class="text-primary tabular-nums">{{ day(p.end_date) }}</span>
            <span class="block text-xs text-muted">{{ inDays(p.end_date) }}</span>
          </template>
          <span
            v-else
            class="text-muted"
            >–</span
          >
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
