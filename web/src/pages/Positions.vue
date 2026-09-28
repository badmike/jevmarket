<script setup lang="ts">
import { useOrders, usePositions, useRecommendations, useWallets } from '~/api/queries'
import Icon from '~/components/Icon.vue'
import PositionOrders from '~/components/PositionOrders.vue'
import PositionTable from '~/components/PositionTable.vue'
import QueryError from '~/components/QueryError.vue'
import WalletsPanel from '~/components/WalletsPanel.vue'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { signedUsd, usd } from '~/lib/format'

const positions = usePositions()
const wallets = useWallets()
const orders = useOrders()
const recommendations = useRecommendations()

const data = computed(() => positions.data.value)
const reloading = computed(
  () => positions.isFetching.value || wallets.isFetching.value || orders.isFetching.value
)
const reload = () => Promise.all([positions.refetch(), wallets.refetch(), orders.refetch()])

/** Market titles by slug, so the order log reads as questions instead of slugs. */
const titles = computed(
  () =>
    new Map([
      ...(recommendations.data.value ?? []).map((r) => [r.slug, r.question] as const),
      ...(data.value?.positions ?? []).flatMap((p) => (p.title ? [[p.slug, p.title] as const] : [])),
    ])
)
const pnl = computed(() => data.value?.positions.reduce((sum, p) => sum + p.pnl_usd, 0) ?? 0)
</script>

<template>
  <div class="mx-auto grid max-w-7xl gap-6">
    <header class="flex flex-wrap items-end justify-between gap-3">
      <div class="grid gap-1">
        <h1 class="text-2xl font-semibold text-primary">Positions</h1>
        <p class="text-sm text-muted">
          <template v-if="!wallets.data.value && data?.wallet">
            Watching <span class="font-mono break-all">{{ data.wallet }}</span> ({{ data.wallet_type }})
          </template>
          <template v-else>What you hold, what is at risk, and every order placed.</template>
        </p>
      </div>
      <Button
        size="sm"
        variant="outline"
        :loading="reloading"
        @click="reload()"
      >
        <Icon name="lucide:refresh-cw" />
        Reload
      </Button>
    </header>

    <QueryError
      v-if="positions.error.value"
      title="Could not read the wallet"
      :error="positions.error.value"
      @retry="positions.refetch()"
    />

    <WalletsPanel
      :positions="data"
      :loading="positions.isPending.value"
    />

    <EmptyState
      v-if="data && !data.wallet"
      icon="lucide:wallet-minimal"
      title="No wallet configured"
      description="Set polymarket_private_key, or polymarket_deposit_wallet to watch an address in dry runs, under Settings."
    >
      <Button
        as-child
        size="sm"
      >
        <RouterLink :to="{ name: 'settings' }">Open settings</RouterLink>
      </Button>
    </EmptyState>
    <section
      v-else-if="!positions.error.value"
      class="grid gap-3"
    >
      <div class="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
        <h2 class="text-base font-semibold text-primary">Open positions</h2>
        <p
          v-if="data?.positions.length"
          class="text-sm text-muted tabular-nums"
        >
          {{ data.positions.length }} worth {{ usd(data.exposure.positions_usd) }},
          <span :class="pnl < 0 ? 'text-destructive' : pnl > 0 ? 'text-success' : ''">{{ signedUsd(pnl) }}</span>
          unrealized
        </p>
      </div>
      <PositionTable :positions="data?.positions" />
    </section>

    <PositionOrders
      :open-orders="data?.wallet ? data.open_orders : undefined"
      :titles="titles"
    />
  </div>
</template>
