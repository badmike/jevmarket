<script setup lang="ts">
import { useOrders, usePositions } from '~/api/queries'
import ExposureGauge from '~/components/ExposureGauge.vue'
import Icon from '~/components/Icon.vue'
import OrderStatus from '~/components/OrderStatus.vue'
import QueryError from '~/components/QueryError.vue'
import StatTile from '~/components/StatTile.vue'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Skeleton } from '~/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '~/components/ui/table'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { at, prob, signedUsd, usd } from '~/lib/format'

const positions = usePositions()
const orders = useOrders()
const data = computed(() => positions.data.value)
</script>

<template>
  <div class="mx-auto grid max-w-7xl gap-6">
    <header class="flex flex-wrap items-end justify-between gap-3">
      <div class="grid gap-1">
        <h1 class="text-2xl font-semibold text-primary">Positions</h1>
        <p class="text-sm text-muted">
          <template v-if="data?.wallet">
            Wallet <span class="break-all">{{ data.wallet }}</span> ({{ data.wallet_type }})
          </template>
          <template v-else>What the wallet holds on Polymarket, and every order the bot logged.</template>
        </p>
      </div>
      <Button
        size="sm"
        variant="outline"
        :loading="positions.isFetching.value"
        @click="positions.refetch()"
      >
        <Icon name="lucide:refresh-cw" />
        Reload wallet
      </Button>
    </header>

    <QueryError
      v-if="positions.error.value"
      title="Could not read the wallet"
      :error="positions.error.value"
      @retry="positions.refetch()"
    />
    <template v-else>
      <div class="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <template v-if="data">
          <StatTile
            title="pUSD balance"
            icon="lucide:wallet"
            :detail="data.balance_usd == null ? 'Needs polymarket_private_key' : 'As the exchange sees it'"
          >
            {{ data.balance_usd == null ? 'n/a' : usd(data.balance_usd) }}
          </StatTile>
          <StatTile
            title="Exposure against the cap"
            icon="lucide:gauge"
          >
            <ExposureGauge :exposure="data.exposure" />
          </StatTile>
          <StatTile
            title="Open positions"
            icon="lucide:layers"
            :detail="`Worth ${usd(data.exposure.positions_usd)}`"
          >
            {{ data.positions.length }}
          </StatTile>
          <StatTile
            title="Open orders"
            icon="lucide:list-ordered"
            :detail="`${usd(data.exposure.open_orders_usd)} not yet filled`"
          >
            {{ data.open_orders.length }}
          </StatTile>
        </template>
        <template v-else>
          <Skeleton
            v-for="i in 4"
            :key="i"
            class="h-26 rounded-xl"
          />
        </template>
      </div>

      <EmptyState
        v-if="data && !data.wallet"
        icon="lucide:wallet-minimal"
        title="No wallet configured"
        description="Set polymarket_private_key, or polymarket_wallet to watch an address in dry runs, under Settings."
      >
        <Button
          as-child
          size="sm"
        >
          <RouterLink :to="{ name: 'settings' }">Open settings</RouterLink>
        </Button>
      </EmptyState>

      <section
        v-else
        class="grid gap-3"
      >
        <h2 class="text-base font-semibold text-primary">Open positions</h2>
        <Table label="Open positions">
          <TableHeader>
            <TableRow>
              <TableHead class="w-full">Market</TableHead>
              <TableHead>Outcome</TableHead>
              <TableHead class="text-right">Size</TableHead>
              <TableHead class="text-right">Avg</TableHead>
              <TableHead class="text-right">Now</TableHead>
              <TableHead class="text-right">Value</TableHead>
              <TableHead class="text-right">PnL</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableLoadingRow
              v-if="!data"
              :colspan="7"
              :rows="3"
            />
            <TableRow
              v-for="p in data?.positions ?? []"
              :key="`${p.slug}-${p.outcome}`"
            >
              <TableCell class="font-medium text-primary">
                {{ p.title || p.slug }}
                <span
                  v-if="p.redeemable"
                  class="ml-1 text-xs text-success"
                >
                  resolved, redeemable
                </span>
              </TableCell>
              <TableCell label="Outcome">{{ p.outcome }}</TableCell>
              <TableCell
                label="Size"
                class="text-right tabular-nums"
              >
                {{ p.size.toFixed(2) }}
              </TableCell>
              <TableCell
                label="Avg"
                class="text-right tabular-nums"
              >
                {{ prob(p.avg_price, 3) }}
              </TableCell>
              <TableCell
                label="Now"
                class="text-right tabular-nums"
              >
                {{ prob(p.cur_price, 3) }}
              </TableCell>
              <TableCell
                label="Value"
                class="text-right tabular-nums"
              >
                {{ usd(p.value_usd) }}
              </TableCell>
              <TableCell
                label="PnL"
                class="text-right tabular-nums"
                :class="p.pnl_usd < 0 ? 'text-destructive' : p.pnl_usd > 0 ? 'text-success' : ''"
              >
                {{ signedUsd(p.pnl_usd) }}
              </TableCell>
            </TableRow>
            <TableRow v-if="data && !data.positions.length">
              <TableCell
                :colspan="7"
                class="py-8 text-center text-sm text-muted"
              >
                No open positions.
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>

        <h2 class="pt-3 text-base font-semibold text-primary">Open orders</h2>
        <Table label="Open orders">
          <TableHeader>
            <TableRow>
              <TableHead class="w-full">Order</TableHead>
              <TableHead>Side</TableHead>
              <TableHead>Outcome</TableHead>
              <TableHead class="text-right">Price</TableHead>
              <TableHead class="text-right">Filled</TableHead>
              <TableHead>Status</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow
              v-for="o in data?.open_orders ?? []"
              :key="o.id"
            >
              <TableCell>
                <span class="block truncate text-primary">{{ o.id }}</span>
                <span class="text-xs text-muted">{{ at(o.created_at) }}</span>
              </TableCell>
              <TableCell label="Side">{{ o.side }}</TableCell>
              <TableCell label="Outcome">{{ o.outcome }}</TableCell>
              <TableCell
                label="Price"
                class="text-right tabular-nums"
              >
                {{ prob(o.price, 3) }}
              </TableCell>
              <TableCell
                label="Filled"
                class="text-right whitespace-nowrap tabular-nums"
              >
                {{ o.matched }} of {{ o.size }}
              </TableCell>
              <TableCell label="Status">{{ o.status }}</TableCell>
            </TableRow>
            <TableRow v-if="data && !data.open_orders.length">
              <TableCell
                :colspan="6"
                class="py-8 text-center text-sm text-muted"
              >
                No open orders.
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </section>
    </template>

    <section class="grid gap-3">
      <div class="grid gap-1">
        <h2 class="text-base font-semibold text-primary">Order log</h2>
        <p class="text-sm text-muted">Every order the bot placed or, in dry runs, would have placed.</p>
      </div>
      <QueryError
        v-if="orders.error.value"
        :error="orders.error.value"
        @retry="orders.refetch()"
      />
      <Table
        v-else
        label="Order log"
      >
        <TableHeader>
          <TableRow>
            <TableHead class="w-full">Market</TableHead>
            <TableHead>Order</TableHead>
            <TableHead class="text-right">Amount</TableHead>
            <TableHead>Status</TableHead>
            <TableHead class="text-right">When</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableLoadingRow
            v-if="orders.isPending.value"
            :colspan="5"
            :rows="4"
          />
          <TableRow
            v-for="(o, i) in orders.data.value ?? []"
            :key="`${o.ts}-${i}`"
          >
            <TableCell>
              <RouterLink
                :to="{ name: 'markets', params: { slug: o.slug } }"
                class="text-primary hover:underline"
              >
                {{ o.slug }}
              </RouterLink>
            </TableCell>
            <TableCell
              label="Order"
              class="whitespace-nowrap tabular-nums"
            >
              BUY {{ o.outcome }} {{ o.size }} @ {{ prob(o.price, 3) }}
            </TableCell>
            <TableCell
              label="Amount"
              class="text-right tabular-nums"
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
            >
              {{ at(o.ts) }}
            </TableCell>
          </TableRow>
          <TableRow v-if="orders.data.value && !orders.data.value.length">
            <TableCell
              :colspan="5"
              class="py-8 text-center text-sm text-muted"
            >
              No orders logged yet.
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </section>
  </div>
</template>
